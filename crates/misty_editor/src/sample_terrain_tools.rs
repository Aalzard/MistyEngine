//! Procedural TerrainToolsDemo scene in `SampleProject`: the round 20 terrain tools on one
//! 160 m terrain. Seven layers (two splat maps: meadow, scree, cliff and snow by Auto Paint
//! Rules, then a sand path, lush grass and rocky ground painted by hand), a sinkhole cut with
//! Paint Holes over a small pit, the terrain grass plus two grass species and a pebble detail
//! mesh painted with Paint Details, trees placed with height limits, and rock prefabs placed
//! with Place Objects around the hole.
//! Regenerate with `cargo test --release -p misty_editor -- --ignored write_terrain_tools_demo`;
//! `terrain_tools_demo_matches_its_generator` checks the committed files still match.

use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::{
    MeshFilter, MeshRef, MeshRenderer, PrimitiveMesh, Scene, Terrain, TerrainCollider, Transform,
};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::brush::{self, Brush};
use misty_engine::terrain::detail_layers::{self, DetailLayer, DetailTarget};
use misty_engine::terrain::placement::{self, ObjectPlacement};
use misty_engine::terrain::trees::{self, Placement, TreePrototype};
use misty_engine::terrain::{TerrainData, TerrainLayer, details, noise};

const DATA: &str = "Assets/Terrains/TerrainToolsDemo.terrain";
const SCENE: &str = "Assets/Scenes/TerrainToolsDemo.scene";
const SIZE: Vec3 = Vec3::new(160.0, 60.0, 160.0);
const CORNER: Vec3 = Vec3::new(-80.0, 0.0, -80.0);
/// The sinkhole (terrain-local centre, radius).
const HOLE: (Vec2, f32) = (Vec2::new(86.0, 58.0), 4.5);
const ROCKS: [&str; 2] = [
    "Assets/Rocks/Prefabs/Boulder Round.prefab",
    "Assets/Rocks/Prefabs/Rock Small.prefab",
];
const PIT: &str = "Assets/Materials/Dirt.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Rolling meadow rising to a ridge in the north.
fn height(x: f32, z: f32) -> f32 {
    let hills = 4.0 + 5.0 * (noise::fbm(x, z, 45.0, 4, 11) * 0.5 + 0.5);
    let t = ((z - 95.0) / 50.0).clamp(0.0, 1.0);
    let ridge = t * t * (3.0 - 2.0 * t) * (32.0 + 14.0 * noise::fbm(x, z, 30.0, 3, 5));
    (hills + ridge) / SIZE.y
}

fn rules(slope: [f32; 2], priority: i32) -> AutoPaintRules {
    AutoPaintRules {
        enabled: true,
        slope,
        spread: 0.25,
        breakup: 0.6,
        breakup_scale: 12.0,
        priority,
        ..Default::default()
    }
}

fn layer(name: &str, tex: &str, tile: f32) -> TerrainLayer {
    TerrainLayer {
        name: name.into(),
        diffuse: Some(tex.into()),
        tile_size: [tile, tile],
        ..Default::default()
    }
}

fn layers() -> Vec<TerrainLayer> {
    vec![
        TerrainLayer {
            rules: rules([0.0, 22.0], 0),
            ..layer("Meadow", "Assets/Textures/Terrain/Meadow.png", 7.0)
        },
        TerrainLayer {
            rules: rules([20.0, 36.0], 1),
            ..layer("Scree", "Assets/Textures/Terrain/Scree.png", 5.0)
        },
        TerrainLayer {
            normal_map: Some("Assets/Textures/Terrain/CliffNormal.png".into()),
            height_map: Some("Assets/Textures/Terrain/CliffHeight.png".into()),
            smoothness: 0.2,
            rules: rules([34.0, 90.0], 2),
            ..layer("Cliff", "Assets/Textures/Terrain/Cliff.png", 14.0)
        },
        TerrainLayer {
            smoothness: 0.35,
            rules: AutoPaintRules {
                use_height: true,
                height: [44.0, 60.0],
                ..rules([0.0, 48.0], 3)
            },
            ..layer("Snow", "Assets/Textures/Terrain/Snow.png", 9.0)
        },
        // Layers 5–7: the second splat map, painted by hand.
        layer("Sand Path", "Assets/Textures/TerrainSand.png", 4.0),
        layer("Lush Grass", "Assets/Textures/TerrainGrass.png", 5.0),
        TerrainLayer {
            normal_map: Some("Assets/Textures/TerrainRockNormal.png".into()),
            ..layer("Rocky Ground", "Assets/Textures/TerrainRock.png", 6.0)
        },
    ]
}

/// The winding sand path (terrain-local points).
fn path_points() -> Vec<Vec2> {
    (0..=60)
        .map(|i| {
            let t = i as f32 / 60.0;
            Vec2::new(70.0 + 22.0 * (t * 5.0).sin(), 4.0 + t * 100.0)
        })
        .collect()
}

