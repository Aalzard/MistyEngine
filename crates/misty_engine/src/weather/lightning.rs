//! Lightning: strikes at random times (a Poisson process at the weather's frequency), each a
//! flash of a few return strokes that lights the scene and the clouds, a branching bolt for the
//! nearer ones, and thunder that arrives after the distance / the speed of sound.

use glam::Vec3;

/// Speed of sound in air (m/s): thunder arrives `distance / SPEED_OF_SOUND` seconds after the
/// flash.
pub const SPEED_OF_SOUND: f32 = 343.0;

/// Strikes farther than this only light the clouds (no visible bolt).
pub const BOLT_RANGE: f32 = 9000.0;

/// Seconds a strike lasts (flash and bolt).
pub const STRIKE_TIME: f32 = 0.9;

/// Height of the cloud base bolts come from (metres above the camera).
pub const CLOUD_BASE: f32 = 900.0;

/// Seconds after a strike its thunder is heard.
pub fn thunder_delay(distance: f32) -> f32 {
    distance.max(0.0) / SPEED_OF_SOUND
}

/// Small deterministic random numbers (xorshift64*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new(1)
    }
}

/// One piece of a bolt: from `a` to `b`, `width` metres, `brightness` 0..1 (branches dimmer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: Vec3,
    pub b: Vec3,
    pub width: f32,
    pub brightness: f32,
}

/// A branching lightning bolt (procedural).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bolt {
    pub segments: Vec<Segment>,
}

/// Most segments a bolt has (main channel and branches).
pub const MAX_SEGMENTS: usize = 384;

impl Bolt {
    /// A bolt from the cloud at `top` down to `bottom`: the main channel by midpoint
    /// displacement (jagged at every scale), with branches forking off downwards.
    pub fn generate(seed: u64, top: Vec3, bottom: Vec3) -> Self {
        let mut rng = Rng::new(seed ^ 0xB017);
        let mut segments = Vec::new();
        let length = (top - bottom).length().max(1.0);
        let width = (length / 400.0).clamp(1.5, 8.0);
        let main = channel(&mut rng, top, bottom, 7, 0.16);
        push_path(&mut segments, &main, width, 1.0);
        // Branches: from points along the upper two thirds, heading down and outwards.
        let branches = 4 + (rng.f32() * 5.0) as usize;
        for _ in 0..branches {
            let at = ((rng.f32() * 0.65) * (main.len() - 1) as f32) as usize;
            let start = main[at];
            let remaining = start.y - bottom.y;
            if remaining < length * 0.05 {
                continue;
            }
            let side =
                Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)).normalize_or(Vec3::X);
            let reach = remaining * rng.range(0.2, 0.55);
            let end = start + side * reach * 0.7 - Vec3::Y * reach;
            let path = channel(&mut rng, start, end, 5, 0.22);
            push_path(&mut segments, &path, width * 0.5, rng.range(0.3, 0.6));
            // A twig off the branch.
            if path.len() > 4 && rng.f32() < 0.6 {
                let s = path[path.len() / 3];
                let twig_end = s + side.cross(Vec3::Y) * reach * 0.3 - Vec3::Y * reach * 0.35;
                let twig = channel(&mut rng, s, twig_end, 4, 0.25);
                push_path(&mut segments, &twig, width * 0.3, rng.range(0.15, 0.35));
            }
        }
        segments.truncate(MAX_SEGMENTS);
        Self { segments }
    }
}

fn push_path(out: &mut Vec<Segment>, path: &[Vec3], width: f32, brightness: f32) {
    for w in path.windows(2) {
        out.push(Segment {
            a: w[0],
            b: w[1],
            width,
            brightness,
        });
    }
}

/// Points from `a` to `b` with `levels` rounds of midpoint displacement (`rough` × the
/// piece's length at each round).
fn channel(rng: &mut Rng, a: Vec3, b: Vec3, levels: u32, rough: f32) -> Vec<Vec3> {
    let mut pts = vec![a, b];
    for _ in 0..levels {
        let mut next = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            let len = (q - p).length();
            let offset = Vec3::new(
                rng.range(-1.0, 1.0),
                rng.range(-0.3, 0.3),
                rng.range(-1.0, 1.0),
            ) * len
                * rough;
            next.push(p);
            next.push((p + q) * 0.5 + offset);
        }
        next.push(*pts.last().unwrap());
        pts = next;
    }
    pts
}

/// A lightning strike in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Strike {
    /// Seconds since it began.
    pub age: f32,
    /// Where the bolt meets the ground (or the cloud flash centre for far strikes).
    pub position: Vec3,
    /// Metres from the camera when it struck.
    pub distance: f32,
    /// × the flash brightness (the weather's lightning intensity).
    pub intensity: f32,
    /// Return strokes: start times (s) of the flickers.
    pub strokes: [f32; 4],
    pub stroke_count: usize,
    /// The visible bolt (None for far strikes).
    pub bolt: Option<std::sync::Arc<Bolt>>,
    pub seed: u64,
}

