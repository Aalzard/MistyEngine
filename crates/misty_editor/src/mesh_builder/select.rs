//! Element picking (click, Shift+click, box drag) and drawing in the Scene view.

use egui::{Color32, Pos2, Rect, Response, Stroke, Ui, vec2};
use glam::{Mat4, Vec3};
use misty_engine::camera::{Camera, Projection};
use misty_engine::mesh_builder::{EditableMesh, Selection, edge_key};
use misty_engine::render::LineVertex;

use super::{EditMode, editing, mesh_of};
use crate::gizmos::Lines;
use crate::picking::{ray, to_screen};
use crate::state::{EditorState, Tool};

const EDGE: [f32; 4] = [0.75, 0.82, 0.95, 0.75];
const EDGE_DIM: [f32; 4] = [0.05, 0.05, 0.06, 0.6];
const SELECTED: [f32; 4] = [1.0, 0.6, 0.12, 1.0];
const FACE_FILL: [f32; 4] = [1.0, 0.55, 0.1, 0.35];
const PICK_VERTEX_PX: f32 = 10.0;
const PICK_EDGE_PX: f32 = 7.0;

/// The edited mesh in world space, with its triangles (for occlusion and face picks).
struct World {
    mesh: EditableMesh,
    matrix: Mat4,
    points: Vec<Vec3>,
    /// (face, triangle corners in world space)
    tris: Vec<(usize, [Vec3; 3])>,
}

impl World {
    fn new(state: &EditorState) -> Option<Self> {
        let e = editing(state)?;
        let mesh = mesh_of(state, e)?;
        let matrix = state.scene.world_matrix(e);
        let points: Vec<Vec3> = mesh
            .positions
            .iter()
            .map(|p| matrix.transform_point3(*p))
            .collect();
        let mut tris = Vec::new();
        for f in 0..mesh.faces.len() {
            for t in mesh.triangles_of(f) {
                tris.push((f, t.map(|v| points[v as usize])));
            }
        }
        Some(Self {
            mesh,
            matrix,
            points,
            tris,
        })
    }

