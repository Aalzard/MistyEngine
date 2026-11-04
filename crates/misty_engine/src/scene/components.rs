//! Built-in components: GameObject header, Transform, hierarchy node, Camera, Light, mesh, physics,
//! scripts, prefab link and audio components. The Particle System lives with its simulation in
//! `crate::particles` and is re-exported here.

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::camera::{euler_to_quat, quat_to_euler};

pub use crate::navigation::{NavMeshAgent, NavMeshObstacle};
pub use crate::particles::ParticleSystem;
pub use crate::terrain::{Terrain, TerrainCollider};

/// Name, active flag, tag, layer and component order: what the Inspector header shows.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
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
pub(crate) mod entity_bits {
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
#[serde(default)]
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
    /// Layers this camera renders (bit per layer; all by default, left out of files then).
    #[serde(skip_serializing_if = "is_everything")]
    pub culling_mask: u32,
}

fn is_everything(mask: &u32) -> bool {
    *mask == u32::MAX
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
            culling_mask: u32::MAX,
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
#[serde(default)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
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

/// The mesh a Mesh Filter shows: a built-in primitive or a mesh inside an imported model file.
///
/// Stored untagged, so scene files keep the old `"Cube"` form for primitives and use the asset
/// path (`"Assets/Models/Ship.glb#0"`) for imported meshes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MeshRef {
    Primitive(PrimitiveMesh),
    /// "Assets/…/Model.ext#index" (index into the model's mesh list).
    Asset(String),
}

impl From<PrimitiveMesh> for MeshRef {
    fn from(p: PrimitiveMesh) -> Self {
        Self::Primitive(p)
    }
}

impl MeshRef {
    /// Reference to mesh `index` of the model file at `path`.
    pub fn asset(path: &str, index: usize) -> Self {
        Self::Asset(format!("{path}#{index}"))
    }

    /// Model file path and mesh index of an imported mesh.
    pub fn split_asset(&self) -> Option<(&str, usize)> {
        let Self::Asset(s) = self else {
            return None;
        };
        match s.rsplit_once('#') {
            Some((path, i)) => Some((path, i.parse().ok()?)),
            None => Some((s.as_str(), 0)),
        }
    }

