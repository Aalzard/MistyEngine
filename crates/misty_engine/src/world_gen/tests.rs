//! World generation tests: determinism, stamps, erosion, water, biomes and scatter.

use glam::{Vec2, Vec3};

use super::biome::{Biome, Feature, Preset};
use super::classify::{TexelInfo, classify};
use super::erosion::{self, ErosionSettings};
use super::generate::{self, Request, Stages};
use super::stamps::*;
use super::*;

fn settings(res: usize) -> WorldBuilder {
    WorldBuilder {
        size: [200.0, 60.0, 200.0],
        heightmap_resolution: res,
        alphamap_resolution: 128,
        base: BaseSettings {
            generator: Generator::Mountains,
            height: 0.6,
            scale: 120.0,
            seed: 7,
            ..Default::default()
        },
        erosion: ErosionSettings {
            hydraulic_iterations: 2,
            thermal_iterations: 5,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn placed(stamp: WorldStamp, center: Vec2, y: f32) -> PlacedStamp {
    PlacedStamp {
        stamp,
        at: Placement::at(center, y),
        seed: 3,
    }
}

fn request(res: usize, stamps: Vec<PlacedStamp>) -> Request {
    let zone_biome = vec![0; stamps.len()];
    Request {
        settings: settings(res),
        stamps,
        biomes: vec![Preset::Forest.biome(), Preset::SnowyMountains.biome()],
        zone_biome,
        stages: Stages::All,
        current: None,
        preview: false,
    }
}

fn run(req: &Request) -> generate::Output {
    generate::run(req, &Progress::default()).expect("not cancelled")
}

#[test]
fn generation_is_deterministic_per_seed() {
    let req = request(65, vec![]);
    let a = run(&req);
    let b = run(&req);
    assert_eq!(a.data.heights, b.data.heights);
    assert_eq!(a.data.alphamap, b.data.alphamap);
    assert_eq!(a.scatter, b.scatter);
    let mut other = req.clone();
    other.settings.base.seed = 8;
    assert_ne!(run(&other).data.heights, a.data.heights);
    // Every generator produces something within range.
    for g in Generator::ALL {
        let s = BaseSettings {
            generator: g,
            ..Default::default()
        };
        let h = base::generate(&s, 33, Vec3::new(300.0, 80.0, 300.0));
        let (lo, hi) = h
            .iter()
            .fold((1.0f32, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(lo >= 0.0 && hi <= 1.0 && hi > lo, "{g:?} {lo} {hi}");
    }
}

#[test]
fn flatten_makes_the_area_flat() {
    let stamp = WorldStamp {
        radius: 30.0,
        falloff: 0.3,
        kind: StampKind::Flatten(Flatten::default()),
        ..Default::default()
    };
    let center = Vec2::new(100.0, 100.0);
    let out = run(&request(129, vec![placed(stamp, center, 20.0)]));
    let d = &out.data;
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            let p = generate::sample_point(d, ix, iz);
            // Inside the hard part of the radius (1 - falloff).
            if p.distance(center) < 30.0 * 0.65 {
                let h = d.height(ix, iz) * d.size.y;
                assert!((h - 20.0).abs() < 0.05, "{p}: {h}");
            }
        }
    }
}

#[test]
fn river_lowers_the_terrain_along_the_spline() {
    let river = River {
        points: vec![[-60.0, -20.0], [0.0, 10.0], [60.0, -10.0]],
        width: 8.0,
        depth: 4.0,
        bank: 6.0,
        water: true,
    };
    let stamp = WorldStamp {
        kind: StampKind::River(river.clone()),
        ..Default::default()
    };
    let center = Vec2::new(100.0, 100.0);
    let before = run(&request(129, vec![]));
    let after = run(&request(129, vec![placed(stamp.clone(), center, 0.0)]));
    let st = placed(stamp, center, 0.0);
    for p in river_line(&st, &river) {
        let a = before.data.sample_height(p.x, p.y);
        let b = after.data.sample_height(p.x, p.y);
        assert!(b < a - 2.0 || a < 2.0, "{p}: {a} → {b}");
    }
    // Far from the river nothing changes.
    let far = Vec2::new(10.0, 190.0);
    assert_eq!(
        before.data.sample_height(far.x, far.y),
        after.data.sample_height(far.x, far.y)
    );
    assert!(after.water.iter().any(|w| w.name == "River"));
}

/// A noisy slope for the erosion tests (metres).
fn slope_map(res: usize) -> Vec<f32> {
    let f = noise::Fractal::default();
    (0..res * res)
        .map(|i| {
            let (x, z) = ((i % res) as f32, (i / res) as f32);
            40.0 - z * 0.25 + 2.0 * noise::fractal(x * 0.05, z * 0.05, 3, &f)
        })
        .collect()
}

#[test]
fn erosion_conserves_mass_and_cuts_channels() {
    let res = 129;
    let mut h = slope_map(res);
    let before = h.clone();
    let s = ErosionSettings {
        hydraulic_iterations: 16,
        hydraulic_strength: 0.8,
        ..Default::default()
    };
    assert!(erosion::hydraulic(
        &mut h,
        res,
        1.0,
        &s,
        1,
        &Progress::default()
    ));
    let sum = |v: &[f32]| v.iter().map(|x| *x as f64).sum::<f64>();
    let (a, b) = (sum(&before), sum(&h));
    assert!((a - b).abs() / a < 1e-3, "mass {a} → {b}");
    // Channels: some cells are cut well below where they were, and the surface across the
    // slope gets rougher.
    let cut = before.iter().zip(&h).filter(|(a, b)| *a - *b > 0.3).count();
    assert!(cut > res * 2, "{cut} cells cut");
    let rough = |v: &[f32]| {
        let mut r = 0.0;
        for z in 0..res {
            for x in 1..res {
                r += (v[z * res + x] - v[z * res + x - 1]).abs() as f64;
            }
        }
        r
    };
    assert!(
        rough(&h) > rough(&before) * 1.05,
        "{} vs {}",
        rough(&h),
        rough(&before)
    );
    // Same seed, same result.
    let mut again = before.clone();
    erosion::hydraulic(&mut again, res, 1.0, &s, 1, &Progress::default());
    assert_eq!(again, h);

    // Thermal: mass conserved, slopes relax below the talus angle.
    let mut spike = vec![0.0f32; 33 * 33];
    spike[16 * 33 + 16] = 50.0;
    let t = ErosionSettings {
        thermal_iterations: 200,
        talus_angle: 30.0,
        thermal_strength: 0.5,
        ..Default::default()
    };
    assert!(erosion::thermal(
        &mut spike,
        33,
        1.0,
        &t,
        &Progress::default()
    ));
    assert!((sum(&spike) - 50.0).abs() < 1e-2, "{}", sum(&spike));
    assert!(spike[16 * 33 + 16] < 20.0);
}

#[test]
fn erosion_can_be_cancelled() {
    let mut h = slope_map(65);
    let p = Progress::default();
    p.cancel();
    assert!(!erosion::hydraulic(
        &mut h,
        65,
        1.0,
        &ErosionSettings::default(),
        1,
        &p
    ));
}

#[test]
fn lake_fill_stays_in_its_basin() {
    // Two bowls separated by a ridge higher than the lake level.
    let res = 65;
    let h: Vec<f32> = (0..res * res)
        .map(|i| {
            let (x, z) = ((i % res) as f32, (i / res) as f32);
            let a = ((x - 16.0).powi(2) + (z - 32.0).powi(2)).sqrt();
            let b = ((x - 48.0).powi(2) + (z - 32.0).powi(2)).sqrt();
            a.min(b) * 0.5
        })
        .collect();
    let wet = water::flood_fill(&h, res, (16, 32), 6.0).unwrap();
    for (i, w) in wet.iter().enumerate() {
        if *w {
            assert!(h[i] < 6.0);
            assert!(i % res < 32, "stays in the left bowl");
        }
    }
    assert!(wet[32 * res + 16] && !wet[32 * res + 48]);
    assert!(
        water::flood_fill(&h, res, (0, 0), 6.0).is_none(),
        "seed above the level"
    );
}

#[test]
fn scatter_respects_slope_limits_and_spacing() {
    let out = run(&request(129, vec![]));
    let batches = out.scatter.as_ref().unwrap();
    let biome = Preset::Forest.biome();
    let mut total = 0;
    for b in batches.iter().filter(|b| b.biome == 0) {
        let item = &biome.scatter[b.item];
        for (i, a) in b.instances.iter().enumerate() {
            let p = Vec2::new(a.position.x, a.position.z);
            // Slope at the instance (same field the scatter read).
            let n = texel_slope(&out.data, p);
            assert!(n <= item.max_slope + 1e-3, "{} on {n}°", item.name);
            for c in &b.instances[i + 1..] {
                let q = Vec2::new(c.position.x, c.position.z);
                assert!(p.distance(q) >= item.min_spacing - 1e-3, "{}", item.name);
            }
        }
        total += b.instances.len();
    }
    assert!(total > 50, "{total} instances");
    // Packing round-trips (within quantisation).
    let b = batches.iter().find(|b| !b.instances.is_empty()).unwrap();
    let set = ScatterSet::new(Vec::new(), &b.instances);
    let back = set.instances();
    assert_eq!(back.len(), b.instances.len());
    for (a, c) in b.instances.iter().zip(&back) {
        assert!(a.position.distance(c.position) < 0.02);
        assert!((a.scale - c.scale).abs() < 1e-3);
    }
    assert_eq!(set.matrices().len(), back.len());
}

/// Slope (degrees) the scatter saw for a point: its splat texel's, like `classify::fields`.
fn texel_slope(d: &crate::terrain::TerrainData, p: Vec2) -> f32 {
    let n = d.alphamap_resolution as f32;
    let tx = (p.x / d.size.x * n).clamp(0.0, n - 1.0).floor();
    let tz = (p.y / d.size.z * n).clamp(0.0, n - 1.0).floor();
    let (x, z) = ((tx + 0.5) / n * d.size.x, (tz + 0.5) / n * d.size.z);
    let s = d.spacing();
    let gx = (d.sample_height(x + s.x, z) - d.sample_height(x - s.x, z)) / (2.0 * s.x);
    let gz = (d.sample_height(x, z + s.y) - d.sample_height(x, z - s.y)) / (2.0 * s.y);
    (gx * gx + gz * gz).sqrt().atan().to_degrees()
}

#[test]
fn biome_classification() {
    let b = Biome::default();
    let flat = TexelInfo {
        height: 10.0,
        height01: 0.2,
        slope: 3.0,
        water_dist: f32::INFINITY,
        above_water: f32::INFINITY,
        ..Default::default()
    };
    let top = |w: [f32; 5]| Feature::ALL[(0..5).max_by(|a, c| w[*a].total_cmp(&w[*c])).unwrap()];
    assert_eq!(top(classify(&flat, &b, 0.0)), Feature::Flat);
    let steep = TexelInfo {
        slope: 55.0,
        ..flat
    };
    assert_eq!(top(classify(&steep, &b, 0.0)), Feature::Cliff);
    let hill = TexelInfo {
        slope: 30.0,
        ..flat
    };
    assert_eq!(top(classify(&hill, &b, 0.0)), Feature::Slope);
    let peak = TexelInfo {
        height01: 0.9,
        ..flat
    };
    assert_eq!(top(classify(&peak, &b, 0.0)), Feature::Peak);
    let beach = TexelInfo {
        above_water: 0.5,
        water_dist: 2.0,
        ..flat
    };
    assert_eq!(top(classify(&beach, &b, 0.0)), Feature::Shore);
    let under = TexelInfo {
        above_water: -1.0,
        ..flat
    };
    assert_eq!(classify(&under, &b, 0.0)[3], 1.0);
    // Rock areas turn moderate slopes into rock.
    assert_eq!(top(classify(&hill, &b, 1.0)), Feature::Cliff);
    for t in [flat, steep, hill, peak, beach] {
        let s: f32 = classify(&t, &b, 0.3).iter().sum();
        assert!((s - 1.0).abs() < 1e-4);
    }
}

#[test]
fn biome_zones_paint_their_textures() {
    // A snowy zone in the middle of a forest: snow is used there, grass outside.
    let zone = WorldStamp {
        radius: 50.0,
        falloff: 0.2,
        kind: StampKind::BiomeZone(BiomeZone::default()),
        ..Default::default()
    };
    let mut req = request(65, vec![placed(zone, Vec2::new(100.0, 100.0), 0.0)]);
    req.zone_biome = vec![1];
    req.settings.base.generator = Generator::RollingHills;
    req.settings.base.height = 0.05;
    req.settings.biome_breakup = 0.0;
    let out = run(&req);
    let layer = |name: &str| out.data.layers.iter().position(|l| l.name == name);
    let (snow, grass) = (layer("Snow").unwrap(), layer("Grass").unwrap());
    let w = |x: f32, z: f32| out.data.weights_at(x, z);
    assert!(w(100.0, 100.0)[snow] > 0.9, "{:?}", w(100.0, 100.0));
    assert!(w(10.0, 10.0)[grass] > 0.9, "{:?}", w(10.0, 10.0));
    assert!(out.data.layers.len() <= crate::terrain::MAX_LAYERS);
}

#[test]
fn mountain_stamp_blend_modes() {
    let center = Vec2::new(100.0, 100.0);
    let mk = |blend| {
        let s = WorldStamp {
            radius: 40.0,
            kind: StampKind::Mountain(Mountain {
                height: 30.0,
                blend,
                roughness: 0.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        run(&request(65, vec![placed(s, center, 5.0)]))
            .data
            .sample_height(center.x, center.y)
    };
    let plain = run(&request(65, vec![]))
        .data
        .sample_height(center.x, center.y);
    assert!((mk(BlendMode::Add) - (plain + 30.0)).abs() < 1.0);
    assert!((mk(BlendMode::Replace) - 35.0).abs() < 1.0);
    assert!(mk(BlendMode::Max) >= plain.max(34.0));
    // Stamp shapes are bounded.
    for shape in MountainShape::ALL {
        let s = WorldStamp {
            kind: StampKind::Mountain(Mountain {
                shape,
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = run(&request(65, vec![placed(s, center, 0.0)]));
        assert!(out.data.heights.iter().all(|h| (0.0..=1.0).contains(h)));
    }
}

#[test]
fn stages_keep_what_they_do_not_rebuild() {
    let mut req = request(65, vec![]);
    let full = run(&req);
    req.current = Some(full.data.clone());
    req.stages = Stages::Textures;
    req.settings.base.seed = 99;
    let tex = run(&req);
    assert_eq!(
        tex.data.heights, full.data.heights,
        "textures only keeps heights"
    );
    assert!(tex.scatter.is_none());
    req.stages = Stages::Heights;
    let heights = run(&req);
    assert_ne!(heights.data.heights, full.data.heights);
    assert_eq!(heights.data.alphamap, full.data.alphamap);
    req.preview = true;
    req.stages = Stages::All;
    req.settings.heightmap_resolution = 513;
    let prev = run(&req);
    assert_eq!(prev.data.heightmap_resolution, generate::PREVIEW_RESOLUTION);
    assert!(prev.scatter.is_none());
}

#[test]
fn scene_round_trip_and_apply() {
    use crate::scene::{Scene, Terrain};
    let mut scene = Scene::new("t");
    let terrain = scene.spawn("Terrain", None);
    scene.add(
        terrain,
        Terrain {
            terrain_data: "Assets/T.terrain".into(),
            ..Default::default()
        },
    );
    let builder = scene.spawn("World Builder", None);
    let mut s = settings(65);
    s.terrain_data = "Assets/T.terrain".into();
    s.scatter_storage = ScatterStorage::Compact;
    scene.add(builder, s);
    let stamp = scene.spawn("Hill", Some(builder));
    scene.set_transform(
        stamp,
        Transform::from_position(Vec3::new(100.0, 0.0, 100.0)),
    );
    scene.add(stamp, WorldStamp::default());
    let load = |p: &str| {
        Preset::ALL
            .into_iter()
            .find(|b| b.path() == p)
            .map(|b| b.biome())
            .ok_or_else(|| "missing".to_string())
    };
    let (req, warnings) = super::request(&scene, builder, Stages::All, false, None, &load).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(req.stamps.len(), 1);
    assert_eq!(req.stamps[0].at.center, Vec2::new(100.0, 100.0));
    let out = run(&req);
    let n = super::apply(&mut scene, builder, &req, &out, &|_| None);
    assert!(n > 0);
    let draws = crate::render_scene::mesh_draws(&scene, &NoMaterials, |_| false);
    assert!(draws.len() >= n, "{} draws for {n} instances", draws.len());
    // Scene files keep everything.
    let json = scene.to_json();
    let (back, report) = Scene::from_json("t", &json).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.to_json(), json);
    let again = crate::render_scene::mesh_draws(&back, &NoMaterials, |_| false);
    assert_eq!(again.len(), draws.len());
    // Generating twice replaces the generated objects.
    let before = scene.len();
    super::apply(&mut scene, builder, &req, &out, &|_| None);
    assert_eq!(scene.len(), before);
}

struct NoMaterials;
impl crate::render_scene::MaterialSource for NoMaterials {
    fn params(&self, _: Option<&str>) -> crate::render::MaterialParams {
        Default::default()
    }
}

#[test]
fn presets_save_and_load() {
    let dir = std::env::temp_dir().join("misty_world_gen_presets");
    let _ = std::fs::remove_dir_all(&dir);
    for p in Preset::ALL {
        let path = dir.join(p.path());
        p.biome().save(&path).unwrap();
        assert_eq!(Biome::load(&path).unwrap(), p.biome());
    }
    for stem in super::biome::TEXTURES {
        assert!(textures::generate(stem, 16).is_some(), "{stem}");
    }
    write_materials(&dir, &[Preset::Forest.biome()]).unwrap();
    assert!(dir.join(water::MATERIAL).is_file());
    // Old files without newer fields still load.
    let b: Biome = serde_json::from_str(r#"{"name":"Old"}"#).unwrap();
    assert_eq!(b.name, "Old");
}

/// Timing of a 1k² erosion (`cargo test --release -p misty_engine erosion_benchmark -- --ignored
/// --nocapture`).
#[test]
#[ignore = "benchmark"]
fn erosion_benchmark_1k() {
    let res = 1025;
    let mut h = slope_map(res);
    let s = ErosionSettings::default();
    let t = std::time::Instant::now();
    erosion::hydraulic(&mut h, res, 1.0, &s, 1, &Progress::default());
    let hyd = t.elapsed();
    let t = std::time::Instant::now();
    erosion::thermal(&mut h, res, 1.0, &s, &Progress::default());
    println!("1025²: hydraulic {hyd:?}, thermal {:?}", t.elapsed());
}
