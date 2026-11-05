//! Light probe tools: GameObject ▸ Light ▸ Light Probe Group, Scene-view probe editing (Inspector
//! ▸ Edit Light Probes), the gizmos (a sphere per probe, shaded by its baked light once baked;
//! the tetrahedra while editing) and Lighting ▸ Bake Light Probes, which captures one probe at a
//! time over several frames (progress bar, Cancel) and saves
//! `Assets/Scenes/<Scene>/LightProbes.probes`, referenced by the scene's Lighting settings.
//!
//! While editing: click a probe to select it (Shift/Ctrl add), drag probes or the Move handle,
//! Delete removes the selected probes, Ctrl+D duplicates them in place, clicks on empty space
//! clear the probe selection (the group stays selected). Every action is one undo step.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Key, Pos2, Rect, Stroke, Ui, vec2};
use glam::{Mat4, Quat, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::light_probes::bake::{BakeSettings, Baker};
use misty_engine::light_probes::{self, LightProbeField, LightProbeGroup, store};
use misty_engine::render::{MaterialParams, MeshDraw};
use misty_engine::scene::{PrimitiveMesh, Scene, Transform};

use crate::gizmos::Lines;
use crate::handles::{arrow_head, closest_on_axis, dist_to_segment, ray_plane};
use crate::picking::{ray, to_screen};
use crate::state::{EditorState, Tool};
use crate::theme;

const PROBE_PX: f32 = 7.0;
const AXIS_PX: f32 = 80.0;
const SPHERE_RADIUS: f32 = 0.12;
const PROBE: Color32 = Color32::from_rgb(255, 214, 64);
const SELECTED: Color32 = Color32::from_rgb(90, 180, 255);
const TETRA: [f32; 4] = [1.0, 0.85, 0.3, 0.45];
/// Frame time the bake may use per editor frame.
const BAKE_BUDGET: Duration = Duration::from_millis(30);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Probe(usize),
    Axis(usize),
    Free,
}

#[derive(Clone, Debug)]
struct Drag {
    part: Part,
    /// Selected probes and their world positions at the start.
    start: Vec<(usize, Vec3)>,
    origin: Vec3,
    grab: Vec3,
}

struct BakeJob {
    baker: Baker,
    scene_path: String,
    started: Instant,
}

/// Light probe editing and baking state (`EditorState::light_probes`).
pub struct LightProbeTools {
    /// Inspector ▸ Edit Light Probes is on.
    pub editing: bool,
    /// Group being edited (the active selection when it has one).
    pub entity: Option<Entity>,
    pub selected: Vec<usize>,
    drag: Option<Drag>,
    /// The pointer is over the Scene view (Delete / Ctrl+D act on probes, not objects).
    pub view_hovered: bool,
    pub settings: BakeSettings,
    /// Inspector ▸ Fill Box: size (local) and spacing.
    pub fill_size: Vec3,
    pub fill_spacing: f32,
    pub status: String,
    job: Option<BakeJob>,
    /// Tetrahedra edges of the last probe layout drawn.
    edges: std::sync::Mutex<Option<EdgeCache>>,
}

type EdgeCache = (Vec<Vec3>, Vec<(Vec3, Vec3)>);

impl Default for LightProbeTools {
    fn default() -> Self {
        Self {
            editing: false,
            entity: None,
            selected: Vec::new(),
            drag: None,
            view_hovered: false,
            settings: BakeSettings::default(),
            fill_size: Vec3::new(8.0, 3.0, 8.0),
            fill_spacing: 2.0,
            status: String::new(),
            job: None,
            edges: Default::default(),
        }
    }
}

impl LightProbeTools {
    /// Delete and Ctrl+D belong to the probe selection.
    pub fn wants_keys(&self) -> bool {
        self.editing && !self.selected.is_empty() && self.view_hovered
    }

    pub fn baking(&self) -> bool {
        self.job.is_some()
    }

    pub fn progress(&self) -> Option<(f32, String)> {
        self.job.as_ref().map(|j| j.baker.progress())
    }

