//! The Camera Brain (on a real Camera) and Virtual Camera components, as saved in scenes.

use glam::{Vec2, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::scene::SceneComponent;
use crate::scene::components::entity_bits;

/// How the brain moves from one live virtual camera to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendStyle {
    /// Jump straight to the new camera.
    Cut,
    /// Slow start and end (smoothstep).
    #[default]
    EaseInOut,
    EaseIn,
    EaseOut,
    /// Constant speed.
    Linear,
}

impl BlendStyle {
    pub const ALL: [BlendStyle; 5] = [
        Self::Cut,
        Self::EaseInOut,
        Self::EaseIn,
        Self::EaseOut,
        Self::Linear,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cut => "Cut",
            Self::EaseInOut => "Ease In Out",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::Linear => "Linear",
        }
    }

    /// Eased weight of the incoming camera for linear progress `t` (0..1).
    pub fn ease(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Cut => 1.0,
            Self::Linear => t,
            Self::EaseInOut => t * t * (3.0 - 2.0 * t),
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// A blend style and how long it takes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendDefinition {
    pub style: BlendStyle,
    /// Seconds (ignored by Cut).
    pub duration: f32,
}

impl Default for BlendDefinition {
    fn default() -> Self {
        Self {
            style: BlendStyle::EaseInOut,
            duration: 2.0,
        }
    }
}

impl BlendDefinition {
    /// True when switching jumps at once.
    pub fn is_cut(&self) -> bool {
        self.style == BlendStyle::Cut || self.duration <= 0.0
    }
}

/// A blend used between two particular virtual cameras (by object name; empty = any camera).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomBlend {
    pub from: String,
    pub to: String,
    pub blend: BlendDefinition,
}

/// When the brain moves the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BrainUpdate {
    /// After every script's LateUpdate (follows objects moved in Update).
    #[default]
    LateUpdate,
    /// Right after the physics steps, by the fixed time they covered (follows rigidbodies).
    FixedUpdate,
}

impl BrainUpdate {
    pub const ALL: [BrainUpdate; 2] = [Self::LateUpdate, Self::FixedUpdate];

    pub fn label(self) -> &'static str {
        match self {
            Self::LateUpdate => "Late Update",
            Self::FixedUpdate => "Fixed Update",
        }
    }
}

/// Camera Brain: drives the Camera on its object from the live Virtual Camera while playing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraBrain {
    pub enabled: bool,
    /// Blend used when no custom blend matches.
    pub default_blend: BlendDefinition,
    /// Blends for particular pairs, checked in order (exact pairs win over "any").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_blends: Vec<CustomBlend>,
    pub update_method: BrainUpdate,
}

impl Default for CameraBrain {
    fn default() -> Self {
        Self {
            enabled: true,
            default_blend: BlendDefinition::default(),
            custom_blends: Vec::new(),
            update_method: BrainUpdate::LateUpdate,
        }
    }
}

impl CameraBrain {
    /// The blend from `from` to `to` (object names): an exact pair first, then a pair with one
    /// side "any" (empty), then the default blend.
    pub fn blend_for(&self, from: &str, to: &str) -> BlendDefinition {
        let find = |f: &dyn Fn(&CustomBlend) -> bool| self.custom_blends.iter().find(|b| f(b));
        find(&|b| b.from == from && b.to == to)
            .or_else(|| find(&|b| b.from.is_empty() && b.to == to))
            .or_else(|| find(&|b| b.from == from && b.to.is_empty()))
            .or_else(|| find(&|b| b.from.is_empty() && b.to.is_empty()))
            .map_or(self.default_blend, |b| b.blend)
    }
}

impl SceneComponent for CameraBrain {
    const NAME: &'static str = "Camera Brain";
    const CATEGORY: &'static str = "Rendering";
}

/// Lens a virtual camera hands to the real camera.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lens {
    /// Vertical field of view in degrees.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    /// Roll around the view axis in degrees.
    pub dutch: f32,
}

impl Default for Lens {
    fn default() -> Self {
        Self {
            fov: 60.0,
            near: 0.3,
            far: 1000.0,
            dutch: 0.0,
        }
    }
}

/// Which algorithm positions the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BodyKind {
    /// Keep the object's own position.
    DoNothing,
    /// A fixed offset from the follow target.
    #[default]
    Transposer,
    /// Over-the-shoulder rig behind the target.
    ThirdPerson,
    /// Orbit around the target, steered by input axes.
    Orbital,
}

impl BodyKind {
    pub const ALL: [BodyKind; 4] = [
        Self::DoNothing,
        Self::Transposer,
        Self::ThirdPerson,
        Self::Orbital,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::DoNothing => "Do Nothing",
            Self::Transposer => "Transposer",
            Self::ThirdPerson => "Third Person",
            Self::Orbital => "Orbital",
        }
    }
}

/// The space a follow offset is expressed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BindingMode {
    /// World axes.
    World,
    /// The target's rotation, all axes.
    LockToTarget,
    /// The target's heading only (stays level).
    #[default]
    LockToTargetWithWorldUp,
}

