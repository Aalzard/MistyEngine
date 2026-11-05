//! Generation jobs: requests gathered from the scene, run on a worker thread, and their results
//! put back (terrain asset, water, scatter) as one undo step. Full runs share a stage cache, so
//! Generate after a small edit only re-runs the stages that edit affects. (The Preview card has
//! its own passes, see `preview`.)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use misty_engine::hecs::Entity;
use misty_engine::terrain::store;
use misty_engine::world_gen::generate::Cache;
use misty_engine::world_gen::{
    self, Biome, Output, Progress, Request, Stages, WorldBuilder, WorldStamp,
};

use crate::state::EditorState;
use crate::terrain_tools::TerrainSnapshot;

pub struct Job {
    handle: JoinHandle<Option<Output>>,
    pub progress: Arc<Progress>,
    req: Request,
    builder: Entity,
    /// The terrain asset, resolved against this editor's project when the job started.
    path: String,
    pub label: &'static str,
    pub started: Instant,
}

impl Job {
    /// "Hydraulic erosion · 3 s left" (after the first moments, once the rate is known).
    pub fn eta_text(&self) -> String {
        let (stage, _) = self.progress.get();
        let f = self.progress.overall();
        let elapsed = self.started.elapsed().as_secs_f32();
        if f > 0.03 && elapsed > 0.4 && f < 1.0 {
            let left = elapsed * (1.0 - f) / f;
            format!("{stage} · {} left", seconds(left))
        } else {
            stage.to_owned()
        }
    }
}

/// "0.4 s", "12 s", "2 min".
fn seconds(s: f32) -> String {
    if s < 1.0 {
        format!("{s:.1} s")
    } else if s < 90.0 {
        format!("{s:.0} s")
    } else {
        format!("{:.0} min", s / 60.0)
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

/// Start generating for `builder` (does nothing while another generation runs).
pub fn start(state: &mut EditorState, builder: Entity, stages: Stages) {
    if state.world_builder.job.is_some() {
        return;
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
        match world_gen::request(&state.scene, builder, stages, false, current, &load) {
            Ok(r) => r,
            Err(e) => {
                state.world_builder.status = e;
                return;
            }
        };
    for w in warnings {
        state.console.warn(w);
    }
    // Remember the data from before any preview: undo and Revert go back to it.
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
    let cache: Arc<Mutex<Cache>> = state.world_builder.full_cache.clone();
    let handle = std::thread::spawn(move || {
        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        world_gen::generate::run_cached(&run, &shared, &mut cache)
    });
    let path = if std::path::Path::new(&settings.terrain_data).is_absolute() {
        settings.terrain_data.clone()
    } else {
        state
            .project
            .root
            .join(&settings.terrain_data)
            .to_string_lossy()
            .replace('\\', "/")
    };
    state.world_builder.job = Some(Job {
        handle,
        progress,
        req,
        builder,
        path,
        label: stage_label(stages),
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
        state.world_builder.status = "Cancelled.".into();
        return;
    };
    let path = job.path.clone();
    if !state.scene.contains(job.builder) {
        return;
    }
    let apply_start = Instant::now();
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
    // The undo step diffs `before` against the stored result, so store it first.
    store::insert(&path, out.data.clone());
    state
        .undo
        .push_terrain(job.label, &state.scene, &state.selection, before);
    state.scene_dirty = true;
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
    let mut timings = out.timings.clone();
    timings.push(("Apply", apply_start.elapsed().as_secs_f32() * 1000.0));
    state.world_builder.timings = timings;
    state.world_builder.reused = out.reused.clone();
    let scatter = if out.scatter.is_some() {
        format!(", {count} scatter instances")
    } else {
        String::new()
    };
    let total = secs + apply_start.elapsed().as_secs_f32();
    state.world_builder.status = format!("{} done: {res}² in {total:.1} s{scatter}.", job.label);
    state.console.info(state.world_builder.status.clone());
}

/// Everything that changes a generation: settings, stamps and where they are.
pub fn signature(state: &EditorState, builder: Entity) -> Option<String> {
    let scene = &state.scene;
    let s = scene.world.get::<&WorldBuilder>(builder).ok()?;
    // Live in Scene view only decides where the preview shows.
    let mut s = (*s).clone();
    s.live_preview = false;
    let mut out = serde_json::to_string(&s).ok()?;
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
    if let Some(t) = world_gen::terrain_of(scene, builder) {
        out += &format!("{:?}", scene.world_position(t));
    }
    Some(out)
}

/// Put the terrain back as it was before the preview.
pub fn revert_preview(state: &mut EditorState) {
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
