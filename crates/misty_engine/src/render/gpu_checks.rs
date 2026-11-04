//! GPU checks of the round-8 renderer features: per-object material overrides batch and render
//! like separate materials; static batching, GPU culling and the depth prepass render like the
//! plain path; LOD Groups switch levels. Skipped when no adapter exists.

use glam::{Mat4, Quat, Vec3};

use super::gpu_tests::{headless, read_back};
use super::lod::LodRange;
use super::overrides::{MaterialOverrides, OverrideValue};
use super::*;
use crate::camera::{Camera, Projection};
use crate::scene::PrimitiveMesh;

const SIZE: (u32, u32) = (320, 200);

fn camera(position: Vec3, pitch: f32) -> Camera {
    Camera {
        position,
        rotation: Quat::from_rotation_x(pitch.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 80.0,
    }
}

fn sun(shadows: Shadows) -> Vec<LightData> {
    vec![LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.3, -1.0, 0.6).normalize(),
        color: [1.0, 0.95, 0.9],
        intensity: 1.2,
        range: 0.0,
        spot_angle: 0.0,
        shadows,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
    }]
}

fn draw(mesh: PrimitiveMesh, model: Mat4, material: MaterialParams) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material,
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

/// Render `meshes` (several frames, so GPU culling has last frame's visibility).
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    camera: &Camera,
    meshes: &[MeshDraw],
    lights: &[LightData],
    frames: usize,
) -> (Vec<u8>, RenderStats) {
    let environment = Environment::default();
    let frame = Frame {
        camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        water: &[],
        shaded: true,
        lights,
    };
    let target = renderer.create_target(device, SIZE.0, SIZE.1);
    for _ in 0..frames {
        renderer.render(device, queue, &target, &frame);
    }
    (read_back(device, queue, &target), renderer.stats())
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count()
}

/// Save an image when `MISTY_GPU_TEST_DIR` is set (for a look).
fn save(name: &str, pixels: &[u8]) {
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_DIR") {
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        let img = image::RgbaImage::from_raw(SIZE.0, SIZE.1, pixels.to_vec()).expect("size");
        img.save(path).expect("png");
    }
}

/// An 8×8 checker texture on disk (UV changes are visible on it).
fn checker() -> String {
    let path = std::env::temp_dir().join("misty_override_checker.png");
    let img = image::RgbaImage::from_fn(8, 8, |x, y| {
        let c = if (x + y) % 2 == 0 { 230 } else { 40 };
        image::Rgba([c, c, c, 255])
    });
    img.save(&path).expect("checker png");
    path.to_string_lossy().into_owned()
}

#[test]
fn overrides_keep_one_batch_and_render_like_separate_materials() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let shared = MaterialParams {
        albedo_map: Some(checker()),
        smoothness: 0.3,
        ..Default::default()
    };
    let wall = |x: f32| {
        Mat4::from_scale_rotation_translation(
            Vec3::new(1.8, 2.0, 0.2),
            Quat::IDENTITY,
            Vec3::new(x, 1.0, 0.0),
        )
    };
    // The middle wall: other UVs and a tint, as per-object overrides of the shared material.
    let mut o = MaterialOverrides::default();
    o.set("tiling", OverrideValue::Vec2([3.0, 2.0]));
    o.set("offset", OverrideValue::Vec2([0.25, 0.0]));
    o.set("rotation", OverrideValue::Float(30.0));
    o.set("color", OverrideValue::Color([1.0, 0.4, 0.3, 1.0]));
    o.set("smoothness", OverrideValue::Float(2.0));
    let mut tinted = shared.clone();
    o.apply(&mut tinted);
    let overridden: Vec<MeshDraw> = [-2.2, 0.0, 2.2]
        .iter()
        .enumerate()
        .map(|(i, x)| {
            let m = if i == 1 { &tinted } else { &shared };
            draw(PrimitiveMesh::Cube, wall(*x), m.clone())
        })
        .collect();
    let cam = camera(Vec3::new(0.0, 1.2, -6.0), 4.0);
    let lights = sun(Shadows::None);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let (image, stats) = render(
        &device,
        &queue,
        &mut renderer,
        &cam,
        &overridden,
        &lights,
        1,
    );
    save("overrides", &image);
    // One instanced draw for all three walls.
    assert_eq!(
        (stats.batches, stats.saved_by_batching),
        (1, 2),
        "{stats:?}"
    );

    // The same picture as a separate material for the middle wall (drawn on its own).
    let mut separate = Renderer::with_queue(&device, &queue);
    separate.set_options(RenderOptions {
        instancing: false,
        ..Default::default()
    });
    let (reference, ref_stats) = render(
        &device,
        &queue,
        &mut separate,
        &cam,
        &overridden,
        &lights,
        1,
    );
    assert_eq!(ref_stats.batches, 3);
    assert_eq!(differing(&image, &reference), 0);
    // And the override is visible: the middle wall differs from an unmodified one.
    let plain: Vec<MeshDraw> = [-2.2, 0.0, 2.2]
        .iter()
        .map(|x| draw(PrimitiveMesh::Cube, wall(*x), shared.clone()))
        .collect();
    let (untouched, _) = render(&device, &queue, &mut renderer, &cam, &plain, &lights, 1);
    let changed = differing(&image, &untouched);
    assert!(changed > 1000, "{changed} pixels changed");
    // Only the middle third changes.
    let row = |img: &[u8], x: u32| {
        let y = SIZE.1 / 2;
        img[((y * SIZE.0 + x) * 4) as usize..][..4].to_vec()
    };
    assert_eq!(row(&image, SIZE.0 / 4), row(&untouched, SIZE.0 / 4));
    assert_ne!(row(&image, SIZE.0 / 2), row(&untouched, SIZE.0 / 2));
}