fn stroke(d: &mut TerrainData, at: Vec2, size: f32, f: impl Fn(&mut TerrainData, &Brush)) {
    let b = Brush {
        center: at,
        size,
        strength: 1.0,
        falloff: 0.6,
    };
    f(d, &b);
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(257, SIZE, 256);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, height(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = layers();
    d.fit_splat_maps();
    d.rule_seed = 3;
    auto_paint::generate(&mut d);
    // Hand-painted layers of the second splat map.
    for p in path_points() {
        stroke(&mut d, p, 5.0, |d, b| brush::paint(d, b, 1.0, 4));
    }
    for (c, r) in [
        (Vec2::new(40.0, 45.0), 18.0),
        (Vec2::new(120.0, 30.0), 14.0),
    ] {
        stroke(&mut d, c, r, |d, b| brush::paint(d, b, 0.6, 5));
    }
    stroke(&mut d, HOLE.0, HOLE.1 * 4.0, |d, b| {
        brush::paint(d, b, 1.0, 6)
    });
    // The sinkhole.
    stroke(&mut d, HOLE.0, HOLE.1 * 2.0, |d, b| {
        brush::paint_holes(d, b, false);
    });
    // The terrain grass on the meadow, plus two species and pebbles painted on.
    d.details.enabled = true;
    d.details.on_layer = Some(0);
    d.details.density = 8.0;
    d.details.draw_distance = 90.0;
    d.details.rules.slope = [0.0, 24.0];
    details::generate(&mut d);
    let mut tall = DetailLayer::grass("Tall Dry Grass");
    tall.height = [0.7, 1.1];
    tall.width = 0.03;
    tall.dryness = 0.85;
    tall.seed_heads = 0.25;
    tall.density = 30.0;
    let mut lawn = DetailLayer::grass("Short Lawn");
    lawn.height = [0.1, 0.18];
    lawn.width = 0.03;
    lawn.dryness = 0.05;
    lawn.tint = 0.2;
    lawn.density = 120.0;
    let mut pebbles = DetailLayer::mesh(
        "Pebbles",
        MeshRef::asset("Assets/Rocks/Meshes/Rock Small LODs.glb", 2),
        Some("Assets/Rocks/Materials/Rock Small.mat".into()),
    );
    pebbles.scale = [0.15, 0.4];
    pebbles.density = 0.8;
    pebbles.draw_distance = 45.0;
    d.detail_layers = vec![tall, lawn, pebbles];
    for (c, r) in [
        (Vec2::new(55.0, 22.0), 22.0),
        (Vec2::new(104.0, 46.0), 16.0),
    ] {
        stroke(&mut d, c, r, |d, b| {
            detail_layers::paint(d, b, 1.0, DetailTarget::Layer(0), 1.0, false);
        });
    }
    for (c, r) in [
        (Vec2::new(40.0, 45.0), 16.0),
        (Vec2::new(120.0, 30.0), 12.0),
    ] {
        stroke(&mut d, c, r, |d, b| {
            detail_layers::paint(d, b, 1.0, DetailTarget::Layer(1), 1.0, false);
        });
    }
    for p in path_points() {
        stroke(&mut d, p, 6.0, |d, b| {
            detail_layers::paint(d, b, 1.0, DetailTarget::Layer(2), 0.8, false);
        });
    }
    // Trees on the lower slopes only (height limits), clear of the path's start.
    d.trees.prototypes = ["Assets/Trees/Oak.tree", "Assets/Trees/Birch.tree"]
        .map(|t| TreePrototype { tree: t.into() })
        .to_vec();
    let p = Placement {
        spacing: 9.0,
        altitude: [6.0, 30.0],
        max_slope: 28.0,
        seed: 5,
        ..Default::default()
    };
    trees::mass_place(&mut d, 70, &[0, 1], &p);
    d.trees
        .instances
        .retain(|t| (t.position * 160.0).y > 30.0 || (t.position.x * 160.0 - 75.0).abs() > 30.0);
    d
}

// ---- scene -------------------------------------------------------------------------------

fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    CORNER.y + d.sample_height(x - CORNER.x, z - CORNER.z)
}

