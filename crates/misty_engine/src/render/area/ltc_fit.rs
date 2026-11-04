//! Fitting linearly transformed cosines (LTC) to the GGX lobe.
//!
//! A clamped cosine D₀(ω₀) = max(ω₀.z, 0) / π, pushed through a 3×3 matrix M, becomes the
//! distribution D(ω) = D₀(ω₀) · |det M⁻¹| / |M⁻¹ω|³ with ω₀ = M⁻¹ω / |M⁻¹ω|. Integrating D over a
//! polygon equals integrating D₀ over the polygon transformed by M⁻¹, which has a closed form.
//! For each roughness and view angle we look for the M⁻¹ whose D best matches the normalised GGX
//! lobe (BRDF × cos / its integral), minimising the cubed difference with a simplex search. The
//! error is estimated with samples from both distributions (balance heuristic) on fixed grids,
//! so the fit is deterministic. The lobe's integral is split by Schlick's Fresnel weight.

use std::f64::consts::PI;

use glam::{DMat3, DVec3};

use super::ggx::{Ggx, grid, schlick_weight};
use super::nelder_mead;

/// Table resolution along roughness and view angle.
pub const SIZE: usize = 64;
/// Smallest GGX α in the table (the mesh shader's minimum roughness).
pub const MIN_ALPHA: f64 = 0.002;
/// Grazing views are clamped to this cosine.
const MIN_COS: f64 = 0.001;
/// Samples per strategy: GRID² from the BRDF and as many from the LTC.
const GRID: usize = 24;

/// Inverse LTC matrix of an isotropic lobe with the view in the XZ plane, scaled so its middle
/// entry is 1: rows [a 0 b], [0 1 0], [d 0 e].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ltc {
    pub a: f64,
    pub b: f64,
    pub d: f64,
    pub e: f64,
}

impl Ltc {
    /// The plain clamped cosine (α = 1 at normal incidence).
    pub const COSINE: Ltc = Ltc {
        a: 1.0,
        b: 0.0,
        d: 0.0,
        e: 1.0,
    };

    pub fn inverse_matrix(&self) -> DMat3 {
        DMat3::from_cols(
            DVec3::new(self.a, 0.0, self.d),
            DVec3::Y,
            DVec3::new(self.b, 0.0, self.e),
        )
    }

    fn valid(&self) -> bool {
        let det = self.a * self.e - self.b * self.d;
        [self.a, self.b, self.d, self.e]
            .iter()
            .all(|v| v.is_finite())
            && det > 1e-12
    }

    /// Density of the distribution in direction `l` (unit).
    pub fn eval(&self, l: DVec3) -> f64 {
        let minv = self.inverse_matrix();
        let lo = minv * l;
        let len = lo.length();
        let cos = lo.z / len;
        if cos <= 0.0 {
            return 0.0;
        }
        cos / PI * minv.determinant() / (len * len * len)
    }

    /// A direction of the distribution (`u` in 0..1): a cosine sample pushed through M.
    pub fn sample(&self, m: &DMat3, u1: f64, u2: f64) -> DVec3 {
        let r = u1.sqrt();
        let phi = 2.0 * PI * u2;
        let lo = DVec3::new(r * phi.cos(), r * phi.sin(), (1.0 - u1).sqrt());
        (*m * lo).normalize()
    }

    fn to_array(self) -> [f64; 4] {
        [self.a, self.b, self.d, self.e]
    }

    fn from_array(p: &[f64; 4]) -> Self {
        Ltc {
            a: p[0],
            b: p[1],
            d: p[2],
            e: p[3],
        }
    }

    /// The same matrix after a round trip through the f32 table.
    pub fn rounded(self) -> Self {
        let r = |v: f64| v as f32 as f64;
        Ltc {
            a: r(self.a),
            b: r(self.b),
            d: r(self.d),
            e: r(self.e),
        }
    }
}

