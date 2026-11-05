//! Animation rigging components: Rig Builder, Rig and the constraints placed on a rig's child
//! objects. Every bone, target and hint is an ordinary Transform in the scene.

use glam::Vec3;
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::scene::SceneComponent;
use crate::scene::components::entity_bits;

/// One rig evaluated by a Rig Builder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigLayer {
    #[serde(with = "entity_bits::option")]
    pub rig: Option<Entity>,
    pub active: bool,
}

impl Default for RigLayer {
    fn default() -> Self {
        Self {
            rig: None,
            active: true,
        }
    }
}

/// Evaluates its rig layers every frame after animation, in list order. With no layers it
/// evaluates every Rig below it in the hierarchy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigBuilder {
    pub enabled: bool,
    pub layers: Vec<RigLayer>,
}

impl Default for RigBuilder {
    fn default() -> Self {
        Self {
            enabled: true,
            layers: Vec::new(),
        }
    }
}

/// A group of constraints (the Rig object's children) blended in by `weight`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rig {
    pub weight: f32,
}

impl Default for Rig {
    fn default() -> Self {
        Self { weight: 1.0 }
    }
}

/// Reaches `target` with a three-bone chain (`root` ▸ `mid` ▸ `tip`); `hint` picks the side
/// the middle joint bends towards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TwoBoneIk {
    pub weight: f32,
    #[serde(with = "entity_bits::option")]
    pub root: Option<Entity>,
    #[serde(with = "entity_bits::option")]
    pub mid: Option<Entity>,
    #[serde(with = "entity_bits::option")]
    pub tip: Option<Entity>,
    #[serde(with = "entity_bits::option")]
    pub target: Option<Entity>,
    #[serde(with = "entity_bits::option")]
    pub hint: Option<Entity>,
    pub target_position_weight: f32,
    pub target_rotation_weight: f32,
    pub hint_weight: f32,
    pub maintain_target_position_offset: bool,
    pub maintain_target_rotation_offset: bool,
}

impl Default for TwoBoneIk {
    fn default() -> Self {
        Self {
            weight: 1.0,
            root: None,
            mid: None,
            tip: None,
            target: None,
            hint: None,
            target_position_weight: 1.0,
            target_rotation_weight: 1.0,
            hint_weight: 1.0,
            maintain_target_position_offset: false,
            maintain_target_rotation_offset: false,
        }
    }
}

/// A signed local axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    #[serde(rename = "-X")]
    NegX,
    Y,
    #[serde(rename = "-Y")]
    NegY,
    #[default]
    Z,
    #[serde(rename = "-Z")]
    NegZ,
}

impl Axis {
    pub const ALL: [Axis; 6] = [
        Axis::X,
        Axis::NegX,
        Axis::Y,
        Axis::NegY,
        Axis::Z,
        Axis::NegZ,
    ];

    pub fn vector(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::NegX => Vec3::NEG_X,
            Axis::Y => Vec3::Y,
            Axis::NegY => Vec3::NEG_Y,
            Axis::Z => Vec3::Z,
            Axis::NegZ => Vec3::NEG_Z,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::NegX => "-X",
            Axis::Y => "Y",
            Axis::NegY => "-Y",
            Axis::Z => "Z",
            Axis::NegZ => "-Z",
        }
    }
}

/// Where Multi-Aim's up direction comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldUpType {
    /// Smallest rotation that aims; the up axis is free.
    None,
    /// World +Y.
    #[default]
    SceneUp,
    /// Towards the World Up Object.
    ObjectUp,
    /// The World Up Object's rotation applied to World Up Vector.
    ObjectRotationUp,
    /// World Up Vector in world space.
    Vector,
}

impl WorldUpType {
    pub const ALL: [WorldUpType; 5] = [
        WorldUpType::None,
        WorldUpType::SceneUp,
        WorldUpType::ObjectUp,
        WorldUpType::ObjectRotationUp,
        WorldUpType::Vector,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WorldUpType::None => "None",
            WorldUpType::SceneUp => "Scene Up",
            WorldUpType::ObjectUp => "Object Up",
            WorldUpType::ObjectRotationUp => "Object Rotation Up",
            WorldUpType::Vector => "Vector",
        }
    }
}

