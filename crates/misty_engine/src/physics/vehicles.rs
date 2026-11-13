//! Wheel Colliders: raycast suspension and slip-based tyre friction applied to the parent
//! Rigidbody before each fixed step.
//!
//! Each step, every wheel casts a ray down its suspension. A grounded wheel pushes its body
//! off the ground with the spring/damper force plus its share of the body's weight (so at rest
//! the wheel sits at the spring's target position whatever the stiffness). Then, wheel by
//! wheel, the tyre acts at the contact: motor and brake torque spin the wheel, and friction
//! pulls the wheel's rim speed and the ground speed together (forward) and stops sliding along
//! the axle (sideways). While the impulse that would stop the slip fits under the friction
//! curve's extremum times the load, the tyre grips and applies exactly that (no jitter at low
//! speed); otherwise it slides with the curve's value at the slip that results.
//!
//! rapier integrates gravity in substeps, so a body's velocity after a step is not its mean
//! velocity during the step; `mean_velocity` and `step_velocity` correct for that, so wheels
//! rest without creeping (also on slopes).

use glam::{Quat, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use super::convert::{gv, parts, rv};
use super::{Physics, world_pose};
use crate::scene::{Rigidbody, Scene, WheelCollider, WheelFrictionCurve, WheelHit, WheelState};

/// Speeds below this (m/s) count as this much when turning slip speeds into slip ratios.
const SLIP_SPEED_FLOOR: f32 = 1.0;

/// Simulation state of one Wheel Collider.
pub(super) struct Wheel {
    pub entity: Entity,
    /// Object whose body the wheel pushes.
    pub body: Entity,
    /// The wheel object's pose in the body's frame.
    local_pos: Vec3,
    local_rot: Quat,
    layer: u8,
    /// Angular speed around the axle (rad/s).
    omega: f32,
    spin: f32,
    extension: f32,
    hit: Option<WheelHit>,
}

/// Friction passes per step (more settle wheels sharing a body more evenly).
const TYRE_ITERATIONS: usize = 4;

/// A grounded wheel's tyre during the friction passes of one step.
struct Tyre {
    index: usize,
    body: rp::RigidBodyHandle,
    dynamic: bool,
    at: Vec3,
    point: Vec3,
    ground: Option<rp::RigidBodyHandle>,
    forward: Vec3,
    side: Vec3,
    inv_mass_f: f32,
    inv_mass_s: f32,
    /// Rim speed change per unit of forward impulse (0 while the brake holds the wheel).
    spin_per_impulse: f32,
    radius: f32,
    inertia: f32,
    /// Largest forward and sideways impulses this step.
    limit_f: f32,
    limit_s: f32,
    /// Largest combined impulse.
    most: f32,
    locked: bool,
    acc_f: f32,
    acc_s: f32,
}

/// Suspension result of a grounded wheel, used by the friction pass.
struct Contact {
    index: usize,
    /// Where the suspension and tyre forces act.
    at: Vec3,
    normal: Vec3,
    load: f32,
    ground: Option<rp::RigidBodyHandle>,
    collider: Entity,
    point: Vec3,
}

impl Physics {
    /// Wheel Colliders of `objects` (after their bodies exist).
    pub(super) fn add_wheels(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let Some(c) = scene
                .world
                .get::<&WheelCollider>(e)
                .ok()
                .map(|c| (*c).clone())
            else {
                continue;
            };
            let owner = self
                .owner(scene, e)
                .filter(|o| scene.world.get::<&Rigidbody>(*o).is_ok());
            let Some(body) = owner else {
                self.warnings.push(format!(
                    "'{}': a Wheel Collider needs a Rigidbody on the object or a parent.",
                    scene.name(e)
                ));
                continue;
            };
            let (bp, br, _) = world_pose(scene, body);
            let (wp, wr, _) = world_pose(scene, e);
            let inv = br.inverse();
            self.wheels.push(Wheel {
                entity: e,
                body,
                local_pos: inv * (wp - bp),
                local_rot: inv * wr,
                layer: scene.game_object(e).layer,
                omega: 0.0,
                spin: 0.0,
                extension: c.travel().1,
                hit: None,
            });
        }
    }

    /// Drop wheels of a removed object or body.
    pub(super) fn remove_wheels(&mut self, e: Entity) {
        self.wheels.retain(|w| w.entity != e && w.body != e);
    }

    /// The wheel's simulated state (rpm, grounded, hit, suspension extension).
    pub fn wheel_state(&self, e: Entity) -> Option<WheelState> {
        let w = self.wheels.iter().find(|w| w.entity == e)?;
        Some(WheelState {
            is_grounded: w.hit.is_some(),
            rpm: w.omega * 60.0 / std::f32::consts::TAU,
            extension: w.extension,
            hit: w.hit,
            spin: w.spin,
            simulated: true,
        })
    }

    /// Suspension and tyre forces of every wheel for a step of `dt` seconds.
    pub(super) fn apply_wheels(&mut self, scene: &Scene, dt: f32) {
        if self.wheels.is_empty() || dt <= 0.0 {
            return;
        }
        // Each wheel carries an equal share of its body's weight.
        let mut shares: std::collections::HashMap<Entity, usize> = Default::default();
        let settings: Vec<Option<WheelCollider>> = self
            .wheels
            .iter()
            .map(|w| {
                scene
                    .world
                    .get::<&WheelCollider>(w.entity)
                    .ok()
                    .filter(|c| c.enabled && scene.active_in_hierarchy(w.entity))
                    .map(|c| (*c).clone())
            })
            .collect();
        for (w, c) in self.wheels.iter().zip(&settings) {
            if c.is_some() {
                *shares.entry(w.body).or_default() += 1;
            }
        }
        let gravity = self.gravity();
        let mut contacts = Vec::new();
        for (i, c) in settings.iter().enumerate() {
            let Some(c) = c else {
                self.wheels[i].hit = None;
                continue;
            };
            let share = shares
                .get(&self.wheels[i].body)
                .copied()
                .unwrap_or(1)
                .max(1);
            if let Some(contact) = self.suspension(i, c, share, gravity, dt) {
                contacts.push(contact);
            }
        }
        let grounded: Vec<usize> = contacts.iter().map(|c| c.index).collect();
        let mut tyres: Vec<Tyre> = contacts
            .iter()
            .filter_map(|ct| {
                let c = settings[ct.index].as_ref()?;
                self.prepare_tyre(ct, c, dt)
            })
            .collect();
        for _ in 0..TYRE_ITERATIONS {
            for t in &mut tyres {
                self.solve_tyre(t);
            }
        }
        for t in &tyres {
            if let Some(c) = &settings[t.index] {
                let w = &mut self.wheels[t.index];
                if !t.locked {
                    w.omega = slow_down(w.omega, c, dt, t.inertia);
                }
                w.spin = (w.spin + w.omega * dt) % std::f32::consts::TAU;
            }
        }
        // Wheels in the air spin freely.
        for (i, c) in settings.iter().enumerate() {
            if let Some(c) = c
                && !grounded.contains(&i)
            {
                let w = &mut self.wheels[i];
                let inertia = wheel_inertia(c);
                w.omega += c.motor_torque * dt / inertia;
                if !w.omega.is_finite() {
                    // A NaN or huge Motor Torque (scripts): the NaN reached the friction limits below
                    // and `clamp(-NaN, NaN)` panicked.
                    w.omega = 0.0;
                }
                w.omega = slow_down(w.omega, c, dt, inertia);
                w.spin = (w.spin + w.omega * dt) % std::f32::consts::TAU;
            }
        }
    }

    /// Cast wheel `i`'s suspension ray and push its body up; Some when it touches the ground.
    fn suspension(
        &mut self,
        i: usize,
        c: &WheelCollider,
        share: usize,
        gravity: Vec3,
        dt: f32,
    ) -> Option<Contact> {
        let w = &self.wheels[i];
        let handle = self.bodies.get(&w.body)?.handle;
        let body = self.world.bodies.get(handle)?;
        let (bp, br) = parts(body.position());
        let rot = br * w.local_rot;
        let up = rot * Vec3::Y;
        let attach = bp + br * w.local_pos + rot * c.center;
        let (dist, rest) = c.travel();
        let radius = c.radius.max(0.01);
        let dynamic = body.is_dynamic();
        let mass = body.mass();

        let mask = self.settings.collision_mask(w.layer);
        let test = move |_: rp::ColliderHandle, col: &rp::Collider| {
            col.collision_groups().memberships.bits() & mask != 0
        };
        let filter = rp::QueryFilter::default()
            .exclude_rigid_body(handle)
            .exclude_sensors()
            .predicate(&test);
        let ray = rp::Ray::new(rv(attach), rv(-up));
        let found = self
            .world
            .cast_ray_and_get_normal(&ray, dist + radius, true, filter);
        let Some((col, hit)) = found else {
            let w = &mut self.wheels[i];
            w.extension = dist;
            w.hit = None;
            return None;
        };
        let collider = *self.colliders.get(&col)?;
        let ground = self
            .world
            .colliders
            .get(col)
            .and_then(|c| c.parent())
            .filter(|h| self.world.bodies.get(*h).is_some_and(|b| b.is_dynamic()));
        let extension = (hit.time_of_impact - radius).clamp(0.0, dist);
        let point = attach - up * hit.time_of_impact;
        let normal = gv(hit.normal);
        let at = attach - up * (rest + radius) + up * c.force_app_point_distance;

        let ground_v = |p: Vec3| {
            ground
                .and_then(|g| self.world.bodies.get(g))
                .map_or(Vec3::ZERO, |b| gv(b.velocity_at_point(rv(p))))
        };
        let v = self.mean_velocity(body, point) - ground_v(point);
        let s = &c.suspension_spring;
        let sprung = mass / share as f32;
        let weight = sprung * (-gravity).dot(up).max(0.0);
        let compression_speed = -v.dot(up);
        let load = (weight
            + s.spring.max(0.0) * (rest - extension)
            + s.damper.max(0.0) * compression_speed)
            .max(0.0);
        if dynamic {
            // The ground pushes back along its normal (the tyre carries the rest).
            let impulse = normal * load * dt;
            if let Some(b) = self.world.bodies.get_mut(handle) {
                b.apply_impulse_at_point(rv(impulse), rv(at), true);
            }
            if let Some(g) = ground.and_then(|g| self.world.bodies.get_mut(g)) {
                g.apply_impulse_at_point(rv(-impulse), rv(point), true);
            }
        }
        let w = &mut self.wheels[i];
        w.extension = extension;
        w.hit = Some(WheelHit {
            collider,
            point,
            normal,
            forward_dir: Vec3::ZERO,
            sideways_dir: Vec3::ZERO,
            force: load,
            forward_slip: 0.0,
            sideways_slip: 0.0,
        });
        Some(Contact {
            index: i,
            at,
            normal,
            load,
            ground,
            collider,
            point,
        })
    }

    /// Motor torque, then how the tyre will act this step: its directions, whether the brake
    /// holds the wheel, and the most friction impulse it can give each way.
    fn prepare_tyre(&mut self, contact: &Contact, c: &WheelCollider, dt: f32) -> Option<Tyre> {
        let i = contact.index;
        let handle = self.bodies.get(&self.wheels[i].body)?.handle;
        let body = self.world.bodies.get(handle)?;
        let (_, br) = parts(body.position());
        let steer = Quat::from_rotation_y(c.steer_angle.to_radians());
        let rot = br * self.wheels[i].local_rot * steer;
        let n = contact.normal;
        let on_ground = |d: Vec3| (d - n * d.dot(n)).normalize_or_zero();
        let forward = on_ground(rot * Vec3::Z);
        let side = on_ground(rot * Vec3::X);
        let v =
            self.step_velocity(body, contact.at) - self.ground_velocity(contact.ground, contact.at);
        let (vf, vs) = (v.dot(forward), v.dot(side));
        let inv_mass_f = inv_mass_at(body, contact.at, forward);
        let inv_mass_s = inv_mass_at(body, contact.at, side);

        let radius = c.radius.max(0.01);
        let inertia = wheel_inertia(c);
        let load_dt = contact.load * dt;
        let w = &mut self.wheels[i];
        w.omega += c.motor_torque * dt / inertia;
        if !w.omega.is_finite() {
            // A NaN or huge Motor Torque (scripts): the NaN reached the friction limits below
            // and `clamp(-NaN, NaN)` panicked.
            w.omega = 0.0;
        }

        // Forward, with the wheel free to spin…
        let spin_per_impulse = radius * radius / inertia;
        let omega = w.omega;
        let slip_free = |j: f32| {
            let (vf, rim) = (vf + j * inv_mass_f, omega * radius - j * spin_per_impulse);
            (vf - rim) / vf.abs().max(rim.abs()).max(SLIP_SPEED_FLOOR)
        };
        let stop = (omega * radius - vf) / (spin_per_impulse + inv_mass_f);
        let free = grip(stop, load_dt, &c.forward_friction, slip_free);
        // …or held still by the brake, when it is strong enough for that.
        let slip_locked = |j: f32| {
            let vf = vf + j * inv_mass_f;
            vf / vf.abs().max(SLIP_SPEED_FLOOR)
        };
        let stop = -vf / inv_mass_f.max(1e-6);
        let held = grip(stop, load_dt, &c.forward_friction, slip_locked);
        let locked = c.brake_torque > 0.0
            && (omega * inertia - held.0 * radius).abs() <= c.brake_torque * dt;
        let (jf, forward_slip) = if locked { held } else { free };
        if locked {
            w.omega = 0.0;
        }
        // Sideways: stop sliding along the axle.
        let slip_after = |j: f32| ((vs + j * inv_mass_s) / vf.abs().max(SLIP_SPEED_FLOOR)).atan();
        let stop = -vs / inv_mass_s.max(1e-6);
        let (js, sideways_slip) = grip(stop, load_dt, &c.sideways_friction, slip_after);
        w.hit = Some(WheelHit {
            collider: contact.collider,
            point: contact.point,
            normal: n,
            forward_dir: forward,
            sideways_dir: side,
            force: contact.load,
            forward_slip,
            sideways_slip,
        });
        Some(Tyre {
            index: i,
            body: handle,
            dynamic: body.is_dynamic(),
            at: contact.at,
            point: contact.point,
            ground: contact.ground,
            forward,
            side,
            inv_mass_f,
            inv_mass_s,
            spin_per_impulse: if locked { 0.0 } else { spin_per_impulse },
            radius,
            inertia,
            // A NaN Steer Angle (scripts) turns the axes to NaN; no grip then, not a panic.
            limit_f: finite_or_zero(jf.abs()),
            limit_s: finite_or_zero(js.abs()),
            most: c.forward_friction.peak().max(c.sideways_friction.peak()) * load_dt,
            locked,
            acc_f: 0.0,
            acc_s: 0.0,
        })
    }

    /// One pass of the tyre's friction: the impulse that would stop the slip now, with the
    /// running total kept within the tyre's limits (sequential impulses, so wheels sharing a
    /// body settle together instead of in Hierarchy order).
    fn solve_tyre(&mut self, t: &mut Tyre) {
        let Some(body) = self.world.bodies.get(t.body) else {
            return;
        };
        let v = self.step_velocity(body, t.at) - self.ground_velocity(t.ground, t.at);
        let (vf, vs) = (v.dot(t.forward), v.dot(t.side));
        let w = &mut self.wheels[t.index];
        let rim = w.omega * t.radius;
        let df = (rim - vf) / (t.spin_per_impulse + t.inv_mass_f).max(1e-6);
        let ds = -vs / t.inv_mass_s.max(1e-6);
        let mut f = (t.acc_f + df).clamp(-t.limit_f, t.limit_f);
        let mut s = (t.acc_s + ds).clamp(-t.limit_s, t.limit_s);
        // Both share the grip available (friction circle).
        let total = (f * f + s * s).sqrt();
        if total > t.most && total > 0.0 {
            f *= t.most / total;
            s *= t.most / total;
        }
        let (jf, js) = (f - t.acc_f, s - t.acc_s);
        t.acc_f = f;
        t.acc_s = s;
        if !t.locked {
            w.omega -= jf * t.radius / t.inertia;
        }
        if !t.dynamic {
            return;
        }
        let impulse = t.forward * jf + t.side * js;
        if let Some(b) = self.world.bodies.get_mut(t.body) {
            b.apply_impulse_at_point(rv(impulse), rv(t.at), true);
        }
        if let Some(g) = t.ground.and_then(|g| self.world.bodies.get_mut(g)) {
            g.apply_impulse_at_point(rv(-impulse), rv(t.point), true);
        }
    }

    /// Velocity of the ground body at a point (zero for static ground).
    fn ground_velocity(&self, ground: Option<rp::RigidBodyHandle>, at: Vec3) -> Vec3 {
        ground
            .and_then(|g| self.world.bodies.get(g))
            .map_or(Vec3::ZERO, |b| gv(b.velocity_at_point(rv(at))))
    }

    /// Velocity of a body's point averaged over the last step (rapier's velocity after a step
    /// already holds the whole step's gravity, but positions moved with the substeps').
    fn mean_velocity(&self, body: &rp::RigidBody, point: Vec3) -> Vec3 {
        let v = gv(body.velocity_at_point(rv(point)));
        let n = self
            .world
            .integration_parameters
            .num_solver_iterations
            .max(1) as f32;
        let dt = self.world.integration_parameters.dt;
        v - self.gravity() * body.gravity_scale() * dt * (n - 1.0) / (2.0 * n)
    }

    /// The velocity a body's point would keep on average over the coming step without the
    /// wheel's forces (what tyre friction has to cancel to hold the body still).
    fn step_velocity(&self, body: &rp::RigidBody, point: Vec3) -> Vec3 {
        let v = gv(body.velocity_at_point(rv(point)));
        let n = self
            .world
            .integration_parameters
            .num_solver_iterations
            .max(1) as f32;
        let dt = self.world.integration_parameters.dt;
        v + self.gravity() * body.gravity_scale() * dt * (n + 1.0) / (2.0 * n)
    }

    /// Wheel models follow their wheels; components get the simulated state (for scripts,
    /// gizmos and the Inspector).
    pub(super) fn sync_wheels(&self, scene: &mut Scene) {
        for w in &self.wheels {
            let Some(state) = self.wheel_state(w.entity) else {
                continue;
            };
            if !scene.contains(w.entity) {
                continue;
            }
            let model = match scene.world.get::<&mut WheelCollider>(w.entity) {
                Ok(mut c) => {
                    c.state = state;
                    c.wheel_model
                }
                Err(_) => continue,
            };
            let Some(model) = model.filter(|m| *m != w.entity && scene.contains(*m)) else {
                continue;
            };
            if let Some((p, r)) = world_pose_of(scene, w.entity) {
                scene.set_world_position(model, p);
                scene.set_world_rotation(model, r);
            }
        }
    }
}

