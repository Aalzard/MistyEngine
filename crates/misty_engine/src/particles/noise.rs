//! Smooth 3D gradient noise for the Noise module: three decorrelated channels, fractal
//! octaves, a scroll offset. Pure functions of their inputs (the same everywhere).

use glam::Vec3;

fn hash(x: i32, y: i32, z: i32, channel: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f)
        ^ channel.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

/// A pseudo-random unit-ish gradient of a lattice corner.
fn gradient(x: i32, y: i32, z: i32, channel: u32) -> Vec3 {
    let h = hash(x, y, z, channel);
    let f = |s: u32| ((h >> s) & 0x3ff) as f32 / 511.5 - 1.0;
    Vec3::new(f(0), f(10), f(20))
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Gradient noise of one channel at `p`, about -1..1.
pub fn gradient_noise(p: Vec3, channel: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let (ix, iy, iz) = (i.x as i32, i.y as i32, i.z as i32);
    let u = Vec3::new(fade(f.x), fade(f.y), fade(f.z));
    let corner = |dx: i32, dy: i32, dz: i32| {
        let g = gradient(ix + dx, iy + dy, iz + dz, channel);
        g.dot(f - Vec3::new(dx as f32, dy as f32, dz as f32))
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), u.x);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), u.x);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), u.x);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), u.x);
    let v = lerp(lerp(x00, x10, u.y), lerp(x01, x11, u.y), u.z);
    (v * 1.6).clamp(-1.0, 1.0)
}

/// Three channels of fractal noise at `p` (already scaled by the frequency), shifted by
/// `scroll` along a diagonal: octave k is `scale^k` × finer and `multiplier^k` × weaker, and
/// the sum is normalised back to about -1..1.
pub fn field(p: Vec3, scroll: f32, octaves: u32, multiplier: f32, scale: f32) -> Vec3 {
    let mut out = Vec3::ZERO;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut total = 0.0;
    let offset = Vec3::new(scroll, scroll * 0.7, scroll * 1.3);
    for k in 0..octaves.max(1) {
        let q = (p + offset) * freq + Vec3::splat(k as f32 * 17.31);
        out += Vec3::new(
            gradient_noise(q, 0),
            gradient_noise(q + Vec3::new(31.4, 7.7, 19.3), 1),
            gradient_noise(q + Vec3::new(-12.1, 45.2, 3.9), 2),
        ) * amp;
        total += amp;
        amp *= multiplier.clamp(0.0, 1.0);
        freq *= scale.max(1.0);
    }
    out / total.max(1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_smooth_bounded_and_varies() {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        let mut prev = gradient_noise(Vec3::new(0.0, 0.37, -1.2), 0);
        for i in 1..4000 {
            let p = Vec3::new(i as f32 * 0.002, 0.37, -1.2);
            let v = gradient_noise(p, 0);
            assert!((-1.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.04, "continuous at {p}: {prev} → {v}");
            prev = v;
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(hi - lo > 0.5, "varies: {lo}..{hi}");
        // Lattice points are zero (gradient noise) and channels differ.
        assert_eq!(gradient_noise(Vec3::new(3.0, -2.0, 5.0), 0), 0.0);
        let p = Vec3::new(0.3, 0.6, 0.2);
        assert_ne!(gradient_noise(p, 0), gradient_noise(p, 1));
        // Same input, same output; scrolling moves the field.
        let f = field(p, 0.0, 3, 0.5, 2.0);
        assert_eq!(f, field(p, 0.0, 3, 0.5, 2.0));
        assert_ne!(f, field(p, 0.5, 3, 0.5, 2.0));
        assert!(f.abs().max_element() <= 1.0);
    }
}
