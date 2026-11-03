//! Runtime state of one Particle System and its fixed rules: emission (rate + bursts), aging,
//! gravity and movement. Pure: no scene, no GPU, deterministic for a given seed.

use glam::{Mat4, Vec3};

use super::component::{ParticleSystem, SimulationSpace};
use super::rng::{self, Rng};
use super::shape;

/// Step used to simulate Prewarm.
const PREWARM_STEP: f32 = 1.0 / 30.0;

/// One live particle. Position/velocity are in world space or the emitter's local space,
/// depending on the system's Simulation Space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Seconds since it was emitted.
    pub age: f32,
    pub lifetime: f32,
    pub size: f32,
    /// sRGB colour + alpha picked at birth.
    pub color: [f32; 4],
}

impl Particle {
    /// 0 at birth, 1 at death.
    pub fn life_fraction(&self) -> f32 {
        (self.age / self.lifetime.max(1e-6)).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    #[default]
    Stopped,
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
    /// Start Delay still to wait before emitting.
    delay_left: f32,
    /// Fractional particles carried between frames by Rate over Time.
    accumulator: f32,
    /// Cycles fired this loop, per burst.
    bursts_fired: Vec<u32>,
    /// True while the cycle is emitting (false after a Stop or when a non-looping cycle ends).
    emitting: bool,
    rng: Rng,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            state: PlayState::Stopped,
            time: 0.0,
            total_time: 0.0,
            delay_left: 0.0,
            accumulator: 0.0,
            bursts_fired: Vec::new(),
            emitting: false,
            rng: Rng::new(0),
        }
    }
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
            let mut t = 0.0;
            while t < system.main.duration {
                self.step(system, world, gravity, PREWARM_STEP);
                t += PREWARM_STEP;
            }
            self.total_time = 0.0;
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

    /// Remove every live particle.
    pub fn clear(&mut self) {
        self.particles.clear();
    }

    /// Emit `n` particles now, whatever the play state (up to Max Particles).
    pub fn emit(&mut self, system: &ParticleSystem, world: &Mat4, n: u32) {
        for _ in 0..n {
            self.spawn(system, world, Vec3::ZERO, 0.0);
        }
    }

    /// Advance by `dt` seconds: age and move particles, then emit for this slice of the cycle.
    pub fn step(&mut self, system: &ParticleSystem, world: &Mat4, gravity: Vec3, dt: f32) {
        if self.state == PlayState::Paused || dt <= 0.0 {
            return;
        }
        let g = gravity * system.main.gravity_modifier;
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.lifetime {
                return false;
            }
            p.velocity += g * dt;
            p.position += p.velocity * dt;
            true
        });
        if self.state == PlayState::Playing {
            self.total_time += dt;
            self.advance_cycle(system, world, g, dt);
            // A finished one-shot stops once its last particle dies.
            if !self.emitting && self.particles.is_empty() {
                self.state = PlayState::Stopped;
            }
        }
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
        let rate = system
            .emission
            .rate_over_time
            .sample(&mut self.rng)
            .max(0.0);
        self.accumulator += rate * slice;
        let n = self.accumulator.floor();
        self.accumulator -= n;
        let n = n as u32;
        for i in 0..n {
            // Spread births across the slice so low frame rates don't clump particles.
            let born_at = slice * (i as f32 + 0.5) / n as f32;
            let age = (remaining - born_at).max(0.0);
            self.spawn(system, world, g, age);
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
                if at < from || at >= to {
                    break;
                }
                self.bursts_fired[i] += 1;
                if self.rng.next_f32() < b.probability {
                    let age = (to - at) + after;
                    for _ in 0..b.count {
                        self.spawn(system, world, g, age);
                    }
                }
            }
        }
    }

    /// Create one particle (if under Max Particles), already `age` seconds old.
    fn spawn(&mut self, system: &ParticleSystem, world: &Mat4, g: Vec3, age: f32) {
        let main = &system.main;
        if self.particles.len() >= main.max_particles as usize {
            return;
        }
        let lifetime = main.start_lifetime.sample(&mut self.rng);
        if lifetime <= age {
            return;
        }
        let (local_pos, dir) = shape::sample(&system.shape, &mut self.rng);
        let speed = main.start_speed.sample(&mut self.rng);
        let (position, direction) = match main.simulation_space {
            SimulationSpace::Local => (local_pos, dir),
            SimulationSpace::World => (
                world.transform_point3(local_pos),
                world.transform_vector3(dir).normalize_or_zero(),
            ),
        };
        let velocity = direction * speed + g * age;
        self.particles.push(Particle {
            position: position + direction * speed * age + g * (0.5 * age * age),
            velocity,
            age,
            lifetime,
            size: main.start_size.sample(&mut self.rng).max(0.0),
            color: main.start_color.sample(&mut self.rng),
        });
    }
}
