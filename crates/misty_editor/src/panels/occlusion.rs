//! Occlusion Culling window (Window ▸ Rendering ▸ Occlusion Culling): Object (Occluder /
//! Occludee Static of the selection, Occlusion Areas), Bake (Smallest Occluder, Smallest Hole,
//! Backface Threshold, Dynamic Occlusion, Clear / Bake with progress and Cancel) and
//! Visualization (what the game camera culls, drawn in the Scene view while this page shows).
//! Bakes run on a worker thread; the result is saved as `<Scene folder>/<Scene>/OcclusionData.occ`
//! and referenced from the scene's Lighting settings.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use egui::{Grid, RichText, Ui};
use glam::Vec3;
use misty_engine::occlusion::bake::{self, BakeInput, Progress};
use misty_engine::occlusion::{CellLookup, OcclusionData, OcclusionSettings, store};
use misty_engine::scene::{OcclusionArea, StaticFlags, Transform};

use crate::state::EditorState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    Object,
    #[default]
    Bake,
    Visualization,
}

/// A bake in progress.
struct BakeJob {
    handle: JoinHandle<Option<OcclusionData>>,
    progress: Arc<Progress>,
    /// Project path the result is saved to.
    target: String,
    started: Instant,
}

/// Occlusion Culling window state (kept while the window is closed).
pub struct OcclusionWindow {
    pub page: Page,
    /// egui frame the window was last drawn in.
    pub seen_frame: Option<u64>,
    /// Visualization: draw the camera's cell and the view volumes, hide culled objects.
    pub show_cell: bool,
    pub show_volumes: bool,
    pub hide_culled: bool,
    /// Last bake result or problem.
    pub status: String,
    job: Option<BakeJob>,
}

impl Default for OcclusionWindow {
    fn default() -> Self {
        Self {
            page: Page::default(),
            seen_frame: None,
            show_cell: true,
            show_volumes: true,
            hide_culled: true,
            status: String::new(),
            job: None,
        }
    }
}

impl OcclusionWindow {
    pub fn baking(&self) -> bool {
        self.job.is_some()
    }
}

/// The window is shown on its Visualization page (drawn this frame or the one before).
pub fn visualizing(state: &EditorState, ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_frame_nr();
    state.occlusion.page == Page::Visualization
        && state.occlusion.seen_frame.is_some_and(|f| f + 1 >= now)
}

