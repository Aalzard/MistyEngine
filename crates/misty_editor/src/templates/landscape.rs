//! The Landscape template: World Builder terrain (grassy mountains, a forest, snowy peaks at
//! the back) under volumetric clouds, with volumetric fog and sun shafts, ambient occlusion and
//! bloom. Textures are generated small (128 px) so the project is made in a few seconds.

use std::path::Path;

use glam::{Mat4, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    CloudPreset, Light, PostProcessVolume, Scene, Shadows, Terrain, TerrainCollider, TonemapMode,
    Transform,
};
use misty_engine::world_gen::stamps::BiomeZone;
use misty_engine::world_gen::{
    self, BaseSettings, Biome, ErosionSettings, Generator, Preset, ScatterStorage, Stages,
    StampKind, WorldBuilder, WorldStamp, biome, textures,
};

use super::scenes::{VOLUME, place};

pub const DATA: &str = "Assets/Terrains/Landscape.terrain";
pub const SIZE: [f32; 3] = [800.0, 220.0, 800.0];
const TEXTURE_SIZE: u32 = 128;
/// Centre of the snowy high ground at the back (world metres).
const PEAKS: Vec3 = Vec3::new(80.0, 0.0, 260.0);

fn settings() -> WorldBuilder {
    WorldBuilder {
        terrain_data: DATA.into(),
        size: SIZE,
        heightmap_resolution: 257,
        alphamap_resolution: 128,
        base: BaseSettings {
            generator: Generator::Mountains,
            height: 0.6,
            scale: 450.0,
            noise_amount: 0.3,
            octaves: 6,
            ridge_sharpness: 1.6,
            seed: 7,
        },
        erosion: ErosionSettings {
            hydraulic_iterations: 2,
            thermal_iterations: 10,
            ..Default::default()
        },
        default_biome: Preset::Grassland.path(),
        biome_breakup: 14.0,
        live_preview: true,
        scatter_storage: ScatterStorage::Compact,
    }
}

fn stamps() -> Vec<(&'static str, Vec3, f32, WorldStamp)> {
    let stamp = |radius, falloff, kind| WorldStamp {
        enabled: true,
        radius,
        falloff,
        kind,
    };
    let zone = |preset: Preset| {
        StampKind::BiomeZone(BiomeZone {
            biome: preset.path(),
            polygon: Vec::new(),
        })
    };
    vec![
        (
            "Snowy Peaks",
            PEAKS,
            0.0,
            stamp(260.0, 0.5, zone(Preset::SnowyMountains)),
        ),
        (
            "Forest",
            Vec3::new(-180.0, 0.0, -80.0),
            0.0,
            stamp(160.0, 0.5, zone(Preset::Forest)),
        ),
    ]
}

fn preset_biome(path: &str) -> Result<Biome, String> {
    Preset::ALL
        .into_iter()
        .find(|p| p.path() == path)
        .map(|p| p.biome())
        .ok_or_else(|| format!("unknown biome {path}"))
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.5;
    v.bloom.threshold = 1.0;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.3;
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Cloudy.apply(c);
    c.coverage = 0.42;
    c.wind_speed = 12.0;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 2500.0;
    f.attenuation_distance = 1800.0;
    f.base_height = 0.0;
    f.maximum_height = 120.0;
    f.anisotropy = 0.5;
    f.volumetric_distance = 160.0;
    v.sun_shafts.active = true;
    v.sun_shafts.intensity = 0.4;
    let ao = &mut v.ambient_occlusion;
    ao.active = true;
    ao.intensity = 1.2;
    v
}

/// Camera, sun, terrain, the World Builder with its stamps and the volume (not generated yet).
fn base_scene() -> (Scene, Entity) {
    let mut s = Scene::default_3d("Main");
    if let Some(sun) = place(
        &mut s,
        "Directional Light",
        Vec3::new(0.0, 100.0, 0.0),
        Vec3::new(30.0, 160.0, 0.0),
    ) {
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.92, 0.8];
        l.intensity = 2.0;
        l.shadows = Shadows::Soft;
    }
    let sky = &mut s.render_settings.skybox;
    sky.sun_disk = SunDisk::HighQuality;
    sky.sun_size = 0.04;
    let terrain = s.spawn("Terrain", None);
    s.set_transform(
        terrain,
        Transform::from_position(Vec3::new(-SIZE[0] * 0.5, 0.0, -SIZE[2] * 0.5)),
    );
    s.add(
        terrain,
        Terrain {
            terrain_data: DATA.into(),
            ..Default::default()
        },
    );
    s.add(terrain, TerrainCollider::default());
    let builder = s.spawn("World Builder", None);
    s.add(builder, settings());
    for (name, p, yaw, st) in stamps() {
        let e = s.spawn(name, Some(builder));
        let m = Mat4::from_rotation_translation(glam::Quat::from_rotation_y(yaw.to_radians()), p);
        world_gen::set_world_matrix(&mut s, e, m);
        s.add(e, st);
    }
    let fx = s.spawn(VOLUME, None);
    s.add(fx, volume());
    (s, builder)
}

/// Biome presets, their textures and the scatter materials.
fn write_assets(root: &Path) -> Result<(), String> {
    for p in Preset::ALL {
        p.biome().save(&root.join(p.path()))?;
    }
    for stem in biome::TEXTURES {
        let path = root.join(biome::texture_path(stem));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let img = textures::generate(stem, TEXTURE_SIZE).ok_or(format!("texture {stem}"))?;
        img.save(&path).map_err(|e| e.to_string())?;
    }
    let biomes: Vec<Biome> = Preset::ALL.into_iter().map(|p| p.biome()).collect();
    world_gen::write_materials(root, &biomes)
}

/// Write the assets and terrain data into `root`; returns the generated scene.
pub fn build(root: &Path) -> Result<Scene, String> {
    write_assets(root)?;
    let (mut scene, builder) = base_scene();
    let (req, _warnings) =
        world_gen::request(&scene, builder, Stages::All, false, None, &preset_biome)?;
    let out = world_gen::generate::run(&req, &world_gen::Progress::default())
        .ok_or("terrain generation stopped")?;
    world_gen::apply(&mut scene, builder, &req, &out, &|_| None);
    let data_path = root.join(DATA);
    if let Some(dir) = data_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    out.data.save(&data_path)?;
    // The camera looks across the land from the south-west corner, above the hills in front.
    let (x, z, yaw) = (-300.0f32, -330.0f32, 40.0f32);
    let dir = Vec3::new(yaw.to_radians().sin(), 0.0, yaw.to_radians().cos());
    let ground = |p: Vec3| {
        out.data
            .sample_height(p.x + SIZE[0] * 0.5, p.z + SIZE[2] * 0.5)
    };
    let start = Vec3::new(x, 0.0, z);
    let highest = (0..=30)
        .map(|i| ground(start + dir * (i as f32 * 10.0)))
        .fold(0.0f32, f32::max);
    if let Some(cam) = place(
        &mut scene,
        "Main Camera",
        Vec3::new(x, highest + 30.0, z),
        Vec3::new(10.0, yaw, 0.0),
    ) && let Ok(mut c) = scene.world.get::<&mut misty_engine::scene::Camera>(cam)
    {
        c.far = 3000.0;
    }
    Ok(scene)
}
