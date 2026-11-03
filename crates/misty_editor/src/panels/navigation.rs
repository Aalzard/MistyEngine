//! Navigation window (Window ▸ AI ▸ Navigation): Agents (radius, height, step, slope), Bake
//! (advanced voxel settings, Clear / Bake with a progress line) and Object (Navigation Static of
//! the selection). Bakes run on a worker thread; the result is saved as
//! `<Scene folder>/<Scene>/NavMesh.navmesh` and referenced from the scene. While the window is
//! shown the Scene view draws the nav mesh (see `nav_gizmos`).

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use egui::{Color32, Grid, RichText, Stroke, Ui, pos2, vec2};
use misty_engine::navigation::{self, NavMesh, NavigationSettings, geometry};

use crate::state::EditorState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Agents,
    Bake,
    Object,
}

/// A bake in progress.
struct BakeJob {
    handle: JoinHandle<NavMesh>,
    progress: Arc<Mutex<(f32, &'static str)>>,
    /// Project path the result is saved to.
    target: String,
    objects: usize,
    started: Instant,
}

/// Navigation window state (kept while the window is closed).
pub struct NavigationWindow {
    /// Draw the nav mesh in the Scene view while the window is open.
    pub show_mesh: bool,
    /// egui frame the window was last drawn in.
    pub seen_frame: Option<u64>,
    /// Last bake result or problem.
    pub status: String,
    job: Option<BakeJob>,
}

impl Default for NavigationWindow {
    fn default() -> Self {
        Self {
            show_mesh: true,
            seen_frame: None,
            status: String::new(),
            job: None,
        }
    }
}

impl NavigationWindow {
    pub fn baking(&self) -> bool {
        self.job.is_some()
    }
}

/// Whether the window was drawn this frame or the one before (the overlay follows it).
pub fn is_open(state: &EditorState, ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_frame_nr();
    state.navigation.seen_frame.is_some_and(|f| f + 1 >= now)
}

/// The open scene's baked nav mesh, if any.
pub fn current_mesh(state: &EditorState) -> Option<Arc<NavMesh>> {
    let rel = state.scene.navigation.nav_mesh.as_ref()?;
    navigation::store::load(&state.project.root.join(rel)).ok()
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    state.navigation.seen_frame = Some(ui.ctx().cumulative_frame_nr());
    poll(state);
    if state.navigation.baking() {
        ui.ctx().request_repaint();
    }
    let id = ui.id().with("navigation_page");
    let mut page: Page = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.selectable_value(&mut page, Page::Agents, "Agents");
        ui.selectable_value(&mut page, Page::Bake, "Bake");
        ui.selectable_value(&mut page, Page::Object, "Object");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut state.navigation.show_mesh, "Show NavMesh");
        });
    });
    ui.data_mut(|d| d.insert_temp(id, page));
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match page {
            Page::Agents => agents_page(ui, state),
            Page::Bake => bake_page(ui, state),
            Page::Object => object_page(ui, state),
        });
}

fn agents_page(ui: &mut Ui, state: &mut EditorState) {
    let mut s = state.scene.navigation.clone();
    ui.label(RichText::new("Humanoid").strong());
    agent_diagram(ui, &s);
    let changed = grid(ui, "nav_agent", |ui| {
        let a = &mut s.agent;
        let mut ch = number(ui, "Agent Radius", &mut a.radius, 0.01, 0.05, 50.0);
        ch |= number(ui, "Agent Height", &mut a.height, 0.01, 0.1, 50.0);
        ch |= number(ui, "Step Height", &mut a.step_height, 0.01, 0.0, 50.0);
        ui.label("Max Slope");
        ch |= ui
            .add(egui::Slider::new(&mut a.max_slope, 0.0..=60.0).suffix("°"))
            .changed();
        ui.end_row();
        ch
    });
    if changed {
        state.scene.navigation = s;
        state.mark_edited("Navigation");
    }
    ui.add_space(6.0);
    ui.weak("Bake again (Bake tab) after changing the agent.");
}

