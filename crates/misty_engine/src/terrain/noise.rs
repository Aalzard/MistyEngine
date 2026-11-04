//! Seeded, deterministic 2D noise for terrain rules: a lattice hash, smooth value noise and
//! fractal (fBm) sums. Same seed and inputs give the same values on every machine.

/// Integer hash of a lattice point to 0..1.
pub fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// Smooth value noise, -1..1, one lattice cell per unit.
pub fn value(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    // Quintic fade: no visible grid creases.
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v) = (fade(fx), fade(fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let a = hash(ix, iy, seed);
    let b = hash(ix + 1, iy, seed);
    let c = hash(ix, iy + 1, seed);
    let d = hash(ix + 1, iy + 1, seed);
    let top = a + (b - a) * u;
    let bottom = c + (d - c) * u;
    (top + (bottom - top) * v) * 2.0 - 1.0
}

/// Fractal sum of `octaves` value-noise layers (gain 0.5, lacunarity 2), about -1..1.
/// `scale` is the feature size of the first octave in the same units as `x`/`y`.
pub fn fbm(x: f32, y: f32, scale: f32, octaves: u32, seed: u32) -> f32 {
    let inv = 1.0 / scale.max(1e-3);
    let (mut sum, mut amp, mut norm, mut f) = (0.0, 1.0, 0.0, inv);
    for o in 0..octaves.max(1) {
        // Rotate each octave a little so lattice axes don't line up.
        let (rx, ry) = (x * 0.8 - y * 0.6, x * 0.6 + y * 0.8);
        let (px, py) = if o % 2 == 0 { (x, y) } else { (rx, ry) };
        sum += amp * value(px * f + o as f32 * 17.3, py * f - o as f32 * 9.1, seed + o);
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// Ridged fBm (sharp crests), 0..1: good for mountain ranges.
pub fn ridged(x: f32, y: f32, scale: f32, octaves: u32, seed: u32) -> f32 {
    let inv = 1.0 / scale.max(1e-3);
    let (mut sum, mut amp, mut norm, mut f) = (0.0, 1.0, 0.0, inv);
    for o in 0..octaves.max(1) {
        let r = 1.0 - value(x * f + o as f32 * 31.7, y * f, seed + o).abs();
        sum += amp * r * r;
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        for i in 0..500 {
            let (x, y) = (i as f32 * 0.37 - 50.0, i as f32 * 0.91 + 3.0);
            let a = fbm(x, y, 8.0, 4, 7);
            assert_eq!(a, fbm(x, y, 8.0, 4, 7));
            assert!((-1.0..=1.0).contains(&a), "{a}");
            assert!((0.0..=1.0).contains(&ridged(x, y, 8.0, 4, 7)));
        }
        // Another seed gives another field.
        let differ = (0..100)
            .filter(|i| fbm(*i as f32, 0.5, 4.0, 3, 1) != fbm(*i as f32, 0.5, 4.0, 3, 2))
            .count();
        assert!(differ > 90);
    }

    #[test]
    fn value_noise_is_continuous() {
        // Tiny steps change the value by tiny amounts (no seams at lattice lines).
        let mut prev = value(0.0, 0.3, 5);
        for i in 1..2000 {
            let v = value(i as f32 * 0.002, 0.3, 5);
            assert!((v - prev).abs() < 0.02, "jump at {i}");
            prev = v;
        }
    }
}
