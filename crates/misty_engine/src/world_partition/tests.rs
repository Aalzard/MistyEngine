//! World Partition: grid, Convert, streaming with hysteresis and a frame budget, terrain tiles,
//! HLOD proxies and the Lua API. Every test works in its own temporary folder with absolute
//! paths (tests beside it move the process-wide project root).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use hecs::Entity;

use super::streamer::NoHooks;
use super::*;
use crate::render::MaterialParams;
use crate::render_scene::MaterialSource;
use crate::scene::{
    MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, Scripts, Terrain, Transform,
};
use crate::terrain::TerrainData;

fn temp_root(name: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_wp_{name}_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    root
}

fn folder(root: &Path) -> String {
    root.join("Assets/World")
        .to_string_lossy()
        .replace('\\', "/")
}

/// A 200 m square world: 4 × 4 cells of 50 m with three crates each, the default camera and
/// light, the partition object and an Always Loaded sky dome far away.
fn world(root: &Path) -> (Scene, Entity) {
    let mut s = Scene::default_3d("World");
    let wp = s.spawn("World Partition", None);
    s.add(
        wp,
        WorldPartition {
            folder: folder(root),
            cell_size: 50.0,
            load_radius: 60.0,
            unload_radius: 90.0,
            ..Default::default()
        },
    );
    for cx in 0..4 {
        for cz in 0..4 {
            for k in 0..3 {
                let e = s.spawn(&format!("Crate {cx} {cz} {k}"), None);
                let p = Vec3::new(
                    cx as f32 * 50.0 + 10.0 + k as f32 * 12.0,
                    0.5,
                    cz as f32 * 50.0 + 25.0,
                );
                let mut t = Transform::from_position(p);
                t.scale = Vec3::splat(2.0);
                s.set_transform(e, t);
                s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
                s.add(e, MeshRenderer::default());
            }
        }
    }
    let dome = s.spawn("Sky Dome", None);
    s.set_transform(dome, Transform::from_position(Vec3::new(5000.0, 0.0, 0.0)));
    s.add(dome, AlwaysLoaded {});
    (s, wp)
}

fn player(s: &mut Scene, at: Vec3) -> Entity {
    let p = s.spawn("Player", None);
    s.set_transform(p, Transform::from_position(at));
    s.add(p, StreamingSource::default());
    p
}

fn loaded(st: &Streamer) -> Vec<CellCoord> {
    st.loaded_cells()
}

#[test]
fn grid_cells_paths_and_hysteresis() {
    let wp = WorldPartition {
        cell_size: 100.0,
        origin: [-200.0, -200.0],
        folder: "Assets/W".into(),
        load_radius: 100.0,
        unload_radius: 150.0,
        ..Default::default()
    };
    assert_eq!(wp.cell_of(Vec3::new(-200.0, 5.0, -200.0)), (0, 0));
    assert_eq!(wp.cell_of(Vec3::new(-201.0, 0.0, 99.0)), (-1, 2));
    assert_eq!(
        wp.cell_rect((1, 2)),
        (Vec2::new(-100.0, 0.0), Vec2::new(0.0, 100.0))
    );
    assert_eq!(
        wp.distance_to_cell(Vec3::new(-50.0, 9.0, 50.0), (1, 2)),
        0.0
    );
    assert_eq!(
        wp.distance_to_cell(Vec3::new(30.0, 0.0, 50.0), (1, 2)),
        30.0
    );
    assert_eq!(wp.cell_path((3, -2)), "Assets/W/Cells/Cell_3_-2.scene");
    assert_eq!(cell_of_path(&wp.cell_path((3, -2))), Some((3, -2)));
    assert_eq!(cell_of_path("Assets/Level.scene"), None);
    let at = |x: f32| Source {
        name: "s".into(),
        position: Vec3::new(x, 0.0, 50.0),
        load_radius: 100.0,
        unload_radius: 150.0,
    };
    // Cell (1, 2) spans x -100..0: loads within 100 m, kept up to 150 m, then unloads.
    assert_eq!(wanted(&wp, &[at(90.0)], (1, 2)), Some(true));
    assert_eq!(wanted(&wp, &[at(120.0)], (1, 2)), Some(false));
    assert_eq!(wanted(&wp, &[at(160.0)], (1, 2)), None);
    assert_eq!(wanted(&wp, &[at(160.0), at(10.0)], (1, 2)), Some(true));
}

