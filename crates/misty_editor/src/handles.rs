//! Move / Rotate / Scale handles in the Scene view (screen-space drawing and hit testing).
//!
//! Handles act on the top-level selection. Hold Ctrl to snap by the Grid and Snap increments,
//! Ctrl+Shift on the centre handle to snap onto surfaces, V to snap vertex to vertex; the Scene
//! toolbar's grid snapping rounds moves to the world grid.

use egui::{Color32, Key, Pos2, Rect, Shape, Stroke, Ui, vec2};
use glam::{Mat4, Quat, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::Transform;

use crate::picking::{self, ray, to_screen};
use crate::snap;
use crate::state::{EditorState, Tool};
use crate::theme;

const AXIS_PX: f32 = 90.0;
const PICK_PX: f32 = 7.0;
/// Vertex snapping only considers target vertices this close to the cursor.
const VERTEX_PX: f32 = 40.0;
const HOT: Color32 = Color32::from_rgb(255, 230, 0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    MoveAxis(usize),
    /// Plane whose normal is this axis.
    MovePlane(usize),
    MoveFree,
    RotateAxis(usize),
    RotateView,
    ScaleAxis(usize),
    ScaleUniform,
}

/// State captured when a handle drag starts.
#[derive(Clone, Debug)]
pub struct Drag {
    part: Part,
    origin: Vec3,
    axes: [Vec3; 3],
    start_mouse: Pos2,
    /// Axis parameter / plane point / ring vector at the start, depending on `part`.
    start_point: Vec3,
    targets: Vec<(Entity, Mat4, Transform)>,
}

#[derive(Default)]
pub struct HandleState {
    pub hot: Option<Part>,
    pub drag: Option<Drag>,
    /// The current press started on a handle: don't treat its release as a selection click.
    pub suppress_click: bool,
}

impl HandleState {
    /// The click being released started on a handle.
    pub fn click_suppressed(&self) -> bool {
        self.suppress_click
    }
}

/// Handle placement for the current selection.
struct Frame {
    origin: Vec3,
    axes: [Vec3; 3],
    /// World length of a 90 px axis.
    length: f32,
}

fn axis_color(i: usize) -> Color32 {
    [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][i]
}

fn handle_frame(state: &EditorState, camera: &Camera, rect: Rect, local: bool) -> Option<Frame> {
    let active = state.selection.active()?;
    let targets = state.selection.top_level(&state.scene);
    if targets.is_empty() {
        return None;
    }
    let origin = if state.pivot_center {
        targets
            .iter()
            .map(|e| state.scene.world_position(*e))
            .sum::<Vec3>()
            / targets.len() as f32
    } else {
        state.scene.world_position(active)
    };
    let rot = if local {
        state.scene.world_rotation(active)
    } else {
        Quat::IDENTITY
    };
    let axes = [rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z];
    let length = camera.world_per_pixel(origin, rect.height()) * AXIS_PX;
    Some(Frame {
        origin,
        axes,
        length,
    })
}

/// Draw handles, process hover/drag. Call before selection picking.
pub fn show(ui: &Ui, state: &mut EditorState, camera: &Camera, rect: Rect, view_hovered: bool) {
    // Every new press decides afresh whether its click belongs to a handle.
    if ui.input(|i| i.pointer.primary_pressed()) {
        state.handles.suppress_click = false;
    }
    let tool = state.tool;
    if matches!(tool, Tool::Hand | Tool::Rect) {
        state.handles.hot = None;
        return;
    }
    let local = !state.global_space || tool == Tool::Scale;
    let Some(mut frame) = handle_frame(state, camera, rect, local) else {
        state.handles.hot = None;
        state.handles.drag = None;
        return;
    };
    // Holding V puts the move handle on the selection's vertex nearest the cursor.
    let moves = matches!(tool, Tool::Move | Tool::Transform);
    if moves
        && state.handles.drag.is_none()
        && ui.input(|i| i.key_down(Key::V))
        && let Some(m) = ui.input(|i| i.pointer.hover_pos())
    {
        let selected = state.selection.top_level(&state.scene);
        let scene = &state.scene;
        let mine = |e: Entity| selected.iter().any(|s| scene.is_ancestor(*s, e));
        if let Some(v) = picking::nearest_vertex(scene, camera, rect, m, f32::INFINITY, mine) {
            frame.origin = v;
        }
    }
    let Some(center) = to_screen(camera, rect, frame.origin) else {
        return;
    };
    let (mouse, pressed, down, alt) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.alt,
        )
    });

    // Hover test (not while dragging).
    if state.handles.drag.is_none() {
        state.handles.hot = mouse
            .filter(|m| view_hovered && rect.contains(*m) && !alt)
            .and_then(|m| hit_test(tool, camera, rect, &frame, center, m));
    }

    // Start a drag.
    if pressed
        && let (Some(part), Some(m)) = (state.handles.hot, mouse)
        && state.handles.drag.is_none()
    {
        let targets = state
            .selection
            .top_level(&state.scene)
            .into_iter()
            .map(|e| (e, state.scene.world_matrix(e), state.scene.transform(e)))
            .collect();
        let start_point = grab_point(part, camera, rect, &frame, m).unwrap_or(frame.origin);
        state.handles.drag = Some(Drag {
            part,
            origin: frame.origin,
            axes: frame.axes,
            start_mouse: m,
            start_point,
            targets,
        });
        state.handles.suppress_click = true;
    }

    // Update or finish.
    if let Some(drag) = state.handles.drag.clone() {
        if down {
            if let Some(m) = mouse {
                apply_drag(ui, state, camera, rect, &drag, m);
                ui.ctx().request_repaint();
            }
        } else {
            state.handles.drag = None;
        }
    }

    let hot = state
        .handles
        .drag
        .as_ref()
        .map(|d| d.part)
        .or(state.handles.hot);
    draw(ui, tool, camera, rect, &frame, center, hot);
}