    pub fn set_editing(&mut self, on: bool) {
        self.editing = on;
        self.selected.clear();
        self.drag = None;
    }
}

fn group(scene: &Scene, e: Entity) -> Option<LightProbeGroup> {
    scene
        .world
        .get::<&LightProbeGroup>(e)
        .ok()
        .map(|g| (*g).clone())
}

fn store_group(scene: &mut Scene, e: Entity, g: LightProbeGroup) {
    if let Ok(mut slot) = scene.world.get::<&mut LightProbeGroup>(e) {
        *slot = g;
    }
}

/// GameObject ▸ Light ▸ Light Probe Group: at the Scene view pivot (or under `parent`).
pub fn create_group(state: &mut EditorState, parent: Option<Entity>) {
    let parent = parent.filter(|p| state.scene.contains(*p));
    state.record("Create Light Probe Group");
    let e = state.scene.spawn("Light Probe Group", parent);
    let mut t = Transform::default();
    if parent.is_none() {
        t.position = state.scene_view.camera.pivot().round();
    }
    state.scene.set_transform(e, t);
    state.scene.add(e, LightProbeGroup::default());
    state.select(e);
    state.console.info("Created 'Light Probe Group'.");
}

/// Inspector ▸ Add Probe: one probe at the Scene view's centre (its pivot), selected.
pub fn add_probe_at_view_center(state: &mut EditorState, e: Entity) {
    let Some(mut g) = group(&state.scene, e) else {
        return;
    };
    let world = state.scene_view.camera.pivot();
    let local = state
        .scene
        .world_matrix(e)
        .inverse()
        .transform_point3(world);
    state.record("Add Light Probe");
    g.probes.push(local);
    state.light_probes.selected = vec![g.probes.len() - 1];
    store_group(&mut state.scene, e, g);
}

/// Remove the selected probes of group `e`.
pub fn delete_selected(state: &mut EditorState, e: Entity) {
    let Some(mut g) = group(&state.scene, e) else {
        return;
    };
    let mut sel = std::mem::take(&mut state.light_probes.selected);
    if sel.is_empty() {
        return;
    }
    state.record(if sel.len() == 1 {
        "Delete Light Probe"
    } else {
        "Delete Light Probes"
    });
    sel.sort_unstable();
    sel.dedup();
    for i in sel.into_iter().rev() {
        if i < g.probes.len() {
            g.probes.remove(i);
        }
    }
    store_group(&mut state.scene, e, g);
}

/// Copy the selected probes in place and select the copies (drag them away next).
pub fn duplicate_selected(state: &mut EditorState, e: Entity) {
    let Some(mut g) = group(&state.scene, e) else {
        return;
    };
    let sel: Vec<usize> = state
        .light_probes
        .selected
        .iter()
        .copied()
        .filter(|i| *i < g.probes.len())
        .collect();
    if sel.is_empty() {
        return;
    }
    state.record(if sel.len() == 1 {
        "Duplicate Light Probe"
    } else {
        "Duplicate Light Probes"
    });
    let first = g.probes.len();
    for i in &sel {
        g.probes.push(g.probes[*i]);
    }
    state.light_probes.selected = (first..g.probes.len()).collect();
    store_group(&mut state.scene, e, g);
}

/// Inspector ▸ Fill Box: replace the probes with a grid over the box (centred on the object).
pub fn fill_box(state: &mut EditorState, e: Entity) {
    let Some(mut g) = group(&state.scene, e) else {
        return;
    };
    state.record("Fill Light Probe Box");
    let (size, spacing) = (
        state.light_probes.fill_size,
        state.light_probes.fill_spacing,
    );
    g.fill_box(Vec3::ZERO, size, spacing);
    state.light_probes.selected.clear();
    store_group(&mut state.scene, e, g);
}

