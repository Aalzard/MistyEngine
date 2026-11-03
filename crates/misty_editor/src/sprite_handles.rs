//! 2D Scene-view handles: the Rect tool on a selected sprite (drag corners or edges to resize by
//! scaling the object, drag inside to move it) and Edit Collider handles for Box and Circle
//! Collider 2D (drag an edge or the rim). Drag state lives in egui's memory.

use egui::{Color32, Pos2, Rect, Stroke, Ui, vec2};
use glam::{Mat4, Vec2};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::physics2d::collider_frame;
use misty_engine::scene::{BoxCollider2D, CircleCollider2D, Transform};

use crate::picking::{ray, to_screen};
use crate::state::{EditorState, Tool};

const PICK_PX: f32 = 7.0;
const RECT_COLOR: Color32 = Color32::from_rgb(90, 160, 255);
const COLLIDER_COLOR: Color32 = Color32::from_rgb(145, 245, 140);

/// What a handle changes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// Rect tool: which sides move (-1 = min, 1 = max, 0 = none) per axis; (0, 0) = move.
    Rect(i8, i8),
    /// Box Collider 2D edge: axis and side.
    BoxEdge(usize, i8),
    /// Circle Collider 2D rim.
    Rim,
}

#[derive(Clone, Debug)]
struct Drag {
    entity: Entity,
    part: Part,
    /// Object (rect tool) or collider frame (collider handles) at the start.
    frame: Mat4,
    start: Transform,
    /// Local point under the mouse at the start.
    grab: Vec2,
}

/// The egui memory slot holding the object whose 2D collider is being edited (Inspector ▸
/// Edit Collider).
pub fn edit_collider_id() -> egui::Id {
    egui::Id::new("misty_edit_collider_2d")
}

fn drag_id() -> egui::Id {
    egui::Id::new("misty_sprite_handles_drag")
}

/// Point on the frame's XY plane under the mouse, in the frame's local space.
fn local_point(camera: &Camera, rect: Rect, frame: &Mat4, at: Pos2) -> Option<Vec2> {
    let (o, d) = ray(camera, rect, at);
    let n = frame.z_axis.truncate().normalize_or_zero();
    let p0 = frame.w_axis.truncate();
    let denom = d.dot(n);
    if denom.abs() < 1e-6 {
        return None;
    }
    let hit = o + d * ((p0 - o).dot(n) / denom);
    Some(frame.inverse().transform_point3(hit).truncate())
}

/// Handles of the selection: (part, local position) plus the frame they live in.
/// The handles' object, frame, (part, local position) list and colour.
type HandleSet = (Entity, Mat4, Vec<(Part, Vec2)>, Color32);

fn handles(state: &EditorState, ui: &Ui) -> Option<HandleSet> {
    let e = state.selection.active()?;
    let scene = &state.scene;
    let editing = ui.ctx().data(|d| d.get_temp::<u64>(edit_collider_id()));
    if editing == Some(e.to_bits().get()) {
        let (m, s) = collider_frame(scene, e);
        let mut list = Vec::new();
        if let Ok(c) = scene.world.get::<&BoxCollider2D>(e) {
            let (o, h) = (c.common.offset * s, c.size * s * 0.5);
            for (axis, side) in [(0, -1), (0, 1), (1, -1), (1, 1)] {
                let mut p = o;
                p[axis] += h[axis] * side as f32;
                list.push((Part::BoxEdge(axis, side), p));
            }
        } else if let Ok(c) = scene.world.get::<&CircleCollider2D>(e) {
            let (o, r) = (c.common.offset * s, c.radius * s.max_element());
            for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
                list.push((Part::Rim, o + dir * r));
            }
        }
        return (!list.is_empty()).then_some((e, m, list, COLLIDER_COLOR));
    }
    if state.tool != Tool::Rect {
        return None;
    }
    let (min, max) = misty_engine::sprites::local_bounds(scene, e)?;
    let mut list = Vec::new();
    for sy in [-1i8, 0, 1] {
        for sx in [-1i8, 0, 1] {
            if sx == 0 && sy == 0 {
                continue;
            }
            let pick = |s: i8, lo: f32, hi: f32| match s {
                -1 => lo,
                1 => hi,
                _ => (lo + hi) * 0.5,
            };
            let p = Vec2::new(pick(sx, min.x, max.x), pick(sy, min.y, max.y));
            list.push((Part::Rect(sx, sy), p));
        }
    }
    Some((e, scene.world_matrix(e), list, RECT_COLOR))
}

