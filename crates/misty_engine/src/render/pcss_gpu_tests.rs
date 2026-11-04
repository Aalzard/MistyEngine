//! Headless GPU check of contact-hardening shadows: a thin pole under a low sun casts a shadow
//! whose edges are sharp at its base and widen towards the tip.

use glam::{Mat4, Quat, Vec3};

use super::super::gpu_tests::{headless, read_back};
use super::super::*;
use crate::camera::{Camera, Projection};
use crate::scene::PrimitiveMesh;

const SIZE: u32 = 512;
/// The top-down view spans x = -1..15 m and z = -8..8 m.
const HALF: f32 = 8.0;
const LEFT: f32 = -1.0;

fn draw(mesh: PrimitiveMesh, model: Mat4) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material: MaterialParams {
            smoothness: 0.0,
            ..Default::default()
        },
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
    }
}

/// Top-down image of a 6 m pole lit by a 25° sun travelling along +X.
fn render_pole(shadows: Shadows, angle: f32) -> Vec<u8> {
    let (device, queue) = headless().expect("adapter");
    let camera = Camera {
        position: Vec3::new(LEFT + HALF, 30.0, 0.0),
        // Looking straight down: image right = +X, image up = +Z.
        rotation: Quat::from_rotation_x(90f32.to_radians()),
        projection: Projection::Orthographic { size: HALF },
        near: 0.3,
        far: 100.0,
    };
    let meshes = [
        draw(
            PrimitiveMesh::Plane,
            Mat4::from_scale(Vec3::new(6.0, 1.0, 6.0)),
        ),
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_scale_rotation_translation(
                Vec3::new(0.3, 6.0, 0.3),
                Quat::IDENTITY,
                Vec3::new(0.0, 3.0, 0.0),
            ),
        ),
    ];
    let elevation = 25f32.to_radians();
    let lights = [LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(elevation.cos(), -elevation.sin(), 0.0),
        color: [1.0; 3],
        intensity: 1.5,
        range: 0.0,
        spot_angle: 0.0,
        shadows,
        shadow: LightShadow {
            size: angle,
            ..Default::default()
        },
        volumetric: Default::default(),
    }];
    let environment = Environment::default();
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: false,
        background: [0.0; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
    };
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, SIZE, SIZE);
    renderer.render(&device, &queue, &target, &frame);
    let pixels = read_back(&device, &queue, &target);
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_DIR") {
        let path = std::path::Path::new(&dir).join(format!("pcss_{shadows:?}_{angle}.png"));
        image::RgbaImage::from_raw(SIZE, SIZE, pixels.clone())
            .expect("size")
            .save(path)
            .expect("png");
    }
    pixels
}

/// Brightness down the image column at world x (across the shadow).
fn column(pixels: &[u8], x: f32) -> Vec<f32> {
    let col = ((x - LEFT) / (2.0 * HALF) * SIZE as f32) as usize;
    (0..SIZE as usize)
        .map(|row| {
            let i = (row * SIZE as usize + col) * 4;
            pixels[i..i + 3].iter().map(|v| *v as f32).sum::<f32>()
        })
        .collect()
}

/// Pixels between 10% and 90% of the way from the shadow's darkest point to the lit ground.
fn edge_width(profile: &[f32]) -> usize {
    let lit = profile.iter().cloned().fold(f32::MIN, f32::max);
    let dark = profile.iter().cloned().fold(f32::MAX, f32::min);
    assert!(lit - dark > 30.0, "no shadow here: {dark}..{lit}");
    let (lo, hi) = (dark + 0.1 * (lit - dark), dark + 0.9 * (lit - dark));
    profile.iter().filter(|v| **v > lo && **v < hi).count()
}

#[test]
fn penumbra_widens_with_distance_from_the_caster() {
    if headless().is_none() {
        eprintln!("no GPU adapter: skipped");
        return;
    }
    let soft = render_pole(Shadows::Soft, 4.0);
    // Near the base (~1 m from the pole) and near the tip's shadow (~11 m away).
    let near = edge_width(&column(&soft, 1.0));
    let far = edge_width(&column(&soft, 10.0));
    eprintln!("soft edge width: near {near} px, far {far} px");
    assert!(near <= 8, "contact shadow should stay sharp: {near} px");
    assert!(
        far >= 3 * near.max(2),
        "penumbra should widen: {near} → {far} px"
    );

    // Hard shadows don't widen.
    let hard = render_pole(Shadows::Hard, 4.0);
    let (hn, hf) = (
        edge_width(&column(&hard, 1.0)),
        edge_width(&column(&hard, 10.0)),
    );
    eprintln!("hard edge width: near {hn} px, far {hf} px");
    assert!(hf <= hn + 3, "{hn} → {hf}");
}

/// Mostly `rgb`'s channels: the debug tint of one cascade.
fn tinted(pixels: &[u8], rgb: [bool; 3]) -> bool {
    pixels.chunks(4).any(|p| {
        let on = (0..3).filter(|i| rgb[*i]).map(|i| p[i] as i32).min();
        let off = (0..3).filter(|i| !rgb[*i]).map(|i| p[i] as i32).max();
        on.zip(off)
            .is_some_and(|(on, off)| on > 90 && off * 4 < on * 3)
    })
}

/// ShadowsDemo through its game camera, plain and with the cascades tinted
/// (`MISTY_GPU_TEST_DIR=<dir>` saves both; `MISTY_SHADOW_CAMERA=x,y,z,pitch,yaw` moves the view).
#[test]
fn shadows_demo_shows_every_cascade() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let (scene, _) = crate::scene::Scene::load(&root.join("Assets/Scenes/ShadowsDemo.scene"))
        .expect("ShadowsDemo.scene (generate it with write_shadows_demo)");
    let materials = crate::assets::AssetCache::open(&root);
    let mut view = crate::render_scene::GameView::new(&scene, &materials).expect("camera");
    if let Ok(pose) = std::env::var("MISTY_SHADOW_CAMERA") {
        let v: Vec<f32> = pose.split(',').filter_map(|p| p.parse().ok()).collect();
        view.camera.position = Vec3::new(v[0], v[1], v[2]);
        view.camera.rotation = crate::camera::euler_to_quat(Vec3::new(v[3], v[4], 0.0));
    }
    let (width, height) = (960, 540);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, width, height);
    let mut images = Vec::new();
    for debug in [false, true] {
        renderer.set_cascade_debug(debug);
        renderer.render(&device, &queue, &target, &view.frame());
        let pixels = read_back(&device, &queue, &target);
        if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_DIR") {
            let path = std::path::Path::new(&dir).join(format!("shadows_demo_{debug}.png"));
            image::RgbaImage::from_raw(width, height, pixels.clone())
                .expect("size")
                .save(path)
                .expect("png");
        }
        images.push(pixels);
    }
    if std::env::var_os("MISTY_SHADOW_CAMERA").is_some() {
        return;
    }
    for (name, rgb) in [
        ("red", [true, false, false]),
        ("green", [false, true, false]),
        ("blue", [false, false, true]),
        ("yellow", [true, true, false]),
    ] {
        assert!(tinted(&images[1], rgb), "no {name} cascade on screen");
    }
    assert!(!tinted(&images[0], [false, true, false]), "untinted image");
    // The tint is a one-render switch.
    renderer.render(&device, &queue, &target, &view.frame());
    assert!(read_back(&device, &queue, &target) == images[0]);
}