/// The open scene's baked data, if any.
pub fn current_data(state: &EditorState) -> Option<Arc<OcclusionData>> {
    let rel = state.scene.render_settings.occlusion.data.as_ref()?;
    store::load(&state.project.root.join(rel))
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    state.occlusion.seen_frame = Some(ui.ctx().cumulative_frame_nr());
    poll(state);
    if state.occlusion.baking() || state.occlusion.page == Page::Visualization {
        ui.ctx().request_repaint();
    }
    ui.horizontal(|ui| {
        let page = &mut state.occlusion.page;
        ui.selectable_value(page, Page::Object, "Object");
        ui.selectable_value(page, Page::Bake, "Bake");
        ui.selectable_value(page, Page::Visualization, "Visualization");
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.occlusion.page {
            Page::Object => object_page(ui, state),
            Page::Bake => bake_page(ui, state),
            Page::Visualization => visualization_page(ui, state),
        });
}

fn object_page(ui: &mut Ui, state: &mut EditorState) {
    let targets = state.selection.list().to_vec();
    match targets.first() {
        None => {
            ui.weak("Select objects to set their occlusion flags.");
        }
        Some(&e) => {
            let label = match targets.len() {
                1 => state.scene.name(e).to_string(),
                n => format!("{n} objects"),
            };
            ui.label(RichText::new(label).strong());
            for (flag, name) in [
                (StaticFlags::OCCLUDER, "Occluder Static"),
                (StaticFlags::OCCLUDEE, "Occludee Static"),
            ] {
                let mut on = state.scene.game_object(e).has_static(flag);
                if ui.checkbox(&mut on, name).changed() {
                    state.record(&format!("Change {name}"));
                    for &t in &targets {
                        state.scene.game_object_mut(t).set_static_flag(flag, on);
                    }
                }
            }
            ui.weak(
                "Occluders hide what is behind them (walls, buildings, terrain); occludees are \
                 hidden when nothing of them can be seen. Both are Static flags.",
            );
        }
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Occlusion Areas").strong());
    let areas = state.scene.active_with::<OcclusionArea>();
    if areas.is_empty() {
        ui.weak("None: the bake covers the bounds of the static geometry.");
    } else {
        for a in areas {
            let name = state.scene.name(a).to_string();
            if ui.link(name).clicked() {
                state.select(a);
            }
        }
    }
    if ui.button("Create New Occlusion Area").clicked() {
        create_area(state);
    }
}

/// A View Volume Occlusion Area around the static geometry (3 m tall at its base).
fn create_area(state: &mut EditorState) {
    state.record("Create Occlusion Area");
    let boxes: Vec<_> = bake::occludees(&state.scene)
        .into_iter()
        .map(|(_, b)| b)
        .collect();
    let (center, size) =
        match boxes.iter().copied().reduce(|a, b| {
            misty_engine::render::culling::Aabb::new(a.min.min(b.min), a.max.max(b.max))
        }) {
            Some(b) => {
                let size = Vec3::new(b.max.x - b.min.x, 3.0, b.max.z - b.min.z);
                (
                    Vec3::new(
                        (b.min.x + b.max.x) * 0.5,
                        b.min.y + 1.5,
                        (b.min.z + b.max.z) * 0.5,
                    ),
                    size,
                )
            }
            None => (Vec3::new(0.0, 1.5, 0.0), Vec3::new(10.0, 3.0, 10.0)),
        };
    let e = state.scene.spawn("Occlusion Area", None);
    state
        .scene
        .set_transform(e, Transform::from_position(center));
    state.scene.add(
        e,
        OcclusionArea {
            size: size.max(Vec3::ONE).to_array(),
            ..Default::default()
        },
    );
    state.select(e);
}

fn bake_page(ui: &mut Ui, state: &mut EditorState) {
    let mut s = state.scene.render_settings.occlusion.clone();
    let busy = state.occlusion.baking() || state.play.in_play_mode();
    let changed = grid(ui, "occlusion_bake", |ui| {
        let mut ch = number(
            ui,
            "Smallest Occluder",
            &mut s.smallest_occluder,
            0.05,
            0.25,
            500.0,
        );
        ch |= number(ui, "Smallest Hole", &mut s.smallest_hole, 0.01, 0.01, 50.0);
        ui.label("Backface Threshold");
        ch |= ui
            .add(egui::Slider::new(&mut s.backface_threshold, 5.0..=100.0).suffix("%"))
            .changed();
        ui.end_row();
        ui.label("Dynamic Occlusion");
        ch |= ui
            .checkbox(&mut s.dynamic_occlusion, "")
            .on_hover_text("Also cull non-static renderers with the GPU depth pyramid (Hi-Z)")
            .changed();
        ui.end_row();
        ch
    });
    if changed && !busy {
        let label = crate::undo_names::struct_change(
            "Occlusion",
            &state.scene.render_settings.occlusion,
            &s,
        );
        state.scene.render_settings.occlusion = s.clone();
        state.mark_edited(&label);
    }
    let cell = s.cell_size();
    ui.weak(format!(
        "View cells of {cell:.2} m, {} rays per cube side from each sample point.",
        s.rays_per_side()
    ));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let has = state.scene.render_settings.occlusion.data.is_some();
        if ui
            .add_enabled(!busy && has, egui::Button::new("Clear"))
            .clicked()
        {
            clear(state);
        }
        if ui.add_enabled(!busy, egui::Button::new("Bake")).clicked() {
            start_bake(state);
        }
    });
    if let Some(job) = &state.occlusion.job {
        let f = job.progress.fraction();
        let secs = job.started.elapsed().as_secs_f32();
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::ProgressBar::new(f)
                    .desired_width(ui.available_width() - 70.0)
                    .text(format!("Baking occlusion… {:.0}% ({secs:.0} s)", f * 100.0)),
            );
            cancel = ui.button("Cancel").clicked();
        });
        if cancel {
            job.progress
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    } else if !state.occlusion.status.is_empty() {
        ui.weak(&state.occlusion.status);
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Baked Data").strong());
    match current_data(state) {
        Some(d) => {
            let kb = d.to_bytes().len() as f32 / 1024.0;
            ui.weak(format!(
                "{} — {} view cells ({}×{}×{}), {} distinct sets, {} occludees, {kb:.1} KB",
                state
                    .scene
                    .render_settings
                    .occlusion
                    .data
                    .as_deref()
                    .unwrap_or(""),
                d.cell_count(),
                d.dims.x,
                d.dims.y,
                d.dims.z,
                d.set_count(),
                d.occludees.len()
            ));
            let p = &state.scene.render_settings.occlusion;
            let params = [p.smallest_occluder, p.smallest_hole, p.backface_threshold];
            if d.params != params {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 190, 90),
                    "The bake parameters changed: bake again.",
                );
            }
        }
        None => {
            ui.weak("Not baked yet.");
        }
    }
}

