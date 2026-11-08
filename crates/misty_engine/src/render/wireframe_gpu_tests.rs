//! The wireframe draw modes on the GPU (render/wireframe.rs): MeadowDemo in Wireframe and
//! Shaded Wireframe renders without validation errors and draws the terrain's edges (also
//! with virtual geometry on and through the barycentric fallback). Shots and the cost are
//! ignored tests, run on purpose in release:
//! `cargo test --release -p misty_engine --lib wireframe_ -- --ignored --nocapture --test-threads=1`

use std::path::PathBuf;

use glam::{Quat, Vec3};
use image::RgbaImage;

use crate::camera::{Camera, Projection};
use crate::quality::QualityLevel;
use crate::render::gpu_tests::{headless, hold_gpu_lock, instance, read_back, test_meshes};
use crate::render::terrain_grass::gpu_tests::{Shot, meadow_view};
use crate::render::{
    Environment, Frame, PostSettings, Renderer, WirePath, Wireframe, set_grass_streaming,
};
use crate::render_scene::GameView;

/// The editor's Scene view background and wireframe colours (misty_editor gizmos.rs).
const BACKGROUND: [f32; 3] = [0.28, 0.28, 0.28];
const WIRE: Wireframe = Wireframe {
    color: [0.05, 0.05, 0.05, 0.9],
    shaded: false,
};
const SHADED_WIRE: Wireframe = Wireframe {
    color: [0.0, 0.0, 0.0, 0.35],
    shaded: true,
};

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

/// `frames` renders of `frame` at `size` (`wire` before each one), read back; panics on a
/// validation error.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    r: &mut Renderer,
    frame: &Frame,
    wire: Option<Wireframe>,
    (w, h): (u32, u32),
    frames: usize,
) -> RgbaImage {
    let target = r.create_view_target(device, w, h);
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    for _ in 0..frames {
        if let Some(wire) = wire {
            assert!(r.set_wireframe(wire));
        }
        r.render(device, queue, &target, frame);
    }
    let pixels = read_back(device, queue, &target);
    if let Some(e) = pollster::block_on(scope.pop()) {
        panic!("validation error with {wire:?}: {e}");
    }
    RgbaImage::from_raw(w, h, pixels).expect("read back")
}

fn renderer(device: &wgpu::Device, queue: &wgpu::Queue, quality: &QualityLevel) -> Renderer {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    r
}

/// Share of the pixels in rows `rows` (fractions of the height) that differ from `bg` by more
/// than `tolerance` in a channel.
fn share_off(img: &RgbaImage, rows: std::ops::Range<f32>, bg: [u8; 3], tolerance: i32) -> f32 {
    let h = img.height() as f32;
    let (y0, y1) = ((rows.start * h) as u32, (rows.end * h) as u32);
    let mut off = 0;
    let mut all = 0;
    for y in y0..y1 {
        for x in 0..img.width() {
            let p = img.get_pixel(x, y);
            all += 1;
            if (0..3).any(|c| (p[c] as i32 - bg[c] as i32).abs() > tolerance) {
                off += 1;
            }
        }
    }
    off as f32 / all.max(1) as f32
}

/// Share of the pixels in rows `rows` that are darker in `wire` than in `plain` (the shaded
/// render without edges) by more than `by` in their sum of channels.
fn share_darker(wire: &RgbaImage, plain: &RgbaImage, rows: std::ops::Range<f32>, by: i32) -> f32 {
    let h = wire.height() as f32;
    let (y0, y1) = ((rows.start * h) as u32, (rows.end * h) as u32);
    let sum = |p: &image::Rgba<u8>| p[0] as i32 + p[1] as i32 + p[2] as i32;
    let mut darker = 0;
    let mut all = 0;
    for y in y0..y1 {
        for x in 0..wire.width() {
            all += 1;
            if sum(plain.get_pixel(x, y)) - sum(wire.get_pixel(x, y)) > by {
                darker += 1;
            }
        }
    }
    darker as f32 / all.max(1) as f32
}

/// The flat background as the final image shows it (no tonemapping in the Wireframe mode).
fn background_pixel() -> [u8; 3] {
    BACKGROUND.map(|c| (c * 255.0).round() as u8)
}

/// MeadowDemo re-posed for `shot`.
fn meadow(shot: Shot) -> GameView {
    meadow_view(shot)
}

/// Its frame with the editor Scene view's flat grey background.
fn meadow_frame(view: &GameView) -> Frame<'_> {
    Frame {
        background: BACKGROUND,
        ..view.frame()
    }
}

/// MeadowDemo in both wireframe modes on `device` (with `path`): no validation errors, and
/// edges over the ground (the lower part of the view is the terrain, its grass, trees and
/// rocks).
fn check_meadow(device: &wgpu::Device, queue: &wgpu::Queue, path: Option<WirePath>, vgeo: bool) {
    let view = meadow(Shot::WideGolden);
    let frame = meadow_frame(&view);
    let quality = QualityLevel {
        virtual_geometry: vgeo,
        ..QualityLevel::high()
    };
    let size = (480, 270);
    let new = || {
        let mut r = renderer(device, queue, &quality);
        if let Some(p) = path {
            r.force_wire_path(Some(p));
        }
        r
    };
    let wire = render(device, queue, &mut new(), &frame, Some(WIRE), size, 4);
    let bg = background_pixel();
    // Above the horizon: the flat background (some tree tops and the edges' anti-aliasing).
    let sky = share_off(&wire, 0.0..0.15, bg, 6);
    // The ground: covered in edges.
    let ground = share_off(&wire, 0.6..1.0, bg, 12);
    eprintln!(
        "wireframe ({path:?}, vgeo {vgeo}): sky {sky:.3}, ground {ground:.3} off the background"
    );
    assert!(sky < 0.5, "the background is flat: {sky}");
    assert!(ground > 0.05, "the terrain draws edges: {ground}");

    let shaded = render(device, queue, &mut new(), &frame, None, size, 4);
    let over = render(
        device,
        queue,
        &mut new(),
        &frame,
        Some(SHADED_WIRE),
        size,
        4,
    );
    let darker = share_darker(&over, &shaded, 0.6..1.0, 12);
    eprintln!("shaded wireframe: {darker:.3} of the ground darker");
    assert!(darker > 0.03, "edges over the shaded terrain: {darker}");
}

