//! Textures the templates generate (no downloaded art: everything is made here, free to reuse):
//! the First Person ground grid and the Racing road, kerbs and start line.

use std::path::Path;

use misty_engine::image::{Rgb, RgbImage};

use super::io_error;

/// Small integer hash → 0..1 (a stable "noise" for the asphalt grain).
fn hash(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn grey(v: f32) -> Rgb<u8> {
    let v = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb([v, v, v])
}

/// White with faint grey lines every 64 pixels (one tile; the material repeats it).
pub fn grid() -> RgbImage {
    RgbImage::from_fn(256, 256, |x, y| {
        let line = |v: u32| v % 128 < 1 || v % 128 > 126;
        let fine = |v: u32| v.is_multiple_of(32);
        if line(x) || line(y) {
            grey(0.86)
        } else if fine(x) || fine(y) {
            grey(0.92)
        } else {
            grey(0.97)
        }
    })
}

/// Mottled grass (one tile; the material tints and repeats it).
pub fn grass() -> RgbImage {
    RgbImage::from_fn(128, 128, |x, y| {
        let blotch = hash(x / 16, y / 16) * 0.5 + hash(x / 5, y / 5) * 0.3 + hash(x, y) * 0.2;
        let v = 0.78 + blotch * 0.22;
        Rgb([(v * 235.0) as u8, (v * 250.0) as u8, (v * 225.0) as u8])
    })
}

/// Asphalt across the road (u = 0 left edge, 1 right edge; v along it): a fine grain, white
/// edge lines and a dashed centre line (the dash is the first half of the tile along v).
pub fn asphalt() -> RgbImage {
    let n = 256;
    RgbImage::from_fn(n, n, |x, y| {
        let u = (x as f32 + 0.5) / n as f32;
        let v = (y as f32 + 0.5) / n as f32;
        let edge = (0.035..0.06).contains(&u) || (0.94..0.965).contains(&u);
        let centre = (0.49..0.51).contains(&u) && v < 0.5;
        if edge || centre {
            grey(0.86)
        } else {
            let grain = hash(x, y) * 0.05 + hash(x / 4, y / 4) * 0.03;
            grey(0.20 + grain)
        }
    })
}

/// Red and white kerb bands along v (one band of each per tile).
pub fn kerb() -> RgbImage {
    RgbImage::from_fn(16, 64, |_, y| {
        if y < 32 {
            Rgb([200, 30, 25])
        } else {
            Rgb([235, 235, 235])
        }
    })
}

/// A black and white chequer (8 × 8 squares) for the start line.
pub fn checker() -> RgbImage {
    RgbImage::from_fn(64, 64, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            grey(0.92)
        } else {
            grey(0.05)
        }
    })
}

/// Save `image` as a PNG at `root/rel`.
pub fn save(root: &Path, rel: &str, image: &RgbImage) -> Result<(), String> {
    let path = root.join(rel);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_error("Could not create the folder", dir))?;
    }
    image
        .save(&path)
        .map_err(|e| format!("Could not write {}: {e}", path.display()))
}
