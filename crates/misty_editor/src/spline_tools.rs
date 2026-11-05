//! Scene-view editing of a selected Spline Container: the curve, knots and the tangent handles of
//! selected Bézier knots. Click a knot to select it (Shift/Ctrl add), drag knots or tangents,
//! move or rotate selected knots with the Move / Rotate tool handle, Ctrl+click the curve to
//! insert a knot, Delete removes the selected knots. The Draw Spline tool places knots on
//! surfaces (colliders, terrain, else the ground plane): click for an Auto Smooth knot, drag for
//! a Bézier knot with tangents, click the first knot to close the loop; Enter or Esc finishes.
//! Every action is one undo step.

use egui::{Color32, Key, Pos2, Rect, Stroke, Ui, vec2};
use glam::{Mat4, Quat, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::Scene;
use misty_engine::splines::{Knot, Spline, SplineContainer, TangentMode};

use crate::gizmos::Lines;
use crate::handles::{arrow_head, closest_on_axis, dist_to_segment, ray_plane};
use crate::picking::{ray, to_screen};
use crate::selection::Selection;
use crate::state::{EditorState, Tool};
use crate::theme;

const KNOT_PX: f32 = 7.0;
const CURVE_PX: f32 = 6.0;
const AXIS_PX: f32 = 80.0;
/// Pointer travel that turns a Draw Spline click into a tangent drag.
const DRAG_PX: f32 = 4.0;
const CURVE: [f32; 4] = [0.35, 0.78, 1.0, 1.0];
const CURVE_DIM: [f32; 4] = [0.3, 0.6, 0.85, 0.55];
const KNOT: Color32 = Color32::from_rgb(90, 180, 255);
const SELECTED: Color32 = Color32::from_rgb(255, 214, 64);
const TANGENT: Color32 = Color32::from_rgb(200, 120, 255);

/// A knot of the edited container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnotRef {
    pub spline: usize,
    pub knot: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Knot(KnotRef),
    /// Tangent handle (true = out).
    Tangent(KnotRef, bool),
    Axis(usize),
    Free,
    Ring(usize),
}

#[derive(Clone, Debug)]
struct Drag {
    part: Part,
    /// Selected knots: world position and world rotation at the start.
    start: Vec<(KnotRef, Vec3, Quat)>,
    origin: Vec3,
    axes: [Vec3; 3],
    /// Grab point (axis / plane point, ring vector).
    grab: Vec3,
}

/// Knot placed by the Draw Spline tool while its press is held.
#[derive(Clone, Copy, Debug)]
struct Placing {
    knot: KnotRef,
    press: Pos2,
    point: Vec3,
    normal: Vec3,
}

/// Spline editing state of the Scene view (`EditorState::splines`).
#[derive(Default)]
pub struct SplineEdit {
    /// Container being edited (the active selection when it has one).
    pub entity: Option<Entity>,
    pub selected: Vec<KnotRef>,
    /// The Draw Spline tool is on.
    pub drawing: bool,
    /// Spline the Draw Spline tool is adding to.
    draw_spline: Option<usize>,
    drag: Option<Drag>,
    placing: Option<Placing>,
    /// The pointer is over the Scene view (Delete removes knots, not objects).
    pub view_hovered: bool,
}

impl SplineEdit {
    /// Delete belongs to the Scene view's knot selection.
    pub fn wants_delete(&self) -> bool {
        !self.selected.is_empty() && self.view_hovered
    }

    /// Turn the Draw Spline tool on or off.
    pub fn set_drawing(&mut self, on: bool) {
        self.drawing = on;
        self.draw_spline = None;
        self.placing = None;
    }
}

fn container(scene: &Scene, e: Entity) -> Option<SplineContainer> {
    scene
        .world
        .get::<&SplineContainer>(e)
        .ok()
        .map(|c| (*c).clone())
}

fn store(scene: &mut Scene, e: Entity, c: SplineContainer) {
    if let Ok(mut slot) = scene.world.get::<&mut SplineContainer>(e) {
        *slot = c;
    }
}

