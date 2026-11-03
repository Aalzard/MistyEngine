//! Scene-view gizmos of the selected object's joints (anchor, hinge axis and limit arc, spring
//! line) and its Character Controller capsule.

use glam::{Mat4, Quat, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::physics::collider_matrix;
use misty_engine::scene::{CharacterController, FixedJoint, HingeJoint, Scene, SpringJoint};

use super::{COLLIDER, Lines};

const JOINT: [f32; 4] = [1.0, 0.6, 0.15, 0.95];

/// World point of a connected anchor (the connected body's local space, or world space).
fn connected_point(scene: &Scene, body: Option<Entity>, local: Vec3) -> Vec3 {
    match body.filter(|b| scene.contains(*b)) {
        Some(b) => scene.world_matrix(b).transform_point3(local),
        None => local,
    }
}

impl Lines {
    /// Small three-axis cross at `p`.
    fn cross(&mut self, p: Vec3, size: f32, color: [f32; 4]) {
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.line(p - axis * size, p + axis * size, color);
        }
    }

    /// Joint gizmos of `e` (when selected).
    pub(super) fn joints(&mut self, scene: &Scene, e: Entity) {
        let w = &scene.world;
        let m = scene.world_matrix(e);
        let rot = scene.world_rotation(e);
        if let Ok(j) = w.get::<&FixedJoint>(e) {
            let p = m.transform_point3(Vec3::ZERO);
            self.cross(p, 0.1, JOINT);
            if let Some(b) = j.connected_body.filter(|b| scene.contains(*b)) {
                self.line(p, scene.world_position(b), JOINT);
            }
        }
        if let Ok(j) = w.get::<&HingeJoint>(e) {
            let anchor = m.transform_point3(j.anchor);
            let axis = (rot * j.axis.try_normalize().unwrap_or(Vec3::X)).normalize_or_zero();
            self.cross(anchor, 0.08, JOINT);
            // Axis arrow.
            let tip = anchor + axis * 0.8;
            self.line(anchor - axis * 0.2, tip, JOINT);
            let side = axis.any_orthonormal_vector();
            for s in [side, axis.cross(side)] {
                self.line(tip, tip - axis * 0.15 + s * 0.07, JOINT);
                self.line(tip, tip - axis * 0.15 - s * 0.07, JOINT);
            }
            if j.use_limits {
                self.limit_arc(anchor, axis, side, j.limits.min, j.limits.max);
            }
            if !j.auto_configure_connected_anchor {
                let c = connected_point(scene, j.connected_body, j.connected_anchor);
                self.cross(c, 0.05, JOINT);
            }
        }
        if let Ok(j) = w.get::<&SpringJoint>(e) {
            let anchor = m.transform_point3(j.anchor);
            self.cross(anchor, 0.08, JOINT);
            let other = if j.auto_configure_connected_anchor {
                j.connected_body
                    .filter(|b| scene.contains(*b))
                    .map(|b| scene.world_position(b))
            } else {
                Some(connected_point(scene, j.connected_body, j.connected_anchor))
            };
            if let Some(o) = other {
                self.zigzag(anchor, o);
            }
        }
    }

    /// Arc from `min` to `max` degrees around `axis`, starting along `side`.
    fn limit_arc(&mut self, at: Vec3, axis: Vec3, side: Vec3, min: f32, max: f32) {
        let r = 0.5;
        let point = |deg: f32| at + Quat::from_axis_angle(axis, deg.to_radians()) * side * r;
        let steps = 24;
        for i in 0..steps {
            let a = min + (max - min) * i as f32 / steps as f32;
            let b = min + (max - min) * (i + 1) as f32 / steps as f32;
            self.line(point(a), point(b), JOINT);
        }
        self.line(at, point(min), JOINT);
        self.line(at, point(max), JOINT);
    }

    /// Spring drawn as a zigzag between two points.
    fn zigzag(&mut self, a: Vec3, b: Vec3) {
        let d = b - a;
        if d.length() < 1e-4 {
            return;
        }
        let side = d.normalize().any_orthonormal_vector() * 0.06;
        let n = 10;
        let mut prev = a;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let off = if i == n {
                Vec3::ZERO
            } else if i % 2 == 0 {
                side
            } else {
                -side
            };
            let p = a + d * t + off;
            self.line(prev, p, JOINT);
            prev = p;
        }
    }

    /// Character Controller capsule (always along Y), sized like the physics capsule.
    pub(super) fn character(&mut self, scene: &Scene, e: Entity) {
        let Ok(c) = scene.world.get::<&CharacterController>(e) else {
            return;
        };
        let s = scene
            .world_matrix(e)
            .to_scale_rotation_translation()
            .0
            .abs();
        let r = c.radius * s.x.max(s.z);
        let half = (c.height * s.y * 0.5 - r).max(0.0);
        let m: Mat4 = collider_matrix(scene, e, c.center);
        self.capsule(&m, 1, r, half, COLLIDER);
    }
}
