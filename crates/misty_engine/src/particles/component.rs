//! The Particle System component: serialised settings split into the familiar modules (Main,
//! Emission, Shape, Velocity / Limit Velocity / Force over Lifetime, Color over Lifetime, Color
//! by Speed, Size over Lifetime / by Speed, Rotation over Lifetime / by Speed, Noise,
//! Collision, Sub Emitters, Texture Sheet Animation, Trails, Renderer).
//!
//! Numbers are [`MinMax`] curves (Constant, Curve, Random Between Two Constants, Random Between
//! Two Curves) and colours [`MinMaxColor`] gradients (Color, Gradient, Random Between Two
//! Colors, Random Between Two Gradients), like the reference engine's MinMaxCurve and
//! MinMaxGradient. Modules added after the first version are skipped in files while they hold
//! their defaults, so older scenes keep their exact text.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::rng::Rng;

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// A piecewise-linear curve over 0..1 (time, value) keys, like a simple animation curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curve {
    pub keys: Vec<[f32; 2]>,
}

impl Default for Curve {
    fn default() -> Self {
        Self::linear(1.0, 1.0)
    }
}

impl Curve {
    /// Straight line from `a` (t = 0) to `b` (t = 1).
    pub fn linear(a: f32, b: f32) -> Self {
        Self {
            keys: vec![[0.0, a], [1.0, b]],
        }
    }

    /// A curve through `(time, value)` keys (sorted by time).
    pub fn from_keys(keys: &[[f32; 2]]) -> Self {
        let mut c = Self {
            keys: keys.to_vec(),
        };
        c.sort();
        c
    }

    pub fn evaluate(&self, t: f32) -> f32 {
        let mut prev: Option<[f32; 2]> = None;
        for k in &self.keys {
            if t <= k[0] {
                return match prev {
                    Some(p) if k[0] > p[0] => p[1] + (k[1] - p[1]) * (t - p[0]) / (k[0] - p[0]),
                    _ => k[1],
                };
            }
            prev = Some(*k);
        }
        prev.map_or(1.0, |p| p[1])
    }

    pub fn sort(&mut self) {
        self.keys.sort_by(|a, b| a[0].total_cmp(&b[0]));
    }

    /// Largest value on the curve.
    pub fn max_value(&self) -> f32 {
        self.keys.iter().map(|k| k[1]).fold(f32::MIN, f32::max)
    }
}

/// How a [`MinMax`] produces its value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveMode {
    #[default]
    Constant,
    Curve,
    RandomBetweenTwoConstants,
    RandomBetweenTwoCurves,
}

impl CurveMode {
    pub const ALL: [Self; 4] = [
        Self::Constant,
        Self::Curve,
        Self::RandomBetweenTwoConstants,
        Self::RandomBetweenTwoCurves,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Constant => "Constant",
            Self::Curve => "Curve",
            Self::RandomBetweenTwoConstants => "Random Between Two Constants",
            Self::RandomBetweenTwoCurves => "Random Between Two Curves",
        }
    }
}

/// A number: a constant (`a`), random between two constants (`a`..`b`), a curve (`curve_max` ×
/// `a`) or random between two curves (`curve_min`..`curve_max`, × `a`). Curves are evaluated
/// over the particle's life (or the system's cycle for start values and emission).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MinMax {
    pub a: f32,
    pub b: f32,
    pub random: bool,
    /// Curve modes (`a` is then the curve multiplier).
    #[serde(skip_serializing_if = "is_false")]
    pub curve: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub curve_min: Curve,
    #[serde(skip_serializing_if = "is_default")]
    pub curve_max: Curve,
}

impl Default for MinMax {
    fn default() -> Self {
        Self::constant(0.0)
    }
}

impl MinMax {
    pub fn constant(v: f32) -> Self {
        Self {
            a: v,
            b: v,
            random: false,
            curve: false,
            curve_min: Curve::default(),
            curve_max: Curve::default(),
        }
    }

    pub fn between(a: f32, b: f32) -> Self {
        Self {
            random: true,
            b,
            ..Self::constant(a)
        }
    }

    /// `curve` × `multiplier`.
    pub fn curve(multiplier: f32, curve: Curve) -> Self {
        Self {
            curve: true,
            curve_max: curve,
            ..Self::constant(multiplier)
        }
    }