fn visualization_page(ui: &mut Ui, state: &mut EditorState) {
    let w = &mut state.occlusion;
    ui.checkbox(&mut w.show_cell, "Camera Cell");
    ui.checkbox(&mut w.show_volumes, "View Volumes");
    ui.checkbox(&mut w.hide_culled, "Hide Culled Objects in the Scene view");
    ui.add_space(6.0);
    for line in summary(state) {
        ui.label(line);
    }
}

/// What the game camera's cell culls, as text lines (window and Scene view overlay).
pub fn summary(state: &EditorState) -> Vec<String> {
    let Some(data) = current_data(state) else {
        return vec!["No occlusion data: bake on the Bake tab.".into()];
    };
    let Some((camera, settings)) = misty_engine::render_scene::game_camera(&state.scene) else {
        return vec!["No camera rendering.".into()];
    };
    let total = data.occludees.len();
    let mut out = Vec::new();
    if !settings.occlusion_culling {
        out.push("The camera has Occlusion Culling off.".into());
    }
    match data.lookup(camera.position) {
        CellLookup::Outside => out.push("Camera outside the view volumes: nothing culled.".into()),
        CellLookup::Invalid { cell } => out.push(format!(
            "Camera cell {cell} is inside geometry: nothing culled."
        )),
        CellLookup::Cell { cell, set } => {
            let c = data.coords(cell);
            let visible = data.visible_count(set);
            out.push(format!("Camera cell {cell} ({}, {}, {})", c.x, c.y, c.z));
            out.push(format!(
                "Occludees: {visible} potentially visible, {} culled of {total}",
                total - visible
            ));
        }
    }
    out
}

/// Start baking the open scene on a worker thread.
pub fn start_bake(state: &mut EditorState) {
    if state.occlusion.baking() {
        return;
    }
    let Some(scene_path) = state.scene.active_path() else {
        state.occlusion.status = "Save the scene before baking.".into();
        return;
    };
    let input = BakeInput::from_scene(&state.scene);
    if input.occludees.is_empty() {
        state.occlusion.status =
            "Nothing to cull: mark renderers Occludee Static (Object tab) and bake again.".into();
        return;
    }
    let progress = Arc::new(Progress::default());
    let shared = progress.clone();
    let handle = std::thread::spawn(move || bake::bake(&input, &shared));
    state.occlusion.job = Some(BakeJob {
        handle,
        progress,
        target: OcclusionSettings::asset_path_for(&scene_path),
        started: Instant::now(),
    });
}

