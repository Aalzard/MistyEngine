//! VolumetricFogDemo sample scene in `SampleProject`: a forest of trunks behind a ruined wall
//! with window slits, a low sun shining through them into volumetric fog (god rays), a street
//! lamp spot light with a visible cone, a point-light lantern, a Fog Volume filling a valley,
//! and a camera slowly orbiting the forest (`CameraOrbit.lua`). Regenerate with
//! `cargo test -p misty_editor -- --ignored write_volumetric_fog_demo`; the other tests check
//! the committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    FogQuality, FogVolume, Light, LightType, MeshFilter, MeshRenderer, PostProcessVolume,
    PrimitiveMesh, Scene, ScriptRef, ScriptValue, Scripts, Shadows, TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/VolumetricFogDemo.scene";
const ORBIT: &str = "Assets/Scripts/CameraOrbit.lua";
const BARK: &str = "Assets/Materials/Bark.mat";
const FLOOR: &str = "Assets/Materials/Forest Floor.mat";
const RUIN: &str = "Assets/Materials/Ruin Stone.mat";
const IRON: &str = "Assets/Materials/Lamp Iron.mat";
const GLOW: &str = "Assets/Materials/Lantern Glow.mat";

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
        (BARK, m([0.33, 0.25, 0.19, 1.0], 0.0, 0.15)),
        (FLOOR, m([0.24, 0.28, 0.17, 1.0], 0.0, 0.1)),
        (RUIN, m([0.56, 0.53, 0.48, 1.0], 0.0, 0.2)),
        (IRON, m([0.14, 0.14, 0.15, 1.0], 0.7, 0.5)),
        (
            GLOW,
            Material {
                emission_enabled: true,
                emission: [1.0, 0.62, 0.28],
                emission_intensity: 3.0,
                ..m([1.0, 0.8, 0.5, 1.0], 0.0, 0.6)
            },
        ),
    ]
}