impl BindingMode {
    pub const ALL: [BindingMode; 3] = [
        Self::World,
        Self::LockToTarget,
        Self::LockToTargetWithWorldUp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::World => "World Space",
            Self::LockToTarget => "Lock To Target",
            Self::LockToTargetWithWorldUp => "Lock To Target With World Up",
        }
    }
}

/// Transposer: sit at `offset` from the follow target.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transposer {
    pub binding: BindingMode,
    pub offset: Vec3,
    /// Seconds to catch up per axis of the binding space (0 = rigid).
    pub damping: Vec3,
}

impl Default for Transposer {
    fn default() -> Self {
        Self {
            binding: BindingMode::LockToTargetWithWorldUp,
            offset: Vec3::new(0.0, 2.0, -8.0),
            damping: Vec3::new(1.0, 1.0, 1.0),
        }
    }
}

/// Third person: a shoulder pivot on the target, the camera `camera_distance` behind it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThirdPerson {
    /// Shoulder position relative to the target (x flips with `camera_side`).
    pub shoulder_offset: Vec3,
    /// Height of the arm above the shoulder.
    pub vertical_arm_length: f32,
    /// 0 = left shoulder, 1 = right shoulder.
    pub camera_side: f32,
    pub camera_distance: f32,
    /// Seconds the tracked point takes to catch the target (target-local x, y, z).
    pub damping: Vec3,
    /// Screen fraction (width, height) the target may move in without the camera following.
    pub dead_zone: Vec2,
}

impl Default for ThirdPerson {
    fn default() -> Self {
        Self {
            shoulder_offset: Vec3::new(0.5, 1.5, 0.0),
            vertical_arm_length: 0.4,
            camera_side: 1.0,
            camera_distance: 4.0,
            damping: Vec3::new(0.1, 0.5, 0.3),
            dead_zone: Vec2::ZERO,
        }
    }
}

/// One input-driven orbit axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrbitAxis {
    /// Current angle in degrees (changes while playing).
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// Wraps around instead of stopping at min / max.
    pub wrap: bool,
    /// Degrees per second at full input.
    pub speed: f32,
    /// Input Manager axis that drives it (empty = none).
    pub input_axis: String,
    pub invert: bool,
}

impl OrbitAxis {
    fn yaw() -> Self {
        Self {
            value: 0.0,
            min: -180.0,
            max: 180.0,
            wrap: true,
            speed: 300.0,
            input_axis: "Mouse X".into(),
            invert: false,
        }
    }

    fn pitch() -> Self {
        Self {
            value: 15.0,
            min: -10.0,
            max: 70.0,
            wrap: false,
            speed: 200.0,
            input_axis: "Mouse Y".into(),
            invert: true,
        }
    }

    /// Advance by `input` (−1..1 or a mouse delta) over `dt` seconds.
    pub fn apply(&mut self, input: f32, dt: f32) {
        let sign = if self.invert { -1.0 } else { 1.0 };
        self.value += input * sign * self.speed * dt;
        self.clamp();
    }

    /// Keep the value in range (wrapping axes go round).
    pub fn clamp(&mut self) {
        let (lo, hi) = (self.min.min(self.max), self.max.max(self.min));
        if self.wrap && hi - lo > 1e-3 {
            let span = hi - lo;
            self.value = lo + (self.value - lo).rem_euclid(span);
        } else {
            self.value = self.value.clamp(lo, hi);
        }
    }
}

impl Default for OrbitAxis {
    fn default() -> Self {
        Self::yaw()
    }
}

/// Orbital: circle the follow target at `radius`, yaw and pitch from input axes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Orbital {
    /// World: yaw 0 looks along +Z. With world up: yaw is relative to the target's heading.
    pub binding: BindingMode,
    pub radius: f32,
    /// Height of the orbit centre above the target.
    pub height_offset: f32,
    pub x_axis: OrbitAxis,
    pub y_axis: OrbitAxis,
    /// Seconds the orbit centre takes to catch the target (x, y, z).
    pub damping: Vec3,
}

impl Default for Orbital {
    fn default() -> Self {
        Self {
            binding: BindingMode::World,
            radius: 6.0,
            height_offset: 1.0,
            x_axis: OrbitAxis::yaw(),
            y_axis: OrbitAxis::pitch(),
            damping: Vec3::new(0.2, 0.2, 0.2),
        }
    }
}

/// Which algorithm rotates the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AimKind {
    /// Keep the rotation from the body (third person) or the object.
    DoNothing,
    /// Keep the look-at target in a screen region, with damping.
    #[default]
    Composer,
    /// Always look straight at the target.
    HardLookAt,
}

impl AimKind {
    pub const ALL: [AimKind; 3] = [Self::DoNothing, Self::Composer, Self::HardLookAt];

    pub fn label(self) -> &'static str {
        match self {
            Self::DoNothing => "Do Nothing",
            Self::Composer => "Composer",
            Self::HardLookAt => "Hard Look At",
        }
    }
}

