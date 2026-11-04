//! Scene-view handles of a selected Rectangle light: a dot on the middle of each edge that
//! drags that edge in the light's plane. The rectangle stays centred on the light, so the
//! opposite edge moves too (Width / Height change by twice the drag).

use egui::{Color32, Pos2, Rect, Stroke, Ui};
use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Light, LightType};

use crate::picking::{ray, to_screen};
use crate::state::EditorState;

const PICK_PX: f32 = 8.0;
const MIN_SIZE: f32 = 0.01;

/// An edge handle being dragged.
#[derive(Clone, Copy, Debug)]
struct Drag {
    entity: Entity,
    /// 0 = a side edge (changes the width), 1 = top / bottom (the height).
    axis: usize,
    /// Size at the start and the axis parameter under the mouse then.
    start: f32,
    grab: f32,
}

fn drag_id() -> egui::Id {
    egui::Id::new("misty_area_light_drag")
}

/// Parameter along `p0 + s·axis` closest to the mouse ray.
fn axis_param(camera: &Camera, rect: Rect, at: Pos2, p0: Vec3, axis: Vec3) -> Option<f32> {
    let (o, d) = ray(camera, rect, at);
    let w = p0 - o;
    let b = axis.dot(d);
    let denom = 1.0 - b * b;
    (denom.abs() > 1e-4).then(|| (b * d.dot(w) - axis.dot(w)) / denom)
}

/// The active selection when it is a Rectangle light: (entity, light, centre, right, up).
fn target(state: &EditorState) -> Option<(Entity, Light, Vec3, Vec3, Vec3)> {
    let e = state.selection.active()?;
    let l = state.scene.world.get::<&Light>(e).ok()?;
    if l.kind != LightType::Rectangle {
        return None;
    }
    let (_, rot, pos) = state.scene.world_matrix(e).to_scale_rotation_translation();
    Some((e, (*l).clone(), pos, rot * Vec3::X, rot * Vec3::Y))
}

/// Draw and drag the edge handles. True while they own the mouse.
pub fn handles(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    hovered: bool,
) -> bool {
    let id = drag_id();
    let Some((e, light, center, right, up)) = target(state) else {
        ui.ctx().data_mut(|d| d.remove::<Drag>(id));
        return false;
    };
    let axes = [right, up];
    let half = [light.width * 0.5, light.height * 0.5];
    // Edge midpoints: +right, -right, +up, -up.
    let edges: Vec<(usize, f32, Pos2)> = [(0, 1.0), (0, -1.0), (1, 1.0), (1, -1.0)]
        .into_iter()
        .filter_map(|(axis, side)| {
            let p = center + axes[axis] * half[axis] * side;
            Some((axis, side, to_screen(camera, rect, p)?))
        })
        .collect();
    let (mouse, pressed, down) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
        )
    });
    let mut drag = ui.ctx().data(|d| d.get_temp::<Drag>(id));
    let hot = mouse
        .filter(|m| hovered && rect.contains(*m))
        .and_then(|m| {
            edges
                .iter()
                .filter(|h| h.2.distance(m) < PICK_PX)
                .min_by(|a, b| a.2.distance(m).total_cmp(&b.2.distance(m)))
                .copied()
        });
    if pressed
        && drag.is_none()
        && let (Some((axis, _, _)), Some(m)) = (hot, mouse)
        && let Some(grab) = axis_param(camera, rect, m, center, axes[axis])
    {
        let start = if axis == 0 { light.width } else { light.height };
        drag = Some(Drag {
            entity: e,
            axis,
            start,
            grab,
        });
    }
    if let (Some(d), Some(m)) = (drag, mouse)
        && down
        && d.entity == e
    {
        if let Some(s) = axis_param(camera, rect, m, center, axes[d.axis]) {
            // Moving away from the centre grows the rectangle on both sides.
            let outward = d.grab.signum();
            let size = (d.start + 2.0 * (s - d.grab) * outward).max(MIN_SIZE);
            let mut l = light.clone();
            if d.axis == 0 {
                l.width = size;
            } else {
                l.height = size;
            }
            if l.width != light.width || l.height != light.height {
                let _ = state.scene.world.insert_one(e, l);
                state.mark_edited("Light Size");
            }
        }
        ui.ctx().data_mut(|data| data.insert_temp(id, d));
    } else {
        ui.ctx().data_mut(|data| data.remove::<Drag>(id));
        drag = None;
    }

    let painter = ui.painter().with_clip_rect(rect);
    for (axis, side, at) in &edges {
        let active = drag.is_some_and(|d| d.axis == *axis)
            || hot.is_some_and(|h| h.0 == *axis && h.1 == *side);
        let color = if active {
            Color32::WHITE
        } else {
            Color32::from_rgb(255, 235, 115)
        };
        painter.circle_filled(*at, 4.5, color);
        painter.circle_stroke(*at, 4.5, Stroke::new(1.0, Color32::BLACK));
    }
    hot.is_some() || drag.is_some()
}
