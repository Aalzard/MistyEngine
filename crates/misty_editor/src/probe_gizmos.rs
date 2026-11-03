//! Reflection probe gizmos: a small chrome sphere at each probe (it reflects what the probes
//! captured), the influence box and inner blend box of selected probes (all boxes in the
//! Reflection Probes draw mode) and, with Edit Bounding Volume on, a handle on each box face
//! to drag it along its axis.

use egui::{Color32, Pos2, Rect, Stroke, Ui, vec2};
use glam::{Mat4, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::render::probes;
use misty_engine::render::{MaterialParams, MeshDraw};
use misty_engine::scene::{PrimitiveMesh, ReflectionProbe};

use crate::gizmos::Lines;
use crate::picking::{ray, to_screen};
use crate::state::EditorState;

const BOX: [f32; 4] = [1.0, 0.85, 0.35, 0.9];
const BLEND: [f32; 4] = [0.45, 0.75, 1.0, 0.6];
const SPHERE_RADIUS: f32 = 0.25;
const PICK_PX: f32 = 8.0;

/// Wire box between two corners.
fn wire_box(lines: &mut Lines, lo: Vec3, hi: Vec3, color: [f32; 4]) {
    let c = |i: usize| {
        Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    };
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                lines.line(c(i), c(i | bit), color);
            }
        }
    }
}

/// Boxes of the selected probes, or of every probe with `all`.
pub fn lines(lines: &mut Lines, state: &EditorState, all: bool) {
    for (e, p) in probes::scene_probe_list(&state.scene) {
        if !all && !state.selection.contains(e) {
            continue;
        }
        let (lo, hi) = p.world_box(state.scene.world_position(e));
        wire_box(lines, lo, hi, BOX);
        let b = Vec3::splat(p.blend_distance.max(0.0));
        if p.blend_distance > 0.0 && (hi - lo).min_element() > 2.0 * p.blend_distance {
            wire_box(lines, lo + b, hi - b, BLEND);
        }
    }
}

/// A chrome sphere per probe, drawn in the Scene view only.
pub fn spheres(state: &EditorState, meshes: &mut Vec<MeshDraw>) {
    let chrome = MaterialParams {
        color: [1.0; 4],
        metallic: 1.0,
        smoothness: 1.0,
        ..Default::default()
    };
    for (e, _) in probes::scene_probe_list(&state.scene) {
        let at = state.scene.world_position(e);
        meshes.push(MeshDraw {
            mesh: PrimitiveMesh::Sphere.into(),
            model: Mat4::from_scale_rotation_translation(
                Vec3::splat(SPHERE_RADIUS * 2.0),
                glam::Quat::IDENTITY,
                at,
            ),
            material: chrome.clone(),
            cast_shadows: false,
            receive_shadows: false,
            outline: state.selection.contains(e),
            sorting: Default::default(),
        });
    }
}

/// A face handle being dragged.
#[derive(Clone, Copy, Debug)]
struct Drag {
    entity: Entity,
    axis: usize,
    /// -1 = min face, 1 = max face.
    side: f32,
    start: (Vec3, Vec3),
    /// Axis parameter under the mouse at the start.
    grab: f32,
}

fn drag_id() -> egui::Id {
    egui::Id::new("misty_probe_bounds_drag")
}

/// Parameter along the line `p0 + s·axis` closest to the mouse ray.
fn axis_param(camera: &Camera, rect: Rect, at: Pos2, p0: Vec3, axis: Vec3) -> Option<f32> {
    let (o, d) = ray(camera, rect, at);
    let w = p0 - o;
    let b = axis.dot(d);
    let denom = 1.0 - b * b;
    (denom.abs() > 1e-4).then(|| (b * d.dot(w) - axis.dot(w)) / denom)
}

fn face_center(lo: Vec3, hi: Vec3, axis: usize, side: f32) -> Vec3 {
    let mut c = (lo + hi) * 0.5;
    c[axis] = if side < 0.0 { lo[axis] } else { hi[axis] };
    c
}