/// Which handle part is under `m`.
fn hit_test(
    tool: Tool,
    camera: &Camera,
    rect: Rect,
    f: &Frame,
    center: Pos2,
    m: Pos2,
) -> Option<Part> {
    let mut best: Option<(Part, f32)> = None;
    let mut consider = |part: Part, d: f32| {
        if d < PICK_PX && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((part, d));
        }
    };
    let moves = matches!(tool, Tool::Move | Tool::Transform);
    let rotates = matches!(tool, Tool::Rotate | Tool::Transform);
    let scales = matches!(tool, Tool::Scale | Tool::Transform);

    if moves || scales {
        // Uniform/free square in the centre.
        if Rect::from_center_size(center, vec2(14.0, 14.0)).contains(m) {
            return Some(if moves {
                Part::MoveFree
            } else {
                Part::ScaleUniform
            });
        }
        if moves {
            for n in 0..3 {
                if let Some(quad) = plane_quad(camera, rect, f, n)
                    && point_in_quad(m, &quad)
                {
                    return Some(Part::MovePlane(n));
                }
            }
        }
        for i in 0..3 {
            if let Some(tip) = to_screen(camera, rect, f.origin + f.axes[i] * f.length) {
                let part = if moves {
                    Part::MoveAxis(i)
                } else {
                    Part::ScaleAxis(i)
                };
                consider(part, dist_to_segment(m, center, tip));
            }
        }
    }
    if rotates {
        let r = if tool == Tool::Transform { 0.6 } else { 1.0 };
        for i in 0..3 {
            let pts = ring_points(camera, rect, f, i, r);
            for w in pts.windows(2) {
                if let [(a, fa), (b, fb)] = w
                    && (*fa || *fb)
                {
                    consider(Part::RotateAxis(i), dist_to_segment(m, *a, *b));
                }
            }
        }
        let outer = AXIS_PX * r * 1.12;
        consider(Part::RotateView, ((m - center).length() - outer).abs());
    }
    best.map(|(p, _)| p)
}

/// Starting reference for a drag: point on axis/plane, or ring vector.
fn grab_point(part: Part, camera: &Camera, rect: Rect, f: &Frame, m: Pos2) -> Option<Vec3> {
    let (o, d) = ray(camera, rect, m);
    match part {
        Part::MoveAxis(i) => {
            closest_on_axis(f.origin, f.axes[i], o, d).map(|t| f.origin + f.axes[i] * t)
        }
        Part::MovePlane(n) => ray_plane(o, d, f.origin, f.axes[n]),
        Part::MoveFree => ray_plane(o, d, f.origin, camera.forward()),
        Part::RotateAxis(i) => ray_plane(o, d, f.origin, f.axes[i]).map(|p| p - f.origin),
        Part::RotateView | Part::ScaleAxis(_) | Part::ScaleUniform => Some(f.origin),
    }
}

