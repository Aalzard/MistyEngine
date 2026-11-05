//! Lighting window ▸ Generate Lighting: bakes the open scene's lightmaps on a background thread
//! (the editor keeps running; progress bar and Cancel in the window), then saves
//! `<Scene folder>/<Scene>/Lightmap-N.lightmap`, points every lightmapped renderer at its
//! lightmap (index + scale-offset, undoable) and lists the lightmaps in the scene's settings.
//! Clear Baked Data removes them again.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Instant;

use misty_engine::hecs::Entity;
use misty_engine::lightmap::bake::{self, BakeOutput, Progress, ReceiverResult};
use misty_engine::lightmap::{self, store};
use misty_engine::scene::lightmapping::IDENTITY_ST;
use misty_engine::scene::{MeshRenderer, Scene};

use crate::state::EditorState;

struct Job {
    scene_path: String,
    progress: Arc<Progress>,
    handle: Option<JoinHandle<Option<BakeOutput>>>,
    started: Instant,
}

#[derive(Default)]
pub struct LightmapTools {
    job: Option<Job>,
    pub status: String,
}

impl LightmapTools {
    pub fn baking(&self) -> bool {
        self.job.is_some()
    }

    /// Fraction done and a label while baking.
    pub fn progress(&self) -> Option<(f32, String)> {
        let j = self.job.as_ref()?;
        let p = &j.progress;
        let label = match p.stage.load(Ordering::Relaxed) {
            0 => "Preparing…".to_string(),
            1 => format!(
                "Baking lightmaps: {} of {} texels",
                p.done.load(Ordering::Relaxed),
                p.total.load(Ordering::Relaxed)
            ),
            _ => "Denoising…".to_string(),
        };
        Some((p.fraction(), label))
    }
}

/// Generate Lighting: gather the scene and start the bake thread.
pub fn start(state: &mut EditorState) {
    if state.lightmapper.job.is_some() {
        return;
    }
    let Some(scene_path) = state.scene.active_path() else {
        state.lightmapper.status = "Save the scene before generating lighting.".into();
        return;
    };
    if !state.scene.render_settings.lightmapping.baked_gi {
        state.lightmapper.status = "Baked Global Illumination is off.".into();
        return;
    }
    let input = bake::input::gather(&state.scene, &state.assets);
    if input.receivers.is_empty() {
        state.lightmapper.status =
            "Nothing to bake: mark objects Static with Contribute Global Illumination on.".into();
        return;
    }
    let progress = Arc::new(Progress::default());
    let handle = bake::spawn(input, progress.clone());
    state.lightmapper.job = Some(Job {
        scene_path,
        progress,
        handle: Some(handle),
        started: Instant::now(),
    });
    state.lightmapper.status.clear();
}

/// Stop the running bake (its results are dropped).
pub fn cancel(state: &mut EditorState) {
    if let Some(j) = &state.lightmapper.job {
        j.progress.cancel.store(true, Ordering::Relaxed);
    }
}

/// Per frame: finish the bake when its thread is done. True while baking (keep repainting).
pub fn poll(state: &mut EditorState) -> bool {
    let Some(job) = state.lightmapper.job.as_mut() else {
        return false;
    };
    if !job.handle.as_ref().is_some_and(|h| h.is_finished()) {
        return true;
    }
    let mut job = state.lightmapper.job.take().expect("checked");
    let result = job.handle.take().and_then(|h| h.join().ok()).flatten();
    match result {
        Some(out) => apply(state, &job, out),
        None => state.lightmapper.status = "Lightmap bake cancelled.".into(),
    }
    false
}

/// Save the lightmaps and point the renderers at them.
fn apply(state: &mut EditorState, job: &Job, out: BakeOutput) {
    if state.scene.active_path().as_deref() != Some(job.scene_path.as_str()) {
        state.lightmapper.status = "Bake dropped: another scene was opened.".into();
        return;
    }
    state.record("Generate Lighting");
    let mut paths = Vec::new();
    for (i, map) in out.maps.into_iter().enumerate() {
        let rel = lightmap::asset_path_for(&job.scene_path, i);
        let path = state.project.root.join(&rel);
        if let Err(e) = map.save(&path) {
            state.lightmapper.status = format!("Could not save {rel}: {e}");
            return;
        }
        store::insert(&path, Arc::new(map));
        paths.push(rel);
    }
    remove_stale_files(state, &job.scene_path, paths.len());
    let count = paths.len();
    assign(&mut state.scene, &out.receivers, paths);
    state.lightmapper.status = format!(
        "Baked {count} lightmap{} ({}×{}, {} texels) in {:.1} s.",
        if count == 1 { "" } else { "s" },
        out.size,
        out.size,
        out.texels,
        job.started.elapsed().as_secs_f32()
    );
    state.console.info(state.lightmapper.status.clone());
    state.refresh_assets();
}

/// Point the baked renderers at their lightmaps (the rest at none) and list the lightmaps.
pub fn assign(scene: &mut Scene, receivers: &[ReceiverResult], lightmaps: Vec<String>) {
    reset_renderers(scene);
    for r in receivers {
        let Some(e) = Entity::from_bits(r.id) else {
            continue;
        };
        if let Ok(mut m) = scene.world.get::<&mut MeshRenderer>(e) {
            m.lightmap_index = r.lightmap as i32;
            m.lightmap_scale_offset = r.scale_offset;
        }
    }
    scene.render_settings.lightmapping.lightmaps = lightmaps;
}

/// Every renderer back to "not lightmapped".
fn reset_renderers(scene: &mut Scene) {
    for r in scene.world.query_mut::<&mut MeshRenderer>() {
        r.lightmap_index = -1;
        r.lightmap_scale_offset = IDENTITY_ST;
    }
}

/// Delete `Lightmap-N` files from an earlier bake with more lightmaps.
fn remove_stale_files(state: &EditorState, scene_path: &str, keep: usize) {
    for i in keep.. {
        let path = state
            .project
            .root
            .join(lightmap::asset_path_for(scene_path, i));
        if !path.exists() {
            break;
        }
        let _ = std::fs::remove_file(&path);
        store::forget(&path);
    }
}

/// Clear Baked Data: delete the lightmaps and un-map the renderers.
pub fn clear(state: &mut EditorState) {
    let had = !state
        .scene
        .render_settings
        .lightmapping
        .lightmaps
        .is_empty();
    let mapped = state
        .scene
        .world
        .query_mut::<&MeshRenderer>()
        .into_iter()
        .any(|r| r.lightmap_index >= 0);
    if !had && !mapped {
        return;
    }
    state.record("Clear Baked Lighting");
    for rel in std::mem::take(&mut state.scene.render_settings.lightmapping.lightmaps) {
        let path = state.project.root.join(&rel);
        let _ = std::fs::remove_file(&path);
        store::forget(&path);
    }
    reset_renderers(&mut state.scene);
    state.lightmapper.status = "Cleared baked lighting.".into();
    state.refresh_assets();
}