/// Draw and drive the 2D handles. True when they own the mouse this frame (a handle is hot or
/// being dragged), so the transform handles and click selection stay out of the way.
pub fn show(ui: &Ui, state: &mut EditorState, camera: &Camera, rect: Rect, hovered: bool) -> bool {
    let id = drag_id();
    let drag = ui.ctx().data(|d| d.get_temp::<Drag>(id));
    let Some((e, frame, list, color)) = handles(state, ui) else {
        ui.ctx().data_mut(|d| d.remove::<Drag>(id));
        return false;
    };
    let (mouse, pressed, down) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
        )
    });
    let screen: Vec<(Part, Pos2)> = list
        .iter()
        .filter_map(|(p, l)| {
            Some((
                *p,
                to_screen(camera, rect, frame.transform_point3(l.extend(0.0)))?,
            ))
        })
        .collect();
    let mut hot = mouse
        .filter(|m| hovered && rect.contains(*m))
        .and_then(|m| {
            screen
                .iter()
                .map(|(p, s)| (*p, s.distance(m)))
                .filter(|(_, d)| *d < PICK_PX)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(p, _)| p)
        });
    // Rect tool: inside the sprite moves it.
    if hot.is_none()
        && matches!(list.first(), Some((Part::Rect(..), _)))
        && let Some(m) = mouse.filter(|m| hovered && rect.contains(*m))
        && let Some(q) = local_point(camera, rect, &frame, m)
        && let Some((min, max)) = misty_engine::sprites::local_bounds(&state.scene, e)
        && q.cmpge(min).all()
        && q.cmple(max).all()
        && state.selection.contains(e)
    {
        hot = Some(Part::Rect(0, 0));
    }
    let mut owned = hot.is_some();
    if pressed
        && drag.is_none()
        && let (Some(part), Some(m)) = (hot, mouse)
        && let Some(grab) = local_point(camera, rect, &frame, m)
    {
        let d = Drag {
            entity: e,
            part,
            frame,
            start: state.scene.transform(e),
            grab,
        };
        ui.ctx().data_mut(|data| data.insert_temp(id, d));
        state.handles.suppress_click = true;
    }
    if let Some(d) = ui.ctx().data(|data| data.get_temp::<Drag>(id)) {
        owned = true;
        if down && d.entity == e {
            if let Some(q) = mouse.and_then(|m| local_point(camera, rect, &d.frame, m)) {
                apply(state, &d, q);
                ui.ctx().request_repaint();
            }
        } else {
            ui.ctx().data_mut(|data| data.remove::<Drag>(id));
        }
    }
    draw(ui, rect, &screen, color, hot);
    owned
}

fn apply(state: &mut EditorState, d: &Drag, q: Vec2) {
    let e = d.entity;
    match d.part {
        Part::Rect(sx, sy) => {
            let Some((min, max)) = misty_engine::sprites::local_bounds(&state.scene, e) else {
                return;
            };
            let size = (max - min).max(Vec2::splat(1e-4));
            let (mut nmin, mut nmax) = (min, max);
            if (sx, sy) == (0, 0) {
                let delta = q - d.grab;
                nmin += delta;
                nmax += delta;
            }
            for (axis, side) in [(0, sx), (1, sy)] {
                match side {
                    -1 => nmin[axis] = q[axis].min(nmax[axis] - size[axis] * 0.01),
                    1 => nmax[axis] = q[axis].max(nmin[axis] + size[axis] * 0.01),
                    _ => {}
                }
            }
            // Map the sprite's bounds onto the new rect: scale k, origin o (start's local frame).
            let k = (nmax - nmin) / size;
            let o = nmin - k * min;
            let mut t = d.start;
            t.scale = d.start.scale * k.extend(1.0);
            state.scene.set_transform(e, t);
            let world = d.frame.transform_point3(o.extend(0.0));
            state.scene.set_world_position(e, world);
            state.mark_edited("Rect Tool");
        }
        Part::BoxEdge(axis, side) => {
            let (_, s) = collider_frame(&state.scene, e);
            let s = s.max(Vec2::splat(1e-4));
            let p = q / s;
            if let Ok(mut c) = state.scene.world.get::<&mut BoxCollider2D>(e) {
                let half = c.size[axis] * 0.5;
                let fixed = c.common.offset[axis] - half * side as f32;
                let moved = p[axis];
                let size = ((moved - fixed) * side as f32).max(0.01);
                c.size[axis] = size;
                c.common.offset[axis] = fixed + size * 0.5 * side as f32;
            }
            state.mark_edited("Edit Collider");
        }
        Part::Rim => {
            let (_, s) = collider_frame(&state.scene, e);
            if let Ok(mut c) = state.scene.world.get::<&mut CircleCollider2D>(e) {
                let center = c.common.offset * s;
                c.radius = (q.distance(center) / s.max_element().max(1e-4)).max(0.001);
            }
            state.mark_edited("Edit Collider");
        }
    }
}

