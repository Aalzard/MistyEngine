//! One virtual camera's pose for a frame: body (position), aim (rotation), then the collider
//! and noise corrections on top.

use glam::{EulerRot, Quat, Vec3};
use hecs::Entity;

use super::components::*;
use super::noise;
use crate::camera::euler_to_quat;
use crate::physics::Physics;
use crate::scene::Scene;

/// What a camera hands to the brain: where it is, where it looks and its lens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CamState {
    pub position: Vec3,
    /// Includes dutch and noise.
    pub rotation: Quat,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

impl CamState {
    /// `self` → `to` by weight `t` (0..1).
    pub fn lerp(&self, to: &CamState, t: f32) -> CamState {
        CamState {
            position: self.position.lerp(to.position, t),
            rotation: self.rotation.slerp(to.rotation, t).normalize(),
            fov: self.fov + (to.fov - self.fov) * t,
            near: self.near + (to.near - self.near) * t,
            far: self.far + (to.far - self.far) * t,
        }
    }
}

/// Per-camera memory between frames (reset when the camera is disabled).
#[derive(Clone, Copy, Debug, Default)]
pub struct VcamRuntime {
    /// False until the first solve: the first frame snaps instead of damping.
    pub valid: bool,
    /// Pose from body and aim, before dutch, collider and noise.
    pub raw_position: Vec3,
    pub raw_rotation: Quat,
    /// Damped follow point (third person and orbital).
    pub tracked: Vec3,
    /// How far the collider has pulled the camera towards its target.
    pub pulled_in: f32,
    pub noise_time: f32,
    pub output: Option<CamState>,
}

/// Frame inputs shared by every camera.
pub struct SolveContext<'a> {
    pub dt: f32,
    /// Width / height of the game image.
    pub aspect: f32,
    pub physics: Option<&'a Physics>,
    /// Input Manager axis value by name (orbital input).
    pub axis: &'a dyn Fn(&str) -> f32,
}

/// Fraction of a gap closed over `dt` when it should be ~99 % closed after `time` seconds.
pub fn damp(amount: f32, time: f32, dt: f32) -> f32 {
    if time <= 1e-4 {
        return amount;
    }
    amount * (1.0 - (-4.605 * dt.max(0.0) / time).exp())
}

fn damp3(v: Vec3, time: Vec3, dt: f32) -> Vec3 {
    Vec3::new(
        damp(v.x, time.x, dt),
        damp(v.y, time.y, dt),
        damp(v.z, time.z, dt),
    )
}

/// Heading and pitch (radians; positive pitch looks down) of a direction.
pub fn yaw_pitch(dir: Vec3) -> (f32, f32) {
    let d = dir.normalize_or(Vec3::Z);
    (d.x.atan2(d.z), -d.y.clamp(-1.0, 1.0).asin())
}

fn from_yaw_pitch(yaw: f32, pitch: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)
}

fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

fn target_pose(scene: &Scene, me: Entity, t: Option<Entity>) -> Option<(Entity, Vec3, Quat)> {
    let t = t.filter(|t| *t != me && scene.contains(*t))?;
    let (_, r, p) = scene.world_matrix(t).to_scale_rotation_translation();
    Some((t, p, r))
}

fn binding_rotation(binding: BindingMode, target: Quat) -> Quat {
    match binding {
        BindingMode::World => Quat::IDENTITY,
        BindingMode::LockToTarget => target,
        BindingMode::LockToTargetWithWorldUp => {
            let (yaw, _) = yaw_pitch(target * Vec3::Z);
            Quat::from_rotation_y(yaw)
        }
    }
}

