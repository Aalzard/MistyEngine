//! Virtual camera gizmos: its frustum (and lines to its targets) in the Scene view, and the
//! Composer's dead / soft zones over the Game view while it is selected.

use egui::{Color32, Rect, Stroke, Ui, pos2};
use glam::{Mat4, Vec3};
use misty_engine::camera::{Camera as ViewCamera, Projection};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Camera, Scene};
use misty_engine::vcam::{AimKind, VirtualCamera};

use super::Lines;
use crate::state::EditorState;

const VCAM: [f32; 4] = [0.95, 0.5, 0.35, 0.9];
const TARGET: [f32; 4] = [0.95, 0.5, 0.35, 0.35];

impl Lines {
    pub(super) fn virtual_camera(&mut self, scene: &Scene, e: Entity) {
        let Ok(v) = scene.world.get::<&VirtualCamera>(e) else {
            return;
        };
        let (_, rot, pos) = scene.world_matrix(e).to_scale_rotation_translation();
        let dutch = glam::Quat::from_rotation_z(v.lens.dutch.to_radians());
        let m = Mat4::from_rotation_translation(rot * dutch, pos);
        let cam = Camera {
            fov: v.lens.fov,
            near: v.lens.near,
            far: v.lens.far,
            ..Default::default()
        };
        self.frustum(&m, &cam, VCAM);
        for t in [v.follow, v.look_at].into_iter().flatten() {
            if scene.contains(t) && t != e {
                self.line(pos, scene.world_position(t), TARGET);
            }
        }
    }
}

/// The selected virtual camera's Composer zones over the Game view image `view`: red outside
/// the soft zone, blue between soft and dead zone, clear in the dead zone, and a dot on the
/// target.
pub fn vcam_overlay(ui: &Ui, state: &EditorState, view: Rect) {
    let Some(e) = state.selection.active() else {
        return;
    };
    let Ok(v) = state
        .scene
        .world
        .get::<&VirtualCamera>(e)
        .map(|v| (*v).clone())
    else {
        return;
    };
    if v.aim != AimKind::Composer {
        return;
    }
    let (dead, soft) = v.composer.zones();
    let at = |x: f32, y: f32| {
        pos2(
            view.left() + x.clamp(0.0, 1.0) * view.width(),
            view.top() + y.clamp(0.0, 1.0) * view.height(),
        )
    };
    let rect = |r: [f32; 4]| Rect::from_min_max(at(r[0], r[1]), at(r[2], r[3]));
    let painter = ui.painter().with_clip_rect(view);
    let red = Color32::from_rgba_unmultiplied(200, 40, 40, 60);
    let blue = Color32::from_rgba_unmultiplied(40, 90, 200, 45);
    // A frame of `color` between `outer` and `inner`.
    let ring = |outer: Rect, inner: Rect, color: Color32| {
        let bands = [
            Rect::from_min_max(outer.min, pos2(outer.max.x, inner.min.y)),
            Rect::from_min_max(pos2(outer.min.x, inner.max.y), outer.max),
            Rect::from_min_max(
                pos2(outer.min.x, inner.min.y),
                pos2(inner.min.x, inner.max.y),
            ),
            Rect::from_min_max(
                pos2(inner.max.x, inner.min.y),
                pos2(outer.max.x, inner.max.y),
            ),
        ];
        for b in bands {
            if b.width() > 0.0 && b.height() > 0.0 {
                painter.rect_filled(b, 0.0, color);
            }
        }
    };
    let (soft_r, dead_r) = (rect(soft), rect(dead));
    ring(view, soft_r, red);
    ring(soft_r, dead_r, blue);
    let edge = egui::StrokeKind::Inside;
    painter.rect_stroke(
        soft_r,
        0.0,
        Stroke::new(1.0, Color32::from_rgb(90, 140, 230)),
        edge,
    );
    painter.rect_stroke(dead_r, 0.0, Stroke::new(1.0, Color32::from_gray(230)), edge);

    // The tracked point as this camera sees it.
    let Some(target) = v.look_at.filter(|t| state.scene.contains(*t)) else {
        return;
    };
    let (_, trot, tpos) = state
        .scene
        .world_matrix(target)
        .to_scale_rotation_translation();
    let point = tpos + trot * v.composer.tracked_offset;
    let (_, rot, pos) = state.scene.world_matrix(e).to_scale_rotation_translation();
    let cam = ViewCamera {
        position: pos,
        rotation: rot * glam::Quat::from_rotation_z(v.lens.dutch.to_radians()),
        projection: Projection::Perspective { fov_y: v.lens.fov },
        near: v.lens.near,
        far: v.lens.far,
    };
    let aspect = view.width() / view.height().max(1.0);
    if let Some((uv, _)) = cam.world_to_screen(aspect, point + Vec3::ZERO) {
        let p = at(uv.x, uv.y);
        painter.circle_filled(p, 4.0, Color32::from_rgb(255, 210, 60));
        painter.circle_stroke(p, 4.0, Stroke::new(1.0, Color32::BLACK));
    }
}
