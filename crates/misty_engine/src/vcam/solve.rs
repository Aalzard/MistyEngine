//! One virtual camera's pose for a frame: body (position), aim (rotation), then the collider
//! and noise corrections on top.

use glam::{EulerRot, Quat, Vec3};
use hecs::Entity;

use super::components::*;
use super::{group, noise};
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
    /// Group Composer: distance to the group centre and field of view it settled on.
    pub group_distance: f32,
    pub group_fov: f32,
    /// Confiner: how far the camera is pushed back inside its volume.
    pub confine: Vec3,
}

/// Frame inputs shared by every camera.
pub struct SolveContext<'a> {
    pub dt: f32,
    /// Width / height of the game image.
    pub aspect: f32,
    pub physics: Option<&'a Physics>,
    /// Input Manager axis value by name (orbital input).
    pub axis: &'a dyn Fn(&str) -> f32,
    /// Input System action value by name ("Map/Action"), for orbit axes that use one.
    pub action: &'a dyn Fn(&str) -> glam::Vec2,
}

/// No Input System actions (tests, edit-mode previews).
pub fn no_action(_: &str) -> glam::Vec2 {
    glam::Vec2::ZERO
}

/// No Input Manager axes.
pub fn no_axis(_: &str) -> f32 {
    0.0
}

/// The value of Input System action `name` ("Map/Action", or just "Action" in any map) from
/// the first playing Player Input that has it enabled, as a 2D vector (a 1D action gives x).
pub fn action_value(system: &crate::input::actions::InputSystem, name: &str) -> glam::Vec2 {
    for p in &system.players {
        let Some(set) = system.set(p.set) else {
            continue;
        };
        if let Some((m, a)) = set.asset.find_action(name)
            && let Some(st) = set.state(m, a)
            && st.enabled
        {
            return st.value.as_vec2();
        }
    }
    glam::Vec2::ZERO
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
                for (i, axis) in [&mut o.x_axis, &mut o.y_axis].into_iter().enumerate() {
                    let input = if !axis.input_action.is_empty() {
                        let v = (cx.action)(&axis.input_action);
                        if i == 0 { v.x } else { v.y }
                    } else if axis.input_axis.is_empty() {
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
                let yaw = heading + o.x_axis.value.to_radians();
                let center = tracked + Vec3::Y * o.height_offset;
                match o.style {
                    OrbitStyle::Sphere => {
                        let q = from_yaw_pitch(yaw, o.y_axis.value.to_radians());
                        position = center - q * Vec3::Z * o.radius;
                        rotation = q;
                    }
                    OrbitStyle::ThreeRings => {
                        let (h, r) = o.ring_at(o.y_axis.value);
                        position = center + Vec3::Y * h - Quat::from_rotation_y(yaw) * Vec3::Z * r;
                        let (ly, lp) = yaw_pitch(center - position);
                        rotation = from_yaw_pitch(ly, lp);
                    }
                }
            }
        }
    }

    // Confiner: back inside the volume (before aiming, so the aim sees the final place).
    if vc.confiner.enabled
        && let Some(v) = vc.confiner.volume.filter(|v| *v != e && scene.contains(*v))
    {
        let inside = confine(scene, v, position);
        let push = inside - position;
        st.confine = if valid && vc.confiner.damping > 1e-4 {
            st.confine + (push - st.confine) * damp(1.0, vc.confiner.damping, dt)
        } else {
            push
        };
        position += st.confine;
    } else {
        st.confine = Vec3::ZERO;
    }

    // Aim: rotation (the Group Composer may also dolly and zoom).
    let look = target_pose(scene, e, vc.look_at);
    let mut look_point = None;
    let mut dolly = None;
    let mut fov = vc.lens.fov;
    if let Some((look_e, lpos, lrot)) = look {
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
            AimKind::GroupComposer => {
                let b = group::group_bounds(scene, look_e).unwrap_or(group::GroupBounds {
                    center: lpos,
                    radius: 0.5,
                    average: lpos,
                });
                let (p, r, f) = frame_group(vc, cx.aspect, position, prev_rot, b, st, valid, dt);
                dolly = Some(p);
                rotation = r;
                fov = f;
                look_point = Some(b.center);
            }
        }
    }
    // The object keeps the body's position (a dolly moves only the shown camera).
    st.raw_position = position;
    st.raw_rotation = rotation;
    st.valid = true;
    let position = dolly.unwrap_or(position);

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
        fov,
        near: vc.lens.near,
        far: vc.lens.far,
    };
    st.output = Some(out);
    out
}

/// Tangent of the half angle the group has to fit in: the vertical one, the horizontal one or
/// the tighter of the two.
fn framing_tan(mode: FramingMode, fov: f32, aspect: f32) -> f32 {
    let tv = (fov.clamp(1.0, 179.0).to_radians() * 0.5).tan();
    let th = tv * aspect.max(1e-3);
    match mode {
        FramingMode::Horizontal => th,
        FramingMode::Vertical => tv,
        FramingMode::HorizontalAndVertical => tv.min(th),
    }
}

