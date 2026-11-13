//! World-space constraint solvers working directly on scene Transforms.

use glam::{EulerRot, Mat3, Quat, Vec3};
use hecs::Entity;

use super::components::*;
use crate::scene::Scene;

const EPS: f32 = 1e-5;

pub(super) fn pos(s: &Scene, e: Entity) -> Vec3 {
    s.world_position(e)
}

pub(super) fn rot(s: &Scene, e: Entity) -> Quat {
    s.world_rotation(e).normalize()
}

/// Turn `e` by the world-space rotation `delta` about its own pivot.
fn turn(s: &mut Scene, e: Entity, delta: Quat) {
    let r = rot(s, e);
    s.set_world_rotation(e, (delta * r).normalize());
}

/// Interior angle opposite side `a` of a triangle with sides `a`, `b`, `c`.
fn triangle_angle(a: f32, b: f32, c: f32) -> f32 {
    let d = (b * b + c * c - a * a) / (2.0 * b * c).max(EPS);
    d.clamp(-1.0, 1.0).acos()
}

/// Rotation turning `from` onto `to` (both any length; identity when degenerate).
fn arc(from: Vec3, to: Vec3) -> Quat {
    match (from.try_normalize(), to.try_normalize()) {
        (Some(a), Some(b)) => Quat::from_rotation_arc(a, b),
        _ => Quat::IDENTITY,
    }
}

/// Signed angle from `a` to `b` around `axis` (all three roughly perpendicular).
fn signed_angle(a: Vec3, b: Vec3, axis: Vec3) -> f32 {
    a.cross(b).dot(axis).atan2(a.dot(b))
}

/// Two Bone IK towards world position `t` (already blended by the position weight).
/// `hint`: world position and weight. Returns nothing; bones are turned in place.
pub(super) fn two_bone(
    s: &mut Scene,
    [root, mid, tip]: [Entity; 3],
    t: Vec3,
    hint: Option<(Vec3, f32)>,
) {
    let a = pos(s, root);
    let b = pos(s, mid);
    let c = pos(s, tip);
    let ab = b - a;
    let bc = c - b;
    let (lab, lbc) = (ab.length(), bc.length());
    // `!(..)` so NaN bone positions return too (`clamp` below panics on NaN).
    if !(lab >= EPS && lbc >= EPS) || !lab.is_finite() || !lbc.is_finite() {
        return;
    }
    let lac = (c - a).length();
    // Keep the target within the reach of the chain (straight when too far). With a tip
    // bone shorter than EPS * the arm the two bounds crossed and `clamp` panicked.
    let lo = (lab - lbc).abs() + EPS;
    let hi = ((lab + lbc) * (1.0 - EPS)).max(lo);
    let lat = (t - a).length();
    if !lat.is_finite() {
        return;
    }
    let lat = lat.clamp(lo, hi);

    // Bend axis: the current bend plane, or the hint side, or any perpendicular.
    let mut axis = ab.cross(bc);
    if axis.length_squared() < 1e-8 {
        axis = hint
            .map(|(h, _)| (h - a).cross(bc))
            .filter(|v| v.length_squared() > 1e-8)
            .unwrap_or_else(|| ab.any_orthonormal_vector());
    }
    let axis = axis.normalize();
    let old_b = triangle_angle(lac, lab, lbc);
    let new_b = triangle_angle(lat, lab, lbc);
    turn(s, mid, Quat::from_axis_angle(axis, old_b - new_b));

    // Swing the whole chain so the tip lies on the line to the target.
    let c = pos(s, tip);
    turn(s, root, arc(c - a, t - a));

    // Spin the chain around the root → target line so the middle joint faces the hint.
    if let Some((h, w)) = hint
        && w > 0.0
    {
        let c = pos(s, tip);
        let b = pos(s, mid);
        if let Some(n) = (c - a).try_normalize() {
            let project = |v: Vec3| v - n * v.dot(n);
            let pb = project(b - a);
            let ph = project(h - a);
            if pb.length_squared() > 1e-8 && ph.length_squared() > 1e-8 {
                let angle = signed_angle(pb, ph, n);
                turn(s, root, Quat::from_axis_angle(n, angle * w.min(1.0)));
            }
        }
    }
}

