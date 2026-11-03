//! Joint and Character Controller components (the Rigidbody and colliders are in `components`).
//!
//! Anchors are in the object's local space (scaled with it, like positions of children).
//! `connected_body` is another object with a Rigidbody; `None` connects to the world.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::components::entity_bits;

/// Glues the object to its connected body (or the world) at their current relative pose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FixedJoint {
    #[serde(with = "entity_bits::option")]
    pub connected_body: Option<hecs::Entity>,
    /// Let the two bodies collide with each other.
    pub enable_collision: bool,
}

/// Spring that pulls a hinge towards `target_position` (degrees).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct JointSpring {
    pub spring: f32,
    pub damper: f32,
    pub target_position: f32,
}

/// Motor that drives a hinge at `target_velocity` (degrees/second) with at most `force` torque.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct JointMotor {
    pub target_velocity: f32,
    pub force: f32,
}

/// Hinge angle range in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct JointLimits {
    pub min: f32,
    pub max: f32,
}

/// Rotation around one axis through the anchor (doors, wheels, pendulums).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HingeJoint {
    #[serde(with = "entity_bits::option")]
    pub connected_body: Option<hecs::Entity>,
    pub anchor: Vec3,
    pub axis: Vec3,
    /// Compute the connected anchor from the anchor's position when Play starts.
    pub auto_configure_connected_anchor: bool,
    /// Connected body's local space (world space without a connected body).
    pub connected_anchor: Vec3,
    pub use_spring: bool,
    pub spring: JointSpring,
    pub use_motor: bool,
    pub motor: JointMotor,
    pub use_limits: bool,
    pub limits: JointLimits,
    pub enable_collision: bool,
}

impl Default for HingeJoint {
    fn default() -> Self {
        Self {
            connected_body: None,
            anchor: Vec3::ZERO,
            axis: Vec3::X,
            auto_configure_connected_anchor: true,
            connected_anchor: Vec3::ZERO,
            use_spring: false,
            spring: JointSpring::default(),
            use_motor: false,
            motor: JointMotor::default(),
            use_limits: false,
            limits: JointLimits::default(),
            enable_collision: false,
        }
    }
}

/// Spring between the anchor and the connected anchor; no force while their distance is
/// between `min_distance` and `max_distance`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpringJoint {
    #[serde(with = "entity_bits::option")]
    pub connected_body: Option<hecs::Entity>,
    pub anchor: Vec3,
    pub auto_configure_connected_anchor: bool,
    pub connected_anchor: Vec3,
    pub spring: f32,
    pub damper: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub enable_collision: bool,
}

impl Default for SpringJoint {
    fn default() -> Self {
        Self {
            connected_body: None,
            anchor: Vec3::ZERO,
            auto_configure_connected_anchor: true,
            connected_anchor: Vec3::ZERO,
            spring: 10.0,
            damper: 0.2,
            min_distance: 0.0,
            max_distance: 0.0,
            enable_collision: false,
        }
    }
}

/// Capsule moved by scripts with `Move` / `SimpleMove`: slides along walls, climbs steps and
/// slopes, isn't pushed by physics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterController {
    /// Steepest slope (degrees) the controller walks up.
    pub slope_limit: f32,
    /// Highest step it climbs.
    pub step_offset: f32,
    /// Gap kept between the capsule and what it touches.
    pub skin_width: f32,
    /// Moves shorter than this are ignored.
    pub min_move_distance: f32,
    pub center: Vec3,
    pub radius: f32,
    pub height: f32,
}

impl Default for CharacterController {
    fn default() -> Self {
        Self {
            slope_limit: 45.0,
            step_offset: 0.3,
            skin_width: 0.08,
            min_move_distance: 0.001,
            center: Vec3::ZERO,
            radius: 0.5,
            height: 2.0,
        }
    }
}