/// Probe spheres for the Scene view: shaded by the baked probes once the scene has them (an
/// unlit sphere flagged for light probes shows its SH), plain yellow before.
pub fn spheres(state: &EditorState, meshes: &mut Vec<MeshDraw>) {
    let groups = light_probes::scene_groups(&state.scene);
    if groups.is_empty() {
        return;
    }
    let baked = light_probes::scene_field(&state.scene).is_some();
    let material = MaterialParams {
        color: if baked {
            [1.0; 4]
        } else {
            [1.0, 0.84, 0.25, 1.0]
        },
        unlit: true,
        light_probes: baked,
        ..Default::default()
    };
    for (e, g) in groups {
        let m = state.scene.world_matrix(e);
        for p in &g.probes {
            meshes.push(MeshDraw {
                mesh: PrimitiveMesh::Sphere.into(),
                model: Mat4::from_scale_rotation_translation(
                    Vec3::splat(SPHERE_RADIUS * 2.0),
                    Quat::IDENTITY,
                    m.transform_point3(*p),
                ),
                material: material.clone(),
                cast_shadows: false,
                receive_shadows: false,
                outline: false,
                sorting: Default::default(),
                lod: None,
            });
        }
    }
}

/// Unique edges of the tetrahedralization of `points`.
pub fn tetra_edges(points: &[Vec3]) -> Vec<(Vec3, Vec3)> {
    let t = light_probes::delaunay::tetrahedralize(points);
    let mut edges = std::collections::BTreeSet::new();
    for v in &t.tets {
        for a in 0..4 {
            for b in a + 1..4 {
                edges.insert((v[a].min(v[b]), v[a].max(v[b])));
            }
        }
    }
    edges
        .into_iter()
        .map(|(a, b)| (points[a as usize], points[b as usize]))
        .collect()
}

/// The tetrahedra of all probes while a group is being edited.
pub fn lines(state: &EditorState, lines: &mut Lines) {
    let editing = state.light_probes.editing
        && state
            .selection
            .active()
            .is_some_and(|e| state.scene.world.get::<&LightProbeGroup>(e).is_ok());
    if !editing {
        return;
    }
    let points = light_probes::scene_positions(&state.scene);
    let mut cache = state
        .light_probes
        .edges
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if cache.as_ref().is_none_or(|(p, _)| *p != points) {
        let edges = tetra_edges(&points);
        *cache = Some((points, edges));
    }
    if let Some((_, edges)) = cache.as_ref() {
        for (a, b) in edges {
            lines.line(*a, *b, TETRA);
        }
    }
}

