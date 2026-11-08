//! Scene-view gizmo of a selected Decal Projector: its box, and an arrow from the box's near
//! face along the projection (local +Z), like Unity's.

use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{DecalProjector, Scene};

use super::Lines;

const DECAL: [f32; 4] = [0.55, 1.0, 0.6, 0.9];
const DECAL_ARROW: [f32; 4] = [0.55, 1.0, 0.6, 0.6];

impl Lines {
    pub(super) fn decal_projector(&mut self, scene: &Scene, e: Entity) {
        let Ok(d) = scene.world.get::<&DecalProjector>(e) else {
            return;
        };
        let m = scene.world_matrix(e) * d.box_matrix();
        self.wire_box(&m, Vec3::ZERO, Vec3::splat(0.5), DECAL);
        let p = |v: Vec3| m.transform_point3(v);
        // From the middle of the near face to the far face, with a head.
        let (from, to) = (p(Vec3::new(0.0, 0.0, -0.5)), p(Vec3::new(0.0, 0.0, 0.5)));
        self.line(from, to, DECAL_ARROW);
        for side in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y] {
            self.line(to, p(Vec3::new(0.0, 0.0, 0.3) + side * 0.12), DECAL_ARROW);
        }
        // A chevron on the box's top edge: the decal's up (the texture's top).
        self.line(
            p(Vec3::new(-0.15, 0.5, -0.5)),
            p(Vec3::new(0.0, 0.62, -0.5)),
            DECAL,
        );
        self.line(
            p(Vec3::new(0.0, 0.62, -0.5)),
            p(Vec3::new(0.15, 0.5, -0.5)),
            DECAL,
        );
    }
}