/// One table cell: the fitted matrix and the lobe's integral split as
/// ∫ f·cos = `scale` + `bias` with Fresnel F = F₀ + (1 - F₀)(1 - v·h)⁵ giving F₀·scale + bias.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ltc: Ltc,
    pub scale: f64,
    pub bias: f64,
}

/// GGX α of roughness column `i` (perceptual roughness i / (SIZE - 1), squared).
pub fn alpha_at(i: usize) -> f64 {
    let r = i as f64 / (SIZE - 1) as f64;
    (r * r).max(MIN_ALPHA)
}

/// View direction of row `j` (row parameter √(1 - cos θ)).
pub fn view_at(j: usize) -> DVec3 {
    let t = j as f64 / (SIZE - 1) as f64;
    let cos = (1.0 - t * t).max(MIN_COS);
    DVec3::new((1.0 - cos * cos).sqrt(), 0.0, cos)
}

/// ∫ f·cos split by the Fresnel weight, by sampling the BRDF.
fn integrals(brdf: &Ggx, v: DVec3) -> (f64, f64) {
    let (mut scale, mut bias) = (0.0, 0.0);
    for (u1, u2) in grid(GRID * 2) {
        let l = brdf.sample(v, u1, u2);
        let (value, pdf) = brdf.eval(v, l);
        if pdf > 0.0 {
            let w = schlick_weight(v, l);
            scale += value / pdf * (1.0 - w);
            bias += value / pdf * w;
        }
    }
    let n = (GRID * GRID * 4) as f64;
    (scale / n, bias / n)
}

/// Cubed difference between the normalised lobe and the LTC, sampled from both.
fn error(ltc: &Ltc, brdf: &Ggx, v: DVec3, norm: f64) -> f64 {
    if !ltc.valid() {
        return f64::MAX;
    }
    let m = ltc.inverse_matrix().inverse();
    let mut sum = 0.0;
    let mut add = |l: DVec3| {
        let (f, pdf_f) = brdf.eval(v, l);
        let g = ltc.eval(l);
        let pdf = pdf_f + g;
        if pdf > 0.0 {
            sum += (f / norm - g).abs().powi(3) / pdf;
        }
    };
    for (u1, u2) in grid(GRID) {
        let l = brdf.sample(v, u1, u2);
        if l.z > 0.0 {
            add(l);
        }
        add(ltc.sample(&m, u1, u2));
    }
    sum / (2 * GRID * GRID) as f64
}

/// Fit one cell starting from `start` (the neighbouring cell's matrix).
pub fn fit_cell(i: usize, j: usize, start: Ltc) -> Cell {
    let brdf = Ggx { alpha: alpha_at(i) };
    let v = view_at(j);
    let (scale, bias) = integrals(&brdf, v);
    let norm = (scale + bias).max(1e-12);
    let f = |p: &[f64; 4]| error(&Ltc::from_array(p), &brdf, v, norm);
    let mut best = start.to_array();
    // Two rounds: a fresh simplex around the first result escapes early collapse.
    for _ in 0..2 {
        let step = best.map(|x| 0.05 * x.abs().max(0.05 * brdf.alpha + 0.01));
        best = nelder_mead::minimize(f, best, step, 400).0;
    }
    Cell {
        ltc: Ltc::from_array(&best).rounded(),
        scale,
        bias,
    }
}

/// The start of every roughness column: a lobe shrunk around the normal.
pub fn normal_incidence(i: usize) -> Ltc {
    Ltc {
        e: alpha_at(i),
        ..Ltc::COSINE
    }
}

/// Fit a whole roughness column, from normal incidence towards grazing, each cell starting
/// from the previous (stored, f32-rounded) one.
pub fn fit_column(i: usize) -> Vec<Cell> {
    let mut cells: Vec<Cell> = Vec::with_capacity(SIZE);
    for j in 0..SIZE {
        let start = cells.last().map_or(normal_incidence(i), |c| c.ltc);
        cells.push(fit_cell(i, j, start));
    }
    cells
}
