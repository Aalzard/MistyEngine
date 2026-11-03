//! Fixed, Hinge and Spring Joints between an object's body and its connected body (or the world).
//!
//! Fixed and Hinge Joints are rapier impulse joints whose frames match the poses when Play
//! starts (so the hinge angle starts at 0). The Spring Joint is a force applied before each step,
//! because its min/max distance band (no force inside it) has no rapier equivalent.

use std::collections::HashSet;

use glam::{Quat, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use super::convert::{gv, parts, pose, rv};
use super::{Physics, world_pose};
use crate::scene::{FixedJoint, HingeJoint, Rigidbody, Scene, SpringJoint};

/// Velocity-error damping of hinge motors: high, so the motor reaches its target velocity
/// whenever `force` allows.
const MOTOR_DAMPING: f32 = 1.0e5;

/// A Spring Joint, applied as forces each step.
pub(super) struct Spring {
    pub body: Entity,
    pub connected: Option<Entity>,
    /// In the body's (unscaled) frame.
    anchor: Vec3,
    /// In the connected body's frame, or a world point.
    connected_anchor: Vec3,
    spring: f32,
    damper: f32,
    min: f32,
    max: f32,
}

/// Where a joint attaches: rapier handle + world pose of the connected side.
struct Connection {
    entity: Option<Entity>,
    handle: rp::RigidBodyHandle,
    pos: Vec3,
    rot: Quat,
    scale: Vec3,
}

impl Physics {
    /// Fixed body for joints that connect to the world.
    fn world_anchor(&mut self) -> rp::RigidBodyHandle {
        *self
            .world_anchor
            .get_or_insert_with(|| self.world.insert_body(rp::RigidBodyBuilder::fixed()))
    }

    /// The connected body of a joint on `e`, or the world.
    fn connection(&mut self, scene: &Scene, e: Entity, target: Option<Entity>) -> Connection {
        if let Some(c) = target {
            if c != e
                && scene.contains(c)
                && let Some(b) = self.bodies.get(&c)
            {
                let (pos, rot, scale) = world_pose(scene, c);
                return Connection {
                    entity: Some(c),
                    handle: b.handle,
                    pos,
                    rot,
                    scale,
                };
            }
            self.warnings.push(format!(
                "'{}': the joint's Connected Body has no Rigidbody; connected to the world.",
                scene.name(e)
            ));
        }
        Connection {
            entity: None,
            handle: self.world_anchor(),
            pos: Vec3::ZERO,
            rot: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Joints of `objects` (after their bodies exist).
    pub(super) fn add_joints(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let w = &scene.world;
            let fixed = w.get::<&FixedJoint>(e).ok().map(|j| (*j).clone());
            let hinge = w.get::<&HingeJoint>(e).ok().map(|j| (*j).clone());
            let spring = w.get::<&SpringJoint>(e).ok().map(|j| (*j).clone());
            if fixed.is_none() && hinge.is_none() && spring.is_none() {
                continue;
            }
            let body = match self.bodies.get(&e) {
                Some(b) if w.get::<&Rigidbody>(e).is_ok() => b.handle,
                _ => {
                    self.warnings
                        .push(format!("'{}': joints need a Rigidbody.", scene.name(e)));
                    continue;
                }
            };
            if let Some(j) = fixed {
                let to = self.connection(scene, e, j.connected_body);
                let data = fixed_joint(scene, e, &to, j.enable_collision);
                self.world.insert_impulse_joint(to.handle, body, data);
            }
            if let Some(j) = hinge {
                let to = self.connection(scene, e, j.connected_body);
                let data = hinge_joint(scene, e, &to, &j);
                self.world.insert_impulse_joint(to.handle, body, data);
            }
            if let Some(j) = spring {
                let to = self.connection(scene, e, j.connected_body);
                let (anchor, connected_anchor) = anchors(
                    scene,
                    e,
                    &to,
                    j.anchor,
                    j.auto_configure_connected_anchor,
                    j.connected_anchor,
                );
                if !j.enable_collision {
                    // A joint that constrains nothing, only to switch contacts off.
                    let free = rp::GenericJointBuilder::new(rp::JointAxesMask::empty())
                        .contacts_enabled(false);
                    self.world.insert_impulse_joint(to.handle, body, free);
                }
                self.springs.push(Spring {
                    body: e,
                    connected: to.entity,
                    anchor,
                    connected_anchor,
                    spring: j.spring.max(0.0),
                    damper: j.damper.max(0.0),
                    min: j.min_distance.max(0.0).min(j.max_distance.max(0.0)),
                    max: j.max_distance.max(0.0).max(j.min_distance.max(0.0)),
                });
            }
        }
    }

    /// World position and velocity of a point given in `e`'s body frame (or a world point).
    fn point_state(&self, e: Option<Entity>, local: Vec3) -> Option<(Vec3, Vec3)> {
        let Some(e) = e else {
            return Some((local, Vec3::ZERO));
        };
        let body = self.body(e)?;
        let (p, r) = parts(body.position());
        let world = p + r * local;
        Some((world, gv(body.velocity_at_point(rv(world)))))
    }

    /// Drop the springs attached to `e` and the forces they left on the other body.
    pub(super) fn remove_springs(&mut self, e: Entity) {
        let mut others = Vec::new();
        self.springs.retain(|s| {
            let attached = s.body == e || s.connected == Some(e);
            if attached {
                others.extend([Some(s.body), s.connected].into_iter().flatten());
            }
            !attached
        });
        for o in others {
            if let Some(b) = self.body_mut(o) {
                b.reset_forces(true);
            }
        }
    }

    /// Spring Joint forces for the coming step.
    pub(super) fn apply_springs(&mut self) {
        let mut forces: Vec<(Entity, Vec3, Vec3)> = Vec::new();
        for s in &self.springs {
            let (Some((p1, v1)), Some((p2, v2))) = (
                self.point_state(Some(s.body), s.anchor),
                self.point_state(s.connected, s.connected_anchor),
            ) else {
                continue;
            };
            let d = p2 - p1;
            let dist = d.length();
            let stretch = if dist > s.max {
                dist - s.max
            } else if dist < s.min {
                dist - s.min
            } else {
                0.0
            };
            if stretch == 0.0 || dist < 1e-6 {
                continue;
            }
            let n = d / dist;
            let f = n * (s.spring * stretch + s.damper * (v2 - v1).dot(n));
            forces.push((s.body, f, p1));
            if let Some(c) = s.connected {
                forces.push((c, -f, p2));
            }
        }
        // Forces persist in rapier: clear last step's spring forces first.
        let touched: HashSet<Entity> = self.springs.iter().map(|s| s.body).collect();
        let connected: HashSet<Entity> = self.springs.iter().filter_map(|s| s.connected).collect();
        for e in touched.union(&connected) {
            if let Some(b) = self.body_mut(*e) {
                b.reset_forces(false);
            }
        }
        for (e, f, at) in forces {
            if let Some(b) = self.body_mut(e) {
                b.add_force_at_point(rv(f), rv(at), true);
            }
        }
    }
}

/// Anchor in `e`'s body frame and the connected anchor in the connected frame (or world).
fn anchors(
    scene: &Scene,
    e: Entity,
    to: &Connection,
    anchor: Vec3,
    auto: bool,
    connected_anchor: Vec3,
) -> (Vec3, Vec3) {
    let (ps, rs, ss) = world_pose(scene, e);
    let local = anchor * ss;
    let world = ps + rs * local;
    let connected = if auto {
        to.rot.inverse() * (world - to.pos)
    } else {
        connected_anchor * to.scale
    };
    (local, connected)
}

/// Keeps the current relative pose.
fn fixed_joint(scene: &Scene, e: Entity, to: &Connection, collide: bool) -> rp::GenericJoint {
    let (ps, rs, _) = world_pose(scene, e);
    let inv = to.rot.inverse();
    rp::GenericJointBuilder::new(rp::JointAxesMask::LOCKED_FIXED_AXES)
        .local_frame1(pose(inv * (ps - to.pos), inv * rs))
        .local_frame2(pose(Vec3::ZERO, Quat::IDENTITY))
        .contacts_enabled(collide)
        .build()
}

/// Free rotation about `axis` (local), angle 0 at the start pose, optional limits/spring/motor.
fn hinge_joint(scene: &Scene, e: Entity, to: &Connection, j: &HingeJoint) -> rp::GenericJoint {
    let (_, rs, _) = world_pose(scene, e);
    let (anchor, connected_anchor) = anchors(
        scene,
        e,
        to,
        j.anchor,
        j.auto_configure_connected_anchor,
        j.connected_anchor,
    );
    let axis = j.axis.try_normalize().unwrap_or(Vec3::X);
    // Joint frame: its X axis is the hinge axis; both frames coincide at the start.
    let frame = Quat::from_rotation_arc(Vec3::X, axis);
    let mut b = rp::GenericJointBuilder::new(rp::JointAxesMask::LOCKED_REVOLUTE_AXES)
        .local_frame1(pose(connected_anchor, to.rot.inverse() * rs * frame))
        .local_frame2(pose(anchor, frame))
        .contacts_enabled(j.enable_collision);
    let ang = rp::JointAxis::AngX;
    if j.use_limits {
        let (lo, hi) = (
            j.limits.min.min(j.limits.max),
            j.limits.max.max(j.limits.min),
        );
        b = b.limits(ang, [lo.to_radians(), hi.to_radians()]);
    }
    if j.use_motor {
        b = b
            .motor_velocity(ang, j.motor.target_velocity.to_radians(), MOTOR_DAMPING)
            .motor_max_force(ang, j.motor.force.max(0.0))
            .motor_model(ang, rp::MotorModel::ForceBased);
    } else if j.use_spring {
        b = b
            .motor_position(
                ang,
                j.spring.target_position.to_radians(),
                j.spring.spring.max(0.0),
                j.spring.damper.max(0.0),
            )
            .motor_model(ang, rp::MotorModel::ForceBased);
    }
    b.build()
}