/// A field of cubes and spheres behind a wall, in two materials, on a ground plane.
fn occlusion_scene() -> Vec<MeshDraw> {
    let red = MaterialParams {
        color: [0.9, 0.3, 0.25, 1.0],
        ..Default::default()
    };
    let blue = MaterialParams {
        color: [0.3, 0.5, 0.9, 1.0],
        metallic: 0.6,
        ..Default::default()
    };
    let mut out = vec![
        draw(
            PrimitiveMesh::Plane,
            Mat4::from_scale(Vec3::new(6.0, 1.0, 6.0)),
            red.clone(),
        ),
        // The occluder: a wall across the middle.
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_scale_rotation_translation(
                Vec3::new(14.0, 5.0, 0.5),
                Quat::IDENTITY,
                Vec3::new(0.0, 2.5, 2.0),
            ),
            blue.clone(),
        ),
    ];
    for x in -6i32..=6 {
        for z in -4i32..=12 {
            let mesh = if (x + z) % 2 == 0 {
                PrimitiveMesh::Cube
            } else {
                PrimitiveMesh::Sphere
            };
            let m = if (x * 7 + z) % 3 == 0 { &blue } else { &red };
            let model = Mat4::from_scale_rotation_translation(
                Vec3::splat(0.7),
                Quat::from_rotation_y(x as f32 * 0.3),
                Vec3::new(x as f32 * 1.6, 0.5, z as f32 * 1.6),
            );
            out.push(draw(mesh, model, m.clone()));
        }
    }
    out
}

#[test]
fn gpu_culling_and_depth_prepass_render_identically() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let meshes = occlusion_scene();
    let lights = sun(Shadows::Soft);
    let cam = camera(Vec3::new(0.0, 2.0, -9.0), 5.0);
    let mut plain = Renderer::with_queue(&device, &queue);
    let (reference, _) = render(&device, &queue, &mut plain, &cam, &meshes, &lights, 1);
    save("occlusion_cpu", &reference);

    let mut gpu = Renderer::with_queue(&device, &queue);
    if gpu.culler.is_none() {
        eprintln!("no INDIRECT_FIRST_INSTANCE: GPU culling skipped");
    } else {
        gpu.set_options(RenderOptions {
            gpu_culling: true,
            ..Default::default()
        });
        // Frame 1 draws everything first; from frame 2 the wall hides most of the field.
        let (image, _) = render(&device, &queue, &mut gpu, &cam, &meshes, &lights, 3);
        save("occlusion_gpu", &image);
        assert_eq!(
            differing(&image, &reference),
            0,
            "GPU culling changed pixels"
        );
        let culler = gpu.culler.as_ref().unwrap();
        let candidates = meshes.len() as u32;
        let [first, second] = culler.read_counts(&device, &queue, gpu.last_gpu_batches);
        eprintln!("gpu culling: {first} + {second} of {candidates} drawn");
        assert!(first + second < candidates / 2, "the wall hides the field");
        assert!(first > 2, "the wall and ground are drawn");
    }

    let mut pre = Renderer::with_queue(&device, &queue);
    pre.set_options(RenderOptions {
        depth_prepass: true,
        ..Default::default()
    });
    let (image, stats) = render(&device, &queue, &mut pre, &cam, &meshes, &lights, 1);
    assert_eq!(
        differing(&image, &reference),
        0,
        "the prepass changed pixels"
    );
    assert!(stats.batches > 0);
}

