//! Root motion: the movement of a clip's root curves turned into GameObject movement.
//!
//! The root is the Animator's own object (its position and Y rotation curves) or a bone named by
//! the Animator's Root Motion Bone (its horizontal position and Y rotation). Each frame the
//! movement of every playing clip between its previous and current time is measured in the
//! clip's own root space (so a walk forwards moves the character along its facing, whichever
//! way it faces), across loop wraps, and the weighted sum moves the object. The root keys then
//! stay out of the pose: the object's own curves are skipped, a bone is held at its first-frame
//! horizontal position and heading.

use glam::{Quat, Vec3};

use super::clip::{AnimationClip, Property};

/// Most loop wraps measured in one frame (a huge time step would otherwise loop for long).
const MAX_SEGMENTS: usize = 64;

/// Whether a curve on `path` animating `p` drives root motion for root path `root`.
pub fn is_root_key(root: &str, path: &str, p: Property) -> bool {
    if path != root {
        return false;
    }
    match p {
        Property::Position(1) => root.is_empty(),
        Property::Position(_) | Property::Rotation(1) => true,
        _ => false,
    }
}

/// Root position and heading (degrees) of a clip at clip time `t` (missing curves count as 0).
fn root_at(clip: &AnimationClip, root: &str, t: f32) -> (Vec3, f32) {
    let v = |p: Property| clip.curve(root, p).map_or(0.0, |c| c.evaluate(t));
    (
        Vec3::new(
            v(Property::Position(0)),
            if root.is_empty() {
                v(Property::Position(1))
            } else {
                0.0
            },
            v(Property::Position(2)),
        ),
        v(Property::Rotation(1)),
    )
}

/// A movement: translation in the space of the root where it starts, then a turn (degrees).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Delta {
    pub position: Vec3,
    pub yaw: f32,
}

impl Delta {
    /// This movement followed by `next` (measured from where this one ends).
    pub fn then(self, next: Delta) -> Delta {
        Delta {
            position: self.position + yaw_quat(self.yaw) * next.position,
            yaw: self.yaw + next.yaw,
        }
    }

    /// The movement that undoes this one.
    pub fn inverse(self) -> Delta {
        Delta {
            position: -(yaw_quat(-self.yaw) * self.position),
            yaw: -self.yaw,
        }
    }

    pub fn scaled(self, w: f32) -> Delta {
        Delta {
            position: self.position * w,
            yaw: self.yaw * w,
        }
    }
}

pub fn yaw_quat(degrees: f32) -> Quat {
    Quat::from_rotation_y(degrees.to_radians())
}

/// Movement within one loop between clip times `a` ≤ `b`.
fn segment(clip: &AnimationClip, root: &str, a: f32, b: f32) -> Delta {
    let (pa, ya) = root_at(clip, root, a);
    let (pb, yb) = root_at(clip, root, b);
    Delta {
        position: yaw_quat(-ya) * (pb - pa),
        yaw: yb - ya,
    }
}

/// Root movement of a clip from normalised time `a` to `b` (looping clips wrap; others hold at
/// their ends).
pub fn clip_delta(clip: &AnimationClip, root: &str, a: f32, b: f32, looping: bool) -> Delta {
    let len = clip.length.max(1e-4);
    if b < a {
        return clip_delta(clip, root, b, a, looping).inverse();
    }
    if !looping {
        let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
        return segment(clip, root, a * len, b * len);
    }
    let mut total = Delta::default();
    let mut k = a.floor();
    for _ in 0..MAX_SEGMENTS {
        if k >= b {
            break;
        }
        let (s, e) = (a.max(k), b.min(k + 1.0));
        if e > s {
            total = total.then(segment(clip, root, (s - k) * len, (e - k) * len));
        }
        k += 1.0;
    }
    total
}

/// Root keys hold these first-frame values in the pose (a bone root keeps its height and tilt
/// but not its travel).
pub fn pinned_value(clip: &AnimationClip, root: &str, p: Property) -> Option<f32> {
    if root.is_empty() {
        return None;
    }
    clip.curve(root, p).map(|c| c.evaluate(0.0))
}
