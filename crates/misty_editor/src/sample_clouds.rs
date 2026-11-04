//! CloudsDemo sample scene in `SampleProject`: a 4 km valley of rolling meadows under
//! mountains, a low afternoon sun (24°) ahead of the camera (silver linings on the clouds in front
//! of it, long cloud shadows drifting over the slopes), Cloudy volumetric clouds blowing
//! across the view, light haze and sun shafts. The terrain reuses TerrainDemo2's generated
//! textures. Regenerate with `cargo test -p misty_editor -- --ignored write_clouds_demo`; the
//! other tests check the committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    CloudPreset, Light, PostProcessVolume, Scene, Shadows, Terrain, TerrainCollider, TonemapMode,
    Transform,
};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::{TerrainData, TerrainLayer, noise};

const DATA: &str = "Assets/Terrains/CloudsDemo.terrain";
const SCENE: &str = "Assets/Scenes/CloudsDemo.scene";
// Written by `sample_terrain2`.
const GRASS: &str = "Assets/Textures/Terrain/Meadow.png";
const SCREE: &str = "Assets/Textures/Terrain/Scree.png";
const ROCK: &str = "Assets/Textures/Terrain/Cliff.png";
const SNOW: &str = "Assets/Textures/Terrain/Snow.png";

const SIZE: Vec3 = Vec3::new(4000.0, 520.0, 4000.0);
const CORNER: Vec3 = Vec3::new(-2000.0, 0.0, -2000.0);

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Normalised height at local (x, z) metres: a meadow valley running along +Z (the view)
/// between ridges, with mountains in the distance.
fn height(x: f32, z: f32) -> f32 {
    let across = ((x - 2000.0) / 900.0).abs();
    let valley = (across * across).min(1.0);
    let far = ((z - 1800.0) / 1600.0).clamp(0.0, 1.0);
    let rolling = noise::fbm(x, z, 260.0, 4, 3) * 0.5 + 0.5;
    let ridges = noise::ridged(x, z, 700.0, 5, 11);
    let mountains = (valley * 0.7 + far * 0.6).min(1.0);
    (0.02 + 0.05 * rolling + mountains * (0.1 + 0.75 * ridges * ridges)).clamp(0.0, 1.0)
}

fn layer(name: &str, diffuse: &str, tile: f32, rules: AutoPaintRules) -> TerrainLayer {
    TerrainLayer {
        name: name.into(),
        diffuse: Some(diffuse.into()),
        tile_size: [tile, tile],
        rules,
        ..Default::default()
    }
}

fn rules(slope: [f32; 2], priority: i32) -> AutoPaintRules {
    AutoPaintRules {
        enabled: true,
        slope,
        spread: 0.25,
        breakup: 0.6,
        breakup_scale: 40.0,
        priority,
        ..Default::default()
    }
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(257, SIZE, 256);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, height(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = vec![
        layer("Meadow", GRASS, 9.0, rules([0.0, 20.0], 0)),
        layer("Scree", SCREE, 7.0, rules([18.0, 32.0], 1)),
        layer("Cliff", ROCK, 30.0, rules([32.0, 90.0], 2)),
        layer(
            "Snow",
            SNOW,
            14.0,
            AutoPaintRules {
                use_height: true,
                height: [330.0, 520.0],
                ..rules([0.0, 45.0], 3)
            },
        ),
    ];
    d.rule_seed = 5;
    auto_paint::generate(&mut d);
    d
}

fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    CORNER.y + d.sample_height(x - CORNER.x, z - CORNER.z)
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.5;
    v.bloom.threshold = 1.0;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.4;
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Cloudy.apply(c);
    c.coverage = 0.45;
    c.density = 0.75;
    c.wind_direction = 100.0;
    c.wind_speed = 18.0;
    c.silver_lining = 0.8;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 3000.0;
    f.attenuation_distance = 9000.0;
    f.base_height = 0.0;
    f.maximum_height = 300.0;
    f.anisotropy = 0.35;
    f.volumetric_distance = 200.0;
    v.sun_shafts.active = true;
    v.sun_shafts.intensity = 0.4;
    v
}

fn demo_scene(d: &TerrainData) -> Scene {
    let mut s = Scene::default_3d("CloudsDemo");
    let eye = (-60.0, -1500.0);
    if let Some(cam) = s.find_by_name("Main Camera") {
        let y = ground(d, eye.0, eye.1) + 6.0;
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(eye.0, y, eye.1))
                .with_euler(Vec3::new(-9.0, 12.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.far = 6000.0;
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Low afternoon sun ahead of the camera, a little to the right.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 200.0, 0.0))
                .with_euler(Vec3::new(24.0, 212.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.9, 0.76];
        l.intensity = 2.2;
        l.shadows = Shadows::Soft;
    }
    let rs = &mut s.render_settings;
    rs.skybox.sun_disk = SunDisk::HighQuality;
    rs.skybox.sun_size = 0.04;
    rs.skybox.atmosphere_thickness = 1.1;

    let terrain = s.spawn("Terrain", None);
    s.set_transform(terrain, Transform::from_position(CORNER));
    s.add(
        terrain,
        Terrain {
            terrain_data: DATA.into(),
            ..Default::default()
        },
    );
    s.add(terrain, TerrainCollider::default());
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_clouds_demo() {
    let root = project();
    let d = terrain_data();
    d.save(&root.join(DATA)).unwrap();
    demo_scene(&d).save(&root.join(SCENE)).unwrap();
}

#[test]
fn clouds_demo_matches_its_generator() {
    let root = project();
    for rel in [GRASS, SCREE, ROCK, SNOW] {
        assert!(
            root.join(rel).exists(),
            "{rel} (written by sample_terrain2)"
        );
    }
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.size, generated.size);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(scene.to_json(), demo_scene(&generated).to_json());
    let post = misty_engine::render_scene::post_settings(&scene);
    assert!(post.clouds.is_on() && post.clouds.shadows);
    // The sun is low and ahead of the camera: silver linings in view.
    let sun = misty_engine::render_scene::lights(&scene)[0].direction;
    assert!(-sun.y < 0.45 && sun.z < 0.0, "{sun}");
}

/// Playing works (no scripts; the terrain collider builds).
#[test]
fn clouds_demo_plays() {
    let root = project();
    let data = TerrainData::load(&root.join(DATA)).unwrap();
    let scene = demo_scene(&data);
    let terrain = scene.find_by_name("Terrain").unwrap();
    scene
        .world
        .get::<&mut Terrain>(terrain)
        .unwrap()
        .terrain_data = root.join(DATA).to_string_lossy().into_owned();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
}
