//! Reflection probe tools: GameObject ▸ Light ▸ Reflection Probe, Generate Probes (automatic
//! placement under a "Reflection Probes" parent, undoable and re-generatable) and Bake Reflection
//! Probes. Bakes run one probe per frame on the editor's renderer with a progress bar, then save
//! `<Scene folder>/<Scene>/ReflectionProbe-N.probe` and point the probes at them. Realtime probes
//! are updated here every frame too. Gizmos and box handles are in `probe_gizmos`.

use std::sync::Arc;
use std::time::Instant;

use misty_engine::hecs::Entity;
use misty_engine::render::probes::capture::{
    BAKE_SAMPLES, CaptureInputs, CaptureSettings, ProbeCapture, probe_set,
};
use misty_engine::render::probes::realtime::RealtimeProbes;
use misty_engine::render::probes::{self, ProbeData, placement, store};
use misty_engine::scene::reflection_probe::ProbeMode;
use misty_engine::scene::{ReflectionProbe, ReflectionProbeGroup, Transform};

use crate::state::EditorState;
use crate::viewport::Gpu;

/// Name of the parent of generated probes.
pub const GROUP_NAME: &str = "Reflection Probes";

/// A bake in progress: probes captured one per frame, `passes` times (second bounce).
struct BakeJob {
    scene_path: String,
    probes: Vec<(Entity, ReflectionProbe)>,
    passes: usize,
    pass: usize,
    index: usize,
    inputs: Option<CaptureInputs>,
    done: Vec<Arc<ProbeData>>,
    previous: Vec<Arc<ProbeData>>,
    capture: Option<ProbeCapture>,
    started: Instant,
}

/// Probe settings and work kept with the editor.
#[derive(Default)]
pub struct ProbeTools {
    /// Generator settings used while the scene has no Reflection Probes group.
    pub settings: ReflectionProbeGroup,
    pub status: String,
    job: Option<BakeJob>,
    /// Bake as soon as the GPU is reachable (after Generate Probes).
    bake_requested: bool,
    pub realtime: RealtimeProbes,
    /// Inspector ▸ Edit Bounding Volume: box handles in the Scene view.
    pub edit_bounds: bool,
}

impl ProbeTools {
    pub fn baking(&self) -> bool {
        self.job.is_some() || self.bake_requested
    }

    /// Fraction done and a label.
    pub fn progress(&self) -> Option<(f32, String)> {
        let j = self.job.as_ref()?;
        let total = (j.probes.len() * j.passes).max(1);
        let done = j.pass * j.probes.len() + j.index;
        let label = format!(
            "Baking probe {} of {}{}",
            j.index + 1,
            j.probes.len(),
            if j.passes > 1 {
                format!(" (bounce {})", j.pass + 1)
            } else {
                String::new()
            }
        );
        Some((done as f32 / total as f32, label))
    }
}

/// The generated-probes parent of a scene.
pub fn group_of(s: &misty_engine::scene::Scene) -> Option<Entity> {
    s.roots()
        .iter()
        .copied()
        .find(|e| s.world.get::<&ReflectionProbeGroup>(*e).is_ok())
}

/// The scene's generated-probes parent.
pub fn group(state: &EditorState) -> Option<Entity> {
    group_of(&state.scene)
}

/// Generator settings: the group's, else the window's.
pub fn group_settings(state: &EditorState) -> ReflectionProbeGroup {
    group(state)
        .and_then(|g| {
            let c = state.scene.world.get::<&ReflectionProbeGroup>(g).ok()?;
            Some((*c).clone())
        })
        .unwrap_or_else(|| state.probes.settings.clone())
}

pub fn set_group_settings(state: &mut EditorState, s: ReflectionProbeGroup) {
    match group(state) {
        Some(g) => {
            let _ = state.scene.world.insert_one(g, s);
            state.mark_edited("Reflection Probe Settings");
        }
        None => state.probes.settings = s,
    }
}

/// GameObject ▸ Light ▸ Reflection Probe: at the Scene view pivot (or under `parent`).
pub fn create_probe(state: &mut EditorState, parent: Option<Entity>) {
    let parent = parent.filter(|p| state.scene.contains(*p));
    state.record("Create Reflection Probe");
    let e = state.scene.spawn("Reflection Probe", parent);
    let mut t = Transform::default();
    if parent.is_none() {
        t.position = state.scene_view.camera.pivot().round();
    }
    state.scene.set_transform(e, t);
    state.scene.add(e, ReflectionProbe::default());
    state.select(e);
    state.console.info("Created 'Reflection Probe'.");
}

