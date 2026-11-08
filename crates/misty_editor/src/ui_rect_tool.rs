//! The Rect tool on a selected UI object in the Scene view: drag a corner or an edge to resize
//! (changing the Rect Transform's offsets), inside to move, just outside a corner to rotate
//! (the Transform's z rotation, Shift snaps to 15°), the blue circle to move the pivot (the rect
//! stays put) and the white triangles to move the anchors (the rect stays put; anchors that
//! meet move together). A World Space root canvas resizes its own rect and moves by its
//! Transform. Drag state lives in egui's memory.

use egui::{Color32, Pos2, Rect, Stroke, Ui, vec2};
use glam::{Affine2, Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Canvas, CanvasRenderMode, RectTransform, Transform};
use misty_engine::ui::{UiLayout, UiNode};

use crate::state::{EditorState, Tool};
use crate::ui_gizmos::{anchor_points, screen_corners, to_canvas, to_screen};

const PICK_PX: f32 = 7.0;
/// How far outside a corner the rotate zone reaches.
const ROTATE_PX: f32 = 22.0;
const RECT_COLOR: Color32 = Color32::from_rgb(90, 165, 255);

/// What a handle changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    /// Which sides move per axis (-1 = left / bottom, 1 = right / top, 0 = neither).
    Resize(i8, i8),
    Move,
    Rotate,
    Pivot,
    /// An anchor corner (-1 = min, 1 = max per axis, y up).
    Anchor(i8, i8),
}

#[derive(Clone, Debug)]
struct Drag {
    entity: Entity,
    part: Part,
    start_rt: RectTransform,
    start_t: Transform,
    /// Pointer at the start (canvas pixels).
    grab: Vec2,
    /// Canvas pixels → the object's own frame (resize, pivot).
    own_inv: Affine2,
    /// Canvas pixels → its parent's frame (move, anchors).
    parent_inv: Affine2,
    /// The object's unrotated rect (canvas pixels) and pixels per canvas unit.
    screen: misty_engine::ui::Rect,
    scale: f32,
    /// Parent rect (its frame) and size in canvas units.
    parent_screen: misty_engine::ui::Rect,
    parent_units: Vec2,
    /// Pivot as drawn (canvas pixels): rotation centre.
    pivot: Vec2,
    /// World Space root canvas: canvas pixels → world.
    world_root: Option<glam::Mat4>,
    /// Where the canvas was at the start: the pointer is read in these canvas pixels, so a
    /// World Space canvas being moved doesn't drag its own frame along.
    place: misty_engine::ui::CanvasPlacement,
    /// Image size the placement maps to (the view's, in points).
    target: Vec2,
}

fn drag_id() -> egui::Id {
    egui::Id::new("misty_ui_rect_tool_drag")
}

/// Is `e` a root canvas (its rect driven by the canvas, except World Space)?
fn root_canvas(state: &EditorState, e: Entity) -> Option<CanvasRenderMode> {
    let c = state.scene.world.get::<&Canvas>(e).ok()?;
    let nested = state
        .scene
        .parent(e)
        .is_some_and(|p| misty_engine::ui::canvas_of(&state.scene, p).is_some());
    (!nested).then_some(c.render_mode)
}

/// The selected UI object the Rect tool works on.
fn target<'a>(state: &EditorState, l: &'a UiLayout) -> Option<&'a UiNode> {
    if state.tool != Tool::Rect {
        return None;
    }
    let e = state.selection.active()?;
    state.scene.world.get::<&RectTransform>(e).ok()?;
    match root_canvas(state, e) {
        Some(CanvasRenderMode::WorldSpace) | None => l.node(e),
        Some(_) => None,
    }
}

