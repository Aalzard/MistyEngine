//! SoftShadowsDemo sample scene in `SampleProject`: area-light soft shadows on a white floor.
//! A 2 × 0.6 m rectangle light on the ceiling and a spot light with a 0.35 m source shine
//! on a tall box and a cube, so each shadow is crisp where the object meets the floor and its
//! penumbra widens with the distance from the occluder (the tall box's top edge throws the
//! widest one); the rectangle's shadows are softer across its long side. Contact shadows
//! darken the thin gaps at the base. Best at Quality High / Ultra (Temporal Shadows and
//! Shadow Denoise on). Regenerate with
//! `cargo test -p misty_editor -- --ignored write_soft_shadows_demo`; the other test checks the
//! committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::render_settings::{AmbientSource, SkyboxKind};
use misty_engine::scene::{
    Light, LightType, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene, Shadows,
    TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/SoftShadowsDemo.scene";
const FLOOR: &str = "Assets/Materials/Soft Shadow Floor.mat";
const BOX: &str = "Assets/Materials/Soft Shadow Box.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 2] {
    let m = |color: [f32; 4], smoothness| Material {
        color,
        smoothness,
        ..Default::default()
    };
    [
        (FLOOR, m([0.86, 0.86, 0.85, 1.0], 0.1)),
        (BOX, m([0.8, 0.78, 0.74, 1.0], 0.2)),
    ]
}

fn mesh(s: &mut Scene, name: &str, kind: PrimitiveMesh, position: Vec3, scale: Vec3, mat: &str) {
    let e = s.spawn(name, None);
    s.set_transform(
        e,
        Transform {
            position,
            scale,
            ..Default::default()
        },
    );
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
}

fn light(s: &mut Scene, name: &str, t: Transform, light: Light) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, light);
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.contact_shadows.active = true;
    v.contact_shadows.length = 0.2;
    v.contact_shadows.sample_count = 16;
    v
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("SoftShadowsDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.3, 2.0, -2.8))
                .with_euler(Vec3::new(18.0, -3.0, 0.0)),
        );
    }
    // Only the two area lights: no sun, a faint ambient.
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.despawn(sun);
    }
    let rs = &mut s.render_settings;
    rs.skybox.kind = SkyboxKind::SolidColor;
    rs.skybox.color = [0.03, 0.03, 0.035];
    rs.environment_lighting.source = AmbientSource::Color;
    rs.environment_lighting.sky_color = [0.12, 0.12, 0.13];
    rs.environment_lighting.intensity = 1.0;
    rs.reflection_intensity = 0.2;

    mesh(
        &mut s,
        "Floor",
        PrimitiveMesh::Cube,
        Vec3::new(0.0, -0.05, 3.0),
        Vec3::new(14.0, 0.1, 14.0),
        FLOOR,
    );
    mesh(
        &mut s,
        "Back Wall",
        PrimitiveMesh::Cube,
        Vec3::new(0.0, 2.0, 7.0),
        Vec3::new(14.0, 4.0, 0.1),
        FLOOR,
    );
    mesh(
        &mut s,
        "Tall Box",
        PrimitiveMesh::Cube,
        Vec3::new(-0.6, 1.1, 3.4),
        Vec3::new(0.5, 2.2, 0.5),
        BOX,
    );
    mesh(
        &mut s,
        "Cube",
        PrimitiveMesh::Cube,
        Vec3::new(0.6, 0.35, 2.2),
        Vec3::splat(0.7),
        BOX,
    );

    // A ceiling panel in front of and right of the boxes, wide across x: the shadows stretch
    // back to the wall and soften more along x than along z.
    light(
        &mut s,
        "Ceiling Light",
        Transform::from_position(Vec3::new(0.8, 3.2, 1.6)).with_euler(Vec3::new(90.0, 0.0, 0.0)),
        Light {
            color: [1.0, 0.93, 0.84],
            intensity: 30.0,
            range: 16.0,
            width: 2.0,
            height: 0.6,
            contact_shadows: true,
            ..Light::rectangle()
        },
    );
    // A cool spot light from the front left with a big (0.35 m) source: shadows back and to
    // the right, up the wall.
    light(
        &mut s,
        "Spot Light",
        Transform::from_position(Vec3::new(-2.2, 2.6, -0.5)).with_euler(Vec3::new(23.5, 32.0, 0.0)),
        Light {
            kind: LightType::Spot,
            color: [0.7, 0.82, 1.0],
            intensity: 6.0,
            range: 14.0,
            spot_angle: 70.0,
            shadows: Shadows::Soft,
            shadow_radius: 0.35,
            contact_shadows: true,
            ..Default::default()
        },
    );
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_soft_shadows_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn soft_shadows_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_soft_shadows_demo",
    );
    let (scene, report) = Scene::from_json("SoftShadowsDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // A soft rectangle and a soft spot with its big source after the file round trip.
    let lights = misty_engine::render_scene::lights(&scene);
    assert_eq!(lights.len(), 2);
    assert!(lights.iter().all(|l| l.shadows == Shadows::Soft));
    let rect = lights.iter().find(|l| l.kind == LightType::Rectangle);
    assert_eq!(rect.unwrap().area.size, glam::Vec2::new(2.0, 0.6));
    let spot = lights.iter().find(|l| l.kind == LightType::Spot);
    assert_eq!(spot.unwrap().shadow.size, 0.35);
    assert!(lights.iter().all(|l| l.contact_shadows));
}
