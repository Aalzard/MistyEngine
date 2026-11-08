//! Joint, Ragdoll and Character Controller components (the Rigidbody and colliders are in
//! `components`, the Cloth in `cloth_component`).
//!
//! Anchors are in the object's local space (scaled with it, like positions of children).
//! `connected_body` is another object with a Rigidbody; `None` connects to the world.
//! Every joint breaks when the force (newtons) or torque (newton metres) holding it together
//! goes over `break_force` / `break_torque`; unbreakable is infinity (written as null).

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::components::entity_bits;

/// Break Force / Break Torque values: infinity is stored as null (JSON has no infinity).
pub mod infinite {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if v.is_finite() {
            v.serialize(s)
        } else {
            Option::<f32>::None.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        Ok(Option::<f32>::deserialize(d)?.unwrap_or(f32::INFINITY))
    }
}

/// Glues the object to its connected body (or the world) at their current relative pose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FixedJoint {
    #[serde(with = "entity_bits::option")]
    pub connected_body: Option<hecs::Entity>,
    /// Let the two bodies collide with each other.
    pub enable_collision: bool,
    #[serde(with = "infinite")]
    pub break_force: f32,
    #[serde(with = "infinite")]
    pub break_torque: f32,
}

impl Default for FixedJoint {
    fn default() -> Self {
        Self {
            connected_body: None,
            enable_collision: false,
            break_force: f32::INFINITY,
            break_torque: f32::INFINITY,
        }
    }
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
    #[serde(with = "infinite")]
    pub break_force: f32,
    #[serde(with = "infinite")]
    pub break_torque: f32,
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
            break_force: f32::INFINITY,
            break_torque: f32::INFINITY,
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
    #[serde(with = "infinite")]
    pub break_force: f32,
    #[serde(with = "infinite")]
    pub break_torque: f32,
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
            break_force: f32::INFINITY,
            break_torque: f32::INFINITY,
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

/// How one axis of a Configurable Joint may move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JointMotion {
    #[default]
    Locked,
    /// Within the axis' limit.
    Limited,
    Free,
}

impl JointMotion {
    pub const ALL: [Self; 3] = [Self::Locked, Self::Limited, Self::Free];

    pub fn name(self) -> &'static str {
        match self {
            Self::Locked => "Locked",
            Self::Limited => "Limited",
            Self::Free => "Free",
        }
    }
}

/// A joint limit: `limit` (metres or degrees), how much of the speed comes back when it is hit
/// (`bounciness`, 0–1) and how early it starts acting (`contact_distance`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoftJointLimit {
    pub limit: f32,
    pub bounciness: f32,
    pub contact_distance: f32,
}

impl SoftJointLimit {
    pub fn new(limit: f32) -> Self {
        Self {
            limit,
            ..Default::default()
        }
    }
}

/// A spring that makes a limit soft: zero spring = a hard stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoftJointLimitSpring {
    pub spring: f32,
    pub damper: f32,
}

/// A drive pulls an axis towards its target position (`position_spring`) and target velocity
/// (`position_damper`), with at most `maximum_force`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JointDrive {
    pub position_spring: f32,
    pub position_damper: f32,
    #[serde(with = "infinite")]
    pub maximum_force: f32,
}

impl Default for JointDrive {
    fn default() -> Self {
        Self {
            position_spring: 0.0,
            position_damper: 0.0,
            maximum_force: f32::INFINITY,
        }
    }
}

impl JointDrive {
    /// Does this drive do anything?
    pub fn active(&self) -> bool {
        self.position_spring > 0.0 || self.position_damper > 0.0
    }
}

/// Configurable Joint ▸ Rotation Drive Mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationDriveMode {
    /// Angular X Drive about the primary axis, Angular YZ Drive about the other two.
    #[default]
    XAndYz,
    /// Slerp Drive on all three.
    Slerp,
}

/// Joint ▸ Projection Mode: pull the body back when the joint drifts too far (stops stretched
/// chains from exploding).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JointProjectionMode {
    #[default]
    None,
    PositionAndRotation,
}

/// Every linear and angular axis Locked, Limited or Free, with limits, limit springs, drives
/// and projection. The joint frame: X = `axis`, Y = `secondary_axis` (made perpendicular),
/// Z = X × Y; the start pose is the zero position of every axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigurableJoint {
    #[serde(with = "entity_bits::option")]
    pub connected_body: Option<hecs::Entity>,
    pub anchor: Vec3,
    pub axis: Vec3,
    pub secondary_axis: Vec3,
    pub auto_configure_connected_anchor: bool,
    pub connected_anchor: Vec3,
    pub x_motion: JointMotion,
    pub y_motion: JointMotion,
    pub z_motion: JointMotion,
    pub angular_x_motion: JointMotion,
    pub angular_y_motion: JointMotion,
    pub angular_z_motion: JointMotion,
    pub linear_limit_spring: SoftJointLimitSpring,
    /// Distance each Limited linear axis may move either way from the start.
    pub linear_limit: SoftJointLimit,
    pub angular_x_limit_spring: SoftJointLimitSpring,
    /// Degrees (negative).
    pub low_angular_x_limit: SoftJointLimit,
    pub high_angular_x_limit: SoftJointLimit,
    pub angular_yz_limit_spring: SoftJointLimitSpring,
    /// Degrees either way.
    pub angular_y_limit: SoftJointLimit,
    pub angular_z_limit: SoftJointLimit,
    /// Drive targets in the joint frame (metres, metres/second).
    pub target_position: Vec3,
    pub target_velocity: Vec3,
    pub x_drive: JointDrive,
    pub y_drive: JointDrive,
    pub z_drive: JointDrive,
    /// Euler degrees about the joint axes.
    pub target_rotation: Vec3,
    /// Degrees/second about the joint axes.
    pub target_angular_velocity: Vec3,
    pub rotation_drive_mode: RotationDriveMode,
    pub angular_x_drive: JointDrive,
    pub angular_yz_drive: JointDrive,
    pub slerp_drive: JointDrive,
    pub projection_mode: JointProjectionMode,
    pub projection_distance: f32,
    /// Degrees.
    pub projection_angle: f32,
    pub enable_collision: bool,
    #[serde(with = "infinite")]
    pub break_force: f32,
    #[serde(with = "infinite")]
    pub break_torque: f32,
}

