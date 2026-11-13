//! Fixed, Hinge, Spring, Configurable and Character Joints between an object's body and its
//! connected body (or the world), with break forces and projection.
//!
//! Fixed, Hinge, Configurable and Character Joints are rapier impulse joints whose frames match
//! the poses when Play starts (so every joint coordinate starts at 0). The Spring Joint is a
//! force applied before each step, because its min/max distance band (no force inside it) has
//! no rapier equivalent. Soft limits (a limit with a spring) are forces too: rapier's joint
//! softness would soften the locked axes as well.
//!
//! After each step the force and torque each breakable joint used are compared to its Break
//! Force / Break Torque; a joint over either is removed and reported ([`JointBreak`]), and the
//! script host calls `OnJointBreak(breakForce)` and removes the component.

use std::collections::HashSet;

use glam::{Mat3, Quat, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use super::convert::{gv, parts, pose, rv};
use super::{Physics, world_pose};
use crate::scene::{
    CharacterJoint, ConfigurableJoint, FixedJoint, HingeJoint, JointDrive, JointMotion,
    JointProjectionMode, Rigidbody, RotationDriveMode, Scene, SoftJointLimitSpring, SpringJoint,
};

/// Velocity-error damping of hinge motors: high, so the motor reaches its target velocity
/// whenever `force` allows.
const MOTOR_DAMPING: f32 = 1.0e5;

const AXES: [rp::JointAxis; 6] = [
    rp::JointAxis::LinX,
    rp::JointAxis::LinY,
    rp::JointAxis::LinZ,
    rp::JointAxis::AngX,
    rp::JointAxis::AngY,
    rp::JointAxis::AngZ,
];

const MASKS: [rp::JointAxesMask; 6] = [
    rp::JointAxesMask::LIN_X,
    rp::JointAxesMask::LIN_Y,
    rp::JointAxesMask::LIN_Z,
    rp::JointAxesMask::ANG_X,
    rp::JointAxesMask::ANG_Y,
    rp::JointAxesMask::ANG_Z,
];

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
    break_force: f32,
    /// The constraint-free joint that switches contacts off (Enable Collision off).
    no_contacts: Option<rp::ImpulseJointHandle>,
}

/// A limit with a spring: no hard stop, a restoring force past `min` / `max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SoftLimit {
    /// 0–2 linear X/Y/Z, 3–5 angular X/Y/Z of the joint frame.
    pub axis: usize,
    /// Metres or radians.
    pub min: f32,
    pub max: f32,
    pub spring: f32,
    pub damper: f32,
}

/// A rapier joint and what is needed to break, project and soften it.
pub(super) struct JointRecord {
    pub entity: Entity,
    /// Component name ("Hinge Joint"…).
    pub kind: &'static str,
    pub handle: rp::ImpulseJointHandle,
    pub body: rp::RigidBodyHandle,
    pub connected: rp::RigidBodyHandle,
    pub connected_entity: Option<Entity>,
    /// Joint frame in the connected body's frame (or the world).
    pub frame1: rp::Pose,
    /// Joint frame in the body's frame.
    pub frame2: rp::Pose,
    pub break_force: f32,
    pub break_torque: f32,
    /// Projection distance (m) and angle (radians); the angle only acts on locked rotations.
    pub projection: Option<(f32, f32)>,
    /// Every angular axis locked (angular projection can target the start rotation).
    pub angular_locked: bool,
    /// Per linear axis: `Some(0)` locked, `Some(limit)` hard-limited, `None` free (projection).
    pub linear: [Option<f32>; 3],
    pub soft: Vec<SoftLimit>,
}

/// A joint's object, kind and the world poses of its two frames (connected side, body side).
pub type JointFrames = (Entity, &'static str, (Vec3, Quat), (Vec3, Quat));

/// A joint that broke during a step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointBreak {
    pub entity: Entity,
    /// Component name of the joint ("Fixed Joint"…).
    pub joint: &'static str,
    /// The force (N) that broke it (or the torque, N·m, when that was over its limit).
    pub force: f32,
}

/// Where a joint attaches: rapier handle + world pose of the connected side.
struct Connection {
    entity: Option<Entity>,
    handle: rp::RigidBodyHandle,
    pos: Vec3,
    rot: Quat,
    scale: Vec3,
}