fn apply_drag(ui: &Ui, state: &mut EditorState, camera: &Camera, rect: Rect, drag: &Drag, m: Pos2) {
    let (ctrl, shift, vertex) =
        ui.input(|i| (i.modifiers.command, i.modifiers.shift, i.key_down(Key::V)));
    let snap = ctrl && !shift;
    let steps = state.snap.clone();
    // Grid snapping works with the world-aligned (Global) handle only.
    let grid = steps.grid_snap && state.global_space;
    let (o, d) = ray(camera, rect, m);
    let f = Frame {
        origin: drag.origin,
        axes: drag.axes,
        length: camera.world_per_pixel(drag.origin, rect.height()) * AXIS_PX,
    };
    match drag.part {
        Part::MoveAxis(_) | Part::MovePlane(_) | Part::MoveFree if vertex => {
            let exclude = selected_subtrees(state, drag);
            let other = |e: Entity| !exclude.contains(&e);
            if let Some(target) =
                picking::nearest_vertex(&state.scene, camera, rect, m, VERTEX_PX, other)
            {
                translate(state, drag, target - drag.origin);
            }
        }
        Part::MoveFree if ctrl && shift => {
            let exclude = selected_subtrees(state, drag);
            let other = |e: Entity| !exclude.contains(&e);
            if let Some(hit) = picking::surface_point(&state.scene, camera, rect, m, other) {
                translate(state, drag, hit - drag.origin);
            }
        }
        Part::MoveAxis(i) => {
            let Some(t) = closest_on_axis(f.origin, f.axes[i], o, d) else {
                return;
            };
            let start = (drag.start_point - f.origin).dot(f.axes[i]);
            let mut delta = f.axes[i] * (t - start);
            if snap {
                delta = snap::snap_delta(delta, f.axes, steps.move_step);
            }
            if grid {
                let mut mask = [false; 3];
                mask[i] = true;
                delta = snap::snap_to_grid(f.origin + delta, steps.grid_size, mask) - f.origin;
            }
            translate(state, drag, delta);
        }
        Part::MovePlane(_) | Part::MoveFree => {
            let normal = match drag.part {
                Part::MovePlane(n) => f.axes[n],
                _ => camera.forward(),
            };
            let Some(p) = ray_plane(o, d, f.origin, normal) else {
                return;
            };
            let mut delta = p - drag.start_point;
            if snap {
                delta = snap::snap_delta(delta, f.axes, steps.move_step);
            }
            if grid {
                let mask = match drag.part {
                    Part::MovePlane(n) => std::array::from_fn(|i| i != n),
                    _ => [true; 3],
                };
                delta = snap::snap_to_grid(f.origin + delta, steps.grid_size, mask) - f.origin;
            }
            translate(state, drag, delta);
        }
        Part::RotateAxis(i) => {
            let Some(p) = ray_plane(o, d, f.origin, f.axes[i]) else {
                return;
            };
            let a = drag.start_point.normalize_or_zero();
            let b = (p - f.origin).normalize_or_zero();
            let mut angle = a.cross(b).dot(f.axes[i]).atan2(a.dot(b)).to_degrees();
            if snap {
                angle = snap::snap(angle, steps.rotate_step);
            }
            rotate(
                state,
                drag,
                Quat::from_axis_angle(f.axes[i], angle.to_radians()),
            );
        }
        Part::RotateView => {
            let Some(c) = to_screen(camera, rect, f.origin) else {
                return;
            };
            let a = drag.start_mouse - c;
            let b = m - c;
            // Screen y is down, so negate to get the view-axis angle.
            let mut angle = -(a.x * b.y - a.y * b.x)
                .atan2(a.x * b.x + a.y * b.y)
                .to_degrees();
            if snap {
                angle = snap::snap(angle, steps.rotate_step);
            }
            rotate(
                state,
                drag,
                Quat::from_axis_angle(camera.forward(), -angle.to_radians()),
            );
        }
        Part::ScaleAxis(i) => {
            let (Some(c), Some(tip)) = (
                to_screen(camera, rect, f.origin),
                to_screen(camera, rect, f.origin + f.axes[i] * f.length),
            ) else {
                return;
            };
            let dir = (tip - c).normalized();
            let moved = (m - drag.start_mouse).dot(dir);
            let mut factor = 1.0 + moved / AXIS_PX;
            if snap {
                factor = snap::snap(factor, steps.scale_step);
            }
            let mut v = Vec3::ONE;
            v[i] = factor;
            scale(state, drag, v);
        }
        Part::ScaleUniform => {
            let moved = (m - drag.start_mouse).x - (m - drag.start_mouse).y;
            let mut factor = (1.0 + moved / AXIS_PX).max(0.01);
            if snap {
                factor = snap::snap(factor, steps.scale_step).max(steps.scale_step);
            }
            scale(state, drag, Vec3::splat(factor));
        }
    }
    state.mark_edited("Transform");
}

