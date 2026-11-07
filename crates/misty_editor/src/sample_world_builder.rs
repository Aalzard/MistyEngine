//! Procedural WorldBuilderDemo scene in `SampleProject`: a mountain island made entirely with the
//! World Builder — an Islands base with erosion, a volcano holding a crater lake, a river down to
//! the south coast, a flattened village plateau, a rock area, forest (default), snowy-peak and
//! beach biomes, the sea, and compact instanced scatter.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_world_builder_demo`; the other
//! tests check the committed files still match the generator and that the scene plays.

use std::path::{Path, PathBuf};

use glam::{Mat4, Vec3};
use misty_engine::scene::{Scene, Terrain, TerrainCollider, Transform};
use misty_engine::terrain::TerrainData;
use misty_engine::world_gen::stamps::{
    BiomeZone, BlendMode, Flatten, Mountain, MountainShape, River, RockArea, Water, WaterMode,
};
use misty_engine::world_gen::{
    self, BaseSettings, Biome, ErosionSettings, Generator, Preset, ScatterStorage, Stages,
    StampKind, WorldBuilder, WorldStamp,
};

const SCENE: &str = "Assets/Scenes/WorldBuilderDemo.scene";
const DATA: &str = "Assets/WorldBuilder/WorldBuilderDemo.terrain";
const SIZE: [f32; 3] = [600.0, 160.0, 600.0];

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn settings() -> WorldBuilder {
    WorldBuilder {
        terrain_data: DATA.into(),
        size: SIZE,
        heightmap_resolution: 513,
        alphamap_resolution: 256,
        base: BaseSettings {
            generator: Generator::Islands,
            height: 0.45,
            scale: 220.0,
            noise_amount: 0.5,
            octaves: 7,
            ridge_sharpness: 2.2,
            seed: 2024,
        },
        erosion: ErosionSettings::default(),
        default_biome: Preset::Forest.path(),
        biome_breakup: 14.0,
        live_preview: false,
        scatter_storage: ScatterStorage::Compact,
    }
}