fn demo_scene(d: &TerrainData, root: &Path) -> Scene {
    let mut s = Scene::default_3d("TerrainToolsDemo");
    let eye = (0.0, -74.0);
    if let Some(cam) = s.find_by_name("Main Camera") {
        let y = ground(d, eye.0, eye.1) + 9.0;
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(eye.0, y, eye.1))
                .with_euler(Vec3::new(12.0, 0.0, 0.0)),
        );
    }
    if let Some(light) = s.find_by_name("Directional Light") {
        s.set_transform(
            light,
            Transform::from_position(Vec3::new(0.0, 60.0, 0.0))
                .with_euler(Vec3::new(38.0, -40.0, 0.0)),
        );
    }
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
    // A pit under the sinkhole: floor and walls.
    let c = CORNER + Vec3::new(HOLE.0.x, 0.0, HOLE.0.y);
    let top = ground(d, c.x, c.z);
    let pit = s.spawn("Pit", None);
    let r = HOLE.1 + 1.5;
    for (name, pos, scale) in [
        (
            "Floor",
            Vec3::new(0.0, -7.0, 0.0),
            Vec3::new(2.0 * r, 1.0, 2.0 * r),
        ),
        (
            "Wall N",
            Vec3::new(0.0, -3.5, r),
            Vec3::new(2.0 * r, 8.0, 0.5),
        ),
        (
            "Wall S",
            Vec3::new(0.0, -3.5, -r),
            Vec3::new(2.0 * r, 8.0, 0.5),
        ),
        (
            "Wall E",
            Vec3::new(r, -3.5, 0.0),
            Vec3::new(0.5, 8.0, 2.0 * r),
        ),
        (
            "Wall W",
            Vec3::new(-r, -3.5, 0.0),
            Vec3::new(0.5, 8.0, 2.0 * r),
        ),
    ] {
        let e = s.spawn(name, Some(pit));
        let mut t = Transform::from_position(Vec3::new(c.x, top, c.z) + pos);
        t.scale = scale;
        s.set_transform(e, t);
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(
            e,
            MeshRenderer {
                material: Some(PIT.into()),
                ..Default::default()
            },
        );
    }
    // Rocks placed with Place Objects around the sinkhole and on the slopes.
    for (i, rel) in ROCKS.iter().enumerate() {
        let Ok(prefab) = Prefab::load(&root.join(rel)) else {
            continue;
        };
        let p = ObjectPlacement {
            density: if i == 0 { 1.2 } else { 4.0 },
            spacing: if i == 0 { 6.0 } else { 2.0 },
            scale: if i == 0 { [0.5, 0.9] } else { [0.6, 1.3] },
            slope: [0.0, 35.0],
            seed: 9 + i as u32,
            ..Default::default()
        };
        for step in 0..40 {
            placement::place(&mut s, terrain, d, &prefab, rel, HOLE.0, 18.0, &p, step);
        }
    }
    s
}

fn write_all(root: &Path, d: &TerrainData) {
    d.save(&root.join(DATA)).unwrap();
    misty_engine::terrain::store::insert(&root.join(DATA).to_string_lossy(), d.clone());
    demo_scene(d, root).save(&root.join(SCENE)).unwrap();
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_terrain_tools_demo() {
    let root = project();
    misty_engine::assets::set_project_root(&root);
    write_all(&root, &terrain_data());
}

#[test]
fn terrain_tools_demo_matches_its_generator() {
    let root = project();
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.layers.len(), 7, "two splat maps");
    assert_eq!(on_disk.splat_maps(), 2);
    assert_eq!(on_disk.holes, generated.holes);
    assert!(on_disk.has_holes());
    assert_eq!(on_disk.details, generated.details);
    assert_eq!(on_disk.detail_layers.len(), 3);
    for (a, b) in on_disk.detail_layers.iter().zip(&generated.detail_layers) {
        assert_eq!(a.settings(), b.settings());
        let close = a
            .map
            .iter()
            .zip(&b.map)
            .filter(|(x, y)| x.abs_diff(**y) <= 2)
            .count();
        assert!(close as f32 >= a.map.len() as f32 * 0.999, "{} map", a.name);
    }
    assert_eq!(
        on_disk.trees.instances.len(),
        generated.trees.instances.len()
    );
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let all = |d: &TerrainData| -> Vec<[u8; 16]> {
        (0..d.alphamap.len()).map(|t| d.layer_weights(t)).collect()
    };
    let close = all(&on_disk)
        .iter()
        .zip(&all(&generated))
        .filter(|(a, b)| a.iter().zip(*b).all(|(x, y)| x.abs_diff(*y) <= 3))
        .count();
    assert!(
        close as f32 > on_disk.alphamap.len() as f32 * 0.999,
        "splat maps match"
    );
    // The second map holds the hand-painted layers.
    let path = on_disk.weights_at(HOLE.0.x + 5.5, HOLE.0.y);
    assert!(path[6] > 0.5, "rocky ground around the hole: {path:?}");
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let terrain = scene.find_by_name("Terrain").unwrap();
    let rocks = placement::placed(&scene, terrain);
    assert!(rocks.len() > 10, "{} rocks placed", rocks.len());
    assert!(
        rocks.iter().all(|r| !on_disk.hole_at(r.x, r.y)),
        "no rock over the hole"
    );
}
