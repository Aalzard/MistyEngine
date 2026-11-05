//! MeadowDemo sample scene in `SampleProject`: a 2 km basin of wild meadow grass (dense
//! tufts of photographed blades, timothy and panicle seed heads, translucent in the low sun;
//! soft lush and dry patches with taller and shorter grass, gust waves rolling across the field
//! from a Wind Zone, grass shadows) ringed by jagged mountains, under Cloudy volumetric
//! clouds with haze and sun shafts. The ground is a PBR set (albedo, normal, height) made
//! from a generated top-down meadow photo under `Assets/Textures/Meadow`.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_meadow_demo`; the other
//! tests check the committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    CloudPreset, Light, PostProcessVolume, Scene, Shadows, Terrain, TerrainCollider, TonemapMode,
    Transform, WindZone,
};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::{TerrainData, TerrainLayer, details, noise};

const DATA: &str = "Assets/Terrains/MeadowDemo.terrain";
const SCENE: &str = "Assets/Scenes/MeadowDemo.scene";
const GROUND: &str = "Assets/Textures/Meadow/MeadowGround.png";
const GROUND_NORMAL: &str = "Assets/Textures/Meadow/MeadowGroundNormal.png";
const GROUND_HEIGHT: &str = "Assets/Textures/Meadow/MeadowGroundHeight.png";
// Written by `sample_terrain2`.
const SCREE: &str = "Assets/Textures/Terrain/Scree.png";
const ROCK: &str = "Assets/Textures/Terrain/Cliff.png";
const ROCK_NORMAL: &str = "Assets/Textures/Terrain/CliffNormal.png";

const SIZE: Vec3 = Vec3::new(2048.0, 420.0, 2048.0);
const CORNER: Vec3 = Vec3::new(-1024.0, 0.0, -1024.0);
/// Where the camera stands (terrain-local x, z), looking along +Z.
const EYE: (f32, f32) = (1024.0, 520.0);

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Normalised height at local (x, z) metres: a gently rolling meadow basin, mountains rising
/// beyond ~650 m from its centre (closer ahead of the camera than behind it).
fn height(x: f32, z: f32) -> f32 {
    let (cx, cz) = (1024.0, 700.0);
    let dx = (x - cx) / 900.0;
    let dz = (z - cz) / 650.0;
    let r = (dx * dx + dz * dz).sqrt();
    let ring = ((r - 0.75) / 0.5).clamp(0.0, 1.0);
    let ring = ring * ring * (3.0 - 2.0 * ring);
    let rolling = noise::fbm(x, z, 180.0, 4, 3) * 0.5 + 0.5;
    let swells = noise::fbm(x, z, 45.0, 2, 8) * 0.5 + 0.5;
    let ridges = noise::ridged(x, z, 420.0, 6, 11);
    let peaks = ridges * ridges * (0.55 + 0.45 * (noise::fbm(x, z, 900.0, 2, 4) * 0.5 + 0.5));
    (0.03 + 0.03 * rolling + 0.004 * swells + ring * (0.06 + 0.8 * peaks)).clamp(0.0, 1.0)
}

fn rules(slope: [f32; 2], priority: i32) -> AutoPaintRules {
    AutoPaintRules {
        enabled: true,
        slope,
        spread: 0.3,
        breakup: 0.55,
        breakup_scale: 30.0,
        priority,
        ..Default::default()
    }
}

