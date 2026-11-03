//! LightingDemo sample scene in `SampleProject`: a sunset procedural sky with a low sun,
//! exponential-squared fog over a row of pillars, and a global Post-process Volume with
//! ACES tonemapping, a little exposure/saturation and a vignette. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_lighting_demo`; the other test checks the
//! committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::render_settings::{FogMode, SunDisk};
use misty_engine::scene::{
    Light, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene, Shadows, TonemapMode,
    Transform,
};

const SCENE: &str = "Assets/Scenes/LightingDemo.scene";
const SAND: &str = "Assets/Materials/Sand.mat";
const STONE: &str = "Assets/Materials/Sandstone.mat";
// Its own material: the shared "Blue Metal.mat" is the user's to edit.
const METAL: &str = "Assets/Materials/Blue Steel.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn sand() -> Material {
    Material {
        color: [0.62, 0.5, 0.38, 1.0],
        smoothness: 0.1,
        ..Default::default()
    }
}

fn sandstone() -> Material {
    Material {
        color: [0.78, 0.62, 0.48, 1.0],
        smoothness: 0.25,
        ..Default::default()
    }
}

fn blue_steel() -> Material {
    Material {
        color: [0.2, 0.45, 0.9, 1.0],
        metallic: 0.6,
        smoothness: 0.8,
        ..Default::default()
    }
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

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.7;
    v.color_adjustments.saturation = 10.0;
    v.color_adjustments.contrast = 5.0;
    v.vignette.active = true;
    v.vignette.intensity = 0.4;
    v.vignette.smoothness = 0.45;
    v
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("LightingDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 1.7, -9.0))
                .with_euler(Vec3::new(-4.0, 0.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Low evening sun ahead of the camera, slightly to the right.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 3.0, 0.0))
                .with_euler(Vec3::new(6.0, 183.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.66, 0.42];
        l.intensity = 1.4;
        l.shadows = Shadows::Soft;
    }

    let rs = &mut s.render_settings;
    rs.skybox.sun_disk = SunDisk::HighQuality;
    rs.skybox.sun_size = 0.06;
    rs.skybox.atmosphere_thickness = 1.6;
    rs.skybox.sky_tint = [0.55, 0.48, 0.46];
    rs.skybox.ground = [0.3, 0.24, 0.2];
    rs.skybox.exposure = 1.2;
    rs.fog.enabled = true;
    rs.fog.color = [0.86, 0.6, 0.46];
    rs.fog.mode = FogMode::ExponentialSquared;
    rs.fog.density = 0.028;

    let mut ground = Transform::from_position(Vec3::new(0.0, 0.0, 30.0));
    ground.scale = Vec3::new(12.0, 1.0, 12.0);
    add_mesh(&mut s, "Ground", PrimitiveMesh::Plane, ground, SAND);
    for i in 0..8 {
        for side in [-1.0f32, 1.0] {
            let mut t = Transform::from_position(Vec3::new(side * 3.5, 2.0, i as f32 * 8.0));
            t.scale = Vec3::new(0.8, 4.0, 0.8);
            let name = format!("Pillar {}{}", i + 1, if side < 0.0 { "L" } else { "R" });
            add_mesh(&mut s, &name, PrimitiveMesh::Cube, t, STONE);
        }
    }
    let mut ball = Transform::from_position(Vec3::new(-1.0, 0.75, -2.0));
    ball.scale = Vec3::splat(1.5);
    add_mesh(&mut s, "Metal Sphere", PrimitiveMesh::Sphere, ball, METAL);
    let crate_t =
        Transform::from_position(Vec3::new(1.4, 0.5, -1.0)).with_euler(Vec3::new(0.0, 25.0, 0.0));
    add_mesh(&mut s, "Block", PrimitiveMesh::Cube, crate_t, STONE);

    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_lighting_demo() {
    let root = project();
    sand().save(&root.join(SAND)).unwrap();
    sandstone().save(&root.join(STONE)).unwrap();
    blue_steel().save(&root.join(METAL)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn lighting_demo_matches_its_generator() {
    let root = project();
    assert_eq!(Material::load(&root.join(SAND)).unwrap(), sand());
    assert_eq!(Material::load(&root.join(STONE)).unwrap(), sandstone());
    assert_eq!(Material::load(&root.join(METAL)).unwrap(), blue_steel());
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text.replace("\r\n", "\n"),
        demo_scene().to_json(),
        "regenerate with write_lighting_demo"
    );
    let (scene, report) = Scene::from_json("LightingDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(scene.render_settings.fog.enabled);
    let post = misty_engine::render_scene::post_settings(&scene);
    assert_eq!(post.tonemap, TonemapMode::Aces);
    assert!(post.vignette_intensity > 0.0);
}
