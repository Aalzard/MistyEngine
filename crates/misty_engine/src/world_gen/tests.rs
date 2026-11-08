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
        preview_resolution: generate::PREVIEW_RESOLUTION,
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

/// Scatter Storage ▸ Terrain Trees: tree items become terrain tree instances (prototypes
/// added), other items stay scene objects, hand-painted trees of other prototypes survive a
/// regeneration and the generated ones are replaced, not doubled.
#[test]
fn scatter_becomes_editable_terrain_trees() {
    use crate::scene::{Scene, Terrain};
    let mut req = request(65, Vec::new());
    req.settings.scatter_storage = ScatterStorage::TerrainTrees;
    let forest = &req.biomes[0];
    let tree_items: Vec<usize> = (0..forest.scatter.len())
        .filter(|i| tree_asset(&forest.scatter[*i]).is_some())
        .collect();
    assert!(!tree_items.is_empty(), "the forest preset has trees");
    let out = run(&req);
    let batches = out.scatter.as_ref().expect("scatter ran");
    let expected: usize = (batches.iter())
        .filter(|b| tree_asset(&req.biomes[b.biome].scatter[b.item]).is_some())
        .map(|b| b.instances.len())
        .sum();
    assert!(expected > 10, "{expected} trees scattered");
    let trees = &out.data.trees;
    assert_eq!(
        trees.instances.len(),
        expected,
        "every tree in the terrain data"
    );
    assert!(trees.prototypes.iter().any(|p| p.tree == BROADLEAF_TREE));
    // On the ground where the scatter put them.
    let b = batches
        .iter()
        .find(|b| {
            tree_asset(&req.biomes[b.biome].scatter[b.item]).is_some() && !b.instances.is_empty()
        })
        .unwrap();
    let first = b.instances[0].position;
    assert!(trees.instances.iter().any(|t| {
        let p = crate::terrain::trees::local_position(&out.data, t);
        (p.x - first.x).abs() < 1e-3 && (p.z - first.z).abs() < 1e-3
    }));

    // Applying puts only the other items into the scene.
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
    scene.add(builder, req.settings.clone());
    super::apply(&mut scene, builder, &req, &out, &|_| None);
    let names: Vec<String> = (scene.world.query::<&crate::scene::GameObject>().iter())
        .map(|g| g.name.clone())
        .collect();
    for i in &tree_items {
        let item = &req.biomes[0].scatter[*i];
        let holder = format!("{} {}", req.biomes[0].name, item.name);
        assert!(
            !names.contains(&holder),
            "{holder} went into the terrain, not the scene"
        );
    }

    // A hand-painted tree of another asset survives regenerating; generated ones are replaced.
    let mut current = out.data.clone();
    current
        .trees
        .prototypes
        .push(crate::terrain::trees::TreePrototype {
            tree: "Assets/Trees/Mine.tree".into(),
        });
    let mine = current.trees.prototypes.len() as u16 - 1;
    current
        .trees
        .instances
        .push(crate::terrain::trees::TreeInstance {
            prototype: mine,
            ..Default::default()
        });
    req.current = Some(current);
    let again = run(&req);
    let t = &again.data.trees;
    assert_eq!(t.instances.len(), expected + 1);
    assert_eq!(
        t.instances.iter().filter(|i| i.prototype == mine).count(),
        1
    );
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

/// The WorldBuilderDemo's request (see the editor's `sample_world_builder.rs`) at `res`.
pub(super) fn demo_request(res: usize) -> Request {
    use super::stamps::{BiomeZone, Mountain, MountainShape, RockArea, Water, WaterMode};
    let stamp = |radius, falloff, kind| WorldStamp {
        enabled: true,
        radius,
        falloff,
        kind,
    };
    // World positions; the terrain's corner is at (-300, 0, -300).
    let at = |x: f32, y: f32, z: f32, st: WorldStamp, seed: u32| PlacedStamp {
        stamp: st,
        at: Placement::at(Vec2::new(x + 300.0, z + 300.0), y),
        seed: 2024u32.wrapping_add(seed * 7919),
    };
    let stamps = vec![
        at(
            30.0,
            0.0,
            40.0,
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
            0,
        ),
        at(
            30.0,
            50.0,
            40.0,
            stamp(
                30.0,
                0.0,
                StampKind::Water(Water {
                    mode: WaterMode::Lake,
                }),
            ),
            1,
        ),
        at(
            -130.0,
            0.0,
            -70.0,
            stamp(38.0, 0.45, StampKind::Flatten(Flatten { sample: true })),
            2,
        ),
        at(
            30.0,
            0.0,
            40.0,
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
            3,
        ),
        at(
            140.0,
            0.0,
            90.0,
            stamp(60.0, 0.5, StampKind::RockArea(RockArea { strength: 1.0 })),
            4,
        ),
        at(
            30.0,
            0.0,
            40.0,
            stamp(
                95.0,
                0.4,
                StampKind::BiomeZone(BiomeZone {
                    biome: Preset::SnowyMountains.path(),
                    polygon: Vec::new(),
                }),
            ),
            5,
        ),
        at(
            -170.0,
            0.0,
            -170.0,
            stamp(
                110.0,
                0.5,
                StampKind::BiomeZone(BiomeZone {
                    biome: Preset::BeachCoast.path(),
                    polygon: Vec::new(),
                }),
            ),
            6,
        ),
        at(
            0.0,
            9.0,
            0.0,
            stamp(
                40.0,
                0.0,
                StampKind::Water(Water {
                    mode: WaterMode::Sea,
                }),
            ),
            7,
        ),
    ];
    Request {
        settings: WorldBuilder {
            size: [600.0, 160.0, 600.0],
            heightmap_resolution: res,
            alphamap_resolution: (res - 1).clamp(256, 1024),
            base: BaseSettings {
                generator: Generator::Islands,
                height: 0.45,
                scale: 220.0,
                noise_amount: 0.5,
                octaves: 7,
                ridge_sharpness: 2.2,
                seed: 2024,
            },
            biome_breakup: 14.0,
            scatter_storage: ScatterStorage::Compact,
            ..Default::default()
        },
        zone_biome: vec![0, 0, 0, 0, 0, 1, 2, 0],
        stamps,
        biomes: vec![
            Preset::Forest.biome(),
            Preset::SnowyMountains.biome(),
            Preset::BeachCoast.biome(),
        ],
        stages: Stages::All,
        current: None,
        preview: false,
        preview_resolution: generate::PREVIEW_RESOLUTION,
    }
}

/// Stage timings of the demo at 513² and 1025² (`cargo test --release -p misty_engine
/// world_gen_profile -- --ignored --nocapture`).
#[test]
#[ignore = "benchmark"]
fn world_gen_profile() {
    for res in [513, 1025] {
        let req = demo_request(res);
        let t = std::time::Instant::now();
        let out = run(&req);
        let total = t.elapsed().as_secs_f32() * 1000.0;
        let stages: Vec<String> = out
            .timings
            .iter()
            .map(|(n, ms)| format!("{n} {ms:.0}"))
            .collect();
        println!("{res}²: total {total:.0} ms | {}", stages.join(" | "));
    }
}

#[test]
fn preview_matches_the_full_result_at_large_scale() {
    let full = run(&demo_request(513));
    let mut req = demo_request(513);
    req.preview = true;
    req.preview_resolution = 129;
    let coarse = run(&req);
    assert_eq!(coarse.data.heightmap_resolution, 129);
    assert!(coarse.scatter.is_none());
    let a = preview::resample(&full.data.heights, 513, 48);
    let b = preview::resample(&coarse.data.heights, 129, 48);
    let r = preview::correlation(&a, &b);
    // The sharp pass (257²) is closer still.
    req.preview_resolution = 257;
    let sharp = run(&req);
    let c = preview::resample(&sharp.data.heights, 257, 48);
    let r2 = preview::correlation(&a, &c);
    eprintln!("preview correlation: 129² {r}, 257² {r2}");
    assert!(r > 0.93, "coarse correlation {r}");
    assert!(r2 > r && r2 > 0.96, "sharp correlation {r2}");
    // Images in every mode have the requested size and are not blank.
    for mode in preview::PreviewMode::ALL {
        let img = preview::render(&coarse.data, Some(&coarse.wet), mode, 64, &|_| {
            [0.4, 0.6, 0.3]
        });
        assert_eq!(img.rgba.len(), 64 * 64);
        let first = img.rgba[0];
        assert!(img.rgba.iter().any(|p| *p != first), "{mode:?} is flat");
    }
    // Relief shows the sea in blue at the island's edge.
    let img = preview::render(
        &coarse.data,
        Some(&coarse.wet),
        preview::PreviewMode::Relief,
        64,
        &|_| [0.5; 3],
    );
    let corner = img.rgba[0];
    assert!(corner[2] > corner[0] + 20, "sea at the corner: {corner:?}");
}

#[test]
fn incremental_regeneration_reuses_stages_and_matches_a_full_run() {
    let mut cache = generate::Cache::default();
    let p = Progress::default();
    let mut req = demo_request(129);
    let first = generate::run_cached(&req, &p, &mut cache).unwrap();
    assert!(first.reused.is_empty());
    // Nothing changed: everything comes from the cache, identical.
    let again = generate::run_cached(&req, &p, &mut cache).unwrap();
    assert_eq!(again.reused.len(), generate::STAGES.len());
    assert!(again.timings.is_empty(), "{:?}", again.timings);
    assert_eq!(again.data, first.data);
    assert_eq!(again.scatter, first.scatter);
    // Move a Biome Zone: heights and water are reused, the rest re-runs.
    let zone = req
        .stamps
        .iter()
        .position(|s| matches!(s.stamp.kind, StampKind::BiomeZone(_)))
        .unwrap();
    req.stamps[zone].at.center += Vec2::new(40.0, -25.0);
    let moved = generate::run_cached(&req, &p, &mut cache).unwrap();
    assert_eq!(&moved.reused[..], &generate::STAGES[..5]);
    let fresh = run(&req);
    assert_eq!(moved.data, fresh.data);
    assert_eq!(moved.scatter, fresh.scatter);
    assert_eq!(moved.water, fresh.water);
    assert_ne!(moved.data.alphamap, first.data.alphamap);
    // Move a mountain: base and erosion are reused.
    req.stamps[0].at.center += Vec2::new(10.0, 0.0);
    let stamped = generate::run_cached(&req, &p, &mut cache).unwrap();
    assert_eq!(&stamped.reused[..], &generate::STAGES[..3]);
    let fresh = run(&req);
    assert_eq!(stamped.data, fresh.data);
    assert_eq!(stamped.scatter, fresh.scatter);
    // Change erosion: base is reused.
    req.settings.erosion.thermal_iterations += 3;
    let eroded = generate::run_cached(&req, &p, &mut cache).unwrap();
    assert_eq!(&eroded.reused[..], &generate::STAGES[..2]);
    assert_eq!(eroded.data, run(&req).data);
    // Overall progress ends at 1.
    assert_eq!(p.overall(), 1.0);
}

#[test]
fn progress_plan_covers_the_stages_that_run() {
    let p = Progress::default();
    p.set_plan(vec![("A", 1.0), ("B", 3.0)]);
    p.set("A", 0.5);
    assert!((p.overall() - 0.125).abs() < 1e-6);
    p.set("B", 0.5);
    assert!((p.overall() - 0.625).abs() < 1e-6);
    p.set("Done", 1.0);
    assert_eq!(p.overall(), 1.0);
}

/// 16-bin histogram of normalised heights (fractions).
fn histogram(d: &crate::terrain::TerrainData) -> [f32; 16] {
    let mut h = [0.0f32; 16];
    for v in &d.heights {
        h[((v * 16.0) as usize).min(15)] += 1.0;
    }
    let n = d.heights.len() as f32;
    h.map(|c| c / n)
}

#[test]
fn templates_generate_distinct_worlds_deterministically() {
    use super::templates::Template;
    let mut results = Vec::new();
    for t in Template::ALL {
        let req = t.request([600.0, 0.0, 600.0], 129);
        // Every biome the template uses is a preset (no missing-biome warnings).
        for st in &req.stamps {
            if let StampKind::BiomeZone(z) = &st.stamp.kind {
                assert!(
                    Preset::ALL.iter().any(|p| p.path() == z.biome),
                    "{t:?}: {}",
                    z.biome
                );
            }
        }
        assert_eq!(req.biomes.len(), t.presets().len(), "{t:?}");
        let a = run(&req);
        let b = run(&req);
        assert_eq!(a.data, b.data, "{t:?} is deterministic");
        assert_eq!(a.scatter, b.scatter, "{t:?}");
        let (lo, hi) = a.data.height_range();
        assert!(hi - lo > 0.05, "{t:?} has relief: {lo}..{hi}");
        assert!(!a.data.layers.is_empty(), "{t:?} is painted");
        let count: usize = a.scatter.iter().flatten().map(|b| b.instances.len()).sum();
        assert!(count > 0, "{t:?} scatters something");
        results.push((t, histogram(&a.data), a));
    }
    for (i, (ta, ha, a)) in results.iter().enumerate() {
        for (tb, hb, b) in results.iter().skip(i + 1) {
            let l1: f32 = ha.iter().zip(hb).map(|(x, y)| (x - y).abs()).sum();
            let metres = (a.data.size.y - b.data.size.y).abs();
            assert!(
                l1 > 0.3 || metres > 20.0 || a.data.layers != b.data.layers,
                "{ta:?} and {tb:?} look alike (histogram distance {l1})"
            );
        }
    }
    // Water where promised.
    let has = |t: Template, name: &str| {
        results
            .iter()
            .find(|r| r.0 == t)
            .is_some_and(|r| r.2.water.iter().any(|w| w.name == name))
    };
    assert!(has(Template::Islands, "Sea"));
    assert!(has(Template::Volcanic, "Sea"));
    assert!(has(Template::Tundra, "Lake"));
    assert!(has(Template::Forest, "River"));
}

#[test]
fn templates_keep_the_terrain_asset_and_scale_with_it() {
    use super::templates::Template;
    let current = WorldBuilder {
        terrain_data: "Assets/World.terrain".into(),
        size: [1200.0, 50.0, 1200.0],
        heightmap_resolution: 1025,
        ..Default::default()
    };
    let small = Template::Mountains.setup(&WorldBuilder::default());
    let big = Template::Mountains.setup(&current);
    assert_eq!(big.settings.terrain_data, current.terrain_data);
    assert_eq!(big.settings.heightmap_resolution, 1025);
    assert_eq!(big.settings.size[0], 1200.0);
    assert_eq!(
        big.settings.size[1], small.settings.size[1],
        "height comes from the template"
    );
    assert!(big.stamps[0].stamp.radius > small.stamps[0].stamp.radius * 1.9);
    for t in &big.stamps {
        assert!((0.0..=1200.0).contains(&t.position.x) && (0.0..=1200.0).contains(&t.position.z));
    }
}

/// Render the template picker's thumbnails into the editor's assets (`cargo test --release -p
/// misty_engine write_template_thumbnails -- --ignored`).
#[test]
#[ignore = "writes the editor's template thumbnails; run on purpose"]
fn write_template_thumbnails() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../misty_editor/assets/world_templates");
    std::fs::create_dir_all(&dir).unwrap();
    for t in super::templates::Template::ALL {
        t.thumbnail(96)
            .save(dir.join(format!("{}.png", t.label())))
            .unwrap();
    }
}
