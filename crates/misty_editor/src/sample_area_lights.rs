//! AreaLightDemo sample scene in `SampleProject`: a dim room lit by three rectangle lights of
//! different shapes: a long warm strip on the ceiling, a tall cool panel on the left wall and
//! a small square two-sided panel hanging at the back. A glossy floor shows their stretched
//! reflections; chrome, glossy and rough objects show how the highlights widen with roughness.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_area_light_demo`; the other
//! test checks the committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::render_settings::{AmbientSource, SkyboxKind};
use misty_engine::scene::{
    Light, LightType, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene, Shadows,
    TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/AreaLightDemo.scene";
const FLOOR: &str = "Assets/Materials/Area Glossy Floor.mat";
const WALL: &str = "Assets/Materials/Area Rough Wall.mat";
const CHROME: &str = "Assets/Materials/Area Chrome.mat";
const GLOSSY: &str = "Assets/Materials/Area Glossy Red.mat";
const ROUGH: &str = "Assets/Materials/Area Rough Clay.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 5] {
    let m = |color: [f32; 4], metallic, smoothness| Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    };
    [
        (FLOOR, m([0.3, 0.3, 0.32, 1.0], 0.0, 0.85)),
        (WALL, m([0.62, 0.6, 0.57, 1.0], 0.0, 0.15)),
        (CHROME, m([0.92, 0.92, 0.94, 1.0], 1.0, 0.94)),
        (GLOSSY, m([0.7, 0.08, 0.06, 1.0], 0.0, 0.75)),
        (ROUGH, m([0.75, 0.55, 0.38, 1.0], 0.0, 0.2)),
    ]
}

fn mesh(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    mat: &str,
) {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

/// Floor, ceiling and three walls of an 8 × 3 × 10 m room open towards the camera.
fn room(s: &mut Scene) {
    let room = s.spawn("Room", None);
    let parts = [
        (
            "Floor",
            FLOOR,
            Vec3::new(0.0, -0.05, 4.0),
            Vec3::new(8.0, 0.1, 10.0),
        ),
        (
            "Ceiling",
            WALL,
            Vec3::new(0.0, 3.05, 4.0),
            Vec3::new(8.0, 0.1, 10.0),
        ),
        (
            "Back Wall",
            WALL,
            Vec3::new(0.0, 1.5, 9.05),
            Vec3::new(8.0, 3.0, 0.1),
        ),
        (
            "Left Wall",
            WALL,
            Vec3::new(-4.05, 1.5, 4.0),
            Vec3::new(0.1, 3.0, 10.0),
        ),
        (
            "Right Wall",
            WALL,
            Vec3::new(4.05, 1.5, 4.0),
            Vec3::new(0.1, 3.0, 10.0),
        ),
    ];
    for (name, mat, at, size) in parts {
        mesh(
            s,
            name,
            Some(room),
            PrimitiveMesh::Cube,
            scaled(at, size),
            mat,
        );
    }
}

fn props(s: &mut Scene) {
    let props = s.spawn("Props", None);
    let items = [
        (
            "Chrome Sphere",
            PrimitiveMesh::Sphere,
            CHROME,
            Vec3::new(-1.4, 0.55, 5.2),
            Vec3::splat(1.1),
        ),
        (
            "Glossy Sphere",
            PrimitiveMesh::Sphere,
            GLOSSY,
            Vec3::new(0.4, 0.45, 4.0),
            Vec3::splat(0.9),
        ),
        (
            "Rough Sphere",
            PrimitiveMesh::Sphere,
            ROUGH,
            Vec3::new(2.0, 0.5, 5.6),
            Vec3::splat(1.0),
        ),
        (
            "Pedestal",
            PrimitiveMesh::Cube,
            ROUGH,
            Vec3::new(1.6, 0.4, 7.4),
            Vec3::new(0.8, 0.8, 0.8),
        ),
        (
            "Glossy Box",
            PrimitiveMesh::Cube,
            GLOSSY,
            Vec3::new(1.6, 1.05, 7.4),
            Vec3::splat(0.5),
        ),
        (
            "Column",
            PrimitiveMesh::Cylinder,
            CHROME,
            Vec3::new(-2.6, 1.5, 7.6),
            Vec3::new(0.35, 1.5, 0.35),
        ),
    ];
    for (name, kind, mat, at, size) in items {
        mesh(s, name, Some(props), kind, scaled(at, size), mat);
    }
}

fn rect_light(s: &mut Scene, name: &str, t: Transform, light: Light) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, light);
}

fn lights(s: &mut Scene) {
    // A long, thin warm strip along the ceiling, shining down (12 : 1).
    rect_light(
        s,
        "Ceiling Strip",
        Transform::from_position(Vec3::new(0.0, 2.97, 4.6)).with_euler(Vec3::new(90.0, 0.0, 0.0)),
        Light {
            color: [1.0, 0.82, 0.62],
            intensity: 20.0,
            range: 12.0,
            width: 3.6,
            height: 0.3,
            ..Light::rectangle()
        },
    );
    // A tall cool panel on the left wall, facing into the room (1 : 3).
    rect_light(
        s,
        "Wall Panel",
        Transform::from_position(Vec3::new(-3.97, 1.4, 3.2)).with_euler(Vec3::new(0.0, 90.0, 0.0)),
        Light {
            color: [0.55, 0.75, 1.0],
            intensity: 9.0,
            range: 10.0,
            width: 0.6,
            height: 1.8,
            ..Light::rectangle()
        },
    );
    // A small square two-sided panel hanging in front of the back wall.
    rect_light(
        s,
        "Hanging Panel",
        Transform::from_position(Vec3::new(0.2, 1.9, 8.2)).with_euler(Vec3::new(0.0, 180.0, 0.0)),
        Light {
            color: [1.0, 0.45, 0.75],
            intensity: 10.0,
            range: 8.0,
            width: 0.7,
            height: 0.7,
            two_sided: true,
            shadows: Shadows::None,
            ..Light::rectangle()
        },
    );
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.35;
    v.bloom.threshold = 1.2;
    v
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("AreaLightDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.6, 1.6, -0.6))
                .with_euler(Vec3::new(13.0, -4.0, 0.0)),
        );
    }
    // Only the rectangle lights: no sun, a faint ambient and dim reflections.
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.despawn(sun);
    }
    let rs = &mut s.render_settings;
    rs.skybox.kind = SkyboxKind::SolidColor;
    rs.skybox.color = [0.02, 0.02, 0.025];
    rs.environment_lighting.source = AmbientSource::Color;
    rs.environment_lighting.sky_color = [0.05, 0.05, 0.06];
    rs.environment_lighting.intensity = 1.0;
    rs.reflection_intensity = 0.2;

    room(&mut s);
    props(&mut s);
    lights(&mut s);
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_area_light_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn area_light_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_area_light_demo",
    );
    let (scene, report) = Scene::from_json("AreaLightDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Three rectangles with their sizes and orientation after the file round trip.
    let lights = misty_engine::render_scene::lights(&scene);
    assert_eq!(lights.len(), 3);
    assert!(lights.iter().all(|l| l.kind == LightType::Rectangle));
    let strip = &lights[0];
    assert_eq!(strip.area.size, glam::Vec2::new(3.6, 0.3));
    assert!(
        strip.direction.dot(Vec3::NEG_Y) > 0.99,
        "{:?}",
        strip.direction
    );
    assert!(lights[1].direction.dot(Vec3::X) > 0.99);
    assert!(lights[2].area.two_sided && lights[2].area.show_emitter);
}