/// Solve camera `e` for this frame. Orbit axis values in `vc` advance with input.
pub fn solve(
    scene: &Scene,
    e: Entity,
    vc: &mut VirtualCamera,
    st: &mut VcamRuntime,
    cx: &SolveContext,
) -> CamState {
    let (_, own_rot, own_pos) = scene.world_matrix(e).to_scale_rotation_translation();
    let valid = st.valid;
    let prev_pos = if valid { st.raw_position } else { own_pos };
    let prev_rot = if valid { st.raw_rotation } else { own_rot };
    let dt = cx.dt;
    let follow = target_pose(scene, e, vc.follow);

    // Body: position (and a rotation for bodies that imply one).
    let mut position = own_pos;
    let mut rotation = own_rot;
    if let Some((_, tpos, trot)) = follow {
        match vc.body {
            BodyKind::DoNothing => {}
            BodyKind::Transposer => {
                let t = &vc.transposer;
                let rb = binding_rotation(t.binding, trot);
                let desired = tpos + rb * t.offset;
                position = if valid {
                    let local = rb.inverse() * (desired - prev_pos);
                    prev_pos + rb * damp3(local, t.damping, dt)
                } else {
                    desired
                };
            }
            BodyKind::ThirdPerson => {
                let t = &vc.third_person;
                let tracked = if valid {
                    // Dead zone: screen fractions at the camera distance, in metres.
                    let half_h =
                        t.camera_distance.max(0.01) * (vc.lens.fov.to_radians() * 0.5).tan();
                    let half = glam::Vec2::new(half_h * cx.aspect, half_h) * t.dead_zone * 0.5;
                    let mut local = trot.inverse() * (tpos - st.tracked);
                    local.x = local.x.signum() * (local.x.abs() - half.x).max(0.0);
                    local.y = local.y.signum() * (local.y.abs() - half.y).max(0.0);
                    st.tracked + trot * damp3(local, t.damping, dt)
                } else {
                    tpos
                };
                st.tracked = tracked;
                let side = t.camera_side.clamp(0.0, 1.0) * 2.0 - 1.0;
                let o = t.shoulder_offset;
                let hand = tracked
                    + trot * Vec3::new(o.x * side, o.y, o.z)
                    + trot * Vec3::Y * t.vertical_arm_length;
                position = hand - trot * Vec3::Z * t.camera_distance;
                rotation = trot;
            }
            BodyKind::Orbital => {
                let o = &mut vc.orbital;
                for axis in [&mut o.x_axis, &mut o.y_axis] {
                    let input = if axis.input_axis.is_empty() {
                        0.0
                    } else {
                        (cx.axis)(&axis.input_axis)
                    };
                    axis.apply(input, dt);
                }
                let tracked = if valid {
                    st.tracked + damp3(tpos - st.tracked, o.damping, dt)
                } else {
                    tpos
                };
                st.tracked = tracked;
                let heading = match o.binding {
                    BindingMode::World => 0.0,
                    _ => yaw_pitch(trot * Vec3::Z).0,
                };
                let q = from_yaw_pitch(
                    heading + o.x_axis.value.to_radians(),
                    o.y_axis.value.to_radians(),
                );
                let center = tracked + Vec3::Y * o.height_offset;
                position = center - q * Vec3::Z * o.radius;
                rotation = q;
            }
        }
    }

    // Aim: rotation.
    let look = target_pose(scene, e, vc.look_at);
    let mut look_point = None;
    if let Some((_, lpos, lrot)) = look {
        match vc.aim {
            AimKind::DoNothing => {}
            AimKind::HardLookAt => {
                let (yaw, pitch) = yaw_pitch(lpos - position);
                rotation = from_yaw_pitch(yaw, pitch);
                look_point = Some(lpos);
            }
            AimKind::Composer => {
                let p = lpos + lrot * vc.composer.tracked_offset;
                rotation = compose(
                    &vc.composer,
                    &vc.lens,
                    cx.aspect,
                    position,
                    prev_rot,
                    p,
                    valid,
                    dt,
                );
                look_point = Some(p);
            }
        }
    }
    st.raw_position = position;
    st.raw_rotation = rotation;
    st.valid = true;

    // Collider: pull in front of whatever blocks the view of the target.
    let mut final_pos = position;
    let origin = look_point.or(follow.map(|f| f.1));
    if vc.collider.enabled
        && let (Some(origin), Some(physics)) = (origin, cx.physics)
    {
        let c = &vc.collider;
        let to_cam = position - origin;
        let dist = to_cam.length();
        let mut want = 0.0;
        if dist > 1e-4 {
            let dir = to_cam / dist;
            let ignored = |hit: Entity| {
                hit == e
                    || [vc.follow, vc.look_at]
                        .into_iter()
                        .flatten()
                        .any(|t| t == hit || scene.is_ancestor(t, hit))
                    || (!c.ignore_tag.is_empty()
                        && scene.contains(hit)
                        && scene.game_object(hit).tag == c.ignore_tag)
            };
            let blocked = physics
                .raycast_all(origin, dir, dist + c.camera_radius, c.collide_against)
                .into_iter()
                .find(|h| !ignored(h.entity));
            if let Some(hit) = blocked {
                let free = (hit.distance - c.camera_radius).max(0.0);
                let limit = (dist - c.minimum_distance).max(0.0);
                want = (dist - free).clamp(0.0, limit);
            }
            let gap = want - st.pulled_in;
            st.pulled_in = if !valid {
                want
            } else if gap > 0.0 {
                st.pulled_in + damp(gap, c.damping_when_occluded, dt)
            } else {
                st.pulled_in + damp(gap, c.damping, dt)
            };
            final_pos = origin + dir * (dist - st.pulled_in);
        }
    } else {
        st.pulled_in = 0.0;
    }

    // Dutch, then noise in camera space.
    let mut final_rot = rotation * Quat::from_rotation_z(vc.lens.dutch.to_radians());
    if vc.noise.enabled {
        st.noise_time += dt;
        let (dp, dr) = noise::sample(&vc.noise, st.noise_time);
        final_pos += final_rot * dp;
        final_rot *= euler_to_quat(dr);
    }
    let out = CamState {
        position: final_pos,
        rotation: final_rot.normalize(),
        fov: vc.lens.fov,
        near: vc.lens.near,
        far: vc.lens.far,
    };
    st.output = Some(out);
    out
}

