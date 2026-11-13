//! Runtime state of one Particle System and its rules: emission (rate + bursts), aging,
//! gravity, the velocity / force / limit modules, noise, rotation, collisions, trails and the
//! events sub emitters listen to. Pure: no scene, no GPU, deterministic for a given seed.

use std::collections::{BTreeMap, VecDeque};

use glam::{Mat4, Vec3};

use super::component::{
    CollisionType, ParticleSystem, SimulationSpace, SubEmitterTrigger, TrailMode,
};
use super::noise;
use super::rng::{self, Rng};
use super::shape;

/// Step used to simulate Prewarm.
const PREWARM_STEP: f32 = 1.0 / 30.0;
/// Longest Prewarm run (seconds of simulation inside Play).
const MAX_PREWARM: f32 = 300.0;

/// Salts of the per-particle random numbers (one per module term).
pub mod salt {
    pub const VELOCITY: u32 = 1;
    pub const FORCE: u32 = 4;
    pub const LIMIT: u32 = 7;
    pub const SPEED_MOD: u32 = 8;
    pub const COLOR: u32 = 9;
    pub const SIZE: u32 = 10;
    pub const ROTATION: u32 = 11;
    pub const NOISE: u32 = 12;
    pub const SHEET: u32 = 13;
    pub const TRAIL: u32 = 16;
    pub const TRAIL_LIFE: u32 = 17;
    pub const SIZE_SPEED: u32 = 18;
    pub const ROT_SPEED: u32 = 19;
    pub const DRAG: u32 = 20;
}

/// One live particle. Position/velocity are in world space or the emitter's local space,
/// depending on the system's Simulation Space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Seconds since it was emitted.
    pub age: f32,
    pub lifetime: f32,
    /// Start size (modules multiply it when drawn).
    pub size: f32,
    /// sRGB colour + alpha picked at birth.
    pub color: [f32; 4],
    /// Degrees around the facing axis.
    pub rotation: f32,
    /// Per-particle random numbers come from this.
    pub seed: u32,
    /// Birth order (ribbons and trails follow it).
    pub id: u32,
    /// Size multiplier of the Noise module (1 = none).
    pub noise_size: f32,
}

impl Particle {
    /// 0 at birth, 1 at death.
    pub fn life_fraction(&self) -> f32 {
        (self.age / self.lifetime.max(1e-6)).clamp(0.0, 1.0)
    }