    pub fn primitive(&self) -> Option<PrimitiveMesh> {
        match self {
            Self::Primitive(p) => Some(*p),
            Self::Asset(_) => None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshFilter {
    pub mesh: Option<MeshRef>,
}

impl MeshFilter {
    pub fn primitive(p: PrimitiveMesh) -> Self {
        Self {
            mesh: Some(MeshRef::Primitive(p)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshRenderer {
    pub enabled: bool,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// Material asset path ("Assets/…/Name.mat"); `None` = Default-Material. Element 0.
    pub material: Option<String>,
    /// Materials list elements 1.. (each draws the mesh again on top, like extra materials on
    /// a single-submesh mesh). Left out of files when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_materials: Vec<Option<String>>,
    /// Play mode: per-renderer material copies made by scripts (`renderer.material`), by
    /// element. Never saved.
    #[serde(skip)]
    pub instances: Vec<Option<crate::assets::Material>>,
    /// Per-object material overrides by Materials element (only overridden values; left out
    /// of files when there are none). See [`crate::render::overrides`].
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub overrides: std::collections::BTreeMap<usize, crate::render::overrides::MaterialOverrides>,
}

impl Default for MeshRenderer {
    fn default() -> Self {
        Self {
            enabled: true,
            cast_shadows: true,
            receive_shadows: true,
            material: None,
            extra_materials: Vec::new(),
            instances: Vec::new(),
            overrides: Default::default(),
        }
    }
}

impl MeshRenderer {
    /// Size of the Materials list.
    pub fn material_count(&self) -> usize {
        1 + self.extra_materials.len()
    }

    /// Material path of element `i` (`None` = Default-Material or out of range).
    pub fn material_at(&self, i: usize) -> Option<&str> {
        match i {
            0 => self.material.as_deref(),
            _ => self.extra_materials.get(i - 1)?.as_deref(),
        }
    }

    /// Set element `i` (the list grows when needed).
    pub fn set_material_at(&mut self, i: usize, path: Option<String>) {
        if i == 0 {
            self.material = path;
            return;
        }
        if self.extra_materials.len() < i {
            self.extra_materials.resize(i, None);
        }
        self.extra_materials[i - 1] = path;
    }

    /// Resize the Materials list (at least one element; new ones repeat the last).
    pub fn set_material_count(&mut self, n: usize) {
        let last = self
            .material_at(self.material_count() - 1)
            .map(str::to_owned);
        self.extra_materials.resize(n.max(1) - 1, last);
    }

    /// Script-made copy of element `i`, if any.
    pub fn instance(&self, i: usize) -> Option<&crate::assets::Material> {
        self.instances.get(i)?.as_ref()
    }

    /// Element `i`'s per-object overrides, if it has any.
    pub fn overrides_at(&self, i: usize) -> Option<&crate::render::overrides::MaterialOverrides> {
        self.overrides.get(&i)
    }

    /// Change element `i`'s overrides; empty ones are dropped so files only keep real changes.
    pub fn edit_overrides<R>(
        &mut self,
        i: usize,
        f: impl FnOnce(&mut crate::render::overrides::MaterialOverrides) -> R,
    ) -> R {
        let o = self.overrides.entry(i).or_default();
        let r = f(o);
        if o.is_empty() {
            self.overrides.remove(&i);
        }
        r
    }
}

// ---- physics -------------------------------------------------------------------------------

/// Rigidbody: the object is simulated by physics while playing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Rigidbody {
    pub mass: f32,
    /// Linear damping.
    pub drag: f32,
    pub angular_drag: f32,
    pub use_gravity: bool,
    /// Moved by its Transform (animation/scripts), pushes dynamic bodies.
    pub is_kinematic: bool,
    /// Continuous collision detection for fast objects.
    pub continuous: bool,
    pub freeze_position: [bool; 3],
    pub freeze_rotation: [bool; 3],
    /// Smooth the drawn pose between fixed steps.
    pub interpolation: Interpolation,
}

/// Rigidbody ▸ Interpolate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    #[default]
    None,
    /// Blend between the last two physics poses (drawn up to one step behind).
    Interpolate,
}

impl Default for Rigidbody {
    fn default() -> Self {
        Self {
            mass: 1.0,
            drag: 0.0,
            angular_drag: 0.05,
            use_gravity: true,
            is_kinematic: false,
            continuous: false,
            freeze_position: [false; 3],
            freeze_rotation: [false; 3],
            interpolation: Interpolation::None,
        }
    }
}

/// Surface response shared by all colliders (stands in for a physics material asset).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PhysicsSurface {
    pub friction: f32,
    pub bounciness: f32,
}

impl Default for PhysicsSurface {
    fn default() -> Self {
        Self {
            friction: 0.6,
            bounciness: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxCollider {
    pub enabled: bool,
    pub is_trigger: bool,
    pub surface: PhysicsSurface,
    pub center: Vec3,
    pub size: Vec3,
}

impl Default for BoxCollider {
    fn default() -> Self {
        Self {
            enabled: true,
            is_trigger: false,
            surface: PhysicsSurface::default(),
            center: Vec3::ZERO,
            size: Vec3::ONE,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SphereCollider {
    pub enabled: bool,
    pub is_trigger: bool,
    pub surface: PhysicsSurface,
    pub center: Vec3,
    pub radius: f32,
}

impl Default for SphereCollider {
    fn default() -> Self {
        Self {
            enabled: true,
            is_trigger: false,
            surface: PhysicsSurface::default(),
            center: Vec3::ZERO,
            radius: 0.5,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CapsuleCollider {
    pub enabled: bool,
    pub is_trigger: bool,
    pub surface: PhysicsSurface,
    pub center: Vec3,
    pub radius: f32,
    pub height: f32,
    /// 0 = X, 1 = Y, 2 = Z.
    pub direction: u8,
}

impl Default for CapsuleCollider {
    fn default() -> Self {
        Self {
            enabled: true,
            is_trigger: false,
            surface: PhysicsSurface::default(),
            center: Vec3::ZERO,
            radius: 0.5,
            height: 2.0,
            direction: 1,
        }
    }
}

/// Collider using the object's Mesh Filter mesh (convex hull when `convex`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshCollider {
    pub enabled: bool,
    pub is_trigger: bool,
    pub surface: PhysicsSurface,
    pub convex: bool,
}

impl Default for MeshCollider {
    fn default() -> Self {
        Self {
            enabled: true,
            is_trigger: false,
            surface: PhysicsSurface::default(),
            convex: false,
        }
    }
}

impl PrimitiveMesh {
    /// The collider created together with this primitive.
    pub fn default_collider(self) -> ColliderKind {
        match self {
            Self::Cube => ColliderKind::Box,
            Self::Sphere => ColliderKind::Sphere,
            Self::Capsule | Self::Cylinder => ColliderKind::Capsule,
            Self::Plane | Self::Quad => ColliderKind::Mesh,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderKind {
    Box,
    Sphere,
    Capsule,
    Mesh,
}

// ---- scripting -----------------------------------------------------------------------------

/// A public script field value shown in the Inspector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptValue {
    Bool(bool),
    Number(f64),
    Vector3([f32; 3]),
    Text(String),
}

/// One script attached to an object, with the field values set in the Inspector.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptRef {
    /// Project-relative path ("Assets/Scripts/Rotator.lua").
    pub path: String,
    pub enabled: bool,
    /// Overrides of the script's public fields (missing = script default).
    pub fields: std::collections::BTreeMap<String, ScriptValue>,
}

impl Default for ScriptRef {
    fn default() -> Self {
        Self {
            path: String::new(),
            enabled: true,
            fields: Default::default(),
        }
    }
}

impl ScriptRef {
    /// Script name from its file ("Rotator").
    pub fn name(&self) -> String {
        std::path::Path::new(&self.path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// All scripts on an object (each shows as its own "Name (Script)" section).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scripts {
    pub list: Vec<ScriptRef>,
}

// ---- prefabs -------------------------------------------------------------------------------

/// Marks the root of a prefab instance: the project-relative path of its `.prefab` asset.
/// Not listed in the Inspector's component list (the header shows a Prefab row instead).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PrefabLink {
    pub path: String,
}

// ---- audio ---------------------------------------------------------------------------------

use super::audio_components::defaults as audio_defaults;

/// How a 3D sound gets quieter with distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioRolloff {
    /// Natural falloff: `min / distance` past the min distance.
    #[default]
    Logarithmic,
    /// Straight fade from full volume at min distance to silence at max distance.
    Linear,
}

impl AudioRolloff {
    pub const ALL: [Self; 2] = [Self::Logarithmic, Self::Linear];

    pub fn label(self) -> &'static str {
        match self {
            Self::Logarithmic => "Logarithmic Rolloff",
            Self::Linear => "Linear Rolloff",
        }
    }
}

/// Plays an audio clip, in 2D or positioned in 3D around the Audio Listener.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSource {
    pub enabled: bool,
    /// Clip asset path ("Assets/Audio/Beep.wav"); `None` = no clip.
    pub clip: Option<String>,
    pub mute: bool,
    pub play_on_awake: bool,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub volume: f32,
    pub pitch: f32,
    /// -1 (left) .. 1 (right), applied to the 2D part of the mix.
    pub stereo_pan: f32,
    /// 0 = 2D, 1 = fully 3D.
    pub spatial_blend: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: AudioRolloff,
    // Newer fields are left out of files at their defaults (older scenes re-save unchanged).
    /// Mixer group: `"Assets/Audio/Main.mixer#SFX"`; `None` = straight to the listener.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// 0 = most important .. 256 = least (which voices stay audible past the voice limit).
    #[serde(skip_serializing_if = "audio_defaults::is_priority")]
    pub priority: u16,
    /// Skip the filter components on this object.
    #[serde(skip_serializing_if = "audio_defaults::is_false")]
    pub bypass_effects: bool,
    #[serde(skip_serializing_if = "audio_defaults::is_false")]
    pub bypass_listener_effects: bool,
    #[serde(skip_serializing_if = "audio_defaults::is_false")]
    pub bypass_reverb_zones: bool,
    /// 0..1.1: how much of the signal reaches Audio Reverb Zones.
    #[serde(skip_serializing_if = "audio_defaults::is_one")]
    pub reverb_zone_mix: f32,
    /// 0..5: pitch shift from relative motion (0 = none).
    #[serde(skip_serializing_if = "audio_defaults::is_one")]
    pub doppler_level: f32,
    /// 0..360°: how widely a 3D sound spreads over the speakers (0 = a point).
    #[serde(skip_serializing_if = "audio_defaults::is_zero")]
    pub spread: f32,
}

impl Default for AudioSource {
    fn default() -> Self {
        Self {
            enabled: true,
            clip: None,
            mute: false,
            play_on_awake: true,
            looping: false,
            volume: 1.0,
            pitch: 1.0,
            stereo_pan: 0.0,
            spatial_blend: 0.0,
            min_distance: 1.0,
            max_distance: 500.0,
            rolloff: AudioRolloff::Logarithmic,
            output: None,
            priority: 128,
            bypass_effects: false,
            bypass_listener_effects: false,
            bypass_reverb_zones: false,
            reverb_zone_mix: 1.0,
            doppler_level: 1.0,
            spread: 0.0,
        }
    }
}

/// The "ears" of the scene: 3D sounds are mixed relative to this object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioListener {
    pub enabled: bool,
}

impl Default for AudioListener {
    fn default() -> Self {
        Self { enabled: true }
    }
}

// ---- animation -----------------------------------------------------------------------------

/// What happens when playback reaches the end of a clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WrapMode {
    /// Use the clip's own Loop Time setting (Loop or Once).
    #[default]
    Default,
    /// Stop at the end, holding the last pose.
    Once,
    Loop,
    /// Play forwards, then backwards, and so on.
    PingPong,
    /// Hold the last pose and keep "playing".
    ClampForever,
}

impl WrapMode {
    pub const ALL: [Self; 5] = [
        Self::Default,
        Self::Once,
        Self::Loop,
        Self::PingPong,
        Self::ClampForever,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Once => "Once",
            Self::Loop => "Loop",
            Self::PingPong => "Ping Pong",
            Self::ClampForever => "Clamp Forever",
        }
    }
}

/// Plays animation clips (`.anim`) on this object and its children.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Animation {
    pub enabled: bool,
    /// Default clip asset path, played automatically and by `Play()` without a name.
    pub clip: Option<String>,
    /// Clips `Play(name)` can choose from (asset paths; the name is the file name).
    pub clips: Vec<String>,
    pub play_automatically: bool,
    pub wrap_mode: WrapMode,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            enabled: true,
            clip: None,
            clips: Vec::new(),
            play_automatically: true,
            wrap_mode: WrapMode::Default,
        }
    }
}

impl Animation {
    /// Every clip path, the default clip first, without duplicates.
    pub fn all_clips(&self) -> Vec<String> {
        let mut out: Vec<String> = self.clip.iter().cloned().collect();
        for c in &self.clips {
            if !out.contains(c) {
                out.push(c.clone());
            }
        }
        out
    }

    /// Path of the clip called `name` (file name without extension).
    pub fn find_clip(&self, name: &str) -> Option<String> {
        self.all_clips().into_iter().find(|c| clip_name(c) == name)
    }
}

/// Clip name shown to users and used by `Play(name)`: the file name without extension.
pub fn clip_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
