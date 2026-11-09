//! Navigation window (Window ▸ AI ▸ Navigation): **Agents** (agent types: radius, height, step,
//! slope), **Areas** (32 named areas with path costs), **Bake** (the scene's Navigation Static
//! bake for the Humanoid, Bake All Surfaces, progress) and **Object** (Navigation Static of the
//! selection). Bakes run on the background pool, tile by tile; tiles whose input didn't change
//! are reused. Results are saved next to the scene (`<Scene>/NavMesh.navmesh` and
//! `<Scene>/NavMesh-<Surface>.navmesh`). While the window is shown, or a NavMesh Surface is
//! selected, the Scene view draws the nav meshes coloured by area (see `nav_gizmos`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{Color32, Grid, RichText, Stroke, Ui, pos2, vec2};
use misty_engine::hecs::Entity;
use misty_engine::navigation::{
    self, AREA_COUNT, AgentSettings, NavArea, NavMesh, NavMeshSurface, NavigationSettings, geometry,
};

use crate::state::EditorState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Agents,
    Areas,
    Bake,
    Object,
}

type Shared<T> = Arc<Mutex<T>>;

/// A bake in progress.
struct BakeJob {
    result: Shared<Option<NavMesh>>,
    progress: Shared<(f32, &'static str)>,
    /// Project path the result is saved to.
    target: String,
    objects: usize,
    started: Instant,
    /// The surface being baked (None = the scene's Navigation Static bake).
    surface: Option<Entity>,
    previous: Option<Arc<NavMesh>>,
}

impl BakeJob {
    fn progress(&self) -> (f32, &'static str) {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finished(&self) -> bool {
        self.result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
}

/// Navigation window state (kept while the window is closed).
pub struct NavigationWindow {
    /// Draw the nav mesh in the Scene view while the window is open.
    pub show_mesh: bool,
    /// Draw tile borders.
    pub show_tiles: bool,
    /// Draw every agent's path in Play mode (not just the selected ones).
    pub show_all_paths: bool,
    /// egui frame the window was last drawn in.
    pub seen_frame: Option<u64>,
    /// Last bake result or problem.
    pub status: String,
    /// Agent type shown on the Agents page.
    pub selected_agent: u32,
    job: Option<BakeJob>,
    surface_jobs: Vec<BakeJob>,
    surface_status: HashMap<Entity, String>,
}

impl Default for NavigationWindow {
    fn default() -> Self {
        // `MISTY_NAV_TILES=1` / `MISTY_NAV_PATHS=1` turn the options on (screenshots).
        Self {
            show_mesh: true,
            show_tiles: std::env::var_os("MISTY_NAV_TILES").is_some(),
            show_all_paths: std::env::var_os("MISTY_NAV_PATHS").is_some(),
            seen_frame: None,
            status: String::new(),
            selected_agent: 0,
            job: None,
            surface_jobs: Vec::new(),
            surface_status: HashMap::new(),
        }
    }
}

impl NavigationWindow {
    /// Any bake running.
    pub fn baking(&self) -> bool {
        self.job.is_some() || !self.surface_jobs.is_empty()
    }
}

/// Whether the window was drawn this frame or the one before (the overlay follows it).
pub fn is_open(state: &EditorState, ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_frame_nr();
    state.navigation.seen_frame.is_some_and(|f| f + 1 >= now)
}

/// The open scene's Navigation Static nav mesh, if baked.
pub fn current_mesh(state: &EditorState) -> Option<Arc<NavMesh>> {
    let rel = state.scene.navigation.nav_mesh.as_ref()?;
    navigation::store::load(&state.project.root.join(rel)).ok()
}

/// Baked data of every NavMesh Surface in the scene.
pub fn surface_meshes(state: &EditorState) -> Vec<(Entity, Arc<NavMesh>)> {
    let mut out = Vec::new();
    for (e, _) in state.scene.walk() {
        let Ok(s) = state.scene.world.get::<&NavMeshSurface>(e) else {
            continue;
        };
        if let Some(rel) = &s.nav_mesh_data
            && let Ok(m) = navigation::store::load(&state.project.root.join(rel))
        {
            out.push((e, m));
        }
    }
    out
}

/// Bake progress of the surface on `e`.
pub fn surface_progress(state: &EditorState, e: Entity) -> Option<(f32, &'static str)> {
    state
        .navigation
        .surface_jobs
        .iter()
        .find(|j| j.surface == Some(e))
        .map(BakeJob::progress)
}

/// Last bake result of the surface on `e`.
pub fn surface_status(state: &EditorState, e: Entity) -> Option<&str> {
    state.navigation.surface_status.get(&e).map(String::as_str)
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    state.navigation.seen_frame = Some(ui.ctx().cumulative_frame_nr());
    poll(state);
    if state.navigation.baking() {
        ui.ctx().request_repaint();
    }
    let id = ui.id().with("navigation_page");
    let mut page: Page = ui.data(|d| d.get_temp(id)).unwrap_or_else(env_page);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut page, Page::Agents, "Agents");
        ui.selectable_value(&mut page, Page::Areas, "Areas");
        ui.selectable_value(&mut page, Page::Bake, "Bake");
        ui.selectable_value(&mut page, Page::Object, "Object");
    });
    ui.horizontal(|ui| {
        let n = &mut state.navigation;
        ui.checkbox(&mut n.show_mesh, "Show NavMesh");
        ui.checkbox(&mut n.show_tiles, "Show Tiles");
        ui.checkbox(&mut n.show_all_paths, "Show All Agent Paths");
    });
    ui.data_mut(|d| d.insert_temp(id, page));
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match page {
            Page::Agents => agents_page(ui, state),
            Page::Areas => areas_page(ui, state),
            Page::Bake => bake_page(ui, state),
            Page::Object => object_page(ui, state),
        });
}