    /// Random between `min` and `max` curves, × `multiplier`.
    pub fn between_curves(multiplier: f32, min: Curve, max: Curve) -> Self {
        Self {
            curve: true,
            random: true,
            curve_min: min,
            curve_max: max,
            ..Self::constant(multiplier)
        }
    }

    pub fn mode(&self) -> CurveMode {
        match (self.curve, self.random) {
            (false, false) => CurveMode::Constant,
            (false, true) => CurveMode::RandomBetweenTwoConstants,
            (true, false) => CurveMode::Curve,
            (true, true) => CurveMode::RandomBetweenTwoCurves,
        }
    }

    pub fn set_mode(&mut self, m: CurveMode) {
        self.curve = matches!(m, CurveMode::Curve | CurveMode::RandomBetweenTwoCurves);
        self.random = matches!(
            m,
            CurveMode::RandomBetweenTwoConstants | CurveMode::RandomBetweenTwoCurves
        );
    }

    /// The value at `t` (0..1) for a random pick `r` (0..1).
    pub fn evaluate(&self, t: f32, r: f32) -> f32 {
        match self.mode() {
            CurveMode::Constant => self.a,
            CurveMode::RandomBetweenTwoConstants => self.a + (self.b - self.a) * r,
            CurveMode::Curve => self.curve_max.evaluate(t) * self.a,
            CurveMode::RandomBetweenTwoCurves => {
                let (lo, hi) = (self.curve_min.evaluate(t), self.curve_max.evaluate(t));
                (lo + (hi - lo) * r) * self.a
            }
        }
    }

    /// A start value: the curve at `t` (the system's cycle), a fresh random pick.
    pub fn sample_at(&self, t: f32, rng: &mut Rng) -> f32 {
        let r = if self.random { rng.next_f32() } else { 0.0 };
        self.evaluate(t, r)
    }

    /// [`Self::sample_at`] at the start of the cycle.
    pub fn sample(&self, rng: &mut Rng) -> f32 {
        self.sample_at(0.0, rng)
    }

    /// Largest value it can produce (for gizmos and bounds).
    pub fn max(&self) -> f32 {
        match self.mode() {
            CurveMode::Constant => self.a,
            CurveMode::RandomBetweenTwoConstants => self.a.max(self.b),
            CurveMode::Curve => self.curve_max.max_value() * self.a,
            CurveMode::RandomBetweenTwoCurves => {
                self.curve_min.max_value().max(self.curve_max.max_value()) * self.a
            }
        }
    }

    /// Is this the constant zero (a module term that does nothing)?
    pub fn is_zero(&self) -> bool {
        self.mode() == CurveMode::Constant && self.a == 0.0
    }
}

/// How a [`MinMaxColor`] produces its colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode {
    #[default]
    Color,
    Gradient,
    RandomBetweenTwoColors,
    RandomBetweenTwoGradients,
}

impl ColorMode {
    pub const ALL: [Self; 4] = [
        Self::Color,
        Self::Gradient,
        Self::RandomBetweenTwoColors,
        Self::RandomBetweenTwoGradients,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Color => "Color",
            Self::Gradient => "Gradient",
            Self::RandomBetweenTwoColors => "Random Between Two Colors",
            Self::RandomBetweenTwoGradients => "Random Between Two Gradients",
        }
    }
}

/// A colour (sRGB + alpha): constant, random between two colours, a gradient or random between
/// two gradients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MinMaxColor {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub random: bool,
    /// Gradient modes.
    #[serde(skip_serializing_if = "is_false")]
    pub gradient: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub gradient_a: Gradient,
    #[serde(skip_serializing_if = "is_default")]
    pub gradient_b: Gradient,
}

impl Default for MinMaxColor {
    fn default() -> Self {
        Self {
            a: [1.0; 4],
            b: [1.0; 4],
            random: false,
            gradient: false,
            gradient_a: Gradient::default(),
            gradient_b: Gradient::default(),
        }
    }
}

impl MinMaxColor {
    pub fn constant(c: [f32; 4]) -> Self {
        Self {
            a: c,
            b: c,
            ..Default::default()
        }
    }