/// Side view of the agent cylinder, its step and its steepest slope.
fn agent_diagram(ui: &mut Ui, s: &NavigationSettings) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width().min(320.0), 96.0),
        egui::Sense::hover(),
    );
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_gray(40));
    let a = s.agent;
    let ground = rect.bottom() - 14.0;
    // Pixels per metre: fit the taller of height and diameter.
    let scale = (70.0 / a.height.max(a.radius * 2.0).max(0.1)).min(60.0);
    let cx = rect.left() + 70.0;
    let (r, h) = (a.radius * scale, a.height * scale);
    let body = egui::Rect::from_min_max(pos2(cx - r, ground - h), pos2(cx + r, ground));
    p.rect_filled(
        body,
        2.0,
        Color32::from_rgba_unmultiplied(80, 160, 255, 120),
    );
    p.rect_stroke(
        body,
        2.0,
        Stroke::new(1.0, Color32::from_rgb(120, 190, 255)),
        egui::StrokeKind::Inside,
    );
    let line = Stroke::new(1.0, Color32::from_gray(170));
    p.line_segment(
        [
            pos2(rect.left() + 8.0, ground),
            pos2(rect.right() - 8.0, ground),
        ],
        line,
    );
    // Step in front of the agent.
    let step = (a.step_height * scale).min(rect.height() - 20.0);
    let sx = cx + r + 14.0;
    p.line_segment([pos2(sx, ground), pos2(sx, ground - step)], line);
    p.line_segment(
        [pos2(sx, ground - step), pos2(sx + 30.0, ground - step)],
        line,
    );
    // Slope.
    let len = 70.0;
    let ang = a.max_slope.to_radians();
    let start = pos2(sx + 44.0, ground);
    let end = pos2(start.x + len * ang.cos(), ground - len * ang.sin());
    p.line_segment(
        [start, end],
        Stroke::new(1.5, Color32::from_rgb(120, 190, 255)),
    );
    let font = egui::FontId::proportional(11.0);
    p.text(
        pos2(start.x + 4.0, ground + 2.0),
        egui::Align2::LEFT_TOP,
        format!("{:.0}°", a.max_slope),
        font.clone(),
        Color32::GRAY,
    );
    p.text(
        pos2(cx, ground + 2.0),
        egui::Align2::CENTER_TOP,
        format!("R {:.2}  H {:.2}", a.radius, a.height),
        font,
        Color32::GRAY,
    );
}

