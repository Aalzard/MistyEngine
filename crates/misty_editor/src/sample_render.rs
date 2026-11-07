//! Headless rendering for the sample scenes' GPU checks: render a frame into an offscreen
//! target and read the final image back. Tests skip themselves when there is no adapter.

use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::render::{Frame, Renderer};
use misty_engine::wgpu;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 270;

/// One GPU test at a time (concurrent devices could deadlock in the driver).
static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    /// The lock this test thread holds; released when the test's thread ends.
    static GPU_HELD: std::cell::RefCell<Option<std::sync::MutexGuard<'static, ()>>> =
        const { std::cell::RefCell::new(None) };
}

/// A device without a window (`None` = no adapter); the calling test holds the GPU lock.
pub fn headless() -> Option<(wgpu::Device, wgpu::Queue)> {
    device(false)
}

/// Like [`headless`], with GPU timestamp queries when the adapter has them (for
/// [`Renderer::with_queue`] and its `gpu_ms`).
pub fn headless_timed() -> Option<(wgpu::Device, wgpu::Queue)> {
    device(true)
}

/// One wgpu instance for every GPU test of the process, never dropped: the Vulkan loader
/// unloads the driver with its last instance, and an instance per test unloaded and reloaded
/// it for every test while the previous device's driver threads could still be winding down.
fn instance() -> &'static wgpu::Instance {
    static INSTANCE: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| {
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
    })
}

fn device(timed: bool) -> Option<(wgpu::Device, wgpu::Queue)> {
    GPU_HELD.with(|held| {
        let mut held = held.borrow_mut();
        if held.is_none() {
            *held = Some(GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
        }
    });
    // Renders must be complete on their first frame: grass chunks are built in the frame
    // that needs them rather than streamed in over several. (Once the lock is held: the
    // flag is process-wide.)
    misty_engine::render::set_grass_streaming(false);
    let adapter =
        pollster::block_on(instance().request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let desc = wgpu::DeviceDescriptor {
        required_features: match timed {
            true => misty_engine::render::timestamp_features(&adapter),
            false => wgpu::Features::empty(),
        },
        required_limits: misty_engine::render::device_limits(&adapter, Default::default()),
        ..Default::default()
    };
    pollster::block_on(adapter.request_device(&desc)).ok()
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
    let pixels = crate::project_thumbnail::read_pixels(device, queue, &target);
    Image { pixels }
}