#[test]
fn convert_splits_objects_into_cell_scenes_and_keeps_the_rest() {
    let root = temp_root("convert");
    let (mut s, wp) = world(&root);
    let report = convert::convert(&mut s, wp, &root).unwrap();
    assert_eq!(report.cells, 16);
    assert_eq!(report.moved, 48);
    assert_eq!(report.kept, 4, "camera, light, partition, sky dome");
    assert!(s.find_by_name("Crate 0 0 0").is_none());
    assert!(s.find_by_name("Sky Dome").is_some());
    let part = (*s.world.get::<&WorldPartition>(wp).unwrap()).clone();
    assert_eq!(part.cells.len(), 16);
    assert!(part.cells.iter().all(|c| c.objects == 3));
    let cell = part.cell((1, 2)).unwrap();
    assert!(cell.height[1] >= 1.4, "{:?}", cell.height);
    let text = std::fs::read_to_string(root.join(part.cell_path((1, 2)))).unwrap();
    assert!(!text.contains('\n'), "cells are written compact");
    let (cell_scene, report) = Scene::from_json("Cell", &text).unwrap();
    assert!(report.warnings.is_empty());
    let names: Vec<String> = cell_scene
        .roots()
        .iter()
        .map(|e| cell_scene.name(*e))
        .collect();
    assert_eq!(names, ["Crate 1 2 0", "Crate 1 2 1", "Crate 1 2 2"]);

    // Converting again with bigger cells reads the cells back and splits them again.
    s.world.get::<&mut WorldPartition>(wp).unwrap().cell_size = 100.0;
    let again = convert::convert(&mut s, wp, &root).unwrap();
    assert_eq!(again.cells, 4);
    assert_eq!(again.moved, 48);
    let files = std::fs::read_dir(root.join("Assets/World/Cells"))
        .unwrap()
        .count();
    assert_eq!(files, 4, "old cell files are gone");
}

#[test]
fn cells_stream_in_and_out_around_a_source_with_hysteresis() {
    let root = temp_root("stream");
    let (mut s, wp) = world(&root);
    convert::convert(&mut s, wp, &root).unwrap();
    let p = player(&mut s, Vec3::new(25.0, 0.0, 25.0));
    let mut st = Streamer::new();
    assert!(st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(20)));
    let mut got = loaded(&st);
    got.sort();
    assert_eq!(got, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    assert!(s.find_by_name("Crate 1 1 2").is_some());
    assert!(s.find_by_name("Crate 2 0 0").is_none());
    assert!(area_loaded(
        &s,
        &find(&s).unwrap().1,
        Vec3::new(25.0, 0.0, 25.0),
        30.0
    ));
    assert!(!area_loaded(
        &s,
        &find(&s).unwrap().1,
        Vec3::new(25.0, 0.0, 25.0),
        80.0
    ));
    let entry = s
        .find_loaded(&format!("{}/Cells/Cell_1_1.scene", folder(&root)))
        .unwrap();
    assert!(s.entry(entry).unwrap().streamed);
    assert_eq!(st.stats().loaded, 4);
    assert!(st.stats().memory_bytes > 0);

    // 70 m from cell (1, 0): past the load radius but inside the unload radius: kept.
    s.set_transform(p, Transform::from_position(Vec3::new(-20.0, 0.0, 25.0)));
    st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(20));
    assert!(loaded(&st).contains(&(1, 0)), "hysteresis keeps it");
    // 100 m away: unloaded, objects gone, the entry too.
    s.set_transform(p, Transform::from_position(Vec3::new(-50.0, 0.0, 25.0)));
    st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(20));
    assert!(!loaded(&st).contains(&(1, 0)));
    assert!(s.find_by_name("Crate 1 0 0").is_none());
    assert!(
        s.find_by_path(&format!("{}/Cells/Cell_1_0.scene", folder(&root)))
            .is_none()
    );
    assert!(st.stats().unloads >= 1);
    // By hand: a pinned cell stays loaded wherever the sources are.
    st.load((3, 3));
    st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(20));
    assert!(s.find_by_name("Crate 3 3 0").is_some());
    st.unload((3, 3));
    st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(20));
    assert!(s.find_by_name("Crate 3 3 0").is_none());
}

