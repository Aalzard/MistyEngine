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
    /// A cubic Bézier curve from (0, 0) to (1, 1) through two control points.
    Custom,
}

impl BlendStyle {
    pub const ALL: [BlendStyle; 6] = [
        Self::Cut,
        Self::EaseInOut,
        Self::EaseIn,
        Self::EaseOut,
        Self::Linear,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cut => "Cut",
            Self::EaseInOut => "Ease In Out",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::Linear => "Linear",
            Self::Custom => "Custom",
        }
    }

    /// Eased weight of the incoming camera for linear progress `t` (0..1). Custom curves need
    /// their control points ([`BlendDefinition::weight`]); here they act as Ease In Out.
    pub fn ease(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Cut => 1.0,
            Self::Linear => t,
            Self::Custom => t * t * (3.0 - 2.0 * t),
            Self::EaseInOut => t * t * (3.0 - 2.0 * t),
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// Control points (x1, y1, x2, y2) of a Custom blend curve; the default is a gentle ease.
pub const DEFAULT_CURVE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];

/// A blend style and how long it takes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendDefinition {
    pub style: BlendStyle,
    /// Seconds (ignored by Cut).
    pub duration: f32,
    /// Custom style: the curve's two control points (x1, y1, x2, y2), x in 0..1.
    #[serde(skip_serializing_if = "is_default_curve")]
    pub curve: [f32; 4],
}

fn is_default_curve(c: &[f32; 4]) -> bool {
    *c == DEFAULT_CURVE
}

impl Default for BlendDefinition {
    fn default() -> Self {
        Self {
            style: BlendStyle::EaseInOut,
            duration: 2.0,
            curve: DEFAULT_CURVE,
        }
    }
}

impl BlendDefinition {
    /// True when switching jumps at once.
    pub fn is_cut(&self) -> bool {
        self.style == BlendStyle::Cut || self.duration <= 0.0
    }

    /// Weight of the incoming camera for linear progress `t` (0..1), custom curves included.
    pub fn weight(&self, t: f32) -> f32 {
        match self.style {
            BlendStyle::Custom => bezier_ease(self.curve, t),
            s => s.ease(t),
        }
    }
}

/// A cubic Bézier ease from (0, 0) to (1, 1) with control points `c` = (x1, y1, x2, y2):
/// solves x(u) = t (x stays monotonic with x1, x2 clamped into 0..1) and returns y(u).
pub fn bezier_ease(c: [f32; 4], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (x1, y1, x2, y2) = (c[0].clamp(0.0, 1.0), c[1], c[2].clamp(0.0, 1.0), c[3]);
    let coord = |u: f32, p1: f32, p2: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if coord(mid, x1, x2) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    coord(0.5 * (lo + hi), y1, y2)
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
    /// Blend list asset (`.blends`), checked before `custom_blends`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend_list: Option<String>,
    pub update_method: BrainUpdate,
}

impl Default for CameraBrain {
    fn default() -> Self {
        Self {
            enabled: true,
            default_blend: BlendDefinition::default(),
            custom_blends: Vec::new(),
            blend_list: None,
            update_method: BrainUpdate::LateUpdate,
        }
    }
}

/// The best match for `from` → `to` in `list`: an exact pair first, then a pair with one side
/// "any" (empty), then any → any.
pub fn find_blend(list: &[CustomBlend], from: &str, to: &str) -> Option<BlendDefinition> {
    let find = |f: &dyn Fn(&CustomBlend) -> bool| list.iter().find(|b| f(b));
    find(&|b| b.from == from && b.to == to)
        .or_else(|| find(&|b| b.from.is_empty() && b.to == to))
        .or_else(|| find(&|b| b.from == from && b.to.is_empty()))
        .or_else(|| find(&|b| b.from.is_empty() && b.to.is_empty()))
        .map(|b| b.blend)
}

impl CameraBrain {
    /// The blend from `from` to `to` (object names): the inline list, else the default.
    pub fn blend_for(&self, from: &str, to: &str) -> BlendDefinition {
        self.blend_with(&[], from, to)
    }

    /// The blend from `from` to `to`: the blend list asset's entries (`list`) first, then the
    /// inline custom blends, then the default blend.
    pub fn blend_with(&self, list: &[CustomBlend], from: &str, to: &str) -> BlendDefinition {
        find_blend(list, from, to)
            .or_else(|| find_blend(&self.custom_blends, from, to))
            .unwrap_or(self.default_blend)
    }
}

/// File extension of blend list assets.
pub const BLEND_LIST_EXTENSION: &str = "blends";

/// A blend list asset (`.blends`, JSON): custom blends per camera pair, shared by brains.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendList {
    pub blends: Vec<CustomBlend>,
}

