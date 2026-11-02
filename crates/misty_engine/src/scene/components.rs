//! Built-in components: GameObject header, Transform, hierarchy node, Camera, Light, mesh components.

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::camera::{euler_to_quat, quat_to_euler};

/// Name, active flag, tag, layer and component order: what the Inspector header shows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameObject {
    pub name: String,
    pub active: bool,
    pub is_static: bool,
    pub tag: String,
    pub layer: u8,
    /// Component names in Inspector order (Transform first).
    pub component_order: Vec<String>,
}

impl Default for GameObject {
    fn default() -> Self {
        Self {
            name: "GameObject".into(),
            active: true,
            is_static: false,
            tag: "Untagged".into(),
            layer: 0,
            component_order: Vec::new(),
        }
    }
}

/// Local position/rotation/scale relative to the parent.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// Euler angles last typed by the user, kept so the Inspector shows e.g. 360 instead of 0.
    pub euler_hint: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            euler_hint: Vec3::ZERO,
        }
    }
}

impl Transform {
    pub fn from_position(position: Vec3) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    pub fn with_euler(mut self, degrees: Vec3) -> Self {
        self.set_euler(degrees);
        self
    }

    pub fn local_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    /// Euler angles for display: the hint if it still matches the rotation.
    pub fn euler(&self) -> Vec3 {
        if euler_to_quat(self.euler_hint).dot(self.rotation).abs() > 0.99999 {
            self.euler_hint
        } else {
            quat_to_euler(self.rotation)
        }
    }

    pub fn set_euler(&mut self, degrees: Vec3) {
        self.euler_hint = degrees;
        self.rotation = euler_to_quat(degrees);
    }

    pub fn set_rotation(&mut self, rotation: Quat) {
        self.rotation = rotation.normalize();
        self.euler_hint = quat_to_euler(self.rotation);
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }
}

/// Parent/children links. Every scene object has one; it is not shown in the Inspector.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Node {
    #[serde(with = "entity_bits::option")]
    pub parent: Option<hecs::Entity>,
    #[serde(with = "entity_bits::list")]
    pub children: Vec<hecs::Entity>,
}

/// Entities are stored by their id bits (only meaningful inside one world; scene files remap them).
mod entity_bits {
    use hecs::Entity;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    fn to_bits(e: &Entity) -> u64 {
        e.to_bits().get()
    }

    fn from_bits<'de, D: Deserializer<'de>>(bits: u64) -> Result<Entity, D::Error> {
        Entity::from_bits(bits).ok_or_else(|| serde::de::Error::custom("invalid entity id"))
    }

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(v: &Option<Entity>, s: S) -> Result<S::Ok, S::Error> {
            v.as_ref().map(to_bits).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Entity>, D::Error> {
            Option::<u64>::deserialize(d)?
                .map(from_bits::<D>)
                .transpose()
        }
    }

    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer>(v: &[Entity], s: S) -> Result<S::Ok, S::Error> {
            v.iter().map(to_bits).collect::<Vec<_>>().serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Entity>, D::Error> {
            Vec::<u64>::deserialize(d)?
                .into_iter()
                .map(from_bits::<D>)
                .collect()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ClearFlags {
    #[default]
    Skybox,
    SolidColor,
    DepthOnly,
    Nothing,
}

impl ClearFlags {
    pub const ALL: [ClearFlags; 4] = [
        Self::Skybox,
        Self::SolidColor,
        Self::DepthOnly,
        Self::Nothing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Skybox => "Skybox",
            Self::SolidColor => "Solid Color",
            Self::DepthOnly => "Depth only",
            Self::Nothing => "Don't Clear",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Camera {
    pub enabled: bool,
    pub clear_flags: ClearFlags,
    /// sRGB background colour.
    pub background: [f32; 4],
    pub orthographic: bool,
    /// Vertical field of view in degrees.
    pub fov: f32,
    pub ortho_size: f32,
    pub near: f32,
    pub far: f32,
    /// Normalised viewport rect: x, y, w, h.
    pub viewport: [f32; 4],
    pub depth: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            enabled: true,
            clear_flags: ClearFlags::Skybox,
            background: [49.0 / 255.0, 77.0 / 255.0, 121.0 / 255.0, 0.0],
            orthographic: false,
            fov: 60.0,
            ortho_size: 5.0,
            near: 0.3,
            far: 1000.0,
            viewport: [0.0, 0.0, 1.0, 1.0],
            depth: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LightType {
    Directional,
    #[default]
    Point,
    Spot,
}

impl LightType {
    pub const ALL: [LightType; 3] = [Self::Directional, Self::Point, Self::Spot];

    pub fn label(self) -> &'static str {
        match self {
            Self::Directional => "Directional",
            Self::Point => "Point",
            Self::Spot => "Spot",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Shadows {
    None,
    Hard,
    #[default]
    Soft,
}

impl Shadows {
    pub const ALL: [Shadows; 3] = [Self::None, Self::Hard, Self::Soft];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No Shadows",
            Self::Hard => "Hard Shadows",
            Self::Soft => "Soft Shadows",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Light {
    pub enabled: bool,
    pub kind: LightType,
    /// sRGB colour.
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    /// Full cone angle in degrees.
    pub spot_angle: f32,
    pub shadows: Shadows,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: LightType::Point,
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            range: 10.0,
            spot_angle: 30.0,
            shadows: Shadows::None,
        }
    }
}

impl Light {
    /// Defaults used by GameObject ▸ Light ▸ Directional Light.
    pub fn directional() -> Self {
        Self {
            kind: LightType::Directional,
            color: [1.0, 0.957, 0.839],
            shadows: Shadows::Soft,
            ..Default::default()
        }
    }
}

/// Built-in meshes (GameObject ▸ 3D Object).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PrimitiveMesh {
    #[default]
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Plane,
    Quad,
}

impl PrimitiveMesh {
    pub const ALL: [PrimitiveMesh; 6] = [
        Self::Cube,
        Self::Sphere,
        Self::Capsule,
        Self::Cylinder,
        Self::Plane,
        Self::Quad,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cube => "Cube",
            Self::Sphere => "Sphere",
            Self::Capsule => "Capsule",
            Self::Cylinder => "Cylinder",
            Self::Plane => "Plane",
            Self::Quad => "Quad",
        }
    }

    /// Local-space half extents of the mesh bounds.
    pub fn half_extents(self) -> Vec3 {
        match self {
            Self::Cube | Self::Sphere => Vec3::splat(0.5),
            Self::Capsule | Self::Cylinder => Vec3::new(0.5, 1.0, 0.5),
            Self::Plane => Vec3::new(5.0, 0.0, 5.0),
            Self::Quad => Vec3::new(0.5, 0.5, 0.0),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MeshFilter {
    pub mesh: Option<PrimitiveMesh>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshRenderer {
    pub enabled: bool,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// Material asset path ("Assets/…/Name.mat"); `None` = Default-Material.
    pub material: Option<String>,
}

impl Default for MeshRenderer {
    fn default() -> Self {
        Self {
            enabled: true,
            cast_shadows: true,
            receive_shadows: true,
            material: None,
        }
    }
}