/// Edit the selected group's probes. True while they own the mouse.
pub fn handles(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    hovered: bool,
) -> bool {
    let (mouse, pressed, down, mods) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers,
        )
    });
    let in_view = mouse.is_some_and(|m| rect.contains(m)) && hovered;
    state.light_probes.view_hovered = in_view;
    let active = state
        .selection
        .active()
        .filter(|e| state.scene.world.get::<&LightProbeGroup>(*e).is_ok());
    if active != state.light_probes.entity {
        state.light_probes.entity = active;
        state.light_probes.selected.clear();
        state.light_probes.drag = None;
        if active.is_none() {
            state.light_probes.editing = false;
        }
    }
    let (Some(e), true) = (active, state.light_probes.editing) else {
        return false;
    };
    let Some(g) = group(&state.scene, e) else {
        return false;
    };
    // Undo can take probes away under the selection.
    state.light_probes.selected.retain(|i| *i < g.probes.len());
    if pressed {
        state.handles.suppress_click = false;
    }
    let m = state.scene.world_matrix(e);
    let tool = state.tool;
    let points: Vec<(usize, Pos2, Vec3)> = g
        .probes
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let w = m.transform_point3(*p);
            Some((i, to_screen(camera, rect, w)?, w))
        })
        .collect();
    let sel = state.light_probes.selected.clone();
    let center = (!sel.is_empty()).then(|| {
        sel.iter()
            .map(|i| m.transform_point3(g.probes[*i]))
            .sum::<Vec3>()
            / sel.len() as f32
    });
    let moves = matches!(tool, Tool::Move | Tool::Transform);
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let length = center.map_or(1.0, |c| camera.world_per_pixel(c, rect.height()) * AXIS_PX);

    let mut hot = None;
    if let Some(at) =
        mouse.filter(|_| in_view && state.light_probes.drag.is_none() && tool != Tool::Hand)
    {
        hot = points
            .iter()
            .filter(|p| p.1.distance(at) < PROBE_PX)
            .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
            .map(|p| Part::Probe(p.0));
        if hot.is_none()
            && moves
            && let Some(c) = center
            && let Some(cs) = to_screen(camera, rect, c)
        {
            if Rect::from_center_size(cs, vec2(14.0, 14.0)).contains(at) {
                hot = Some(Part::Free);
            } else {
                let mut best = 7.0;
                for (i, axis) in axes.iter().enumerate() {
                    if let Some(tip) = to_screen(camera, rect, c + *axis * length) {
                        let d = dist_to_segment(at, cs, tip);
                        if d < best {
                            best = d;
                            hot = Some(Part::Axis(i));
                        }
                    }
                }
            }
        }
    }

    if pressed && in_view && tool != Tool::Hand {
        match hot {
            Some(part) => {
                if let Part::Probe(i) = part {
                    let sel = &mut state.light_probes.selected;
                    if mods.shift || mods.command {
                        match sel.iter().position(|s| *s == i) {
                            Some(k) => {
                                sel.remove(k);
                            }
                            None => sel.push(i),
                        }
                    } else if !sel.contains(&i) {
                        *sel = vec![i];
                    }
                }
                let at = mouse.unwrap_or_default();
                let (o, d) = ray(camera, rect, at);
                let start: Vec<(usize, Vec3)> = state
                    .light_probes
                    .selected
                    .iter()
                    .map(|i| (*i, m.transform_point3(g.probes[*i])))
                    .collect();
                let origin = match part {
                    Part::Probe(i) => m.transform_point3(g.probes[i]),
                    _ => center.unwrap_or_default(),
                };
                let grab = match part {
                    Part::Axis(i) => closest_on_axis(origin, axes[i], o, d)
                        .map_or(origin, |t| origin + axes[i] * t),
                    _ => ray_plane(o, d, origin, camera.forward()).unwrap_or(origin),
                };
                state.light_probes.drag = Some(Drag {
                    part,
                    start,
                    origin,
                    grab,
                });
            }
            None => {
                if !mods.shift && !mods.command {
                    state.light_probes.selected.clear();
                }
            }
        }
        // The group stays selected while editing.
        state.handles.suppress_click = true;
    }

    if let Some(drag) = state.light_probes.drag.clone() {
        if down {
            if let Some(at) = mouse {
                apply_drag(state, e, &drag, camera, rect, at, m);
                ui.ctx().request_repaint();
            }
        } else {
            state.light_probes.drag = None;
        }
    }

    if in_view && !state.light_probes.selected.is_empty() {
        let (delete, duplicate) = ui.input(|i| {
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.events.iter().any(|e| {
                    matches!(e, egui::Event::Key { key: Key::D, pressed: true, modifiers, .. }
                        if modifiers.command)
                }),
            )
        });
        if delete {
            delete_selected(state, e);
        } else if duplicate {
            duplicate_selected(state, e);
        }
    }

    // Paint the probe selection and the move handle.
    let g = group(&state.scene, e).unwrap_or(g);
    let m = state.scene.world_matrix(e);
    let painter = ui.painter().with_clip_rect(rect);
    let active_part = state.light_probes.drag.as_ref().map(|d| d.part).or(hot);
    for (i, p) in g.probes.iter().enumerate() {
        let Some(at) = to_screen(camera, rect, m.transform_point3(*p)) else {
            continue;
        };
        let selected = state.light_probes.selected.contains(&i);
        let hover = active_part == Some(Part::Probe(i));
        let col = if selected { SELECTED } else { PROBE };
        let r = if hover { 6.0 } else { 4.5 };
        painter.circle_stroke(at, r, Stroke::new(if selected { 2.0 } else { 1.0 }, col));
    }
    let sel = &state.light_probes.selected;
    if moves
        && !sel.is_empty()
        && let Some(c) = Some(
            sel.iter()
                .filter_map(|i| g.probes.get(*i))
                .map(|p| m.transform_point3(*p))
                .sum::<Vec3>()
                / sel.len() as f32,
        )
        && let Some(cs) = to_screen(camera, rect, c)
    {
        let length = camera.world_per_pixel(c, rect.height()) * AXIS_PX;
        let pick = |p: Part, base: Color32| {
            if active_part == Some(p) {
                Color32::from_rgb(255, 230, 0)
            } else {
                base
            }
        };
        for (i, axis) in axes.iter().enumerate() {
            if let Some(tip) = to_screen(camera, rect, c + *axis * length) {
                let col = pick(
                    Part::Axis(i),
                    [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][i],
                );
                painter.line_segment([cs, tip], Stroke::new(2.5, col));
                arrow_head(&painter, cs, tip, col);
            }
        }
        painter.rect_stroke(
            Rect::from_center_size(cs, vec2(12.0, 12.0)),
            1.0,
            Stroke::new(1.5, pick(Part::Free, Color32::from_gray(220))),
            egui::StrokeKind::Middle,
        );
    }
    painter.text(
        rect.left_bottom() + vec2(10.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        "Editing Light Probes: click to select (Shift/Ctrl add), drag to move · Delete removes · Ctrl+D duplicates",
        egui::FontId::proportional(12.0),
        Color32::WHITE,
    );
    in_view
}