/// Handle positions of `n` in canvas pixels.
pub fn handles(state: &EditorState, l: &UiLayout, n: &UiNode) -> Vec<(Part, Vec2)> {
    let s = n.screen;
    let mut out = Vec::new();
    for sy in [1i8, 0, -1] {
        for sx in [-1i8, 0, 1] {
            if sx == 0 && sy == 0 {
                continue;
            }
            let x = match sx {
                -1 => s.min.x,
                1 => s.max.x,
                _ => s.center().x,
            };
            // Canvas pixels run down: the top edge is the min.
            let y = match sy {
                1 => s.min.y,
                -1 => s.max.y,
                _ => s.center().y,
            };
            out.push((Part::Resize(sx, sy), n.xf.transform_point2(Vec2::new(x, y))));
        }
    }
    let pivot = state
        .scene
        .world
        .get::<&RectTransform>(n.entity)
        .map_or(Vec2::splat(0.5), |rt| rt.pivot);
    out.push((Part::Pivot, pivot_point(n, pivot)));
    if let Some(a) = anchor_points(&state.scene, l, n) {
        for (p, (sx, sy)) in a.into_iter().zip([(-1, -1), (1, -1), (-1, 1), (1, 1)]) {
            out.push((Part::Anchor(sx, sy), p));
        }
    }
    out
}

/// The pivot of `n` as drawn (canvas pixels).
fn pivot_point(n: &UiNode, pivot: Vec2) -> Vec2 {
    let s = n.screen;
    n.xf.transform_point2(Vec2::new(
        s.min.x + s.size().x * pivot.x,
        s.max.y - s.size().y * pivot.y,
    ))
}

/// The handle under the mouse (`at`, screen points).
fn hot_part(state: &EditorState, l: &UiLayout, rect: Rect, n: &UiNode, at: Pos2) -> Option<Part> {
    let list = handles(state, l, n);
    let near = |part: &dyn Fn(Part) -> bool| {
        list.iter()
            .filter(|(p, _)| part(*p))
            .filter_map(|(p, q)| Some((*p, to_screen(l, rect, n.canvas, *q)?.distance(at))))
            .filter(|(_, d)| *d < PICK_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p)
    };
    // The pivot sits over the move area and the anchors may sit on a corner: they go first.
    if let Some(p) = near(&|p| p == Part::Pivot) {
        return Some(p);
    }
    if let Some(p) = near(&|p| matches!(p, Part::Resize(..))) {
        return Some(p);
    }
    if let Some(p) = near(&|p| matches!(p, Part::Anchor(..))) {
        return Some(p);
    }
    let q = to_canvas(l, rect, n.canvas, at)?;
    let inside = n.screen.contains(n.local(q));
    if inside {
        return Some(Part::Move);
    }
    // Just outside a corner: rotate.
    let corners = screen_corners(l, rect, n)?;
    corners
        .iter()
        .any(|c| c.distance(at) < ROTATE_PX)
        .then_some(Part::Rotate)
}

/// Draw and drive the Rect tool on a selected UI object. True when it owns the mouse this
/// frame (a handle is hot or being dragged).
pub fn show(ui: &Ui, state: &mut EditorState, rect: Rect, hovered: bool, l: &UiLayout) -> bool {
    let id = drag_id();
    let Some(n) = target(state, l).copied() else {
        ui.ctx().data_mut(|d| d.remove::<Drag>(id));
        return false;
    };
    let (mouse, pressed, down, shift) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
        )
    });
    let drag = ui.ctx().data(|d| d.get_temp::<Drag>(id));
    let hot = match &drag {
        Some(d) => Some(d.part),
        None => mouse
            .filter(|m| hovered && rect.contains(*m))
            .and_then(|m| hot_part(state, l, rect, &n, m)),
    };
    let mut owned = hot.is_some();
    if pressed
        && drag.is_none()
        && let (Some(part), Some(m)) = (hot, mouse)
        && let Some(d) = start(state, l, rect, &n, part, m)
    {
        ui.ctx().data_mut(|data| data.insert_temp(id, d));
        state.handles.suppress_click = true;
    }
    if let Some(d) = ui.ctx().data(|data| data.get_temp::<Drag>(id)) {
        owned = true;
        if down && d.entity == n.entity {
            let read = |m: Pos2| {
                d.place
                    .to_canvas(Vec2::new(m.x - rect.min.x, m.y - rect.min.y), d.target)
            };
            if let Some(q) = mouse.and_then(read) {
                apply(state, &d, q, shift);
                ui.ctx().request_repaint();
            }
        } else {
            ui.ctx().data_mut(|data| data.remove::<Drag>(id));
        }
    }
    paint(ui, rect, state, l, &n, hot);
    owned
}

