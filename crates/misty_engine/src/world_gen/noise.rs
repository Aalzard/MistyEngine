//! Seeded 2D noise for world generation: gradient noise, fractal sums (fBm, ridged, billow),
//! domain warping and a small deterministic random generator.

use serde::{Deserialize, Serialize};

/// Integer hash of a lattice point (well mixed, stable across platforms).
pub fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

/// Hash as 0..1.
pub fn hash01(x: i32, y: i32, seed: u32) -> f32 {
    (hash(x, y, seed) >> 8) as f32 / (1u32 << 24) as f32
}

/// One of 8 unit gradients for a lattice point, dotted with the offset.
fn grad(ix: i32, iy: i32, seed: u32, dx: f32, dy: f32) -> f32 {
    const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let g = match hash(ix, iy, seed) & 7 {
        0 => (1.0, 0.0),
        1 => (-1.0, 0.0),
        2 => (0.0, 1.0),
        3 => (0.0, -1.0),
        4 => (D, D),
        5 => (-D, D),
        6 => (D, -D),
        _ => (-D, -D),
    };
    g.0 * dx + g.1 * dy
}

/// Gradient (Perlin-style) noise, roughly -1..1.
pub fn gradient(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ix, iy) = (x0 as i32, y0 as i32);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v) = (fade(fx), fade(fy));
    let a = grad(ix, iy, seed, fx, fy);
    let b = grad(ix + 1, iy, seed, fx - 1.0, fy);
    let c = grad(ix, iy + 1, seed, fx, fy - 1.0);
    let d = grad(ix + 1, iy + 1, seed, fx - 1.0, fy - 1.0);
    let top = a + (b - a) * u;
    let bottom = c + (d - c) * u;
    (top + (bottom - top) * v) * 1.41
}

/// How octaves are combined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoiseKind {
    /// Plain fractal sum (soft hills).
    #[default]
    Fbm,
    /// 1 - |n|, sharpened: crests (mountain ridges).
    Ridged,
    /// |n|: rounded bumps with creases between them (dunes, clouds).
    Billow,
}

/// Fractal noise settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fractal {
    pub kind: NoiseKind,
    pub octaves: u32,
    pub lacunarity: f32,
    pub gain: f32,
    /// Ridged: exponent on the crest (higher = thinner, sharper ridges).
    pub sharpness: f32,
}

impl Default for Fractal {
    fn default() -> Self {
        Self {
            kind: NoiseKind::Fbm,
            octaves: 5,
            lacunarity: 2.0,
            gain: 0.5,
            sharpness: 2.0,
        }
    }
}

/// Per-octave rotation (sin, cos of 0.5 × octave), computed once.
static ROTATIONS: std::sync::LazyLock<[(f32, f32); 16]> =
    std::sync::LazyLock::new(|| std::array::from_fn(|o| (0.5f32 * o as f32).sin_cos()));

/// Fractal noise: Fbm in -1..1, Ridged and Billow in 0..1.
pub fn fractal(x: f32, y: f32, seed: u32, f: &Fractal) -> f32 {
    let rotations = &*ROTATIONS;
    let (mut sum, mut amp, mut norm, mut freq) = (0.0, 1.0, 0.0, 1.0);
    // Ridged octaves are weighted by the previous one, so crests carry the detail.
    let mut weight = 1.0f32;
    for o in 0..f.octaves.max(1) {
        // Rotate each octave a little to hide the lattice.
        let (s, c) = match rotations.get(o as usize) {
            Some(r) => *r,
            None => (0.5f32 * o as f32).sin_cos(),
        };
        let (px, py) = (x * c - y * s, x * s + y * c);
        let n = gradient(px * freq, py * freq, seed.wrapping_add(o * 1013));
        let v = match f.kind {
            NoiseKind::Fbm => n,
            NoiseKind::Billow => n.abs(),
            NoiseKind::Ridged => {
                let r = (1.0 - n.abs()).max(0.0).powf(f.sharpness.max(0.5));
                let v = r * weight;
                weight = (r * 1.5).clamp(0.0, 1.0);
                v
            }
        };
        sum += v * amp;
        norm += amp;
        amp *= f.gain;
        freq *= f.lacunarity;
    }
    sum / norm
}

/// Domain-warped fractal noise: the lookup point is pushed around by two fBm fields, which bends
/// ridges and valleys into natural, less grid-like shapes. `warp` is in noise units (0 = none).
pub fn warped(x: f32, y: f32, seed: u32, f: &Fractal, warp: f32) -> f32 {
    if warp <= 0.0 {
        return fractal(x, y, seed, f);
    }
    let w = Fractal {
        kind: NoiseKind::Fbm,
        octaves: 3,
        ..*f
    };
    let qx = fractal(x + 5.2, y + 1.3, seed ^ 0x51ed, &w);
    let qy = fractal(x + 1.7, y + 9.2, seed ^ 0x2f1a, &w);
    fractal(x + warp * qx, y + warp * qy, seed, f)
}

/// Small deterministic random generator (SplitMix64).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

/// Smooth 0..1 step between `a` and `b` (either order).
pub fn smoothstep(a: f32, b: f32, v: f32) -> f32 {
    if (b - a).abs() < 1e-9 {
        return if v < a { 0.0 } else { 1.0 };
    }
    let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let f = Fractal::default();
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.11);
            let a = fractal(x, y, 7, &f);
            assert_eq!(a, fractal(x, y, 7, &f));
            assert!((-1.01..=1.01).contains(&a), "{a}");
            let r = fractal(
                x,
                y,
                7,
                &Fractal {
                    kind: NoiseKind::Ridged,
                    ..f
                },
            );
            assert!((0.0..=1.0).contains(&r), "{r}");
        }
        assert_ne!(fractal(0.3, 0.4, 1, &f), fractal(0.3, 0.4, 2, &f));
        let mut a = Rng::new(5);
        let mut b = Rng::new(5);
        assert_eq!(a.next_u64(), b.next_u64());
    }
}