fn apply_drag(
    state: &mut EditorState,
    e: Entity,
    drag: &Drag,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
    m: Mat4,
) {
    let (o, d) = ray(camera, rect, at);
    let delta = match drag.part {
        Part::Axis(i) => {
            let axis = [Vec3::X, Vec3::Y, Vec3::Z][i];
            let Some(t) = closest_on_axis(drag.origin, axis, o, d) else {
                return;
            };
            axis * (t - (drag.grab - drag.origin).dot(axis))
        }
        _ => {
            let Some(p) = ray_plane(o, d, drag.origin, camera.forward()) else {
                return;
            };
            p - drag.grab
        }
    };
    let Some(mut g) = group(&state.scene, e) else {
        return;
    };
    let inv = m.inverse();
    for (i, start) in &drag.start {
        if let Some(p) = g.probes.get_mut(*i) {
            *p = inv.transform_point3(*start + delta);
        }
    }
    store_group(&mut state.scene, e, g);
    state.mark_edited(if drag.start.len() == 1 {
        "Move Light Probe"
    } else {
        "Move Light Probes"
    });
}

/// Lighting ▸ Bake Light Probes: start a bake of every probe of the saved scene.
pub fn start_bake(state: &mut EditorState) {
    if state.light_probes.job.is_some() {
        return;
    }
    let Some(scene_path) = state.scene.active_path() else {
        state.light_probes.status = "Save the scene before baking light probes.".into();
        return;
    };
    let positions = light_probes::scene_positions(&state.scene);
    match Baker::new(
        &state.scene,
        &state.assets,
        positions,
        state.light_probes.settings,
    ) {
        Some(baker) => {
            state.light_probes.job = Some(BakeJob {
                baker,
                scene_path,
                started: Instant::now(),
            });
            state.light_probes.status.clear();
        }
        None => {
            state.light_probes.status =
                "No light probes: add a Light Probe Group (GameObject ▸ Light).".into()
        }
    }
}

pub fn cancel_bake(state: &mut EditorState) {
    if state.light_probes.job.take().is_some() {
        state.light_probes.status = "Light probe bake cancelled.".into();
    }
}