/// Generate Probes: replace the group with probes placed over the scene, then bake them.
pub fn generate(state: &mut EditorState) {
    let settings = group_settings(state);
    if let Some(g) = group(state) {
        // The old probes shouldn't take part in placement (they have no geometry anyway).
        state.record("Generate Reflection Probes");
        state.scene.despawn(g);
    } else {
        state.record("Generate Reflection Probes");
    }
    let geo = placement::scene_geometry(&state.scene);
    let placed = placement::place(&geo, &settings);
    let count = spawn_group(&mut state.scene, &settings, &placed);
    state.selection.retain_existing(&state.scene);
    state.probes.status = format!("Generated {count} reflection probes.");
    state.console.info(state.probes.status.clone());
    // Then bake them (when the scene is saved).
    if count > 0 && state.files.path.is_some() {
        state.probes.bake_requested = true;
    }
}

/// The group object with one child per placed probe; returns the probe count.
pub fn spawn_group(
    scene: &mut misty_engine::scene::Scene,
    settings: &ReflectionProbeGroup,
    placed: &[placement::PlacedProbe],
) -> usize {
    let g = scene.spawn(GROUP_NAME, None);
    scene.add(g, settings.clone());
    for (i, p) in placed.iter().enumerate() {
        let e = scene.spawn(&format!("Reflection Probe {}", i + 1), Some(g));
        scene.set_transform(e, Transform::from_position(p.position));
        scene.add(e, p.component(settings.resolution));
    }
    placed.len()
}

/// Bake Reflection Probes: queue every Baked probe (the capture runs in [`gpu_frame`]).
pub fn start_bake(state: &mut EditorState) {
    if state.probes.job.is_some() {
        return;
    }
    let Some(scene_path) = state.files.path.clone() else {
        state.probes.status = "Save the scene before baking reflection probes.".into();
        return;
    };
    let list: Vec<_> = probes::scene_probe_list(&state.scene)
        .into_iter()
        .filter(|(_, p)| p.mode == ProbeMode::Baked)
        .collect();
    if list.is_empty() {
        state.probes.status = "No Baked reflection probes in the scene.".into();
        return;
    }
    let passes = 1 + group_settings(state).second_bounce as usize;
    state.probes.job = Some(BakeJob {
        scene_path,
        probes: list,
        passes,
        pass: 0,
        index: 0,
        inputs: None,
        done: Vec::new(),
        previous: Vec::new(),
        capture: None,
        started: Instant::now(),
    });
    state.probes.bake_requested = false;
}

/// Per-frame GPU work: one bake step and the Realtime probes. True while work is left.
pub fn gpu_frame(gpu: &mut Gpu, state: &mut EditorState) -> bool {
    if state.probes.bake_requested && state.probes.job.is_none() {
        start_bake(state);
    }
    if state.probes.job.is_some() {
        bake_step(gpu, state);
    }
    let (device, queue) = (&gpu.state.device, &gpu.state.queue);
    let faces = state.probes.realtime.update(
        &mut gpu.renderer,
        device,
        queue,
        &state.scene,
        &state.assets,
    );
    state.probes.job.is_some() || (faces > 0 && state.play.in_play_mode())
}

/// Capture one probe of the bake.
fn bake_step(gpu: &mut Gpu, state: &mut EditorState) {
    let Some(mut job) = state.probes.job.take() else {
        return;
    };
    let (device, queue) = (&gpu.state.device, &gpu.state.queue);
    if job.inputs.is_none() {
        let mut inputs = CaptureInputs::from_scene(&state.scene, &state.assets);
        if job.pass > 0 {
            inputs.environment.probes = probe_set(&state.scene, &job.probes, &job.previous);
        }
        job.inputs = Some(inputs);
    }
    let (e, p) = &job.probes[job.index];
    if !state.scene.contains(*e) {
        state.probes.status = "Bake cancelled: the scene changed.".into();
        return;
    }
    let s = CaptureSettings::of(p, state.scene.world_position(*e));
    if job.capture.as_ref().map(|c| c.size()) != Some(s.size) {
        job.capture = Some(ProbeCapture::new(&gpu.renderer, device, s.size));
    }
    let capture = job.capture.as_ref().expect("made above");
    let inputs = job.inputs.as_ref().expect("made above");
    let cube = capture.cube(&mut gpu.renderer, device, queue, inputs, &s);
    job.done
        .push(Arc::new(ProbeData::from_capture(&cube, BAKE_SAMPLES)));
    job.index += 1;
    if job.index == job.probes.len() {
        job.index = 0;
        job.pass += 1;
        job.previous = std::mem::take(&mut job.done);
        job.inputs = None;
        if job.pass == job.passes {
            finish_bake(state, job);
            return;
        }
    }
    state.probes.job = Some(job);
}

