//! Tonemapped 8-bit previews of HDR images (Project window thumbnails, the Lighting window's
//! texture slot): auto exposure from the log-average luminance, then Reinhard and sRGB.

use glam::Vec3;

use super::{HdrImage, luminance};

/// RGBA8 preview at most `max_width` wide (2:1 for equirect images).
pub fn rgba8(img: &HdrImage, max_width: u32) -> (u32, u32, Vec<u8>) {
    let factor = img.width.div_ceil(max_width.max(1)).max(1);
    let small = img.downsample(factor);
    let exposure = auto_exposure(&small);
    let bytes = small
        .pixels
        .iter()
        .flat_map(|c| {
            let c = *c * exposure;
            let mapped = c / (Vec3::ONE + c);
            let [r, g, b] = mapped
                .to_array()
                .map(|v| (v.powf(1.0 / 2.2) * 255.0).round() as u8);
            [r, g, b, 255]
        })
        .collect();
    (small.width, small.height, bytes)
}

/// Exposure that maps the log-average luminance to middle grey.
fn auto_exposure(img: &HdrImage) -> f32 {
    let n = img.pixels.len().max(1) as f32;
    let log_sum: f32 = img
        .pixels
        .iter()
        .map(|c| (luminance(*c).max(1e-4)).ln())
        .sum();
    let average = (log_sum / n).exp();
    (0.18 / average.max(1e-4)).clamp(1e-3, 1e3) * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bright_and_dark_images_both_preview_mid_grey() {
        for v in [0.01, 1.0, 500.0] {
            let img = HdrImage::from_fn(64, 32, |_| Vec3::splat(v));
            let (w, h, px) = rgba8(&img, 32);
            assert_eq!((w, h, px.len()), (32, 16, 32 * 16 * 4));
            assert!((100..200).contains(&px[0]), "{v}: {}", px[0]);
            assert_eq!(px[3], 255);
        }
    }
}
