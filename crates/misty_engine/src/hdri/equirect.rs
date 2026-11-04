//! Linear RGB float images in the equirectangular (latitude / longitude) layout: decoding
//! `.hdr` / `.exr`, direction ↔ texel mapping, bilinear lookups and box downsampling.
//!
//! Mapping: u = 0.5 + atan2(x, z) / 2π (the image centre faces +Z, +X is to its right),
//! v = acos(y) / π (top row = straight up). The GPU conversion uses the same formulas.

use std::f32::consts::{PI, TAU};
use std::path::Path;

use glam::Vec3;

/// Widest image kept after decoding (larger files are box-filtered down).
pub const MAX_WIDTH: u32 = 4096;
/// Values above this are clamped (half-float textures top out at 65504).
pub const MAX_VALUE: f32 = 60000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct HdrImage {
    pub width: u32,
    pub height: u32,
    /// Rows top to bottom, linear RGB.
    pub pixels: Vec<Vec3>,
}

/// Unit direction of equirect coordinates `u`, `v` in 0..1.
pub fn direction(u: f32, v: f32) -> Vec3 {
    let phi = (u - 0.5) * TAU;
    let theta = v * PI;
    let s = theta.sin();
    Vec3::new(s * phi.sin(), theta.cos(), s * phi.cos())
}

/// Equirect coordinates (0..1) of a direction.
pub fn uv(d: Vec3) -> (f32, f32) {
    let d = d.normalize_or(Vec3::Z);
    let u = 0.5 + d.x.atan2(d.z) / TAU;
    let v = d.y.clamp(-1.0, 1.0).acos() / PI;
    (u, v)
}

impl HdrImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![Vec3::ZERO; (width * height) as usize],
        }
    }

    /// Image filled by `f(direction)` at each texel centre.
    pub fn from_fn(width: u32, height: u32, f: impl Fn(Vec3) -> Vec3 + Sync) -> Self {
        use rayon::prelude::*;
        let pixels = (0..width * height)
            .into_par_iter()
            .map(|i| {
                let (x, y) = (i % width, i / width);
                f(direction(
                    (x as f32 + 0.5) / width as f32,
                    (y as f32 + 0.5) / height as f32,
                ))
            })
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Decode a Radiance `.hdr` or OpenEXR file's bytes (NaN / negative texels become 0).
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| e.to_string())?
            .into_rgb32f();
        let (width, height) = img.dimensions();
        if width < 2 || height < 1 {
            return Err("image is empty".into());
        }
        let pixels = img
            .pixels()
            .map(|p| {
                let c = Vec3::from(p.0);
                if c.is_finite() {
                    c.clamp(Vec3::ZERO, Vec3::splat(MAX_VALUE))
                } else {
                    Vec3::ZERO
                }
            })
            .collect();
        Ok(Self {
            width,
            height,
            pixels,
        }
        .fit(MAX_WIDTH))
    }

    /// Decode a file through the asset file system (pak or disk).
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn get(&self, x: u32, y: u32) -> Vec3 {
        self.pixels[(y.min(self.height - 1) * self.width + x % self.width) as usize]
    }

    /// Bilinear lookup in direction `d` (wraps horizontally, clamps at the poles).
    pub fn sample(&self, d: Vec3) -> Vec3 {
        let (u, v) = uv(d);
        let fx = u * self.width as f32 - 0.5;
        let fy = (v * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let w = self.width as i64;
        let x0i = (x0 as i64).rem_euclid(w) as u32;
        let x1i = (x0i + 1) % self.width;
        let (y0i, y1i) = (y0 as u32, (y0 as u32 + 1).min(self.height - 1));
        let top = self.get(x0i, y0i).lerp(self.get(x1i, y0i), tx);
        let bottom = self.get(x0i, y1i).lerp(self.get(x1i, y1i), tx);
        top.lerp(bottom, ty)
    }

    /// Box-filtered by an integer `factor` (≥ 1).
    pub fn downsample(&self, factor: u32) -> Self {
        let f = factor.max(1);
        if f == 1 {
            return self.clone();
        }
        let (w, h) = ((self.width / f).max(1), (self.height / f).max(1));
        let mut out = Self::new(w, h);
        let scale = 1.0 / (f * f) as f32;
        for y in 0..h {
            for x in 0..w {
                let mut sum = Vec3::ZERO;
                for dy in 0..f {
                    for dx in 0..f {
                        sum += self.get(x * f + dx, y * f + dy);
                    }
                }
                out.pixels[(y * w + x) as usize] = sum * scale;
            }
        }
        out
    }

    /// At most `max` wide (a power-of-two box filter).
    pub fn fit(self, max: u32) -> Self {
        let mut f = 1;
        while self.width / f > max {
            f *= 2;
        }
        if f == 1 { self } else { self.downsample(f) }
    }

    /// Solid angle of a texel in row `y`.
    pub fn texel_solid_angle(&self, y: u32) -> f32 {
        let theta = (y as f32 + 0.5) / self.height as f32 * PI;
        (TAU / self.width as f32) * (PI / self.height as f32) * theta.sin()
    }

    /// Direction through the centre of texel (x, y).
    pub fn texel_direction(&self, x: u32, y: u32) -> Vec3 {
        direction(
            (x as f32 + 0.5) / self.width as f32,
            (y as f32 + 0.5) / self.height as f32,
        )
    }
}

/// Luminance (Rec. 709) of a linear colour.
pub fn luminance(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_round_trip() {
        for (u, v) in [(0.5, 0.5), (0.75, 0.3), (0.1, 0.9), (0.99, 0.01)] {
            let (u2, v2) = uv(direction(u, v));
            assert!((u - u2).abs() < 1e-4 && (v - v2).abs() < 1e-4, "{u} {v}");
        }
        assert!((direction(0.5, 0.5) - Vec3::Z).length() < 1e-5);
        assert!((direction(0.75, 0.5) - Vec3::X).length() < 1e-5);
        assert!((direction(0.3, 0.0) - Vec3::Y).length() < 1e-5);
    }

    #[test]
    fn sampling_and_downsampling_keep_the_image() {
        let img = HdrImage::from_fn(64, 32, |d| Vec3::splat(d.y.max(0.0) * 2.0));
        assert!((img.sample(Vec3::Y).x - 2.0).abs() < 0.05);
        assert!(img.sample(Vec3::NEG_Y).x < 1e-4);
        // Wraps across the seam behind the viewer (u = 0 / 1).
        let seam = img.sample(Vec3::new(0.0, 0.5, -1.0));
        assert!(
            (seam.x - 2.0 * 0.5f32.atan2(1.0).sin()).abs() < 0.1,
            "{seam}"
        );
        let small = img.clone().fit(16);
        assert_eq!((small.width, small.height), (16, 8));
        let total: f32 = (0..img.height)
            .map(|y| img.texel_solid_angle(y) * img.width as f32)
            .sum();
        assert!((total - 4.0 * PI).abs() < 0.01, "{total}");
    }
}
