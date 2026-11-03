//! The Particle System component: serialised settings split into the familiar modules
//! (Main, Emission, Shape, Color over Lifetime, Size over Lifetime, Renderer).

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::rng::Rng;

/// A number that is either a constant (`a`) or random between two constants (`a`..`b`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MinMax {
    pub a: f32,
    pub b: f32,
    pub random: bool,
}

impl Default for MinMax {
    fn default() -> Self {
        Self::constant(0.0)
    }
}

impl MinMax {
    pub const fn constant(v: f32) -> Self {
        Self {
            a: v,
            b: v,
            random: false,
        }
    }

    pub const fn between(a: f32, b: f32) -> Self {
        Self { a, b, random: true }
    }

    pub fn sample(&self, rng: &mut Rng) -> f32 {
        if self.random {
            self.a + (self.b - self.a) * rng.next_f32()
        } else {
            self.a
        }
    }

    /// Largest value it can produce (for gizmos and bounds).
    pub fn max(&self) -> f32 {
        if self.random {
            self.a.max(self.b)
        } else {
            self.a
        }
    }
}

/// A colour (sRGB + alpha) that is constant or random between two colours.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MinMaxColor {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub random: bool,
}

impl Default for MinMaxColor {
    fn default() -> Self {
        Self {
            a: [1.0; 4],
            b: [1.0; 4],
            random: false,
        }
    }
}

impl MinMaxColor {
    pub fn sample(&self, rng: &mut Rng) -> [f32; 4] {
        if !self.random {
            return self.a;
        }
        let t = rng.next_f32();
        std::array::from_fn(|i| self.a[i] + (self.b[i] - self.a[i]) * t)
    }
}

/// Where particles live once emitted.
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
    /// Particles per second.
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
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorOverLifetime {
    pub enabled: bool,
    pub gradient: Gradient,
}

/// Size multiplier going linearly from `start` to `end` over each particle's life.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SizeOverLifetime {
    pub enabled: bool,
    pub start: f32,
    pub end: f32,
}

impl Default for SizeOverLifetime {
    fn default() -> Self {
        Self {
            enabled: false,
            start: 0.0,
            end: 1.0,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RendererModule {
    pub enabled: bool,
    /// Project-relative texture; `None` = the built-in soft round particle.
    pub texture: Option<String>,
    pub blend: ParticleBlend,
    /// HDR brightness multiplier: above 1 the particles glow through bloom.
    pub intensity: f32,
}

impl Default for RendererModule {
    fn default() -> Self {
        Self {
            enabled: true,
            texture: None,
            blend: ParticleBlend::Alpha,
            intensity: 1.0,
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
    pub color_over_lifetime: ColorOverLifetime,
    pub size_over_lifetime: SizeOverLifetime,
    pub renderer: RendererModule,
    /// Random seed; 0 = a different seed each time the system plays.
    pub seed: u32,
}