    /// A random number in [0, 1) fixed for this particle and `salt`.
    pub fn rand(&self, salt: u32) -> f32 {
        let mut h = self.seed ^ salt.wrapping_mul(0x9e37_79b9);
        h ^= h >> 16;
        h = h.wrapping_mul(0x7feb_352d);
        h ^= h >> 15;
        h = h.wrapping_mul(0x846c_a68b);
        h ^= h >> 16;
        (h >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

/// A collider particles bounce off (world space), with its object's layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderShape {
    Sphere {
        center: Vec3,
        radius: f32,
    },
    /// An oriented box: unit axes and half extents along them.
    Box {
        center: Vec3,
        axes: [Vec3; 3],
        half: Vec3,
    },
    /// A segment `a`–`b` grown by `radius`.
    Capsule {
        a: Vec3,
        b: Vec3,
        radius: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collider {
    pub shape: ColliderShape,
    pub layer: u8,
}

impl ColliderShape {
    /// Where a sphere of radius `r` moving from `from` to `to` first touches the shape (and the
    /// surface normal), so fast particles don't pass through thin boxes; else
    /// [`Self::push_out`] at `to`.
    pub fn sweep(&self, from: Vec3, to: Vec3, r: f32) -> Option<(Vec3, Vec3)> {
        if let Self::Box { center, axes, half } = *self {
            let h = half + Vec3::splat(r);
            let rel = from - center;
            let d = to - from;
            let o = Vec3::new(rel.dot(axes[0]), rel.dot(axes[1]), rel.dot(axes[2]));
            let dir = Vec3::new(d.dot(axes[0]), d.dot(axes[1]), d.dot(axes[2]));
            let outside = (0..3).any(|i| o[i].abs() > h[i]);
            if outside {
                let (mut enter, mut exit, mut axis) = (f32::MIN, f32::MAX, None);
                for i in 0..3 {
                    if dir[i].abs() < 1e-9 {
                        if o[i].abs() > h[i] {
                            return None;
                        }
                        continue;
                    }
                    let t1 = (-h[i] - o[i]) / dir[i];
                    let t2 = (h[i] - o[i]) / dir[i];
                    let (lo, hi) = (t1.min(t2), t1.max(t2));
                    if lo > enter {
                        enter = lo;
                        axis = Some(i);
                    }
                    exit = exit.min(hi);
                }
                return match axis {
                    Some(k) if enter <= exit && (0.0..=1.0).contains(&enter) => {
                        Some((from + d * enter, axes[k] * -dir[k].signum()))
                    }
                    _ => None,
                };
            }
        }
        self.push_out(to, r)
    }

    /// Where a sphere of radius `r` at `p` goes to leave the shape, and the surface normal.
    pub fn push_out(&self, p: Vec3, r: f32) -> Option<(Vec3, Vec3)> {
        match *self {
            Self::Sphere { center, radius } => {
                let d = p - center;
                let dist = d.length();
                if dist >= radius + r {
                    return None;
                }
                let n = if dist > 1e-6 { d / dist } else { Vec3::Y };
                Some((center + n * (radius + r), n))
            }
            Self::Capsule { a, b, radius } => {
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
                Self::Sphere {
                    center: a + ab * t,
                    radius,
                }
                .push_out(p, r)
            }
            Self::Box { center, axes, half } => {
                let d = p - center;
                let q = Vec3::new(d.dot(axes[0]), d.dot(axes[1]), d.dot(axes[2]));
                let pen = half + Vec3::splat(r) - q.abs();
                if pen.min_element() <= 0.0 {
                    return None;
                }
                let k = if pen.x <= pen.y && pen.x <= pen.z {
                    0
                } else if pen.y <= pen.z {
                    1
                } else {
                    2
                };
                let n = axes[k] * q[k].signum();
                Some((p + n * pen[k], n))
            }
        }
    }
}

/// A particle event sub emitters listen to (world space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubEmitEvent {
    pub trigger: SubEmitterTrigger,
    pub position: Vec3,
    pub velocity: Vec3,
    /// The particle's colour (sRGB + alpha, with Color over Lifetime) and size then.
    pub color: [f32; 4],
    pub size: f32,
}

/// What a step needs from the scene.
#[derive(Clone, Copy, Debug)]
pub struct StepEnv<'a> {
    pub gravity: Vec3,
    /// World colliders (Collision module, World type).
    pub colliders: &'a [Collider],
    /// False for sub emitters: they only emit when their parent triggers them.
    pub emit: bool,
}

impl<'a> StepEnv<'a> {
    pub fn new(gravity: Vec3) -> Self {
        Self {
            gravity,
            colliders: &[],
            emit: true,
        }
    }
}

/// A point of a trail (world space when the trail is, else simulation space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailPoint {
    pub position: Vec3,
    /// Emitter time it was added.
    pub time: f32,
}

/// The trail of one particle (kept after it dies until its points fade, unless the module
/// removes trails with their particles).
#[derive(Clone, Debug, PartialEq)]
pub struct Trail {
    pub points: VecDeque<TrailPoint>,
    /// Seconds a point lasts.
    pub point_life: f32,
    pub alive: bool,
    /// The particle when it died (colour, size) for fading trails.
    pub last: Option<Particle>,
}

/// Live particles and playback state (a runtime-only component; never saved or copied).
#[derive(Clone, Debug)]
pub struct ParticleEmitter {
    pub particles: Vec<Particle>,
    pub state: PlayState,
    /// Seconds into the current cycle.
    pub time: f32,
    /// Seconds simulated since the last Play/Restart (shown as Playback Time).
    pub total_time: f32,
    /// Seconds simulated since the emitter was made (trail point times).
    pub clock: f32,
    /// Trails by particle id.
    pub trails: BTreeMap<u32, Trail>,
    /// Events of the last step for sub emitters.
    pub events: Vec<SubEmitEvent>,
    /// Start Delay still to wait before emitting.
    delay_left: f32,
    /// Fractional particles carried between frames by Rate over Time.
    accumulator: f32,
    /// Cycles fired this loop, per burst.
    bursts_fired: Vec<u32>,
    /// True while the cycle is emitting (false after a Stop or when a non-looping cycle ends).
    emitting: bool,
    next_id: u32,
    rng: Rng,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            state: PlayState::Stopped,
            time: 0.0,
            total_time: 0.0,
            clock: 0.0,
            trails: BTreeMap::new(),
            events: Vec::new(),
            delay_left: 0.0,
            accumulator: 0.0,
            bursts_fired: Vec::new(),
            emitting: false,
            next_id: 0,
            rng: Rng::new(0),
        }
    }
}