/// A source object and its share of the result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeightedSource {
    #[serde(with = "entity_bits::option")]
    pub object: Option<Entity>,
    pub weight: f32,
}

impl Default for WeightedSource {
    fn default() -> Self {
        Self {
            object: None,
            weight: 1.0,
        }
    }
}

/// Turns `constrained_object` so its aim axis points at the weighted average of its sources.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiAim {
    pub weight: f32,
    #[serde(with = "entity_bits::option")]
    pub constrained_object: Option<Entity>,
    pub aim_axis: Axis,
    pub up_axis: Axis,
    pub world_up_type: WorldUpType,
    pub world_up_vector: Vec3,
    #[serde(with = "entity_bits::option")]
    pub world_up_object: Option<Entity>,
    pub sources: Vec<WeightedSource>,
    pub maintain_offset: bool,
    /// Extra local rotation (Euler degrees) after aiming.
    pub offset: Vec3,
    /// Which local axes (X, Y, Z) may rotate.
    pub constrained_axes: [bool; 3],
    /// Clamp of the rotation away from the input pose, in degrees, on every axis.
    pub min_limit: f32,
    pub max_limit: f32,
}

impl Default for MultiAim {
    fn default() -> Self {
        Self {
            weight: 1.0,
            constrained_object: None,
            aim_axis: Axis::Z,
            up_axis: Axis::Y,
            world_up_type: WorldUpType::SceneUp,
            world_up_vector: Vec3::Y,
            world_up_object: None,
            sources: Vec::new(),
            maintain_offset: false,
            offset: Vec3::ZERO,
            constrained_axes: [true; 3],
            min_limit: -180.0,
            max_limit: 180.0,
        }
    }
}

/// Follows `source` (keeping their starting offset) with a lag: 0 = rigid, 1 = never moves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DampedTransform {
    pub weight: f32,
    #[serde(with = "entity_bits::option")]
    pub constrained_object: Option<Entity>,
    #[serde(with = "entity_bits::option")]
    pub source: Option<Entity>,
    pub damp_position: f32,
    pub damp_rotation: f32,
    /// Turn the object so it keeps pointing away from the source as it lags.
    pub maintain_aim: bool,
}

impl Default for DampedTransform {
    fn default() -> Self {
        Self {
            weight: 1.0,
            constrained_object: None,
            source: None,
            damp_position: 0.5,
            damp_rotation: 0.5,
            maintain_aim: true,
        }
    }
}

/// Chain IK solver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainSolver {
    /// Forward and backward reaching: smooth, spreads the bend along the chain.
    #[default]
    Fabrik,
    /// Cyclic coordinate descent: bends the joints nearest the tip most.
    Ccd,
}

impl ChainSolver {
    pub const ALL: [ChainSolver; 2] = [ChainSolver::Fabrik, ChainSolver::Ccd];

    pub fn label(self) -> &'static str {
        match self {
            ChainSolver::Fabrik => "FABRIK",
            ChainSolver::Ccd => "CCD",
        }
    }
}

/// Bends every bone from `root` down to `tip` so the tip reaches `target`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChainIk {
    pub weight: f32,
    #[serde(with = "entity_bits::option")]
    pub root: Option<Entity>,
    #[serde(with = "entity_bits::option")]
    pub tip: Option<Entity>,
    #[serde(with = "entity_bits::option")]
    pub target: Option<Entity>,
    pub solver: ChainSolver,
    pub chain_rotation_weight: f32,
    pub tip_rotation_weight: f32,
    pub max_iterations: u32,
    /// Stop once the tip is this close to the target (metres).
    pub tolerance: f32,
    pub maintain_target_position_offset: bool,
    pub maintain_target_rotation_offset: bool,
}