/// Curve lines of every Spline Container (selected ones brighter), for the gizmo pass.
pub fn lines(scene: &Scene, selection: &Selection, lines: &mut Lines) {
    let owners: Vec<Entity> = scene
        .world
        .query::<(Entity, &SplineContainer)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in owners {
        if !scene.active_in_hierarchy(e) {
            continue;
        }
        let Some(c) = container(scene, e) else {
            continue;
        };
        let m = scene.world_matrix(e);
        let color = if selection.contains(e) {
            CURVE
        } else {
            CURVE_DIM
        };
        for s in &c.splines {
            let pts = s.path().polyline(24);
            for w in pts.windows(2) {
                lines.line(m.transform_point3(w[0]), m.transform_point3(w[1]), color);
            }
        }
    }
}

/// Screen data of the edited container for one frame.
struct View {
    m: Mat4,
    knots: Vec<(KnotRef, Pos2, Vec3)>,
    tangents: Vec<(KnotRef, bool, Pos2, Pos2)>,
}

fn view(c: &SplineContainer, m: Mat4, selected: &[KnotRef], camera: &Camera, rect: Rect) -> View {
    let mut v = View {
        m,
        knots: Vec::new(),
        tangents: Vec::new(),
    };
    for (si, s) in c.splines.iter().enumerate() {
        for (ki, k) in s.knots.iter().enumerate() {
            let r = KnotRef {
                spline: si,
                knot: ki,
            };
            let world = m.transform_point3(k.position);
            let Some(at) = to_screen(camera, rect, world) else {
                continue;
            };
            v.knots.push((r, at, world));
            if selected.contains(&r) && k.mode.is_bezier() {
                let (tin, tout) = s.tangents(ki);
                for (out, t) in [(false, tin), (true, tout)] {
                    if let Some(end) = to_screen(camera, rect, m.transform_point3(k.position + t)) {
                        v.tangents.push((r, out, at, end));
                    }
                }
            }
        }
    }
    v
}

