//! The details of a 3D collision for `OnCollisionEnter(Collision)`: contact points, normals,
//! impulse and the relative velocity at impact.

use glam::Vec3;
use hecs::Entity;
use rapier3d::prelude as rp;

use super::Physics;
use super::convert::gv;

/// How many points a [`CollisionInfo`] keeps (four per manifold; several manifolds for
/// compound and mesh shapes).
pub const MAX_CONTACT_POINTS: usize = 16;

/// One touching point of a 3D collision (Unity's `ContactPoint` data).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactPoint3D {
    /// World position of the point.
    pub point: Vec3,
    /// World normal pointing away from the other object (towards the one asking).
    pub normal: Vec3,
    /// Distance between the surfaces along the normal (negative: overlapping).
    pub separation: f32,
    /// Impulse the solver applied along the normal at this point in the last step.
    pub impulse: f32,
}

/// A collision between two objects' colliders, seen from one of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CollisionInfo {
    /// Velocity of the other object relative to the one asking (before the last step, so an
    /// impact reports the speed it hit with).
    pub relative_velocity: Vec3,
    /// Total impulse of the last step on the one asking (sum over the points).
    pub impulse: Vec3,
    pub points: Vec<ContactPoint3D>,
}

impl Physics {
    /// Velocities of the moving bodies before a step (the relative velocity of new contacts).
    pub(super) fn remember_velocities(&mut self) {
        self.before.clear();
        if !self.track_impacts {
            return;
        }
        for b in self.bodies.values().filter(|b| !b.kinematic) {
            if let Some(body) = self.world.bodies.get(b.handle)
                && !body.is_sleeping()
            {
                self.before.insert(b.handle, gv(body.linvel()));
            }
        }
    }

    fn velocity_of(&self, c: &rp::Collider) -> Vec3 {
        let Some(h) = c.parent() else {
            return Vec3::ZERO;
        };
        self.before
            .get(&h)
            .copied()
            .or_else(|| self.world.bodies.get(h).map(|b| gv(b.linvel())))
            .unwrap_or(Vec3::ZERO)
    }

    /// The collision between `me`'s and `other`'s colliders as of the last step (no points
    /// once they stopped touching).
    pub fn collision_info(&self, me: Entity, other: Entity) -> CollisionInfo {
        let mut info = CollisionInfo::default();
        let (Some(mine), Some(theirs)) =
            (self.colliders_of.get(&me), self.colliders_of.get(&other))
        else {
            return info;
        };
        let mut velocity = None;
        for &h1 in mine {
            for &h2 in theirs {
                let (Some(c1), Some(c2)) =
                    (self.world.colliders.get(h1), self.world.colliders.get(h2))
                else {
                    continue;
                };
                velocity.get_or_insert_with(|| self.velocity_of(c2) - self.velocity_of(c1));
                let Some(pair) = self.world.contact_pair(h1, h2) else {
                    continue;
                };
                let Some(first) = self.world.colliders.get(pair.collider1) else {
                    continue;
                };
                // Manifold normals point out of the pair's first collider: out of `me` when
                // that is mine, so flip them to point towards `me`.
                let sign = if pair.collider1 == h1 { -1.0 } else { 1.0 };
                for m in pair.manifolds().iter() {
                    let n = gv(first.position().transform_vector(m.local_n1)) * sign;
                    for tp in &m.points {
                        if info.points.len() == MAX_CONTACT_POINTS {
                            break;
                        }
                        let p = ContactPoint3D {
                            point: gv(first.position().transform_point(tp.local_p1)),
                            normal: n,
                            separation: tp.dist,
                            impulse: tp.data.impulse,
                        };
                        info.impulse += n * p.impulse;
                        info.points.push(p);
                    }
                }
            }
        }
        info.relative_velocity = velocity.unwrap_or(Vec3::ZERO);
        info
    }
}
