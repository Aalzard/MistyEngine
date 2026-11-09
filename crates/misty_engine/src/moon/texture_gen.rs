//! The built-in moon texture, made here (deterministic, no inputs): an equirectangular
//! 2048 × 1024 two-channel PNG, luminance = albedo (sRGB) and alpha = height (128 = mean),
//! centred on the near side (selenographic longitude 0 at u = 0.5, east to the right).
//!
//! - Highlands: bright, with sphere-space fbm at several scales.
//! - Maria: the real near-side seas (Imbrium, Serenitatis, Tranquillitatis, Crisium,
//!   Fecunditatis, Nectaris, Nubium, Humorum, Frigoris, Oceanus Procellarum, …) as soft,
//!   noise-edged unions of caps at their selenographic positions, so the familiar face shows.
//! - Craters at many scales (a power-law size distribution, fewer on the younger maria):
//!   bowl, raised rim, central peaks on the large ones, ejecta; young ones bright with ray
//!   systems. Named landmarks on top (Tycho and Copernicus with long rays, Kepler, bright
//!   Aristarchus, dark-floored Plato and Grimaldi, Clavius, Theophilus, …).
//!
//! `cargo test --release -p misty_engine --lib -- --ignored write_moon_texture` writes
//! `assets/moon.png`; `moon_texture_matches_its_generator` checks the committed file.

use glam::Vec3;

/// Width of the built-in texture (height = width / 2).
pub const WIDTH: u32 = 2048;

/// Selenographic latitude / longitude (degrees, east positive) → unit vector in the moon's
/// frame: +Z towards the Earth (the near side's centre), +X east (right on the disc as seen
/// from the northern hemisphere), +Y north.
pub fn selenographic(lat: f32, lon: f32) -> Vec3 {
    let (la, lo) = (lat.to_radians(), lon.to_radians());
    Vec3::new(la.cos() * lo.sin(), la.sin(), la.cos() * lo.cos())
}

// ---- Deterministic noise (integer hashes: the same image on every machine) ----

fn pcg(v: u32) -> u32 {
    let state = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let h = pcg(x as u32 ^ pcg(y as u32 ^ pcg(z as u32 ^ pcg(seed))));
    h as f32 / u32::MAX as f32
}

/// Smooth value noise in 0..1.
fn noise(p: Vec3, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let (x, y, z) = (i.x as i32, i.y as i32, i.z as i32);
    let h = |dx, dy, dz| hash3(x + dx, y + dy, z + dz, seed);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(h(0, 0, 0), h(1, 0, 0), u.x);
    let x10 = lerp(h(0, 1, 0), h(1, 1, 0), u.x);
    let x01 = lerp(h(0, 0, 1), h(1, 0, 1), u.x);
    let x11 = lerp(h(0, 1, 1), h(1, 1, 1), u.x);
    lerp(lerp(x00, x10, u.y), lerp(x01, x11, u.y), u.z)
}

/// Fractal noise around 0.5 (octaves of halving amplitude).
fn fbm(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q) = (0.0, 0.5, 0.0, p);
    for o in 0..octaves {
        sum += noise(q, seed.wrapping_add(o * 977)) * amp;
        norm += amp;
        amp *= 0.5;
        q = q * 2.03 + Vec3::new(1.7, 9.2, 4.1);
    }
    sum / norm
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Deterministic random stream.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = pcg(self.0);
        self.0 as f32 / u32::MAX as f32
    }

    fn unit(&mut self) -> Vec3 {
        let z = self.next() * 2.0 - 1.0;
        let a = self.next() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), z, r * a.sin())
    }
}

// ---- Maria ----

/// One cap of a mare: selenographic latitude, longitude, angular radius (degrees) and how dark
/// (1 = the usual mare albedo, lower = lighter).
struct Cap(f32, f32, f32, f32);