/// Dragged objects and everything under them (never snap onto themselves).
fn selected_subtrees(state: &EditorState, drag: &Drag) -> Vec<Entity> {
    state
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| {
            drag.targets
                .iter()
                .any(|(t, _, _)| state.scene.is_ancestor(*t, *e))
        })
        .collect()
}

fn translate(state: &mut EditorState, drag: &Drag, delta: Vec3) {
    for (e, world, _) in &drag.targets {
        state
            .scene
            .set_world_position(*e, world.w_axis.truncate() + delta);
    }
}

fn rotate(state: &mut EditorState, drag: &Drag, q: Quat) {
    for (e, world, _) in &drag.targets {
        let (_, rot, pos) = world.to_scale_rotation_translation();
        state.scene.set_world_rotation(*e, q * rot);
        if state.pivot_center {
            state
                .scene
                .set_world_position(*e, drag.origin + q * (pos - drag.origin));
        }
    }
}

fn scale(state: &mut EditorState, drag: &Drag, factor: Vec3) {
    for (e, _, start) in &drag.targets {
        let mut t = state.scene.transform(*e);
        t.scale = start.scale * factor;
        state.scene.set_transform(*e, t);
    }
}

// ---- drawing ------------------------------------------------------------------------------

fn draw(
    ui: &Ui,
    tool: Tool,
    camera: &Camera,
    rect: Rect,
    f: &Frame,
    center: Pos2,
    hot: Option<Part>,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let color = |part: Part, base: Color32| if hot == Some(part) { HOT } else { base };
    let moves = matches!(tool, Tool::Move | Tool::Transform);
    let scales = tool == Tool::Scale;
    let rotates = matches!(tool, Tool::Rotate | Tool::Transform);

    if rotates {
        let r = if tool == Tool::Transform { 0.6 } else { 1.0 };
        for i in 0..3 {
            let c = color(Part::RotateAxis(i), axis_color(i));
            let pts = ring_points(camera, rect, f, i, r);
            for w in pts.windows(2) {
                if let [(a, fa), (b, fb)] = w {
                    let col = if *fa && *fb {
                        c
                    } else {
                        c.gamma_multiply(0.15)
                    };
                    painter.line_segment([*a, *b], Stroke::new(2.0, col));
                }
            }
        }
        let outer = AXIS_PX * r * 1.12;
        painter.circle_stroke(
            center,
            outer,
            Stroke::new(1.5, color(Part::RotateView, Color32::from_gray(200))),
        );
    }

    if moves {
        for n in 0..3 {
            if let Some(quad) = plane_quad(camera, rect, f, n) {
                let c = color(Part::MovePlane(n), axis_color(n));
                painter.add(Shape::convex_polygon(
                    quad.to_vec(),
                    c.gamma_multiply(0.35),
                    Stroke::new(1.0, c),
                ));
            }
        }
    }

    if moves || scales {
        // Draw back-to-front isn't needed for lines; draw axes then tips.
        for i in 0..3 {
            let Some(tip) = to_screen(camera, rect, f.origin + f.axes[i] * f.length) else {
                continue;
            };
            let part = if moves {
                Part::MoveAxis(i)
            } else {
                Part::ScaleAxis(i)
            };
            let c = color(part, axis_color(i));
            painter.line_segment([center, tip], Stroke::new(2.5, c));
            if moves {
                arrow_head(&painter, center, tip, c);
            } else {
                painter.rect_filled(Rect::from_center_size(tip, vec2(9.0, 9.0)), 1.0, c);
            }
        }
        let center_part = if moves {
            Part::MoveFree
        } else {
            Part::ScaleUniform
        };
        let c = color(center_part, Color32::from_gray(220));
        painter.rect_stroke(
            Rect::from_center_size(center, vec2(12.0, 12.0)),
            1.0,
            Stroke::new(1.5, c),
            egui::StrokeKind::Middle,
        );
    }
}