/// Save the probes next to the scene and point them at their files.
fn finish_bake(state: &mut EditorState, job: BakeJob) {
    state.record("Bake Reflection Probes");
    let mut saved = 0;
    for (i, ((e, _), data)) in job.probes.iter().zip(job.previous).enumerate() {
        let rel = probes::asset_path_for(&job.scene_path, i);
        let path = state.project.root.join(&rel);
        if let Err(err) = data.save(&path) {
            state.probes.status = format!("Could not save {rel}: {err}");
            return;
        }
        store::insert(&path, data);
        if let Ok(mut p) = state.scene.world.get::<&mut ReflectionProbe>(*e) {
            p.baked = Some(rel);
        }
        saved += 1;
    }
    remove_stale_files(state, &job.scene_path, saved);
    state.probes.status = format!(
        "Baked {saved} reflection probes in {:.1} s.",
        job.started.elapsed().as_secs_f32()
    );
    state.console.info(state.probes.status.clone());
    state.refresh_assets();
}

/// Delete `ReflectionProbe-N` files from an earlier bake with more probes.
fn remove_stale_files(state: &EditorState, scene_path: &str, keep: usize) {
    for i in keep.. {
        let path = state
            .project
            .root
            .join(probes::asset_path_for(scene_path, i));
        if !path.exists() {
            break;
        }
        let _ = std::fs::remove_file(&path);
        store::forget(&path);
    }
}

/// Clear Baked Data: forget the files of every probe.
pub fn clear(state: &mut EditorState) {
    let baked: Vec<Entity> = probes::scene_probe_list(&state.scene)
        .into_iter()
        .filter(|(_, p)| p.baked.is_some())
        .map(|(e, _)| e)
        .collect();
    if baked.is_empty() {
        return;
    }
    state.record("Clear Baked Reflection Probes");
    for e in baked {
        if let Ok(mut p) = state.scene.world.get::<&mut ReflectionProbe>(e)
            && let Some(rel) = p.baked.take()
        {
            let path = state.project.root.join(&rel);
            let _ = std::fs::remove_file(&path);
            store::forget(&path);
        }
    }
    state.probes.status = "Cleared baked reflection probes.".into();
    state.refresh_assets();
}