impl BlendList {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
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
    /// Input System action ("Map/Action" or "Action") read from the playing Player Inputs;
    /// the horizontal axis takes its x, the vertical axis its y. Wins over `input_axis`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input_action: String,
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
            input_action: String::new(),
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
            input_action: String::new(),
        }
    }

    /// The Free Look rig axis: 0 = bottom orbit, 0.5 = middle, 1 = top.
    fn rig() -> Self {
        Self {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            wrap: false,
            speed: 2.0,
            input_axis: "Mouse Y".into(),
            invert: true,
            input_action: String::new(),
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

/// How an orbital camera's vertical axis places it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OrbitStyle {
    /// One sphere: the vertical axis is a pitch angle.
    #[default]
    Sphere,
    /// Free Look: three horizontal rings (top, middle, bottom) joined by a spline; the
    /// vertical axis (0..1) slides between them.
    ThreeRings,
}

impl OrbitStyle {
    pub const ALL: [OrbitStyle; 2] = [Self::Sphere, Self::ThreeRings];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "Sphere",
            Self::ThreeRings => "Three Rings",
        }
    }
}

/// One Free Look ring: its height above the target and its radius.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Orbit {
    pub height: f32,
    pub radius: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            height: 2.5,
            radius: 3.0,
        }
    }
}

/// Top, middle and bottom rings of a new Free Look camera.
pub const DEFAULT_ORBITS: [Orbit; 3] = [
    Orbit {
        height: 4.5,
        radius: 1.75,
    },
    Orbit {
        height: 2.5,
        radius: 3.0,
    },
    Orbit {
        height: 0.4,
        radius: 1.3,
    },
];

fn is_default_orbits(o: &[Orbit; 3]) -> bool {
    *o == DEFAULT_ORBITS
}

fn is_default_curvature(c: &f32) -> bool {
    *c == DEFAULT_SPLINE_CURVATURE
}

fn is_sphere(s: &OrbitStyle) -> bool {
    *s == OrbitStyle::Sphere
}

/// How much the Free Look spline bends between its rings by default.
pub const DEFAULT_SPLINE_CURVATURE: f32 = 0.2;

/// Orbital: circle the follow target at `radius`, yaw and pitch from input axes (Sphere), or
/// slide between three rings (Three Rings, the Free Look rig).
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
    #[serde(skip_serializing_if = "is_sphere")]
    pub style: OrbitStyle,
    /// Three Rings: top, middle and bottom.
    #[serde(skip_serializing_if = "is_default_orbits")]
    pub orbits: [Orbit; 3],
    /// Three Rings: 0 = straight lines between the rings, 1 = a smooth curve through them.
    #[serde(skip_serializing_if = "is_default_curvature")]
    pub spline_curvature: f32,
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
            style: OrbitStyle::Sphere,
            orbits: DEFAULT_ORBITS,
            spline_curvature: DEFAULT_SPLINE_CURVATURE,
        }
    }
}

impl Orbital {
    /// The Free Look rig: three rings, vertical axis 0..1 starting on the middle ring.
    pub fn three_rings() -> Self {
        Self {
            style: OrbitStyle::ThreeRings,
            height_offset: 0.0,
            y_axis: OrbitAxis::rig(),
            ..Default::default()
        }
    }

    /// Three Rings: (height, radius) for vertical axis value `y` (0 = bottom ring, 0.5 =
    /// middle, 1 = top). Straight lines between the rings, bent towards the parabola through
    /// all three by the spline curvature.
    pub fn ring_at(&self, y: f32) -> (f32, f32) {
        let y = y.clamp(0.0, 1.0);
        let [top, mid, bottom] = self.orbits;
        let pts = [
            (bottom.height, bottom.radius),
            (mid.height, mid.radius),
            (top.height, top.radius),
        ];
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let linear = |k: usize| {
            let v = |p: (f32, f32)| if k == 0 { p.0 } else { p.1 };
            if y < 0.5 {
                lerp(v(pts[0]), v(pts[1]), y * 2.0)
            } else {
                lerp(v(pts[1]), v(pts[2]), (y - 0.5) * 2.0)
            }
        };
        // Quadratic through (0, p0), (0.5, p1), (1, p2).
        let curve = |k: usize| {
            let v = |p: (f32, f32)| if k == 0 { p.0 } else { p.1 };
            let (a, b, c) = (v(pts[0]), v(pts[1]), v(pts[2]));
            let l0 = 2.0 * (y - 0.5) * (y - 1.0);
            let l1 = -4.0 * y * (y - 1.0);
            let l2 = 2.0 * y * (y - 0.5);
            a * l0 + b * l1 + c * l2
        };
        let k = self.spline_curvature.clamp(0.0, 1.0);
        (
            lerp(linear(0), curve(0), k),
            lerp(linear(1), curve(1), k).max(0.0),
        )
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
    /// Frame a Target Group: aim at its centre like the Composer, then move closer or further
    /// (dolly) and / or zoom so the whole group fills the framing size.
    GroupComposer,
}

impl AimKind {
    pub const ALL: [AimKind; 4] = [
        Self::DoNothing,
        Self::Composer,
        Self::HardLookAt,
        Self::GroupComposer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::DoNothing => "Do Nothing",
            Self::Composer => "Composer",
            Self::HardLookAt => "Hard Look At",
            Self::GroupComposer => "Group Composer",
        }
    }
}