fn bake_page(ui: &mut Ui, state: &mut EditorState) {
    let mut s = state.scene.navigation.clone();
    ui.label(RichText::new("Baked Agent Size").strong());
    let baked = current_mesh(state);
    match &baked {
        Some(m) => {
            let a = m.agent;
            ui.weak(format!(
                "Radius {:.2}, Height {:.2}, Step {:.2}, Slope {:.0}° — {} polygons, {:.1} m²",
                a.radius,
                a.height,
                a.step_height,
                a.max_slope,
                m.polys.len(),
                m.area()
            ));
        }
        None => {
            ui.weak("Not baked yet.");
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Advanced").strong());
    let changed = grid(ui, "nav_advanced", |ui| {
        ui.label("Manual Voxel Size");
        let mut ch = ui.checkbox(&mut s.manual_voxel_size, "").changed();
        ui.end_row();
        ui.label("Voxel Size");
        if s.manual_voxel_size {
            ch |= ui
                .add(
                    egui::DragValue::new(&mut s.voxel_size)
                        .speed(0.005)
                        .range(0.01..=5.0),
                )
                .changed();
        } else {
            ui.weak(format!("{:.4} (radius / 3)", s.cell_size()));
        }
        ui.end_row();
        ch |= number(
            ui,
            "Min Region Area",
            &mut s.min_region_area,
            0.1,
            0.0,
            1000.0,
        );
        ch
    });
    if changed {
        state.scene.navigation = s;
        state.mark_edited("Navigation");
    }
    ui.add_space(8.0);
    let busy = state.navigation.baking() || state.play.in_play_mode();
    ui.horizontal(|ui| {
        let has = state.scene.navigation.nav_mesh.is_some();
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
    if let Some(job) = &state.navigation.job {
        let (f, stage) = *job.progress.lock().unwrap_or_else(|e| e.into_inner());
        ui.add(egui::ProgressBar::new(f).text(format!("Baking: {stage}…")));
    } else if !state.navigation.status.is_empty() {
        ui.weak(&state.navigation.status);
    }
    ui.add_space(6.0);
    ui.weak(
        "Objects marked Static (see the Object tab) with a Mesh Renderer or Terrain are baked.",
    );
}

fn object_page(ui: &mut Ui, state: &mut EditorState) {
    let Some(e) = state.selection.active() else {
        ui.weak("Select an object to set its navigation flags.");
        return;
    };
    ui.label(RichText::new(state.scene.name(e)).strong());
    let mut on = state.scene.game_object(e).is_static;
    if ui.checkbox(&mut on, "Navigation Static").changed() {
        state.record("Navigation Static");
        for t in state.selection.list().to_vec() {
            state.scene.game_object_mut(t).is_static = on;
        }
    }
    ui.weak("Same as the Static flag in the Inspector header.");
}

/// Start baking the open scene on a worker thread.
pub fn start_bake(state: &mut EditorState) {
    if state.navigation.baking() {
        return;
    }
    let Some(scene_path) = state.files.path.clone() else {
        state.navigation.status = "Save the scene before baking.".into();
        return;
    };
    let geo = geometry::collect(&state.scene);
    let objects = geo.objects;
    let settings = state.scene.navigation.clone();
    let progress = Arc::new(Mutex::new((0.0, "Collecting geometry")));
    let shared = progress.clone();
    let handle = std::thread::spawn(move || {
        navigation::bake::bake(&geo, &settings, &mut |f, stage| {
            *shared.lock().unwrap_or_else(|e| e.into_inner()) = (f, stage);
        })
    });
    state.navigation.job = Some(BakeJob {
        handle,
        progress,
        target: NavigationSettings::asset_path_for(&scene_path),
        objects,
        started: Instant::now(),
    });
}

/// Pick up a finished bake: save it and point the scene at it.
pub fn poll(state: &mut EditorState) {
    if !state
        .navigation
        .job
        .as_ref()
        .is_some_and(|j| j.handle.is_finished())
    {
        return;
    }
    let job = state.navigation.job.take().expect("checked");
    let secs = job.started.elapsed().as_secs_f32();
    let mesh = match job.handle.join() {
        Ok(m) => m,
        Err(_) => {
            state.navigation.status = "Bake failed.".into();
            return;
        }
    };
    let path = state.project.root.join(&job.target);
    if let Err(e) = mesh.save(&path) {
        state.navigation.status = format!("Could not save the nav mesh: {e}");
        return;
    }
    state.navigation.status = format!(
        "Baked {} polygons ({:.1} m²) from {} static objects in {secs:.1} s → {}",
        mesh.polys.len(),
        mesh.area(),
        job.objects,
        job.target
    );
    if mesh.is_empty() {
        state.navigation.status =
            "Nothing walkable: mark floors Static (Object tab) and bake again.".into();
    }
    navigation::store::insert(&path, Arc::new(mesh));
    if state.scene.navigation.nav_mesh.as_deref() != Some(job.target.as_str()) {
        state.record("Bake NavMesh");
        state.scene.navigation.nav_mesh = Some(job.target);
    }
    state.refresh_assets();
}

/// Wait for a running bake (tests).
#[cfg(test)]
pub fn finish_bake(state: &mut EditorState) {
    while state.navigation.baking() {
        if state
            .navigation
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

/// Remove the baked asset and the scene's reference to it.
pub fn clear(state: &mut EditorState) {
    let Some(rel) = state.scene.navigation.nav_mesh.clone() else {
        return;
    };
    let path = state.project.root.join(&rel);
    // To the Recycle Bin when possible.
    if path.exists() && trash::delete(&path).is_err() && std::fs::remove_file(&path).is_err() {
        state.navigation.status = format!("Could not remove {rel}.");
        return;
    }
    navigation::store::forget(&path);
    state.record("Clear NavMesh");
    state.scene.navigation.nav_mesh = None;
    state.navigation.status = "Cleared.".into();
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

    /// A saved scene with a static 10 m floor.
    fn floor_state(name: &str) -> EditorState {
        let mut s = state(name);
        let f = s.scene.spawn("Floor", None);
        let mut t = Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
        t.scale = Vec3::new(10.0, 1.0, 10.0);
        s.scene.set_transform(f, t);
        s.scene.add(f, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.scene.add(f, MeshRenderer::default());
        s.scene.game_object_mut(f).is_static = true;
        s.files.path = Some("Assets/Scenes/Level.scene".into());
        s
    }

    #[test]
    fn window_menu_opens_the_navigation_window() {
        let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| crate::menu_bar::show(ui, s),
            state("misty_navigation_menu_test"),
        );
        h.run();
        h.get_by_label("Window").click();
        h.run();
        h.get_by_label("AI ⏵").hover();
        h.run();
        h.get_by_label("Navigation").click();
        h.run();
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert!(
            matches!(actions.as_slice(), [Action::FocusTab(Tab::Navigation)]),
            "{actions:?}"
        );
    }

    #[test]
    fn bake_button_saves_the_nav_mesh_next_to_the_scene() {
        let mut h = Harness::builder().with_size([520.0, 640.0]).build_ui_state(
            |ui, s: &mut EditorState| show(ui, s),
            floor_state("misty_navigation_bake_test"),
        );
        h.run();
        h.get_by_label("Agent Radius");
        h.get_all_by_label("Bake").next().unwrap().click();
        h.run();
        h.get_by_label("Not baked yet.");
        h.get_all_by_label("Bake").last().unwrap().click();
        // The window repaints while baking.
        h.step();
        finish_bake(h.state_mut());
        h.run();
        let rel = "Assets/Scenes/Level/NavMesh.navmesh";
        {
            let s = h.state();
            assert_eq!(s.scene.navigation.nav_mesh.as_deref(), Some(rel));
            assert!(s.project.root.join(rel).is_file());
            assert!(s.scene_dirty);
            assert!(
                s.navigation.status.starts_with("Baked"),
                "{}",
                s.navigation.status
            );
            let mesh = current_mesh(s).unwrap();
            assert!((mesh.area() - 81.0).abs() < 4.0, "{}", mesh.area());
            // The overlay draws while the window is open.
            let mut lines = crate::gizmos::Lines::default();
            let tris = crate::gizmos::nav_overlay(s, &h.ctx, &mut lines);
            assert!(!tris.is_empty() && !lines.0.is_empty());
        }
        h.get_by_label("Clear").click();
        h.run();
        let s = h.state();
        assert!(s.scene.navigation.nav_mesh.is_none());
        assert!(!s.project.root.join(rel).exists());
    }

    #[test]
    fn baking_needs_a_saved_scene() {
        let mut s = floor_state("misty_navigation_unsaved_test");
        s.files.path = None;
        start_bake(&mut s);
        assert!(!s.navigation.baking());
        assert!(s.navigation.status.contains("Save the scene"));
    }
}