    pub fn between(a: [f32; 4], b: [f32; 4]) -> Self {
        Self {
            a,
            b,
            random: true,
            ..Default::default()
        }
    }

    pub fn mode(&self) -> ColorMode {
        match (self.gradient, self.random) {
            (false, false) => ColorMode::Color,
            (false, true) => ColorMode::RandomBetweenTwoColors,
            (true, false) => ColorMode::Gradient,
            (true, true) => ColorMode::RandomBetweenTwoGradients,
        }
    }

    pub fn set_mode(&mut self, m: ColorMode) {
        self.gradient = matches!(
            m,
            ColorMode::Gradient | ColorMode::RandomBetweenTwoGradients
        );
        self.random = matches!(
            m,
            ColorMode::RandomBetweenTwoColors | ColorMode::RandomBetweenTwoGradients
        );
    }

    /// The colour at `t` (0..1) for a random pick `r`.
    pub fn evaluate(&self, t: f32, r: f32) -> [f32; 4] {
        let lerp = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * r);
        match self.mode() {
            ColorMode::Color => self.a,
            ColorMode::RandomBetweenTwoColors => lerp(self.a, self.b),
            ColorMode::Gradient => self.gradient_a.evaluate(t),
            ColorMode::RandomBetweenTwoGradients => {
                lerp(self.gradient_a.evaluate(t), self.gradient_b.evaluate(t))
            }
        }
    }

    pub fn sample_at(&self, t: f32, rng: &mut Rng) -> [f32; 4] {
        let r = if self.random { rng.next_f32() } else { 0.0 };
        self.evaluate(t, r)
    }

    pub fn sample(&self, rng: &mut Rng) -> [f32; 4] {
        self.sample_at(0.0, rng)
    }
}

/// Where particles live once emitted (also the space of velocity and force modules).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulationSpace {
    /// Particles move with the object.
    #[default]
    Local,
    /// Particles stay where they were emitted.
    World,
}

impl SimulationSpace {
    pub const ALL: [Self; 2] = [Self::Local, Self::World];

    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::World => "World",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MainModule {
    /// Length of one cycle in seconds.
    pub duration: f32,
    pub looping: bool,
    /// Start as if one full cycle had already run (looping systems only).
    pub prewarm: bool,
    pub start_delay: MinMax,
    pub start_lifetime: MinMax,
    pub start_speed: MinMax,
    pub start_size: MinMax,
    /// Degrees around the facing axis.
    #[serde(skip_serializing_if = "is_default")]
    pub start_rotation: MinMax,
    pub start_color: MinMaxColor,
    /// Multiplier on the project's physics gravity.
    pub gravity_modifier: f32,
    pub simulation_space: SimulationSpace,
    pub play_on_awake: bool,
    pub max_particles: u32,
}

impl Default for MainModule {
    fn default() -> Self {
        Self {
            duration: 5.0,
            looping: true,
            prewarm: false,
            start_delay: MinMax::constant(0.0),
            start_lifetime: MinMax::constant(5.0),
            start_speed: MinMax::constant(5.0),
            start_size: MinMax::constant(1.0),
            start_rotation: MinMax::constant(0.0),
            start_color: MinMaxColor::default(),
            gravity_modifier: 0.0,
            simulation_space: SimulationSpace::Local,
            play_on_awake: true,
            max_particles: 1000,
        }
    }
}

/// A one-off batch of particles at a time in each cycle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Burst {
    /// Seconds into the cycle.
    pub time: f32,
    pub count: u32,
    /// How many times it fires per cycle (0 = every interval until the cycle ends).
    pub cycles: u32,
    /// Seconds between repeated cycles.
    pub interval: f32,
    /// 0..1 chance that each cycle fires.
    pub probability: f32,
}

