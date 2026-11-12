//! Headless renders of rectangle lights (skipped without a GPU adapter) and their GPU cost.

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    AreaShape, Environment, Frame, LightData, MaterialParams, MeshDraw, PostSettings, Renderer,
};
use crate::scene::render_settings::RenderSettings;
use crate::scene::{LightType, PrimitiveMesh, Shadows};

fn floor_and_ball() -> Vec<MeshDraw> {
    let grey = MaterialParams {
        color: [0.7, 0.7, 0.7, 1.0],
        smoothness: 0.6,
        ..Default::default()
    };
    let draw = |mesh: PrimitiveMesh, model| MeshDraw {
        mesh: mesh.into(),
        model,
        material: grey.clone(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    };
    vec![
        draw(
            PrimitiveMesh::Plane,
            Mat4::from_scale(Vec3::new(2.0, 1.0, 2.0)),
        ),
        draw(
            PrimitiveMesh::Sphere,
            Mat4::from_translation(Vec3::new(1.5, 0.5, 1.5)),
        ),
    ]
}

/// A rectangle light at `position` whose forward axis is `right × up`.
pub(crate) fn rect(position: Vec3, right: Vec3, up: Vec3, size: Vec2) -> LightData {
    LightData {
        kind: LightType::Rectangle,
        position,
        direction: right.cross(up).normalize(),
        color: [1.0, 0.9, 0.8],
        intensity: 6.0,
        range: 12.0,
        spot_angle: 0.0,
        shadows: Shadows::Soft,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: AreaShape {
            size,
            right,
            up,
            two_sided: false,
            show_emitter: false,
            glow: Default::default(),
        },
    }
}

/// No ambient or sky light: only the frame's lights.
fn dark() -> Environment {
    let mut settings = RenderSettings::default();
    settings.environment_lighting.intensity = 0.0;
    settings.reflection_intensity = 0.0;
    Environment::new(settings, &[])
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    meshes: &[MeshDraw],
    lights: &[LightData],
    frames: usize,
) -> (Vec<u8>, Option<f32>) {
    let camera = Camera {
        position: Vec3::new(0.0, 8.0, -8.0),
        rotation: Quat::from_rotation_x(45f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 60.0,
    };
    let environment = dark();
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: false,
        background: [0.0; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights,
        water: &[],
    };
    let mut renderer = Renderer::with_queue(device, queue);
    let target = renderer.create_target(device, size.0, size.1);
    let mut times = Vec::new();
    for _ in 0..frames {
        renderer.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        times.extend(renderer.gpu_ms());
    }
    times.sort_by(f32::total_cmp);
    (
        read_back(device, queue, &target),
        times.get(times.len() / 2).copied(),
    )
}

fn luma(px: &[u8], w: u32, x: u32, y: u32) -> u32 {
    let i = ((y * w + x) * 4) as usize;
    px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32
}

#[test]
fn rectangle_lights_shine_from_their_front_face() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (96, 96);
    let meshes = floor_and_ball();
    // 2 m × 0.5 m panel 2 m above the floor, facing down (X × Z = -Y).
    let down = rect(
        Vec3::new(0.0, 2.0, 0.0),
        Vec3::X,
        Vec3::Z,
        Vec2::new(2.0, 0.5),
    );
    let (lit, _) = render(&device, &queue, (w, h), &meshes, &[down], 1);
    if let Some(path) = std::env::var_os("MISTY_AREA_TEST_PNG") {
        image::RgbaImage::from_raw(w, h, lit.clone())
            .expect("size")
            .save(path)
            .expect("png");
    }
    let centre = luma(&lit, w, w / 2, h / 2);
    assert!(centre > 60, "floor under the panel is lit ({centre})");
    // Facing up, its back faces the floor: dark. Two-sided: lit again.
    let mut up = down;
    up.area.up = Vec3::NEG_Z;
    up.direction = Vec3::Y;
    let (back, _) = render(&device, &queue, (w, h), &meshes, &[up], 1);
    assert!(luma(&back, w, w / 2, h / 2) < 8, "one-sided");
    up.area.two_sided = true;
    let (both, _) = render(&device, &queue, (w, h), &meshes, &[up], 1);
    assert!(luma(&both, w, w / 2, h / 2) + 10 >= centre, "two-sided");
    // The emitter quad shows the light itself (brighter than the floor it lights).
    let mut shown = down;
    shown.position.y = 4.0;
    shown.area.show_emitter = true;
    shown.area.two_sided = true;
    let (quad, _) = render(&device, &queue, (w, h), &meshes, &[shown], 1);
    let brightest = (0..w * h).map(|i| luma(&quad, w, i % w, i / w)).max();
    assert!(brightest > Some(700), "{brightest:?}");
}

/// `cargo test --release -p misty_engine -- --ignored area_light_benchmark --nocapture`
#[test]
#[ignore = "benchmark: rectangle light GPU cost at 1080p"]
fn area_light_benchmark() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let meshes = floor_and_ball();
    let ring = |i: usize| {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        Vec3::new(a.cos() * 3.0, 2.5, a.sin() * 3.0)
    };
    let rects: Vec<LightData> = (0..8)
        .map(|i| {
            let mut l = rect(ring(i), Vec3::X, Vec3::Z, Vec2::new(1.5, 0.5));
            l.shadows = Shadows::None;
            l
        })
        .collect();
    let points: Vec<LightData> = rects
        .iter()
        .map(|l| LightData {
            kind: LightType::Point,
            ..*l
        })
        .collect();
    let mut shadowed = rects.clone();
    for l in shadowed.iter_mut().take(4) {
        l.shadows = Shadows::Soft;
    }
    let size = (1920, 1080);
    let ms = |lights: &[LightData]| render(&device, &queue, size, &meshes, lights, 60).1;
    // Warm up first (pipeline creation, GPU clocks).
    let _ = ms(&points);
    let none = ms(&[]);
    let point = ms(&points);
    let rect = ms(&rects);
    let soft = ms(&shadowed);
    eprintln!(
        "1080p GPU: no lights {none:?} ms, 8 point {point:?} ms, 8 rectangle {rect:?} ms, \
         8 rectangle with 4 soft-shadowed {soft:?} ms"
    );
}