/// Which screen dimension the group has to fit in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FramingMode {
    Horizontal,
    Vertical,
    /// Whichever is tighter.
    #[default]
    HorizontalAndVertical,
}

impl FramingMode {
    pub const ALL: [FramingMode; 3] = [
        Self::Horizontal,
        Self::Vertical,
        Self::HorizontalAndVertical,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::HorizontalAndVertical => "Horizontal And Vertical",
        }
    }
}

/// How the Group Composer makes the group fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GroupAdjustment {
    /// Change the field of view only.
    ZoomOnly,
    /// Move along the view line only.
    DollyOnly,
    /// Move first (within the dolly limits), then zoom for the rest.
    #[default]
    DollyThenZoom,
}

impl GroupAdjustment {
    pub const ALL: [GroupAdjustment; 3] = [Self::ZoomOnly, Self::DollyOnly, Self::DollyThenZoom];

    pub fn label(self) -> &'static str {
        match self {
            Self::ZoomOnly => "Zoom Only",
            Self::DollyOnly => "Dolly Only",
            Self::DollyThenZoom => "Dolly Then Zoom",
        }
    }
}

/// Group Composer settings: how much of the screen the group fills and how the camera gets
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupFraming {
    /// Fraction of the screen (in the framing dimension) the group's bounds fill.
    pub framing_size: f32,
    pub framing_mode: FramingMode,
    pub adjustment: GroupAdjustment,
    /// Seconds to settle on the new distance / field of view.
    pub damping: f32,
    /// Furthest the camera moves towards / away from the group from where the body put it.
    pub max_dolly_in: f32,
    pub max_dolly_out: f32,
    pub minimum_distance: f32,
    pub maximum_distance: f32,
    /// Zoom limits in degrees (vertical field of view).
    pub minimum_fov: f32,
    pub maximum_fov: f32,
}

impl Default for GroupFraming {
    fn default() -> Self {
        Self {
            framing_size: 0.8,
            framing_mode: FramingMode::HorizontalAndVertical,
            adjustment: GroupAdjustment::DollyThenZoom,
            damping: 1.0,
            max_dolly_in: 100.0,
            max_dolly_out: 100.0,
            minimum_distance: 1.0,
            maximum_distance: 500.0,
            minimum_fov: 3.0,
            maximum_fov: 60.0,
        }
    }
}

/// Confiner: keeps the camera inside a volume (the Box or Sphere Collider of an object, else
/// the object's unit cube).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Confiner {
    pub enabled: bool,
    /// Object whose collider bounds the camera.
    #[serde(with = "entity_bits::option")]
    pub volume: Option<Entity>,
    /// Seconds to settle back inside after leaving the volume (0 = at once).
    pub damping: f32,
}

impl Default for Confiner {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: None,
            damping: 0.0,
        }
    }
}

/// Impulse Listener: the camera shakes with impulses from Impulse Sources near it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImpulseListener {
    pub enabled: bool,
    /// Scales every impulse this camera feels.
    pub gain: f32,
    /// Channels (bits) it listens to.
    pub channel_mask: u32,
    /// Measure distance to sources on the ground plane (ignores height).
    pub use_2d_distance: bool,
}

impl Default for ImpulseListener {
    fn default() -> Self {
        Self {
            enabled: false,
            gain: 1.0,
            channel_mask: u32::MAX,
            use_2d_distance: false,
        }
    }
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
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
    /// Group Composer settings.
    #[serde(skip_serializing_if = "is_default")]
    pub group_framing: GroupFraming,
    #[serde(skip_serializing_if = "is_default")]
    pub confiner: Confiner,
    #[serde(skip_serializing_if = "is_default")]
    pub impulse: ImpulseListener,
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
            group_framing: GroupFraming::default(),
            confiner: Confiner::default(),
            impulse: ImpulseListener::default(),
        }
    }
}

impl VirtualCamera {
    /// A Free Look camera: three orbit rings around its target, steered by input, keeping it
    /// framed.
    pub fn free_look() -> Self {
        Self {
            body: BodyKind::Orbital,
            orbital: Orbital::three_rings(),
            ..Default::default()
        }
    }

    /// A camera that frames a Target Group.
    pub fn group_framing() -> Self {
        Self {
            aim: AimKind::GroupComposer,
            ..Default::default()
        }
    }
}

impl SceneComponent for VirtualCamera {
    const NAME: &'static str = "Virtual Camera";
    const CATEGORY: &'static str = "Rendering";
}