/// Per-frame GPU work: bake steps within the frame budget. True while work is left.
pub fn gpu_frame(gpu: &mut crate::viewport::Gpu, state: &mut EditorState) -> bool {
    let Some(mut job) = state.light_probes.job.take() else {
        return false;
    };
    if state.play.in_play_mode() || state.scene.active_path().as_deref() != Some(&job.scene_path) {
        state.light_probes.status = "Light probe bake cancelled: the scene changed.".into();
        return false;
    }
    let (device, queue) = (&gpu.state.device, &gpu.state.queue);
    let start = Instant::now();
    let mut done = false;
    while !done && start.elapsed() < BAKE_BUDGET {
        done = job.baker.step(&mut gpu.renderer, device, queue);
    }
    if done {
        finish_bake(state, job);
        return false;
    }
    state.light_probes.job = Some(job);
    true
}

/// Save the file next to the scene and point the scene's Lighting settings at it.
fn finish_bake(state: &mut EditorState, job: BakeJob) {
    let count = job.baker.probe_count();
    let data = job.baker.finish();
    let rel = light_probes::asset_path_for(&job.scene_path);
    let path = state.project.root.join(&rel);
    if let Err(err) = data.save(&path) {
        state.light_probes.status = format!("Could not save {rel}: {err}");
        return;
    }
    store::insert(&path, Arc::new(LightProbeField::from_data(&data)));
    if state.scene.render_settings.light_probes.as_deref() != Some(rel.as_str()) {
        state.record("Bake Light Probes");
        state.scene.render_settings.light_probes = Some(rel);
    }
    state.light_probes.status = format!(
        "Baked {count} light probes in {:.1} s.",
        job.started.elapsed().as_secs_f32()
    );
    state.console.info(state.light_probes.status.clone());
    state.refresh_assets();
}

/// Lighting ▸ Clear Baked Light Probes: forget the scene's file.
pub fn clear(state: &mut EditorState) {
    let Some(rel) = state.scene.render_settings.light_probes.clone() else {
        return;
    };
    state.record("Clear Baked Light Probes");
    state.scene.render_settings.light_probes = None;
    let path = state.project.root.join(&rel);
    let _ = std::fs::remove_file(&path);
    store::forget(&path);
    state.light_probes.status = "Cleared baked light probes.".into();
    state.refresh_assets();
}

/// Lighting window ▸ Scene: the Light Probes section.
pub fn lighting_section(ui: &mut Ui, state: &mut EditorState) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Light Probes").strong());
    let probes = light_probes::scene_positions(&state.scene).len();
    egui::Grid::new("lighting_light_probes")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(150.0)
        .show(ui, |ui| {
            ui.label("Probes");
            ui.label(probes.to_string());
            ui.end_row();
            ui.label("Bounces");
            let s = &mut state.light_probes.settings;
            egui::ComboBox::from_id_salt("light_probe_bounces")
                .selected_text(s.bounces.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.bounces, 1, "1");
                    ui.selectable_value(&mut s.bounces, 2, "2");
                });
            ui.end_row();
            ui.label("Baked Data");
            match &state.scene.render_settings.light_probes {
                Some(rel) => ui.weak(rel.rsplit('/').next().unwrap_or(rel)),
                None => ui.weak("None"),
            };
            ui.end_row();
        });
    let busy = state.light_probes.baking() || state.play.in_play_mode();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Bake Light Probes"))
            .on_hover_text("Capture the static scene at every light probe")
            .clicked()
        {
            start_bake(state);
        }
        let has = state.scene.render_settings.light_probes.is_some();
        if ui
            .add_enabled(!busy && has, egui::Button::new("Clear Baked Light Probes"))
            .clicked()
        {
            clear(state);
        }
    });
    if let Some((f, label)) = state.light_probes.progress() {
        ui.horizontal(|ui| {
            ui.add(egui::ProgressBar::new(f).text(label).desired_width(220.0));
            if ui.button("Cancel").clicked() {
                cancel_bake(state);
            }
        });
        ui.ctx().request_repaint();
    } else if !state.light_probes.status.is_empty() {
        ui.weak(&state.light_probes.status);
    }
}

#[cfg(test)]
mod tests;