impl Default for Burst {
    fn default() -> Self {
        Self {
            time: 0.0,
            count: 30,
            cycles: 1,
            interval: 0.01,
            probability: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmissionModule {
    pub enabled: bool,
    /// Particles per second (curves follow the cycle).
    pub rate_over_time: MinMax,
    pub bursts: Vec<Burst>,
}

impl Default for EmissionModule {
    fn default() -> Self {
        Self {
            enabled: true,
            rate_over_time: MinMax::constant(10.0),
            bursts: Vec::new(),
        }
    }
}

/// Volume particles are emitted from; the emission axis is the object's local +Z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Sphere,
    Hemisphere,
    #[default]
    Cone,
    Box,
}

impl ShapeKind {
    pub const ALL: [Self; 4] = [Self::Sphere, Self::Hemisphere, Self::Cone, Self::Box];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "Sphere",
            Self::Hemisphere => "Hemisphere",
            Self::Cone => "Cone",
            Self::Box => "Box",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeModule {
    pub enabled: bool,
    pub shape: ShapeKind,
    /// Cone half-angle in degrees.
    pub angle: f32,
    /// Sphere/hemisphere radius, cone base radius.
    pub radius: f32,
    /// Box size.
    pub box_size: Vec3,
}

impl Default for ShapeModule {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: ShapeKind::Cone,
            angle: 25.0,
            radius: 1.0,
            box_size: Vec3::ONE,
        }
    }
}

/// Colour key (time 0..1, sRGB colour) of a gradient.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorKey {
    pub time: f32,
    pub color: [f32; 3],
}

/// Alpha key (time 0..1, alpha) of a gradient.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlphaKey {
    pub time: f32,
    pub alpha: f32,
}

/// Colour and alpha keys blended linearly, like the gradient editor's two key rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gradient {
    pub color_keys: Vec<ColorKey>,
    pub alpha_keys: Vec<AlphaKey>,
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            color_keys: vec![
                ColorKey {
                    time: 0.0,
                    color: [1.0; 3],
                },
                ColorKey {
                    time: 1.0,
                    color: [1.0; 3],
                },
            ],
            alpha_keys: vec![
                AlphaKey {
                    time: 0.0,
                    alpha: 1.0,
                },
                AlphaKey {
                    time: 1.0,
                    alpha: 1.0,
                },
            ],
        }
    }
}

/// Linear interpolation over `(time, value)` keys sorted by time; clamps outside the keys.
fn sample_keys<const N: usize>(keys: impl Iterator<Item = (f32, [f32; N])>, t: f32) -> [f32; N] {
    let mut prev: Option<(f32, [f32; N])> = None;
    for (kt, v) in keys {
        if t <= kt {
            return match prev {
                Some((pt, pv)) if kt > pt => {
                    let f = (t - pt) / (kt - pt);
                    std::array::from_fn(|i| pv[i] + (v[i] - pv[i]) * f)
                }
                _ => v,
            };
        }
        prev = Some((kt, v));
    }
    prev.map(|(_, v)| v).unwrap_or([1.0; N])
}

impl Gradient {
    /// sRGB colour + alpha at `t` (0..1).
    pub fn evaluate(&self, t: f32) -> [f32; 4] {
        let c = sample_keys(self.color_keys.iter().map(|k| (k.time, k.color)), t);
        let [a] = sample_keys(self.alpha_keys.iter().map(|k| (k.time, [k.alpha])), t);
        [c[0], c[1], c[2], a]
    }

    /// Keys kept sorted by time (after editing).
    pub fn sort(&mut self) {
        self.color_keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.alpha_keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }

    /// A gradient from colour keys and alpha keys.
    pub fn from_keys(colors: &[(f32, [f32; 3])], alphas: &[(f32, f32)]) -> Self {
        Self {
            color_keys: colors
                .iter()
                .map(|&(time, color)| ColorKey { time, color })
                .collect(),
            alpha_keys: alphas
                .iter()
                .map(|&(time, alpha)| AlphaKey { time, alpha })
                .collect(),
        }
    }
}

/// A velocity added over each particle's life (x, y, z curves) and a speed multiplier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VelocityOverLifetime {
    pub enabled: bool,
    pub x: MinMax,
    pub y: MinMax,
    pub z: MinMax,
    pub space: SimulationSpace,
    /// Multiplies the particle's speed (1 = unchanged).
    pub speed_modifier: MinMax,
}

impl Default for VelocityOverLifetime {
    fn default() -> Self {
        Self {
            enabled: false,
            x: MinMax::constant(0.0),
            y: MinMax::constant(0.0),
            z: MinMax::constant(0.0),
            space: SimulationSpace::Local,
            speed_modifier: MinMax::constant(1.0),
        }
    }
}

