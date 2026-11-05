//! Spline components: Spline Container (the splines), Spline Extrude (a tube or road mesh),
//! Spline Instantiate (copies along a spline), Spline Animate (moves its object along one) and
//! the Spline Instance marker on generated copies.

use glam::{Mat4, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::{Knot, Spline, SplinePath};
use crate::scene::components::entity_bits;
use crate::scene::{PrimitiveMesh, Scene, SceneComponent};

/// Object-reference fields of the spline components (remapped when saving and copying).
pub const SPLINE_REFS: &[(&str, &str, &str)] = &[
    ("Spline Extrude", "", "container"),
    ("Spline Instantiate", "", "container"),
    ("Spline Animate", "", "container"),
];

/// One or more splines in the object's local space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplineContainer {
    pub splines: Vec<Spline>,
}

impl Default for SplineContainer {
    fn default() -> Self {
        Self {
            splines: vec![Spline::default()],
        }
    }
}

impl SplineContainer {
    pub fn with(spline: Spline) -> Self {
        Self {
            splines: vec![spline],
        }
    }

    /// Path of spline `i` (empty if there is none).
    pub fn path(&self, i: usize) -> SplinePath {
        self.splines.get(i).map(Spline::path).unwrap_or_default()
    }

    /// Mutable knot `k` of spline `s`.
    pub fn knot_mut(&mut self, s: usize, k: usize) -> Option<&mut Knot> {
        self.splines.get_mut(s)?.knots.get_mut(k)
    }
}

impl SceneComponent for SplineContainer {
    const NAME: &'static str = "Spline Container";
    const CATEGORY: &'static str = "Spline";
}

/// The object holding the Spline Container a component reads: its own reference, or the
/// object itself when unset.
pub fn container_object(scene: &Scene, owner: Entity, container: Option<Entity>) -> Option<Entity> {
    let e = container.filter(|c| scene.contains(*c)).unwrap_or(owner);
    scene.world.get::<&SplineContainer>(e).is_ok().then_some(e)
}

/// World-space path of spline `index` of the container on `e`, with its world matrix.
pub fn world_path(scene: &Scene, e: Entity, index: usize) -> Option<(SplinePath, Mat4)> {
    let c = scene.world.get::<&SplineContainer>(e).ok()?;
    let spline = c.splines.get(index)?;
    Some((spline.path(), scene.world_matrix(e)))
}

/// Cross-section of a Spline Extrude mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtrudeProfile {
    /// A round tube (`radius`, `sides`).
    #[default]
    Circle,
    /// A flat strip facing the spline's up vector (`width`).
    Road,
}

/// Builds a mesh along every spline of a container into this object's Mesh Filter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplineExtrude {
    pub enabled: bool,
    /// Object with the Spline Container (none = this object).
    #[serde(with = "entity_bits::option")]
    pub container: Option<Entity>,
    pub profile: ExtrudeProfile,
    pub radius: f32,
    pub sides: u32,
    /// Road width.
    pub width: f32,
    /// Rings per unit of length.
    pub segments_per_unit: f32,
    /// Close the tube's ends (open splines, Circle profile).
    pub cap_ends: bool,
    /// Part of the spline to cover (start, end), 0–1.
    pub range: [f32; 2],
    /// V texture repeats per unit of length.
    pub uv_tiling: f32,
    /// Key of the mesh last built (not saved).
    #[serde(skip)]
    pub built: Option<String>,
}

impl Default for SplineExtrude {
    fn default() -> Self {
        Self {
            enabled: true,
            container: None,
            profile: ExtrudeProfile::Circle,
            radius: 0.25,
            sides: 8,
            width: 4.0,
            segments_per_unit: 4.0,
            cap_ends: true,
            range: [0.0, 1.0],
            uv_tiling: 1.0,
            built: None,
        }
    }
}

impl SceneComponent for SplineExtrude {
    const NAME: &'static str = "Spline Extrude";
    const CATEGORY: &'static str = "Spline";
}

/// What Spline Instantiate places.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstantiateItem {
    /// Prefab asset ("Assets/…/X.prefab"); when unset a primitive is used.
    pub prefab: Option<String>,
    pub primitive: PrimitiveMesh,
    /// Material of the primitive.
    pub material: Option<String>,
    /// Relative chance of picking this item.
    pub probability: f32,
}

impl Default for InstantiateItem {
    fn default() -> Self {
        Self {
            prefab: None,
            primitive: PrimitiveMesh::Cube,
            material: None,
            probability: 1.0,
        }
    }
}

/// How copies are spread along the spline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstantiateMethod {
    /// `count` copies spread evenly.
    #[default]
    InstanceCount,
    /// A copy every `spacing` units (random between the min and max).
    SplineDistance,
}

/// What copies are oriented by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlignMode {
    /// The spline's tangent and up vector at each copy.
    #[default]
    SplineElement,
    /// The container object's rotation.
    SplineObject,
    /// World axes.
    World,
}

impl AlignMode {
    pub const ALL: [AlignMode; 3] = [Self::SplineElement, Self::SplineObject, Self::World];
    pub fn label(self) -> &'static str {
        match self {
            Self::SplineElement => "Spline Element",
            Self::SplineObject => "Spline Object",
            Self::World => "World Space",
        }
    }
}