/// Closest curve point under the cursor: (spline, curve, u).
fn curve_hit(
    c: &SplineContainer,
    m: Mat4,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
) -> Option<(usize, usize, f32)> {
    let mut best: Option<(usize, usize, f32, f32)> = None;
    for (si, s) in c.splines.iter().enumerate() {
        for ci in 0..s.curve_count() {
            let b = s.curve(ci);
            let steps = 32;
            let pts: Vec<Option<Pos2>> = (0..=steps)
                .map(|k| {
                    to_screen(
                        camera,
                        rect,
                        m.transform_point3(b.position(k as f32 / steps as f32)),
                    )
                })
                .collect();
            for k in 0..steps {
                let (Some(a), Some(z)) = (pts[k], pts[k + 1]) else {
                    continue;
                };
                let d = dist_to_segment(at, a, z);
                if d < CURVE_PX && best.is_none_or(|b| d < b.3) {
                    let ab = z - a;
                    let f = ((at - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    best = Some((si, ci, (k as f32 + f) / steps as f32, d));
                }
            }
        }
    }
    best.map(|(s, c, u, _)| (s, c, u))
}

/// Handle frame of the selected knots: origin and axes (world).
fn knot_frame(
    c: &SplineContainer,
    m: Mat4,
    selected: &[KnotRef],
    global: bool,
) -> Option<(Vec3, [Vec3; 3])> {
    let pts: Vec<Vec3> = selected
        .iter()
        .filter_map(|r| c.splines.get(r.spline)?.knots.get(r.knot))
        .map(|k| m.transform_point3(k.position))
        .collect();
    if pts.is_empty() {
        return None;
    }
    let origin = pts.iter().sum::<Vec3>() / pts.len() as f32;
    let rot = if global {
        Quat::IDENTITY
    } else {
        let (_, mr, _) = m.to_scale_rotation_translation();
        let r = selected[0];
        mr * c.splines[r.spline].knots[r.knot].rotation
    };
    Some((origin, [rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z]))
}

fn axis_color(i: usize) -> Color32 {
    [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][i]
}

fn ring(
    camera: &Camera,
    rect: Rect,
    origin: Vec3,
    axes: &[Vec3; 3],
    i: usize,
    radius: f32,
) -> Vec<Pos2> {
    let (u, v) = (axes[(i + 1) % 3], axes[(i + 2) % 3]);
    (0..=48)
        .filter_map(|k| {
            let a = k as f32 / 48.0 * std::f32::consts::TAU;
            to_screen(camera, rect, origin + (u * a.cos() + v * a.sin()) * radius)
        })
        .collect()
}

/// Surface point for the Draw Spline tool: colliders, then terrain, then the ground plane.
fn surface(
    scene: &Scene,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
    skip: Option<Entity>,
) -> (Vec3, Vec3) {
    let (o, d) = ray(camera, rect, at);
    let physics = misty_engine::physics::Physics::build(scene, Vec3::ZERO);
    let mine = |e: Entity| skip.is_some_and(|s| scene.is_ancestor(s, e));
    if let Some(hit) = physics
        .raycast_all(o, d, 10_000.0, misty_engine::physics::ALL_LAYERS)
        .into_iter()
        .find(|h| !mine(h.entity))
    {
        return (hit.point, hit.normal);
    }
    if let Some((_, p)) = misty_engine::terrain::raycast(scene, o, d) {
        return (p, Vec3::Y);
    }
    match ray_plane(o, d, Vec3::ZERO, Vec3::Y) {
        Some(p) => (p, Vec3::Y),
        None => (o + d * 10.0, -d),
    }
}

/// Draw and edit the selected container's knots. True while they own the mouse (the transform
/// handles and click-selection then stay out of the way).
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
    state.splines.view_hovered = in_view;
    let active = state
        .selection
        .active()
        .filter(|e| state.scene.world.get::<&SplineContainer>(*e).is_ok());
    if active != state.splines.entity {
        state.splines.entity = active;
        state.splines.selected.clear();
        state.splines.drag = None;
        state.splines.placing = None;
        state.splines.draw_spline = None;
    }
    if state.splines.drawing
        && ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter))
    {
        state.splines.set_drawing(false);
    }
    // Draw Spline with no container selected: the first click makes one.
    if active.is_none() {
        if state.splines.drawing && pressed && in_view && state.tool != Tool::Hand {
            state.record("Create Spline");
            let e = state.scene.spawn("Spline", None);
            state.scene.add(e, SplineContainer::default());
            state.select(e);
            state.splines.entity = Some(e);
            state.handles.suppress_click = true;
            if let Some(m) = mouse {
                place(state, e, camera, rect, m);
            }
            return true;
        }
        return state.splines.drawing && in_view;
    }
    let e = active.expect("checked above");
    let Some(mut c) = container(&state.scene, e) else {
        return false;
    };
    // Undo can take knots away under the selection.
    state.splines.selected.retain(|r| {
        c.splines
            .get(r.spline)
            .is_some_and(|s| r.knot < s.knots.len())
    });
    if pressed {
        state.handles.suppress_click = false;
    }
    let m = state.scene.world_matrix(e);
    let tool = state.tool;
    let v = view(&c, m, &state.splines.selected, camera, rect);
    let gizmo_tool = matches!(tool, Tool::Move | Tool::Transform | Tool::Rotate);
    let frame = knot_frame(&c, m, &state.splines.selected, state.global_space)
        .filter(|_| gizmo_tool && !state.splines.drawing);
    let length = frame.map_or(1.0, |(o, _)| {
        camera.world_per_pixel(o, rect.height()) * AXIS_PX
    });
    let moves = matches!(tool, Tool::Move | Tool::Transform);
    let rotates = matches!(tool, Tool::Rotate | Tool::Transform);

    // Hover: tangents, knots, then the knot handle and (with Ctrl) the curve.
    let mut hot: Option<Part> = None;
    if let Some(m_at) =
        mouse.filter(|_| in_view && state.splines.drag.is_none() && tool != Tool::Hand)
    {
        hot = v
            .tangents
            .iter()
            .filter(|t| t.3.distance(m_at) < KNOT_PX)
            .min_by(|a, b| a.3.distance(m_at).total_cmp(&b.3.distance(m_at)))
            .map(|t| Part::Tangent(t.0, t.1));
        if hot.is_none() {
            hot = v
                .knots
                .iter()
                .filter(|k| k.1.distance(m_at) < KNOT_PX)
                .min_by(|a, b| a.1.distance(m_at).total_cmp(&b.1.distance(m_at)))
                .map(|k| Part::Knot(k.0));
        }
        if hot.is_none()
            && let Some((o, axes)) = frame
            && let Some(center) = to_screen(camera, rect, o)
        {
            let mut best = (f32::INFINITY, None);
            if moves {
                if Rect::from_center_size(center, vec2(14.0, 14.0)).contains(m_at) {
                    best = (0.0, Some(Part::Free));
                }
                for (i, axis) in axes.iter().enumerate() {
                    if let Some(tip) = to_screen(camera, rect, o + *axis * length) {
                        let d = dist_to_segment(m_at, center, tip);
                        if d < 7.0 && d < best.0 {
                            best = (d, Some(Part::Axis(i)));
                        }
                    }
                }
            }
            if rotates && best.1.is_none() {
                let r = if tool == Tool::Transform { 0.6 } else { 1.0 };
                for i in 0..3 {
                    let pts = ring(camera, rect, o, &axes, i, length * r);
                    for w in pts.windows(2) {
                        let d = dist_to_segment(m_at, w[0], w[1]);
                        if d < 6.0 && d < best.0 {
                            best = (d, Some(Part::Ring(i)));
                        }
                    }
                }
            }
            hot = best.1;
        }
    }
    let curve = mouse
        .filter(|_| {
            in_view && hot.is_none() && mods.command && !state.splines.drawing && tool != Tool::Hand
        })
        .and_then(|at| curve_hit(&c, m, camera, rect, at));

    let mut owns = hot.is_some() || curve.is_some() || state.splines.drag.is_some();
    if pressed && in_view && tool != Tool::Hand {
        match (hot, curve) {
            (Some(Part::Knot(r)), _) if state.splines.drawing => {
                // Clicking the first knot of the spline being drawn closes it.
                let first = state.splines.draw_spline == Some(r.spline) && r.knot == 0;
                if first && c.splines[r.spline].knots.len() > 2 {
                    state.record("Close Spline");
                    c.splines[r.spline].closed = true;
                    store(&mut state.scene, e, c.clone());
                    state.splines.set_drawing(false);
                }
                state.handles.suppress_click = true;
                owns = true;
            }
            (Some(part), _) => {
                if let Part::Knot(r) = part {
                    let sel = &mut state.splines.selected;
                    if mods.shift || mods.command {
                        if let Some(i) = sel.iter().position(|s| *s == r) {
                            sel.remove(i);
                        } else {
                            sel.push(r);
                        }
                    } else if !sel.contains(&r) {
                        *sel = vec![r];
                    }
                }
                let (origin, axes) = frame.unwrap_or((Vec3::ZERO, [Vec3::X, Vec3::Y, Vec3::Z]));
                let grab_at = mouse.unwrap_or_default();
                let (o, d) = ray(camera, rect, grab_at);
                let pivot = match part {
                    Part::Knot(r) | Part::Tangent(r, _) => {
                        m.transform_point3(c.splines[r.spline].knots[r.knot].position)
                    }
                    _ => origin,
                };
                let grab = match part {
                    Part::Axis(i) => closest_on_axis(origin, axes[i], o, d)
                        .map_or(origin, |t| origin + axes[i] * t),
                    Part::Ring(i) => {
                        ray_plane(o, d, origin, axes[i]).map_or(Vec3::X, |p| p - origin)
                    }
                    _ => ray_plane(o, d, pivot, camera.forward()).unwrap_or(pivot),
                };
                let (_, mr, _) = m.to_scale_rotation_translation();
                let start = state
                    .splines
                    .selected
                    .iter()
                    .map(|r| {
                        let k = &c.splines[r.spline].knots[r.knot];
                        (*r, m.transform_point3(k.position), mr * k.rotation)
                    })
                    .collect();
                state.splines.drag = Some(Drag {
                    part,
                    start,
                    origin: pivot,
                    axes,
                    grab,
                });
                state.handles.suppress_click = true;
                owns = true;
            }
            (None, Some((si, ci, u))) => {
                state.record("Insert Knot");
                let k = c.splines[si].insert_on_curve(ci, u);
                store(&mut state.scene, e, c.clone());
                state.splines.selected = vec![KnotRef {
                    spline: si,
                    knot: k,
                }];
                state.handles.suppress_click = true;
                owns = true;
            }
            (None, None) if state.splines.drawing => {
                if let Some(at) = mouse {
                    place(state, e, camera, rect, at);
                    c = container(&state.scene, e).unwrap_or(c);
                }
                state.handles.suppress_click = true;
                owns = true;
            }
            (None, None) => {
                // Empty space: drop the knot selection; the click selects objects as usual.
                if !mods.shift && !mods.command {
                    state.splines.selected.clear();
                }
            }
        }
    }

    // Draw Spline: dragging after placing a knot pulls out its tangents.
    if let Some(p) = state.splines.placing {
        if down {
            if let Some(at) = mouse.filter(|at| at.distance(p.press) > DRAG_PX) {
                let (o, d) = ray(camera, rect, at);
                if let Some(hit) = ray_plane(o, d, p.point, p.normal) {
                    let inv = m.inverse();
                    let out = inv.transform_vector3(hit - p.point);
                    if out.length() > 1e-4
                        && let Some(k) = c.knot_mut(p.knot.spline, p.knot.knot)
                    {
                        *k = Knot::bezier(k.position, out);
                        store(&mut state.scene, e, c.clone());
                        state.mark_edited("Draw Spline");
                    }
                }
            }
            owns = true;
        } else {
            state.splines.placing = None;
        }
    }

    // Drag update.
    if let Some(drag) = state.splines.drag.clone() {
        if down {
            if let Some(at) = mouse {
                apply_drag(state, e, &mut c, &drag, camera, rect, at, m);
                ui.ctx().request_repaint();
            }
            owns = true;
        } else {
            state.splines.drag = None;
        }
    }

    // Delete removes the selected knots.
    if in_view
        && !state.splines.selected.is_empty()
        && ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
    {
        let n = state.splines.selected.len();
        state.record(if n == 1 {
            "Delete Knot"
        } else {
            "Delete Knots"
        });
        let mut sel = std::mem::take(&mut state.splines.selected);
        sel.sort_by_key(|r| std::cmp::Reverse((r.spline, r.knot)));
        for r in sel {
            if let Some(s) = c.splines.get_mut(r.spline) {
                s.knots.remove(r.knot);
                if s.knots.len() < 3 {
                    s.closed = false;
                }
            }
        }
        store(&mut state.scene, e, c.clone());
    }

    // Paint (re-read: the edits above may have moved things).
    let c = container(&state.scene, e).unwrap_or(c);
    let v = view(
        &c,
        state.scene.world_matrix(e),
        &state.splines.selected,
        camera,
        rect,
    );
    let painter = ui.painter().with_clip_rect(rect);
    let active_part = state.splines.drag.as_ref().map(|d| d.part).or(hot);
    for (r, out, base, end) in &v.tangents {
        let hot_t = active_part == Some(Part::Tangent(*r, *out));
        let col = if hot_t { Color32::WHITE } else { TANGENT };
        painter.line_segment([*base, *end], Stroke::new(1.5, col));
        painter.rect_filled(Rect::from_center_size(*end, vec2(7.0, 7.0)), 1.0, col);
    }
    for (r, at, _) in &v.knots {
        let selected = state.splines.selected.contains(r);
        let hover = active_part == Some(Part::Knot(*r));
        let fill = if selected { SELECTED } else { KNOT };
        let radius = if hover { 6.0 } else { 5.0 };
        painter.circle_filled(*at, radius, fill);
        painter.circle_stroke(*at, radius, Stroke::new(1.0, Color32::BLACK));
        // The first knot of an open spline shows its direction with a ring.
        if r.knot == 0 && !c.splines[r.spline].closed {
            painter.circle_stroke(*at, radius + 3.0, Stroke::new(1.0, fill));
        }
    }
    if let Some((s, ci, u)) = curve {
        let p = c.splines[s].curve(ci).position(u);
        if let Some(at) = to_screen(camera, rect, v.m.transform_point3(p)) {
            painter.circle_stroke(at, 5.0, Stroke::new(1.5, Color32::WHITE));
            painter.text(
                at + vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                "+",
                egui::FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
    }
    if let Some((o, axes)) = knot_frame(&c, v.m, &state.splines.selected, state.global_space)
        .filter(|_| gizmo_tool && !state.splines.drawing)
        && let Some(center) = to_screen(camera, rect, o)
    {
        let length = camera.world_per_pixel(o, rect.height()) * AXIS_PX;
        let pick = |p: Part, base: Color32| {
            if active_part == Some(p) {
                Color32::from_rgb(255, 230, 0)
            } else {
                base
            }
        };
        if rotates {
            let r = if tool == Tool::Transform { 0.6 } else { 1.0 };
            for i in 0..3 {
                let pts = ring(camera, rect, o, &axes, i, length * r);
                painter.add(egui::Shape::line(
                    pts,
                    Stroke::new(2.0, pick(Part::Ring(i), axis_color(i))),
                ));
            }
        }
        if moves {
            for (i, axis) in axes.iter().enumerate() {
                if let Some(tip) = to_screen(camera, rect, o + *axis * length) {
                    let col = pick(Part::Axis(i), axis_color(i));
                    painter.line_segment([center, tip], Stroke::new(2.5, col));
                    arrow_head(&painter, center, tip, col);
                }
            }
            painter.rect_stroke(
                Rect::from_center_size(center, vec2(12.0, 12.0)),
                1.0,
                Stroke::new(1.5, pick(Part::Free, Color32::from_gray(220))),
                egui::StrokeKind::Middle,
            );
        }
    }
    if state.splines.drawing {
        painter.text(
            rect.left_bottom() + vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            "Draw Spline: click to add knots, drag for tangents, click the first knot to close · Enter/Esc to finish",
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
    }
    owns || state.splines.drawing && in_view || !state.splines.selected.is_empty() && gizmo_tool
}

/// Draw Spline: add a knot under the cursor to the spline being drawn.
fn place(state: &mut EditorState, e: Entity, camera: &Camera, rect: Rect, at: Pos2) {
    let Some(mut c) = container(&state.scene, e) else {
        return;
    };
    let (point, normal) = surface(&state.scene, camera, rect, at, Some(e));
    let local = state
        .scene
        .world_matrix(e)
        .inverse()
        .transform_point3(point);
    let si = match state.splines.draw_spline.filter(|s| *s < c.splines.len()) {
        Some(s) => s,
        None => {
            // Start in an empty spline if there is one, else add a new one.
            let s = match c.splines.iter().position(|s| s.knots.is_empty()) {
                Some(s) => s,
                None => {
                    c.splines.push(Spline::default());
                    c.splines.len() - 1
                }
            };
            state.splines.draw_spline = Some(s);
            s
        }
    };
    state.record("Add Knot");
    c.splines[si].knots.push(Knot::at(local));
    let r = KnotRef {
        spline: si,
        knot: c.splines[si].knots.len() - 1,
    };
    store(&mut state.scene, e, c);
    state.splines.selected = vec![r];
    let normal = if normal.length_squared() > 0.5 {
        normal
    } else {
        Vec3::Y
    };
    state.splines.placing = Some(Placing {
        knot: r,
        press: at,
        point,
        normal,
    });
}

#[allow(clippy::too_many_arguments)]
fn apply_drag(
    state: &mut EditorState,
    e: Entity,
    c: &mut SplineContainer,
    drag: &Drag,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
    m: Mat4,
) {
    let (o, d) = ray(camera, rect, at);
    let inv = m.inverse();
    let (_, mr, _) = m.to_scale_rotation_translation();
    let label = match drag.part {
        Part::Knot(_) | Part::Free | Part::Axis(_) => {
            let delta = match drag.part {
                Part::Axis(i) => {
                    let Some(t) = closest_on_axis(drag.origin, drag.axes[i], o, d) else {
                        return;
                    };
                    drag.axes[i] * (t - (drag.grab - drag.origin).dot(drag.axes[i]))
                }
                _ => {
                    let Some(p) = ray_plane(o, d, drag.origin, camera.forward()) else {
                        return;
                    };
                    p - drag.grab
                }
            };
            for (r, start, _) in &drag.start {
                if let Some(k) = c.knot_mut(r.spline, r.knot) {
                    k.position = inv.transform_point3(*start + delta);
                }
            }
            if drag.start.len() == 1 {
                "Move Knot"
            } else {
                "Move Knots"
            }
        }
        Part::Ring(i) => {
            let Some(p) = ray_plane(o, d, drag.origin, drag.axes[i]) else {
                return;
            };
            let a = drag.grab.normalize_or_zero();
            let b = (p - drag.origin).normalize_or_zero();
            let angle = a.cross(b).dot(drag.axes[i]).atan2(a.dot(b));
            let q = Quat::from_axis_angle(drag.axes[i], angle);
            let before = c.clone();
            for (r, start, rot) in &drag.start {
                let (tin, tout) = before.splines[r.spline].tangents(r.knot);
                if let Some(k) = c.knot_mut(r.spline, r.knot) {
                    // Auto and Linear knots have no tangents of their own: keep the ones they
                    // show (in the knot's frame) as Broken Bézier ones, which then turn with it.
                    if !k.mode.is_bezier() {
                        let back = k.rotation.inverse();
                        k.tangent_in = back * tin;
                        k.tangent_out = back * tout;
                        k.mode = TangentMode::Broken;
                    }
                    k.position = inv.transform_point3(drag.origin + q * (*start - drag.origin));
                    k.rotation = (mr.inverse() * q * *rot).normalize();
                }
            }
            if drag.start.len() == 1 {
                "Rotate Knot"
            } else {
                "Rotate Knots"
            }
        }
        Part::Tangent(r, out) => {
            let Some(p) = ray_plane(o, d, drag.grab, camera.forward()) else {
                return;
            };
            let Some(k) = c.knot_mut(r.spline, r.knot) else {
                return;
            };
            let local = inv.transform_point3(p) - k.position;
            let v = k.rotation.inverse() * local;
            if out {
                k.tangent_out = v;
            } else {
                k.tangent_in = v;
            }
            k.enforce(!out);
            "Move Tangent"
        }
    };
    store(&mut state.scene, e, c.clone());
    state.mark_edited(label);
}

/// New knot after the last one of spline `s`, a little further along (Inspector "Add Knot").
pub fn append_knot(c: &mut SplineContainer, s: usize) {
    let Some(spline) = c.splines.get_mut(s) else {
        return;
    };
    let next = match spline.knots.as_slice() {
        [] => Vec3::ZERO,
        [one] => one.position + Vec3::Z * 2.0,
        [.., a, b] => b.position + (b.position - a.position).normalize_or_zero() * 2.0,
    };
    spline.knots.push(Knot::at(next));
}

#[cfg(test)]
mod tests;
