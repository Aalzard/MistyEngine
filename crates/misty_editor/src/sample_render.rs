//! Headless rendering for the sample scenes' GPU checks: render a frame into an offscreen
//! target and read the final image back. Tests skip themselves when there is no adapter.

use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::render::{Frame, Renderer};
use misty_engine::wgpu;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 270;

pub fn headless() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    pollster::block_on(adapter.request_device(&Default::default())).ok()
}

/// The final 8-bit sRGB image of a render.
pub struct Image {
    pub pixels: Vec<u8>,
}

impl Image {
    /// Mean brightness (0..255) of a 5×5 block around the pixel showing world point `p`.
    pub fn brightness(&self, camera: &Camera, p: Vec3) -> f32 {
        let ndc = camera
            .view_projection(WIDTH as f32 / HEIGHT as f32)
            .project_point3(p);
        let x = ((ndc.x * 0.5 + 0.5) * WIDTH as f32) as i32;
        let y = ((0.5 - ndc.y * 0.5) * HEIGHT as f32) as i32;
        let mut sum = 0.0;
        for dy in -2..=2 {
            for dx in -2..=2 {
                let (px, py) = ((x + dx) as u32, (y + dy) as u32);
                let i = ((py * WIDTH + px) * 4) as usize;
                sum += self.pixels[i..i + 3].iter().map(|&c| c as f32).sum::<f32>() / 3.0;
            }
        }
        sum / 25.0
    }
}

/// Render `frame` `frames` times (temporal effects settle) and read the result back.
pub fn render(gpu: &(wgpu::Device, wgpu::Queue), frame: &Frame, frames: u32) -> Image {
    let (device, queue) = gpu;
    let mut renderer = Renderer::new(device);
    let target = renderer.create_target(device, WIDTH, HEIGHT);
    for _ in 0..frames.max(1) {
        renderer.render(device, queue, &target, frame);
    }
    let size = wgpu::Extent3d {
        width: WIDTH,
        height: HEIGHT,
        depth_or_array_layers: 1,
    };
    let copy = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sample render copy"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: misty_engine::render::COLOR_FORMAT,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (WIDTH * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sample render readback"),
        size: (row * HEIGHT) as u64,
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
                rows_per_image: Some(HEIGHT),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("mapped");
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT as usize {
        let start = y * row as usize;
        pixels.extend_from_slice(&data[start..start + WIDTH as usize * 4]);
    }
    Image { pixels }
}