/// Parent chain from `root` down to `tip` (inclusive), or `None` when `tip` is not below it.
pub(super) fn chain(s: &Scene, root: Entity, tip: Entity) -> Option<Vec<Entity>> {
    let mut out = vec![tip];
    let mut cur = tip;
    while cur != root {
        cur = s.parent(cur)?;
        out.push(cur);
    }
    out.reverse();
    (out.len() >= 2).then_some(out)
}

/// Chain IK: bend `bones` so the last one reaches `t`. `rotation_weight` scales each bone's turn.
pub(super) fn chain_ik(
    s: &mut Scene,
    bones: &[Entity],
    t: Vec3,
    solver: ChainSolver,
    iterations: u32,
    tolerance: f32,
    rotation_weight: f32,
) {
    let n = bones.len() - 1;
    let tolerance = tolerance.max(1e-6);
    let iterations = iterations.max(1);
    match solver {
        ChainSolver::Fabrik => {
            let mut p: Vec<Vec3> = bones.iter().map(|b| pos(s, *b)).collect();
            let len: Vec<f32> = (0..n).map(|i| (p[i + 1] - p[i]).length()).collect();
            let total: f32 = len.iter().sum();
            let start = p[0];
            if (t - start).length() >= total {
                for i in 0..n {
                    let dir = (t - p[i]).try_normalize().unwrap_or(Vec3::Y);
                    p[i + 1] = p[i] + dir * len[i];
                }
            } else {
                for _ in 0..iterations {
                    if (p[n] - t).length() <= tolerance {
                        break;
                    }
                    p[n] = t;
                    for i in (0..n).rev() {
                        let dir = (p[i] - p[i + 1]).try_normalize().unwrap_or(Vec3::Y);
                        p[i] = p[i + 1] + dir * len[i];
                    }
                    p[0] = start;
                    for i in 0..n {
                        let dir = (p[i + 1] - p[i]).try_normalize().unwrap_or(Vec3::Y);
                        p[i + 1] = p[i] + dir * len[i];
                    }
                }
            }
            for i in 0..n {
                let from = pos(s, bones[i + 1]) - pos(s, bones[i]);
                let want = p[i + 1] - p[i];
                let d = Quat::IDENTITY.slerp(arc(from, want), rotation_weight);
                turn(s, bones[i], d);
            }
        }
        ChainSolver::Ccd => {
            for _ in 0..iterations {
                if (pos(s, bones[n]) - t).length() <= tolerance {
                    break;
                }
                for i in (0..n).rev() {
                    let pj = pos(s, bones[i]);
                    let d = arc(pos(s, bones[n]) - pj, t - pj);
                    turn(s, bones[i], Quat::IDENTITY.slerp(d, rotation_weight));
                }
            }
        }
    }
}

/// World rotation turning local `aim` onto `dir` with local `up` as close to `up_w` as
/// possible (`None` = smallest turn from `current`).
pub(super) fn aim_rotation(
    current: Quat,
    aim: Vec3,
    up: Vec3,
    dir: Vec3,
    up_w: Option<Vec3>,
) -> Quat {
    let Some(d) = dir.try_normalize() else {
        return current;
    };
    let free = || (arc(current * aim, d) * current).normalize();
    // An up axis along the aim axis, or an up direction along the aim, leaves it free.
    let Some(u) = up_w else {
        return free();
    };
    if aim.cross(up).length_squared() < 1e-6 {
        return free();
    }
    let Some(yw) = (u - d * u.dot(d)).try_normalize() else {
        return free();
    };
    let yl = (up - aim * up.dot(aim)).normalize();
    let local = Mat3::from_cols(aim, yl, aim.cross(yl));
    let world = Mat3::from_cols(d, yw, d.cross(yw));
    Quat::from_mat3(&(world * local.transpose())).normalize()
}

/// Mask and clamp the local rotation `delta` (from the input pose) per Multi-Aim's limits.
pub(super) fn limit(delta: Quat, axes: [bool; 3], min: f32, max: f32) -> Quat {
    if axes == [true; 3] && min <= -180.0 && max >= 180.0 {
        return delta;
    }
    let (y, x, z) = delta.to_euler(EulerRot::YXZ);
    let (lo, hi) = (min.min(max).to_radians(), max.max(min).to_radians());
    let f = |v: f32, on: bool| if on { v.clamp(lo, hi) } else { 0.0 };
    Quat::from_euler(EulerRot::YXZ, f(y, axes[1]), f(x, axes[0]), f(z, axes[2]))
}