/// Slows particles above a speed limit (and an air drag).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimitVelocityOverLifetime {
    pub enabled: bool,
    pub limit: MinMax,
    /// Fraction of the excess speed removed per step (at 30 steps per second).
    pub dampen: f32,
    /// Linear drag (per second).
    pub drag: MinMax,
}

impl Default for LimitVelocityOverLifetime {
    fn default() -> Self {
        Self {
            enabled: false,
            limit: MinMax::constant(1.0),
            dampen: 1.0,
            drag: MinMax::constant(0.0),
        }
    }
}

/// An acceleration over each particle's life (x, y, z curves).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForceOverLifetime {
    pub enabled: bool,
    pub x: MinMax,
    pub y: MinMax,
    pub z: MinMax,
    pub space: SimulationSpace,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorOverLifetime {
    pub enabled: bool,
    pub gradient: Gradient,
    /// Random Between Two Gradients (`gradient` .. `gradient_b`, picked per particle).
    #[serde(skip_serializing_if = "is_false")]
    pub random: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub gradient_b: Gradient,
}

impl ColorOverLifetime {
    pub fn evaluate(&self, t: f32, r: f32) -> [f32; 4] {
        let a = self.gradient.evaluate(t);
        if !self.random {
            return a;
        }
        let b = self.gradient_b.evaluate(t);
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * r)
    }
}

/// Colour by the particle's speed over a speed range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorBySpeed {
    pub enabled: bool,
    pub gradient: Gradient,
    pub range: [f32; 2],
}

impl Default for ColorBySpeed {
    fn default() -> Self {
        Self {
            enabled: false,
            gradient: Gradient::default(),
            range: [0.0, 1.0],
        }
    }
}

/// Size multiplier over each particle's life: a curve (or the original start → end line).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SizeOverLifetime {
    pub enabled: bool,
    /// The start → end line of older files (used while `size` is unset).
    pub start: f32,
    pub end: f32,
    /// The size curve (Curve / Random Between Two Curves / constants).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<MinMax>,
}

impl Default for SizeOverLifetime {
    fn default() -> Self {
        Self {
            enabled: false,
            start: 0.0,
            end: 1.0,
            size: None,
        }
    }
}

impl SizeOverLifetime {
    /// The size multiplier as a curve (the start → end line when no curve was set).
    pub fn curve(&self) -> MinMax {
        self.size
            .clone()
            .unwrap_or_else(|| MinMax::curve(1.0, Curve::linear(self.start, self.end)))
    }
}

/// Size multiplier by speed over a speed range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SizeBySpeed {
    pub enabled: bool,
    pub size: MinMax,
    pub range: [f32; 2],
}

impl Default for SizeBySpeed {
    fn default() -> Self {
        Self {
            enabled: false,
            size: MinMax::curve(1.0, Curve::linear(0.0, 1.0)),
            range: [0.0, 1.0],
        }
    }
}

/// Spin over each particle's life (degrees per second).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RotationOverLifetime {
    pub enabled: bool,
    pub angular_velocity: MinMax,
}

impl Default for RotationOverLifetime {
    fn default() -> Self {
        Self {
            enabled: false,
            angular_velocity: MinMax::constant(45.0),
        }
    }
}

/// Spin by speed (degrees per second over a speed range).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RotationBySpeed {
    pub enabled: bool,
    pub angular_velocity: MinMax,
    pub range: [f32; 2],
}

impl Default for RotationBySpeed {
    fn default() -> Self {
        Self {
            enabled: false,
            angular_velocity: MinMax::constant(45.0),
            range: [0.0, 1.0],
        }
    }
}

/// Turbulent movement from a scrolling noise field (fractal value noise per axis).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseModule {
    pub enabled: bool,
    /// Offset per second at full noise (a curve over the particle's life).
    pub strength: MinMax,
    /// Spatial frequency (higher = smaller swirls).
    pub frequency: f32,
    /// How fast the field moves (per second).
    pub scroll_speed: f32,
    /// Strength scales with 1 / frequency, so changing the frequency keeps the look.
    pub damping: bool,
    /// Noise layers (1..4), each `octave_scale` × finer and `octave_multiplier` × weaker.
    pub octaves: u32,
    pub octave_multiplier: f32,
    pub octave_scale: f32,
    /// How much of the noise moves particles, turns them (degrees / s) and scales them.
    pub position_amount: f32,
    pub rotation_amount: f32,
    pub size_amount: f32,
}

