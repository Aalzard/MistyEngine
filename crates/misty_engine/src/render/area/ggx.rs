//! Isotropic GGX microfacet BRDF with height-correlated Smith masking, in a shading frame with
//! the normal along +Z and the view in the XZ plane: the lobe the LTC tables are fitted to.
//! Values are BRDF × cos(light), so the lobe integrates to the directional albedo.

use std::f64::consts::PI;

use glam::DVec3;

#[derive(Clone, Copy, Debug)]
pub struct Ggx {
    /// GGX α (perceptual roughness squared).
    pub alpha: f64,
}

impl Ggx {
    fn d(&self, n_dot_h: f64) -> f64 {
        let a2 = self.alpha * self.alpha;
        let t = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
        a2 / (PI * t * t)
    }

    /// Smith Λ of a direction with this cosine to the normal.
    fn lambda(&self, cos: f64) -> f64 {
        let c2 = (cos * cos).max(1e-12);
        let tan2 = (1.0 - c2) / c2;
        (-1.0 + (1.0 + self.alpha * self.alpha * tan2).sqrt()) * 0.5
    }

    /// (BRDF × cos(l), pdf of [`Ggx::sample`] producing `l`); zero below the horizon.
    pub fn eval(&self, v: DVec3, l: DVec3) -> (f64, f64) {
        if l.z <= 0.0 || v.z <= 0.0 {
            return (0.0, 0.0);
        }
        let h = (v + l).normalize();
        let d = self.d(h.z);
        let g2 = 1.0 / (1.0 + self.lambda(v.z) + self.lambda(l.z));
        let value = d * g2 / (4.0 * v.z);
        let pdf = d * h.z / (4.0 * v.dot(h).max(1e-12));
        (value, pdf)
    }

    /// `v` reflected about a half vector drawn with density D(h)·cos(h) (`u` in 0..1).
    pub fn sample(&self, v: DVec3, u1: f64, u2: f64) -> DVec3 {
        let phi = 2.0 * PI * u2;
        let tan2 = self.alpha * self.alpha * u1 / (1.0 - u1);
        let cos = 1.0 / (1.0 + tan2).sqrt();
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let h = DVec3::new(sin * phi.cos(), sin * phi.sin(), cos);
        2.0 * v.dot(h) * h - v
    }
}

/// Schlick's Fresnel weight (1 - v·h)⁵ of the half vector between `v` and `l`.
pub fn schlick_weight(v: DVec3, l: DVec3) -> f64 {
    let h = (v + l).normalize();
    (1.0 - v.dot(h).clamp(0.0, 1.0)).powi(5)
}

/// Midpoints of an `n` × `n` grid on the unit square (a deterministic sample set).
pub fn grid(n: usize) -> impl Iterator<Item = (f64, f64)> {
    (0..n * n).map(move |k| {
        let (i, j) = (k % n, k / n);
        ((i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64)
    })
}