impl Default for ChainIk {
    fn default() -> Self {
        Self {
            weight: 1.0,
            root: None,
            tip: None,
            target: None,
            solver: ChainSolver::Fabrik,
            chain_rotation_weight: 1.0,
            tip_rotation_weight: 1.0,
            max_iterations: 15,
            tolerance: 0.0001,
            maintain_target_position_offset: false,
            maintain_target_rotation_offset: false,
        }
    }
}

/// Space Override Transform's position and rotation are given in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverrideSpace {
    #[default]
    Pivot,
    Local,
    World,
}

impl OverrideSpace {
    pub const ALL: [OverrideSpace; 3] = [
        OverrideSpace::Pivot,
        OverrideSpace::Local,
        OverrideSpace::World,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OverrideSpace::Pivot => "Pivot",
            OverrideSpace::Local => "Local",
            OverrideSpace::World => "World",
        }
    }
}

/// Replaces the object's pose with the source's (or the given position / rotation).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverrideTransform {
    pub weight: f32,
    #[serde(with = "entity_bits::option")]
    pub constrained_object: Option<Entity>,
    /// When set, its world pose is used instead of `position` / `rotation`.
    #[serde(with = "entity_bits::option")]
    pub source: Option<Entity>,
    pub space: OverrideSpace,
    pub position: Vec3,
    /// Euler degrees.
    pub rotation: Vec3,
    pub position_weight: f32,
    pub rotation_weight: f32,
}

impl Default for OverrideTransform {
    fn default() -> Self {
        Self {
            weight: 1.0,
            constrained_object: None,
            source: None,
            space: OverrideSpace::Pivot,
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            position_weight: 1.0,
            rotation_weight: 1.0,
        }
    }
}

const CATEGORY: &str = "Animation Rigging";

impl SceneComponent for RigBuilder {
    const NAME: &'static str = "Rig Builder";
    const CATEGORY: &'static str = CATEGORY;
}
impl SceneComponent for Rig {
    const NAME: &'static str = "Rig";
    const CATEGORY: &'static str = CATEGORY;
}
impl SceneComponent for TwoBoneIk {
    const NAME: &'static str = "Two Bone IK Constraint";
    const CATEGORY: &'static str = CATEGORY;
}
impl SceneComponent for MultiAim {
    const NAME: &'static str = "Multi-Aim Constraint";
    const CATEGORY: &'static str = CATEGORY;
}
impl SceneComponent for DampedTransform {
    const NAME: &'static str = "Damped Transform";
    const CATEGORY: &'static str = CATEGORY;
}
impl SceneComponent for ChainIk {
    const NAME: &'static str = "Chain IK Constraint";
    const CATEGORY: &'static str = CATEGORY;
}
impl SceneComponent for OverrideTransform {
    const NAME: &'static str = "Override Transform";
    const CATEGORY: &'static str = CATEGORY;
}

/// Names of the constraint components (not Rig / Rig Builder).
pub const CONSTRAINT_NAMES: [&str; 5] = [
    TwoBoneIk::NAME,
    MultiAim::NAME,
    DampedTransform::NAME,
    ChainIk::NAME,
    OverrideTransform::NAME,
];

/// Object-reference fields for scene files and copies (see `scene::file`).
pub const RIG_REFS: &[(&str, &str, &str)] = &[
    ("Rig Builder", "layers", "rig"),
    ("Two Bone IK Constraint", "", "root"),
    ("Two Bone IK Constraint", "", "mid"),
    ("Two Bone IK Constraint", "", "tip"),
    ("Two Bone IK Constraint", "", "target"),
    ("Two Bone IK Constraint", "", "hint"),
    ("Multi-Aim Constraint", "", "constrained_object"),
    ("Multi-Aim Constraint", "", "world_up_object"),
    ("Multi-Aim Constraint", "sources", "object"),
    ("Damped Transform", "", "constrained_object"),
    ("Damped Transform", "", "source"),
    ("Chain IK Constraint", "", "root"),
    ("Chain IK Constraint", "", "tip"),
    ("Chain IK Constraint", "", "target"),
    ("Override Transform", "", "constrained_object"),
    ("Override Transform", "", "source"),
];