fn draw(ui: &Ui, rect: Rect, screen: &[(Part, Pos2)], color: Color32, hot: Option<Part>) {
    let painter = ui.painter().with_clip_rect(rect);
    // Rect tool outline through the corner handles.
    let corner = |sx, sy| {
        screen
            .iter()
            .find(|(p, _)| *p == Part::Rect(sx, sy))
            .map(|(_, s)| *s)
    };
    if let (Some(a), Some(b), Some(c), Some(d)) =
        (corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1))
    {
        let stroke = Stroke::new(1.5, color);
        for (p, q) in [(a, b), (b, c), (c, d), (d, a)] {
            painter.line_segment([p, q], stroke);
        }
    }
    for (part, p) in screen {
        let fill = if Some(*part) == hot {
            Color32::from_rgb(255, 230, 0)
        } else {
            Color32::WHITE
        };
        let r = Rect::from_center_size(*p, vec2(8.0, 8.0));
        painter.rect_filled(r, 1.0, fill);
        painter.rect_stroke(r, 1.0, Stroke::new(1.0, color), egui::StrokeKind::Outside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;
    use glam::Vec3;
    use misty_engine::scene::SpriteRenderer;

    fn world(frame: &Mat4, p: Vec2) -> Vec3 {
        frame.transform_point3(p.extend(0.0))
    }

    fn test_state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_sprite_handles_test"),
        ))
    }

    #[test]
    fn rect_tool_resizes_by_scaling_and_keeps_the_opposite_corner() {
        let mut s = test_state();
        let e = s.scene.spawn("S", None);
        s.scene.add(
            e,
            SpriteRenderer {
                sprite: Some(misty_engine::sprites::builtin::path("Square")),
                ..Default::default()
            },
        );
        s.scene
            .set_transform(e, Transform::from_position(Vec3::new(1.0, 1.0, 0.0)));
        let frame = s.scene.world_matrix(e);
        let d = Drag {
            entity: e,
            part: Part::Rect(1, 1),
            frame,
            start: s.scene.transform(e),
            grab: Vec2::splat(0.5),
        };
        // Drag the top-right corner from (0.5, 0.5) to (1.5, 0.5) local.
        apply(&mut s, &d, Vec2::new(1.5, 0.5));
        let t = s.scene.transform(e);
        assert!((t.scale.x - 2.0).abs() < 1e-4 && (t.scale.y - 1.0).abs() < 1e-4);
        // The bottom-left corner stays at world (0.5, 0.5).
        let bl = world(&s.scene.world_matrix(e), Vec2::splat(-0.5));
        assert!(bl.distance(Vec3::new(0.5, 0.5, 0.0)) < 1e-4, "{bl}");
        // Dragging inside moves.
        let mv = Drag {
            part: Part::Rect(0, 0),
            grab: Vec2::ZERO,
            frame: s.scene.world_matrix(e),
            start: s.scene.transform(e),
            ..d
        };
        apply(&mut s, &mv, Vec2::new(0.25, 0.0));
        assert!((s.scene.world_position(e).x - 2.0).abs() < 1e-4);
    }

    #[test]
    fn box_edge_and_circle_rim_handles() {
        let mut s = test_state();
        let e = s.scene.spawn("C", None);
        s.scene.add(e, BoxCollider2D::default());
        s.scene.add(e, CircleCollider2D::default());
        let d = Drag {
            entity: e,
            part: Part::BoxEdge(0, 1),
            frame: Mat4::IDENTITY,
            start: s.scene.transform(e),
            grab: Vec2::new(0.5, 0.0),
        };
        apply(&mut s, &d, Vec2::new(1.5, 0.3));
        {
            let c = s.scene.world.get::<&BoxCollider2D>(e).unwrap();
            assert!((c.size.x - 2.0).abs() < 1e-4, "left edge stays at -0.5");
            assert!((c.common.offset.x - 0.5).abs() < 1e-4);
            assert_eq!(c.size.y, 1.0);
        }
        apply(
            &mut s,
            &Drag {
                part: Part::Rim,
                ..d
            },
            Vec2::new(0.0, 2.0),
        );
        let r = s.scene.world.get::<&CircleCollider2D>(e).unwrap().radius;
        assert!((r - 2.0).abs() < 1e-4);
    }
}