impl Default for NoiseModule {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: MinMax::constant(1.0),
            frequency: 0.5,
            scroll_speed: 0.0,
            damping: true,
            octaves: 1,
            octave_multiplier: 0.5,
            octave_scale: 2.0,
            position_amount: 1.0,
            rotation_amount: 0.0,
            size_amount: 0.0,
        }
    }
}

/// What particles collide with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollisionType {
    /// Infinite planes listed in the module (world space, one-sided).
    #[default]
    Planes,
    /// The scene's colliders (box, sphere, capsule, mesh bounds).
    World,
}

impl CollisionType {
    pub const ALL: [Self; 2] = [Self::Planes, Self::World];

    pub fn label(self) -> &'static str {
        match self {
            Self::Planes => "Planes",
            Self::World => "World",
        }
    }
}

/// A collision plane: a point and the normal of its solid side's surface (world space).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollisionPlane {
    pub point: Vec3,
    pub normal: Vec3,
}

impl Default for CollisionPlane {
    fn default() -> Self {
        Self {
            point: Vec3::ZERO,
            normal: Vec3::Y,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollisionModule {
    pub enabled: bool,
    pub kind: CollisionType,
    pub planes: Vec<CollisionPlane>,
    /// Fraction of speed lost at each collision.
    pub dampen: f32,
    /// Fraction of the speed into the surface that bounces back.
    pub bounce: f32,
    /// Fraction of the lifetime lost at each collision.
    pub lifetime_loss: f32,
    /// Particles slower than this after a collision die.
    pub min_kill_speed: f32,
    /// Particles faster than this after a collision die.
    pub max_kill_speed: f32,
    /// Collision radius = size × this / 2.
    pub radius_scale: f32,
    /// Layers of colliders hit (World).
    pub collides_with: u32,
}

impl Default for CollisionModule {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: CollisionType::Planes,
            planes: vec![CollisionPlane::default()],
            dampen: 0.0,
            bounce: 1.0,
            lifetime_loss: 0.0,
            min_kill_speed: 0.0,
            max_kill_speed: 10_000.0,
            radius_scale: 1.0,
            collides_with: u32::MAX,
        }
    }
}

/// When a sub emitter emits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubEmitterTrigger {
    /// When a particle is born.
    #[default]
    Birth,
    /// When a particle collides (Collision module).
    Collision,
    /// When a particle dies.
    Death,
}

impl SubEmitterTrigger {
    pub const ALL: [Self; 3] = [Self::Birth, Self::Collision, Self::Death];

    pub fn label(self) -> &'static str {
        match self {
            Self::Birth => "Birth",
            Self::Collision => "Collision",
            Self::Death => "Death",
        }
    }
}

/// Another Particle System (a child object, by name) that emits where particles of this one
/// are born, collide or die.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubEmitter {
    pub trigger: SubEmitterTrigger,
    /// Name of the child object with the sub emitter's Particle System.
    pub emitter: String,
    /// Particles emitted per trigger.
    pub count: u32,
    /// 0..1 chance per trigger.
    pub probability: f32,
    /// Multiply the new particles' colour / size by the parent particle's.
    pub inherit_color: bool,
    pub inherit_size: bool,
    /// Fraction of the parent particle's velocity added to the new particles.
    pub inherit_velocity: f32,
}

impl Default for SubEmitter {
    fn default() -> Self {
        Self {
            trigger: SubEmitterTrigger::Death,
            emitter: String::new(),
            count: 10,
            probability: 1.0,
            inherit_color: false,
            inherit_size: false,
            inherit_velocity: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubEmittersModule {
    pub enabled: bool,
    pub list: Vec<SubEmitter>,
}

/// Which frames of the sheet a particle plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetAnimation {
    /// Every tile, row by row.
    #[default]
    WholeSheet,
    /// One row (a fixed one, or a random row per particle).
    SingleRow,
}

impl SheetAnimation {
    pub const ALL: [Self; 2] = [Self::WholeSheet, Self::SingleRow];

    pub fn label(self) -> &'static str {
        match self {
            Self::WholeSheet => "Whole Sheet",
            Self::SingleRow => "Single Row",
        }
    }
}

/// Flipbook animation of a texture split into tiles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextureSheetAnimation {
    pub enabled: bool,
    pub tiles_x: u32,
    pub tiles_y: u32,
    pub animation: SheetAnimation,
    pub random_row: bool,
    pub row: u32,
    /// 0..1 through the frames over the particle's life.
    pub frame_over_time: MinMax,
    /// First frame (in frames).
    pub start_frame: MinMax,
    /// Times the frames play over the life.
    pub cycles: f32,
}

