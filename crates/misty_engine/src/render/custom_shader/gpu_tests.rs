//! Headless renders: a custom shader draws its colour, a broken one draws magenta and reports
//! its line, and an ocean renders (plus an ignored 1080p water timing benchmark).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};

use super::CustomMaterial;
use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Environment, Frame, MaterialParams, MeshDraw, PostSettings, Renderer};
use crate::scene::PrimitiveMesh;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 0.0, -3.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 100.0,
    }
}

fn custom(path: &str, queue: u32) -> MaterialParams {
    MaterialParams {
        unlit: true,
        custom: Some(CustomMaterial {
            shader: path.into(),
            values: Arc::new(BTreeMap::new()),
            queue,
        }),
        ..Default::default()
    }
}

/// Render a cube filling the middle of a 64² view; returns the centre pixel.
fn centre_pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    material: MaterialParams,
) -> [u8; 4] {
    let cam = camera();
    let env = Environment::default();
    let meshes = [MeshDraw {
        mesh: PrimitiveMesh::Cube.into(),
        model: Mat4::IDENTITY,
        material,
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
    }];
    let frame = Frame {
        camera: &cam,
        grid: None,
        skybox: false,
        background: [0.0; 3],
        environment: &env,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &[],
        water: &[],
    };
    let target = renderer.create_target(device, 64, 64);
    renderer.render(device, queue, &target, &frame);
    let px = read_back(device, queue, &target);
    let i = (32 * 64 + 32) * 4;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

#[test]
fn custom_shader_outputs_its_colour_and_errors_fall_back_to_magenta() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&device);
    // Linear 0.2 / 0.6 / 0.0 ≈ sRGB 124 / 203 / 0 (no tonemapping by default).
    let good = "Assets/Shaders/__test_green.shader";
    crate::shader_asset::set_source(
        good,
        "// @shader unlit\nfn fragment(input: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(0.2, 0.6, 0.0, 1.0);\n}\n",
    );
    let px = centre_pixel(&device, &queue, &mut renderer, custom(good, 2000));
    assert!(renderer.take_shader_errors().is_empty());
    assert!(
        px[1] > 150 && px[0] > 60 && px[0] < 170 && px[2] < 30,
        "{px:?}"
    );

    // Two shaders on the same draw list batch like built-ins; a lit surface shader works too.
    let lit = "Assets/Shaders/__test_lit.shader";
    crate::shader_asset::set_source(lit, crate::shader_asset::templates::SURFACE);
    let px = centre_pixel(&device, &queue, &mut renderer, custom(lit, 2000));
    assert!(
        renderer.take_shader_errors().is_empty(),
        "template compiles"
    );
    assert!(px[0] > 5, "lit by the ambient light: {px:?}");

    let bad = "Assets/Shaders/__test_broken.shader";
    crate::shader_asset::set_source(
        bad,
        "// @shader unlit\nfn fragment(input: SurfaceInput) -> vec4<f32> {\n    return oops;\n}\n",
    );
    let px = centre_pixel(&device, &queue, &mut renderer, custom(bad, 2000));
    let errors = renderer.take_shader_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("Shader error in Assets/Shaders/__test_broken.shader:3:"),
        "{}",
        errors[0]
    );
    assert!(px[0] > 200 && px[1] < 40 && px[2] > 200, "magenta: {px:?}");

    // Fixing it recompiles (hot reload) and the error colour goes away.
    crate::shader_asset::set_source(
        bad,
        "// @shader unlit\nfn fragment(input: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(0.0, 0.0, 1.0, 1.0);\n}\n",
    );
    let px = centre_pixel(&device, &queue, &mut renderer, custom(bad, 2000));
    assert!(px[2] > 200 && px[0] < 40, "recompiled: {px:?}");
}

fn ocean_frame_render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    with_water: bool,
    frames: usize,
) -> (Vec<u8>, Option<f32>) {
    use crate::water::{WaterDraw, WaterKind, WaterSurface};
    let cam = Camera {
        position: Vec3::new(0.0, 6.0, -20.0),
        rotation: Quat::from_rotation_x(12f32.to_radians()),
        projection: Projection::Perspective { fov_y: 55.0 },
        near: 0.3,
        far: 5000.0,
    };
    let rock = MeshDraw {
        mesh: PrimitiveMesh::Sphere.into(),
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(8.0, 6.0, 8.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -1.0, 10.0),
        ),
        material: MaterialParams::default(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
    };
    let sun = crate::render::LightData {
        kind: crate::scene::LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.3, -0.4, 1.0).normalize(),
        color: [1.0, 0.85, 0.7],
        intensity: 1.2,
        range: 0.0,
        spot_angle: 0.0,
        shadows: crate::scene::Shadows::Soft,
    };
    let lights = [sun];
    let environment = Environment::new(Default::default(), &lights);
    let mut renderer = Renderer::with_queue(device, queue);
    let target = renderer.create_target(device, size.0, size.1);
    let mut times = Vec::new();
    for i in 0..frames {
        // Waves move every frame, so the FFT runs every frame.
        let water = [WaterDraw::new(
            &WaterSurface::new(WaterKind::Ocean),
            Mat4::IDENTITY,
            3.0 + i as f32 / 60.0,
        )];
        let frame = Frame {
            camera: &cam,
            grid: None,
            skybox: true,
            background: [0.0; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: std::slice::from_ref(&rock),
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: if with_water { &water } else { &[] },
        };
        renderer.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if let Some(ms) = renderer.gpu_ms() {
            times.push(ms);
        }
    }
    let px = read_back(device, queue, &target);
    times.sort_by(f32::total_cmp);
    (px, times.get(times.len() / 2).copied())
}

#[test]
fn ocean_renders_to_the_horizon() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (160, 90);
    let (sky, _) = ocean_frame_render(&device, &queue, (w, h), false, 1);
    let (sea, _) = ocean_frame_render(&device, &queue, (w, h), true, 2);
    if let Some(path) = std::env::var_os("MISTY_WATER_TEST_PNG") {
        image::RgbaImage::from_raw(w, h, sea.clone())
            .expect("size")
            .save(path)
            .expect("png");
    }
    // The lower half (below the horizon) is now water, not the sky's ground colour.
    let row = |px: &[u8], y: u32| -> Vec<u8> {
        px[(y * w * 4) as usize..((y + 1) * w * 4) as usize].to_vec()
    };
    let bottom_sky = row(&sky, h - 5);
    let bottom_sea = row(&sea, h - 5);
    assert_ne!(bottom_sky, bottom_sea, "water drawn below the horizon");
    // Water near the camera is bluish-green (blue+green beat red).
    let i = ((h - 5) * w + w / 4) as usize * 4;
    let (r, g, b) = (sea[i] as u32, sea[i + 1] as u32, sea[i + 2] as u32);
    assert!(g + b > 2 * r, "water colour {r} {g} {b}");
}

/// `cargo test --release -p misty_engine -- --ignored water_benchmark --nocapture`
#[test]
#[ignore = "benchmark: water GPU cost at 1080p"]
fn water_benchmark() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let size = (1920, 1080);
    let (_, without) = ocean_frame_render(&device, &queue, size, false, 60);
    let (_, with) = ocean_frame_render(&device, &queue, size, true, 60);
    eprintln!("1080p GPU: without water {without:?} ms, with ocean {with:?} ms");
}