/// What a sub emitter trigger adds to the new particles.
#[derive(Clone, Copy, Debug)]
pub struct Inherit {
    pub velocity: Vec3,
    pub color: Option<[f32; 4]>,
    pub size: Option<f32>,
}

impl ParticleEmitter {
    /// A stopped emitter; `play` starts it.
    pub fn new(system: &ParticleSystem) -> Self {
        let mut e = Self::default();
        e.reseed(system);
        e
    }

    fn reseed(&mut self, system: &ParticleSystem) {
        let seed = if system.seed == 0 {
            rng::fresh_seed()
        } else {
            system.seed as u64
        };
        self.rng = Rng::new(seed);
    }

    pub fn is_playing(&self) -> bool {
        self.state == PlayState::Playing
    }

    pub fn is_paused(&self) -> bool {
        self.state == PlayState::Paused
    }

    pub fn count(&self) -> usize {
        self.particles.len()
    }

    /// Play: resumes when paused, otherwise starts a new cycle (keeping live particles).
    pub fn play(&mut self, system: &ParticleSystem, world: &Mat4, gravity: Vec3) {
        match self.state {
            PlayState::Paused => self.state = PlayState::Playing,
            PlayState::Playing => {}
            PlayState::Stopped => self.start(system, world, gravity),
        }
    }

    /// Clear and start from the beginning (Restart in the Scene view overlay).
    pub fn restart(&mut self, system: &ParticleSystem, world: &Mat4, gravity: Vec3) {
        self.particles.clear();
        self.trails.clear();
        self.state = PlayState::Stopped;
        self.start(system, world, gravity);
    }

    fn start(&mut self, system: &ParticleSystem, world: &Mat4, gravity: Vec3) {
        self.reseed(system);
        self.state = PlayState::Playing;
        self.emitting = true;
        self.time = 0.0;
        self.total_time = 0.0;
        self.accumulator = 0.0;
        self.bursts_fired = vec![0; system.emission.bursts.len()];
        self.delay_left = system.main.start_delay.sample(&mut self.rng).max(0.0);
        if system.main.prewarm && system.main.looping {
            // Run one whole cycle (without delay) so the effect starts "full".
            self.delay_left = 0.0;
            // Counted steps: `t += 1/30` stops growing past 2^20 s (Duration 2e6 never
            // ended), and a huge Duration is capped.
            let duration = system.main.duration;
            let duration = if duration.is_finite() {
                duration.clamp(0.0, MAX_PREWARM)
            } else {
                0.0
            };
            let steps = (duration / PREWARM_STEP).ceil() as u32;
            for _ in 0..steps {
                self.step(system, world, gravity, PREWARM_STEP);
            }
            self.total_time = 0.0;
            self.events.clear();
        }
    }

    /// Stop emitting; live particles finish their lives.
    pub fn stop(&mut self) {
        self.state = PlayState::Stopped;
        self.emitting = false;
    }

    /// Freeze everything until Play.
    pub fn pause(&mut self) {
        if self.state == PlayState::Playing {
            self.state = PlayState::Paused;
        }
    }

    /// Remove every live particle (and trail).
    pub fn clear(&mut self) {
        self.particles.clear();
        self.trails.clear();
    }

    /// Emit `n` particles now, whatever the play state (up to Max Particles).
    pub fn emit(&mut self, system: &ParticleSystem, world: &Mat4, n: u32) {
        // No more tries than free slots: `Emit(4294967295)` spun 4 billion times.
        let free = (system.main.max_particles as usize).saturating_sub(self.particles.len());
        for _ in 0..(n as usize).min(free) {
            self.spawn(system, world, Vec3::ZERO, 0.0, None);
        }
    }

    /// Emit `n` particles from the shape placed at `at` (world space): a sub emitter firing.
    pub fn emit_at(
        &mut self,
        system: &ParticleSystem,
        world: &Mat4,
        at: Vec3,
        n: u32,
        inherit: Inherit,
    ) {
        let free = (system.main.max_particles as usize).saturating_sub(self.particles.len());
        for _ in 0..(n as usize).min(free) {
            self.spawn(system, world, Vec3::ZERO, 0.0, Some((at, inherit)));
        }
    }