/// Edit Bounding Volume handles of the selected probe. True while they own the mouse.
pub fn handles(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    hovered: bool,
) -> bool {
    let id = drag_id();
    let target = state
        .selection
        .active()
        .filter(|_| state.probes.edit_bounds)
        .and_then(|e| {
            let p = state.scene.world.get::<&ReflectionProbe>(e).ok()?;
            Some((e, (*p).clone()))
        });
    let Some((e, probe)) = target else {
        ui.ctx().data_mut(|d| d.remove::<Drag>(id));
        return false;
    };
    let pos = state.scene.world_position(e);
    let (lo, hi) = probe.world_box(pos);
    let (mouse, pressed, down) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
        )
    });
    let faces: Vec<(usize, f32, Pos2)> = (0..3)
        .flat_map(|axis| [(axis, -1.0), (axis, 1.0)])
        .filter_map(|(axis, side)| {
            Some((
                axis,
                side,
                to_screen(camera, rect, face_center(lo, hi, axis, side))?,
            ))
        })
        .collect();
    let mut drag = ui.ctx().data(|d| d.get_temp::<Drag>(id));
    let hot = mouse
        .filter(|m| hovered && rect.contains(*m))
        .and_then(|m| {
            faces
                .iter()
                .filter(|f| f.2.distance(m) < PICK_PX)
                .min_by(|a, b| a.2.distance(m).total_cmp(&b.2.distance(m)))
                .copied()
        });
    if pressed
        && drag.is_none()
        && let (Some((axis, side, _)), Some(m)) = (hot, mouse)
    {
        let mut dir = Vec3::ZERO;
        dir[axis] = 1.0;
        let p0 = face_center(lo, hi, axis, side);
        if let Some(grab) = axis_param(camera, rect, m, p0, dir) {
            drag = Some(Drag {
                entity: e,
                axis,
                side,
                start: (lo, hi),
                grab,
            });
        }
    }
    if let (Some(d), Some(m)) = (drag, mouse)
        && down
        && d.entity == e
    {
        let mut dir = Vec3::ZERO;
        dir[d.axis] = 1.0;
        let p0 = face_center(d.start.0, d.start.1, d.axis, d.side);
        if let Some(s) = axis_param(camera, rect, m, p0, dir) {
            let (mut lo, mut hi) = d.start;
            let moved = s - d.grab;
            if d.side < 0.0 {
                lo[d.axis] = (lo[d.axis] + moved).min(hi[d.axis] - 0.05);
            } else {
                hi[d.axis] = (hi[d.axis] + moved).max(lo[d.axis] + 0.05);
            }
            let mut p = probe.clone();
            p.size = hi - lo;
            p.center = (lo + hi) * 0.5 - pos;
            if p != probe {
                let _ = state.scene.world.insert_one(e, p);
                state.mark_edited("Reflection Probe Bounds");
            }
        }
        ui.ctx().data_mut(|data| data.insert_temp(id, d));
    } else {
        ui.ctx().data_mut(|data| data.remove::<Drag>(id));
        drag = None;
    }

    let painter = ui.painter().with_clip_rect(rect);
    for (axis, side, at) in &faces {
        let active = drag.is_some_and(|d| d.axis == *axis && d.side == *side)
            || hot.is_some_and(|h| h.0 == *axis && h.1 == *side);
        let color = match axis {
            0 => crate::theme::AXIS_X,
            1 => crate::theme::AXIS_Y,
            _ => crate::theme::AXIS_Z,
        };
        let r = Rect::from_center_size(*at, vec2(9.0, 9.0));
        painter.rect_filled(r, 1.0, if active { Color32::WHITE } else { color });
        painter.rect_stroke(
            r,
            1.0,
            Stroke::new(1.0, Color32::BLACK),
            egui::StrokeKind::Outside,
        );
    }
    hot.is_some() || drag.is_some()
}
