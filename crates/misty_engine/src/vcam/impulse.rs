//! Impulses: one-off camera shakes. An Impulse Source sends an impulse (from a script, a
//! collision or a timeline Control clip); it spreads from where it was made, and every virtual
//! camera with an Impulse Listener in range shakes by it. The shake is the source's signal
//! shape times an attack / sustain / decay envelope, scaled by the impulse's velocity and by
//! distance (full inside the impact radius, fading to nothing over the dissipation distance).

use glam::{Quat, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::components::{ImpulseListener, Noise, NoiseProfile};
use super::noise;
use crate::camera::euler_to_quat;
use crate::scene::{Scene, SceneComponent};

/// The raw movement an impulse makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImpulseShape {
    /// One push along the velocity and back.
    #[default]
    Bump,
    /// A sharp kick that rings out: along the velocity, with a little random sideways jolt.
    Explosion,
    /// Fast random shaking in every direction.
    Rumble,
}

impl ImpulseShape {
    pub const ALL: [ImpulseShape; 3] = [Self::Bump, Self::Explosion, Self::Rumble];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bump => "Bump",
            Self::Explosion => "Explosion",
            Self::Rumble => "Rumble",
        }
    }
}

/// What makes an Impulse Source fire on its own (scripts and timelines can always fire it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImpulseTrigger {
    /// Only `GenerateImpulse` (scripts, signals, Control clips).
    #[default]
    Manual,
    /// Also when its collider starts touching another (on the Collide With layers).
    CollisionEnter,
}

impl ImpulseTrigger {
    pub const ALL: [ImpulseTrigger; 2] = [Self::Manual, Self::CollisionEnter];

    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::CollisionEnter => "Collision Enter",
        }
    }
}

/// Impulse Source component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImpulseSource {
    pub enabled: bool,
    pub shape: ImpulseShape,
    /// Scales the shake.
    pub amplitude_gain: f32,
    /// Speeds the shake up (Explosion and Rumble).
    pub frequency_gain: f32,
    /// Direction and strength used when no velocity is given (metres at full envelope).
    pub default_velocity: Vec3,
    /// Seconds to rise to full strength.
    pub attack: f32,
    /// Seconds at full strength.
    pub sustain: f32,
    /// Seconds to fade out.
    pub decay: f32,
    /// Listeners within this distance feel the full impulse.
    pub impact_radius: f32,
    /// Beyond the impact radius the impulse fades to nothing over this distance.
    pub dissipation_distance: f32,
    /// Channels (bits) it is sent on; listeners pick the channels they hear.
    pub channel: u32,
    pub trigger: ImpulseTrigger,
    /// Collision Enter: layers of the other collider that fire it.
    pub collide_with: u32,
}

impl Default for ImpulseSource {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: ImpulseShape::Bump,
            amplitude_gain: 1.0,
            frequency_gain: 1.0,
            default_velocity: Vec3::new(0.0, -0.5, 0.0),
            attack: 0.05,
            sustain: 0.1,
            decay: 0.6,
            impact_radius: 50.0,
            dissipation_distance: 100.0,
            channel: 1,
            trigger: ImpulseTrigger::Manual,
            collide_with: u32::MAX,
        }
    }
}

impl SceneComponent for ImpulseSource {
    const NAME: &'static str = "Impulse Source";
    const CATEGORY: &'static str = "Rendering";
}

/// Envelope strength `t` seconds after an impulse starts: rises over `attack`, holds for
/// `sustain`, then falls to exactly 0 over `decay` (smoothly at both ends).
pub fn envelope(attack: f32, sustain: f32, decay: f32, t: f32) -> f32 {
    let s = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    if t < 0.0 {
        0.0
    } else if t < attack {
        s(t / attack)
    } else if t < attack + sustain {
        1.0
    } else if decay > 0.0 {
        1.0 - s((t - attack - sustain) / decay)
    } else {
        0.0
    }
}

/// Distance falloff: 1 inside `radius`, fading to 0 over `dissipation` beyond it.
pub fn falloff(distance: f32, radius: f32, dissipation: f32) -> f32 {
    if distance <= radius {
        1.0
    } else if dissipation <= 1e-4 {
        0.0
    } else {
        let x = ((distance - radius) / dissipation).clamp(0.0, 1.0);
        1.0 - x * x * (3.0 - 2.0 * x)
    }
}

/// One impulse travelling through the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImpulseEvent {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Rig time it started at.
    pub start: f32,
    pub shape: ImpulseShape,
    pub frequency_gain: f32,
    pub attack: f32,
    pub sustain: f32,
    pub decay: f32,
    pub radius: f32,
    pub dissipation: f32,
    pub channel: u32,
    pub seed: u32,
}

impl ImpulseEvent {
    /// An impulse from `source`'s settings at `position` with `velocity` (already scaled by
    /// the amplitude gain).
    pub fn from_source(
        source: &ImpulseSource,
        position: Vec3,
        velocity: Vec3,
        start: f32,
        seed: u32,
    ) -> Self {
        Self {
            position,
            velocity: velocity * source.amplitude_gain,
            start,
            shape: source.shape,
            frequency_gain: source.frequency_gain,
            attack: source.attack.max(0.0),
            sustain: source.sustain.max(0.0),
            decay: source.decay.max(0.0),
            radius: source.impact_radius.max(0.0),
            dissipation: source.dissipation_distance.max(0.0),
            channel: source.channel,
            seed,
        }
    }