fn start(
    state: &EditorState,
    l: &UiLayout,
    rect: Rect,
    n: &UiNode,
    part: Part,
    at: Pos2,
) -> Option<Drag> {
    let e = n.entity;
    let grab = to_canvas(l, rect, n.canvas, at)?;
    let rt = *state.scene.world.get::<&RectTransform>(e).ok()?;
    let parent = state.scene.parent(e).and_then(|p| l.node(p));
    let world_root = match root_canvas(state, e) {
        Some(CanvasRenderMode::WorldSpace) => l.placement(e).and_then(|p| p.to_world),
        _ => None,
    };
    Some(Drag {
        entity: e,
        part,
        start_rt: rt,
        start_t: state.scene.transform(e),
        grab,
        own_inv: n.xf.inverse(),
        parent_inv: parent.map_or(Affine2::IDENTITY, |p| p.xf.inverse()),
        screen: n.screen,
        scale: n.scale,
        parent_screen: parent.map_or(n.screen, |p| p.screen),
        parent_units: parent.map_or(n.rect.size(), |p| p.rect.size()),
        pivot: pivot_point(n, rt.pivot),
        world_root,
        place: *l.placement(n.canvas)?,
        target: l.target,
    })
}

/// Apply a drag with the pointer at canvas pixel `q`.
fn apply(state: &mut EditorState, d: &Drag, q: Vec2, shift: bool) {
    let e = d.entity;
    let mut rt = d.start_rt;
    // Pixel deltas → canvas units (y up).
    let units = |v: Vec2| Vec2::new(v.x, -v.y) / d.scale.max(1e-6);
    let name = state.scene.name(e);
    match d.part {
        Part::Move => {
            let delta =
                units(d.parent_inv.transform_point2(q) - d.parent_inv.transform_point2(d.grab));
            if let Some(m) = d.world_root {
                let v = q - d.grab;
                let w = m.transform_vector3(Vec3::new(v.x, v.y, 0.0));
                state.scene.set_transform(e, d.start_t);
                let start = state.scene.world_position(e);
                state.scene.set_world_position(e, start + w);
            } else {
                rt.anchored_position = d.start_rt.anchored_position + delta;
            }
            state.mark_edited(&format!("Move {name}"));
        }
        Part::Resize(sx, sy) => {
            let mut delta =
                units(d.own_inv.transform_point2(q) - d.own_inv.transform_point2(d.grab));
            let size = d.screen.size() / d.scale.max(1e-6);
            // Shift on a corner keeps the width / height ratio.
            if shift && sx != 0 && sy != 0 && size.y > 0.0 {
                let ratio = size.x / size.y;
                let grow = (delta.x * sx as f32 / ratio).max(delta.y * sy as f32);
                delta = Vec2::new(grow * ratio * sx as f32, grow * sy as f32);
            }
            let (mut omin, mut omax) = (rt.offset_min(), rt.offset_max());
            // Never turn the rect inside out.
            let dx = match sx {
                -1 => delta.x.min(size.x),
                1 => delta.x.max(-size.x),
                _ => 0.0,
            };
            let dy = match sy {
                -1 => delta.y.min(size.y),
                1 => delta.y.max(-size.y),
                _ => 0.0,
            };
            if sx == -1 {
                omin.x += dx;
            } else if sx == 1 {
                omax.x += dx;
            }
            if sy == -1 {
                omin.y += dy;
            } else if sy == 1 {
                omax.y += dy;
            }
            if let Some(m) = d.world_root {
                // A world canvas: its size changes and its Transform follows the moved pivot.
                let (lo, hi) = (-size * rt.pivot, size * (Vec2::ONE - rt.pivot));
                let pick = |side: i8, want: i8, v: f32| if side == want { v } else { 0.0 };
                let new_lo = lo + Vec2::new(pick(sx, -1, dx), pick(sy, -1, dy));
                let new_hi = hi + Vec2::new(pick(sx, 1, dx), pick(sy, 1, dy));
                let new_size = (new_hi - new_lo).max(Vec2::ONE);
                let moved = new_lo + new_size * rt.pivot;
                rt.size_delta = new_size;
                let w = m.transform_vector3(Vec3::new(moved.x * d.scale, -moved.y * d.scale, 0.0));
                state.scene.set_transform(e, d.start_t);
                let p = state.scene.world_position(e);
                state.scene.set_world_position(e, p + w);
            } else {
                rt.set_offsets(omin, omax);
            }
            state.mark_edited(&format!("Resize {name}"));
        }
        Part::Rotate => {
            let a0 = (d.grab - d.pivot).to_angle();
            let a1 = (q - d.pivot).to_angle();
            // Canvas pixels run down: a turn that looks clockwise is a negative z rotation.
            let start = misty_engine::camera::quat_to_euler(d.start_t.rotation);
            let mut z = start.z - (a1 - a0).to_degrees();
            if shift {
                z = (z / 15.0).round() * 15.0;
            }
            let t = Transform {
                rotation: misty_engine::camera::euler_to_quat(Vec3::new(start.x, start.y, z)),
                ..d.start_t
            };
            state.scene.set_transform(e, t);
            state.mark_edited(&format!("Rotate {name}"));
            return;
        }
        Part::Pivot => {
            let p = d.own_inv.transform_point2(q);
            let s = d.screen;
            let size = s.size().max(Vec2::splat(1e-3));
            let mut pivot = Vec2::new((p.x - s.min.x) / size.x, (s.max.y - p.y) / size.y);
            if shift {
                pivot = (pivot * 2.0).round() / 2.0;
            }
            rt.set_pivot_keep_rect(pivot);
            state.mark_edited(&format!("Set Pivot of {name}"));
        }
        Part::Anchor(sx, sy) => {
            let p = d.parent_inv.transform_point2(q);
            let s = d.parent_screen;
            let size = s.size().max(Vec2::splat(1e-3));
            let a = Vec2::new((p.x - s.min.x) / size.x, (s.max.y - p.y) / size.y)
                .clamp(Vec2::ZERO, Vec2::ONE);
            let (mut lo, mut hi) = (d.start_rt.anchor_min, d.start_rt.anchor_max);
            for (axis, side) in [(0, sx), (1, sy)] {
                if (lo[axis] - hi[axis]).abs() < 1e-5 {
                    lo[axis] = a[axis];
                    hi[axis] = a[axis];
                } else if side < 0 {
                    lo[axis] = a[axis].min(hi[axis]);
                } else {
                    hi[axis] = a[axis].max(lo[axis]);
                }
            }
            rt.set_anchors_keep_rect(d.parent_units, lo, hi);
            state.mark_edited(&format!("Set Anchors of {name}"));
        }
    }
    if let Ok(mut c) = state.scene.world.get::<&mut RectTransform>(e) {
        *c = rt;
    }
}