/// The stamps: (name, world position, yaw degrees, stamp).
fn stamps() -> Vec<(&'static str, Vec3, f32, WorldStamp)> {
    let stamp = |radius, falloff, kind| WorldStamp {
        enabled: true,
        radius,
        falloff,
        kind,
    };
    vec![
        (
            "Volcano",
            Vec3::new(30.0, 0.0, 40.0),
            0.0,
            stamp(
                115.0,
                0.6,
                StampKind::Mountain(Mountain {
                    shape: MountainShape::Volcano,
                    height: 115.0,
                    blend: BlendMode::Replace,
                    roughness: 0.25,
                }),
            ),
        ),
        (
            "Crater Lake",
            // Crater floor at 0.31 × 115 m ≈ 35 m; rim ≈ 81 m.
            Vec3::new(30.0, 50.0, 40.0),
            0.0,
            stamp(
                30.0,
                0.0,
                StampKind::Water(Water {
                    mode: WaterMode::Lake,
                }),
            ),
        ),
        (
            "Village Plateau",
            Vec3::new(-130.0, 0.0, -70.0),
            0.0,
            stamp(38.0, 0.45, StampKind::Flatten(Flatten { sample: true })),
        ),
        (
            "River",
            Vec3::new(30.0, 0.0, 40.0),
            0.0,
            stamp(
                0.0,
                0.0,
                StampKind::River(River {
                    points: vec![
                        [20.0, -90.0],
                        [50.0, -140.0],
                        [40.0, -200.0],
                        [70.0, -260.0],
                        [80.0, -330.0],
                    ],
                    width: 10.0,
                    depth: 3.5,
                    bank: 14.0,
                    water: true,
                }),
            ),
        ),
        (
            "Cliffs",
            Vec3::new(140.0, 0.0, 90.0),
            0.0,
            stamp(60.0, 0.5, StampKind::RockArea(RockArea { strength: 1.0 })),
        ),
        (
            "Snowy Peak",
            Vec3::new(30.0, 0.0, 40.0),
            0.0,
            stamp(
                95.0,
                0.4,
                StampKind::BiomeZone(BiomeZone {
                    biome: Preset::SnowyMountains.path(),
                    polygon: Vec::new(),
                }),
            ),
        ),
        (
            "South-West Beach",
            Vec3::new(-170.0, 0.0, -170.0),
            0.0,
            stamp(
                110.0,
                0.5,
                StampKind::BiomeZone(BiomeZone {
                    biome: Preset::BeachCoast.path(),
                    polygon: Vec::new(),
                }),
            ),
        ),
        (
            "Sea",
            Vec3::new(0.0, 9.0, 0.0),
            0.0,
            stamp(
                40.0,
                0.0,
                StampKind::Water(Water {
                    mode: WaterMode::Sea,
                }),
            ),
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

/// The scene before generating: camera, light, terrain, World Builder and its stamps.
fn base_scene() -> (Scene, misty_engine::hecs::Entity) {
    let mut s = Scene::default_3d("WorldBuilderDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        let mut t = Transform::from_position(Vec3::new(-250.0, 130.0, -330.0))
            .with_euler(Vec3::new(14.0, 36.0, 0.0));
        t.scale = Vec3::ONE;
        s.set_transform(cam, t);
        if let Ok(mut c) = s.world.get::<&mut misty_engine::scene::Camera>(cam) {
            c.far = 2000.0;
        }
    }
    if let Some(light) = s.find_by_name("Directional Light") {
        s.set_transform(
            light,
            Transform::from_position(Vec3::new(0.0, 100.0, 0.0))
                .with_euler(Vec3::new(40.0, -35.0, 0.0)),
        );
    }
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
        world_gen::set_world_matrix(
            &mut s,
            e,
            Mat4::from_rotation_translation(glam::Quat::from_rotation_y(yaw.to_radians()), p),
        );
        s.add(e, st);
    }
    (s, builder)
}

/// Generate everything: the terrain data and the finished scene.
fn generate() -> (TerrainData, Scene) {
    let (mut scene, builder) = base_scene();
    let (req, warnings) =
        world_gen::request(&scene, builder, Stages::All, false, None, &preset_biome).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let out = world_gen::generate::run(&req, &world_gen::Progress::default()).unwrap();
    world_gen::apply(&mut scene, builder, &req, &out, &|_| None);
    (out.data, scene)
}

fn biomes() -> Vec<Biome> {
    Preset::ALL.into_iter().map(|p| p.biome()).collect()
}

/// Write presets, textures and materials (shared assets of the World Builder folder).
fn write_assets(root: &Path) {
    for p in Preset::ALL {
        p.biome().save(&root.join(p.path())).unwrap();
    }
    for stem in world_gen::biome::TEXTURES {
        let img = world_gen::textures::generate(stem, 256).unwrap();
        let path = root.join(world_gen::biome::texture_path(stem));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        img.save(path).unwrap();
    }
    world_gen::write_materials(root, &biomes()).unwrap();
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_world_builder_demo() {
    let root = project();
    write_assets(&root);
    let (data, scene) = generate();
    data.save(&root.join(DATA)).unwrap();
    scene.save(&root.join(SCENE)).unwrap();
}

#[test]
fn world_builder_demo_matches_its_generator() {
    let root = project();
    for p in Preset::ALL {
        crate::test_util::assert_close(
            &Biome::load(&root.join(p.path())).unwrap(),
            &p.biome(),
            &format!("{}", p.path()),
        );
    }
    for stem in world_gen::biome::TEXTURES {
        let on_disk = misty_engine::image::open(root.join(world_gen::biome::texture_path(stem)))
            .unwrap()
            .into_rgba8();
        assert_eq!(
            on_disk,
            world_gen::textures::generate(stem, 256).unwrap(),
            "{stem}"
        );
    }
    let (data, scene) = generate();
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    assert_eq!(on_disk.layers, data.layers);
    assert_eq!(on_disk.heightmap_resolution, data.heightmap_resolution);
    for (a, b) in on_disk.heights.iter().zip(&data.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    for (a, b) in on_disk.alphamap.iter().zip(&data.alphamap) {
        assert!(a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 2));
    }
    let (loaded, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(&loaded.to_json(), &scene.to_json(), SCENE);
}

/// The demo has what it promises: lake in the crater, river and sea water, a flat plateau,
/// snow on the peak, sand on the beach, and thousands of instanced scatter objects that play.
#[test]
fn world_builder_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, LogKind, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let data = TerrainData::load(&root.join(DATA)).unwrap();
    let names: Vec<String> = scene.walk().iter().map(|(e, _)| scene.name(*e)).collect();
    for n in ["Lake", "Sea", "River"] {
        assert!(names.iter().any(|x| x == n), "{n} water");
    }
    // Plateau: flat within 0.5 m over its inner radius.
    let corner = Vec3::new(-SIZE[0] * 0.5, 0.0, -SIZE[2] * 0.5);
    let local = |x: f32, z: f32| (x - corner.x, z - corner.z);
    let (px, pz) = local(-130.0, -70.0);
    let hs: Vec<f32> = (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            data.sample_height(px + a.cos() * 15.0, pz + a.sin() * 15.0)
        })
        .collect();
    let (lo, hi) = hs
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)));
    assert!(hi - lo < 0.5, "plateau {lo}..{hi}");
    let layer = |name: &str| data.layers.iter().position(|l| l.name == name).unwrap();
    // Snow around the volcano's rim, sand on the south-west beach.
    let best = |name: &str, x: f32, z: f32, r: f32| {
        let (cx, cz) = local(x, z);
        let l = layer(name);
        (0..400)
            .map(|i| {
                let (a, d) = (i as f32 * 2.4, r * (i as f32 / 400.0).sqrt());
                data.weights_at(cx + a.cos() * d, cz + a.sin() * d)[l]
            })
            .fold(0.0f32, f32::max)
    };
    assert!(best("Snow", 30.0, 40.0, 60.0) > 0.5, "snow on the peak");
    assert!(
        best("Sand", -170.0, -170.0, 80.0) > 0.5,
        "sand on the beach"
    );
    // Scatter is drawn through the renderer's instancing.
    struct NoMaterials;
    impl misty_engine::render_scene::MaterialSource for NoMaterials {
        fn params(&self, _: Option<&str>) -> misty_engine::render::MaterialParams {
            Default::default()
        }
    }
    let draws = misty_engine::render_scene::mesh_draws(&scene, &NoMaterials, |_| false);
    eprintln!("WorldBuilderDemo: {} mesh draws", draws.len());
    assert!(draws.len() > 1500, "{} draws", draws.len());
    // Play a second of physics on the terrain collider.
    let terrain = scene.find_by_name("Terrain").unwrap();
    scene
        .world
        .get::<&mut Terrain>(terrain)
        .unwrap()
        .terrain_data = root.join(DATA).to_string_lossy().into_owned();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..60 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    for (kind, text) in host.take_logs() {
        assert_ne!(kind, LogKind::Error, "{text}");
    }
}
