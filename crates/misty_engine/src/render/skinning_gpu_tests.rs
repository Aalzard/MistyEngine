//! Headless checks of GPU skinning against the CPU reference ([`crate::skinning::deform`]):
//! the compute output read back vertex by vertex, and whole frames (shadows, depth prepass,
//! GPU culling) drawn from the GPU-skinned mesh against the same pose deformed on the CPU.

use glam::{Mat4, Quat, Vec3};

use super::Skins;
use crate::camera::{Camera, Projection};
use crate::models;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    Environment, Frame, LightData, MaterialParams, MeshDraw, PostSettings, RenderOptions, Renderer,
};
use crate::scene::{LightType, MeshRef, PrimitiveMesh, Shadows};
use crate::skinning::{self, SkinPose};

/// The sample tentacle in a temp folder (absolute path).
fn tentacle() -> String {
    let dir = std::env::temp_dir().join(format!("misty_skin_gpu_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Tentacle.glb");
    if !file.exists() {
        std::fs::write(&file, skinning::sample::tentacle_glb()).unwrap();
    }
    file.to_string_lossy().replace('\\', "/")
}

/// A strongly bent pose of the tentacle with both blend shapes partly on.
fn bent_pose(rel: &str) -> SkinPose {
    let model = models::model(rel).unwrap();
    let m = &model.meshes[0];
    let skin = m.skin.as_ref().unwrap();
    // Chain of bones along +Y, each turned about Z (and a little about X).
    let mut world = Mat4::IDENTITY;
    let mut matrices = Vec::new();
    for (j, y) in skinning::sample::JOINTS.iter().enumerate() {
        let prev = if j == 0 {
            0.0
        } else {
            skinning::sample::JOINTS[j - 1]
        };
        world = world
            * Mat4::from_translation(Vec3::new(0.0, y - prev, 0.0))
            * Mat4::from_quat(Quat::from_rotation_z(0.5) * Quat::from_rotation_x(0.15));
        matrices.push(world * skin.bind_poses[j]);
    }
    let bounds = skinning::posed_bounds(skin, &matrices).unwrap();
    SkinPose {
        mesh: MeshRef::asset(rel, 0),
        matrices,
        weights: vec![0.8, 0.35],
        bounds,
        version: 0,
        vertices: None,
    }
}

#[test]
fn gpu_skinning_matches_the_cpu_reference_vertex_for_vertex() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let rel = tentacle();
    let key = "skinned://gpu-test/1#0";
    let pose = skinning::publish(key, bent_pose(&rel));
    let mut skins = Skins::default();
    skins.prepare(&device, &queue, [key].into_iter());
    assert_eq!(skins.dispatched, 1);
    let gpu = skins.read_vertices(&device, &queue, key).expect("posed");
    let cpu = skinning::deform_pose(&pose).unwrap();
    assert_eq!(gpu.len(), cpu.len());
    let mut worst = 0.0f32;
    for (g, c) in gpu.iter().zip(&cpu) {
        let dp = (glam::Vec3::from(g.position) - glam::Vec3::from(c.position)).length();
        let dn = (glam::Vec3::from(g.normal) - glam::Vec3::from(c.normal)).length();
        worst = worst.max(dp).max(dn);
        assert_eq!(g.uv, c.uv);
    }
    assert!(worst < 1e-4, "worst difference {worst}");
    // The deformation is real: the tip moved far from its bind position.
    let model = models::model(&rel).unwrap();
    let tip = model.meshes[0].data.vertices.len() - 1;
    let moved = glam::Vec3::from(gpu[tip].position)
        - glam::Vec3::from(model.meshes[0].data.vertices[tip].position);
    assert!(moved.length() > 0.5, "{moved}");

    // An unchanged pose is not skinned again; a changed one is.
    skins.prepare(&device, &queue, [key].into_iter());
    assert_eq!(skins.dispatched, 0);
    let mut changed = bent_pose(&rel);
    changed.weights = vec![0.0, 0.0];
    skinning::publish(key, changed);
    skins.prepare(&device, &queue, [key].into_iter());
    assert_eq!(skins.dispatched, 1);
}