/// Composer rotation: keep `target` inside the dead zone (damped), never outside the soft zone.
#[allow(clippy::too_many_arguments)]
fn compose(
    c: &Composer,
    lens: &Lens,
    aspect: f32,
    position: Vec3,
    prev: Quat,
    target: Vec3,
    valid: bool,
    dt: f32,
) -> Quat {
    let tan_y = (lens.fov.clamp(1.0, 179.0).to_radians() * 0.5).tan();
    let tan_x = tan_y * aspect.max(1e-3);
    // Screen fraction → angle off the view axis (right / up positive).
    let ax = |s: f32| ((2.0 * s - 1.0) * tan_x).atan();
    let ay = |s: f32| ((1.0 - 2.0 * s) * tan_y).atan();
    let (ty, tp) = yaw_pitch(target - position);
    if !valid {
        return from_yaw_pitch(ty - ax(c.screen_x), tp + ay(c.screen_y));
    }
    let (dead, soft) = c.zones();
    let (mut yaw, mut pitch) = yaw_pitch(prev * Vec3::Z);
    // Horizontal: target angle to the right of the view axis.
    let excess = |v: f32, lo: f32, hi: f32| {
        if v > hi {
            v - hi
        } else if v < lo {
            v - lo
        } else {
            0.0
        }
    };
    let dx = wrap_angle(ty - yaw);
    yaw += damp(
        excess(dx, ax(dead[0]), ax(dead[2])),
        c.horizontal_damping,
        dt,
    );
    yaw += excess(wrap_angle(ty - yaw), ax(soft[0]), ax(soft[2]));
    // Vertical: target angle above the view axis (screen y grows downwards).
    let v = pitch - tp;
    pitch -= damp(excess(v, ay(dead[3]), ay(dead[1])), c.vertical_damping, dt);
    pitch -= excess(pitch - tp, ay(soft[3]), ay(soft[1]));
    let limit = 89f32.to_radians();
    from_yaw_pitch(yaw, pitch.clamp(-limit, limit))
}