/// WheelCollider.GetWorldPose: the wheel centre and rotation (steering and rolling) from the
/// object's current Transform and its simulated suspension.
pub fn world_pose_of(scene: &Scene, e: Entity) -> Option<(Vec3, Quat)> {
    let c = scene.world.get::<&WheelCollider>(e).ok()?;
    let (pos, rot, _) = world_pose(scene, e);
    let (extension, spin) = if c.state.simulated {
        (c.state.extension, c.state.spin)
    } else {
        (c.travel().1, 0.0)
    };
    Some(c.wheel_pose(pos, rot, extension, spin))
}

fn wheel_inertia(c: &WheelCollider) -> f32 {
    let r = c.radius.max(0.01);
    (0.5 * c.mass.max(0.01) * r * r).max(1e-3)
}

/// Tyre impulse along one direction and the slip it leaves: `stop` would end the sliding;
/// within the curve's extremum the tyre grips and applies it (the slip reported is where the
/// curve's rising part gives that much force), otherwise it slides with the curve's value at
/// the slip `slip_after` gives for a full-grip impulse.
fn grip(
    stop: f32,
    load_dt: f32,
    curve: &WheelFrictionCurve,
    slip_after: impl Fn(f32) -> f32,
) -> (f32, f32) {
    let stiffness = curve.stiffness.max(0.0);
    let peak = curve.extremum_value.max(0.0) * stiffness * load_dt;
    // Slip is opposite to the impulse that corrects it.
    let sign = -stop.signum();
    if stop.abs() <= peak {
        let used = if peak > 0.0 { stop.abs() / peak } else { 0.0 };
        let slip = curve.extremum_slip.max(0.0) * (1.0 - (1.0 - used).max(0.0).sqrt());
        return (stop, sign * slip);
    }
    let slip = slip_after(stop.signum() * peak);
    let mu = if slip.abs() <= curve.extremum_slip {
        curve.extremum_value.max(0.0) * stiffness
    } else {
        curve.evaluate(slip)
    };
    (stop.signum() * mu * load_dt, slip)
}

/// Brake torque and wheel damping slow the wheel towards rest (never past it).
fn slow_down(omega: f32, c: &WheelCollider, dt: f32, inertia: f32) -> f32 {
    let torque = c.brake_torque.max(0.0) + c.wheel_damping_rate.max(0.0) * omega.abs();
    let dw = torque * dt / inertia;
    if omega.abs() <= dw {
        0.0
    } else {
        omega - dw * omega.signum()
    }
}

/// Inverse of the body's effective mass for an impulse along `dir` at `point`.
fn inv_mass_at(body: &rp::RigidBody, point: Vec3, dir: Vec3) -> f32 {
    if !body.is_dynamic() {
        return 0.0;
    }
    let mp = body.mass_properties();
    let r = point - gv(mp.world_com);
    let rxd = rv(r.cross(dir));
    let lin = gv(mp.effective_inv_mass) * dir;
    let ang = gv(mp.effective_world_inv_inertia * rxd);
    (lin.dot(dir) + gv(rxd).dot(ang)).max(0.0)
}

/// `v`, or 0 when it is NaN or infinite.
fn finite_or_zero(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}