    /// Advance by `dt` seconds without colliders.
    pub fn step(&mut self, system: &ParticleSystem, world: &Mat4, gravity: Vec3, dt: f32) {
        self.step_with(system, world, StepEnv::new(gravity), dt);
    }

    /// Advance by `dt` seconds: age and move particles (modules, collisions, trails), then emit
    /// for this slice of the cycle. Events for sub emitters land in `events`.
    pub fn step_with(&mut self, system: &ParticleSystem, world: &Mat4, env: StepEnv, dt: f32) {
        self.events.clear();
        if self.state == PlayState::Paused || dt <= 0.0 {
            return;
        }
        self.clock += dt;
        let g = env.gravity * system.main.gravity_modifier;
        self.update_particles(system, world, &env, g, dt);
        if self.state == PlayState::Playing {
            self.total_time += dt;
            if env.emit {
                self.advance_cycle(system, world, g, dt);
            } else {
                self.advance_clock_only(system, dt);
            }
            // A finished one-shot stops once its last particle dies.
            if !self.emitting && self.particles.is_empty() {
                self.state = PlayState::Stopped;
            }
        }
        self.update_trails(system, world);
    }

    /// Sub emitters keep their cycle clock without emitting by themselves.
    fn advance_clock_only(&mut self, system: &ParticleSystem, dt: f32) {
        let duration = system.main.duration.max(0.05);
        self.time += dt;
        if self.time >= duration {
            if system.main.looping {
                self.time %= duration;
            } else {
                self.time = duration;
                self.emitting = false;
            }
        }
    }

    fn wants(system: &ParticleSystem, t: SubEmitterTrigger) -> bool {
        system.sub_emitters.enabled
            && system
                .sub_emitters
                .list
                .iter()
                .any(|s| s.trigger == t && !s.emitter.is_empty())
    }

    fn event(
        &mut self,
        system: &ParticleSystem,
        world: &Mat4,
        trigger: SubEmitterTrigger,
        p: &Particle,
        velocity: Vec3,
    ) {
        let local = system.main.simulation_space == SimulationSpace::Local;
        let (position, velocity) = if local {
            (
                world.transform_point3(p.position),
                world.transform_vector3(velocity),
            )
        } else {
            (p.position, velocity)
        };
        self.events.push(SubEmitEvent {
            trigger,
            position,
            velocity,
            color: super::particle_color_srgb(system, p),
            size: super::particle_size(system, p),
        });
    }

