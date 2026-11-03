//! Scene-view gizmo of a selected Fog Volume: its box, plus the inner box where the soft edge
//! (Blend Distance) ends.

use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{FogVolume, Scene};

use super::Lines;

const FOG: [f32; 4] = [0.75, 0.85, 1.0, 0.9];
const FOG_INNER: [f32; 4] = [0.75, 0.85, 1.0, 0.35];

impl Lines {
    pub(super) fn fog_volume(&mut self, scene: &Scene, e: Entity) {
        let Ok(v) = scene.world.get::<&FogVolume>(e) else {
            return;
        };
        let m = scene.world_matrix(e);
        let half = Vec3::from(v.size).abs() * 0.5;
        self.wire_box(&m, Vec3::ZERO, half, FOG);
        if v.blend_distance > 0.0 {
            // The blend width is in metres: shrink each axis by it in local units.
            let (scale, _, _) = m.to_scale_rotation_translation();
            let inner = (half - Vec3::splat(v.blend_distance) / scale.abs().max(Vec3::splat(1e-4)))
                .max(Vec3::ZERO);
            self.wire_box(&m, Vec3::ZERO, inner, FOG_INNER);
        }
    }
}
