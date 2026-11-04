//! HDRI skies: equirectangular `.hdr` / `.exr` images used as the skybox (Lighting window ▸
//! Environment ▸ Skybox = HDRI). This module is the CPU side: decoding ([`HdrImage`]), our own
//! Radiance writer ([`rgbe`]), the diffuse SH and brightest direction computed once per file
//! ([`HdriSky`]), tonemapped previews and the shared file cache ([`store`]). The GPU cubemaps
//! (background + GGX mip chain) are built in `render::hdri_sky`.

mod equirect;
pub mod preview;
pub mod rgbe;
pub mod store;

use std::sync::{Arc, Mutex};

use glam::{Quat, Vec3};

pub use equirect::{HdrImage, MAX_WIDTH, direction, luminance, uv};

use crate::render::probes::sh;

/// File extensions loaded as HDR images.
pub const EXTENSIONS: [&str; 2] = ["hdr", "exr"];

pub fn is_hdri(path: &str) -> bool {
    crate::assets::has_extension(path, &EXTENSIONS)
}

/// Width of the copy used for SH, the brightest point and CPU sky lookups.
const SMALL_WIDTH: u32 = 128;

/// A decoded HDRI and what the renderer derives from it on the CPU.
#[derive(Debug)]
pub struct HdriSky {
    /// Full image (at most [`MAX_WIDTH`] wide), uploaded for the GPU cubemaps.
    pub image: HdrImage,
    /// 128 × 64 copy for CPU lookups (ambient, fog).
    pub small: HdrImage,
    /// Irradiance / π as SH9 (`probes::sh` basis), unrotated, before exposure and tint.
    pub sh: [Vec3; 9],
    /// Unit direction of the brightest area (the sun), unrotated.
    pub brightest: Vec3,
    /// Last [`HdriSky::sh_rotated`] result (rotation bits, coefficients).
    rotated: Mutex<Option<(u32, [Vec3; 9])>>,
}

impl HdriSky {
    pub fn new(image: HdrImage) -> Self {
        let factor = (image.width / SMALL_WIDTH).max(1);
        let small = image.downsample(factor);
        Self {
            sh: irradiance_sh(&small),
            brightest: brightest(&small),
            rotated: Mutex::new(None),
            small,
            image,
        }
    }

    /// Diffuse SH of the sky rotated by `degrees` about +Y (re-projected, cached).
    pub fn sh_rotated(&self, degrees: f32) -> [Vec3; 9] {
        if degrees == 0.0 {
            return self.sh;
        }
        let mut cache = self.rotated.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((bits, sh)) = *cache
            && bits == degrees.to_bits()
        {
            return sh;
        }
        let q = rotation(degrees);
        let sh = sh::cosine_convolve(project_with(&self.small, |d| q * d));
        *cache = Some((degrees.to_bits(), sh));
        sh
    }
}

/// SH projection of an equirect image (solid-angle weighted), each texel at `dir(direction)`.
fn project_with(img: &HdrImage, dir: impl Fn(Vec3) -> Vec3) -> [Vec3; 9] {
    let mut out = [Vec3::ZERO; 9];
    for y in 0..img.height {
        let w = img.texel_solid_angle(y);
        for x in 0..img.width {
            let c = img.get(x, y) * w;
            for (s, b) in out
                .iter_mut()
                .zip(sh::basis(dir(img.texel_direction(x, y))))
            {
                *s += c * b;
            }
        }
    }
    out
}

/// SH9 approximation of the image itself (no cosine convolution).
pub fn project_sh(img: &HdrImage) -> [Vec3; 9] {
    project_with(img, |d| d)
}

/// Diffuse SH of an equirect image: evaluating it at a normal gives irradiance / π.
pub fn irradiance_sh(img: &HdrImage) -> [Vec3; 9] {
    sh::cosine_convolve(project_sh(img))
}

