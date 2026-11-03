//! Headless GPU checks: frustum culling and automatic instancing must not change a pixel,
//! plus the StressDemo benchmark (ignored; run in release). Skipped when no adapter exists.

use glam::{Mat4, Quat, Vec3};

use super::*;
use crate::camera::{Camera, Projection};
use crate::scene::PrimitiveMesh;

/// A device without a window (`None` = no usable adapter on this machine).
pub(crate) fn headless() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("misty test device"),
        required_features: timestamp_features(&adapter),
        ..Default::default()
    }))
    .ok()
}

/// The target's final image as tightly packed RGBA rows.
pub(crate) fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &RenderTarget,
) -> Vec<u8> {
    let (width, height) = target.size();
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let copy = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("misty test copy"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR_FORMAT,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty test readback"),
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

fn material(color: [f32; 4], mode: RenderingMode) -> MaterialParams {
    MaterialParams {
        color,
        mode,
        smoothness: 0.6,
        ..Default::default()
    }
}

fn draw(mesh: PrimitiveMesh, model: Mat4, material: &MaterialParams) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material: material.clone(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
    }
}

/// A grid of cubes and spheres in four materials (opaque, metal, cutout, fade), some outside
/// the view and behind the camera, on a ground plane.
fn test_meshes() -> Vec<MeshDraw> {
    let red = material([0.9, 0.2, 0.2, 1.0], RenderingMode::Opaque);
    let mut metal = material([0.3, 0.5, 0.9, 1.0], RenderingMode::Opaque);
    metal.metallic = 0.8;
    let cutout = material([0.3, 0.9, 0.3, 0.8], RenderingMode::Cutout);
    let glass = material([1.0, 1.0, 0.4, 0.45], RenderingMode::Fade);
    let mut out = vec![draw(
        PrimitiveMesh::Plane,
        Mat4::from_scale(Vec3::new(4.0, 1.0, 4.0)),
        &red,
    )];
    for x in -8i32..=8 {
        for z in -6i32..=10 {
            let i = (x + z).rem_euclid(4);
            let mesh = if (x * 3 + z) % 2 == 0 {
                PrimitiveMesh::Cube
            } else {
                PrimitiveMesh::Sphere
            };
            let m = match i {
                0 => &red,
                1 => &metal,
                2 => &cutout,
                _ => &glass,
            };
            let model = Mat4::from_scale_rotation_translation(
                Vec3::splat(0.6 + 0.1 * (z.rem_euclid(3)) as f32),
                Quat::from_rotation_y(x as f32 * 0.4),
                Vec3::new(x as f32 * 2.2, 0.6, z as f32 * 2.2),
            );
            out.push(draw(mesh, model, m));
        }
    }
    out
}

fn test_lights() -> Vec<LightData> {
    let light = |kind, position: Vec3, direction: Vec3, color| LightData {
        kind,
        position,
        direction: direction.normalize(),
        color,
        intensity: 1.2,
        range: 9.0,
        spot_angle: 50.0,
        shadows: Shadows::Soft,
    };
    vec![
        light(
            LightType::Directional,
            Vec3::ZERO,
            Vec3::new(0.4, -1.0, 0.5),
            [1.0, 0.95, 0.9],
        ),
        light(
            LightType::Spot,
            Vec3::new(-3.0, 5.0, 2.0),
            Vec3::new(0.2, -1.0, 0.1),
            [0.4, 0.6, 1.0],
        ),
        light(
            LightType::Point,
            Vec3::new(4.0, 2.5, 6.0),
            Vec3::Z,
            [1.0, 0.5, 0.2],
        ),
    ]
}

fn test_particles() -> Vec<ParticleBatch> {
    let batch = |origin: Vec3| ParticleBatch {
        origin,
        particles: (0..20)
            .map(|i| ParticleInstance {
                position: (origin + Vec3::new(i as f32 * 0.1, i as f32 * 0.05, 0.0)).to_array(),
                size: 0.3,
                color: [1.0, 0.8, 0.5, 0.8],
                params: [0.0; 4],
            })
            .collect(),
        texture: None,
        additive: false,
    };
    // One in view, one far behind the camera.
    vec![
        batch(Vec3::new(1.0, 2.0, 3.0)),
        batch(Vec3::new(0.0, 2.0, -60.0)),
    ]
}

fn render_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    options: RenderOptions,
) -> (Vec<u8>, RenderStats) {
    let camera = Camera {
        position: Vec3::new(0.0, 7.0, -12.0),
        rotation: Quat::from_rotation_x(28f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 60.0,
    };
    let (meshes, lights, particles) = (test_meshes(), test_lights(), test_particles());
    let environment = Environment::default();
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &particles,
        terrains: &[],
        shaded: true,
        lights: &lights,
    };
    let mut renderer = Renderer::with_queue(device, queue);
    renderer.set_options(options);
    let target = renderer.create_target(device, 320, 200);
    renderer.render(device, queue, &target, &frame);
    (read_back(device, queue, &target), renderer.stats())
}