#[test]
fn static_batching_renders_like_separate_objects() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let meshes = occlusion_scene();
    let merged = static_batch::combine("gpu-check", &meshes);
    assert!(merged.len() < meshes.len() / 4, "{} batches", merged.len());
    let lights = sun(Shadows::Soft);
    let cam = camera(Vec3::new(0.0, 7.0, -12.0), 25.0);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let (reference, ref_stats) = render(&device, &queue, &mut renderer, &cam, &meshes, &lights, 1);
    let (image, stats) = render(&device, &queue, &mut renderer, &cam, &merged, &lights, 1);
    save("static_off", &reference);
    save("static_on", &image);
    crate::models::runtime::unregister("gpu-check/");
    // Vertices moved on the CPU can round differently: allow a few edge pixels, by a little.
    let diff = differing(&image, &reference);
    let worst = image
        .iter()
        .zip(&reference)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    eprintln!("static batching: {diff} pixels differ, by at most {worst}");
    assert!(
        diff * 1000 < (SIZE.0 * SIZE.1) as usize,
        "{diff} pixels differ"
    );
    // Far fewer objects for the CPU to cull and batch.
    let objects = |s: &RenderStats| s.visible + s.culled;
    assert!(
        objects(&stats) * 4 < objects(&ref_stats),
        "{stats:?} vs {ref_stats:?}"
    );
}

#[test]
fn lod_levels_switch_with_distance_and_cross_fade() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let lit = MaterialParams::default();
    let level = |mesh, min, max, fade| MeshDraw {
        lod: Some(LodRange {
            center: Vec3::new(0.0, 1.0, 0.0),
            size: 2.0,
            min,
            max,
            fade,
        }),
        ..draw(mesh, Mat4::from_translation(Vec3::Y), lit.clone())
    };
    let group = |fade| {
        vec![
            level(PrimitiveMesh::Cube, 0.5, f32::INFINITY, fade),
            level(PrimitiveMesh::Sphere, 0.0, 0.5, fade),
        ]
    };
    let only = |mesh| vec![draw(mesh, Mat4::from_translation(Vec3::Y), lit.clone())];
    let lights = sun(Shadows::Hard);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let mut shot = |cam: &Camera, meshes: &[MeshDraw]| {
        render(&device, &queue, &mut renderer, cam, meshes, &lights, 1).0
    };
    // Near: the cube (LOD 0); far: the sphere (LOD 1).
    let near = camera(Vec3::new(0.0, 1.0, -3.0), 0.0);
    let far = camera(Vec3::new(0.0, 1.0, -6.0), 0.0);
    assert_eq!(
        shot(&near, &group(0.0)),
        shot(&near, &only(PrimitiveMesh::Cube))
    );
    assert_eq!(
        shot(&far, &group(0.0)),
        shot(&far, &only(PrimitiveMesh::Sphere))
    );
    // Inside the fade band both draw, dithered: neither level alone.
    let tan = 25f32.to_radians().tan();
    let d = 2.0 / (2.0 * 0.55 * tan);
    let band = camera(Vec3::new(0.0, 1.0, -d), 0.0);
    let faded = shot(&band, &group(0.2));
    save("lod_fade", &faded);
    assert_ne!(faded, shot(&band, &only(PrimitiveMesh::Cube)));
    assert_ne!(faded, shot(&band, &only(PrimitiveMesh::Sphere)));
    // The LOD Bias keeps the finer level farther away.
    renderer.set_options(RenderOptions {
        lod_bias: 2.0,
        ..Default::default()
    });
    let mut shot = |cam: &Camera, meshes: &[MeshDraw]| {
        render(&device, &queue, &mut renderer, cam, meshes, &lights, 1).0
    };
    assert_eq!(
        shot(&far, &group(0.0)),
        shot(&far, &only(PrimitiveMesh::Cube))
    );
}
