//! Scene-view outlines (green) of the selected object's 2D colliders, in the same frame the 2D
//! physics world uses: world position, Z angle, sizes times the world X/Y scale.

use glam::{Mat4, Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::physics2d::{collider_frame, polygon_points};
use misty_engine::scene::{
    BoxCollider2D, CapsuleCollider2D, CapsuleDirection2D, CircleCollider2D, EdgeCollider2D,
    PolygonCollider2D, Scene,
};

use super::{COLLIDER, Lines};

/// Segments of a full circle.
const SEGMENTS: usize = 32;

impl Lines {
    /// Closed or open chain of 2D points through the frame `m`.
    fn chain_2d(&mut self, m: &Mat4, pts: &[Vec2], closed: bool) {
        let w: Vec<Vec3> = pts
            .iter()
            .map(|p| m.transform_point3(p.extend(0.0)))
            .collect();
        for i in 1..w.len() {
            self.line(w[i - 1], w[i], COLLIDER);
        }
        if closed && w.len() > 2 {
            self.line(w[w.len() - 1], w[0], COLLIDER);
        }
    }

    /// Arc around `c` from angle `a0` (radians) over `sweep`.
    fn arc_2d(&mut self, m: &Mat4, c: Vec2, r: f32, a0: f32, sweep: f32) {
        let n = ((SEGMENTS as f32 * sweep.abs() / std::f32::consts::TAU).ceil() as usize).max(2);
        let pts: Vec<Vec2> = (0..=n)
            .map(|i| c + Vec2::from_angle(a0 + sweep * i as f32 / n as f32) * r)
            .collect();
        self.chain_2d(m, &pts, false);
    }

    /// Outlines of `e`'s enabled 2D colliders.
    pub(super) fn colliders_2d(&mut self, scene: &Scene, e: Entity) {
        let w = &scene.world;
        let (m, s) = collider_frame(scene, e);
        if let Ok(c) = w.get::<&BoxCollider2D>(e)
            && c.common.enabled
        {
            let o = c.common.offset * s;
            let h = c.size * s * 0.5 + Vec2::splat(c.edge_radius);
            let pts = [
                o + Vec2::new(-h.x, -h.y),
                o + Vec2::new(h.x, -h.y),
                o + h,
                o + Vec2::new(-h.x, h.y),
            ];
            self.chain_2d(&m, &pts, true);
        }
        if let Ok(c) = w.get::<&CircleCollider2D>(e)
            && c.common.enabled
        {
            let r = c.radius * s.max_element();
            self.arc_2d(&m, c.common.offset * s, r, 0.0, std::f32::consts::TAU);
        }
        if let Ok(c) = w.get::<&CapsuleCollider2D>(e)
            && c.common.enabled
        {
            let o = c.common.offset * s;
            let size = c.size * s;
            let pi = std::f32::consts::PI;
            let (axis, r, half) = match c.direction {
                CapsuleDirection2D::Vertical => (Vec2::Y, size.x * 0.5, size.y * 0.5),
                CapsuleDirection2D::Horizontal => (Vec2::X, size.y * 0.5, size.x * 0.5),
            };
            let seg = (half - r).max(0.0);
            let side = axis.perp();
            let (top, bottom) = (o + axis * seg, o - axis * seg);
            let base = axis.to_angle();
            self.arc_2d(&m, top, r, base - pi * 0.5, pi);
            self.arc_2d(&m, bottom, r, base + pi * 0.5, pi);
            self.chain_2d(&m, &[top + side * r, bottom + side * r], false);
            self.chain_2d(&m, &[top - side * r, bottom - side * r], false);
        }
        if let Ok(c) = w.get::<&PolygonCollider2D>(e)
            && c.common.enabled
        {
            let pts: Vec<Vec2> = polygon_points(scene, e, &c)
                .into_iter()
                .map(|p| (p + c.common.offset) * s)
                .collect();
            self.chain_2d(&m, &pts, true);
        }
        if let Ok(c) = w.get::<&EdgeCollider2D>(e)
            && c.common.enabled
        {
            let pts: Vec<Vec2> = c
                .points
                .iter()
                .map(|p| (*p + c.common.offset) * s)
                .collect();
            self.chain_2d(&m, &pts, false);
        }
    }
}
