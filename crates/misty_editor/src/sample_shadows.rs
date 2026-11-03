//! ShadowsDemo sample scene in `SampleProject`: soft shadows that harden at contact. A low sun
//! with a 1° disc stretches long shadows from tall thin poles and a figure built from
//! primitives, a spot light with a 0.5 m source softens a table's shadow, and pillars out to
//! 180 m show the four sun cascades (Scene view ▸ Shadow Cascades). Regenerate with
//! `cargo test -p misty_editor -- --ignored write_shadows_demo`; the other test checks the
//! committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::{
    Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/ShadowsDemo.scene";
const GROUND: &str = "Assets/Materials/Shadow Ground.mat";
const POLE: &str = "Assets/Materials/Shadow Pole.mat";
const FIGURE: &str = "Assets/Materials/Shadow Figure.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn material(color: [f32; 4], smoothness: f32) -> Material {
    Material {
        color,
        smoothness,
        ..Default::default()
    }
}

fn ground() -> Material {
    material([0.72, 0.7, 0.66, 1.0], 0.15)
}

fn pole() -> Material {
    material([0.35, 0.33, 0.32, 1.0], 0.35)
}

fn figure() -> Material {
    material([0.85, 0.42, 0.22, 1.0], 0.4)
}

fn add_mesh(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, material: &str) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    let mut t = Transform::from_position(position);
    t.scale = scale;
    t
}

/// A standing figure of primitives: legs, body, arms and head (1.8 m tall).
fn figure_at(s: &mut Scene, base: Vec3) {
    let parts = [
        (
            "Leg L",
            PrimitiveMesh::Capsule,
            Vec3::new(-0.13, 0.45, 0.0),
            Vec3::new(0.18, 0.45, 0.18),
        ),
        (
            "Leg R",
            PrimitiveMesh::Capsule,
            Vec3::new(0.13, 0.45, 0.0),
            Vec3::new(0.18, 0.45, 0.18),
        ),
        (
            "Body",
            PrimitiveMesh::Cube,
            Vec3::new(0.0, 1.18, 0.0),
            Vec3::new(0.5, 0.6, 0.26),
        ),
        (
            "Arm L",
            PrimitiveMesh::Capsule,
            Vec3::new(-0.36, 1.12, 0.0),
            Vec3::new(0.13, 0.36, 0.13),
        ),
        (
            "Arm R",
            PrimitiveMesh::Capsule,
            Vec3::new(0.36, 1.12, 0.0),
            Vec3::new(0.13, 0.36, 0.13),
        ),
        (
            "Head",
            PrimitiveMesh::Sphere,
            Vec3::new(0.0, 1.64, 0.0),
            Vec3::splat(0.3),
        ),
    ];
    for (name, mesh, offset, scale) in parts {
        let name = format!("Figure {name}");
        add_mesh(s, &name, mesh, scaled(base + offset, scale), FIGURE);
    }
}

/// A table under the spot light: its legs' shadows show the source radius.
fn table_at(s: &mut Scene, at: Vec3) {
    let top = scaled(at + Vec3::new(0.0, 0.8, 0.0), Vec3::new(1.6, 0.06, 1.0));
    add_mesh(s, "Table Top", PrimitiveMesh::Cube, top, POLE);
    for (i, (x, z)) in [(-0.7, -0.4), (0.7, -0.4), (-0.7, 0.4), (0.7, 0.4)]
        .into_iter()
        .enumerate()
    {
        let leg = scaled(at + Vec3::new(x, 0.4, z), Vec3::new(0.06, 0.8, 0.06));
        add_mesh(
            s,
            &format!("Table Leg {}", i + 1),
            PrimitiveMesh::Cube,
            leg,
            POLE,
        );
    }
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("ShadowsDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(1.5, 4.5, -9.0))
                .with_euler(Vec3::new(17.0, -8.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Low sun from the camera's right: long shadows stretch to the left.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 3.0, 0.0))
                .with_euler(Vec3::new(20.0, -62.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.86, 0.7];
        l.intensity = 1.3;
        l.shadows = Shadows::Soft;
        l.shadow_angle = 1.0;
    }

    let ground = scaled(Vec3::new(0.0, 0.0, 80.0), Vec3::new(40.0, 1.0, 40.0));
    add_mesh(&mut s, "Ground", PrimitiveMesh::Plane, ground, GROUND);
    // Tall thin poles (6 m × 0.3 m): the shadow is sharp at the foot and blurs towards the tip.
    for i in 0..6 {
        let at = Vec3::new(2.0 + i as f32 * 1.6, 3.0, 1.0 + i as f32 * 2.2);
        let pole = scaled(at, Vec3::new(0.3, 3.0, 0.3));
        add_mesh(
            &mut s,
            &format!("Pole {}", i + 1),
            PrimitiveMesh::Cylinder,
            pole,
            POLE,
        );
    }
    figure_at(&mut s, Vec3::new(-0.6, 0.0, 0.5));

    table_at(&mut s, Vec3::new(-4.5, 0.0, 3.0));
    let spot = s.spawn("Spot Light", None);
    s.set_transform(
        spot,
        Transform::from_position(Vec3::new(-4.2, 4.5, 2.6)).with_euler(Vec3::new(88.0, 0.0, 0.0)),
    );
    s.add(
        spot,
        Light {
            kind: LightType::Spot,
            color: [0.7, 0.85, 1.0],
            intensity: 5.0,
            range: 12.0,
            spot_angle: 70.0,
            shadows: Shadows::Soft,
            shadow_radius: 0.5,
            ..Default::default()
        },
    );

    // Pillars into the distance: each sun cascade covers a longer stretch.
    for i in 0..12 {
        for side in [-1.0f32, 1.0] {
            let z = 15.0 + i as f32 * 15.0;
            let pillar = scaled(Vec3::new(side * 7.0, 2.5, z), Vec3::new(1.0, 5.0, 1.0));
            let name = format!("Pillar {}{}", i + 1, if side < 0.0 { "L" } else { "R" });
            add_mesh(&mut s, &name, PrimitiveMesh::Cube, pillar, POLE);
        }
    }
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_shadows_demo() {
    let root = project();
    ground().save(&root.join(GROUND)).unwrap();
    pole().save(&root.join(POLE)).unwrap();
    figure().save(&root.join(FIGURE)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn shadows_demo_matches_its_generator() {
    let root = project();
    assert_eq!(Material::load(&root.join(GROUND)).unwrap(), ground());
    assert_eq!(Material::load(&root.join(POLE)).unwrap(), pole());
    assert_eq!(Material::load(&root.join(FIGURE)).unwrap(), figure());
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text.replace("\r\n", "\n"),
        demo_scene().to_json(),
        "regenerate with write_shadows_demo"
    );
    let (scene, report) = Scene::from_json("ShadowsDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // The soft-shadow sizes survive the file round trip.
    let lights = misty_engine::render_scene::lights(&scene);
    let sun = lights.iter().find(|l| l.kind == LightType::Directional);
    assert_eq!(sun.unwrap().shadow.size, 1.0);
    let spot = lights.iter().find(|l| l.kind == LightType::Spot);
    assert_eq!(spot.unwrap().shadow.size, 0.5);
    assert!(lights.iter().all(|l| l.shadows == Shadows::Soft));
}
