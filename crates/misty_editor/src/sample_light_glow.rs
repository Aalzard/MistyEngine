//! LightGlowDemo sample scene in `SampleProject`: a foggy night street lined with buildings,
//! warm sodium and cool LED street lamps with halos, a doorway spot light shining down, and a
//! floodlight at the far end facing the camera with a halo and a Lens Flare. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_light_glow_demo`; the other test checks the
//! committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::render_settings::{AmbientSource, SkyboxKind, SunDisk};
use misty_engine::scene::{
    FogQuality, FogVolume, LensFlare, Light, LightHalo, LightType, MeshFilter, MeshRenderer,
    PostProcessVolume, PrimitiveMesh, Scene, Shadows, TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/LightGlowDemo.scene";
const ASPHALT: &str = "Assets/Materials/Wet Asphalt.mat";
const PAVEMENT: &str = "Assets/Materials/Pavement.mat";
const BRICK: &str = "Assets/Materials/Night Brick.mat";
const PLASTER: &str = "Assets/Materials/Night Plaster.mat";
const IRON: &str = "Assets/Materials/Street Iron.mat";
const SODIUM: &str = "Assets/Materials/Sodium Bulb.mat";
const LED: &str = "Assets/Materials/LED Bulb.mat";
const WINDOW: &str = "Assets/Materials/Window Glow.mat";

/// Warm sodium and cool LED light colours (sRGB).
const WARM: [f32; 3] = [1.0, 0.62, 0.3];
const COOL: [f32; 3] = [0.7, 0.84, 1.0];

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 8] {
    let m = |color: [f32; 4], metallic, smoothness| Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    };
    let glow = |color: [f32; 3], intensity| Material {
        emission_enabled: true,
        emission: color,
        emission_intensity: intensity,
        ..m([color[0], color[1], color[2], 1.0], 0.0, 0.7)
    };
    [
        (ASPHALT, m([0.16, 0.16, 0.17, 1.0], 0.0, 0.7)),
        (PAVEMENT, m([0.4, 0.39, 0.37, 1.0], 0.0, 0.35)),
        (BRICK, m([0.32, 0.17, 0.13, 1.0], 0.0, 0.2)),
        (PLASTER, m([0.42, 0.4, 0.37, 1.0], 0.0, 0.15)),
        (IRON, m([0.08, 0.08, 0.09, 1.0], 0.8, 0.45)),
        (SODIUM, glow(WARM, 4.0)),
        (LED, glow(COOL, 4.0)),
        (WINDOW, glow([1.0, 0.72, 0.42], 0.6)),
    ]
}