impl Default for ConfigurableJoint {
    fn default() -> Self {
        Self {
            connected_body: None,
            anchor: Vec3::ZERO,
            axis: Vec3::X,
            secondary_axis: Vec3::Y,
            auto_configure_connected_anchor: true,
            connected_anchor: Vec3::ZERO,
            x_motion: JointMotion::Free,
            y_motion: JointMotion::Free,
            z_motion: JointMotion::Free,
            angular_x_motion: JointMotion::Free,
            angular_y_motion: JointMotion::Free,
            angular_z_motion: JointMotion::Free,
            linear_limit_spring: SoftJointLimitSpring::default(),
            linear_limit: SoftJointLimit::default(),
            angular_x_limit_spring: SoftJointLimitSpring::default(),
            low_angular_x_limit: SoftJointLimit::default(),
            high_angular_x_limit: SoftJointLimit::default(),
            angular_yz_limit_spring: SoftJointLimitSpring::default(),
            angular_y_limit: SoftJointLimit::default(),
            angular_z_limit: SoftJointLimit::default(),
            target_position: Vec3::ZERO,
            target_velocity: Vec3::ZERO,
            x_drive: JointDrive::default(),
            y_drive: JointDrive::default(),
            z_drive: JointDrive::default(),
            target_rotation: Vec3::ZERO,
            target_angular_velocity: Vec3::ZERO,
            rotation_drive_mode: RotationDriveMode::XAndYz,
            angular_x_drive: JointDrive::default(),
            angular_yz_drive: JointDrive::default(),
            slerp_drive: JointDrive::default(),
            projection_mode: JointProjectionMode::None,
            projection_distance: 0.1,
            projection_angle: 180.0,
            enable_collision: false,
            break_force: f32::INFINITY,
            break_torque: f32::INFINITY,
        }
    }
}

impl ConfigurableJoint {
    /// Motions of the X, Y, Z linear axes, then the angular ones.
    pub fn motions(&self) -> [JointMotion; 6] {
        [
            self.x_motion,
            self.y_motion,
            self.z_motion,
            self.angular_x_motion,
            self.angular_y_motion,
            self.angular_z_motion,
        ]
    }
}

/// A ball-and-socket joint for limbs: twist about `axis` between the low and high twist
/// limits, swing about `swing_axis` (Swing 1) and about the third axis (Swing 2) up to their
/// limits, in degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterJoint {
    #[serde(with = "entity_bits::option")]
    pub connected_body: Option<hecs::Entity>,
    pub anchor: Vec3,
    pub axis: Vec3,
    pub swing_axis: Vec3,
    pub auto_configure_connected_anchor: bool,
    pub connected_anchor: Vec3,
    pub twist_limit_spring: SoftJointLimitSpring,
    pub low_twist_limit: SoftJointLimit,
    pub high_twist_limit: SoftJointLimit,
    pub swing_limit_spring: SoftJointLimitSpring,
    pub swing1_limit: SoftJointLimit,
    pub swing2_limit: SoftJointLimit,
    pub enable_projection: bool,
    pub projection_distance: f32,
    pub projection_angle: f32,
    pub enable_collision: bool,
    #[serde(with = "infinite")]
    pub break_force: f32,
    #[serde(with = "infinite")]
    pub break_torque: f32,
}

impl Default for CharacterJoint {
    fn default() -> Self {
        Self {
            connected_body: None,
            anchor: Vec3::ZERO,
            axis: Vec3::X,
            swing_axis: Vec3::Y,
            auto_configure_connected_anchor: true,
            connected_anchor: Vec3::ZERO,
            twist_limit_spring: SoftJointLimitSpring::default(),
            low_twist_limit: SoftJointLimit::new(-20.0),
            high_twist_limit: SoftJointLimit::new(70.0),
            swing_limit_spring: SoftJointLimitSpring::default(),
            swing1_limit: SoftJointLimit::new(40.0),
            swing2_limit: SoftJointLimit::new(40.0),
            enable_projection: false,
            projection_distance: 0.1,
            projection_angle: 180.0,
            enable_collision: false,
            break_force: f32::INFINITY,
            break_torque: f32::INFINITY,
        }
    }
}

/// Marks the root of a ragdoll made by the Ragdoll Wizard: its bones (objects with a
/// Rigidbody, a collider and a Character Joint) follow the animation while the ragdoll is off
/// (kinematic) and fall under physics while it is on. Scripts switch it with
/// `Ragdoll.Enable(go)` / `Ragdoll.Disable(go [, blendTime])`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ragdoll {
    /// Start under physics instead of animated.
    pub enabled_on_start: bool,
    /// Seconds the bones take to blend from the physics pose back to the animation.
    pub blend_time: f32,
    #[serde(with = "super::skinned_mesh::bone_list")]
    pub bones: Vec<Option<hecs::Entity>>,
}

impl Default for Ragdoll {
    fn default() -> Self {
        Self {
            enabled_on_start: false,
            blend_time: 0.4,
            bones: Vec::new(),
        }
    }
}