/// `MISTY_NAV_PAGE=areas` (agents / areas / bake / object) opens that page first (screenshots).
fn env_page() -> Page {
    match std::env::var("MISTY_NAV_PAGE").as_deref() {
        Ok("areas") => Page::Areas,
        Ok("bake") => Page::Bake,
        Ok("object") => Page::Object,
        _ => Page::Agents,
    }
}

/// Apply an edited copy of the scene's navigation settings (undoable).
fn commit(state: &mut EditorState, s: NavigationSettings) {
    if s != state.scene.navigation {
        let label = crate::undo_names::struct_change("Navigation", &state.scene.navigation, &s);
        state.scene.navigation = s;
        state.mark_edited(&label);
    }
}

fn agents_page(ui: &mut Ui, state: &mut EditorState) {
    let mut s = state.scene.navigation.clone();
    let types = s.all_agent_types();
    if !types
        .iter()
        .any(|t| t.id == state.navigation.selected_agent)
    {
        state.navigation.selected_agent = 0;
    }
    let mut selected = state.navigation.selected_agent;
    ui.horizontal(|ui| {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(140.0);
            ui.vertical(|ui| {
                for t in &types {
                    ui.selectable_value(&mut selected, t.id, &t.name);
                }
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("Add an agent type").clicked() {
                        selected = s.add_agent_type("New Agent");
                    }
                    let can_remove = selected != 0;
                    if ui
                        .add_enabled(can_remove, egui::Button::new("−"))
                        .on_hover_text("Remove the selected agent type")
                        .clicked()
                    {
                        s.agent_types.retain(|t| t.id != selected);
                        selected = 0;
                    }
                });
            });
        });
        ui.vertical(|ui| {
            let name = s.agent_type_name(selected);
            if selected == 0 {
                ui.label(RichText::new("Humanoid").strong());
            } else if let Some(t) = s.agent_types.iter_mut().find(|t| t.id == selected) {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut t.name);
                });
            } else {
                ui.label(name);
            }
            let mut a = s.agent_settings(selected).unwrap_or_default();
            agent_diagram(ui, &a);
            let changed = grid(ui, "nav_agent", |ui| {
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
                if selected == 0 {
                    s.agent = a;
                } else if let Some(t) = s.agent_types.iter_mut().find(|t| t.id == selected) {
                    t.settings = a;
                }
            }
            ui.add_space(6.0);
            ui.weak(format!(
                "Agent type id {selected}. Bake again (Bake tab, or the NavMesh Surfaces) after \
                 changing it."
            ));
        });
    });
    state.navigation.selected_agent = selected;
    commit(state, s);
}

