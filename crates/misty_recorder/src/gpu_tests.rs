//! The staging-buffer ring on a real GPU (skipped when the machine has no adapter): more
//! frames than buffers come back complete, in order, and identical to a blocking read of the
//! same image.

use misty_engine::camera::{Camera, Projection};
use misty_engine::render::{Environment, Frame as RenderFrame, PostSettings, Renderer};
use misty_engine::wgpu;

use crate::Frame;
use crate::readback::{Readback, SLOTS};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    // The features and limits the editor asks for (the renderer's pipelines need them).
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("recorder test device"),
        required_features: misty_engine::render::timestamp_features(&adapter),
        required_limits: misty_engine::render::device_limits(&adapter, wgpu::Limits::default()),
        ..Default::default()
    }))
    .ok()
}

/// A blocking copy of the target's image (what the ring must match).
fn read_now(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &misty_engine::render::RenderTarget,
) -> Vec<u8> {
    let (w, h) = target.output_size();
    let mut ring = Readback::new(device, w, h);
    let mut out = Vec::new();
    ring.capture(device, queue, target, 0, &mut out);
    ring.collect(device, true, &mut out);
    out.remove(0).rgba
}

#[test]
fn the_readback_ring_returns_every_frame_in_order() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (64, 48);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_view_target(&device, w, h);
    let camera = Camera {
        position: glam::Vec3::new(0.0, 1.0, 5.0),
        rotation: glam::Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 100.0,
    };
    let environment = Environment::default();
    let mut ring = Readback::new(&device, w, h);
    let mut got: Vec<Frame> = Vec::new();
    let mut want: Vec<Vec<u8>> = Vec::new();
    let n = SLOTS as u64 * 2 + 1;
    for i in 0..n {
        // A different background each frame.
        let v = i as f32 / n as f32;
        let frame = RenderFrame {
            camera: &camera,
            grid: None,
            skybox: false,
            background: [v, 1.0 - v, 0.5],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &[],
            water: &[],
        };
        renderer.render(&device, &queue, &target, &frame);
        ring.capture(&device, &queue, &target, i, &mut got);
        assert!(ring.in_flight() <= SLOTS);
        let mut px = read_now(&device, &queue, &target);
        crate::readback::opaque(&mut px);
        want.push(px);
    }
    ring.collect(&device, true, &mut got);
    assert_eq!(ring.in_flight(), 0);
    assert_eq!(got.len() as u64, n, "every frame came back");
    for (i, f) in got.iter().enumerate() {
        assert_eq!(f.index, i as u64, "in order");
        assert_eq!((f.width, f.height), (w, h));
        assert_eq!(f.rgba, want[i], "frame {i} matches a blocking read");
    }
    assert_ne!(got[0].rgba, got[n as usize - 1].rgba, "the frames differ");
}