/// Composer: the look-at target rests in the dead zone; in the soft zone the camera turns
/// towards it with damping; it never leaves the soft zone.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Composer {
    /// Point on the target to frame, in its local space.
    pub tracked_offset: Vec3,
    /// Where the target sits on screen (0..1, y down; 0.5 = centre).
    pub screen_x: f32,
    pub screen_y: f32,
    /// Screen fractions around the screen position.
    pub dead_zone_width: f32,
    pub dead_zone_height: f32,
    pub soft_zone_width: f32,
    pub soft_zone_height: f32,
    /// Seconds to turn towards the target.
    pub horizontal_damping: f32,
    pub vertical_damping: f32,
}

impl Default for Composer {
    fn default() -> Self {
        Self {
            tracked_offset: Vec3::ZERO,
            screen_x: 0.5,
            screen_y: 0.5,
            dead_zone_width: 0.1,
            dead_zone_height: 0.1,
            soft_zone_width: 0.8,
            soft_zone_height: 0.8,
            horizontal_damping: 0.5,
            vertical_damping: 0.5,
        }
    }
}

impl Composer {
    /// Dead and soft zones as screen rects (min x, min y, max x, max y), soft ⊇ dead.
    pub fn zones(&self) -> ([f32; 4], [f32; 4]) {
        let (cx, cy) = (self.screen_x, self.screen_y);
        let dw = self.dead_zone_width.clamp(0.0, 2.0);
        let dh = self.dead_zone_height.clamp(0.0, 2.0);
        let sw = self.soft_zone_width.clamp(dw, 2.0);
        let sh = self.soft_zone_height.clamp(dh, 2.0);
        let rect = |w: f32, h: f32| [cx - w * 0.5, cy - h * 0.5, cx + w * 0.5, cy + h * 0.5];
        (rect(dw, dh), rect(sw, sh))
    }
}

/// Built-in noise shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NoiseProfile {
    /// Slow, gentle sway like a hand-held camera.
    #[default]
    Handheld,
    /// Fast, strong shake.
    Shake,
}

impl NoiseProfile {
    pub const ALL: [NoiseProfile; 2] = [Self::Handheld, Self::Shake];

    pub fn label(self) -> &'static str {
        match self {
            Self::Handheld => "Handheld",
            Self::Shake => "Shake",
        }
    }
}

/// Procedural (Perlin) shake on top of the camera pose.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Noise {
    pub enabled: bool,
    pub profile: NoiseProfile,
    pub amplitude_gain: f32,
    pub frequency_gain: f32,
    /// Same seed, same shake.
    pub seed: u32,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            enabled: false,
            profile: NoiseProfile::Handheld,
            amplitude_gain: 1.0,
            frequency_gain: 1.0,
            seed: 0,
        }
    }
}

/// Deoccluder: pulls the camera in front of colliders between it and its target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraCollider {
    pub enabled: bool,
    /// Layers that block the view.
    pub collide_against: u32,
    /// Objects with this tag never block (empty = none).
    pub ignore_tag: String,
    /// Gap kept between the camera and the obstacle.
    pub camera_radius: f32,
    /// Never closer to the target than this.
    pub minimum_distance: f32,
    /// Seconds to move back out once the view clears.
    pub damping: f32,
    /// Seconds to move in when something blocks the view (0 = at once).
    pub damping_when_occluded: f32,
}

impl Default for CameraCollider {
    fn default() -> Self {
        Self {
            enabled: false,
            collide_against: u32::MAX,
            ignore_tag: String::new(),
            camera_radius: 0.2,
            minimum_distance: 0.5,
            damping: 0.5,
            damping_when_occluded: 0.0,
        }
    }
}

/// Virtual Camera: a camera pose recipe. The highest-priority active one drives the brain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VirtualCamera {
    pub enabled: bool,
    pub priority: i32,
    /// Object the body follows.
    #[serde(with = "entity_bits::option")]
    pub follow: Option<Entity>,
    /// Object the aim looks at.
    #[serde(with = "entity_bits::option")]
    pub look_at: Option<Entity>,
    pub lens: Lens,
    pub body: BodyKind,
    pub transposer: Transposer,
    pub third_person: ThirdPerson,
    pub orbital: Orbital,
    pub aim: AimKind,
    pub composer: Composer,
    pub noise: Noise,
    pub collider: CameraCollider,
}

impl Default for VirtualCamera {
    fn default() -> Self {
        Self {
            enabled: true,
            priority: 10,
            follow: None,
            look_at: None,
            lens: Lens::default(),
            body: BodyKind::Transposer,
            transposer: Transposer::default(),
            third_person: ThirdPerson::default(),
            orbital: Orbital::default(),
            aim: AimKind::Composer,
            composer: Composer::default(),
            noise: Noise::default(),
            collider: CameraCollider::default(),
        }
    }
}

impl VirtualCamera {
    /// A Free Look camera: orbits its target from input and keeps it framed.
    pub fn free_look() -> Self {
        Self {
            body: BodyKind::Orbital,
            ..Default::default()
        }
    }
}

impl SceneComponent for VirtualCamera {
    const NAME: &'static str = "Virtual Camera";
    const CATEGORY: &'static str = "Rendering";
}