fn grass() -> details::DetailSettings {
    details::DetailSettings {
        enabled: true,
        rules: AutoPaintRules {
            enabled: true,
            slope: [0.0, 24.0],
            spread: 0.25,
            breakup: 0.25,
            breakup_scale: 18.0,
            ..Default::default()
        },
        on_layer: Some(0),
        density: 450.0,
        draw_distance: 160.0,
        height: [0.12, 0.6],
        width: 0.02,
        healthy_color: [0.32, 0.52, 0.14],
        dry_color: [0.56, 0.58, 0.28],
        wind_strength: 0.3,
        wind_speed: 1.0,
        seed_heads: 0.1,
        seed_color: [0.8, 0.72, 0.5],
        clumping: 0.6,
        bend: 0.4,
        translucency: 0.6,
        tip_dryness: 0.2,
        lod_distance: 15.0,
        // Soft patches ~16 m across: lush dark-green hollows with taller, fuller tufts and
        // paler, drier, shorter stretches; every tuft a little warmer or cooler.
        patch_scale: 16.0,
        lush_color: [0.17, 0.34, 0.07],
        lush_amount: 0.6,
        dry_amount: 0.5,
        color_variation: 0.6,
        height_variation: 0.65,
        density_variation: 0.25,
        ground_tint: 0.92,
        cast_shadows: true,
        // Gust waves rolling from the left across the view (the Wind Zone turns them).
        wind_direction: 80.0,
        gust_strength: 0.8,
        gust_scale: 22.0,
        turbulence: 0.45,
    }
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(513, SIZE, 512);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, height(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = vec![
        TerrainLayer {
            name: "Meadow Ground".into(),
            diffuse: Some(GROUND.into()),
            normal_map: Some(GROUND_NORMAL.into()),
            height_map: Some(GROUND_HEIGHT.into()),
            normal_scale: 1.0,
            tile_size: [3.5, 3.5],
            smoothness: 0.05,
            rules: rules([0.0, 22.0], 0),
            ..Default::default()
        },
        TerrainLayer {
            name: "Scree".into(),
            diffuse: Some(SCREE.into()),
            tile_size: [8.0, 8.0],
            rules: rules([20.0, 34.0], 1),
            ..Default::default()
        },
        TerrainLayer {
            name: "Cliff".into(),
            diffuse: Some(ROCK.into()),
            normal_map: Some(ROCK_NORMAL.into()),
            tile_size: [40.0, 40.0],
            rules: rules([34.0, 90.0], 2),
            ..Default::default()
        },
    ];
    d.rule_seed = 7;
    d.details = grass();
    auto_paint::generate(&mut d);
    details::generate(&mut d);
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
    v.bloom.intensity = 0.35;
    v.bloom.threshold = 1.1;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.35;
    v.color_adjustments.contrast = -4.0;
    v.color_adjustments.saturation = 4.0;
    v.ambient_occlusion.active = true;
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Cloudy.apply(c);
    c.coverage = 0.5;
    c.density = 0.7;
    c.wind_direction = 100.0;
    c.wind_speed = 12.0;
    c.silver_lining = 0.9;
    c.shadow_opacity = 0.6;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 3000.0;
    f.attenuation_distance = 6000.0;
    f.base_height = 0.0;
    f.maximum_height = 150.0;
    f.anisotropy = 0.5;
    f.volumetric_distance = 250.0;
    v.sun_shafts.active = true;
    v.sun_shafts.intensity = 0.5;
    v
}

fn demo_scene(d: &TerrainData) -> Scene {
    let mut s = Scene::default_3d("MeadowDemo");
    let eye = (CORNER.x + EYE.0, CORNER.z + EYE.1);
    if let Some(cam) = s.find_by_name("Main Camera") {
        let y = ground(d, eye.0, eye.1) + 1.6;
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(eye.0, y, eye.1))
                .with_euler(Vec3::new(-4.0, 0.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.far = 5000.0;
        c.near = 0.05;
        c.fov = 62.0;
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Low afternoon sun ahead of the camera, to the right: the grass is backlit.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 200.0, 0.0))
                .with_euler(Vec3::new(26.0, 205.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.86, 0.66];
        l.intensity = 2.4;
        l.shadows = Shadows::Soft;
    }
    let rs = &mut s.render_settings;
    rs.skybox.sun_disk = SunDisk::HighQuality;
    rs.skybox.sun_size = 0.04;
    rs.skybox.atmosphere_thickness = 1.0;

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
    // Wind across the field from the left, slightly away from the camera.
    let wind = s.spawn("Wind Zone", None);
    s.set_transform(
        wind,
        Transform::from_position(Vec3::new(CORNER.x + EYE.0, 10.0, CORNER.z + EYE.1))
            .with_euler(Vec3::new(0.0, 75.0, 0.0)),
    );
    s.add(wind, WindZone::default());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_meadow_demo() {
    let root = project();
    let d = terrain_data();
    d.save(&root.join(DATA)).unwrap();
    demo_scene(&d).save(&root.join(SCENE)).unwrap();
}

#[test]
fn meadow_demo_matches_its_generator() {
    let root = project();
    for rel in [
        GROUND,
        GROUND_NORMAL,
        GROUND_HEIGHT,
        SCREE,
        ROCK,
        ROCK_NORMAL,
    ] {
        assert!(root.join(rel).exists(), "{rel}");
    }
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.details, generated.details);
    assert_eq!(on_disk.size, generated.size);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(scene.to_json(), demo_scene(&generated).to_json());
    // Dense grass all around the camera.
    let lo = glam::Vec2::new(EYE.0 - 8.0, EYE.1);
    let blades = details::scatter(&generated, lo, lo + 16.0);
    assert!(blades.len() > 40_000, "{}", blades.len());
    assert!(blades.iter().any(|b| b.seed_head()), "seed heads");
}

/// Playing works (no scripts; the terrain collider builds).
#[test]
fn meadow_demo_plays() {
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