/// The near-side seas as unions of caps (positions from the IAU gazetteer; irregular seas are
/// several caps).
const MARIA: &[Cap] = &[
    // Mare Imbrium and Sinus Iridum.
    Cap(33.0, -16.0, 16.5, 1.0),
    Cap(38.0, -27.0, 9.0, 1.0),
    Cap(44.0, -31.0, 4.0, 0.95),
    // Oceanus Procellarum (the large western sea) and Mare Insularum.
    Cap(22.0, -55.0, 15.0, 0.95),
    Cap(10.0, -52.0, 14.0, 0.95),
    Cap(32.0, -52.0, 11.0, 0.9),
    Cap(0.0, -45.0, 11.0, 0.9),
    Cap(-5.0, -58.0, 10.0, 0.9),
    Cap(15.0, -66.0, 10.0, 0.92),
    Cap(42.0, -45.0, 8.0, 0.85),
    Cap(25.0, -62.0, 10.0, 0.9),
    Cap(-12.0, -48.0, 7.0, 0.85),
    Cap(5.0, -64.0, 9.0, 0.9),
    Cap(18.0, -40.0, 8.0, 0.9),
    Cap(7.5, -30.9, 8.0, 0.9),
    Cap(-10.0, -23.0, 6.0, 0.85),
    // Mare Serenitatis, Tranquillitatis, Vaporum, Sinus Medii.
    Cap(28.0, 17.5, 10.0, 0.9),
    Cap(8.5, 31.4, 10.5, 1.05),
    Cap(13.0, 22.0, 6.0, 1.0),
    Cap(3.0, 39.0, 6.0, 1.05),
    Cap(13.3, 3.6, 3.6, 0.95),
    Cap(2.0, 1.0, 2.6, 0.85),
    // Mare Crisium, Fecunditatis, Nectaris.
    Cap(17.0, 59.1, 8.4, 1.05),
    Cap(-5.0, 51.0, 8.0, 0.95),
    Cap(-11.0, 55.0, 5.5, 0.95),
    Cap(-2.0, 47.0, 4.5, 0.9),
    Cap(-15.2, 35.5, 5.0, 0.95),
    // Mare Nubium, Cognitum, Humorum.
    Cap(-21.3, -16.6, 8.5, 0.9),
    Cap(-13.0, -20.0, 5.0, 0.85),
    Cap(-24.4, -38.6, 6.0, 1.0),
    // Mare Frigoris: a long thin band across the north.
    Cap(56.0, -35.0, 4.5, 0.85),
    Cap(57.0, -18.0, 4.5, 0.85),
    Cap(57.0, 0.0, 4.5, 0.85),
    Cap(56.0, 18.0, 4.0, 0.85),
    Cap(54.0, 33.0, 3.5, 0.8),
    // Lacus Somniorum, Mare Vaporum's neighbours, Palus Putredinis.
    Cap(38.0, 29.0, 4.0, 0.8),
    Cap(26.5, 0.5, 3.0, 0.85),
    // Near the limbs: Mare Marginis, Smythii, Australe, Humboldtianum.
    Cap(13.3, 86.5, 4.5, 0.85),
    Cap(-1.3, 87.0, 5.5, 0.85),
    Cap(-39.0, 93.0, 7.0, 0.65),
    Cap(57.0, 81.0, 4.0, 0.7),
    // Far side: Mare Moscoviense, Tsiolkovskiy's floor, Mare Orientale's centre.
    Cap(27.3, 147.9, 4.5, 0.9),
    Cap(-20.4, 129.1, 2.2, 0.95),
    Cap(-19.4, -92.8, 3.2, 0.8),
    // South Pole–Aitken basin: a faint large darkening.
    Cap(-53.0, -169.0, 32.0, 0.18),
];

/// A named crater: latitude, longitude (degrees), radius (km), freshness 0..1 (1 = young and
/// bright with rays), and floor darkness (lava-flooded floors).
struct Named(f32, f32, f32, f32, f32);