fn paint(ui: &Ui, rect: Rect, state: &EditorState, l: &UiLayout, n: &UiNode, hot: Option<Part>) {
    let painter = ui.painter().with_clip_rect(rect);
    if let Some(c) = screen_corners(l, rect, n) {
        for i in 0..4 {
            painter.line_segment([c[i], c[(i + 1) % 4]], Stroke::new(1.5, RECT_COLOR));
        }
    }
    let fill = |p: Part| {
        if Some(p) == hot {
            Color32::from_rgb(255, 230, 0)
        } else {
            Color32::WHITE
        }
    };
    for (part, q) in handles(state, l, n) {
        let Some(s) = to_screen(l, rect, n.canvas, q) else {
            continue;
        };
        match part {
            Part::Resize(..) => {
                let r = Rect::from_center_size(s, vec2(7.0, 7.0));
                painter.rect_filled(r, 1.0, fill(part));
                painter.rect_stroke(
                    r,
                    1.0,
                    Stroke::new(1.0, RECT_COLOR),
                    egui::StrokeKind::Outside,
                );
            }
            Part::Pivot => {
                painter.circle_stroke(s, 5.0, Stroke::new(2.0, RECT_COLOR));
                if hot == Some(Part::Pivot) {
                    painter.circle_filled(s, 3.0, fill(part));
                }
            }
            Part::Anchor(sx, sy) => {
                // A small triangle pointing away from the rect's corner it belongs to.
                let dir = vec2(sx as f32, -sy as f32) * 7.0;
                let tri = vec![
                    s,
                    s + vec2(dir.x, 0.0) + vec2(0.0, dir.y * 0.4),
                    s + vec2(dir.x * 0.4, dir.y),
                ];
                painter.add(egui::Shape::convex_polygon(
                    tri,
                    fill(part),
                    Stroke::new(1.0, Color32::from_gray(40)),
                ));
            }
            _ => {}
        }
    }
    if hot == Some(Part::Rotate)
        && let Some(m) = ui.input(|i| i.pointer.hover_pos())
    {
        painter.circle_stroke(m, 8.0, Stroke::new(1.5, RECT_COLOR));
    }
}

