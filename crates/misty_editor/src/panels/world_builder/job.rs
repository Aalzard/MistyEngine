//! Generation jobs: requests gathered from the scene, run on a worker thread, and their results
//! put back (terrain asset, water, scatter) as one undo step. Also the debounced live preview.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use misty_engine::hecs::Entity;
use misty_engine::terrain::store;
use misty_engine::world_gen::{
    self, Biome, Output, Progress, Request, Stages, WorldBuilder, WorldStamp,
};

use crate::state::EditorState;
use crate::terrain_tools::TerrainSnapshot;

/// Quiet time after the last edit before the preview regenerates.
const DEBOUNCE: Duration = Duration::from_millis(350);

pub struct Job {
    handle: JoinHandle<Option<Output>>,
    pub progress: Arc<Progress>,
    req: Request,
    builder: Entity,
    pub label: &'static str,
    started: Instant,
}

impl Job {
    pub fn preview(&self) -> bool {
        self.req.preview
    }
}

pub fn stage_label(stages: Stages) -> &'static str {
    match stages {
        Stages::All => "Generate World",
        Stages::Heights => "Generate Heights",
        Stages::Textures => "Generate Textures",
        Stages::Scatter => "Generate Scatter",
    }
}

/// Start generating for `builder` (cancels a running preview; a full run waits for nothing).
pub fn start(state: &mut EditorState, builder: Entity, stages: Stages, preview: bool) {
    if let Some(job) = &state.world_builder.job {
        if job.preview() {
            job.progress.cancel();
        } else {
            return;
        }
    }
    let Ok(settings) = state
        .scene
        .world
        .get::<&WorldBuilder>(builder)
        .map(|s| (*s).clone())
    else {
        return;
    };
    let current = store::get(&settings.terrain_data).map(|(d, _)| (*d).clone());
    let root = state.project.root.clone();
    let load = |p: &str| Biome::load(&root.join(p));
    let (req, warnings) =
        match world_gen::request(&state.scene, builder, stages, preview, current, &load) {
            Ok(r) => r,
            Err(e) => {
                state.world_builder.status = e;
                return;
            }
        };
    if !preview {
        for w in warnings {
            state.console.warn(w);
        }
    }
    // Remember the data from before the first preview: undo and Revert go back to it.
    if state.world_builder.preview_base.is_none()
        && let Some((d, _)) = store::get(&settings.terrain_data)
    {
        state.world_builder.preview_base = Some(TerrainSnapshot {
            path: settings.terrain_data.clone(),
            data: (*d).clone(),
        });
    }
    let progress = Arc::new(Progress::default());
    let shared = progress.clone();
    let run = req.clone();
    let handle = std::thread::spawn(move || world_gen::generate::run(&run, &shared));
    state.world_builder.job = Some(Job {
        handle,
        progress,
        req,
        builder,
        label: if preview {
            "Preview"
        } else {
            stage_label(stages)
        },
        started: Instant::now(),
    });
}