const NAMED: &[Named] = &[
    Named(-43.3, -11.4, 43.0, 1.0, 0.0),   // Tycho
    Named(9.6, -20.1, 47.0, 0.95, 0.0),    // Copernicus
    Named(8.1, -38.0, 16.0, 0.9, 0.0),     // Kepler
    Named(23.7, -47.4, 20.0, 1.0, 0.0),    // Aristarchus
    Named(16.1, 46.8, 14.0, 0.9, 0.0),     // Proclus
    Named(-8.9, 61.0, 66.0, 0.7, 0.0),     // Langrenus
    Named(-11.4, 26.4, 50.0, 0.7, 0.0),    // Theophilus
    Named(-13.0, 23.0, 50.0, 0.3, 0.0),    // Cyrillus
    Named(-58.4, -14.4, 113.0, 0.15, 0.0), // Clavius
    Named(51.6, -9.3, 51.0, 0.3, 0.9),     // Plato
    Named(-5.2, -68.6, 86.0, 0.2, 0.9),    // Grimaldi
    Named(-9.3, -1.9, 77.0, 0.15, 0.35),   // Ptolemaeus
    Named(-13.4, -2.8, 54.0, 0.25, 0.2),   // Alphonsus
    Named(-18.2, -1.9, 48.0, 0.4, 0.0),    // Arzachel
    Named(29.7, -4.0, 41.0, 0.3, 0.6),     // Archimedes
    Named(14.5, -11.3, 29.0, 0.55, 0.0),   // Eratosthenes
    Named(-25.1, 60.4, 88.0, 0.4, 0.0),    // Petavius
    Named(-44.3, -54.6, 115.0, 0.1, 0.45), // Schickard
    Named(-50.0, -6.2, 97.0, 0.1, 0.0),    // Maginus
    Named(-5.6, 5.0, 75.0, 0.1, 0.0),      // Hipparchus
    Named(-11.7, 4.0, 57.0, 0.2, 0.0),     // Albategnius
    Named(-24.6, 22.3, 50.0, 0.8, 0.0),    // Stevinus-like bright crater (Furnerius A stand-in)
    Named(-24.5, -65.3, 9.0, 0.95, 0.0),   // Byrgius A
    Named(36.0, 102.9, 11.0, 1.0, 0.0),    // Giordano Bruno (far side)
    Named(-26.4, -15.1, 56.0, 0.2, 0.0),   // Pitatus
    Named(40.5, -9.4, 8.0, 0.75, 0.0),     // Aristillus-like
    Named(-35.0, 7.0, 32.0, 0.6, 0.0),     // highland bright crater
];

/// Moon radius in km (named crater sizes → angles).
const RADIUS_KM: f32 = 1737.4;

/// One crater to stamp.
struct Crater {
    centre: Vec3,
    /// Angular radius (radians).
    r: f32,
    /// 0 = old and soft, 1 = young, bright and rayed.
    fresh: f32,
    /// Darkening of the floor (lava-flooded).
    dark_floor: f32,
    seed: u32,
}

/// Working image: albedo (sRGB-encoded 0..1) and height (radians of the moon's radius).
struct Canvas {
    w: usize,
    h: usize,
    albedo: Vec<f32>,
    height: Vec<f32>,
    dirs: Vec<Vec3>,
    lat: Vec<f32>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        let mut dirs = Vec::with_capacity(w * h);
        let mut lat = Vec::with_capacity(h);
        for y in 0..h {
            let la = 90.0 - (y as f32 + 0.5) / h as f32 * 180.0;
            lat.push(la.to_radians());
            for x in 0..w {
                let lo = (x as f32 + 0.5) / w as f32 * 360.0 - 180.0;
                dirs.push(selenographic(la, lo));
            }
        }
        Self {
            w,
            h,
            albedo: vec![0.0; w * h],
            height: vec![0.0; w * h],
            dirs,
            lat,
        }
    }

    /// Calls `f(index, direction)` for every texel within `reach` radians of `c`.
    fn around(&mut self, c: Vec3, reach: f32, mut f: impl FnMut(&mut Self, usize, Vec3)) {
        let c_lat = c.y.clamp(-1.0, 1.0).asin();
        let c_lon = c.x.atan2(c.z);
        let (w, h) = (self.w, self.h);
        let y0 = (((std::f32::consts::FRAC_PI_2 - (c_lat + reach)) / std::f32::consts::PI
            * h as f32)
            .floor()
            .max(0.0)) as usize;
        let y1 = ((((std::f32::consts::FRAC_PI_2 - (c_lat - reach)) / std::f32::consts::PI)
            * h as f32)
            .ceil()
            .min(h as f32)) as usize;
        let cos_r = reach.cos();
        for y in y0..y1 {
            let la = self.lat[y];
            // Longitudes within `reach` of the centre on this row.
            let denom = la.cos() * c_lat.cos();
            let rhs = if denom.abs() < 1e-6 {
                -2.0
            } else {
                (cos_r - la.sin() * c_lat.sin()) / denom
            };
            if rhs > 1.0 {
                continue;
            }
            let span = if rhs <= -1.0 {
                std::f32::consts::PI
            } else {
                rhs.acos() + 2.0 * std::f32::consts::PI / w as f32
            };
            let to_x = |lon: f32| (lon + std::f32::consts::PI) / std::f32::consts::TAU * w as f32;
            let (x0, x1) = if span >= std::f32::consts::PI {
                (0i64, w as i64)
            } else {
                (
                    to_x(c_lon - span).floor() as i64,
                    to_x(c_lon + span).ceil() as i64,
                )
            };
            for xx in x0..x1 {
                let x = xx.rem_euclid(w as i64) as usize;
                let i = y * w + x;
                let d = self.dirs[i];
                f(self, i, d);
            }
        }
    }
}

