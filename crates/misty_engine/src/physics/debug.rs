//! The Scene view's Physics Debug draw mode: every collider as a wireframe coloured by what its
//! body is doing, contact points with their normals, joints (the line between the two joint
//! frames and the frames' axes) and cloth particles.
//!
//! Colours: static colliders green, dynamic bodies blue (grey while asleep), kinematic purple,
//! triggers yellow; contacts red; joints orange (the connected side) and white (the body's side).

use glam::{Quat, Vec3};
use rapier3d::prelude as rp;

use super::Physics;
use super::convert::{gv, parts};

pub const STATIC: [f32; 4] = [0.45, 0.85, 0.45, 0.9];
pub const DYNAMIC: [f32; 4] = [0.35, 0.65, 1.0, 0.95];
pub const SLEEPING: [f32; 4] = [0.5, 0.5, 0.55, 0.8];
pub const KINEMATIC: [f32; 4] = [0.8, 0.45, 1.0, 0.95];
pub const TRIGGER: [f32; 4] = [1.0, 0.9, 0.3, 0.9];
pub const CONTACT: [f32; 4] = [1.0, 0.2, 0.15, 1.0];
pub const JOINT: [f32; 4] = [1.0, 0.6, 0.15, 1.0];
pub const JOINT_BODY: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub const CLOTH: [f32; 4] = [0.95, 0.55, 0.8, 0.9];

/// One coloured line segment in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: Vec3,
    pub b: Vec3,
    pub color: [f32; 4],
}

/// What the debug view found (for the overlay's legend and tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub colliders: usize,
    pub sleeping: usize,
    pub contacts: usize,
    pub joints: usize,
}

#[derive(Default)]
struct Out {
    segments: Vec<Segment>,
}

impl Out {
    fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        self.segments.push(Segment { a, b, color });
    }

    fn circle(&mut self, c: Vec3, r: Vec3, s: Vec3, color: [f32; 4]) {
        const N: usize = 24;
        let at = |i: usize| {
            let a = i as f32 / N as f32 * std::f32::consts::TAU;
            c + r * a.cos() + s * a.sin()
        };
        for i in 0..N {
            self.line(at(i), at(i + 1), color);
        }
    }

    fn cross(&mut self, p: Vec3, size: f32, color: [f32; 4]) {
        for a in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.line(p - a * size, p + a * size, color);
        }
    }

    fn wire_box(&mut self, pos: Vec3, rot: Quat, half: Vec3, color: [f32; 4]) {
        let c = |x: f32, y: f32, z: f32| pos + rot * (Vec3::new(x, y, z) * half);
        let k = [
            c(-1., -1., -1.),
            c(1., -1., -1.),
            c(1., 1., -1.),
            c(-1., 1., -1.),
            c(-1., -1., 1.),
            c(1., -1., 1.),
            c(1., 1., 1.),
            c(-1., 1., 1.),
        ];
        for (a, b) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            self.line(k[a], k[b], color);
        }
    }

    /// Wireframe of `shape` at the world pose (`pos`, `rot`).
    fn shape(&mut self, shape: &dyn rp::Shape, pos: Vec3, rot: Quat, color: [f32; 4]) {
        if let Some(b) = shape.as_ball() {
            let r = b.radius;
            for (u, v) in [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)] {
                self.circle(pos, rot * u * r, rot * v * r, color);
            }
        } else if let Some(c) = shape.as_cuboid() {
            self.wire_box(pos, rot, gv(c.half_extents), color);
        } else if let Some(c) = shape.as_capsule() {
            let a = pos + rot * gv(c.segment.a);
            let b = pos + rot * gv(c.segment.b);
            let r = c.radius;
            let axis = (b - a).normalize_or(rot * Vec3::Y);
            let u = axis.any_orthonormal_vector();
            let v = axis.cross(u);
            for end in [a, b] {
                self.circle(end, u * r, v * r, color);
                self.circle(end, u * r, axis * r, color);
                self.circle(end, v * r, axis * r, color);
            }
            for side in [u, -u, v, -v] {
                self.line(a + side * r, b + side * r, color);
            }
        } else if let Some(c) = shape.as_compound() {
            for (local, s) in c.shapes() {
                let (lp, lq) = parts(local);
                self.shape(s.as_ref(), pos + rot * lp, rot * lq, color);
            }
        } else {
            // Meshes, hulls, heightfields: their bounding box.
            let aabb = shape.compute_local_aabb();
            let (lo, hi) = (gv(aabb.mins), gv(aabb.maxs));
            self.wire_box(pos + rot * ((lo + hi) * 0.5), rot, (hi - lo) * 0.5, color);
        }
    }
}

impl Physics {
    /// The Physics Debug lines of the current state of the world.
    pub fn debug_lines(&self) -> (Vec<Segment>, Counts) {
        let mut out = Out::default();
        let mut counts = Counts::default();
        let world = &self.world;
        for (_, c) in world.colliders.iter() {
            let color = match c.parent().and_then(|b| world.bodies.get(b)) {
                _ if c.is_sensor() => TRIGGER,
                Some(b) if b.is_dynamic() && b.is_sleeping() => {
                    counts.sleeping += 1;
                    SLEEPING
                }
                Some(b) if b.is_dynamic() => DYNAMIC,
                Some(b) if b.is_kinematic() => KINEMATIC,
                _ => STATIC,
            };
            counts.colliders += 1;
            let (p, q) = parts(c.position());
            out.shape(c.shape(), p, q, color);
        }
        for pair in world.narrow_phase.contact_pairs() {
            let Some(c1) = world.colliders.get(pair.collider1) else {
                continue;
            };
            let (p1, q1) = parts(c1.position());
            for m in pair.manifolds() {
                let normal = gv(m.data.normal);
                for pt in m.points.iter().filter(|pt| pt.dist <= 0.01) {
                    let at = p1 + q1 * gv(pt.local_p1);
                    out.cross(at, 0.04, CONTACT);
                    out.line(at, at + normal * 0.25, CONTACT);
                    counts.contacts += 1;
                }
            }
        }
        for (_, _, (p1, q1), (p2, q2)) in self.joint_frames_world() {
            counts.joints += 1;
            out.line(p1, p2, JOINT);
            for (axis, len) in [(Vec3::X, 0.25), (Vec3::Y, 0.12), (Vec3::Z, 0.12)] {
                out.line(p1, p1 + q1 * axis * len, JOINT);
                out.line(p2, p2 + q2 * axis * len, JOINT_BODY);
            }
        }
        for (a, b) in self.spring_anchors() {
            counts.joints += 1;
            out.line(a, b, JOINT);
        }
        (out.segments, counts)
    }
}