#[test]
fn wireframe_meadow_draws_terrain_edges() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    if !Renderer::with_queue(&device, &queue).gpu_wireframe() {
        eprintln!("no polygon mode Line or barycentrics: CPU lines (skipped)");
        return;
    }
    check_meadow(&device, &queue, None, false);
    // Virtual geometry on: the rocks draw their clusters' edges.
    check_meadow(&device, &queue, None, true);
}

/// The barycentric fallback (devices without polygon mode Line), on a device that has
/// barycentrics.
#[test]
fn wireframe_barycentric_fallback_draws_terrain_edges() {
    hold_gpu_lock();
    set_grass_streaming(false);
    let Ok(adapter) =
        pollster::block_on(instance().request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    if !adapter
        .features()
        .contains(wgpu::Features::SHADER_BARYCENTRICS)
    {
        eprintln!("no barycentrics on this adapter: skipped");
        return;
    }
    let Ok((device, queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("misty test device (barycentrics)"),
        required_features: super::timestamp_features(&adapter)
            | wgpu::Features::SHADER_BARYCENTRICS,
        required_limits: super::device_limits(&adapter, wgpu::Limits::default()),
        ..Default::default()
    })) else {
        eprintln!("no device with barycentrics: skipped");
        return;
    };
    check_meadow(&device, &queue, Some(WirePath::Barycentric), false);
}

/// The cubes and spheres of the renderer's GPU tests on their plane.
fn simple_frame<'a>(
    camera: &'a Camera,
    environment: &'a Environment,
    meshes: &'a [crate::render::MeshDraw],
    lights: &'a [crate::render::LightData],
) -> Frame<'a> {
    Frame {
        camera,
        grid: None,
        skybox: true,
        background: BACKGROUND,
        environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights,
        water: &[],
    }
}

fn simple_camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 7.0, -12.0),
        rotation: Quat::from_rotation_x(28f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 60.0,
    }
}

/// `docs/screenshots/gfx/wireframe_*.png`: MeadowDemo wide and close and a simple scene, each
/// shaded, Wireframe and Shaded Wireframe, at High (1600×900, 48 frames as the meadow shots).
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn wireframe_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = screenshots();
    std::fs::create_dir_all(&dir).unwrap();
    let quality = QualityLevel::high();
    let size = (1600, 900);
    let modes = [
        ("shaded", None),
        ("wire", Some(WIRE)),
        ("shadedwire", Some(SHADED_WIRE)),
    ];
    let save = |img: RgbaImage, name: &str| {
        let path = dir.join(format!("wireframe_{name}.png"));
        img.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
    };
    for (shot, name) in [(Shot::WideGolden, "wide"), (Shot::CloseGolden, "close")] {
        let view = meadow(shot);
        let frame = meadow_frame(&view);
        for (mode, wire) in modes {
            let mut r = renderer(&device, &queue, &quality);
            save(
                render(&device, &queue, &mut r, &frame, wire, size, 48),
                &format!("meadow_{name}_{mode}"),
            );
        }
    }
    let (camera, environment) = (simple_camera(), Environment::default());
    let meshes = test_meshes();
    let lights = super::gpu_tests::test_lights();
    let frame = simple_frame(&camera, &environment, &meshes, &lights);
    for (mode, wire) in modes {
        let mut r = renderer(&device, &queue, &quality);
        save(
            render(&device, &queue, &mut r, &frame, wire, (960, 600), 8),
            &format!("simple_{mode}"),
        );
    }
}

/// Median GPU frame time of MeadowDemo (wide, 1920×1080, High) shaded, in Wireframe and in
/// Shaded Wireframe, with virtual geometry off and on.
#[test]
#[ignore = "timing, prints only"]
fn wireframe_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = meadow(Shot::WideGolden);
    let frame = meadow_frame(&view);
    for vgeo in [false, true] {
        let quality = QualityLevel {
            virtual_geometry: vgeo,
            ..QualityLevel::high()
        };
        for (mode, wire) in [
            ("shaded", None),
            ("wireframe", Some(WIRE)),
            ("shaded wireframe", Some(SHADED_WIRE)),
        ] {
            let mut r = renderer(&device, &queue, &quality);
            let target = r.create_view_target(&device, 1920, 1080);
            let mut gpu = Vec::new();
            let mut cpu = Vec::new();
            for i in 0..180 {
                if let Some(w) = wire {
                    r.set_wireframe(w);
                }
                r.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if i >= 60 {
                    gpu.extend(r.gpu_ms());
                    cpu.push(r.stats().cpu_ms);
                }
            }
            let median = |mut v: Vec<f32>| {
                v.sort_by(f32::total_cmp);
                v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
            };
            eprintln!(
                "== vgeo {vgeo} {mode:<17} GPU {:6.2} ms  CPU {:5.2} ms",
                median(gpu),
                median(cpu)
            );
        }
    }
}