/// Pick up a finished bake: save it and point the scene at it.
pub fn poll(state: &mut EditorState) {
    if !state
        .occlusion
        .job
        .as_ref()
        .is_some_and(|j| j.handle.is_finished())
    {
        return;
    }
    let job = state.occlusion.job.take().expect("checked");
    let secs = job.started.elapsed().as_secs_f32();
    let data = match job.handle.join() {
        Ok(Some(d)) => d,
        Ok(None) => {
            state.occlusion.status = "Bake cancelled.".into();
            return;
        }
        Err(_) => {
            state.occlusion.status = "Bake failed.".into();
            return;
        }
    };
    let path = state.project.root.join(&job.target);
    if let Err(e) = data.save(&path) {
        state.occlusion.status = format!("Could not save the occlusion data: {e}");
        return;
    }
    state.occlusion.status = format!(
        "Baked {} view cells ({} sets) for {} occludees in {secs:.1} s → {}",
        data.cell_count(),
        data.set_count(),
        data.occludees.len(),
        job.target
    );
    store::insert(&path, Arc::new(data));
    if state.scene.render_settings.occlusion.data.as_deref() != Some(job.target.as_str()) {
        state.record("Bake Occlusion");
        state.scene.render_settings.occlusion.data = Some(job.target);
    }
    state.refresh_assets();
}

