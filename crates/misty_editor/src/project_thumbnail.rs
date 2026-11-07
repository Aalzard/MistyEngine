//! The Hub's project preview: when a project closes, the last frame of the view the user was
//! looking at (Scene or Game) is saved as `Library/HubThumbnail.png`.

use std::path::Path;

use misty_engine::image::{self, imageops::FilterType};
use misty_engine::render::RenderTarget;
use misty_engine::wgpu;

/// Saved size: the same 16:9 as the template thumbnails.
pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 270;

/// Read a render target's final (tonemapped, sRGB) image back as tightly packed RGBA8.
pub fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, target: &RenderTarget) -> Vec<u8> {
    let (width, height) = target.output_size();
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let copy = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("readback copy"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: misty_engine::render::COLOR_FORMAT,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback buffer"),
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    target.copy_output(&mut encoder, &copy);
    encoder.copy_texture_to_buffer(
        copy.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("mapped");
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        let start = y * row as usize;
        pixels.extend_from_slice(&data[start..start + width as usize * 4]);
    }
    pixels
}

/// Centre-crop `rgba` (`width` × `height`) to 16:9 and scale it to the thumbnail size.
pub fn fit(width: u32, height: u32, rgba: Vec<u8>) -> Option<image::RgbImage> {
    let img = image::RgbaImage::from_raw(width, height, rgba)?;
    let (w, h) = if width * HEIGHT > height * WIDTH {
        (height * WIDTH / HEIGHT, height)
    } else {
        (width, width * HEIGHT / WIDTH)
    };
    if w == 0 || h == 0 {
        return None;
    }
    let crop = image::imageops::crop_imm(&img, (width - w) / 2, (height - h) / 2, w, h).to_image();
    let small = image::imageops::resize(&crop, WIDTH, HEIGHT, FilterType::Triangle);
    Some(image::DynamicImage::ImageRgba8(small).to_rgb8())
}

/// Write the thumbnail into `<project>/Library/HubThumbnail.png`.
pub fn save(project: &Path, img: &image::RgbImage) -> Result<(), String> {
    let path = project.join(crate::templates::PROJECT_THUMBNAIL);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    img.save(&path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_crops_to_16_9_and_scales() {
        // A tall 100 × 200 image: the middle 100 × 56 band is kept.
        let mut rgba = vec![0u8; 100 * 200 * 4];
        for y in 72..128 {
            for x in 0..100 {
                let i = (y * 100 + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        let img = fit(100, 200, rgba).unwrap();
        assert_eq!((img.width(), img.height()), (WIDTH, HEIGHT));
        let mid = img.get_pixel(WIDTH / 2, HEIGHT / 2);
        assert!(mid[0] > 200 && mid[1] < 50, "{mid:?}");
        assert!(fit(0, 0, Vec::new()).is_none());
    }
}
