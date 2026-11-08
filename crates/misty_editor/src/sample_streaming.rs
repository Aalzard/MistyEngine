//! Procedural StreamingDemo in `SampleProject`: a 4 × 4 km world streamed with World
//! Partition. Rolling meadow hills, a river valley and mountains to the north-east are 64
//! terrain tiles of 512 m (one per cell, stitched: no seams), forests of the sample trees
//! (`Assets/Trees`, terrain trees on each tile) and the sample boulders (`Assets/Rocks`, prefab
//! instances) lie in 8 × 8 cell sub-scenes under `Assets/Scenes/StreamingDemo/Cells`, with an
//! HLOD proxy each (`…/HLOD`) drawn while a cell is not loaded. The persistent scene holds the
//! camera (which flies a loop over the world in Play mode; cells stream in around it), the sun,
//! a global volume with haze and the World Partition. Everything is generated deterministically
//! and kept small (tiles are 65 × 65 samples, cells compact JSON, proxies a few kB).
//! Regenerate with `cargo test --release -p misty_editor -- --ignored write_streaming_demo`;
//! `streaming_demo_matches_its_generator` checks the committed files still match.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::{
    Light, PostProcessVolume, Scene, ScriptRef, ScriptValue, Scripts, Shadows, TonemapMode,
    Transform,
};
use misty_engine::terrain::auto_paint::AutoPaintRules;
use misty_engine::terrain::trees::{TreeInstance, TreePrototype};
use misty_engine::terrain::{MAX_LAYERS, TerrainData, TerrainLayer, noise};
use misty_engine::world_partition::{CellCoord, CellInfo, WorldPartition, hlod, tiles};

pub const SCENE: &str = "Assets/Scenes/StreamingDemo.scene";
pub const FOLDER: &str = "Assets/Scenes/StreamingDemo";
const SCRIPT: &str = "Assets/Scripts/StreamingFlyover.lua";
/// World side (metres) and cells per side.
const WORLD: f32 = 4096.0;
const CELLS: i32 = 8;
const CELL: f32 = WORLD / CELLS as f32;
const ORIGIN: Vec2 = Vec2::new(-WORLD * 0.5, -WORLD * 0.5);
/// Tile heightmap samples and splat texels per side.
const RES: usize = 65;
const ALPHA: usize = 32;
/// Highest terrain point the tiles can hold (metres above 0).
const MAX_H: f32 = 480.0;
const TREES: [&str; 4] = [
    "Assets/Trees/Oak.tree",
    "Assets/Trees/Birch.tree",
    "Assets/Trees/Linden.tree",
    "Assets/Trees/Young Oak.tree",
];
const ROCKS: [&str; 4] = [
    "Assets/Rocks/Prefabs/Boulder Large.prefab",
    "Assets/Rocks/Prefabs/Boulder Round.prefab",
    "Assets/Rocks/Prefabs/Boulder Split.prefab",
    "Assets/Rocks/Prefabs/Outcrop Layered.prefab",
];
/// Radius of the camera's loop in Play mode (metres).
const LOOP: f32 = 1400.0;

pub fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The river: its centre line's z at `x`.
fn river_z(x: f32) -> f32 {
    -350.0 + 420.0 * (x * 0.0011).sin() + 160.0 * (x * 0.0031 + 1.0).sin()
}

/// Ground height (metres) at a world point.
fn height(x: f32, z: f32) -> f32 {
    let hills = 70.0 + 55.0 * (noise::fbm(x, z, 900.0, 5, 3) * 0.5 + 0.5);
    let bumps = 6.0 * noise::fbm(x, z, 120.0, 3, 9);
    // Mountains rise to the north-east.
    let m = smoothstep(500.0, 1700.0, z * 0.8 + x * 0.35);
    let ridges = 1.0 - noise::fbm(x, z, 520.0, 5, 17).abs();
    let mountains = m * (90.0 + 260.0 * ridges * ridges);
    // A river valley winds across the middle.
    let d = (z - river_z(x)).abs();
    let valley = 38.0 * (1.0 - smoothstep(40.0, 260.0, d));
    (hills + bumps + mountains - valley).clamp(1.0, MAX_H - 1.0)
}

