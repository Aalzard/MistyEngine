//! Split-sum environment BRDF lookup table: for (N·V, perceptual roughness) the scale A and bias
//! B with ∫ specular = F0 · A + B (Smith-GGX visibility, Schlick Fresnel).

use glam::Vec3;

use super::prefilter::{hammersley, importance_ggx};

/// LUT width and height.
pub const LUT_SIZE: u32 = 32;
const SAMPLES: u32 = 256;

/// Smith-GGX geometry term with the image-based-lighting k = α / 2.
fn g_smith(n_dot_v: f32, n_dot_l: f32, a: f32) -> f32 {
    let k = a * 0.5;
    let g = |x: f32| x / (x * (1.0 - k) + k);
    g(n_dot_v) * g(n_dot_l)
}

/// (A, B) for one view angle and roughness.
pub fn integrate(n_dot_v: f32, perceptual: f32) -> (f32, f32) {
    let n_dot_v = n_dot_v.max(1e-3);
    let a = (perceptual * perceptual).max(1e-3);
    let v = Vec3::new((1.0 - n_dot_v * n_dot_v).sqrt(), 0.0, n_dot_v);
    let (mut sa, mut sb) = (0.0, 0.0);
    for i in 0..SAMPLES {
        let h = importance_ggx(hammersley(i, SAMPLES), Vec3::Z, a);
        let v_dot_h = v.dot(h);
        let l = h * (2.0 * v_dot_h) - v;
        let (n_dot_l, n_dot_h) = (l.z, h.z.max(0.0));
        if n_dot_l <= 0.0 {
            continue;
        }
        let g_vis = g_smith(n_dot_v, n_dot_l, a) * v_dot_h.max(0.0) / (n_dot_h * n_dot_v).max(1e-6);
        let fc = (1.0 - v_dot_h.max(0.0)).powi(5);
        sa += (1.0 - fc) * g_vis;
        sb += fc * g_vis;
    }
    (sa / SAMPLES as f32, sb / SAMPLES as f32)
}

/// RGBA16F texels, row = roughness (0 at the top), column = N·V.
pub fn lut_rgba16() -> Vec<half::f16> {
    let mut out = Vec::with_capacity((LUT_SIZE * LUT_SIZE * 4) as usize);
    for y in 0..LUT_SIZE {
        for x in 0..LUT_SIZE {
            let nv = (x as f32 + 0.5) / LUT_SIZE as f32;
            let r = (y as f32 + 0.5) / LUT_SIZE as f32;
            let (a, b) = integrate(nv, r);
            out.extend([a, b, 0.0, 1.0].map(half::f16::from_f32));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        // Smooth and facing: almost all energy, no bias.
        let (a, b) = integrate(1.0, 0.0);
        assert!(a > 0.95 && b < 0.05, "{a} {b}");
        // Grazing smooth: Fresnel bias dominates.
        let (_, b) = integrate(0.05, 0.05);
        assert!(b > 0.3, "{b}");
        // Rough surfaces lose energy (single scattering).
        let (a, b) = integrate(0.5, 1.0);
        assert!(a + b < 0.8, "{}", a + b);
        assert_eq!(lut_rgba16().len(), (LUT_SIZE * LUT_SIZE * 4) as usize);
    }
}
