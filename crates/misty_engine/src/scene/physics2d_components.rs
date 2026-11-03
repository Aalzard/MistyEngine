//! 2D physics components (simulated by `crate::physics2d` in the XY plane, separate from 3D).
//!
//! Sizes and offsets are in the object's local space and scale with its world X/Y scale.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Rigidbody 2D ▸ Body Type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyType2D {
    #[default]
    Dynamic,
    /// Moved by its Transform / MovePosition, pushes dynamic bodies.
    Kinematic,
    /// Never moves.
    Static,
}

impl BodyType2D {
    pub const ALL: [Self; 3] = [Self::Dynamic, Self::Kinematic, Self::Static];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dynamic => "Dynamic",
            Self::Kinematic => "Kinematic",
            Self::Static => "Static",
        }
    }
}

/// Rigidbody 2D ▸ Collision Detection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollisionDetection2D {
    #[default]
    Discrete,
    /// Swept collision for fast bodies.
    Continuous,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rigidbody2D {
    pub body_type: BodyType2D,
    pub mass: f32,
    pub linear_drag: f32,
    pub angular_drag: f32,
    /// Multiplies Physics 2D gravity.
    pub gravity_scale: f32,
    pub collision_detection: CollisionDetection2D,
    /// Constraints ▸ Freeze Position X / Y.
    pub freeze_position: [bool; 2],
    /// Constraints ▸ Freeze Rotation Z.
    pub freeze_rotation: bool,
}

impl Default for Rigidbody2D {
    fn default() -> Self {
        Self {
            body_type: BodyType2D::Dynamic,
            mass: 1.0,
            linear_drag: 0.0,
            angular_drag: 0.05,
            gravity_scale: 1.0,
            collision_detection: CollisionDetection2D::Discrete,
            freeze_position: [false; 2],
            freeze_rotation: false,
        }
    }
}

/// Physics Material 2D values of a collider.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicsMaterial2D {
    pub friction: f32,
    pub bounciness: f32,
}

impl Default for PhysicsMaterial2D {
    fn default() -> Self {
        Self {
            friction: 0.4,
            bounciness: 0.0,
        }
    }
}

/// Settings every 2D collider has.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collider2DCommon {
    pub enabled: bool,
    pub is_trigger: bool,
    pub material: PhysicsMaterial2D,
    pub offset: Vec2,
}

impl Default for Collider2DCommon {
    fn default() -> Self {
        Self {
            enabled: true,
            is_trigger: false,
            material: PhysicsMaterial2D::default(),
            offset: Vec2::ZERO,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxCollider2D {
    #[serde(flatten)]
    pub common: Collider2DCommon,
    pub size: Vec2,
    /// Rounds the corners (the box grows by this radius).
    pub edge_radius: f32,
}

impl Default for BoxCollider2D {
    fn default() -> Self {
        Self {
            common: Collider2DCommon::default(),
            size: Vec2::ONE,
            edge_radius: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CircleCollider2D {
    #[serde(flatten)]
    pub common: Collider2DCommon,
    pub radius: f32,
}

impl Default for CircleCollider2D {
    fn default() -> Self {
        Self {
            common: Collider2DCommon::default(),
            radius: 0.5,
        }
    }
}

/// Capsule Collider 2D ▸ Direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapsuleDirection2D {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapsuleCollider2D {
    #[serde(flatten)]
    pub common: Collider2DCommon,
    /// Full width and height; the round ends sit on the shorter side.
    pub size: Vec2,
    pub direction: CapsuleDirection2D,
}

impl Default for CapsuleCollider2D {
    fn default() -> Self {
        Self {
            common: Collider2DCommon::default(),
            size: Vec2::new(0.5, 1.0),
            direction: CapsuleDirection2D::Vertical,
        }
    }
}

/// Convex or concave outline; no points = the object's sprite rect (or a unit square).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolygonCollider2D {
    #[serde(flatten)]
    pub common: Collider2DCommon,
    pub points: Vec<Vec2>,
}

/// Open chain of line segments (ground lines, one-sided walls).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeCollider2D {
    #[serde(flatten)]
    pub common: Collider2DCommon,
    pub points: Vec<Vec2>,
    pub edge_radius: f32,
}

impl Default for EdgeCollider2D {
    fn default() -> Self {
        Self {
            common: Collider2DCommon::default(),
            points: vec![Vec2::new(-0.5, 0.0), Vec2::new(0.5, 0.0)],
            edge_radius: 0.0,
        }
    }
}