/// Angle between two unit vectors (chord form: exact for small angles).
fn angle(a: Vec3, b: Vec3) -> f32 {
    2.0 * ((a - b).length() * 0.5).min(1.0).asin()
}

/// Mare cover (0..1) and how dark the mare is there.
fn maria(p: Vec3) -> (f32, f32) {
    let wobble = fbm(p * 4.0, 4, 11) - 0.5;
    let fine = fbm(p * 13.0, 3, 12) - 0.5;
    let (mut cover, mut dark) = (0.0f32, 0.0f32);
    for cap in MARIA {
        let c = selenographic(cap.0, cap.1);
        let r = cap.2.to_radians();
        let d = angle(p, c) + (wobble * 0.8 + fine * 0.2) * r.min(0.2);
        let edge = (r * 0.3).clamp(0.02, 0.08);
        let t = smoothstep(r + edge, r - edge, d);
        if t > 0.0 {
            cover = cover.max(t * cap.3.min(1.0));
            dark = dark.max(t * cap.3);
        }
    }
    (cover, dark)
}

/// Cumulative crater counts follow N(>r) ∝ r^-SLOPE between these radii (radians).
const R_MIN: f32 = 0.0032;
const R_MAX: f32 = 0.11;
const SLOPE: f32 = 2.0;

fn random_craters(seed: u32) -> Vec<Crater> {
    let mut rng = Rng(seed);
    let mut out = Vec::new();
    let count = 36_000;
    let tail = (R_MIN / R_MAX).powf(SLOPE);
    for i in 0..count {
        // Inverse-CDF of the truncated power law.
        let u = rng.next();
        let r = R_MIN * (1.0 - u * (1.0 - tail)).powf(-1.0 / SLOPE);
        let centre = rng.unit();
        let age = rng.next();
        // The maria are younger: most craters there were flooded.
        let (cover, _) = maria(centre);
        if cover > 0.5 && rng.next() < 0.78 {
            continue;
        }
        // A few young craters, more often small ones.
        let fresh = if r < 0.03 && age > 0.985 - 0.02 * (R_MIN / r) {
            0.6 + 0.4 * rng.next()
        } else {
            age * 0.35
        };
        out.push(Crater {
            centre,
            r,
            fresh,
            dark_floor: 0.0,
            seed: seed ^ (i as u32).wrapping_mul(2_654_435_761),
        });
    }
    // Large first so small craters sit on top of big ones.
    out.sort_by(|a, b| b.r.total_cmp(&a.r));
    out
}

fn named_craters() -> Vec<Crater> {
    NAMED
        .iter()
        .enumerate()
        .map(|(i, n)| Crater {
            centre: selenographic(n.0, n.1),
            r: n.2 / RADIUS_KM,
            fresh: n.3,
            dark_floor: n.4,
            seed: 9_000 + i as u32,
        })
        .collect()
}

/// Wrap an angle to -π..π.
fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    a - (a / t).round() * t
}