impl Default for TextureSheetAnimation {
    fn default() -> Self {
        Self {
            enabled: false,
            tiles_x: 1,
            tiles_y: 1,
            animation: SheetAnimation::WholeSheet,
            random_row: true,
            row: 0,
            frame_over_time: MinMax::curve(1.0, Curve::linear(0.0, 1.0)),
            start_frame: MinMax::constant(0.0),
            cycles: 1.0,
        }
    }
}

impl TextureSheetAnimation {
    /// UV rectangle (offset x, offset y, scale x, scale y) of the frame at life `t` for the
    /// particle's random picks `r` (frame over time / start frame / row).
    pub fn uv(&self, t: f32, r: [f32; 3]) -> [f32; 4] {
        let (cols, rows) = (self.tiles_x.max(1), self.tiles_y.max(1));
        let (frames, row) = match self.animation {
            SheetAnimation::WholeSheet => (cols * rows, None),
            SheetAnimation::SingleRow => {
                let row = if self.random_row {
                    ((r[2] * rows as f32) as u32).min(rows - 1)
                } else {
                    self.row.min(rows - 1)
                };
                (cols, Some(row))
            }
        };
        let f = self.frame_over_time.evaluate(t, r[0]).clamp(0.0, 1.0) * self.cycles.max(0.0);
        let start = self.start_frame.evaluate(t, r[1]).max(0.0);
        let mut frame = (f * frames as f32).floor();
        if f > 0.0 && (f * frames as f32).fract() == 0.0 && f.fract() == 0.0 {
            // The last moment of a cycle shows the last frame, not the first.
            frame -= 1.0;
        }
        let frame = ((frame + start.floor()) as i64).rem_euclid(frames as i64) as u32;
        let (col, row) = match row {
            Some(r) => (frame, r),
            None => (frame % cols, frame / cols),
        };
        let (w, h) = (1.0 / cols as f32, 1.0 / rows as f32);
        [col as f32 * w, row as f32 * h, w, h]
    }
}

/// Trails: a ribbon behind each particle, or one ribbon through all particles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrailMode {
    #[default]
    Particles,
    Ribbon,
}

impl TrailMode {
    pub const ALL: [Self; 2] = [Self::Particles, Self::Ribbon];

    pub fn label(self) -> &'static str {
        match self {
            Self::Particles => "Particles",
            Self::Ribbon => "Ribbon",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrailsModule {
    pub enabled: bool,
    pub mode: TrailMode,
    /// Fraction of particles that get a trail.
    pub ratio: f32,
    /// Seconds a trail point lasts (Particles mode), as a fraction of the particle's life.
    pub lifetime: MinMax,
    /// A new point once the particle moved this far.
    pub min_vertex_distance: f32,
    /// Trails stay in the world when the object moves (else they follow it).
    pub world_space: bool,
    /// Remove a trail when its particle dies (else it fades out).
    pub die_with_particles: bool,
    /// Width = particle size × this curve along the trail (0 = head, 1 = tail).
    pub width_over_trail: MinMax,
    /// Colour along the trail (multiplied with the particle colour when inherited).
    pub color_over_trail: Gradient,
    /// Colour over the particle's life.
    pub color_over_lifetime: Gradient,
    pub inherit_particle_color: bool,
    pub size_affects_width: bool,
    /// Texture of the trail (None = a soft strip).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

impl Default for TrailsModule {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: TrailMode::Particles,
            ratio: 1.0,
            lifetime: MinMax::constant(0.5),
            min_vertex_distance: 0.1,
            world_space: true,
            die_with_particles: true,
            width_over_trail: MinMax::curve(1.0, Curve::linear(1.0, 0.0)),
            color_over_trail: Gradient::from_keys(
                &[(0.0, [1.0; 3]), (1.0, [1.0; 3])],
                &[(0.0, 1.0), (1.0, 0.0)],
            ),
            color_over_lifetime: Gradient::default(),
            inherit_particle_color: true,
            size_affects_width: true,
            texture: None,
        }
    }
}

/// How particle colours combine with what is behind them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticleBlend {
    /// Transparent, sorted back to front.
    #[default]
    Alpha,
    /// Adds light (fire, sparks, magic); order doesn't matter.
    Additive,
}

