//! Virtual cameras: a Camera Brain on the real camera shows the live Virtual Camera (highest
//! priority, ties → most recently enabled) and blends between them. Each virtual camera has a
//! lens, a body (Transposer, Third Person, Orbital / Free Look), an aim (Composer, Hard Look
//! At, Group Composer) and optional Collider, Confiner, Noise and Impulse Listener extensions.
//! Timeline Camera tracks can take a brain over (see [`CameraOverride`]). Runs only while
//! playing (see `CameraRig`); the Timeline window previews poses in edit mode with
//! [`solve::preview_pose`].
//!
//! - [`group`]: Target Groups (weighted members with radii) and their bounds.
//! - [`impulse`]: Impulse Sources, the envelope / falloff maths and the live impulses.

pub mod components;
pub mod group;
pub mod impulse;
pub mod noise;
pub mod rig;
pub mod solve;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_r19;

pub use components::*;
pub use group::{GroupBounds, GroupMember, GroupPositionMode, TargetGroup};
pub use impulse::{ImpulseManager, ImpulseShape, ImpulseSource, ImpulseTrigger};
pub use rig::{CameraOverride, CameraRig};
pub use solve::{CamState, SolveContext, damp};

/// Object references inside camera components, remapped on load / duplicate / prefab like
/// the scene's own (see `scene/file.rs`).
pub const VCAM_REFS: &[(&str, &str, &str)] = &[
    ("Virtual Camera", "confiner", "volume"),
    ("Target Group", "members", "target"),
];