fn areas_page(ui: &mut Ui, state: &mut EditorState) {
    let mut s = state.scene.navigation.clone();
    ui.weak("Path cost per metre on each area. Built-in areas keep their names.");
    Grid::new("nav_areas")
        .num_columns(4)
        .striped(true)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            ui.weak("");
            ui.weak("");
            ui.label(RichText::new("Name").strong());
            ui.label(RichText::new("Cost").strong());
            ui.end_row();
            for i in 0..AREA_COUNT {
                let mut a = s.area(i);
                let builtin = i < 3;
                ui.label(if builtin {
                    format!("Built-in {i}")
                } else {
                    format!("User {i}")
                });
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                let color = if a.name.is_empty() {
                    Color32::from_gray(70)
                } else {
                    super::inspector::nav_area_color(i as u8)
                };
                ui.painter().rect_filled(rect, 2.0, color);
                let mut ch = false;
                if builtin {
                    ui.label(&a.name);
                } else {
                    ch |= ui
                        .add(egui::TextEdit::singleline(&mut a.name).desired_width(140.0))
                        .changed();
                }
                if i == 1 {
                    ui.weak("—");
                } else {
                    ch |= ui
                        .add(
                            egui::DragValue::new(&mut a.cost)
                                .speed(0.05)
                                .range(1.0..=1000.0)
                                .max_decimals(2),
                        )
                        .changed();
                }
                ui.end_row();
                if ch {
                    s.set_area(i, NavArea { ..a });
                }
            }
        });
    commit(state, s);
}

/// Side view of the agent cylinder, its step and its steepest slope.
fn agent_diagram(ui: &mut Ui, a: &AgentSettings) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width().min(320.0), 96.0),
        egui::Sense::hover(),
    );
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_gray(40));
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
                "Radius {:.2}, Height {:.2}, Step {:.2}, Slope {:.0}° — {} polygons in {} tiles, \
                 {:.1} m²",
                a.radius,
                a.height,
                a.step_height,
                a.max_slope,
                m.poly_count(),
                m.tiles().len(),
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
        ui.label("Tile Size");
        let mut tiles = if s.tile_size == 0 {
            navigation::DEFAULT_TILE_SIZE
        } else {
            s.tile_size
        };
        if ui
            .add(
                egui::DragValue::new(&mut tiles)
                    .range(16..=1024)
                    .suffix(" voxels"),
            )
            .changed()
        {
            s.tile_size = if tiles == navigation::DEFAULT_TILE_SIZE {
                0
            } else {
                tiles
            };
            ch = true;
        }
        ui.end_row();
        ch
    });
    if changed {
        commit(state, s);
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
        let surfaces = surface_entities(state);
        if ui
            .add_enabled(
                !busy && !surfaces.is_empty(),
                egui::Button::new(format!("Bake All Surfaces ({})", surfaces.len())),
            )
            .clicked()
        {
            for e in surfaces {
                start_surface_bake(state, e);
            }
        }
    });
    if let Some(job) = &state.navigation.job {
        let (f, stage) = job.progress();
        ui.add(egui::ProgressBar::new(f).text(format!("Baking: {stage}…")));
    } else if !state.navigation.status.is_empty() {
        ui.weak(&state.navigation.status);
    }
    for job in &state.navigation.surface_jobs {
        let (f, stage) = job.progress();
        let name = job
            .surface
            .filter(|e| state.scene.contains(*e))
            .map_or_else(String::new, |e| state.scene.name(e));
        ui.add(egui::ProgressBar::new(f).text(format!("{name}: {stage}…")));
    }
    ui.add_space(6.0);
    ui.weak(
        "Bake: objects marked Navigation Static (see the Object tab) with a Mesh Renderer or \
         Terrain, for the Humanoid. NavMesh Surfaces bake their own objects for any agent type.",
    );
}