impl ParticleBlend {
    pub const ALL: [Self; 2] = [Self::Alpha, Self::Additive];

    pub fn label(self) -> &'static str {
        match self {
            Self::Alpha => "Alpha Blended",
            Self::Additive => "Additive",
        }
    }
}

/// How billboards face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderMode {
    /// Facing the camera.
    #[default]
    Billboard,
    /// Stretched along the velocity.
    Stretched,
    /// Lying flat (facing world up).
    Horizontal,
    /// Upright, turning to the camera around world up.
    Vertical,
}

impl RenderMode {
    pub const ALL: [Self; 4] = [
        Self::Billboard,
        Self::Stretched,
        Self::Horizontal,
        Self::Vertical,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Billboard => "Billboard",
            Self::Stretched => "Stretched Billboard",
            Self::Horizontal => "Horizontal Billboard",
            Self::Vertical => "Vertical Billboard",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RendererModule {
    pub enabled: bool,
    /// Project-relative texture; `None` = the built-in soft round particle.
    pub texture: Option<String>,
    pub blend: ParticleBlend,
    /// HDR brightness multiplier: above 1 the particles glow through bloom.
    pub intensity: f32,
    #[serde(skip_serializing_if = "is_default")]
    pub render_mode: RenderMode,
    /// Stretched Billboard: extra length per unit of speed, and length × size.
    #[serde(skip_serializing_if = "is_default")]
    pub speed_scale: f32,
    #[serde(skip_serializing_if = "is_one")]
    pub length_scale: f32,
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

impl Default for RendererModule {
    fn default() -> Self {
        Self {
            enabled: true,
            texture: None,
            blend: ParticleBlend::Alpha,
            intensity: 1.0,
            render_mode: RenderMode::Billboard,
            speed_scale: 0.0,
            length_scale: 1.0,
        }
    }
}

/// Emits and simulates camera-facing particles. Runtime state lives in
/// [`ParticleEmitter`](super::ParticleEmitter), which is never saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleSystem {
    pub main: MainModule,
    pub emission: EmissionModule,
    pub shape: ShapeModule,
    #[serde(skip_serializing_if = "is_default")]
    pub velocity_over_lifetime: VelocityOverLifetime,
    #[serde(skip_serializing_if = "is_default")]
    pub limit_velocity_over_lifetime: LimitVelocityOverLifetime,
    #[serde(skip_serializing_if = "is_default")]
    pub force_over_lifetime: ForceOverLifetime,
    pub color_over_lifetime: ColorOverLifetime,
    #[serde(skip_serializing_if = "is_default")]
    pub color_by_speed: ColorBySpeed,
    pub size_over_lifetime: SizeOverLifetime,
    #[serde(skip_serializing_if = "is_default")]
    pub size_by_speed: SizeBySpeed,
    #[serde(skip_serializing_if = "is_default")]
    pub rotation_over_lifetime: RotationOverLifetime,
    #[serde(skip_serializing_if = "is_default")]
    pub rotation_by_speed: RotationBySpeed,
    #[serde(skip_serializing_if = "is_default")]
    pub noise: NoiseModule,
    #[serde(skip_serializing_if = "is_default")]
    pub collision: CollisionModule,
    #[serde(skip_serializing_if = "is_default")]
    pub sub_emitters: SubEmittersModule,
    #[serde(skip_serializing_if = "is_default")]
    pub texture_sheet_animation: TextureSheetAnimation,
    #[serde(skip_serializing_if = "is_default")]
    pub trails: TrailsModule,
    pub renderer: RendererModule,
    /// Random seed; 0 = a different seed each time the system plays.
    pub seed: u32,
}