/// Pick up a finished job.
pub fn poll(state: &mut EditorState) {
    let finished = state
        .world_builder
        .job
        .as_ref()
        .is_some_and(|j| j.handle.is_finished());
    if !finished {
        return;
    }
    let job = state.world_builder.job.take().expect("checked");
    let secs = job.started.elapsed().as_secs_f32();
    let Ok(Some(out)) = job.handle.join() else {
        if !job.req.preview {
            state.world_builder.status = "Cancelled.".into();
        }
        return;
    };
    let path = job.req.settings.terrain_data.clone();
    state.world_builder.timings = out.timings.clone();
    if job.req.preview {
        store::insert(&path, out.data);
        state.world_builder.status = format!("Preview updated in {secs:.2} s.");
        return;
    }
    if !state.scene.contains(job.builder) {
        return;
    }
    let res = out.data.heightmap_resolution;
    let before = state
        .world_builder
        .preview_base
        .take()
        .unwrap_or_else(|| TerrainSnapshot {
            path: path.clone(),
            data: store::get(&path)
                .map(|(d, _)| (*d).clone())
                .unwrap_or_else(|| out.data.clone()),
        });
    state
        .undo
        .push_terrain(job.label, &state.scene, &state.selection, before);
    state.scene_dirty = true;
    store::insert(&path, out.data.clone());
    if let Err(e) = store::save(&path) {
        state.console.warn(format!("Could not save {path}: {e}"));
    }
    let root = state.project.root.clone();
    if let Err(e) = world_gen::write_materials(&root, &job.req.biomes) {
        state.console.warn(format!("World Builder materials: {e}"));
    }
    // Prefabs the scatter places, read up front.
    let mut prefabs = HashMap::new();
    for b in &job.req.biomes {
        for p in b.scatter.iter().filter_map(|i| i.prefab.clone()) {
            match state.assets.prefab(&p) {
                Ok(pf) => {
                    prefabs.insert(p, (*pf).clone());
                }
                Err(e) => state.console.warn(format!("Scatter prefab {p}: {e}")),
            }
        }
    }
    let count = world_gen::apply(&mut state.scene, job.builder, &job.req, &out, &|p| {
        prefabs.get(p).cloned()
    });
    state.refresh_assets();
    let scatter = if out.scatter.is_some() {
        format!(", {count} scatter instances")
    } else {
        String::new()
    };
    state.world_builder.status = format!("{} done: {res}² in {secs:.1} s{scatter}.", job.label);
    state.console.info(state.world_builder.status.clone());
    // What was generated is the new baseline for the preview.
    state.world_builder.signature = signature(state, job.builder);
}

/// Everything that changes a generation: settings, stamps and where they are.
pub fn signature(state: &EditorState, builder: Entity) -> Option<String> {
    let scene = &state.scene;
    let s = scene.world.get::<&WorldBuilder>(builder).ok()?;
    let mut out = serde_json::to_string(&*s).ok()?;
    for e in world_gen::stamps_of(scene, builder) {
        if let Ok(st) = scene.world.get::<&WorldStamp>(e) {
            out += &serde_json::to_string(&*st).unwrap_or_default();
        }
        out += &format!(
            "{:?}{}",
            scene.world_matrix(e).to_cols_array(),
            scene.active_in_hierarchy(e)
        );
    }
    Some(out)
}

/// Live preview: regenerate at low resolution shortly after the last change.
pub fn preview_tick(state: &mut EditorState, builder: Entity) {
    let enabled = state
        .scene
        .world
        .get::<&WorldBuilder>(builder)
        .is_ok_and(|s| s.live_preview);
    let sig = signature(state, builder);
    let w = &mut state.world_builder;
    if w.signature.is_none() || w.builder != Some(builder) {
        // First look at this builder: nothing to preview yet.
        w.signature = sig;
        w.builder = Some(builder);
        return;
    }
    if sig != w.signature {
        w.signature = sig;
        w.changed_at = Some(Instant::now());
    }
    let busy = w.job.as_ref().is_some_and(|j| !j.preview());
    let due = w.changed_at.is_some_and(|t| t.elapsed() >= DEBOUNCE);
    if enabled && due && !busy && !state.play.in_play_mode() {
        state.world_builder.changed_at = None;
        start(state, builder, Stages::All, true);
    }
}

/// Put the terrain back as it was before the preview.
pub fn revert_preview(state: &mut EditorState) {
    if let Some(job) = &state.world_builder.job
        && job.preview()
    {
        job.progress.cancel();
    }
    if let Some(base) = state.world_builder.preview_base.take() {
        store::insert(&base.path, base.data);
        state.world_builder.status = "Preview reverted.".into();
    }
}

/// Wait for the running job (tests).
#[cfg(test)]
pub fn finish(state: &mut EditorState) {
    while state.world_builder.job.is_some() {
        if state
            .world_builder
            .job
            .as_ref()
            .is_some_and(|j| j.handle.is_finished())
        {
            poll(state);
        } else {
            std::thread::yield_now();
        }
    }
}