fn arrow_head(painter: &egui::Painter, from: Pos2, tip: Pos2, color: Color32) {
    let dir = (tip - from).normalized();
    let perp = vec2(-dir.y, dir.x) * 5.0;
    let end = tip + dir * 12.0;
    painter.add(Shape::convex_polygon(
        vec![end, tip + perp, tip - perp],
        color,
        Stroke::NONE,
    ));
}

/// Screen points of the ring around axis `i`, each flagged with whether it faces the camera.
fn ring_points(camera: &Camera, rect: Rect, f: &Frame, i: usize, scale: f32) -> Vec<(Pos2, bool)> {
    let (u, v) = (f.axes[(i + 1) % 3], f.axes[(i + 2) % 3]);
    let radius = f.length * scale;
    let to_cam = match camera.projection {
        misty_engine::camera::Projection::Orthographic { .. } => -camera.forward(),
        _ => (camera.position - f.origin).normalize_or_zero(),
    };
    (0..=64)
        .filter_map(|k| {
            let a = k as f32 / 64.0 * std::f32::consts::TAU;
            let dir = u * a.cos() + v * a.sin();
            let facing = dir.dot(to_cam) > -0.05;
            to_screen(camera, rect, f.origin + dir * radius).map(|p| (p, facing))
        })
        .collect()
}

/// Small square for the move plane with normal `n`, placed in the quadrant facing the camera.
fn plane_quad(camera: &Camera, rect: Rect, f: &Frame, n: usize) -> Option<[Pos2; 4]> {
    let (a, b) = (f.axes[(n + 1) % 3], f.axes[(n + 2) % 3]);
    let to_cam = (camera.position - f.origin).normalize_or_zero();
    let sa = if a.dot(to_cam) >= 0.0 { 1.0 } else { -1.0 };
    let sb = if b.dot(to_cam) >= 0.0 { 1.0 } else { -1.0 };
    let (lo, hi) = (0.18 * f.length, 0.42 * f.length);
    let p = |x: f32, y: f32| to_screen(camera, rect, f.origin + a * sa * x + b * sb * y);
    Some([p(lo, lo)?, p(hi, lo)?, p(hi, hi)?, p(lo, hi)?])
}

// ---- maths -------------------------------------------------------------------------------

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

fn point_in_quad(p: Pos2, q: &[Pos2; 4]) -> bool {
    let sign = |a: Pos2, b: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let s: Vec<f32> = (0..4).map(|i| sign(q[i], q[(i + 1) % 4])).collect();
    s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
}

/// Parameter along the axis line `p + t·a` closest to the ray `o + s·d`.
fn closest_on_axis(p: Vec3, a: Vec3, o: Vec3, d: Vec3) -> Option<f32> {
    let w0 = p - o;
    let (aa, ad, dd) = (a.dot(a), a.dot(d), d.dot(d));
    let denom = aa * dd - ad * ad;
    if denom.abs() < 1e-6 {
        return None;
    }
    Some((ad * d.dot(w0) - dd * a.dot(w0)) / denom)
}

fn ray_plane(o: Vec3, d: Vec3, p: Vec3, n: Vec3) -> Option<Vec3> {
    let denom = d.dot(n);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (p - o).dot(n) / denom;
    (t > 0.0).then(|| o + d * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn closest_point_on_axis_from_ray() {
        // Ray straight down at x = 3 hits the X axis at t = 3.
        let t = closest_on_axis(Vec3::ZERO, Vec3::X, Vec3::new(3.0, 10.0, 0.0), -Vec3::Y).unwrap();
        assert!((t - 3.0).abs() < 1e-5);
        assert!(closest_on_axis(Vec3::ZERO, Vec3::X, Vec3::new(0.0, 1.0, 0.0), Vec3::X).is_none());
    }

    #[test]
    fn segment_and_quad_hits() {
        assert!(dist_to_segment(pos2(5.0, 1.0), pos2(0.0, 0.0), pos2(10.0, 0.0)) < 1.01);
        let q = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
        ];
        assert!(point_in_quad(pos2(5.0, 5.0), &q));
        assert!(!point_in_quad(pos2(15.0, 5.0), &q));
    }
}