#[test]
fn culling_and_instancing_render_identically() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let off = RenderOptions {
        culling: false,
        instancing: false,
    };
    let (plain, plain_stats) = render_with(&device, &queue, off);
    let (fast, fast_stats) = render_with(&device, &queue, RenderOptions::default());
    let differing = plain
        .chunks(4)
        .zip(fast.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    // `MISTY_GPU_TEST_PNG=out.png` saves the image for a look.
    if let Some(path) = std::env::var_os("MISTY_GPU_TEST_PNG") {
        image::RgbaImage::from_raw(320, 200, fast.clone())
            .expect("size")
            .save(path)
            .expect("png");
    }
    eprintln!(
        "without: {plain_stats:?}
with: {fast_stats:?}"
    );
    assert_eq!(differing, 0, "{differing} pixels differ");
    // Something was actually drawn (not just sky).
    let distinct: std::collections::HashSet<&[u8]> = plain.chunks(4).collect();
    assert!(distinct.len() > 100);

    assert_eq!(plain_stats.culled, 0);
    assert!(fast_stats.culled > 50, "{fast_stats:?}");
    assert!(fast_stats.shadow_culled > 0, "spot/point maps skip casters");
    assert!(fast_stats.saved_by_batching > 100, "{fast_stats:?}");
    assert!(
        fast_stats.batches * 4 < plain_stats.batches,
        "{} vs {}",
        fast_stats.batches,
        plain_stats.batches
    );
    assert!(fast_stats.triangles < plain_stats.triangles);
    assert_eq!(plain_stats.saved_by_batching, 0);
    assert!(fast_stats.shadow_casters > 0 && fast_stats.set_pass > 0);
}

#[test]
fn instancing_alone_and_culling_alone_match_too() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let both = render_with(&device, &queue, RenderOptions::default()).0;
    for options in [
        RenderOptions {
            culling: true,
            instancing: false,
        },
        RenderOptions {
            culling: false,
            instancing: true,
        },
    ] {
        let (image, _) = render_with(&device, &queue, options);
        assert!(image == both, "{options:?}");
    }
}

/// `cargo test --release -p misty_engine stress_benchmark -- --ignored --nocapture`
#[test]
#[ignore = "benchmark: renders StressDemo for a few hundred frames"]
fn stress_benchmark() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let (scene, _) = crate::scene::Scene::load(&root.join("Assets/Scenes/StressDemo.scene"))
        .expect("StressDemo.scene (generate it with write_stress_demo)");
    let materials = crate::assets::AssetCache::open(&root);
    let configs = [
        ("baseline (no culling, no instancing)", false, false),
        ("culling only", true, false),
        ("instancing only", false, true),
        ("culling + instancing", true, true),
    ];
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, 1280, 720);
    for (name, culling, instancing) in configs {
        renderer.set_options(RenderOptions {
            culling,
            instancing,
        });
        let frames = 120;
        let (mut gather, mut cpu, mut total) = (0.0, 0.0, 0.0);
        let mut stats = RenderStats::default();
        for i in 0..frames + 10 {
            let start = std::time::Instant::now();
            let mut view = crate::render_scene::GameView::new(&scene, &materials).expect("camera");
            // Orbit like the demo's camera script.
            let angle = i as f32 * 0.02;
            view.camera.position = Vec3::new(angle.sin() * 70.0, 30.0, -angle.cos() * 70.0);
            view.camera.rotation = Quat::from_rotation_y(-angle) * Quat::from_rotation_x(0.38);
            let gathered = start.elapsed().as_secs_f32() * 1000.0;
            renderer.render(&device, &queue, &target, &view.frame());
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 10 {
                gather += gathered;
                cpu += renderer.stats().cpu_ms;
                total += start.elapsed().as_secs_f32() * 1000.0;
                stats = renderer.stats();
            }
        }
        let n = frames as f32;
        println!(
            "{name}: {:.1} fps | frame {:.2} ms = gather {:.2} + render CPU {:.2} + GPU wait | \
             batches {} (saved {}), tris {}k, visible {}, culled {}, shadow casters {}",
            1000.0 / (total / n),
            total / n,
            gather / n,
            cpu / n,
            stats.batches,
            stats.saved_by_batching,
            stats.triangles / 1000,
            stats.visible,
            stats.culled,
            stats.shadow_casters,
        );
    }
}