fn surface_entities(state: &EditorState) -> Vec<Entity> {
    state
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| {
            state
                .scene
                .world
                .get::<&NavMeshSurface>(*e)
                .is_ok_and(|s| s.enabled)
        })
        .collect()
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
    ui.weak(
        "Same as the Static flag in the Properties panel header. Add a NavMesh Modifier to give an \
             object an area or leave it out of bakes.",
    );
}

/// Run a bake on the background pool.
fn spawn_bake(
    input: geometry::BuildInput,
    settings: navigation::navmesh::BuildSettings,
    previous: Option<Arc<NavMesh>>,
    target: String,
    surface: Option<Entity>,
) -> BakeJob {
    let objects = input.geometry.objects;
    let progress = Arc::new(Mutex::new((0.0, "Collecting geometry")));
    let result = Arc::new(Mutex::new(None));
    let (p, r, prev) = (progress.clone(), result.clone(), previous.clone());
    misty_engine::background::pool().spawn(move || {
        let mesh = navigation::bake::bake(input, settings, prev.as_deref(), &|f, stage| {
            *p.lock().unwrap_or_else(|e| e.into_inner()) = (f, stage);
        });
        *r.lock().unwrap_or_else(|e| e.into_inner()) = Some(mesh);
    });
    BakeJob {
        result,
        progress,
        target,
        objects,
        started: Instant::now(),
        surface,
        previous,
    }
}

/// Start baking the open scene's Navigation Static objects in the background.
pub fn start_bake(state: &mut EditorState) {
    if state.navigation.job.is_some() {
        return;
    }
    let Some(scene_path) = state.scene.active_path() else {
        state.navigation.status = "Save the scene before baking.".into();
        return;
    };
    let input = geometry::collect_static(&state.scene);
    let settings = navigation::bake::scene_settings(&input, &state.scene.navigation);
    let target = NavigationSettings::asset_path_for(&scene_path);
    let previous = navigation::store::load(&state.project.root.join(&target)).ok();
    state.navigation.job = Some(spawn_bake(input, settings, previous, target, None));
}

/// Start baking the NavMesh Surface on `e` in the background.
pub fn start_surface_bake(state: &mut EditorState, e: Entity) {
    if state
        .navigation
        .surface_jobs
        .iter()
        .any(|j| j.surface == Some(e))
    {
        return;
    }
    let s = match state.scene.world.get::<&NavMeshSurface>(e) {
        Ok(s) => (*s).clone(),
        Err(_) => return,
    };
    let Some(scene_path) = state.scene.active_path() else {
        state
            .navigation
            .surface_status
            .insert(e, "Save the scene before baking.".into());
        return;
    };
    let input = geometry::collect_surface(&state.scene, e, &s);
    let settings = navigation::bake::surface_settings(&input, &state.scene.navigation, &s);
    let target = s.nav_mesh_data.clone().unwrap_or_else(|| {
        NavigationSettings::surface_asset_path_for(&scene_path, &state.scene.name(e))
    });
    let previous = navigation::store::load(&state.project.root.join(&target)).ok();
    let job = spawn_bake(input, settings, previous, target, Some(e));
    state.navigation.surface_jobs.push(job);
}

/// `MISTY_NAV_VIEW=x,y,z,pitch,yaw` puts the Scene view camera there once, a few frames in
/// (after the scene has loaded; screenshots).
fn apply_env_view(state: &mut EditorState) {
    static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) != 5 {
        return;
    }
    let Ok(v) = std::env::var("MISTY_NAV_VIEW") else {
        return;
    };
    let n: Vec<f32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    if let [x, y, z, pitch, yaw] = n[..] {
        let t =
            misty_engine::scene::Transform::default().with_euler(glam::Vec3::new(pitch, yaw, 0.0));
        state
            .scene_view
            .camera
            .align_to(glam::Vec3::new(x, y, z), t.rotation);
    }
}

