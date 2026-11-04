//! GGX prefiltering of a captured cubemap into a roughness mip chain (split-sum, N = V = R):
//! level i holds perceptual roughness i / (LEVELS - 1). Filtered importance sampling reads a
//! box-filtered source mip matched to each sample's solid angle, so few samples stay smooth.

use glam::Vec3;

use super::cube::{Cube, direction, texel_uv};

/// Roughness levels per probe (the shader's `PROBE_LEVELS`).
pub const LEVELS: usize = 7;

/// Perceptual roughness stored in level `i`.
pub fn level_roughness(i: usize) -> f32 {
    i as f32 / (LEVELS - 1) as f32
}

/// Van der Corput / Hammersley point `i` of `n`.
pub fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (
        i as f32 / n as f32,
        i.reverse_bits() as f32 * 2.328_306_4e-10,
    )
}

/// GGX half vector around `n` for GGX alpha `a` (= perceptual²).
pub fn importance_ggx(xi: (f32, f32), n: Vec3, a: f32) -> Vec3 {
    let phi = std::f32::consts::TAU * xi.0;
    let cos_t = ((1.0 - xi.1) / (1.0 + (a * a - 1.0) * xi.1))
        .max(0.0)
        .sqrt();
    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
    let h = Vec3::new(sin_t * phi.cos(), sin_t * phi.sin(), cos_t);
    let up = if n.z.abs() < 0.999 { Vec3::Z } else { Vec3::X };
    let t = up.cross(n).normalize();
    let b = n.cross(t);
    (t * h.x + b * h.y + n * h.z).normalize()
}

/// GGX normal distribution.
pub fn d_ggx(n_dot_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    a2 / (std::f32::consts::PI * d * d)
}

/// Trilinear lookup in a mip chain.
fn sample_chain(chain: &[Cube], d: Vec3, lod: f32) -> Vec3 {
    let lod = lod.clamp(0.0, (chain.len() - 1) as f32);
    let (lo, t) = (lod.floor() as usize, lod.fract());
    let a = chain[lo].sample(d);
    if t < 1e-3 || lo + 1 >= chain.len() {
        return a;
    }
    a.lerp(chain[lo + 1].sample(d), t)
}

/// One prefiltered texel: GGX-weighted average of the environment around `n`.
fn filter_texel(chain: &[Cube], n: Vec3, a: f32, samples: u32) -> Vec3 {
    let base = chain[0].size as f32;
    let texel_angle = 4.0 * std::f32::consts::PI / (6.0 * base * base);
    let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
    for i in 0..samples {
        let h = importance_ggx(hammersley(i, samples), n, a);
        let n_dot_h = n.dot(h);
        let l = h * (2.0 * n_dot_h) - n;
        let n_dot_l = n.dot(l);
        if n_dot_l <= 0.0 {
            continue;
        }
        // pdf = D · NdotH / (4 VdotH) with V = N.
        let pdf = d_ggx(n_dot_h, a) * 0.25;
        let sample_angle = 1.0 / (samples as f32 * pdf + 1e-6);
        let lod = 0.5 * (sample_angle / texel_angle).log2() + 1.0;
        sum += sample_chain(chain, l, lod) * n_dot_l;
        weight += n_dot_l;
    }
    sum / weight.max(1e-6)
}

/// The roughness chain of `base` (level 0 = `base` itself). `samples` per texel; work is split
/// over the CPU's threads by face.
pub fn prefilter(base: &Cube, samples: u32) -> Vec<Cube> {
    let mut chain = vec![base.clone()];
    while chain.last().is_some_and(|c| c.size > 1) {
        let next = chain.last().expect("non-empty").downsample();
        chain.push(next);
    }
    let mut levels = vec![base.clone()];
    for i in 1..LEVELS {
        let size = (base.size >> i).max(1);
        let a = level_roughness(i).powi(2).max(1e-3);
        let mut out = Cube::new(size);
        std::thread::scope(|scope| {
            for (face, texels) in out.faces.iter_mut().enumerate() {
                let chain = &chain;
                scope.spawn(move || {
                    for y in 0..size {
                        for x in 0..size {
                            let (u, v) = texel_uv(size, x, y);
                            let n = direction(face, u, v);
                            texels[(y * size + x) as usize] = filter_texel(chain, n, a, samples);
                        }
                    }
                });
            }
        });
        levels.push(out);
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_stays_constant_and_rough_levels_blur() {
        let c = Cube::from_fn(32, |_| Vec3::splat(0.7));
        let levels = prefilter(&c, 32);
        assert_eq!(levels.len(), LEVELS);
        assert_eq!(levels[1].size, 16);
        for l in &levels {
            assert!((l.sample(Vec3::new(0.3, 0.2, -0.9)) - Vec3::splat(0.7)).length() < 1e-3);
        }
        // A bright spot spreads out with roughness and keeps its energy roughly.
        let spot = Cube::from_fn(64, |d| Vec3::splat(if d.y > 0.95 { 20.0 } else { 0.0 }));
        let levels = prefilter(&spot, 64);
        let side = Vec3::new(0.6, 0.8, 0.0).normalize();
        assert!(levels[1].sample(side).x < levels[5].sample(side).x);
        assert!(levels[1].sample(Vec3::Y).x > levels[5].sample(Vec3::Y).x);
        let ratio = levels[3].average().x / spot.average().x;
        assert!((0.6..1.4).contains(&ratio), "{ratio}");
    }
}