/// Group Composer: from the body's `position`, move along the line to the group centre
/// (dolly) and / or change the field of view (zoom) so the group's bounding sphere fills the
/// framing size, then aim at the centre with the Composer's settings. Returns the shown
/// position, rotation and vertical field of view.
#[allow(clippy::too_many_arguments)]
fn frame_group(
    vc: &VirtualCamera,
    aspect: f32,
    position: Vec3,
    prev_rot: Quat,
    b: group::GroupBounds,
    st: &mut VcamRuntime,
    valid: bool,
    dt: f32,
) -> (Vec3, Quat, f32) {
    let g = &vc.group_framing;
    let framing = g.framing_size.clamp(0.01, 10.0);
    let r = b.radius.max(0.01);
    let to_cam = position - b.center;
    let d0 = to_cam.length();
    let dir = if d0 > 1e-3 {
        to_cam / d0
    } else {
        -(prev_rot * Vec3::Z)
    };
    let mut d = d0.max(1e-3);
    if g.adjustment != GroupAdjustment::ZoomOnly {
        // Distance at which the sphere's half angle is `framing` of the screen's.
        let ta = framing * framing_tan(g.framing_mode, vc.lens.fov, aspect);
        let fit = r * (1.0 + ta * ta).sqrt() / ta.max(1e-4);
        let near = g.minimum_distance.max(0.0);
        d = fit
            .clamp(d0 - g.max_dolly_in.max(0.0), d0 + g.max_dolly_out.max(0.0))
            .clamp(near, g.maximum_distance.max(near));
    }
    let mut fov = vc.lens.fov;
    if g.adjustment != GroupAdjustment::DollyOnly {
        let (lo, hi) = (
            g.minimum_fov.clamp(1.0, 179.0),
            g.maximum_fov.clamp(1.0, 179.0),
        );
        fov = if d <= r * 1.0001 {
            hi
        } else {
            // Tangent of the sphere's half angle, then of the screen half angle it needs.
            let need = r / (d * d - r * r).sqrt() / framing;
            let tv = match g.framing_mode {
                FramingMode::Horizontal => need / aspect.max(1e-3),
                FramingMode::Vertical => need,
                FramingMode::HorizontalAndVertical => need / aspect.clamp(1e-3, 1.0),
            };
            (2.0 * tv.atan().to_degrees()).clamp(lo.min(hi), hi.max(lo))
        };
    }
    if valid && g.damping > 1e-4 {
        d = st.group_distance + damp(d - st.group_distance, g.damping, dt);
        fov = st.group_fov + damp(fov - st.group_fov, g.damping, dt);
    }
    st.group_distance = d;
    st.group_fov = fov;
    let shown = b.center + dir * d;
    let lens = Lens { fov, ..vc.lens };
    let rot = compose(
        &vc.composer,
        &lens,
        aspect,
        shown,
        prev_rot,
        b.center,
        valid,
        dt,
    );
    (shown, rot, fov)
}

/// `p` moved to the nearest point inside the confining volume of `v`: its Box Collider, else
/// its Sphere Collider, else the unit cube of its transform.
pub fn confine(scene: &Scene, v: Entity, p: Vec3) -> Vec3 {
    use crate::scene::{BoxCollider, SphereCollider};
    let m = scene.world_matrix(v);
    if let Ok(s) = scene.world.get::<&SphereCollider>(v) {
        let (scale, _, _) = m.to_scale_rotation_translation();
        let c = m.transform_point3(s.center);
        let r = s.radius.max(0.0) * scale.abs().max_element();
        let off = p - c;
        return if off.length() <= r {
            p
        } else {
            c + off.normalize_or_zero() * r
        };
    }
    let (center, size) = scene
        .world
        .get::<&BoxCollider>(v)
        .map(|b| (b.center, b.size))
        .unwrap_or((Vec3::ZERO, Vec3::ONE));
    let local = m.inverse().transform_point3(p) - center;
    let half = size.abs() * 0.5;
    let clamped = local.clamp(-half, half);
    if clamped == local {
        p
    } else {
        m.transform_point3(clamped + center)
    }
}

/// A camera's pose right now without any memory (no damping): edit-mode previews (the
/// Timeline window's Camera track) use it.
pub fn preview_pose(scene: &Scene, e: Entity, aspect: f32) -> Option<CamState> {
    let mut vc = scene
        .world
        .get::<&VirtualCamera>(e)
        .ok()
        .map(|v| (*v).clone())?;
    let mut st = VcamRuntime::default();
    vc.noise.enabled = false;
    let cx = SolveContext {
        dt: 0.0,
        aspect,
        physics: None,
        axis: &no_axis,
        action: &no_action,
    };
    Some(solve(scene, e, &mut vc, &mut st, &cx))
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