impl Strike {
    /// Flash envelope at its current age: 0..1 (a sharp attack and fast decay per stroke).
    pub fn envelope(&self) -> f32 {
        let mut v: f32 = 0.0;
        for &s in &self.strokes[..self.stroke_count] {
            let t = self.age - s;
            if t >= 0.0 {
                let attack = (t / 0.008).min(1.0);
                v = v.max(attack * (-t / 0.07).exp());
            }
        }
        // A dim afterglow between strokes.
        if self.age < STRIKE_TIME {
            v = v.max(0.12 * (1.0 - self.age / STRIKE_TIME));
        }
        v
    }

    /// How bright the flash is at the camera: brightness falls with distance (scattered by
    /// the clouds, so slower than 1/d²).
    pub fn brightness(&self) -> f32 {
        let fall = 1.0 / (1.0 + self.distance / 1500.0);
        self.envelope() * self.intensity * fall
    }

    pub fn finished(&self) -> bool {
        self.age > STRIKE_TIME
    }
}

/// A thunder sound waiting for its delay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingThunder {
    /// Seconds until it is heard.
    pub delay: f32,
    pub distance: f32,
    /// 0..1.
    pub volume: f32,
}

/// The lightning of a weather: random strikes, the ones in progress and thunder on its way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lightning {
    pub rng: Rng,
    pub strikes: Vec<Strike>,
    pub thunder: Vec<PendingThunder>,
    seeded: bool,
}

/// What happened this update.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightningEvents {
    /// New strikes: (position, distance, intensity).
    pub strikes: Vec<(Vec3, f32, f32)>,
    /// Thunder heard now: (distance, volume).
    pub thunder: Vec<(f32, f32)>,
}

impl Lightning {
    pub fn seed(&mut self, seed: u64) {
        if !self.seeded {
            self.rng = Rng::new(seed ^ 0x11_6874);
            self.seeded = true;
        }
    }

    /// Advance `dt` seconds: age strikes, deliver due thunder and maybe strike (`frequency`
    /// strikes per minute) at `min..max` metres from `camera`.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f32,
        frequency: f32,
        min: f32,
        max: f32,
        intensity: f32,
        camera: Vec3,
        bolts: bool,
    ) -> LightningEvents {
        let mut ev = LightningEvents::default();
        for s in &mut self.strikes {
            s.age += dt;
        }
        self.strikes.retain(|s| !s.finished());
        for t in &mut self.thunder {
            t.delay -= dt;
            if t.delay <= 0.0 {
                ev.thunder.push((t.distance, t.volume));
            }
        }
        self.thunder.retain(|t| t.delay > 0.0);
        if frequency > 0.0 && dt > 0.0 {
            // Poisson: chance of a strike in dt (several per frame at absurd rates are merged).
            let p = 1.0 - (-frequency / 60.0 * dt).exp();
            if self.rng.f32() < p {
                let r = self.rng.f32();
                // More far strikes than near ones (area grows with distance).
                let d = min + (max - min).max(0.0) * r.sqrt();
                let a = self.rng.range(0.0, std::f32::consts::TAU);
                let pos = camera + Vec3::new(a.sin() * d, 0.0, a.cos() * d);
                ev.strikes
                    .push(self.strike_at(pos, d, intensity, bolts, camera));
            }
        }
        ev
    }

    /// Start a strike at `position` (`distance` from the camera); returns its event.
    pub fn strike_at(
        &mut self,
        position: Vec3,
        distance: f32,
        intensity: f32,
        bolts: bool,
        camera: Vec3,
    ) -> (Vec3, f32, f32) {
        let seed = self.rng.next_u64();
        let mut strokes = [0.0; 4];
        let stroke_count = 1 + (self.rng.f32() * 3.99) as usize;
        let mut t = 0.0;
        for s in strokes.iter_mut().take(stroke_count) {
            *s = t;
            t += self.rng.range(0.04, 0.16);
        }
        let bolt = (bolts && distance < BOLT_RANGE).then(|| {
            let ground = Vec3::new(position.x, camera.y - 30.0, position.z);
            let lean = Vec3::new(self.rng.range(-1.0, 1.0), 0.0, self.rng.range(-1.0, 1.0))
                * CLOUD_BASE
                * 0.35;
            let top = Vec3::new(position.x, camera.y + CLOUD_BASE, position.z) + lean;
            std::sync::Arc::new(Bolt::generate(seed, top, ground))
        });
        self.strikes.push(Strike {
            age: 0.0,
            position,
            distance,
            intensity,
            strokes,
            stroke_count,
            bolt,
            seed,
        });
        // Near thunder is loud and cracks; far thunder is a low rumble.
        let volume = (intensity * (1.0 / (1.0 + distance / 2500.0))).clamp(0.0, 1.0);
        self.thunder.push(PendingThunder {
            delay: thunder_delay(distance),
            distance,
            volume,
        });
        (position, distance, intensity)
    }

    /// The flash lighting the scene now: brightness and the unit direction towards it.
    pub fn flash(&self, camera: Vec3) -> (f32, Vec3) {
        let mut total = 0.0;
        let mut dir = Vec3::ZERO;
        for s in &self.strikes {
            let b = s.brightness();
            total += b;
            // From high above the strike (the lit cloud).
            let towards = (s.position + Vec3::Y * CLOUD_BASE * 0.8 - camera).normalize_or(Vec3::Y);
            dir += towards * b;
        }
        (total, dir.normalize_or(Vec3::Y))
    }
}