fn stamp(canvas: &mut Canvas, c: &Crater) {
    let mut rng = Rng(c.seed);
    let r = c.r;
    // Depth relative to the radius: simple bowls are deep, large complex craters shallower.
    let depth = r * if r < 0.009 { 0.36 } else { 0.22 };
    let rim = depth * 0.32;
    let peak = if r > 0.012 { depth * 0.35 } else { 0.0 };
    // Rays on young craters.
    let rays: Vec<(f32, f32, f32)> = if c.fresh > 0.6 && r > 0.004 {
        let n = 10 + (rng.next() * 14.0) as usize;
        (0..n)
            .map(|_| {
                let theta = rng.next() * std::f32::consts::TAU;
                let width = 0.05 + rng.next() * 0.12;
                let length = r * (6.0 + rng.next() * 16.0) * (0.5 + c.fresh * 0.6);
                (theta, width, length)
            })
            .collect()
    } else {
        Vec::new()
    };
    let ray_reach = rays.iter().map(|ray| ray.2).fold(0.0f32, f32::max);
    let reach = (r * 2.6).max(ray_reach);
    // Tangent frame at the centre (bearings for the rays).
    let east = Vec3::Y.cross(c.centre).normalize_or(Vec3::X);
    let north = c.centre.cross(east);
    let seed = c.seed;
    let wobble_seed = seed & 0xffff;
    canvas.around(c.centre, reach, |cv, i, p| {
        let d = angle(p, c.centre);
        // Rim outline wobbles a little (no perfect circles).
        let q = d / r * (1.0 + (noise(p * (8.0 / r.max(0.004)), wobble_seed) - 0.5) * 0.12);
        // ---- Height ----
        let mut h = 0.0;
        if q < 1.0 {
            h += -depth * (1.0 - q * q) + rim * q.powi(6);
            if peak > 0.0 {
                h += peak * (-(q / 0.16).powi(2)).exp();
            }
            if c.dark_floor > 0.0 {
                // Flooded floor: flat.
                let flat = smoothstep(0.85, 0.6, q);
                h = h * (1.0 - flat) + (-depth * 0.55) * flat;
            }
        } else if q < 2.6 {
            h += rim * (-(q - 1.0) * 3.2).exp();
        }
        cv.height[i] += h;
        // ---- Albedo ----
        let mut a = 0.0;
        let fresh = c.fresh;
        if q < 1.0 {
            // Bright inner walls on young craters, slightly darker old floors.
            a += fresh * fresh * (0.16 * smoothstep(0.3, 0.95, q) + 0.06);
            a -= (1.0 - fresh) * 0.025 * smoothstep(0.9, 0.2, q);
            a -= c.dark_floor * 0.3 * smoothstep(0.92, 0.7, q);
        }
        // Rim and ejecta blanket.
        a += (0.008 + fresh * 0.1) * (-((q - 1.0) / 0.12).powi(2)).exp();
        if q > 1.0 {
            a += fresh * fresh * 0.12 * (-(q - 1.0) * 2.2).exp();
        }
        if !rays.is_empty() && q > 0.9 {
            let v = p - c.centre;
            let bearing = v.dot(north).atan2(v.dot(east));
            let mut ray = 0.0;
            for &(theta, width, length) in &rays {
                if d < length {
                    let b = wrap(bearing - theta) / width;
                    let fade = 1.0 - d / length;
                    ray += (-b * b).exp() * fade * fade;
                }
            }
            // Streaky breakup along the rays.
            let streak = fbm(p * 60.0, 3, seed & 0xfff) * 1.6 - 0.3;
            a += 0.3 * fresh * ray.min(1.4) * streak.clamp(0.0, 1.0);
        }
        cv.albedo[i] += a;
    });
}