/// Lighting window ▸ Scene: generator settings, Generate / Bake / Clear and the progress.
pub fn lighting_section(ui: &mut egui::Ui, state: &mut EditorState) {
    ui.label(egui::RichText::new("Reflection Probes").strong());
    let mut s = group_settings(state);
    let changed = egui::Grid::new("lighting_probes")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(150.0)
        .show(ui, |ui| {
            let mut ch = false;
            ui.label("Probe Spacing");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut s.spacing)
                        .speed(0.1)
                        .range(0.5..=100.0)
                        .suffix(" m"),
                )
                .changed();
            ui.end_row();
            ui.label("Height Above Floor");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut s.height)
                        .speed(0.05)
                        .range(0.2..=20.0)
                        .suffix(" m"),
                )
                .changed();
            ui.end_row();
            ui.label("Merge Similar Probes");
            ch |= ui.checkbox(&mut s.merge, "").changed();
            ui.end_row();
            ui.label("Resolution");
            egui::ComboBox::from_id_salt("probe_gen_res")
                .selected_text(s.resolution.to_string())
                .show_ui(ui, |ui| {
                    for r in misty_engine::scene::reflection_probe::RESOLUTIONS {
                        ch |= ui
                            .selectable_value(&mut s.resolution, r, r.to_string())
                            .changed();
                    }
                });
            ui.end_row();
            ui.label("Second Bounce");
            ch |= ui.checkbox(&mut s.second_bounce, "").changed();
            ui.end_row();
            ch
        })
        .inner;
    if changed {
        set_group_settings(state, s);
    }
    let busy = state.probes.baking() || state.play.in_play_mode();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Generate Probes"))
            .on_hover_text(
                "Place probes over the scene (replaces the Reflection Probes group), then bake",
            )
            .clicked()
        {
            generate(state);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Bake Reflection Probes"))
            .clicked()
        {
            start_bake(state);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Clear Baked Data"))
            .clicked()
        {
            clear(state);
        }
    });
    if let Some((f, label)) = state.probes.progress() {
        ui.add(egui::ProgressBar::new(f).text(label));
        ui.ctx().request_repaint();
    } else if !state.probes.status.is_empty() {
        ui.weak(&state.probes.status);
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use glam::Vec3;
    use misty_engine::scene::{MeshFilter, MeshRenderer, PrimitiveMesh};

    use super::*;
    use crate::project::ProjectBrowser;

    /// A saved scene with a closed 6 � 3 � 6 m box room.
    fn room_state(name: &str) -> EditorState {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root));
        for (at, size) in [
            (Vec3::new(0.0, -0.1, 0.0), Vec3::new(6.4, 0.2, 6.4)),
            (Vec3::new(0.0, 3.1, 0.0), Vec3::new(6.4, 0.2, 6.4)),
            (Vec3::new(-3.1, 1.5, 0.0), Vec3::new(0.2, 3.0, 6.0)),
            (Vec3::new(3.1, 1.5, 0.0), Vec3::new(0.2, 3.0, 6.0)),
            (Vec3::new(0.0, 1.5, -3.1), Vec3::new(6.0, 3.0, 0.2)),
            (Vec3::new(0.0, 1.5, 3.1), Vec3::new(6.0, 3.0, 0.2)),
        ] {
            let e = s.scene.spawn("Wall", None);
            let mut t = Transform::from_position(at);
            t.scale = size;
            s.scene.set_transform(e, t);
            s.scene.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
            s.scene.add(e, MeshRenderer::default());
        }
        s.files.path = Some("Assets/Scenes/Room.scene".into());
        s
    }

    #[test]
    fn generate_probes_fills_the_room_and_undoes() {
        let mut h = Harness::builder().with_size([520.0, 760.0]).build_ui_state(
            |ui, s: &mut EditorState| lighting_section(ui, s),
            room_state("misty_probe_generate_test"),
        );
        h.run();
        h.get_by_label("Probe Spacing");
        let s = h.state_mut();
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
        h.get_by_label("Generate Probes").click();
        h.run();
        let s = h.state_mut();
        let g = group(s).expect("a Reflection Probes group");
        let probes = s.scene.children(g);
        assert!(!probes.is_empty());
        // Probes in the room (one more may sit on the roof, in the open).
        let inside: Vec<_> = probes
            .iter()
            .filter(|e| s.scene.world_position(**e).y < 3.0)
            .collect();
        assert!(!inside.is_empty());
        for e in inside {
            let p = s.scene.world.get::<&ReflectionProbe>(*e).unwrap();
            let (lo, hi) = p.world_box(s.scene.world_position(*e));
            assert!(
                p.box_projection && lo.x < -2.9 && hi.x > 2.9 && hi.y > 2.9,
                "{lo} {hi}"
            );
        }
        // Baking waits for the GPU (the app calls gpu_frame after the panels).
        assert!(s.probes.baking());
        assert!(
            s.probes.status.starts_with("Generated"),
            "{}",
            s.probes.status
        );
        s.undo.end_frame(false);
        crate::commands::apply(s, crate::actions::Action::Undo);
        assert!(group(s).is_none());
    }

    #[test]
    fn create_menu_adds_a_probe_at_the_pivot() {
        let mut s = room_state("misty_probe_create_test");
        create_probe(&mut s, None);
        let e = s.selection.active().unwrap();
        assert!(s.scene.world.get::<&ReflectionProbe>(e).is_ok());
        assert_eq!(s.scene.name(e), "Reflection Probe");
        // Baking needs a saved scene.
        s.files.path = None;
        start_bake(&mut s);
        assert!(!s.probes.baking());
        assert!(s.probes.status.contains("Save the scene"));
    }
}