/// Slope (degrees) of the ground at a world point.
fn slope(x: f32, z: f32) -> f32 {
    let e = 4.0;
    let dx = (height(x + e, z) - height(x - e, z)) / (2.0 * e);
    let dz = (height(x, z + e) - height(x, z - e)) / (2.0 * e);
    (dx * dx + dz * dz).sqrt().atan().to_degrees()
}

/// 0..1 hash of an integer point.
fn hash(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (z as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

fn layer(name: &str, tex: &str, tile: f32) -> TerrainLayer {
    TerrainLayer {
        name: name.into(),
        diffuse: Some(tex.into()),
        tile_size: [tile, tile],
        rules: AutoPaintRules::default(),
        ..Default::default()
    }
}

fn template() -> TerrainData {
    let mut d = TerrainData::new(RES, Vec3::new(CELL, MAX_H, CELL), ALPHA);
    d.layers = vec![
        layer("Meadow", "Assets/Textures/Terrain/Meadow.png", 9.0),
        layer("Scree", "Assets/Textures/Terrain/Scree.png", 7.0),
        TerrainLayer {
            normal_map: Some("Assets/Textures/Terrain/CliffNormal.png".into()),
            smoothness: 0.2,
            ..layer("Cliff", "Assets/Textures/Terrain/Cliff.png", 18.0)
        },
        TerrainLayer {
            smoothness: 0.35,
            ..layer("Snow", "Assets/Textures/Terrain/Snow.png", 12.0)
        },
    ];
    d.trees.prototypes = TREES.map(|t| TreePrototype { tree: t.into() }).to_vec();
    // Trees beyond 1.4 km are the HLOD's job (their cell is unloaded by then anyway).
    d.trees.draw_distance = 1400.0;
    d.fit_splat_maps();
    d
}

/// Layer weights (meadow, scree, cliff, snow) at a world point, from slope, height and
/// world-space noise (seamless across tiles).
fn weights(x: f32, z: f32) -> [u8; MAX_LAYERS] {
    let s = slope(x, z);
    let h = height(x, z);
    let breakup = noise::fbm(x, z, 60.0, 3, 31) * 4.0;
    let cliff = smoothstep(31.0, 39.0, s + breakup);
    let scree = smoothstep(17.0, 26.0, s + breakup) * (1.0 - cliff);
    let snow = smoothstep(330.0, 370.0, h + breakup * 6.0) * (1.0 - cliff * 0.6);
    let meadow = (1.0 - cliff - scree).max(0.0) * (1.0 - snow);
    let raw = [
        meadow,
        scree * (1.0 - snow),
        cliff * (1.0 - snow * 0.4),
        snow,
    ];
    let total: f32 = raw.iter().sum::<f32>().max(1e-4);
    let mut w = [0u8; MAX_LAYERS];
    let mut sum = 0u32;
    for i in 0..4 {
        w[i] = (raw[i] / total * 255.0).round() as u8;
        sum += w[i] as u32;
    }
    // Exactly 255 (the rounding goes to the strongest layer).
    let top = (0..4)
        .max_by(|a, b| raw[*a].total_cmp(&raw[*b]))
        .unwrap_or(0);
    w[top] = (w[top] as i32 + 255 - sum as i32).clamp(0, 255) as u8;
    w
}

/// Every tree of the world: a jittered 20 m grid kept where the forest mask, slope and height
/// allow. (world position, instance without its tile position).
fn trees() -> Vec<(Vec2, TreeInstance)> {
    let step = 20.0;
    let n = (WORLD / step) as i32;
    let mut out = Vec::new();
    for gz in 0..n {
        for gx in 0..n {
            let jitter = Vec2::new(hash(gx, gz, 1), hash(gx, gz, 2)) - 0.5;
            let p = ORIGIN + (Vec2::new(gx as f32, gz as f32) + 0.5 + jitter * 0.8) * step;
            let forest =
                noise::fbm(p.x, p.y, 420.0, 4, 21) + 0.25 * noise::fbm(p.x, p.y, 70.0, 2, 5);
            if forest < 0.18 || hash(gx, gz, 3) > 0.85 {
                continue;
            }
            let h = height(p.x, p.y);
            if slope(p.x, p.y) > 26.0 || h > 300.0 || (p.y - river_z(p.x)).abs() < 70.0 {
                continue;
            }
            let pick = hash(gx, gz, 4);
            let prototype = if h > 220.0 {
                1 // birch up the slopes
            } else {
                (pick * 4.0) as u16 % 4
            };
            let size = 0.8 + 0.45 * hash(gx, gz, 5);
            out.push((
                p,
                TreeInstance {
                    position: Vec2::ZERO,
                    prototype,
                    rotation: hash(gx, gz, 6) * std::f32::consts::TAU,
                    height_scale: size,
                    width_scale: size * (0.9 + 0.2 * hash(gx, gz, 7)),
                    color: hash(gx, gz, 8),
                },
            ));
        }
    }
    out
}

fn all_coords() -> Vec<CellCoord> {
    (0..CELLS)
        .flat_map(|z| (0..CELLS).map(move |x| (x, z)))
        .collect()
}

/// The 64 terrain tiles, painted and planted.
pub fn terrain_tiles() -> Vec<tiles::Tile> {
    let mut list = tiles::from_fn(&template(), ORIGIN, 0.0, CELL, RES, &all_coords(), height);
    tiles::stitch(&mut list);
    let forest = trees();
    for t in &mut list {
        let a = t.data.alphamap_resolution;
        for z in 0..a {
            for x in 0..a {
                let p = Vec2::new(t.corner.x, t.corner.z)
                    + Vec2::new(x as f32 + 0.5, z as f32 + 0.5) / a as f32 * CELL;
                let w = weights(p.x, p.y);
                t.data.set_layer_weights(z * a + x, &w);
            }
        }
        let lo = Vec2::new(t.corner.x, t.corner.z);
        t.data.trees.instances = forest
            .iter()
            .filter(|(p, _)| p.x >= lo.x && p.y >= lo.y && p.x < lo.x + CELL && p.y < lo.y + CELL)
            .map(|(p, i)| TreeInstance {
                position: (*p - lo) / CELL,
                ..*i
            })
            .collect();
    }
    list
}

fn partition() -> WorldPartition {
    WorldPartition {
        cell_size: CELL,
        origin: ORIGIN.to_array(),
        folder: FOLDER.into(),
        load_radius: 800.0,
        unload_radius: 1000.0,
        hlod_distance: 0.0,
        budget_ms: 2.0,
        max_loads: 4,
        cells: all_coords()
            .into_iter()
            .map(|(x, z)| CellInfo {
                x,
                z,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// Boulders of one cell (prefab, position, yaw, scale), on slopes and in the open.
fn rocks(c: CellCoord) -> Vec<(usize, Vec3, f32, f32)> {
    let lo = ORIGIN + Vec2::new(c.0 as f32, c.1 as f32) * CELL;
    let mut out = Vec::new();
    for k in 0..14 {
        let q = Vec2::new(hash(c.0 * 31 + k, c.1, 11), hash(c.0, c.1 * 29 + k, 12));
        let p = lo + Vec2::new(16.0, 16.0) + q * (CELL - 32.0);
        let s = slope(p.x, p.y);
        let h = height(p.x, p.y);
        if (p.y - river_z(p.x)).abs() < 40.0 || h > 360.0 {
            continue;
        }
        // Outcrops on steep ground, boulders elsewhere; fewer in flat meadow.
        let kind = if s > 22.0 {
            3
        } else if hash(k, c.0 + c.1 * 8, 13) < 0.45 + s / 40.0 {
            (hash(k, c.0 * 8 + c.1, 14) * 3.0) as usize % 3
        } else {
            continue;
        };
        let scale = if kind == 3 { 1.2 } else { 0.9 } + 0.6 * hash(k, c.0 - c.1, 15);
        let yaw = hash(k, c.1 - c.0, 16) * 360.0;
        out.push((kind, Vec3::new(p.x, h - 0.35 * scale, p.y), yaw, scale));
    }
    out
}

/// The sub-scene of one cell: its terrain tile and boulders.
pub fn cell_scene(c: CellCoord, prefabs: &[Prefab]) -> Scene {
    let wp = partition();
    let mut s = Scene::new(&misty_engine::world_partition::cell_name(c));
    let corner = Vec3::new(
        ORIGIN.x + c.0 as f32 * CELL,
        0.0,
        ORIGIN.y + c.1 as f32 * CELL,
    );
    let stub = tiles::Tile {
        coord: c,
        corner,
        data: TerrainData::new(33, Vec3::ONE, 16),
    };
    tiles::spawn_tiles(&mut s, &wp, std::slice::from_ref(&stub));
    for (kind, p, yaw, scale) in rocks(c) {
        let e = s.instantiate_prefab(&prefabs[kind], Some(ROCKS[kind]), None);
        let mut t = Transform::from_position(p).with_euler(Vec3::new(0.0, yaw, 0.0));
        t.scale = Vec3::splat(scale);
        s.set_transform(e, t);
    }
    s
}

fn load_prefabs(root: &Path) -> Vec<Prefab> {
    ROCKS
        .iter()
        .map(|r| Prefab::load(&root.join(r)).expect("rock prefab"))
        .collect()
}

/// Altitude of the camera's loop: clear of the highest ground under it.
fn flight_height() -> f32 {
    let top = (0..360)
        .map(|i| {
            let a = (i as f32).to_radians();
            let p = Vec2::new(a.cos(), a.sin() * 0.8) * LOOP;
            height(p.x, p.y)
        })
        .fold(0.0f32, f32::max);
    (top + 90.0).round()
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.3;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 6000.0;
    f.attenuation_distance = 5200.0;
    f.base_height = 0.0;
    f.maximum_height = 600.0;
    f.anisotropy = 0.3;
    v
}

/// The persistent scene: camera, sun, haze and the World Partition (cells with proxies).
pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("StreamingDemo");
    let start = Vec2::new(LOOP, 0.0);
    let alt = flight_height();
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(start.x, alt, start.y))
                .with_euler(Vec3::new(14.0, -30.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.far = 6000.0;
        c.near = 0.3;
        c.fov = 60.0;
        drop(c);
        let mut fields = BTreeMap::new();
        fields.insert("radius".to_owned(), ScriptValue::Number(LOOP as f64));
        fields.insert("height".to_owned(), ScriptValue::Number(alt as f64));
        s.add(
            cam,
            Scripts {
                list: vec![ScriptRef {
                    path: SCRIPT.into(),
                    fields,
                    ..Default::default()
                }],
            },
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 600.0, 0.0))
                .with_euler(Vec3::new(35.0, 140.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.94, 0.84];
        l.intensity = 2.2;
        l.shadows = Shadows::Soft;
    }
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    let wp = s.spawn("World Partition", None);
    let mut part = partition();
    for c in &mut part.cells {
        c.hlod = true;
    }
    s.add(wp, part);
    s
}

const FLYOVER: &str = r#"-- StreamingDemo: the camera flies a loop over the 4 km world. Cells stream in around it
-- (the main camera is the World Partition's streaming source); unloaded cells show their HLOD.
local F = {
    radius = 1400,  -- metres from the world's centre
    height = 300,   -- altitude of the loop
    speed = 70,     -- metres per second
}

function F:Start()
    self.angle = 0
    self.ready = false
    self.transform.position = self:at(0)
    -- Wait until the cells around the start are in.
    WorldPartition.OnAreaLoaded(self:at(0), 400, function()
        self.ready = true
        Debug.Log("StreamingDemo: start area loaded (" .. WorldPartition.loadedCells .. " cells)")
    end)
end

function F:at(angle)
    return Vector3.new(math.cos(angle) * self.radius, self.height, math.sin(angle) * self.radius * 0.8)
end

function F:Update()
    if not self.ready then return end
    self.angle = self.angle + Time.deltaTime * self.speed / self.radius
    local here = self:at(self.angle)
    local ahead = self:at(self.angle + 0.08)
    self.transform.position = here
    self.transform:LookAt(Vector3.new(ahead.x, ahead.y - 110, ahead.z))
end

return F
"#;

/// Deterministic `.meta` next to `path` (so opening the project adds none).
fn write_meta(path: &Path, rel: &str) {
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for b in format!("streaming-demo:{rel}").bytes() {
        h ^= b as u128;
        h = h.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    let guid = misty_engine::asset_db::Guid(h);
    let text = misty_engine::asset_db::meta::compose(guid, &Default::default());
    std::fs::write(misty_engine::asset_db::meta::meta_path(path), text).unwrap();
}

fn write_all(root: &Path) {
    misty_engine::assets::set_project_root(root);
    let base = root.join(FOLDER);
    for dir in ["", "/Cells", "/Terrain", "/HLOD"] {
        let p = PathBuf::from(format!("{}{dir}", base.display()));
        std::fs::create_dir_all(&p).unwrap();
        write_meta(&p, &format!("{FOLDER}{dir}"));
    }
    let wp = partition();
    for t in terrain_tiles() {
        let rel = tiles::tile_path(&wp, t.coord);
        t.data.save(&root.join(&rel)).unwrap();
        write_meta(&root.join(&rel), &rel);
        misty_engine::terrain::store::insert_loaded(&rel, t.data);
    }
    let prefabs = load_prefabs(root);
    let mut infos = Vec::new();
    for c in all_coords() {
        let s = cell_scene(c, &prefabs);
        let rel = wp.cell_path(c);
        let roots = s.roots().to_vec();
        std::fs::write(root.join(&rel), s.subtrees_json(&roots, true)).unwrap();
        write_meta(&root.join(&rel), &rel);
        let objects = roots
            .iter()
            .map(|r| {
                s.walk()
                    .iter()
                    .filter(|(e, _)| s.is_ancestor(*r, *e))
                    .count() as u32
            })
            .sum();
        let (lo, hi) = cell_heights(c);
        infos.push(CellInfo {
            x: c.0,
            z: c.1,
            objects,
            height: [lo, hi],
            hlod: true,
        });
    }
    std::fs::write(root.join(SCRIPT), FLYOVER).unwrap();
    write_meta(&root.join(SCRIPT), SCRIPT);
    let mut scene = demo_scene();
    let part = misty_engine::world_partition::find(&scene).unwrap().0;
    scene.world.get::<&mut WorldPartition>(part).unwrap().cells = infos;
    let report = hlod::build_all(&mut scene, part, root).unwrap();
    eprintln!(
        "HLODs: {} cells, {} triangles, {} kB",
        report.cells,
        report.triangles,
        report.bytes / 1024
    );
    for f in ["HLOD.mat", "HLOD.shader"] {
        let p = base.join("HLOD").join(f);
        write_meta(&p, &format!("{FOLDER}/HLOD/{f}"));
    }
    for c in all_coords() {
        let rel = wp.hlod_path(c);
        if root.join(&rel).exists() {
            write_meta(&root.join(&rel), &rel);
        }
    }
    scene.save(&root.join(SCENE)).unwrap();
    write_meta(&root.join(SCENE), SCENE);
}

/// Lowest and highest ground of a cell (sampled).
fn cell_heights(c: CellCoord) -> (f32, f32) {
    let lo = ORIGIN + Vec2::new(c.0 as f32, c.1 as f32) * CELL;
    let mut range = (f32::MAX, f32::MIN);
    for i in 0..=16 {
        for j in 0..=16 {
            let h = height(lo.x + i as f32 * CELL / 16.0, lo.y + j as f32 * CELL / 16.0);
            range = (range.0.min(h), range.1.max(h));
        }
    }
    (range.0.round(), (range.1 + 30.0).round())
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_streaming_demo() {
    write_all(&project());
}

#[test]
fn streaming_demo_matches_its_generator() {
    let root = project();
    let wp = partition();
    // Tiles: heights, borders, paint and trees.
    let generated = terrain_tiles();
    assert_eq!(generated.len(), 64);
    let mut trees = 0;
    for t in &generated {
        let on_disk = TerrainData::load(&root.join(tiles::tile_path(&wp, t.coord))).unwrap();
        assert_eq!(on_disk.heightmap_resolution, RES);
        assert_eq!(on_disk.layers, t.data.layers);
        for (a, b) in on_disk.heights.iter().zip(&t.data.heights) {
            assert!((a - b).abs() < 2.0 / 65535.0, "{:?} heights", t.coord);
        }
        assert_eq!(on_disk.border.len(), 4 * RES);
        for (a, b) in on_disk.border.iter().zip(&t.data.border) {
            assert!((a - b).abs() < 2.0 / 65535.0, "{:?} border", t.coord);
        }
        let close = (0..on_disk.alphamap.len())
            .filter(|i| {
                let (x, y) = (on_disk.layer_weights(*i), t.data.layer_weights(*i));
                x.iter().zip(&y).all(|(a, b)| a.abs_diff(*b) <= 2)
            })
            .count();
        assert!(
            close as f32 >= on_disk.alphamap.len() as f32 * 0.999,
            "{:?} paint",
            t.coord
        );
        assert_eq!(on_disk.trees.instances.len(), t.data.trees.instances.len());
        trees += on_disk.trees.instances.len();
    }
    assert!(trees > 5000, "{trees} trees");
    // Neighbouring tiles meet without seams.
    let at = |c: CellCoord| generated.iter().find(|t| t.coord == c).unwrap();
    for c in all_coords() {
        for n in [(c.0 + 1, c.1), (c.0, c.1 + 1)] {
            if n.0 < CELLS && n.1 < CELLS {
                let (h, angle) = tiles::seam_error(at(c), at(n));
                assert!(h < 1e-3 && angle < 0.05, "{c:?}-{n:?}: {h} m, {angle}°");
            }
        }
    }
    // Cells: the tile and the boulders, in place.
    let prefabs = load_prefabs(&root);
    let mut rocks = 0;
    for c in all_coords() {
        let text = std::fs::read_to_string(root.join(wp.cell_path(c))).unwrap();
        let (on_disk, report) = Scene::from_json("Cell", &text).unwrap();
        assert!(report.warnings.is_empty(), "{c:?}: {:?}", report.warnings);
        let generated_cell = cell_scene(c, &prefabs);
        let names = |s: &Scene| s.roots().iter().map(|e| s.name(*e)).collect::<Vec<_>>();
        assert_eq!(names(&on_disk), names(&generated_cell), "{c:?}");
        for (a, b) in on_disk.roots().iter().zip(generated_cell.roots()) {
            let (ta, tb) = (on_disk.transform(*a), generated_cell.transform(*b));
            assert!(
                ta.position.distance(tb.position) < 1e-2,
                "{c:?} {}",
                on_disk.name(*a)
            );
        }
        rocks += on_disk.roots().len() - 1;
        // Its proxy.
        let proxy = std::fs::read_to_string(root.join(wp.hlod_path(c))).unwrap();
        let mesh = hlod::HlodMesh::from_file_str(&proxy).unwrap();
        assert!(mesh.triangles() > 100, "{c:?} proxy");
        assert!(
            proxy.len() < 64 * 1024,
            "{c:?} proxy is {} bytes",
            proxy.len()
        );
    }
    assert!(rocks > 150, "{rocks} boulders");
    // The persistent scene: the partition lists every cell with a proxy.
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let (_, part) = misty_engine::world_partition::find(&scene).unwrap();
    assert_eq!(part.cells.len(), 64);
    assert!(part.cells.iter().all(|c| c.hlod && c.objects > 0));
    assert_eq!(part.cell_size, CELL);
    assert_eq!(part.origin, ORIGIN.to_array());
    assert_eq!(part.folder, FOLDER);
    // Only the camera, sun, volume and partition stay loaded.
    assert_eq!(scene.roots().len(), 4);
    assert_eq!(std::fs::read_to_string(root.join(SCRIPT)).unwrap(), FLYOVER);
}