    /// Nearest face hit by a ray: (face, distance).
    fn hit(&self, origin: Vec3, dir: Vec3) -> Option<(usize, f32)> {
        self.tris
            .iter()
            .filter_map(|(f, t)| ray_triangle(origin, dir, t).map(|d| (*f, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Is world point `p` in view (not hidden behind the mesh itself)?
    fn visible(&self, camera: &Camera, p: Vec3) -> bool {
        let (origin, dir) = eye_ray(camera, p);
        let dist = (p - origin).dot(dir);
        let eps = 1e-3 * dist.max(1.0);
        self.hit(origin, dir).is_none_or(|(_, d)| d > dist - eps)
    }
}

/// Ray from the eye through `p` (orthographic: along the view direction).
fn eye_ray(camera: &Camera, p: Vec3) -> (Vec3, Vec3) {
    match camera.projection {
        Projection::Orthographic { .. } => {
            let d = camera.forward();
            (p - d * 1000.0, d)
        }
        _ => (camera.position, (p - camera.position).normalize_or_zero()),
    }
}

/// Möller–Trumbore, both sides; distance along `dir`.
fn ray_triangle(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-10 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = e2.dot(q) * inv;
    (dist > 0.0).then_some(dist)
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Element under the cursor in the current mode.
fn pick(state: &EditorState, camera: &Camera, rect: Rect, at: Pos2) -> Option<Selection> {
    let w = World::new(state)?;
    let screen = |p: Vec3| to_screen(camera, rect, p);
    match state.mesh_builder.mode {
        EditMode::Vertex => w
            .points
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some((i as u32, screen(*p)?.distance(at), *p)))
            .filter(|(_, d, p)| *d < PICK_VERTEX_PX && w.visible(camera, *p))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(v, _, _)| Selection::verts(vec![v])),
        EditMode::Edge => w
            .mesh
            .edges()
            .into_iter()
            .filter_map(|e| {
                let (a, b) = (w.points[e[0] as usize], w.points[e[1] as usize]);
                let d = dist_to_segment(at, screen(a)?, screen(b)?);
                Some((e, d, (a + b) * 0.5))
            })
            .filter(|(_, d, mid)| *d < PICK_EDGE_PX && w.visible(camera, *mid))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _, _)| Selection::edges(vec![e])),
        EditMode::Face => {
            let (o, d) = ray(camera, rect, at);
            w.hit(o, d).map(|(f, _)| Selection::faces(vec![f]))
        }
        EditMode::Object => None,
    }
}

/// Elements inside a screen rectangle (visible ones only).
fn box_pick(state: &EditorState, camera: &Camera, rect: Rect, area: Rect) -> Selection {
    let Some(w) = World::new(state) else {
        return Selection::default();
    };
    let inside = |p: Vec3| to_screen(camera, rect, p).is_some_and(|s| area.contains(s));
    match state.mesh_builder.mode {
        EditMode::Vertex => Selection::verts(
            (0..w.points.len() as u32)
                .filter(|v| {
                    let p = w.points[*v as usize];
                    inside(p) && w.visible(camera, p)
                })
                .collect(),
        ),
        EditMode::Edge => Selection::edges(
            w.mesh
                .edges()
                .into_iter()
                .filter(|e| {
                    let (a, b) = (w.points[e[0] as usize], w.points[e[1] as usize]);
                    inside(a) && inside(b) && w.visible(camera, (a + b) * 0.5)
                })
                .collect(),
        ),
        EditMode::Face => Selection::faces(
            (0..w.mesh.faces.len())
                .filter(|f| {
                    let c = w.matrix.transform_point3(w.mesh.face_center(*f));
                    let n = w.matrix.transform_vector3(w.mesh.face_normal(*f));
                    let (o, _) = eye_ray(camera, c);
                    inside(c) && n.dot(o - c) > 0.0 && w.visible(camera, c)
                })
                .collect(),
        ),
        EditMode::Object => Selection::default(),
    }
}

/// Add `add` to `sel` (Shift), or toggle it (Ctrl).
fn merge(sel: &mut Selection, add: Selection, toggle: bool) {
    fn apply<T: PartialEq + Copy>(list: &mut Vec<T>, items: Vec<T>, toggle: bool) {
        for x in items {
            match list.iter().position(|y| *y == x) {
                Some(i) if toggle => {
                    list.remove(i);
                }
                Some(_) => {}
                None => list.push(x),
            }
        }
    }
    apply(&mut sel.verts, add.verts, toggle);
    apply(&mut sel.edges, add.edges, toggle);
    apply(&mut sel.faces, add.faces, toggle);
}

/// Element clicks and box selection while editing. Returns true when it handled the click
/// (the Scene view then skips object picking). Call after the handles.
pub fn scene_view(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    response: &Response,
    suppressed: bool,
) -> bool {
    if editing(state).is_none() || state.tool == Tool::Hand {
        state.mesh_builder.box_start = None;
        return false;
    }
    let mods = ui.input(|i| i.modifiers);
    // Box selection: a left drag that did not start on a handle.
    if response.drag_started_by(egui::PointerButton::Primary)
        && !mods.alt
        && state.handles.drag.is_none()
        && state.handles.hot.is_none()
    {
        state.mesh_builder.box_start = ui.input(|i| i.pointer.press_origin());
    }
    if let Some(start) = state.mesh_builder.box_start {
        let now = ui.input(|i| i.pointer.hover_pos()).unwrap_or(start);
        let area = Rect::from_two_pos(start, now);
        if response.dragged_by(egui::PointerButton::Primary) {
            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_filled(area, 0.0, Color32::from_rgba_unmultiplied(90, 140, 220, 40));
            painter.rect_stroke(
                area,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(120, 170, 240)),
                egui::StrokeKind::Inside,
            );
        } else {
            state.mesh_builder.box_start = None;
            if area.width() > 3.0 || area.height() > 3.0 {
                let found = box_pick(state, camera, rect, area);
                let sel = &mut state.mesh_builder.selection;
                if mods.shift || mods.command {
                    merge(sel, found, mods.command);
                } else {
                    *sel = found;
                }
            }
            return true;
        }
    }
    if !response.clicked() || suppressed || mods.alt {
        return false;
    }
    let Some(at) = response.interact_pointer_pos() else {
        return false;
    };
    match pick(state, camera, rect, at) {
        Some(hit) => {
            let sel = &mut state.mesh_builder.selection;
            if mods.shift || mods.command {
                merge(sel, hit, mods.command);
            } else {
                *sel = hit;
            }
            true
        }
        None => {
            // Another object under the cursor: select it (the mode stays); empty space clears
            // the elements but keeps the object.
            let other =
                crate::picking::pick(&state.scene, camera, rect, at, &state.scene_view.icons)
                    .filter(|o| Some(*o) != editing(state));
            match other {
                Some(o) => state.select(o),
                None if mods.shift || mods.command => {}
                None => state.mesh_builder.selection = Selection::default(),
            }
            true
        }
    }
}