/// The texture as (albedo, height) floats, `w` × `w / 2`.
fn build(w: usize) -> Canvas {
    let h = w / 2;
    let mut cv = Canvas::new(w, h);
    // Highlands and maria.
    for i in 0..w * h {
        let p = cv.dirs[i];
        let broad = fbm(p * 3.0, 4, 1);
        let mid = fbm(p * 14.0, 4, 2);
        let fine = fbm(p * 70.0, 3, 3);
        let highland = 0.64 + (broad - 0.5) * 0.16 + (mid - 0.5) * 0.12 + (fine - 0.5) * 0.08;
        let (cover, dark) = maria(p);
        let mare_tone = 0.37 + (fbm(p * 5.0, 5, 4) - 0.5) * 0.2 + (fine - 0.5) * 0.05;
        let mare = mare_tone + (1.0 - dark.min(1.2)) * 0.18;
        cv.albedo[i] = highland + (mare - highland) * cover;
        // Highlands are rough and higher; maria smooth and low.
        let rough = (mid - 0.5) * 0.004 + (fine - 0.5) * 0.0015 + (broad - 0.5) * 0.01;
        cv.height[i] = rough * (1.0 - 0.8 * cover) - 0.004 * cover;
    }
    for c in random_craters(0x6d6f_6f6e) {
        stamp(&mut cv, &c);
    }
    for c in named_craters() {
        stamp(&mut cv, &c);
    }
    cv
}

/// Height (radians of the radius) per alpha step: alpha 128 ± 127 covers ± this × 127.
pub const HEIGHT_SCALE: f32 = 0.00012;

/// The texture: luminance = albedo (sRGB), alpha = height.
pub fn generate(w: u32) -> image::GrayAlphaImage {
    let cv = build(w as usize);
    image::GrayAlphaImage::from_fn(w, w / 2, |x, y| {
        let i = y as usize * cv.w + x as usize;
        let a = (cv.albedo[i].clamp(0.0, 1.0) * 255.0).round() as u8;
        let h = (128.0 + cv.height[i] / HEIGHT_SCALE)
            .clamp(0.0, 255.0)
            .round() as u8;
        image::LumaA([a, h])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/moon/assets/moon.png")
    }

    #[test]
    #[ignore = "writes src/moon/assets/moon.png; run on purpose to regenerate"]
    fn write_moon_texture() {
        let img = generate(WIDTH);
        std::fs::create_dir_all(path().parent().unwrap()).unwrap();
        img.save(path()).unwrap();
    }

    /// The committed PNG is what the generator makes (a level or two of rounding is allowed for
    /// another machine's maths library).
    #[test]
    fn moon_texture_matches_its_generator() {
        let made = generate(WIDTH);
        let committed = image::load_from_memory(super::super::texture::BUILT_IN_PNG)
            .unwrap()
            .into_luma_alpha8();
        assert_eq!(committed.dimensions(), made.dimensions());
        let (mut sum, mut worst) = (0u64, 0u8);
        for (a, b) in committed.pixels().zip(made.pixels()) {
            for c in 0..2 {
                let d = a[c].abs_diff(b[c]);
                sum += d as u64;
                worst = worst.max(d);
            }
        }
        let mean = sum as f64 / (made.width() * made.height() * 2) as f64;
        assert!(
            mean < 0.05 && worst <= 4,
            "mean {mean}, worst {worst}: regenerate with write_moon_texture"
        );
    }

    /// Maria are darker than highlands, and the near side has the familiar seas where they
    /// belong: Imbrium and Tranquillitatis dark, the southern highlands bright.
    #[test]
    fn maria_are_where_the_real_ones_are() {
        let img = image::load_from_memory(super::super::texture::BUILT_IN_PNG)
            .unwrap()
            .into_luma_alpha8();
        let (w, h) = img.dimensions();
        // Mean albedo in a small window around a selenographic point.
        let at = |lat: f32, lon: f32| {
            let cx = ((lon + 180.0) / 360.0 * w as f32) as i32;
            let cy = ((90.0 - lat) / 180.0 * h as f32) as i32;
            let mut s = 0.0;
            for dy in -6..=6 {
                for dx in -6..=6 {
                    s += img.get_pixel((cx + dx) as u32, (cy + dy) as u32)[0] as f32;
                }
            }
            s / 169.0
        };
        let imbrium = at(30.0, -14.0);
        let tranquillitatis = at(6.0, 33.0);
        let highlands = at(-30.0, 10.0);
        assert!(imbrium < highlands - 40.0, "{imbrium} vs {highlands}");
        assert!(tranquillitatis < highlands - 40.0);
    }
}