/// Deterministic 0..1 value per integer pair (building sizes).
fn hash(x: i32, z: i32) -> f32 {
    let n = (x.wrapping_mul(73_856_093) ^ z.wrapping_mul(19_349_663)) as u32;
    (n.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

fn mesh(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    material: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

/// A glowing part the light inside it must not shadow.
fn bulb(s: &mut Scene, parent: Entity, t: Transform, material: &str) {
    let e = mesh(s, "Bulb", Some(parent), PrimitiveMesh::Sphere, t, material);
    s.world.get::<&mut MeshRenderer>(e).unwrap().cast_shadows = false;
}

fn halo(size: f32, intensity: f32) -> LightHalo {
    LightHalo {
        enabled: true,
        size,
        intensity,
        ..Default::default()
    }
}

/// A street lamp at the kerb: post, arm over the road, head, bulb and a point light with a
/// halo. `side` = -1 (left) or 1 (right).
fn street_lamp(s: &mut Scene, parent: Entity, index: usize, side: f32, z: f32, warm: bool) {
    let lamp = s.spawn(&format!("Street Lamp {}", index + 1), Some(parent));
    s.set_transform(
        lamp,
        Transform::from_position(Vec3::new(side * 5.2, 0.0, z)),
    );
    let post = scaled(Vec3::new(0.0, 2.3, 0.0), Vec3::new(0.14, 2.3, 0.14));
    mesh(s, "Post", Some(lamp), PrimitiveMesh::Cylinder, post, IRON);
    let arm = scaled(
        Vec3::new(-side * 0.6, 4.55, 0.0),
        Vec3::new(1.4, 0.08, 0.08),
    );
    mesh(s, "Arm", Some(lamp), PrimitiveMesh::Cube, arm, IRON);
    let head = scaled(
        Vec3::new(-side * 1.2, 4.5, 0.0),
        Vec3::new(0.55, 0.14, 0.32),
    );
    mesh(s, "Head", Some(lamp), PrimitiveMesh::Cube, head, IRON);
    let at = Vec3::new(-side * 1.2, 4.33, 0.0);
    let (color, material) = if warm { (WARM, SODIUM) } else { (COOL, LED) };
    bulb(s, lamp, scaled(at, Vec3::new(0.3, 0.12, 0.2)), material);
    let light = s.spawn("Lamp Light", Some(lamp));
    s.set_transform(light, Transform::from_position(at - Vec3::Y * 0.05));
    s.add(
        light,
        Light {
            kind: LightType::Point,
            color,
            intensity: 4.0,
            range: 11.0,
            shadows: if index < 2 {
                Shadows::Soft
            } else {
                Shadows::None
            },
            volumetric_multiplier: 1.2,
            halo: halo(1.5, 1.0),
            ..Default::default()
        },
    );
}

/// Buildings along one side of the street, with a few lit windows.
fn buildings(s: &mut Scene, parent: Entity, side: f32) {
    let mut z = -18.0;
    let mut i = 0;
    while z < 46.0 {
        let depth = 6.0 + hash(i, side as i32) * 5.0;
        let height = 7.0 + hash(side as i32 + 3, i) * 9.0;
        let center = Vec3::new(side * 11.5, height * 0.5, z + depth * 0.5);
        let material = if i % 2 == 0 { BRICK } else { PLASTER };
        let name = format!("Building {}{}", if side < 0.0 { "L" } else { "R" }, i + 1);
        let b = mesh(
            s,
            &name,
            Some(parent),
            PrimitiveMesh::Cube,
            scaled(center, Vec3::new(5.0, height, depth - 0.3)),
            material,
        );
        // Lit windows on the street face (local space of the unit cube).
        for w in 0..3 {
            if hash(i * 7 + w, side as i32 * 5) < 0.45 {
                continue;
            }
            let y = (2.5 + w as f32 * 3.0) / height - 0.5;
            if y > 0.45 {
                continue;
            }
            let t = scaled(
                Vec3::new(-side * 0.51, y, 0.0),
                Vec3::new(0.02, 1.2 / height, 0.25),
            );
            mesh(s, "Window", Some(b), PrimitiveMesh::Cube, t, WINDOW);
        }
        z += depth;
        i += 1;
    }
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.7;
    v.bloom.threshold = 0.8;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.8;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 200.0;
    f.attenuation_distance = 22.0;
    f.base_height = 0.0;
    f.maximum_height = 14.0;
    f.albedo = [0.9, 0.92, 1.0];
    f.anisotropy = 0.55;
    f.ambient_dimmer = 0.6;
    f.volumetric_distance = 64.0;
    f.quality = FogQuality::Medium;
    f.noise_intensity = 0.3;
    f.noise_scale = 6.0;
    f.noise_scroll = [0.4, 0.0, 0.2];
    v
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("LightGlowDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(-1.2, 1.7, -14.0))
                .with_euler(Vec3::new(-4.0, 4.0, 0.0)),
        );
    }
    if let Some(moon) = s.find_by_name("Directional Light") {
        // A faint, cool moon high above.
        s.set_transform(
            moon,
            Transform::from_position(Vec3::new(0.0, 20.0, 0.0))
                .with_euler(Vec3::new(55.0, -30.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(moon).unwrap();
        l.color = [0.62, 0.72, 1.0];
        l.intensity = 0.12;
        l.shadows = Shadows::Soft;
        l.volumetric_multiplier = 0.5;
    }
    let rs = &mut s.render_settings;
    rs.skybox.kind = SkyboxKind::SolidColor;
    rs.skybox.color = [0.015, 0.02, 0.04];
    rs.skybox.sun_disk = SunDisk::None;
    rs.environment_lighting.source = AmbientSource::Color;
    rs.environment_lighting.sky_color = [0.07, 0.08, 0.12];
    rs.environment_lighting.intensity = 0.6;

    // The street: road, two pavements, buildings on both sides.
    let street = s.spawn("Street", None);
    let road = scaled(Vec3::new(0.0, 0.0, 14.0), Vec3::new(1.0, 1.0, 7.0));
    mesh(
        &mut s,
        "Road",
        Some(street),
        PrimitiveMesh::Plane,
        road,
        ASPHALT,
    );
    for (name, x) in [("Pavement Left", -7.0), ("Pavement Right", 7.0)] {
        let t = scaled(Vec3::new(x, 0.08, 14.0), Vec3::new(4.0, 0.16, 70.0));
        mesh(&mut s, name, Some(street), PrimitiveMesh::Cube, t, PAVEMENT);
    }
    buildings(&mut s, street, -1.0);
    buildings(&mut s, street, 1.0);

    // Lamps alternate sides and colours down the street.
    let lamps = s.spawn("Street Lamps", None);
    for i in 0..6 {
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let warm = i % 3 != 2;
        street_lamp(&mut s, lamps, i, side, -6.0 + i as f32 * 8.0, warm);
    }

    // A shop door on the right with a spot light shining down onto the pavement.
    let door = s.spawn("Shop Door", None);
    s.set_transform(door, Transform::from_position(Vec3::new(8.95, 0.0, 9.0)));
    let frame = scaled(Vec3::new(0.0, 1.3, 0.0), Vec3::new(0.1, 2.6, 1.6));
    mesh(&mut s, "Door", Some(door), PrimitiveMesh::Cube, frame, IRON);
    let spot = s.spawn("Door Spot", Some(door));
    s.set_transform(
        spot,
        Transform::from_position(Vec3::new(-0.4, 3.2, 0.0)).with_euler(Vec3::new(70.0, -90.0, 0.0)),
    );
    s.add(
        spot,
        Light {
            kind: LightType::Spot,
            color: [1.0, 0.85, 0.65],
            intensity: 6.0,
            range: 8.0,
            spot_angle: 70.0,
            shadows: Shadows::Hard,
            volumetric_multiplier: 1.5,
            halo: halo(0.7, 1.0),
            ..Default::default()
        },
    );

    // The floodlight at the far end faces down the street: halo and lens flare.
    let tower = s.spawn("Floodlight", None);
    s.set_transform(tower, Transform::from_position(Vec3::new(2.5, 0.0, 44.0)));
    let mast = scaled(Vec3::new(0.0, 3.5, 0.0), Vec3::new(0.2, 3.5, 0.2));
    mesh(
        &mut s,
        "Mast",
        Some(tower),
        PrimitiveMesh::Cylinder,
        mast,
        IRON,
    );
    let housing = scaled(Vec3::new(0.0, 7.1, 0.25), Vec3::new(0.9, 0.6, 0.3));
    mesh(
        &mut s,
        "Housing",
        Some(tower),
        PrimitiveMesh::Cube,
        housing,
        IRON,
    );
    bulb(
        &mut s,
        tower,
        scaled(Vec3::new(0.0, 7.1, 0.05), Vec3::new(0.7, 0.45, 0.1)),
        LED,
    );
    let flood = s.spawn("Flood Light", Some(tower));
    s.set_transform(
        flood,
        Transform::from_position(Vec3::new(0.0, 7.1, -0.1)).with_euler(Vec3::new(12.0, 180.0, 0.0)),
    );
    s.add(
        flood,
        Light {
            kind: LightType::Spot,
            color: [0.85, 0.92, 1.0],
            intensity: 12.0,
            range: 40.0,
            spot_angle: 60.0,
            shadows: Shadows::Soft,
            volumetric_multiplier: 1.0,
            halo: halo(1.8, 1.2),
            ..Default::default()
        },
    );
    s.add(flood, LensFlare::default());

    // Ground mist pooling at the far end of the street.
    let mist = s.spawn("Ground Mist", None);
    s.set_transform(mist, Transform::from_position(Vec3::new(0.0, 1.0, 30.0)));
    s.add(
        mist,
        FogVolume {
            size: [18.0, 2.0, 24.0],
            fog_distance: 9.0,
            albedo: [0.85, 0.9, 1.0],
            blend_distance: 1.5,
            ..Default::default()
        },
    );

    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_light_glow_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn light_glow_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), m, "{path}");
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == demo_scene().to_json(),
        "regenerate with write_light_glow_demo"
    );
    let (scene, report) = Scene::from_json("LightGlowDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let glows = misty_engine::render_scene::glow_sources(&scene);
    let halos = glows.iter().filter(|g| g.halo.is_some()).count();
    let flares = glows.iter().filter(|g| g.flare.is_some()).count();
    assert_eq!((halos, flares), (8, 1));
    assert!(glows.iter().any(|g| g.kind == LightType::Spot));
    let warm = glows.iter().filter(|g| g.color == WARM).count();
    let cool = glows.iter().filter(|g| g.color == COOL).count();
    assert!(warm >= 2 && cool >= 2, "warm {warm}, cool {cool}");
    let post = misty_engine::render_scene::post_settings(&scene);
    assert!(post.fog.is_on() && post.fog.volume.volumetric);
}