/// Screen points of `e`'s Rect tool handles (for tests).
#[cfg(test)]
pub fn handle_screen(
    state: &EditorState,
    camera: &misty_engine::camera::Camera,
    rect: Rect,
    e: Entity,
) -> Vec<(Part, Pos2)> {
    let l = crate::ui_gizmos::scene_layout(state, camera, rect);
    let Some(n) = l.node(e) else {
        return Vec::new();
    };
    handles(state, &l, n)
        .into_iter()
        .filter_map(|(p, q)| Some((p, to_screen(&l, rect, n.canvas, q)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton, Pos2};
    use egui_kittest::Harness;
    use glam::{Vec2, Vec3};
    use misty_engine::hecs::Entity;
    use misty_engine::scene::{Canvas, CanvasScaler, RectTransform, UiImage, UiScaleMode};

    use super::{Part, handle_screen};
    use crate::project::ProjectBrowser;
    use crate::state::{EditorState, Tool};

    /// The Scene view in 2D looking at a 200×100 overlay canvas (one unit per pixel) with a
    /// selected 100×50 image in the middle, Rect tool on.
    fn harness() -> (Harness<'static, EditorState>, Entity) {
        let root = std::env::temp_dir().join("misty_ui_rect_tool_test");
        let mut state = EditorState::new(ProjectBrowser::open(root));
        state.game_view.screen_px = Vec2::new(200.0, 100.0);
        let canvas = state.scene.spawn("Canvas", None);
        state.scene.add(canvas, RectTransform::stretched());
        state.scene.add(canvas, Canvas::default());
        state.scene.add(
            canvas,
            CanvasScaler {
                mode: UiScaleMode::ConstantPixelSize,
                ..Default::default()
            },
        );
        let image = state.scene.spawn("Image", Some(canvas));
        state.scene.add(image, RectTransform::sized(100.0, 50.0));
        state.scene.add(image, UiImage::default());
        state.selection.set(image);
        state.tool = Tool::Rect;
        state.scene_view.is_2d = true;
        state.scene_view.gizmos = true;
        let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| crate::panels::scene::show(ui, s, None),
            state,
        );
        h.run_steps(2);
        h.state_mut()
            .scene_view
            .camera
            .frame(Vec3::new(100.0, 50.0, 0.0), 60.0);
        h.run_steps(120);
        (h, image)
    }

    fn handle(h: &Harness<'_, EditorState>, e: Entity, part: Part) -> Pos2 {
        let s = h.state();
        let cam = s.scene_view.camera.camera();
        handle_screen(s, &cam, s.scene_view.view_rect, e)
            .into_iter()
            .find(|(p, _)| *p == part)
            .map(|(_, q)| q)
            .unwrap_or_else(|| panic!("no {part:?} handle"))
    }

    fn drag(h: &mut Harness<'_, EditorState>, from: Pos2, to: Pos2, mods: Modifiers) {
        let press = |h: &mut Harness<'_, EditorState>, pos: Pos2, pressed: bool| {
            h.event(Event::ModifiersChanged(mods));
            h.event(Event::PointerMoved(pos));
            h.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: mods,
            });
            h.step();
        };
        press(h, from, true);
        for i in 1..=6 {
            h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
            h.step();
        }
        press(h, to, false);
        h.run_steps(2);
    }

    fn rt(h: &Harness<'_, EditorState>, e: Entity) -> RectTransform {
        *h.state().scene.world.get::<&RectTransform>(e).unwrap()
    }

    /// The image's rect in canvas units.
    fn rect_of(h: &Harness<'_, EditorState>, e: Entity) -> misty_engine::ui::Rect {
        misty_engine::ui::layout(&h.state().scene, Vec2::new(200.0, 100.0))
            .node(e)
            .unwrap()
            .rect
    }

    #[test]
    fn corner_handle_resizes_and_keeps_the_opposite_corner() {
        let (mut h, image) = harness();
        let before = rect_of(&h, image);
        let tr = handle(&h, image, Part::Resize(1, 1));
        drag(&mut h, tr, tr + egui::vec2(60.0, -40.0), Modifiers::NONE);
        let after = rect_of(&h, image);
        assert!(after.max.x > before.max.x + 1.0, "{before:?} → {after:?}");
        assert!(after.max.y > before.max.y + 1.0, "dragged up grows the top");
        assert!(after.min.distance(before.min) < 1e-3, "bottom-left stays");
        assert_eq!(h.state().selection.active(), Some(image));
        assert!(h.state().scene_dirty, "an edit (named for Undo)");
    }

    #[test]
    fn dragging_inside_moves_and_edges_resize_one_axis() {
        let (mut h, image) = harness();
        let size = rect_of(&h, image).size();
        let left = handle(&h, image, Part::Resize(-1, 0));
        let mid = left + egui::vec2(25.0, 6.0);
        drag(&mut h, mid, mid + egui::vec2(50.0, 0.0), Modifiers::NONE);
        let r = rt(&h, image);
        assert!(r.anchored_position.x > 1.0, "{r:?}");
        assert!(r.anchored_position.y.abs() < 1e-3);
        assert!(
            rect_of(&h, image).size().distance(size) < 1e-3,
            "moving keeps the size"
        );
        let left = handle(&h, image, Part::Resize(-1, 0));
        drag(
            &mut h,
            left,
            left + egui::vec2(-30.0, 25.0),
            Modifiers::NONE,
        );
        let after = rect_of(&h, image).size();
        assert!(
            after.x > size.x + 1.0 && (after.y - size.y).abs() < 1e-3,
            "{after}"
        );
    }

    #[test]
    fn pivot_and_anchor_handles_keep_the_rect() {
        let (mut h, image) = harness();
        let before = rect_of(&h, image);
        let pivot = handle(&h, image, Part::Pivot);
        drag(
            &mut h,
            pivot,
            pivot + egui::vec2(-60.0, 0.0),
            Modifiers::NONE,
        );
        let r = rt(&h, image);
        assert!(r.pivot.x < 0.4, "pivot moved left: {r:?}");
        let after = rect_of(&h, image);
        assert!(after.min.distance(before.min) < 1e-2 && after.max.distance(before.max) < 1e-2);
        // The anchors start together in the middle: dragging them moves both.
        let anchor = handle(&h, image, Part::Anchor(1, 1));
        drag(
            &mut h,
            anchor,
            anchor + egui::vec2(80.0, -60.0),
            Modifiers::NONE,
        );
        let r = rt(&h, image);
        assert!(
            r.anchor_min.x > 0.55 && r.anchor_min == r.anchor_max,
            "{r:?}"
        );
        let moved = rect_of(&h, image);
        assert!(
            moved.min.distance(before.min) < 1e-2,
            "{before:?} → {moved:?}"
        );
    }

    #[test]
    fn outside_a_corner_rotates_and_shift_snaps() {
        let (mut h, image) = harness();
        let tr = handle(&h, image, Part::Resize(1, 1));
        let from = tr + egui::vec2(12.0, -12.0);
        // Swing the corner round the pivot (downwards on screen = clockwise).
        let pivot = handle(&h, image, Part::Pivot);
        let to = pivot + (from - pivot).rot90() * -1.0;
        drag(&mut h, from, to, Modifiers::SHIFT);
        let z = misty_engine::camera::quat_to_euler(h.state().scene.transform(image).rotation).z;
        assert!(z.abs() > 10.0, "rotated: {z}");
        assert!(
            (z / 15.0 - (z / 15.0).round()).abs() < 1e-3,
            "snapped to 15°: {z}"
        );
        // The drawn rect turned with it (the corners no longer line up with the axes).
        let l = misty_engine::ui::layout(&h.state().scene, Vec2::new(200.0, 100.0));
        let c = l.node(image).unwrap().corners();
        assert!((c[0].y - c[1].y).abs() > 1.0, "{c:?}");
    }
}