/// Wait for a running bake (tests).
#[cfg(test)]
pub fn finish_bake(state: &mut EditorState) {
    while state.occlusion.baking() {
        if state
            .occlusion
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

/// Remove the baked file and the scene's reference to it.
pub fn clear(state: &mut EditorState) {
    let Some(rel) = state.scene.render_settings.occlusion.data.clone() else {
        return;
    };
    let path = state.project.root.join(&rel);
    // To the Recycle Bin when possible.
    if path.exists() && trash::delete(&path).is_err() && std::fs::remove_file(&path).is_err() {
        state.occlusion.status = format!("Could not remove {rel}.");
        return;
    }
    store::forget(&path);
    state.record("Clear Occlusion Data");
    state.scene.render_settings.occlusion.data = None;
    state.occlusion.status = "Cleared.".into();
    state.refresh_assets();
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(130.0)
        .show(ui, body)
        .inner
}

fn number(ui: &mut Ui, label: &str, v: &mut f32, speed: f64, min: f32, max: f32) -> bool {
    ui.label(label);
    let ch = ui
        .add(
            egui::DragValue::new(v)
                .speed(speed)
                .range(min..=max)
                .max_decimals(3),
        )
        .changed();
    ui.end_row();
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use glam::Vec3;
    use misty_engine::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, Transform};

    use super::*;
    use crate::actions::Action;
    use crate::layout::Tab;
    use crate::project::ProjectBrowser;

    fn state(name: &str) -> EditorState {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
        EditorState::new(ProjectBrowser::open(root))
    }

    /// A saved scene: a wall between the origin and a crate.
    fn walled(name: &str) -> EditorState {
        let mut s = state(name);
        let mut add = |n: &str, p: Vec3, scale: Vec3| {
            let e = s.scene.spawn(n, None);
            let mut t = Transform::from_position(p);
            t.scale = scale;
            s.scene.set_transform(e, t);
            s.scene.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
            s.scene.add(e, MeshRenderer::default());
            s.scene.game_object_mut(e).is_static = true;
        };
        add("Wall", Vec3::new(0.0, 5.0, 5.0), Vec3::new(30.0, 10.0, 0.5));
        add("Crate", Vec3::new(0.0, 0.5, 12.0), Vec3::ONE);
        add("Front", Vec3::new(4.0, 0.5, -3.0), Vec3::ONE);
        // Floor slab (an occluder like every static object).
        add(
            "Floor",
            Vec3::new(0.0, -0.25, 4.0),
            Vec3::new(30.0, 0.5, 20.0),
        );
        let cam = s.scene.spawn("Main Camera", None);
        s.scene
            .set_transform(cam, Transform::from_position(Vec3::new(0.0, 1.7, 0.0)));
        s.scene.add(cam, misty_engine::scene::Camera::default());
        s.scene.render_settings.occlusion.smallest_occluder = 3.0;
        s.scene.render_settings.occlusion.smallest_hole = 1.0;
        // The camera stays on the near side of the wall, above the floor.
        let area = s.scene.spawn("Occlusion Area", None);
        s.scene
            .set_transform(area, Transform::from_position(Vec3::new(0.0, 2.0, -0.5)));
        s.scene.add(
            area,
            OcclusionArea {
                size: [24.0, 3.0, 9.0],
                ..Default::default()
            },
        );
        s.scene.set_scene_path(0, "Assets/Scenes/Walled.scene");
        s
    }

    #[test]
    fn window_menu_opens_the_occlusion_window() {
        let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| crate::menu_bar::show(ui, s),
            state("misty_occlusion_menu_test"),
        );
        h.run();
        h.get_by_label("Window").click();
        h.run();
        h.get_by_label("Rendering ⏵").hover();
        h.run();
        h.get_by_label("Occlusion Culling").click();
        h.run();
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert!(
            matches!(
                actions.as_slice(),
                [Action::FocusTab(Tab::OcclusionCulling)]
            ),
            "{actions:?}"
        );
    }

    #[test]
    fn bake_saves_data_next_to_the_scene_and_clear_removes_it() {
        let mut s = walled("misty_occlusion_bake_test");
        start_bake(&mut s);
        assert!(s.occlusion.baking());
        finish_bake(&mut s);
        let rel = s.scene.render_settings.occlusion.data.clone().unwrap();
        assert_eq!(rel, "Assets/Scenes/Walled/OcclusionData.occ");
        assert!(
            s.project.root.join(&rel).is_file(),
            "{}",
            s.occlusion.status
        );
        let data = current_data(&s).unwrap();
        assert_eq!(data.occludees.len(), 4);
        // The camera at the origin can't see the crate behind the wall.
        let lines = summary(&s).join("\n");
        assert!(lines.contains("1 culled of 4"), "{lines}");
        let set = match data.lookup(Vec3::new(0.0, 1.7, 0.0)) {
            CellLookup::Cell { set, .. } => set,
            other => panic!("{other:?}"),
        };
        let seen: Vec<bool> = (0..4).map(|i| data.visible(set, i)).collect();
        assert_eq!(seen, [true, false, true, true], "wall, crate, front, floor");
        // Undo puts the scene back without the reference.
        s.undo_step();
        assert!(s.scene.render_settings.occlusion.data.is_none());
        s.redo_step();
        clear(&mut s);
        assert!(s.scene.render_settings.occlusion.data.is_none());
        assert!(!s.project.root.join(&rel).exists());
    }

    #[test]
    fn cancelling_keeps_the_old_data() {
        let mut s = walled("misty_occlusion_cancel_test");
        start_bake(&mut s);
        let job = s.occlusion.job.as_ref().unwrap();
        job.progress
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        finish_bake(&mut s);
        assert!(s.scene.render_settings.occlusion.data.is_none());
        assert_eq!(s.occlusion.status, "Bake cancelled.");
        // Unsaved scenes can't bake.
        let mut s = state("misty_occlusion_unsaved_test");
        start_bake(&mut s);
        assert!(!s.occlusion.baking());
        assert!(s.occlusion.status.contains("Save the scene"));
    }

    #[test]
    fn object_flags_follow_the_static_rule() {
        let mut s = walled("misty_occlusion_flags_test");
        let e = s.scene.find_by_name("Front").unwrap();
        s.scene.game_object_mut(e).is_static = false;
        let go = &mut *s.scene.game_object_mut(e);
        go.set_static_flag(StaticFlags::OCCLUDEE, true);
        assert!(go.is_static && go.static_flags == StaticFlags::OCCLUDEE);
        go.set_static_flag(StaticFlags::OCCLUDER, true);
        assert!(go.has_static(StaticFlags::OCCLUDER));
        assert!(!go.has_static(StaticFlags::NAVIGATION));
        go.set_static_flag(StaticFlags::OCCLUDER, false);
        go.set_static_flag(StaticFlags::OCCLUDEE, false);
        assert!(!go.is_static && go.static_flags.is_all());
    }
}