#[test]
fn without_a_streaming_source_the_main_camera_streams() {
    let root = temp_root("camera");
    let (mut s, wp) = world(&root);
    convert::convert(&mut s, wp, &root).unwrap();
    let cam = main_camera(&s).unwrap();
    s.set_transform(cam, Transform::from_position(Vec3::new(175.0, 2.0, 175.0)));
    let mut st = Streamer::new();
    assert!(st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(20)));
    assert!(loaded(&st).contains(&(3, 3)));
    assert!(!loaded(&st).contains(&(0, 0)));
    assert_eq!(st.sources()[0].name, "Main Camera");
}

/// One cell of 3000 objects with a 1 ms budget: it arrives over many frames, each frame's
/// main-thread work stays near the budget, and the proxy-hiding flag waits for the last object.
#[test]
fn a_big_cell_is_added_over_several_frames_within_the_budget() {
    let root = temp_root("budget");
    let mut s = Scene::default_3d("World");
    let wp = s.spawn("World Partition", None);
    s.add(
        wp,
        WorldPartition {
            folder: folder(&root),
            cell_size: 100.0,
            budget_ms: 1.0,
            ..Default::default()
        },
    );
    for i in 0..3000 {
        let e = s.spawn(&format!("Pebble {i}"), None);
        let p = Vec3::new(10.0 + (i % 60) as f32, 0.0, 10.0 + (i / 60) as f32);
        s.set_transform(e, Transform::from_position(p));
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(e, MeshRenderer::default());
    }
    convert::convert(&mut s, wp, &root).unwrap();
    assert_eq!(s.world.get::<&WorldPartition>(wp).unwrap().cells.len(), 1);
    player(&mut s, Vec3::new(50.0, 0.0, 50.0));
    let mut st = Streamer::new();
    let start = Instant::now();
    let mut adding_frames = 0;
    let mut times = Vec::new();
    while st.phase((0, 0)) != CellPhase::Loaded {
        st.update(&mut s, &[], &mut NoHooks);
        times.push(st.stats().last_ms);
        if st.phase((0, 0)) == CellPhase::Adding {
            adding_frames += 1;
            assert!(
                !cell_loaded(&s, &find(&s).unwrap().1, (0, 0)),
                "partial until done"
            );
        }
        assert!(start.elapsed() < Duration::from_secs(90));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(s.find_by_name("Pebble 2999").is_some());
    assert!(adding_frames >= 3, "added over {adding_frames} frames");
    let worst = times.iter().copied().fold(0.0f32, f32::max);
    eprintln!(
        "3000 objects: {adding_frames} frames adding, worst main-thread frame {worst:.2} ms (budget 1 ms)"
    );
    // A whole subtree past the deadline at most; generous for a loaded test machine.
    assert!(worst < 16.0, "worst frame {worst} ms");
}

// ---- terrain tiles --------------------------------------------------------------------------

fn hills(x: f32, z: f32) -> f32 {
    20.0 + 8.0 * (x * 0.013).sin() * (z * 0.011).cos() + 3.0 * (x * 0.05 + z * 0.03).sin()
}

#[test]
fn terrain_tiles_meet_without_seams_and_keep_their_border_in_the_file() {
    let template = TerrainData::new(33, Vec3::new(64.0, 50.0, 64.0), 32);
    let coords = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let mut tiles = tiles::from_fn(&template, Vec2::ZERO, 0.0, 64.0, 33, &coords, hills);
    tiles::stitch(&mut tiles);
    let at = |c: CellCoord| tiles.iter().find(|t| t.coord == c).unwrap();
    for (a, b) in [
        ((0, 0), (1, 0)),
        ((0, 0), (0, 1)),
        ((1, 0), (1, 1)),
        ((0, 1), (1, 1)),
    ] {
        let (h, angle) = tiles::seam_error(at(a), at(b));
        assert!(h < 1e-4, "{a:?}-{b:?} height step {h}");
        assert!(angle < 0.01, "{a:?}-{b:?} normals differ by {angle}°");
    }
    // Without the border the edge normals would disagree (one-sided differences).
    let mut bare = tiles.clone();
    for t in &mut bare {
        t.data.border.clear();
    }
    let (_, angle) = tiles::seam_error(&bare[0], &bare[1]);
    assert!(angle > 0.05, "the border matters ({angle}°)");
    // The border survives the file (quantised like the heights).
    let back = TerrainData::from_file_str(&tiles[0].data.to_file_string()).unwrap();
    assert_eq!(back.border.len(), 4 * 33);
    for (a, b) in back.border.iter().zip(&tiles[0].data.border) {
        assert!((a - b).abs() < 2.0 / 65535.0);
    }
    // Old files without one load with none.
    let mut plain = template.clone();
    plain.border.clear();
    assert!(
        TerrainData::from_file_str(&plain.to_file_string())
            .unwrap()
            .border
            .is_empty()
    );
}

#[test]
fn splitting_a_terrain_keeps_its_shape_trees_and_paint() {
    let mut big = TerrainData::new(129, Vec3::new(128.0, 50.0, 128.0), 64);
    for iz in 0..129 {
        for ix in 0..129 {
            big.set_height(ix, iz, hills(ix as f32, iz as f32) / 50.0);
        }
    }
    big.layers = vec![Default::default(), Default::default()];
    big.fit_splat_maps();
    for z in 0..64 {
        for x in 32..64 {
            let mut w = [0u8; crate::terrain::MAX_LAYERS];
            w[1] = 255;
            big.set_layer_weights(z * 64 + x, &w);
        }
    }
    big.trees.prototypes = vec![crate::terrain::trees::TreePrototype {
        tree: "Oak.tree".into(),
    }];
    big.trees.instances = (0..10)
        .map(|i| crate::terrain::trees::TreeInstance {
            position: Vec2::new(0.05 + i as f32 * 0.09, 0.3),
            ..Default::default()
        })
        .collect();
    let corner = Vec3::new(-64.0, 3.0, -64.0);
    let tiles = tiles::split(&big, corner, 2, 65);
    assert_eq!(tiles.len(), 4);
    let trees: usize = tiles.iter().map(|t| t.data.trees.instances.len()).sum();
    assert_eq!(trees, 10, "every tree lands on one tile");
    for t in &tiles {
        assert_eq!(t.data.size, Vec3::new(64.0, 50.0, 64.0));
        // Same surface: compare heights at a few points.
        for (x, z) in [(3.0, 5.0), (31.0, 60.0), (63.9, 0.1)] {
            let world = Vec2::new(t.corner.x + x, t.corner.z + z);
            let mine = t.corner.y + t.data.sample_height(x, z);
            let theirs = corner.y + big.sample_height(world.x - corner.x, world.y - corner.z);
            assert!((mine - theirs).abs() < 0.05, "{mine} vs {theirs}");
        }
    }
    // The painted right half stays painted.
    let right = tiles.iter().find(|t| t.coord == (1, 0)).unwrap();
    assert!(right.data.weights_at(32.0, 32.0)[1] > 0.9);
    let left = tiles.iter().find(|t| t.coord == (0, 0)).unwrap();
    assert!(left.data.weights_at(20.0, 32.0)[0] > 0.9);
    let (h, angle) = tiles::seam_error(left, right);
    assert!(h < 1e-4 && angle < 0.01, "{h} m {angle}°");
}

// ---- HLOD -------------------------------------------------------------------------------

struct NoMaterials;
impl MaterialSource for NoMaterials {
    fn params(&self, _: Option<&str>) -> MaterialParams {
        MaterialParams::default()
    }
}

#[test]
fn hlod_proxies_merge_a_cell_round_trip_and_hide_once_the_cell_is_in() {
    let root = temp_root("hlod");
    let (mut s, wp) = world(&root);
    // A terrain tile in cell (0, 0) with a few trees.
    let template = TerrainData::new(33, Vec3::new(50.0, 40.0, 50.0), 32);
    let mut tile = tiles::from_fn(&template, Vec2::ZERO, 0.0, 50.0, 33, &[(0, 0)], hills).remove(0);
    tile.data.trees.prototypes = vec![crate::terrain::trees::TreePrototype {
        tree: "Missing.tree".into(),
    }];
    tile.data.trees.instances = vec![crate::terrain::trees::TreeInstance::default(); 3];
    let terrain_path = root
        .join("Tile.terrain")
        .to_string_lossy()
        .replace('\\', "/");
    tile.data.save(Path::new(&terrain_path)).unwrap();
    let t = s.spawn("Tile", None);
    s.set_transform(t, Transform::from_position(tile.corner));
    s.add(
        t,
        Terrain {
            terrain_data: terrain_path.clone(),
            ..Default::default()
        },
    );
    convert::convert(&mut s, wp, &root).unwrap();
    let report = hlod::build_all(&mut s, wp, &root).unwrap();
    assert_eq!(report.cells, 16);
    let part = (*s.world.get::<&WorldPartition>(wp).unwrap()).clone();
    assert!(part.cells.iter().all(|c| c.hlod));
    assert!(root.join(hlod::material_path(&part)).exists());
    // Cell (0, 0): the terrain grid, three tree crowns and three crates (12 triangles each).
    let file = PathBuf::from(part.hlod_path((0, 0)));
    let text = std::fs::read_to_string(&file).unwrap();
    let proxy = hlod::HlodMesh::from_file_str(&text).unwrap();
    let grid = 2 * 2 * 2; // 50 m / 32 m rounds to 2 steps a side
    assert_eq!(
        proxy.triangles(),
        grid + 3 * 8 + 3 * 12,
        "{}",
        proxy.triangles()
    );
    assert!(text.len() < 4000, "{} bytes", text.len());
    // Round trip within the quantisation.
    let again = hlod::HlodMesh::from_file_str(&proxy.to_file_string((0, 0))).unwrap();
    assert_eq!(again.indices, proxy.indices);
    assert_eq!(proxy.crowns.len(), 3, "trees are stored as crowns");
    for (a, b) in again.crowns.iter().zip(&proxy.crowns) {
        assert!(a.center.distance(b.center) < 0.01 && (a.radius - b.radius).abs() < 0.06);
    }
    // Loaded, crowns become diamonds of the one merged mesh.
    let mesh = proxy.to_mesh_data();
    assert_eq!(mesh.indices.len() / 3, proxy.triangles());
    assert_eq!(mesh.colors.len(), mesh.vertices.len());
    for (a, b) in again.positions.iter().zip(&proxy.positions) {
        assert!(a.distance(*b) < 0.01);
    }
    // Drawn for every unloaded cell once read (on the background pool)…
    let mut out = Vec::new();
    let start = Instant::now();
    while out.len() < 16 {
        out.clear();
        hlod::draws(&s, &NoMaterials, |_| true, &mut out);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{} proxies",
            out.len()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    // …and not for loaded ones.
    player(&mut s, Vec3::new(25.0, 0.0, 25.0));
    let mut st = Streamer::new();
    st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(20));
    out.clear();
    hlod::draws(&s, &NoMaterials, |_| true, &mut out);
    assert_eq!(out.len(), 16 - st.stats().loaded);
    assert!(st.stats().loaded >= 4);
    // The terrain of the loaded cell came through the background read.
    assert!(crate::terrain::store::is_loaded(&terrain_path));
}

// ---- the game ---------------------------------------------------------------------------

/// In Play mode cells stream through the Runtime: streamed objects' scripts start, and a script
/// waits for its area with `WorldPartition.WaitForArea`.
#[test]
fn scripts_wait_for_an_area_and_streamed_objects_start() {
    let root = temp_root("lua");
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(
        root.join("Assets/Scripts/Waiter.lua"),
        r#"
local W = {}
function W:Start()
    Debug.Log("start", WorldPartition.enabled, WorldPartition.IsAreaLoaded(Vector3.new(25, 0, 25), 10))
    WorldPartition.Run(function()
        WorldPartition.WaitForArea(Vector3.new(25, 0, 25), 10)
        local x, z = WorldPartition.CellAt(Vector3.new(25, 0, 25))
        Debug.Log("area loaded", x, z, WorldPartition.GetCellState(0, 0), WorldPartition.loadedCells > 0)
    end)
end
return W
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("Assets/Scripts/Crate.lua"),
        "local C = {}\nfunction C:Start() Debug.Log(\"crate started\", self.gameObject.name) end\nreturn C\n",
    )
    .unwrap();
    let (mut s, wp) = world(&root);
    let scripts = |path: &str| Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    };
    let c = s.find_by_name("Crate 0 0 0").unwrap();
    s.add(c, scripts("Assets/Scripts/Crate.lua"));
    convert::convert(&mut s, wp, &root).unwrap();
    let p = player(&mut s, Vec3::new(25.0, 0.0, 25.0));
    s.add(p, scripts("Assets/Scripts/Waiter.lua"));
    let settings = crate::settings::ProjectSettings::default();
    let (audio, _) = crate::audio::Audio::null(root.clone());
    let mut rt = crate::runtime::Runtime::start_with_audio(&mut s, root.clone(), &settings, audio);
    let mut time = crate::runtime::Time::default();
    let mut logs = Vec::new();
    let start = Instant::now();
    while !logs.iter().any(|l: &String| l.starts_with("area loaded")) {
        time.advance(1.0 / 60.0);
        rt.update(&mut s, &time, crate::scripting::Input::default());
        logs.extend(rt.take_logs().into_iter().map(|(_, t)| t));
        assert!(start.elapsed() < Duration::from_secs(20), "{logs:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(logs.contains(&"start true false".to_string()), "{logs:?}");
    assert!(
        logs.contains(&"area loaded 0 0 Loaded true".to_string()),
        "{logs:?}"
    );
    assert!(
        logs.contains(&"crate started Crate 0 0 0".to_string()),
        "{logs:?}"
    );
    assert!(rt.streaming().stats().loaded >= 1);
}

/// Edit mode: the main camera doesn't stream; a source handed in (the Scene view camera) does,
/// and cells stay when it goes.
#[test]
fn edit_mode_streams_only_around_a_given_source() {
    let root = temp_root("edit");
    let (mut s, wp) = world(&root);
    convert::convert(&mut s, wp, &root).unwrap();
    let mut st = Streamer::new();
    st.auto = false;
    st.update(&mut s, &[], &mut NoHooks);
    assert!(st.settle(&mut s, &[], &mut NoHooks, Duration::from_secs(5)));
    assert!(
        loaded(&st).is_empty(),
        "the main camera doesn't stream in Edit mode"
    );
    let scene_camera = Source {
        name: "Scene Camera".into(),
        position: Vec3::new(175.0, 50.0, 175.0),
        load_radius: 20.0,
        unload_radius: 40.0,
    };
    assert!(st.settle(
        &mut s,
        std::slice::from_ref(&scene_camera),
        &mut NoHooks,
        Duration::from_secs(20)
    ));
    assert_eq!(loaded(&st), [(3, 3)]);
    for _ in 0..5 {
        st.update(&mut s, &[], &mut NoHooks);
    }
    assert_eq!(loaded(&st), [(3, 3)], "stays when the source goes");
}