/// Joint coordinates of a record now: offset and angles in the connected frame, and their rates.
#[derive(Clone, Copy, Debug)]
pub(super) struct JointState {
    /// World poses of the two joint frames.
    pub f1: (Vec3, Quat),
    pub f2: (Vec3, Quat),
    /// Linear offset (m) then angles (radians) along the joint axes.
    pub coords: [f32; 6],
    /// Their rates (m/s, rad/s).
    pub rates: [f32; 6],
}

/// Angles about the frame's X, Y and Z of a relative rotation (each exact when the others are 0).
pub(super) fn axis_angles(q: Quat) -> Vec3 {
    let q = if q.w < 0.0 { -q } else { q };
    Vec3::new(
        2.0 * q.x.atan2(q.w),
        2.0 * q.y.atan2(q.w),
        2.0 * q.z.atan2(q.w),
    )
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

    /// Insert a rapier joint and remember it.
    #[allow(clippy::too_many_arguments)]
    fn insert_joint(
        &mut self,
        e: Entity,
        kind: &'static str,
        body: rp::RigidBodyHandle,
        to: &Connection,
        data: rp::GenericJoint,
        breaks: (f32, f32),
        projection: Option<(f32, f32)>,
        soft: Vec<SoftLimit>,
    ) {
        let frame1 = data.local_frame1;
        let frame2 = data.local_frame2;
        let angular_locked = data.locked_axes.contains(rp::JointAxesMask::ANG_AXES);
        let linear = std::array::from_fn(|i| {
            if data.locked_axes.contains(MASKS[i]) {
                Some(0.0)
            } else if data.limit_axes.contains(MASKS[i]) {
                Some(data.limits[i].max.abs())
            } else {
                None
            }
        });
        let handle = self.world.insert_impulse_joint(to.handle, body, data);
        self.joints.push(JointRecord {
            entity: e,
            kind,
            handle,
            body,
            connected: to.handle,
            connected_entity: to.entity,
            frame1,
            frame2,
            break_force: breaks.0.max(0.0),
            break_torque: breaks.1.max(0.0),
            projection,
            angular_locked,
            linear,
            soft,
        });
    }

    /// Joints of `objects` (after their bodies exist).
    pub(super) fn add_joints(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let w = &scene.world;
            let fixed = w.get::<&FixedJoint>(e).ok().map(|j| (*j).clone());
            let hinge = w.get::<&HingeJoint>(e).ok().map(|j| (*j).clone());
            let spring = w.get::<&SpringJoint>(e).ok().map(|j| (*j).clone());
            let config = w.get::<&ConfigurableJoint>(e).ok().map(|j| (*j).clone());
            let character = w.get::<&CharacterJoint>(e).ok().map(|j| (*j).clone());
            if fixed.is_none()
                && hinge.is_none()
                && spring.is_none()
                && config.is_none()
                && character.is_none()
            {
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
                let breaks = (j.break_force, j.break_torque);
                self.insert_joint(e, "Fixed Joint", body, &to, data, breaks, None, Vec::new());
            }
            if let Some(j) = hinge {
                let to = self.connection(scene, e, j.connected_body);
                let data = hinge_joint(scene, e, &to, &j);
                let breaks = (j.break_force, j.break_torque);
                self.insert_joint(e, "Hinge Joint", body, &to, data, breaks, None, Vec::new());
            }
            if let Some(j) = config {
                let to = self.connection(scene, e, j.connected_body);
                let frames = joint_frames(
                    scene,
                    e,
                    &to,
                    (
                        j.anchor,
                        j.auto_configure_connected_anchor,
                        j.connected_anchor,
                    ),
                    j.axis,
                    j.secondary_axis,
                );
                let (data, soft) = configurable_joint(&j, frames);
                let projection = (j.projection_mode == JointProjectionMode::PositionAndRotation)
                    .then(|| {
                        (
                            j.projection_distance.max(0.0),
                            j.projection_angle.max(0.0).to_radians(),
                        )
                    });
                let breaks = (j.break_force, j.break_torque);
                self.insert_joint(
                    e,
                    "Configurable Joint",
                    body,
                    &to,
                    data,
                    breaks,
                    projection,
                    soft,
                );
            }
            if let Some(j) = character {
                let to = self.connection(scene, e, j.connected_body);
                let frames = joint_frames(
                    scene,
                    e,
                    &to,
                    (
                        j.anchor,
                        j.auto_configure_connected_anchor,
                        j.connected_anchor,
                    ),
                    j.axis,
                    j.swing_axis,
                );
                let (data, soft) = character_joint(&j, frames);
                let projection = j.enable_projection.then(|| {
                    (
                        j.projection_distance.max(0.0),
                        j.projection_angle.max(0.0).to_radians(),
                    )
                });
                let breaks = (j.break_force, j.break_torque);
                self.insert_joint(
                    e,
                    "Character Joint",
                    body,
                    &to,
                    data,
                    breaks,
                    projection,
                    soft,
                );
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
                let no_contacts = (!j.enable_collision).then(|| {
                    // A joint that constrains nothing, only to switch contacts off.
                    let free = rp::GenericJointBuilder::new(rp::JointAxesMask::empty())
                        .contacts_enabled(false);
                    self.world.insert_impulse_joint(to.handle, body, free)
                });
                self.springs.push(Spring {
                    body: e,
                    connected: to.entity,
                    anchor,
                    connected_anchor,
                    spring: j.spring.max(0.0),
                    damper: j.damper.max(0.0),
                    min: j.min_distance.max(0.0).min(j.max_distance.max(0.0)),
                    max: j.max_distance.max(0.0).max(j.min_distance.max(0.0)),
                    break_force: j.break_force.max(0.0),
                    no_contacts,
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

    /// World anchor points of the Spring Joints (debug view).
    pub(super) fn spring_anchors(&self) -> Vec<(Vec3, Vec3)> {
        self.springs
            .iter()
            .filter_map(|s| {
                let a = self.point_state(Some(s.body), s.anchor)?.0;
                let b = self.point_state(s.connected, s.connected_anchor)?.0;
                Some((a, b))
            })
            .collect()
    }

    /// Drop the springs and joints attached to `e` and the forces they left on the other body.
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
        self.joints
            .retain(|j| j.entity != e && j.connected_entity != Some(e));
    }

    /// Joint coordinates of `r` now (`None` once a body is gone).
    pub(super) fn joint_state(&self, r: &JointRecord) -> Option<JointState> {
        let b1 = self.world.bodies.get(r.connected)?;
        let b2 = self.world.bodies.get(r.body)?;
        let (p1, q1) = parts(b1.position());
        let (p2, q2) = parts(b2.position());
        let (l1, lq1) = parts(&r.frame1);
        let (l2, lq2) = parts(&r.frame2);
        let f1 = (p1 + q1 * l1, q1 * lq1);
        let f2 = (p2 + q2 * l2, q2 * lq2);
        let inv = f1.1.inverse();
        let offset = inv * (f2.0 - f1.0);
        let angles = axis_angles(inv * f2.1);
        let v1 = gv(b1.velocity_at_point(rv(f2.0)));
        let v2 = gv(b2.velocity_at_point(rv(f2.0)));
        let lin_rate = inv * (v2 - v1);
        let ang_rate = inv * (gv(b2.angvel()) - gv(b1.angvel()));
        Some(JointState {
            f1,
            f2,
            coords: [offset.x, offset.y, offset.z, angles.x, angles.y, angles.z],
            rates: [
                lin_rate.x, lin_rate.y, lin_rate.z, ang_rate.x, ang_rate.y, ang_rate.z,
            ],
        })
    }

    /// Spring Joint and soft limit forces for the coming step.
    pub(super) fn apply_springs(&mut self) {
        let mut forces: Vec<(Entity, Vec3, Vec3)> = Vec::new();
        let mut broken = Vec::new();
        for (i, s) in self.springs.iter().enumerate() {
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
            if f.length() > s.break_force {
                broken.push((i, f.length()));
                continue;
            }
            forces.push((s.body, f, p1));
            if let Some(c) = s.connected {
                forces.push((c, -f, p2));
            }
        }
        for (i, force) in broken.into_iter().rev() {
            let s = self.springs.remove(i);
            // A broken spring lets go: its contact switch goes (the bodies passed through each
            // other for good), and so does the force it applied last step (rapier keeps
            // forces, and only bodies of the remaining springs are reset below).
            if let Some(h) = s.no_contacts {
                self.world.remove_impulse_joint(h);
            }
            for e in [Some(s.body), s.connected].into_iter().flatten() {
                if let Some(b) = self.body_mut(e) {
                    b.reset_forces(true);
                }
            }
            self.joint_breaks.push(JointBreak {
                entity: s.body,
                joint: "Spring Joint",
                force,
            });
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
        self.apply_soft_limits();
    }

    /// Restoring forces of soft limits past their range.
    fn apply_soft_limits(&mut self) {
        let mut out: Vec<(rp::RigidBodyHandle, Vec3, Vec3, Option<Vec3>)> = Vec::new();
        let mut touched: HashSet<rp::RigidBodyHandle> = HashSet::new();
        for r in self.joints.iter().filter(|r| !r.soft.is_empty()) {
            let Some(s) = self.joint_state(r) else {
                continue;
            };
            touched.insert(r.body);
            touched.insert(r.connected);
            for l in &r.soft {
                let c = s.coords[l.axis];
                let excess = if c > l.max {
                    c - l.max
                } else if c < l.min {
                    c - l.min
                } else {
                    continue;
                };
                let mut local = Vec3::ZERO;
                local[l.axis % 3] = 1.0;
                let axis = s.f1.1 * local;
                let magnitude = -(l.spring * excess + l.damper * s.rates[l.axis]);
                if l.axis < 3 {
                    out.push((r.body, axis * magnitude, Vec3::ZERO, Some(s.f2.0)));
                    out.push((r.connected, -axis * magnitude, Vec3::ZERO, Some(s.f2.0)));
                } else {
                    out.push((r.body, Vec3::ZERO, axis * magnitude, None));
                    out.push((r.connected, Vec3::ZERO, -axis * magnitude, None));
                }
            }
        }
        // Spring Joint bodies were reset above; reset the others once.
        let springs: HashSet<rp::RigidBodyHandle> = self
            .springs
            .iter()
            .flat_map(|s| [Some(s.body), s.connected])
            .flatten()
            .filter_map(|e| self.bodies.get(&e).map(|b| b.handle))
            .collect();
        for h in touched.difference(&springs) {
            if let Some(b) = self.world.bodies.get_mut(*h) {
                b.reset_forces(false);
                b.reset_torques(false);
            }
        }
        for (h, force, torque, at) in out {
            let Some(b) = self.world.bodies.get_mut(h) else {
                continue;
            };
            if !b.is_dynamic() {
                continue;
            }
            match at {
                Some(p) => b.add_force_at_point(rv(force), rv(p), true),
                None => b.add_torque(rv(torque), true),
            }
        }
    }

    /// Force (N) and torque about the anchor (N·m) joint `r` used in the last step. The torque
    /// counts the moment of the joint force about the body's centre of mass too: the solver may
    /// hold a bent joint with its linear rows, which still bends the joint.
    fn joint_load(&self, r: &JointRecord) -> Option<(f32, f32)> {
        let j = self.world.impulse_joints.get(r.handle)?;
        let s = self.joint_state(r)?;
        let b2 = self.world.bodies.get(r.body)?;
        // Joint impulses are per solver substep, along the joint frame's axes.
        let substeps = self
            .world
            .integration_parameters
            .num_solver_iterations
            .max(1) as f32;
        let h = (self.world.integration_parameters.dt / substeps).max(1e-6);
        let imp = j.impulses;
        let force = s.f1.1 * Vec3::new(imp[0], imp[1], imp[2]) / h;
        let torque = Vec3::new(imp[3], imp[4], imp[5]).length() / h;
        let com = gv(b2.center_of_mass());
        let moment = (com - s.f2.0).cross(force).length();
        Some((force.length(), torque.max(moment)))
    }

    /// Break joints whose force or torque went over their limits during the last step.
    pub(super) fn check_breaks(&mut self) {
        let mut broken = Vec::new();
        for (i, r) in self.joints.iter().enumerate() {
            if r.break_force.is_infinite() && r.break_torque.is_infinite() {
                continue;
            }
            let Some((force, torque)) = self.joint_load(r) else {
                continue;
            };
            if force > r.break_force {
                broken.push((i, force));
            } else if torque > r.break_torque {
                broken.push((i, torque));
            }
        }
        for (i, force) in broken.into_iter().rev() {
            let r = self.joints.remove(i);
            self.world.remove_impulse_joint(r.handle);
            self.joint_breaks.push(JointBreak {
                entity: r.entity,
                joint: r.kind,
                force,
            });
        }
    }

    /// The force (N) and torque (N·m) the joint of kind `kind` on `e` used in the last step.
    pub fn joint_force(&self, e: Entity, kind: &str) -> Option<(f32, f32)> {
        let r = self
            .joints
            .iter()
            .find(|r| r.entity == e && r.kind == kind)?;
        self.joint_load(r)
    }

    /// Does `e` still have a working joint of kind `kind`?
    pub fn has_joint(&self, e: Entity, kind: &str) -> bool {
        self.joints.iter().any(|r| r.entity == e && r.kind == kind)
            || (kind == "Spring Joint" && self.springs.iter().any(|s| s.body == e))
    }

    /// Joints broken since the last call (the script host calls `OnJointBreak`).
    pub fn take_joint_breaks(&mut self) -> Vec<JointBreak> {
        std::mem::take(&mut self.joint_breaks)
    }

    /// Projection: pull bodies whose joint drifted past its projection distance (and locked
    /// rotations past the projection angle) back.
    pub(super) fn project_joints(&mut self) {
        let mut moves: Vec<(rp::RigidBodyHandle, Vec3, Option<Quat>)> = Vec::new();
        for r in &self.joints {
            let Some((dist, angle)) = r.projection else {
                continue;
            };
            let Some(s) = self.joint_state(r) else {
                continue;
            };
            let Some(b2) = self.world.bodies.get(r.body) else {
                continue;
            };
            if !b2.is_dynamic() {
                continue;
            }
            // How far the anchor is outside what its locked and limited axes allow.
            let mut local = Vec3::ZERO;
            for (i, allowed) in r.linear.iter().enumerate() {
                let c = s.coords[i];
                if let Some(l) = allowed {
                    local[i] = if c > *l {
                        c - l
                    } else if c < -l {
                        c + l
                    } else {
                        0.0
                    };
                }
            }
            let err = s.f1.1 * local;
            let len = err.length();
            let mut shift = Vec3::ZERO;
            if len > dist && len > 1e-6 {
                shift = -err * (1.0 - dist / len);
            }
            let mut rotation = None;
            if r.angular_locked {
                let (_, lq2) = parts(&r.frame2);
                let target = s.f1.1 * lq2.inverse();
                let (_, current) = parts(b2.position());
                let off = current.angle_between(target);
                if off > angle && off > 1e-5 {
                    rotation = Some(target.slerp(current, angle / off));
                }
            }
            if shift != Vec3::ZERO || rotation.is_some() {
                moves.push((r.body, shift, rotation));
            }
        }
        for (h, shift, rotation) in moves {
            if let Some(b) = self.world.bodies.get_mut(h) {
                let (p, q) = parts(b.position());
                b.set_position(pose(p + shift, rotation.unwrap_or(q)), true);
                if shift != Vec3::ZERO {
                    // Lose the velocity along the correction (like the solver would).
                    let v = gv(b.linvel());
                    let n = shift.normalize();
                    let along = v.dot(n);
                    if along < 0.0 {
                        b.set_linvel(rv(v - n * along), true);
                    }
                }
            }
        }
    }

    /// World poses of the joint frames of every joint (debug view).
    pub fn joint_frames_world(&self) -> Vec<JointFrames> {
        self.joints
            .iter()
            .filter_map(|r| {
                let s = self.joint_state(r)?;
                Some((r.entity, r.kind, s.f1, s.f2))
            })
            .collect()
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

/// Rotation whose X axis is `axis` and whose Y axis is `secondary` made perpendicular.
pub fn joint_basis(axis: Vec3, secondary: Vec3) -> Quat {
    let x = axis.try_normalize().unwrap_or(Vec3::X);
    let y = (secondary - x * secondary.dot(x))
        .try_normalize()
        .unwrap_or_else(|| x.any_orthonormal_vector());
    let z = x.cross(y);
    Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize()
}

/// Joint frames (connected side, body side) that coincide at the start pose.
fn joint_frames(
    scene: &Scene,
    e: Entity,
    to: &Connection,
    (anchor, auto, connected_anchor): (Vec3, bool, Vec3),
    axis: Vec3,
    secondary: Vec3,
) -> (rp::Pose, rp::Pose) {
    let (_, rs, _) = world_pose(scene, e);
    let (a, ca) = anchors(scene, e, to, anchor, auto, connected_anchor);
    let frame = joint_basis(axis, secondary);
    (pose(ca, to.rot.inverse() * rs * frame), pose(a, frame))
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

/// A limit on `axis`: hard (rapier) without a spring, soft (a force) with one.
fn limit(
    b: rp::GenericJointBuilder,
    soft: &mut Vec<SoftLimit>,
    axis: usize,
    range: [f32; 2],
    spring: SoftJointLimitSpring,
) -> rp::GenericJointBuilder {
    let (lo, hi) = (range[0].min(range[1]), range[1].max(range[0]));
    if spring.spring > 0.0 {
        soft.push(SoftLimit {
            axis,
            min: lo,
            max: hi,
            spring: spring.spring,
            damper: spring.damper.max(0.0),
        });
        b
    } else {
        b.limits(AXES[axis], [lo, hi])
    }
}

/// A drive on `axis` towards `target` / `velocity` (force-based rapier motor).
fn drive(
    b: rp::GenericJointBuilder,
    axis: usize,
    d: &JointDrive,
    target: f32,
    velocity: f32,
) -> rp::GenericJointBuilder {
    if !d.active() {
        return b;
    }
    let max = if d.maximum_force.is_finite() {
        d.maximum_force.max(0.0)
    } else {
        f32::MAX
    };
    b.set_motor(
        AXES[axis],
        target,
        velocity,
        d.position_spring.max(0.0),
        d.position_damper.max(0.0),
    )
    .motor_max_force(AXES[axis], max)
    .motor_model(AXES[axis], rp::MotorModel::ForceBased)
}

/// Configurable Joint: locked axes, hard or soft limits, drives.
pub(super) fn configurable_joint(
    j: &ConfigurableJoint,
    (frame1, frame2): (rp::Pose, rp::Pose),
) -> (rp::GenericJoint, Vec<SoftLimit>) {
    let motions = j.motions();
    let locked = motions
        .iter()
        .zip(MASKS)
        .filter(|(m, _)| **m == JointMotion::Locked)
        .fold(rp::JointAxesMask::empty(), |acc, (_, mask)| acc | mask);
    let mut b = rp::GenericJointBuilder::new(locked)
        .local_frame1(frame1)
        .local_frame2(frame2)
        .contacts_enabled(j.enable_collision);
    let mut soft = Vec::new();
    let lin = j.linear_limit.limit.max(0.0);
    for (axis, m) in motions.iter().enumerate().take(3) {
        if *m == JointMotion::Limited {
            b = limit(b, &mut soft, axis, [-lin, lin], j.linear_limit_spring);
        }
    }
    if motions[3] == JointMotion::Limited {
        let range = [
            j.low_angular_x_limit.limit.to_radians(),
            j.high_angular_x_limit.limit.to_radians(),
        ];
        b = limit(b, &mut soft, 3, range, j.angular_x_limit_spring);
    }
    for (axis, l) in [(4, j.angular_y_limit), (5, j.angular_z_limit)] {
        if motions[axis] == JointMotion::Limited {
            let r = l.limit.abs().to_radians();
            b = limit(b, &mut soft, axis, [-r, r], j.angular_yz_limit_spring);
        }
    }
    let drives = [j.x_drive, j.y_drive, j.z_drive];
    for axis in 0..3 {
        if motions[axis] != JointMotion::Locked {
            b = drive(
                b,
                axis,
                &drives[axis],
                j.target_position[axis],
                j.target_velocity[axis],
            );
        }
    }
    let (dx, dyz) = match j.rotation_drive_mode {
        RotationDriveMode::XAndYz => (j.angular_x_drive, j.angular_yz_drive),
        RotationDriveMode::Slerp => (j.slerp_drive, j.slerp_drive),
    };
    for (k, d) in [dx, dyz, dyz].iter().enumerate() {
        let axis = 3 + k;
        if motions[axis] != JointMotion::Locked {
            b = drive(
                b,
                axis,
                d,
                j.target_rotation[k].to_radians(),
                j.target_angular_velocity[k].to_radians(),
            );
        }
    }
    (b.build(), soft)
}

/// Character Joint: a ball joint with twist and two swing limits.
pub(super) fn character_joint(
    j: &CharacterJoint,
    (frame1, frame2): (rp::Pose, rp::Pose),
) -> (rp::GenericJoint, Vec<SoftLimit>) {
    let mut b = rp::GenericJointBuilder::new(rp::JointAxesMask::LIN_AXES)
        .local_frame1(frame1)
        .local_frame2(frame2)
        .contacts_enabled(j.enable_collision);
    let mut soft = Vec::new();
    let twist = [
        j.low_twist_limit.limit.to_radians(),
        j.high_twist_limit.limit.to_radians(),
    ];
    b = limit(b, &mut soft, 3, twist, j.twist_limit_spring);
    for (axis, l) in [(4, j.swing1_limit), (5, j.swing2_limit)] {
        let r = l.limit.abs().to_radians();
        b = limit(b, &mut soft, axis, [-r, r], j.swing_limit_spring);
    }
    (b.build(), soft)
}
