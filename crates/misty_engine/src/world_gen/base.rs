//! Base heightmap generators: domain-warped fractal noise shaped into a landform.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::noise::{Fractal, NoiseKind, fractal, smoothstep, warped};
use super::par_rows;

/// Landform the base heightmap starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Generator {
    #[default]
    RollingHills,
    Mountains,
    Plateaus,
    Canyons,
    Islands,
    Dunes,
}

impl Generator {
    pub const ALL: [Self; 6] = [
        Self::RollingHills,
        Self::Mountains,
        Self::Plateaus,
        Self::Canyons,
        Self::Islands,
        Self::Dunes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RollingHills => "Rolling Hills",
            Self::Mountains => "Mountains",
            Self::Plateaus => "Plateaus / Mesas",
            Self::Canyons => "Canyons",
            Self::Islands => "Islands",
            Self::Dunes => "Dunes",
        }
    }
}

/// Base heightmap settings (World Builder ▸ Base Heightmap).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BaseSettings {
    pub generator: Generator,
    /// Fraction (0..1) of the terrain's maximum height the landform reaches.
    pub height: f32,
    /// Size of the main features in metres.
    pub scale: f32,
    /// Domain warp and detail (0 = smooth and regular, 1 = very broken up).
    pub noise_amount: f32,
    /// Fractal octaves (roughness).
    pub octaves: u32,
    /// Mountain crest sharpness (ridged noise exponent).
    pub ridge_sharpness: f32,
    pub seed: u32,
}

impl Default for BaseSettings {
    fn default() -> Self {
        Self {
            generator: Generator::RollingHills,
            height: 0.5,
            scale: 250.0,
            noise_amount: 0.4,
            octaves: 6,
            ridge_sharpness: 2.0,
            seed: 1,
        }
    }
}

/// Normalised (0..1) heights, `res²` samples row by row along z, for a terrain of `size`.
pub fn generate(s: &BaseSettings, res: usize, size: Vec3) -> Vec<f32> {
    let mut out = vec![0.0; res * res];
    let cells = (res - 1).max(1) as f32;
    par_rows(&mut out, res, |iz, row| {
        for (ix, h) in row.iter_mut().enumerate() {
            let x = ix as f32 / cells * size.x;
            let z = iz as f32 / cells * size.z;
            *h = (sample(s, x, z, size) * s.height).clamp(0.0, 1.0);
        }
    });
    out
}

/// Landform height (about 0..1) at local metres (x, z).
pub fn sample(s: &BaseSettings, x: f32, z: f32, size: Vec3) -> f32 {
    let scale = s.scale.max(1.0);
    let (u, v) = (x / scale, z / scale);
    let warp = s.noise_amount.clamp(0.0, 2.0) * 1.2;
    let octaves = s.octaves.clamp(1, 12);
    let seed = s.seed;
    let fbm = Fractal {
        kind: NoiseKind::Fbm,
        octaves,
        ..Default::default()
    };
    let ridged = Fractal {
        kind: NoiseKind::Ridged,
        octaves,
        sharpness: s.ridge_sharpness,
        ..Default::default()
    };
    let billow = Fractal {
        kind: NoiseKind::Billow,
        octaves,
        ..Default::default()
    };
    let detail = |amount: f32| amount * s.noise_amount * fractal(u * 4.0, v * 4.0, seed ^ 77, &fbm);
    match s.generator {
        Generator::RollingHills => {
            let n = warped(u, v, seed, &fbm, warp) * 0.5 + 0.5;
            (0.15 + 0.75 * n * n + detail(0.05)).max(0.0)
        }
        Generator::Mountains => {
            let r = warped(u, v, seed, &ridged, warp);
            let base = warped(u * 0.5, v * 0.5, seed ^ 9, &fbm, warp) * 0.5 + 0.5;
            (r * r * (0.4 + 0.8 * base) + 0.05).min(1.2)
        }
        Generator::Plateaus => {
            let n = warped(u, v, seed, &fbm, warp) * 0.5 + 0.5;
            // Terraces: flat steps with steep risers.
            let steps = 4.0;
            let t = n * steps;
            let k = t.floor();
            let f = smoothstep(0.7, 0.95, t - k);
            ((k + f) / steps + detail(0.03)).max(0.0)
        }
        Generator::Canyons => {
            let n = warped(u, v, seed, &fbm, warp);
            // Deep, winding channels where the noise crosses zero; flat mesas between them.
            let channel = smoothstep(0.0, 0.22, n.abs());
            let mesa = 0.75 + 0.1 * (fractal(u * 0.7, v * 0.7, seed ^ 3, &fbm));
            (0.1 + (mesa - 0.1) * channel + detail(0.03)).max(0.0)
        }
        Generator::Islands => {
            // Distance from the centre (0..1 at the edge), pushed around so coasts are ragged.
            let cx = x / size.x.max(1.0) - 0.5;
            let cz = z / size.z.max(1.0) - 0.5;
            let wobble = 0.12 * fractal(u * 0.8, v * 0.8, seed ^ 41, &fbm) * (0.5 + s.noise_amount);
            let d = (cx * cx + cz * cz).sqrt() * 2.0 + wobble;
            let mask = 1.0 - smoothstep(0.45, 0.95, d);
            let r = warped(u, v, seed, &ridged, warp);
            let hills = warped(u, v, seed ^ 5, &fbm, warp) * 0.5 + 0.5;
            (mask * (0.18 + 0.5 * hills * mask + 0.55 * r * r * mask * mask)).max(0.0)
        }
        Generator::Dunes => {
            let bend = warped(u * 0.5, v * 0.5, seed, &fbm, warp) * 2.5;
            let phase = (u * 3.0 + bend) * std::f32::consts::PI;
            // Asymmetric crest: gentle windward side, steeper lee side.
            let s0 = super::portable::sin(phase) * 0.5 + 0.5;
            let crest = super::portable::powf(s0, 1.6);
            let swell = fractal(u * 0.4, v * 0.4, seed ^ 11, &billow);
            (0.15 + 0.45 * crest * (0.5 + swell) + 0.2 * swell + detail(0.02)).max(0.0)
        }
    }
}
