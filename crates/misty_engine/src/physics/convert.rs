//! rapier uses its own glam version: values convert through arrays.

use glam::{Quat, Vec3};
use rapier3d::prelude as rp;

pub(crate) fn rv(v: Vec3) -> rp::Vector {
    rp::Vector::new(v.x, v.y, v.z)
}

pub(crate) fn gv(v: rp::Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub(crate) fn rq(q: Quat) -> rp::Rotation {
    rp::Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}

pub(crate) fn gq(q: rp::Rotation) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

pub(crate) fn pose(t: Vec3, r: Quat) -> rp::Pose {
    rp::Pose::from_parts(rv(t), rq(r))
}

/// Translation and rotation of a rapier pose.
pub(crate) fn parts(p: &rp::Pose) -> (Vec3, Quat) {
    (gv(p.translation), gq(p.rotation))
}