fn draw(mesh: MeshRef, model: Mat4, color: [f32; 4]) -> MeshDraw {
    MeshDraw {
        mesh,
        model,
        material: MaterialParams {
            color,
            smoothness: 0.4,
            ..Default::default()
        },
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    options: RenderOptions,
    meshes: &[MeshDraw],
) -> Vec<u8> {
    let camera = Camera {
        position: Vec3::new(0.0, 2.2, -5.5),
        rotation: Quat::from_rotation_x(15f32.to_radians()),
        projection: Projection::Perspective { fov_y: 45.0 },
        near: 0.1,
        far: 40.0,
    };
    let sun = LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.6, -1.0, 0.4).normalize(),
        color: [1.0, 0.95, 0.9],
        intensity: 1.3,
        range: 10.0,
        spot_angle: 30.0,
        shadows: Shadows::Hard,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    };
    let environment = Environment::default();
    let lights = [sun];
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: false,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    let mut renderer = Renderer::with_queue(device, queue);
    renderer.set_options(options);
    let target = renderer.create_target(device, 240, 180);
    renderer.render(device, queue, &target, &frame);
    read_back(device, queue, &target)
}

/// Pixels whose channels differ by more than 3.
fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4)
        .zip(b.chunks(4))
        .filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 3))
        .count()
}

#[test]
fn skinned_frames_match_the_cpu_deformed_mesh_in_every_pass() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let rel = tentacle();
    let pose = bent_pose(&rel);
    // The CPU-deformed copy, drawn as an ordinary mesh.
    let model = models::model(&rel).unwrap();
    let data = crate::render::mesh::MeshData {
        vertices: skinning::deform_pose(&pose).unwrap(),
        indices: model.meshes[0].data.indices.clone(),
        colors: Vec::new(),
    };
    let cpu_key = models::runtime::register("skin-gpu-test-reference", data);
    let key = "skinned://gpu-frame-test/1#0";
    skinning::publish(key, pose);

    let at = Mat4::from_translation(Vec3::new(0.3, 0.0, 0.0));
    let floor = draw(
        PrimitiveMesh::Plane.into(),
        Mat4::from_scale(Vec3::splat(0.6)),
        [0.8, 0.8, 0.8, 1.0],
    );
    let pink = [0.8, 0.25, 0.5, 1.0];
    let gpu_scene = [floor.clone(), draw(MeshRef::Asset(key.into()), at, pink)];
    let cpu_scene = [
        floor.clone(),
        draw(MeshRef::Asset(cpu_key.clone()), at, pink),
    ];
    let bind_scene = [floor, draw(MeshRef::asset(&rel, 0), at, pink)];

    for options in [
        RenderOptions::default(),
        RenderOptions {
            depth_prepass: true,
            ..Default::default()
        },
        RenderOptions {
            gpu_culling: true,
            ..Default::default()
        },
    ] {
        let gpu = render(&device, &queue, options, &gpu_scene);
        let cpu = render(&device, &queue, options, &cpu_scene);
        let bind = render(&device, &queue, options, &bind_scene);
        if let Some(dir) = std::env::var_os("MISTY_SKIN_TEST_DIR") {
            for (name, img) in [("gpu", &gpu), ("cpu", &cpu), ("bind", &bind)] {
                image::RgbaImage::from_raw(240, 180, img.clone())
                    .unwrap()
                    .save(std::path::Path::new(&dir).join(format!("{name}.png")))
                    .unwrap();
            }
        }
        let total = 240 * 180;
        let d = differing(&gpu, &cpu);
        assert!(
            d * 1000 < total,
            "{options:?}: {d} pixels differ from the CPU reference"
        );
        let posed = differing(&gpu, &bind);
        assert!(
            posed * 20 > total,
            "{options:?}: pose barely shows ({posed} pixels)"
        );
    }
    models::runtime::unregister("skin-gpu-test-reference");
}