/// Deterministic 0..1 value per integer pair (trunk sizes and offsets).
fn hash(x: i32, z: i32) -> f32 {
    let n = (x.wrapping_mul(73_856_093) ^ z.wrapping_mul(19_349_663)) as u32;
    (n.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32
}

fn mesh(
    s: &mut Scene,
    name: &str,
    parent: Option<misty_engine::hecs::Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    material: &str,
) -> misty_engine::hecs::Entity {
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

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

fn orbit() -> Scripts {
    let fields = [
        ("center", ScriptValue::Vector3([0.0, 1.5, 8.0])),
        ("radius", ScriptValue::Number(24.0)),
        ("height", ScriptValue::Number(2.5)),
        ("speed", ScriptValue::Number(0.05)),
    ];
    Scripts {
        list: vec![ScriptRef {
            path: ORBIT.into(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            ..Default::default()
        }],
    }
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.8;
    v.bloom.threshold = 0.9;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.6;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 250.0;
    f.attenuation_distance = 38.0;
    f.base_height = 0.0;
    f.maximum_height = 18.0;
    f.albedo = [1.0, 0.97, 0.92];
    f.anisotropy = 0.6;
    f.ambient_dimmer = 0.5;
    f.volumetric_distance = 72.0;
    f.quality = FogQuality::High;
    f.noise_intensity = 0.35;
    f.noise_scale = 7.0;
    f.noise_scroll = [0.5, 0.05, 0.25];
    v
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("VolumetricFogDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 4.0, -16.0))
                .with_euler(Vec3::new(-2.0, 0.0, 0.0)),
        );
        s.add(cam, orbit());
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Low warm sun behind the wall, facing the camera.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 6.0, 30.0))
                .with_euler(Vec3::new(11.0, 188.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.8, 0.58];
        l.intensity = 2.6;
        l.shadows = Shadows::Soft;
    }
    let rs = &mut s.render_settings;
    rs.skybox.sun_disk = SunDisk::HighQuality;
    rs.skybox.sun_size = 0.05;
    rs.skybox.atmosphere_thickness = 1.4;
    rs.skybox.exposure = 0.9;
    rs.environment_lighting.intensity = 0.8;

    mesh(
        &mut s,
        "Ground",
        None,
        PrimitiveMesh::Plane,
        scaled(Vec3::new(0.0, 0.0, 8.0), Vec3::new(9.0, 1.0, 9.0)),
        FLOOR,
    );

    // The ruined wall: tall blocks with narrow slits, a lintel across the top.
    let wall = s.spawn("Ruined Wall", None);
    let mut x = -19.0;
    let mut i = 0;
    while x < 19.0 {
        let width = 1.6 + hash(i, 7) * 1.6;
        let height = 8.0 + hash(i, 3) * 3.0;
        let block = scaled(
            Vec3::new(x + width * 0.5, height * 0.5, 28.0),
            Vec3::new(width, height, 1.2),
        );
        mesh(
            &mut s,
            &format!("Wall Block {}", i + 1),
            Some(wall),
            PrimitiveMesh::Cube,
            block,
            RUIN,
        );
        x += width + 0.45 + hash(i, 11) * 0.5;
        i += 1;
    }
    mesh(
        &mut s,
        "Lintel",
        Some(wall),
        PrimitiveMesh::Cube,
        scaled(Vec3::new(0.0, 11.5, 28.0), Vec3::new(40.0, 1.0, 1.6)),
        RUIN,
    );

    // The forest: trunks on a jittered grid.
    let forest = s.spawn("Forest", None);
    for row in 0..4 {
        for col in 0..7 {
            let (h0, h1) = (hash(col, row), hash(row + 17, col + 5));
            let pos = Vec3::new(
                -13.5 + col as f32 * 4.5 + (h0 - 0.5) * 2.5,
                0.0,
                4.0 + row as f32 * 5.5 + (h1 - 0.5) * 2.5,
            );
            let (height, radius) = (7.0 + h1 * 5.0, 0.55 + h0 * 0.5);
            let t = scaled(
                pos + Vec3::Y * height * 0.5,
                Vec3::new(radius, height * 0.5, radius),
            );
            let name = format!("Trunk {}-{}", row + 1, col + 1);
            mesh(
                &mut s,
                &name,
                Some(forest),
                PrimitiveMesh::Cylinder,
                t,
                BARK,
            );
        }
    }

    // Street lamp: iron post with a spot light shining down.
    let lamp = s.spawn("Street Lamp", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(7.0, 0.0, -5.0)));
    mesh(
        &mut s,
        "Post",
        Some(lamp),
        PrimitiveMesh::Cylinder,
        scaled(Vec3::new(0.0, 2.6, 0.0), Vec3::new(0.18, 2.6, 0.18)),
        IRON,
    );
    mesh(
        &mut s,
        "Head",
        Some(lamp),
        PrimitiveMesh::Cube,
        scaled(Vec3::new(0.0, 5.35, 0.0), Vec3::new(0.9, 0.25, 0.9)),
        IRON,
    );
    let spot = s.spawn("Lamp Light", Some(lamp));
    s.set_transform(
        spot,
        Transform::from_position(Vec3::new(0.0, 5.1, 0.0)).with_euler(Vec3::new(90.0, 0.0, 0.0)),
    );
    s.add(
        spot,
        Light {
            kind: LightType::Spot,
            color: [1.0, 0.86, 0.62],
            intensity: 9.0,
            range: 13.0,
            spot_angle: 55.0,
            shadows: Shadows::Hard,
            volumetric_multiplier: 1.5,
            ..Default::default()
        },
    );

    // Lantern on a short post: a glowing globe with a point light inside.
    let lantern = s.spawn("Lantern", None);
    s.set_transform(
        lantern,
        Transform::from_position(Vec3::new(-6.0, 0.0, -3.0)),
    );
    mesh(
        &mut s,
        "Stand",
        Some(lantern),
        PrimitiveMesh::Cylinder,
        scaled(Vec3::new(0.0, 0.6, 0.0), Vec3::new(0.12, 0.6, 0.12)),
        IRON,
    );
    let globe = mesh(
        &mut s,
        "Globe",
        Some(lantern),
        PrimitiveMesh::Sphere,
        scaled(Vec3::new(0.0, 1.45, 0.0), Vec3::splat(0.35)),
        GLOW,
    );
    // The light sits inside the globe: it must not shadow it.
    s.world
        .get::<&mut MeshRenderer>(globe)
        .unwrap()
        .cast_shadows = false;
    let point = s.spawn("Lantern Light", Some(lantern));
    s.set_transform(point, Transform::from_position(Vec3::new(0.0, 1.45, 0.0)));
    s.add(
        point,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.6, 0.28],
            intensity: 4.0,
            range: 9.0,
            shadows: Shadows::Soft,
            volumetric_multiplier: 2.0,
            ..Default::default()
        },
    );

    // A valley to the left with a pool of dense, cool fog.
    let valley = s.spawn("Valley", None);
    for (name, x, angle) in [("Ridge West", -30.0, -28.0f32), ("Ridge East", -18.0, 28.0)] {
        let t = Transform {
            position: Vec3::new(x, 0.0, 8.0),
            scale: Vec3::new(6.0, 4.0, 34.0),
            ..Default::default()
        }
        .with_euler(Vec3::new(0.0, 0.0, angle));
        mesh(&mut s, name, Some(valley), PrimitiveMesh::Cube, t, FLOOR);
    }
    let pool = s.spawn("Valley Fog", Some(valley));
    s.set_transform(pool, Transform::from_position(Vec3::new(-24.0, 1.2, 8.0)));
    s.add(
        pool,
        FogVolume {
            size: [9.0, 3.0, 30.0],
            fog_distance: 2.5,
            albedo: [0.78, 0.86, 1.0],
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
fn write_volumetric_fog_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn volumetric_fog_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), m, "{path}");
    }
    // CameraOrbit.lua is shared with StressDemo (written by its generator).
    assert!(root.join(ORBIT).exists());
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == demo_scene().to_json(),
        "regenerate with write_volumetric_fog_demo"
    );
    let (scene, report) = Scene::from_json("VolumetricFogDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let post = misty_engine::render_scene::post_settings(&scene);
    assert!(post.fog.is_on() && post.fog.volume.volumetric);
    let lights = misty_engine::render_scene::lights(&scene);
    let kinds: Vec<_> = lights.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        [LightType::Directional, LightType::Spot, LightType::Point]
    );
    assert_eq!(misty_engine::render_scene::fog_volumes(&scene).len(), 1);
}

/// Playing: the camera orbits the forest, without script errors.
#[test]
fn volumetric_fog_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::ZERO);
    let cam = scene.find_by_name("Main Camera").unwrap();
    let start = scene.world_position(cam);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..120 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
    }
    let pos = scene.world_position(cam);
    assert!(pos.distance(start) > 0.5, "camera orbits");
    assert!((pos.y - 4.0).abs() < 1e-3, "{pos}");
    assert!((Vec3::new(pos.x, 0.0, pos.z - 8.0).length() - 24.0).abs() < 1e-3);
}