    /// Seconds from its start until it has died away.
    pub fn length(&self) -> f32 {
        self.attack + self.sustain + self.decay
    }

    /// Position (world) and rotation offsets it gives a listener at `at` at rig time `now`.
    pub fn signal(&self, at: Vec3, use_2d: bool, now: f32) -> (Vec3, Quat) {
        let t = now - self.start;
        let env = envelope(self.attack, self.sustain, self.decay, t);
        if env <= 0.0 {
            return (Vec3::ZERO, Quat::IDENTITY);
        }
        let mut d = at - self.position;
        if use_2d {
            d.y = 0.0;
        }
        let fall = falloff(d.length(), self.radius, self.dissipation);
        if fall <= 0.0 {
            return (Vec3::ZERO, Quat::IDENTITY);
        }
        let k = env * fall;
        let strength = self.velocity.length();
        let ft = t * self.frequency_gain.max(0.0);
        match self.shape {
            ImpulseShape::Bump => (self.velocity * k, Quat::IDENTITY),
            ImpulseShape::Explosion => {
                // A ringing kick: the push oscillates (decaying with the envelope).
                let ring = (ft * 18.0).cos();
                let side = Vec3::new(
                    noise::perlin(ft * 9.0, self.seed),
                    noise::perlin(ft * 9.0 + 3.1, self.seed ^ 0x5bd1_e995),
                    noise::perlin(ft * 9.0 + 7.3, self.seed ^ 0x1b87_3593),
                ) * strength
                    * 0.35;
                let off = (self.velocity * ring + side) * k;
                let rot = Vec3::new(off.y, off.x, off.x * 0.5) * 4.0;
                (off, euler_to_quat(rot))
            }
            ImpulseShape::Rumble => {
                let n = Noise {
                    enabled: true,
                    profile: NoiseProfile::Shake,
                    amplitude_gain: 1.0,
                    frequency_gain: self.frequency_gain,
                    seed: self.seed,
                };
                let (dp, dr) = noise::sample(&n, t);
                // The Shake profile is about 0.07 m at gain 1: scale to the velocity.
                let g = k * strength / 0.07;
                (dp * g, euler_to_quat(dr * g * 0.5))
            }
        }
    }
}

/// Every live impulse, and the clock they run on (advanced by the camera rig).
#[derive(Clone, Debug, Default)]
pub struct ImpulseManager {
    pub time: f32,
    pub events: Vec<ImpulseEvent>,
    next_seed: u32,
}

impl ImpulseManager {
    /// Advance the clock and forget impulses that have died away.
    pub fn advance(&mut self, dt: f32) {
        self.time += dt.max(0.0);
        let now = self.time;
        self.events.retain(|e| now - e.start <= e.length());
    }

    /// Start an impulse with `source`'s settings at `position`.
    pub fn add(&mut self, source: &ImpulseSource, position: Vec3, velocity: Vec3) {
        self.next_seed = self.next_seed.wrapping_add(1);
        let ev = ImpulseEvent::from_source(
            source,
            position,
            velocity,
            self.time,
            self.next_seed.wrapping_mul(0x9e37_79b9),
        );
        self.events.push(ev);
    }

    /// Fire the Impulse Source on `e` (its default velocity × `force` when no velocity is
    /// given). Returns false when it has no enabled source.
    pub fn generate(
        &mut self,
        scene: &Scene,
        e: Entity,
        velocity: Option<Vec3>,
        force: f32,
    ) -> bool {
        let Some(src) = scene
            .world
            .get::<&ImpulseSource>(e)
            .ok()
            .map(|s| (*s).clone())
            .filter(|s| s.enabled)
        else {
            return false;
        };
        let v = velocity.unwrap_or(src.default_velocity * force);
        self.add(&src, scene.world_position(e), v);
        true
    }

    /// Total offsets for a listener at `at`.
    pub fn offset(&self, listener: &ImpulseListener, at: Vec3) -> (Vec3, Quat) {
        let mut pos = Vec3::ZERO;
        let mut rot = Quat::IDENTITY;
        for e in self
            .events
            .iter()
            .filter(|e| e.channel & listener.channel_mask != 0)
        {
            let (p, r) = e.signal(at, listener.use_2d_distance, self.time);
            pos += p;
            rot *= r;
        }
        let g = listener.gain;
        (pos * g, Quat::IDENTITY.slerp(rot, g.clamp(0.0, 4.0)))
    }

    /// Collision-triggered sources: fire those whose collider started touching something on
    /// their Collide With layers.
    pub fn on_contacts(&mut self, scene: &Scene, contacts: &[crate::physics::Contact]) {
        for c in contacts.iter().filter(|c| c.started && !c.trigger) {
            for (me, other) in [(c.a, c.b), (c.b, c.a)] {
                let fires = scene.contains(me)
                    && scene.contains(other)
                    && scene.world.get::<&ImpulseSource>(me).is_ok_and(|s| {
                        s.enabled
                            && s.trigger == ImpulseTrigger::CollisionEnter
                            && s.collide_with & (1u32 << scene.game_object(other).layer.min(31))
                                != 0
                    });
                if fires {
                    self.generate(scene, me, None, 1.0);
                }
            }
        }
    }
}