/// Edges of the edited mesh (selected ones highlighted) and selected faces as a translucent
/// overlay. Vertex dots are painted on top by [`paint`].
pub fn lines(
    state: &EditorState,
    camera: &Camera,
    lines: &mut Lines,
    overlay: &mut Vec<LineVertex>,
) {
    let Some(w) = World::new(state) else {
        return;
    };
    // Lift towards the eye so lines win the depth test against their own faces.
    let lift = |p: Vec3| {
        let (o, _) = eye_ray(camera, p);
        let d = p - o;
        p - d.normalize_or_zero() * (d.length() * 0.002).min(0.05)
    };
    let mb = &state.mesh_builder;
    let selected_edges: std::collections::HashSet<[u32; 2]> = match mb.mode {
        EditMode::Edge => mb.selection.edges.iter().copied().collect(),
        EditMode::Face => mb
            .selection
            .faces
            .iter()
            .filter_map(|f| w.mesh.faces.get(*f))
            .flat_map(|f| f.edges().map(|(a, b)| edge_key(a, b)))
            .collect(),
        _ => Default::default(),
    };
    let base = if mb.mode == EditMode::Face {
        EDGE_DIM
    } else {
        EDGE
    };
    for e in w.mesh.edges() {
        let color = if selected_edges.contains(&e) {
            SELECTED
        } else {
            base
        };
        lines.line(
            lift(w.points[e[0] as usize]),
            lift(w.points[e[1] as usize]),
            color,
        );
    }
    if mb.mode == EditMode::Face {
        for (f, t) in &w.tris {
            if !mb.selection.faces.contains(f) {
                continue;
            }
            for p in t {
                overlay.push(LineVertex {
                    position: lift(*p).into(),
                    color: FACE_FILL,
                });
            }
        }
    }
}

/// Vertex dots (Vertex mode): visible vertices as small squares, selected ones orange.
pub fn paint(ui: &Ui, state: &EditorState, camera: &Camera, rect: Rect) {
    if state.mesh_builder.mode != EditMode::Vertex {
        return;
    }
    let Some(w) = World::new(state) else {
        return;
    };
    let painter = ui.painter().with_clip_rect(rect);
    let sel = &state.mesh_builder.selection.verts;
    for (i, p) in w.points.iter().enumerate() {
        if !w.visible(camera, *p) {
            continue;
        }
        let Some(s) = to_screen(camera, rect, *p) else {
            continue;
        };
        let on = sel.contains(&(i as u32));
        let (size, color) = if on {
            (7.0, Color32::from_rgb(255, 153, 30))
        } else {
            (5.0, Color32::from_rgb(190, 210, 240))
        };
        painter.rect_filled(Rect::from_center_size(s, vec2(size, size)), 1.0, color);
        painter.rect_stroke(
            Rect::from_center_size(s, vec2(size, size)),
            1.0,
            Stroke::new(1.0, Color32::from_black_alpha(160)),
            egui::StrokeKind::Outside,
        );
    }
}