/// Places copies of prefabs or primitives along a spline, as children of this object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplineInstantiate {
    pub enabled: bool,
    #[serde(with = "entity_bits::option")]
    pub container: Option<Entity>,
    /// Spline of the container to follow.
    pub spline: usize,
    pub items: Vec<InstantiateItem>,
    pub method: InstantiateMethod,
    pub count: u32,
    /// Spacing range (min, max) for Spline Distance.
    pub spacing: [f32; 2],
    pub align: AlignMode,
    /// Offset in the copy's frame (x across, y up, z along), random between min and max.
    pub position_min: Vec3,
    pub position_max: Vec3,
    /// Extra rotation (Euler degrees), random between min and max.
    pub rotation_min: Vec3,
    pub rotation_max: Vec3,
    /// Scale, random between min and max.
    pub scale_min: Vec3,
    pub scale_max: Vec3,
    pub seed: u32,
    /// Hash of the inputs the current copies were built from (not saved).
    #[serde(skip)]
    pub built: u64,
}

impl Default for SplineInstantiate {
    fn default() -> Self {
        Self {
            enabled: true,
            container: None,
            spline: 0,
            items: vec![InstantiateItem::default()],
            method: InstantiateMethod::InstanceCount,
            count: 5,
            spacing: [2.0, 2.0],
            align: AlignMode::SplineElement,
            position_min: Vec3::ZERO,
            position_max: Vec3::ZERO,
            rotation_min: Vec3::ZERO,
            rotation_max: Vec3::ZERO,
            scale_min: Vec3::ONE,
            scale_max: Vec3::ONE,
            seed: 0,
            built: 0,
        }
    }
}

impl SceneComponent for SplineInstantiate {
    const NAME: &'static str = "Spline Instantiate";
    const CATEGORY: &'static str = "Spline";
}

/// Marks a copy made by Spline Instantiate on its parent (replaced whenever it rebuilds).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplineInstance {}

impl SceneComponent for SplineInstance {
    const NAME: &'static str = "Spline Instance";
    const CATEGORY: &'static str = "";
    const REMOVABLE: bool = false;
}

/// Spline Animate: whether `duration` or `speed` sets the pace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimateMethod {
    #[default]
    Time,
    Speed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopMode {
    Once,
    #[default]
    Loop,
    PingPong,
}

impl LoopMode {
    pub const ALL: [LoopMode; 3] = [Self::Once, Self::Loop, Self::PingPong];
    pub fn label(self) -> &'static str {
        match self {
            Self::Once => "Once",
            Self::Loop => "Loop Continuous",
            Self::PingPong => "Ping Pong",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    #[default]
    None,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    pub const ALL: [Easing; 4] = [Self::None, Self::EaseIn, Self::EaseOut, Self::EaseInOut];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::EaseInOut => "Ease In Out",
        }
    }
    pub fn apply(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::None => x,
            Self::EaseIn => x * x,
            Self::EaseOut => 1.0 - (1.0 - x) * (1.0 - x),
            Self::EaseInOut => x * x * (3.0 - 2.0 * x),
        }
    }
}

/// Spline Animate: how the moving object is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimateAlignment {
    /// Keep its rotation.
    None,
    /// Face along the tangent with the spline's up vector.
    #[default]
    SplineElement,
    /// Copy the container's rotation.
    SplineObject,
    /// Face along the tangent with world up.
    World,
}

impl AnimateAlignment {
    pub const ALL: [AnimateAlignment; 4] = [
        Self::None,
        Self::SplineElement,
        Self::SplineObject,
        Self::World,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::SplineElement => "Spline Element",
            Self::SplineObject => "Spline Object",
            Self::World => "World Space",
        }
    }
}

/// Moves this object along a spline while playing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplineAnimate {
    pub enabled: bool,
    #[serde(with = "entity_bits::option")]
    pub container: Option<Entity>,
    pub spline: usize,
    pub play_on_awake: bool,
    pub method: AnimateMethod,
    /// Seconds for one pass (Time method).
    pub duration: f32,
    /// Units per second (Speed method).
    pub speed: f32,
    pub easing: Easing,
    pub loop_mode: LoopMode,
    pub alignment: AnimateAlignment,
    /// Where on the spline it starts (0–1).
    pub start_offset: f32,
    /// Seconds played so far (not saved).
    #[serde(skip)]
    pub elapsed: f32,
    #[serde(skip)]
    pub playing: bool,
    /// Play On Awake has been handled.
    #[serde(skip)]
    pub started: bool,
}

impl Default for SplineAnimate {
    fn default() -> Self {
        Self {
            enabled: true,
            container: None,
            spline: 0,
            play_on_awake: true,
            method: AnimateMethod::Time,
            duration: 5.0,
            speed: 2.0,
            easing: Easing::None,
            loop_mode: LoopMode::Loop,
            alignment: AnimateAlignment::SplineElement,
            start_offset: 0.0,
            elapsed: 0.0,
            playing: false,
            started: false,
        }
    }
}

impl SceneComponent for SplineAnimate {
    const NAME: &'static str = "Spline Animate";
    const CATEGORY: &'static str = "Spline";
}
