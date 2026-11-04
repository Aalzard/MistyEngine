//! Headless probe checks on the real adapter (skipped without one): capture orientation, and a
//! chrome sphere in a red room reflecting red once its probe is baked.

use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};

use super::capture::{CaptureInputs, CaptureSettings, ProbeCapture};
use super::{ProbeData, ProbeHandle, ProbeInstance, ProbeSet};
use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    Environment, Frame, LightData, MaterialParams, MeshDraw, PostSettings, Renderer,
};
use crate::scene::reflection_probe::ProbeClearFlags;
use crate::scene::{LightType, PrimitiveMesh, Shadows};

fn draw(mesh: PrimitiveMesh, model: Mat4, material: MaterialParams) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material,
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
    }
}

fn unlit(color: [f32; 3]) -> MaterialParams {
    MaterialParams {
        color: [color[0], color[1], color[2], 1.0],
        unlit: true,
        ..Default::default()
    }
}

fn settings(position: Vec3, size: u32) -> CaptureSettings {
    CaptureSettings {
        position,
        size,
        near: 0.05,
        far: 100.0,
        clear: ProbeClearFlags::SolidColor,
        background: [0.0; 3],
    }
}

/// Each face shows what lies in its direction (+X red, +Y green, −Z blue).
#[test]
fn faces_capture_their_own_direction() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let quad = |at: Vec3, color| {
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, at),
            unlit(color),
        )
    };
    let inputs = CaptureInputs {
        meshes: vec![
            quad(Vec3::X * 5.0, [1.0, 0.0, 0.0]),
            quad(Vec3::Y * 5.0, [0.0, 1.0, 0.0]),
            quad(Vec3::NEG_Z * 5.0, [0.0, 0.0, 1.0]),
        ],
        lights: Vec::new(),
        terrains: Vec::new(),
        environment: Environment::default(),
    };
    let mut renderer = Renderer::new(&device);
    let capture = ProbeCapture::new(&renderer, &device, 64);
    let cube = capture.cube(
        &mut renderer,
        &device,
        &queue,
        &inputs,
        &settings(Vec3::ZERO, 64),
    );
    let center = |face: usize| cube.faces[face][32 * 64 + 32];
    assert!(center(0).x > 0.5 && center(0).y < 0.1, "{:?}", center(0));
    assert!(center(2).y > 0.5 && center(2).x < 0.1, "{:?}", center(2));
    assert!(center(5).z > 0.5 && center(5).x < 0.1, "{:?}", center(5));
    assert!(center(1).max_element() < 0.05 && center(4).max_element() < 0.05);
    // The left edge of +X (u = -1) looks towards +Z: the red quad doesn't reach there.
    assert!(cube.faces[0][32 * 64 + 1].x < 0.1);
}

/// Red walls, floor and ceiling around a chrome sphere, lit by a point light.
fn red_room() -> (Vec<MeshDraw>, Vec<LightData>) {
    let red = MaterialParams {
        color: [0.9, 0.08, 0.06, 1.0],
        smoothness: 0.2,
        ..Default::default()
    };
    let slab = |at: Vec3, scale: Vec3| {
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, at),
            red.clone(),
        )
    };
    let chrome = MaterialParams {
        color: [0.95, 0.95, 0.95, 1.0],
        metallic: 1.0,
        smoothness: 0.95,
        ..Default::default()
    };
    let meshes = vec![
        slab(Vec3::new(0.0, -0.1, 0.0), Vec3::new(6.4, 0.2, 6.4)),
        slab(Vec3::new(0.0, 3.1, 0.0), Vec3::new(6.4, 0.2, 6.4)),
        slab(Vec3::new(-3.1, 1.5, 0.0), Vec3::new(0.2, 3.0, 6.0)),
        slab(Vec3::new(3.1, 1.5, 0.0), Vec3::new(0.2, 3.0, 6.0)),
        slab(Vec3::new(0.0, 1.5, -3.1), Vec3::new(6.0, 3.0, 0.2)),
        slab(Vec3::new(0.0, 1.5, 3.1), Vec3::new(6.0, 3.0, 0.2)),
        draw(
            PrimitiveMesh::Sphere,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(1.2),
                Quat::IDENTITY,
                Vec3::new(0.0, 1.5, 0.0),
            ),
            chrome,
        ),
    ];
    let light = LightData {
        kind: LightType::Point,
        position: Vec3::new(1.0, 2.5, -1.5),
        direction: Vec3::Z,
        color: [1.0; 3],
        intensity: 2.0,
        range: 12.0,
        spot_angle: 30.0,
        shadows: Shadows::None,
        contact_shadows: false,
        shadow: Default::default(),
        volumetric: Default::default(),
    };
    (meshes, vec![light])
}

fn render_room(
    renderer: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    env: &Environment,
) -> Vec<u8> {
    let (meshes, lights) = red_room();
    let camera = Camera {
        position: Vec3::new(0.0, 1.5, -2.6),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.05,
        far: 50.0,
    };
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: true,
        background: [0.0; 3],
        environment: env,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
    };
    let target = renderer.create_target(device, 96, 96);
    renderer.render(device, queue, &target, &frame);
    read_back(device, queue, &target)
}

#[test]
fn a_chrome_sphere_in_a_red_room_reflects_red() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&device);
    let (meshes, lights) = red_room();
    let sky = Environment::new(Default::default(), &[]);
    let inputs = CaptureInputs {
        meshes,
        lights,
        terrains: Vec::new(),
        environment: sky.clone(),
    };
    let at = Vec3::new(0.0, 1.5, 0.0);
    let capture = ProbeCapture::new(&renderer, &device, 64);
    let cube = capture.cube(
        &mut renderer,
        &device,
        &queue,
        &inputs,
        &CaptureSettings {
            clear: ProbeClearFlags::Skybox,
            ..settings(at, 64)
        },
    );
    let data = Arc::new(ProbeData::from_capture(&cube, 32));
    // The captured room is red all around, so its diffuse SH is red too.
    let ambient = super::sh::eval(&data.sh, Vec3::Y);
    assert!(
        ambient.x > ambient.y * 3.0 && ambient.x > ambient.z * 3.0,
        "{ambient:?}"
    );

    let probe = ProbeInstance {
        position: at,
        box_min: Vec3::new(-3.1, -0.1, -3.1),
        box_max: Vec3::new(3.1, 3.1, 3.1),
        blend_distance: 0.1,
        importance: 1,
        intensity: 1.0,
        box_projection: true,
        data: ProbeHandle(data),
    };
    let lit = sky.clone().with_probes(ProbeSet {
        probes: vec![probe],
        debug: false,
    });
    let before = render_room(&mut renderer, &device, &queue, &sky);
    let after = render_room(&mut renderer, &device, &queue, &lit);
    if let Some(path) = std::env::var_os("MISTY_GPU_TEST_PNG") {
        image::RgbaImage::from_raw(96, 96, after.clone())
            .expect("size")
            .save(path)
            .expect("png");
    }
    // Centre of the sphere: sky-blue tint without the probe, red with it.
    let px = |img: &[u8]| {
        let i = (48 * 96 + 48) * 4;
        [img[i] as f32, img[i + 1] as f32, img[i + 2] as f32]
    };
    let (b, a) = (px(&before), px(&after));
    assert!(b[2] > b[0], "sky reflection before: {b:?}");
    assert!(
        a[0] > a[1] * 2.0 && a[0] > a[2] * 2.0,
        "red reflection after: {a:?}"
    );
}