/// Pick up finished bakes: save them and point the scene (or surface) at them.
pub fn poll(state: &mut EditorState) {
    apply_env_view(state);
    if state.navigation.job.as_ref().is_some_and(BakeJob::finished) {
        let job = state.navigation.job.take().expect("checked");
        finish(state, job);
    }
    let mut i = 0;
    while i < state.navigation.surface_jobs.len() {
        if state.navigation.surface_jobs[i].finished() {
            let job = state.navigation.surface_jobs.remove(i);
            finish(state, job);
        } else {
            i += 1;
        }
    }
}

fn finish(state: &mut EditorState, job: BakeJob) {
    let secs = job.started.elapsed().as_secs_f32();
    let Some(mesh) = job.result.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let path = state.project.root.join(&job.target);
    let report = |state: &mut EditorState, msg: String| match job.surface {
        Some(e) => {
            state.navigation.surface_status.insert(e, msg);
        }
        None => state.navigation.status = msg,
    };
    if let Err(e) = mesh.save(&path) {
        report(state, format!("Could not save the nav mesh: {e}"));
        return;
    }
    let reused = job
        .previous
        .as_deref()
        .map_or(0, |p| navigation::bake::reused_tiles(p, &mesh));
    let msg = if mesh.is_empty() {
        "Nothing walkable: check the collected objects and bake again.".to_owned()
    } else {
        format!(
            "Baked {} polygons in {} tiles ({} reused, {:.1} m²) from {} objects in {secs:.2} s → {}",
            mesh.poly_count(),
            mesh.tiles().len(),
            reused,
            mesh.area(),
            job.objects,
            job.target
        )
    };
    report(state, msg);
    navigation::store::insert(&path, Arc::new(mesh));
    match job.surface {
        None => {
            if state.scene.navigation.nav_mesh.as_deref() != Some(job.target.as_str()) {
                state.record("Bake NavMesh");
                state.scene.navigation.nav_mesh = Some(job.target);
            }
        }
        Some(e) => {
            let current = state
                .scene
                .world
                .get::<&NavMeshSurface>(e)
                .ok()
                .map(|s| s.nav_mesh_data.clone());
            if let Some(current) = current
                && current.as_deref() != Some(job.target.as_str())
            {
                state.record("Bake NavMesh Surface");
                if let Ok(mut s) = state.scene.world.get::<&mut NavMeshSurface>(e) {
                    s.nav_mesh_data = Some(job.target);
                }
            }
        }
    }
    state.refresh_assets();
}

/// Wait for running bakes (tests).
#[cfg(test)]
pub fn finish_bake(state: &mut EditorState) {
    while state.navigation.baking() {
        poll(state);
        std::thread::yield_now();
    }
}

/// Remove `rel` (to the Recycle Bin when possible).
fn remove_asset(state: &EditorState, rel: &str) -> bool {
    let path = state.project.root.join(rel);
    if path.exists() && trash::delete(&path).is_err() && std::fs::remove_file(&path).is_err() {
        return false;
    }
    navigation::store::forget(&path);
    true
}

/// Remove the baked asset and the scene's reference to it.
pub fn clear(state: &mut EditorState) {
    let Some(rel) = state.scene.navigation.nav_mesh.clone() else {
        return;
    };
    if !remove_asset(state, &rel) {
        state.navigation.status = format!("Could not remove {rel}.");
        return;
    }
    state.record("Clear NavMesh");
    state.scene.navigation.nav_mesh = None;
    state.navigation.status = "Cleared.".into();
    state.refresh_assets();
}

/// Remove a surface's baked data.
pub fn clear_surface(state: &mut EditorState, e: Entity) {
    let Some(rel) = state
        .scene
        .world
        .get::<&NavMeshSurface>(e)
        .ok()
        .and_then(|s| s.nav_mesh_data.clone())
    else {
        return;
    };
    if !remove_asset(state, &rel) {
        state
            .navigation
            .surface_status
            .insert(e, format!("Could not remove {rel}."));
        return;
    }
    state.record("Clear NavMesh Surface");
    if let Ok(mut s) = state.scene.world.get::<&mut NavMeshSurface>(e) {
        s.nav_mesh_data = None;
    }
    state.navigation.surface_status.insert(e, "Cleared.".into());
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
#[path = "navigation_tests.rs"]
mod tests;
