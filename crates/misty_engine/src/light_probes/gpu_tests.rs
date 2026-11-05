//! Headless light probe checks (skipped without an adapter): a white room with a red wall on +X
//! and a blue wall on −X is baked; a moving white sphere then picks up red near the red wall
//! and blue near the blue one.

use std::collections::BTreeMap;
use std::f32::consts::PI;

use glam::{Mat4, Quat, Vec3};

use super::bake::{BakeSettings, bake_scene};
use super::{FieldHandle, LightProbeField, LightProbeGroup, scene_positions};
use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Frame, MaterialParams, MeshDraw, PostSettings, Renderer};
use crate::render_scene::{self, MaterialSource};
use crate::scene::{
    GameObject, Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Transform,
};

struct Materials(BTreeMap<&'static str, MaterialParams>);

impl MaterialSource for Materials {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        rel.and_then(|r| self.0.get(r)).cloned().unwrap_or_default()
    }
}

fn materials() -> Materials {
    let m = |color: [f32; 3], emission: f32| MaterialParams {
        color: [color[0], color[1], color[2], 1.0],
        smoothness: 0.1,
        emission: color,
        emission_intensity: emission,
        ..Default::default()
    };
    Materials(BTreeMap::from([
        ("white", m([0.8, 0.8, 0.8], 0.0)),
        ("red", m([1.0, 0.05, 0.05], 1.5)),
        ("blue", m([0.05, 0.1, 1.0], 1.5)),
    ]))
}

fn room() -> Scene {
    let mut s = Scene::new("Room");
    s.render_settings.environment_lighting.intensity = 0.2;
    for (at, size, mat) in [
        (Vec3::new(0.0, -0.1, 0.0), Vec3::new(8.4, 0.2, 6.4), "white"),
        (Vec3::new(0.0, 3.1, 0.0), Vec3::new(8.4, 0.2, 6.4), "white"),
        (Vec3::new(0.0, 1.5, -3.1), Vec3::new(8.0, 3.0, 0.2), "white"),
        (Vec3::new(0.0, 1.5, 3.1), Vec3::new(8.0, 3.0, 0.2), "white"),
        (Vec3::new(4.1, 1.5, 0.0), Vec3::new(0.2, 3.0, 6.0), "red"),
        (Vec3::new(-4.1, 1.5, 0.0), Vec3::new(0.2, 3.0, 6.0), "blue"),
    ] {
        let e = s.spawn("Wall", None);
        let mut t = Transform::from_position(at);
        t.scale = size;
        s.set_transform(e, t);
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(
            e,
            MeshRenderer {
                material: Some(mat.into()),
                ..Default::default()
            },
        );
        s.world.get::<&mut GameObject>(e).unwrap().is_static = true;
    }
    let lamp = s.spawn("Lamp", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(0.0, 2.8, 0.0)));
    s.add(
        lamp,
        Light {
            kind: LightType::Point,
            intensity: 2.0,
            range: 10.0,
            ..Default::default()
        },
    );
    let mut group = LightProbeGroup::default();
    group.fill_box(Vec3::new(0.0, 1.5, 0.0), Vec3::new(7.0, 2.4, 5.0), 1.8);
    let g = s.spawn("Light Probe Group", None);
    s.add(g, group);
    s
}

/// Mean RGB of the centre block of a frame looking at a sphere at `x`.
fn sphere_color(
    renderer: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    field: Option<&FieldHandle>,
    x: f32,
) -> Vec3 {
    let mats = materials();
    let mut meshes = render_scene::mesh_draws(scene, &mats, |_| false);
    let at = Vec3::new(x, 1.2, 0.0);
    meshes.push(MeshDraw {
        mesh: PrimitiveMesh::Sphere.into(),
        model: Mat4::from_scale_rotation_translation(Vec3::splat(1.0), Quat::IDENTITY, at),
        material: MaterialParams {
            color: [0.9, 0.9, 0.9, 1.0],
            smoothness: 0.0,
            light_probes: true,
            ..Default::default()
        },
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
        lod: None,
    });
    let lights = render_scene::lights(scene);
    let mut environment = render_scene::environment(scene, &lights);
    environment.probes.light_probes = field.cloned();
    // Seen from the front, the sphere's left / right halves face the two walls equally.
    let camera = Camera {
        position: at + Vec3::Z * 2.4,
        rotation: Quat::from_rotation_y(PI),
        projection: Projection::Perspective { fov_y: 40.0 },
        near: 0.1,
        far: 50.0,
    };
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: true,
        background: [0.0; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        water: &[],
        shaded: true,
        lights: &lights,
    };
    let target = renderer.create_target(device, 64, 64);
    renderer.render(device, queue, &target, &frame);
    let px = read_back(device, queue, &target);
    let mut sum = Vec3::ZERO;
    let mut n = 0.0;
    for y in 24..40 {
        for x in 24..40 {
            let i = (y * 64 + x) * 4;
            sum += Vec3::new(px[i] as f32, px[i + 1] as f32, px[i + 2] as f32);
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn sphere_near_the_red_wall_is_redder_than_near_the_blue_wall() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let scene = room();
    let mut renderer = Renderer::new(&device);
    let positions = scene_positions(&scene);
    assert!(positions.len() >= 18);
    let data = bake_scene(
        &mut renderer,
        &device,
        &queue,
        &scene,
        &materials(),
        positions,
        BakeSettings {
            bounces: 1,
            resolution: 16,
        },
    )
    .unwrap();
    let field = FieldHandle(std::sync::Arc::new(LightProbeField::from_data(&data)));
    // Probes by the red wall see more red than blue, and the other way round.
    let mut hint = 0;
    let near_red = field.0.sample(Vec3::new(3.0, 1.2, 0.0), &mut hint).unwrap();
    let near_blue = field
        .0
        .sample(Vec3::new(-3.0, 1.2, 0.0), &mut hint)
        .unwrap();
    let up = |sh: &[Vec3; 9]| crate::render::probes::sh::eval(sh, Vec3::Y);
    assert!(up(&near_red).x > up(&near_red).z, "{:?}", up(&near_red));
    assert!(up(&near_blue).z > up(&near_blue).x, "{:?}", up(&near_blue));

    let red_side = sphere_color(&mut renderer, &device, &queue, &scene, Some(&field), 2.6);
    let blue_side = sphere_color(&mut renderer, &device, &queue, &scene, Some(&field), -2.6);
    let tint = |c: Vec3| c.x - c.z;
    assert!(
        tint(red_side) > tint(blue_side) + 15.0,
        "red side {red_side}, blue side {blue_side}"
    );
    // Without probes both spheres get the same (uncoloured) ambient.
    let plain_red = sphere_color(&mut renderer, &device, &queue, &scene, None, 2.6);
    let plain_blue = sphere_color(&mut renderer, &device, &queue, &scene, None, -2.6);
    assert!(
        (tint(plain_red) - tint(plain_blue)).abs() < tint(red_side) - tint(blue_side),
        "{plain_red} {plain_blue}"
    );
}