    fn update_particles(
        &mut self,
        system: &ParticleSystem,
        world: &Mat4,
        env: &StepEnv,
        g: Vec3,
        dt: f32,
    ) {
        let local = system.main.simulation_space == SimulationSpace::Local;
        let inv = if local {
            world.inverse()
        } else {
            Mat4::IDENTITY
        };
        // A vector given in `space` turned into simulation space.
        let to_sim = |v: Vec3, space: SimulationSpace| -> Vec3 {
            match (local, space) {
                (true, SimulationSpace::World) => inv.transform_vector3(v),
                (false, SimulationSpace::Local) => world.transform_vector3(v),
                _ => v,
            }
        };
        let want_death = Self::wants(system, SubEmitterTrigger::Death);
        let want_hit = Self::wants(system, SubEmitterTrigger::Collision);
        let mut particles = std::mem::take(&mut self.particles);
        let mut dead: Vec<Particle> = Vec::new();
        let mut hits: Vec<(Particle, Vec3)> = Vec::new();
        let clock = self.clock;
        particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.lifetime {
                dead.push(*p);
                return false;
            }
            let t = p.life_fraction();
            let mut v = p.velocity + g * dt;
            let f = &system.force_over_lifetime;
            if f.enabled {
                let force = Vec3::new(
                    f.x.evaluate(t, p.rand(salt::FORCE)),
                    f.y.evaluate(t, p.rand(salt::FORCE + 1)),
                    f.z.evaluate(t, p.rand(salt::FORCE + 2)),
                );
                v += to_sim(force, f.space) * dt;
            }
            let lim = &system.limit_velocity_over_lifetime;
            if lim.enabled {
                let limit = lim.limit.evaluate(t, p.rand(salt::LIMIT)).max(0.0);
                let speed = v.length();
                if speed > limit && speed > 0.0 {
                    // Dampen is the fraction of the excess removed per 1/30 s.
                    let keep = (1.0 - lim.dampen.clamp(0.0, 1.0)).powf(dt * 30.0);
                    v *= (limit + (speed - limit) * keep) / speed;
                }
                let drag = lim.drag.evaluate(t, p.rand(salt::DRAG)).max(0.0);
                v /= 1.0 + drag * dt;
            }
            let mut moving = v;
            let vol = &system.velocity_over_lifetime;
            if vol.enabled {
                let lin = Vec3::new(
                    vol.x.evaluate(t, p.rand(salt::VELOCITY)),
                    vol.y.evaluate(t, p.rand(salt::VELOCITY + 1)),
                    vol.z.evaluate(t, p.rand(salt::VELOCITY + 2)),
                );
                moving += to_sim(lin, vol.space);
                moving *= vol.speed_modifier.evaluate(t, p.rand(salt::SPEED_MOD));
            }
            let prev = p.position;
            p.position += moving * dt;
            let n = &system.noise;
            if n.enabled {
                let at = if local {
                    world.transform_point3(p.position)
                } else {
                    p.position
                };
                let field = noise::field(
                    at * n.frequency.max(1e-4),
                    clock * n.scroll_speed,
                    n.octaves.clamp(1, 4),
                    n.octave_multiplier,
                    n.octave_scale,
                );
                let mut strength = n.strength.evaluate(t, p.rand(salt::NOISE));
                if n.damping {
                    strength /= n.frequency.max(1e-4);
                }
                p.position +=
                    to_sim(field, SimulationSpace::World) * (strength * n.position_amount * dt);
                p.rotation += field.x * n.rotation_amount * 90.0 * dt;
                p.noise_size = (1.0 + field.y * n.size_amount).max(0.0);
            }
            let r = &system.rotation_over_lifetime;
            if r.enabled {
                p.rotation += r.angular_velocity.evaluate(t, p.rand(salt::ROTATION)) * dt;
            }
            let rs = &system.rotation_by_speed;
            if rs.enabled {
                let s = speed_fraction(moving.length(), rs.range);
                p.rotation += rs.angular_velocity.evaluate(s, p.rand(salt::ROT_SPEED)) * dt;
            }
            let c = &system.collision;
            if c.enabled {
                let radius = super::particle_size(system, p) * c.radius_scale * 0.5;
                let (pw, prevw, vw) = if local {
                    (
                        world.transform_point3(p.position),
                        world.transform_point3(prev),
                        world.transform_vector3(v),
                    )
                } else {
                    (p.position, prev, v)
                };
                let hit = match c.kind {
                    CollisionType::Planes => c.planes.iter().find_map(|pl| {
                        let nrm = pl.normal.normalize_or_zero();
                        if nrm == Vec3::ZERO {
                            return None;
                        }
                        let d = (pw - pl.point).dot(nrm) - radius;
                        let before = (prevw - pl.point).dot(nrm) - radius;
                        // One-sided: from the front (or resting on it).
                        (d < 0.0 && before >= -radius - 1e-4).then(|| (pw - nrm * d, nrm))
                    }),
                    CollisionType::World => env.colliders.iter().find_map(|col| {
                        if c.collides_with & (1u32 << (col.layer as u32 & 31)) == 0 {
                            return None;
                        }
                        col.shape.sweep(prevw, pw, radius)
                    }),
                };
                if let Some((at, nrm)) = hit {
                    let vn = vw.dot(nrm);
                    let mut vw2 = vw;
                    if vn < 0.0 {
                        vw2 = (vw - nrm * vn) - nrm * (vn * c.bounce.max(0.0));
                        vw2 *= 1.0 - c.dampen.clamp(0.0, 1.0);
                        p.age += c.lifetime_loss.max(0.0) * p.lifetime;
                    }
                    let (pos, vel) = if local {
                        (inv.transform_point3(at), inv.transform_vector3(vw2))
                    } else {
                        (at, vw2)
                    };
                    p.position = pos;
                    v = vel;
                    // An impact (not resting contact) sends the Collision trigger.
                    if vn < -1.0 && want_hit {
                        hits.push((*p, v));
                    }
                    let speed = vw2.length();
                    if vn < 0.0
                        && (speed < c.min_kill_speed
                            || speed > c.max_kill_speed
                            || p.age >= p.lifetime)
                    {
                        p.velocity = v;
                        dead.push(*p);
                        return false;
                    }
                }
            }
            p.velocity = v;
            true
        });
        self.particles = particles;
        for (p, v) in hits {
            self.event(system, world, SubEmitterTrigger::Collision, &p, v);
        }
        for p in dead {
            if want_death {
                self.event(system, world, SubEmitterTrigger::Death, &p, p.velocity);
            }
            if let Some(t) = self.trails.get_mut(&p.id) {
                t.alive = false;
                t.last = Some(p);
            }
        }
    }

    /// Trail points: added when a particle moved far enough, dropped when they expire.
    fn update_trails(&mut self, system: &ParticleSystem, world: &Mat4) {
        let tr = &system.trails;
        if !tr.enabled || tr.mode == TrailMode::Ribbon {
            self.trails.clear();
            return;
        }
        let local = system.main.simulation_space == SimulationSpace::Local;
        let now = self.clock;
        for p in &self.particles {
            if p.rand(salt::TRAIL) >= tr.ratio {
                continue;
            }
            let pos = if tr.world_space && local {
                world.transform_point3(p.position)
            } else {
                p.position
            };
            let life = tr
                .lifetime
                .evaluate(p.life_fraction(), p.rand(salt::TRAIL_LIFE))
                * p.lifetime;
            let trail = self.trails.entry(p.id).or_insert_with(|| Trail {
                points: VecDeque::new(),
                point_life: life,
                alive: true,
                last: None,
            });
            trail.point_life = life.max(1e-3);
            let far = trail
                .points
                .front()
                .is_none_or(|q| q.position.distance(pos) >= tr.min_vertex_distance.max(1e-4));
            if far {
                trail.points.push_front(TrailPoint {
                    position: pos,
                    time: now,
                });
            }
        }
        let die = tr.die_with_particles;
        self.trails.retain(|_, t| {
            if !t.alive && die {
                return false;
            }
            while t.points.back().is_some_and(|q| now - q.time > t.point_life) {
                t.points.pop_back();
            }
            t.alive || !t.points.is_empty()
        });
    }

    /// Move the cycle clock forward, emitting by rate and firing bursts; wraps when looping.
    fn advance_cycle(&mut self, system: &ParticleSystem, world: &Mat4, g: Vec3, dt: f32) {
        let mut dt = dt;
        if self.delay_left > 0.0 {
            let used = dt.min(self.delay_left);
            self.delay_left -= used;
            dt -= used;
            if dt <= 0.0 {
                return;
            }
        }
        let duration = system.main.duration.max(0.05);
        while dt > 0.0 && self.emitting {
            let slice = dt.min(duration - self.time);
            let (from, to) = (self.time, self.time + slice);
            if system.emission.enabled {
                self.emit_rate(system, world, g, slice, dt);
                self.fire_bursts(system, world, g, (from, to), dt - slice);
            }
            self.time = to;
            dt -= slice;
            if self.time >= duration - 1e-6 {
                if system.main.looping {
                    self.time = 0.0;
                    self.bursts_fired.iter_mut().for_each(|c| *c = 0);
                } else {
                    self.time = duration;
                    self.emitting = false;
                }
            }
        }
    }

    /// Rate over Time for one slice; `remaining` = seconds of the frame after the slice starts.
    fn emit_rate(
        &mut self,
        system: &ParticleSystem,
        world: &Mat4,
        g: Vec3,
        slice: f32,
        remaining: f32,
    ) {
        let cycle = self.time / system.main.duration.max(0.05);
        let rate = system
            .emission
            .rate_over_time
            .sample_at(cycle, &mut self.rng)
            .max(0.0);
        self.accumulator += rate * slice;
        if !self.accumulator.is_finite() {
            // An infinite rate made the accumulator NaN: emission was dead from then on.
            self.accumulator = f32::MAX;
        }
        let n = self.accumulator.floor();
        self.accumulator = (self.accumulator - n).clamp(0.0, 1.0);
        let n = n as u32;
        // No more tries than free slots: Rate 1e12 spun 4 billion times a frame.
        let free = (system.main.max_particles as usize).saturating_sub(self.particles.len());
        for i in 0..n.min(free as u32) {
            // Spread births across the slice so low frame rates don't clump particles.
            let born_at = slice * (i as f32 + 0.5) / n as f32;
            let age = (remaining - born_at).max(0.0);
            self.spawn(system, world, g, age, None);
        }
    }

    /// Bursts whose next cycle time falls in [from, to) of the cycle; `after` = frame time left
    /// once the slice ends (for the particles' starting age).
    fn fire_bursts(
        &mut self,
        system: &ParticleSystem,
        world: &Mat4,
        g: Vec3,
        span: (f32, f32),
        after: f32,
    ) {
        let (from, to) = span;
        if self.bursts_fired.len() != system.emission.bursts.len() {
            self.bursts_fired.resize(system.emission.bursts.len(), 0);
        }
        for (i, b) in system.emission.bursts.iter().enumerate() {
            loop {
                let fired = self.bursts_fired[i];
                if b.cycles != 0 && fired >= b.cycles {
                    break;
                }
                let at = b.time + fired as f32 * b.interval.max(0.01);
                // `!(..)` so a NaN time ends the loop too.
                if !(at >= from && at < to) {
                    break;
                }
                self.bursts_fired[i] += 1;
                if self.rng.next_f32() < b.probability {
                    let age = (to - at) + after;
                    let free =
                        (system.main.max_particles as usize).saturating_sub(self.particles.len());
                    for _ in 0..(b.count as usize).min(free) {
                        self.spawn(system, world, g, age, None);
                    }
                }
            }
        }
    }

    /// Create one particle (if under Max Particles), already `age` seconds old; from a sub
    /// emitter trigger at a world position when `at` is set.
    fn spawn(
        &mut self,
        system: &ParticleSystem,
        world: &Mat4,
        g: Vec3,
        age: f32,
        at: Option<(Vec3, Inherit)>,
    ) {
        let main = &system.main;
        if self.particles.len() >= main.max_particles as usize {
            return;
        }
        let cycle = self.time / main.duration.max(0.05);
        let lifetime = main.start_lifetime.sample_at(cycle, &mut self.rng);
        if lifetime <= age {
            return;
        }
        let (local_pos, dir) = shape::sample(&system.shape, &mut self.rng);
        let speed = main.start_speed.sample_at(cycle, &mut self.rng);
        let mut size = main.start_size.sample_at(cycle, &mut self.rng).max(0.0);
        let mut color = main.start_color.sample_at(cycle, &mut self.rng);
        let rotation = main.start_rotation.sample_at(cycle, &mut self.rng);
        let seed = self.rng.next_u32();
        let local = main.simulation_space == SimulationSpace::Local;
        let (position, velocity) = match at {
            None => {
                let (position, direction) = if local {
                    (local_pos, dir)
                } else {
                    (
                        world.transform_point3(local_pos),
                        world.transform_vector3(dir).normalize_or_zero(),
                    )
                };
                let velocity = direction * speed + g * age;
                (
                    position + direction * speed * age + g * (0.5 * age * age),
                    velocity,
                )
            }
            Some((origin, inherit)) => {
                // The shape (rotated with the object) centred on the trigger point.
                let dir_w = world.transform_vector3(dir).normalize_or_zero();
                let pos_w = origin + world.transform_vector3(local_pos);
                let vel_w = dir_w * speed + inherit.velocity;
                if let Some(c) = inherit.color {
                    color = std::array::from_fn(|i| color[i] * c[i]);
                }
                if let Some(s) = inherit.size {
                    size *= s;
                }
                if local {
                    let inv = world.inverse();
                    (inv.transform_point3(pos_w), inv.transform_vector3(vel_w))
                } else {
                    (pos_w, vel_w)
                }
            }
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let p = Particle {
            position,
            velocity,
            age,
            lifetime,
            size,
            color,
            rotation,
            seed,
            id,
            noise_size: 1.0,
        };
        self.particles.push(p);
        if Self::wants(system, SubEmitterTrigger::Birth) {
            self.event(system, world, SubEmitterTrigger::Birth, &p, velocity);
        }
    }
}

/// 0..1 of `speed` across a speed range.
pub fn speed_fraction(speed: f32, range: [f32; 2]) -> f32 {
    let span = range[1] - range[0];
    if span.abs() < 1e-6 {
        return if speed >= range[1] { 1.0 } else { 0.0 };
    }
    ((speed - range[0]) / span).clamp(0.0, 1.0)
}