/// Direction of the brightest texel, refined by the luminance-weighted centre of its 3 × 3
/// neighbourhood.
pub fn brightest(img: &HdrImage) -> Vec3 {
    let (mut best, mut at) = (-1.0, (0, 0));
    for y in 0..img.height {
        for x in 0..img.width {
            let l = luminance(img.get(x, y));
            if l > best {
                (best, at) = (l, (x, y));
            }
        }
    }
    let mut sum = Vec3::ZERO;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            let y = (at.1 as i32 + dy).clamp(0, img.height as i32 - 1) as u32;
            let x = (at.0 as i32 + dx).rem_euclid(img.width as i32) as u32;
            sum += img.texel_direction(x, y) * luminance(img.get(x, y)).max(0.0);
        }
    }
    sum.try_normalize()
        .unwrap_or_else(|| img.texel_direction(at.0, at.1))
}

/// Rotation of the sky by `degrees` about +Y (Lighting window ▸ Rotation).
pub fn rotation(degrees: f32) -> Quat {
    Quat::from_rotation_y(degrees.to_radians())
}

impl HdriSky {
    /// World direction towards the brightest point with the sky rotated by `degrees`.
    pub fn sun_direction(&self, degrees: f32) -> Vec3 {
        rotation(degrees) * self.brightest
    }
}

/// Euler angles (degrees, the Transform's convention) of a Directional Light shining from
/// `towards_sun`: its forward (+Z) points the other way.
pub fn sun_euler(towards_sun: Vec3) -> Vec3 {
    let f = -towards_sun.normalize_or(Vec3::Y);
    let pitch = (-f.y).clamp(-1.0, 1.0).asin().to_degrees();
    let yaw = f.x.atan2(f.z).to_degrees();
    Vec3::new(pitch, yaw, 0.0)
}

/// Shared decoded sky compared by identity (what the GPU cache keys on).
#[derive(Clone)]
pub struct HdriHandle(pub Arc<HdriSky>);

impl PartialEq for HdriHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for HdriHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let i = &self.0.image;
        write!(f, "HdriHandle({}×{})", i.width, i.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_sky_gives_its_colour_as_ambient() {
        let img = HdrImage::from_fn(64, 32, |_| Vec3::new(0.2, 0.5, 1.0));
        let sh = irradiance_sh(&img);
        for n in [Vec3::Y, Vec3::NEG_Z, Vec3::new(0.4, -0.3, 0.8).normalize()] {
            assert!((sh::eval(&sh, n) - Vec3::new(0.2, 0.5, 1.0)).length() < 0.01);
        }
    }

    #[test]
    fn brightest_point_finds_the_sun() {
        let sun = Vec3::new(0.6, 0.3, -0.7).normalize();
        let img = HdrImage::from_fn(256, 128, |d| {
            Vec3::splat(0.5) + Vec3::splat(if d.dot(sun) > 0.999 { 500.0 } else { 0.0 })
        });
        let sky = HdriSky::new(img);
        assert!(sky.brightest.dot(sun) > 0.998, "{}", sky.brightest);
        // The sun's side of the sky lights more.
        assert!(sh::eval(&sky.sh, sun).x > sh::eval(&sky.sh, -sun).x);
        // Rotating the sky carries its lighting along.
        let turned = sky.sh_rotated(90.0);
        let (a, b) = (
            sh::eval(&turned, rotation(90.0) * sun),
            sh::eval(&sky.sh, sun),
        );
        assert!((a - b).length() < 0.02 * b.length(), "{a} vs {b}");
        assert!(is_hdri("Assets/HDRI/Sky.HDR") && is_hdri("a.exr") && !is_hdri("a.png"));
        // A light at those angles shines from the (rotated) sun.
        let towards = sky.sun_direction(40.0);
        let forward = crate::camera::euler_to_quat(sun_euler(towards)) * Vec3::Z;
        assert!(forward.dot(-towards) > 0.9999, "{forward} vs {towards}");
    }
}
