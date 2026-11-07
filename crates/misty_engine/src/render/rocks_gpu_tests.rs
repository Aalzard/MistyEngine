//! MeadowDemo rocks (`Assets/Rocks`): comparison shots and GPU cost, with and without
//! virtual geometry (all ignored; run on purpose in release):
//! `cargo test --release -p misty_engine rock_ -- --ignored --nocapture --test-threads=1`
//!
//! - `rock_shots`: `docs/screenshots/gfx/rocks_<step>_<shot>.png` (1600×900, High) for the
//!   shots close (next to the near boulder group), wide (the player's start view) and behind
//!   (turned round). `MISTY_GFX_STEP=<name>` (default "latest"), `MISTY_ROCKS=0` renders
//!   without the rocks (the "before" shots), `MISTY_VGEO=1` with Quality ▸ Virtual Geometry
//!   on (plus the Clusters and Levels debug views of the close shot).
//! - `rock_cost`: MeadowDemo at 1920×1080 per quality level: no rocks, rocks through their LOD
//!   Groups, rocks through virtual geometry (160 frames, median GPU time of the last 120).

use std::path::PathBuf;

use glam::Vec3;

use crate::quality::QualityLevel;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Renderer, VgeoDebug};
use crate::render_scene::GameView;
use crate::scene::{MeshFilter, Scene, Transform};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

#[derive(Clone, Copy, Debug)]
enum Shot {
    Close,
    Wide,
    Behind,
    /// The six rock types in a row in front of the player (front-lit).
    Gallery,
}

impl Shot {
    const ALL: [Shot; 4] = [Shot::Close, Shot::Wide, Shot::Behind, Shot::Gallery];

    fn name(self) -> &'static str {
        match self {
            Shot::Close => "close",
            Shot::Wide => "wide",
            Shot::Behind => "behind",
            Shot::Gallery => "gallery",
        }
    }
}

const GALLERY: [&str; 6] = [
    "Cobbles",
    "Rock Small",
    "Boulder Round",
    "Boulder Split",
    "Boulder Large",
    "Outcrop Layered",
];

/// The rock instance nearest to the player whose name starts with "Boulder Large".
fn near_boulder(s: &Scene, player: Vec3) -> Option<Vec3> {
    let rocks = s.find_by_name("Rocks")?;
    s.children(rocks)
        .into_iter()
        .filter(|&e| s.game_object(e).name.starts_with("Boulder Large"))
        .map(|e| s.world_position(e))
        .min_by(|a, b| a.distance(player).total_cmp(&b.distance(player)))
}

/// MeadowDemo as saved (without its rocks when `rocks` is false), the player placed for `shot`.
fn meadow(shot: Shot, rocks: bool) -> GameView {
    crate::assets::set_project_root(&root());
    let (mut s, _) =
        Scene::load(&root().join("Assets/Scenes/MeadowDemo.scene")).expect("MeadowDemo.scene");
    let player = s.find_by_name("Player").expect("player");
    let p = s.world_position(player);
    // Where to stand and look: (position, yaw, pitch); positive pitch looks down.
    let (pos, yaw, pitch) = match shot {
        Shot::Wide => (p, 0.0, 0.0),
        // Turned round (the sun behind), the rocks 13–23 m ahead.
        Shot::Gallery => (p, 180.0, 8.0),
        Shot::Behind => (p, 180.0, -2.0),
        Shot::Close => {
            let target = near_boulder(&s, p).unwrap_or(p + Vec3::new(-18.0, 0.0, 7.0));
            // 7.5 m from the boulder, on the side towards the player's start (sun side).
            let flat = Vec3::new(p.x - target.x, 0.0, p.z - target.z).normalize_or(Vec3::Z);
            let mut at = target + flat * 7.5;
            let terrain = s.find_by_name("Terrain").expect("terrain");
            let ground = |w: Vec3| {
                crate::terrain::sample_height(&s, terrain, w).unwrap_or(0.0)
                    + s.world_position(terrain).y
            };
            at.y = ground(at) + (p.y - ground(p));
            let to = target - at;
            let yaw = to.x.atan2(to.z).to_degrees();
            (at, yaw, 6.0)
        }
    };
    s.set_transform(
        player,
        Transform::from_position(pos).with_euler(Vec3::new(0.0, yaw, 0.0)),
    );
    if let Some(cam) = s.find_by_name("Camera") {
        s.set_transform(
            cam,
            Transform::default().with_euler(Vec3::new(pitch, 0.0, 0.0)),
        );
    }
    if (!rocks || matches!(shot, Shot::Gallery))
        && let Some(r) = s.find_by_name("Rocks")
    {
        s.despawn(r);
    }
    if rocks && matches!(shot, Shot::Gallery) {
        let terrain = s.find_by_name("Terrain").expect("terrain");
        let corner = s.world_position(terrain);
        let mut x = -12.5;
        for (i, name) in GALLERY.iter().enumerate() {
            let path = format!("Assets/Rocks/Prefabs/{name}.prefab");
            let prefab = crate::scene::prefab::Prefab::load(&root().join(&path)).expect("rock");
            let e = s.instantiate_prefab(&prefab, Some(&path), None);
            let width = [1.2, 0.9, 2.3, 3.4, 5.2, 8.0][i];
            x += width * 0.5;
            let at = pos + Vec3::new(-x, 0.0, -(13.0 + i as f32 * 2.0));
            let y = crate::terrain::sample_height(&s, terrain, at).unwrap_or(0.0) + corner.y;
            s.set_transform(
                e,
                Transform::from_position(Vec3::new(at.x, y - 0.05, at.z)).with_euler(Vec3::new(
                    0.0,
                    25.0 * i as f32,
                    0.0,
                )),
            );
            x += width * 0.5 + 0.8;
        }
    }
    let materials = crate::assets::AssetCache::open(&root());
    let mut view = GameView::new(&s, &materials).expect("camera");
    view.environment.grass_time = Some(3.0);
    view
}

/// Wait until the virtual geometry builds of every rock mesh in `view` are ready (read from
/// the `Library` cache or built on the import worker).
fn wait_for_builds(view: &GameView) {
    let started = std::time::Instant::now();
    let paths: std::collections::BTreeSet<String> = view
        .frame()
        .meshes
        .iter()
        .filter_map(|d| d.mesh.split_asset().map(|(p, _)| p.to_owned()))
        .collect();
    for p in paths {
        while let Ok(m) = crate::models::model(&p) {
            if !m.virtual_geometry || m.virtual_builds.complete() {
                break;
            }
            if started.elapsed().as_secs() > 120 {
                panic!("virtual geometry build of {p} not ready");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

fn quality(base: QualityLevel, vgeo: bool) -> QualityLevel {
    QualityLevel {
        virtual_geometry: vgeo,
        ..base
    }
}

#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn rock_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let step = std::env::var("MISTY_GFX_STEP").unwrap_or_else(|_| "latest".into());
    let only = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
    let rocks = !std::env::var("MISTY_ROCKS").is_ok_and(|v| v == "0");
    let vgeo = std::env::var("MISTY_VGEO").is_ok_and(|v| v == "1");
    let (w, h) = (1600, 900);
    std::fs::create_dir_all(screenshots()).unwrap();
    for shot in Shot::ALL {
        if !only.is_empty() && !shot.name().contains(&only) {
            continue;
        }
        let view = meadow(shot, rocks);
        wait_for_builds(&view);
        let mut debug = vec![("", VgeoDebug::Off)];
        if vgeo && rocks && matches!(shot, Shot::Close) {
            debug.push(("_clusters", VgeoDebug::Clusters));
            debug.push(("_levels", VgeoDebug::Levels));
        }
        for (suffix, view_mode) in debug {
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &quality(QualityLevel::high(), vgeo));
            let target = r.create_target(&device, w, h);
            let frame = view.frame();
            for i in 0..48 {
                if i == 47 && view_mode != VgeoDebug::Off {
                    r.set_virtual_geometry_debug_view(view_mode);
                }
                r.render(&device, &queue, &target, &frame);
            }
            let img =
                image::RgbaImage::from_raw(w, h, read_back(&device, &queue, &target)).unwrap();
            let path = screenshots().join(format!("rocks_{step}_{}{suffix}.png", shot.name()));
            img.save(&path).unwrap();
            eprintln!(
                "wrote {} ({:.2} M triangles)",
                path.display(),
                r.stats().triangles as f64 / 1e6
            );
        }
    }
}

#[test]
#[ignore = "GPU timings of MeadowDemo with and without rocks; run on purpose"]
fn rock_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let levels: Vec<(&str, QualityLevel)> = vec![
        ("low", QualityLevel::low()),
        ("medium", QualityLevel::medium()),
        ("high", QualityLevel::high()),
        ("ultra", QualityLevel::ultra()),
    ];
    let only = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
    for shot in Shot::ALL {
        if !only.is_empty() && !shot.name().contains(&only) {
            continue;
        }
        let with = meadow(shot, true);
        let without = meadow(shot, false);
        wait_for_builds(&with);
        let rock_draws = with
            .frame()
            .meshes
            .iter()
            .filter(|d| {
                d.mesh
                    .split_asset()
                    .is_some_and(|(p, _)| p.starts_with("Assets/Rocks/"))
            })
            .count();
        for (name, base) in &levels {
            // (label, view, virtual geometry)
            let runs = [
                ("none", &without, false),
                ("lod", &with, false),
                ("vgeo", &with, true),
            ];
            let mut out = Vec::new();
            for (label, view, vgeo) in runs {
                let mut r = Renderer::with_queue(&device, &queue);
                r.set_quality(&device, &quality(base.clone(), vgeo));
                let target = r.create_target(&device, 1920, 1080);
                let frame = view.frame();
                let mut times = Vec::new();
                for f in 0..160 {
                    r.render(&device, &queue, &target, &frame);
                    let _ = device.poll(wgpu::PollType::wait_indefinitely());
                    if f >= 40
                        && let Some(t) = r.gpu_ms()
                    {
                        times.push(t);
                    }
                }
                times.sort_by(f32::total_cmp);
                let ms = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
                let tris = r.stats().triangles as f64 / 1e6;
                let vg = if vgeo {
                    let s = r.virtual_geometry_stats(&device, &queue);
                    format!(
                        " [vg {} instances, {} clusters, {:.2} M tris, {:.2} M shadow tris]",
                        s.instances,
                        s.clusters,
                        s.triangles as f64 / 1e6,
                        s.shadow_triangles as f64 / 1e6
                    )
                } else {
                    String::new()
                };
                out.push((label, ms, tris, vg));
            }
            let none = out[0].1;
            let line: Vec<String> = out
                .iter()
                .map(|(l, ms, tris, vg)| {
                    format!("{l} {ms:.2} ms ({:+.2}) {tris:.2} M tris{vg}", ms - none)
                })
                .collect();
            eprintln!(
                "{:>6} {name:>6} ({rock_draws} rock draws): {}",
                shot.name(),
                line.join(" | ")
            );
        }
    }
}

/// Every rock in MeadowDemo has its four LOD levels; the dense level 0 is a virtual mesh and the
/// others are not.
#[test]
fn meadow_rocks_are_lod_groups_over_a_virtual_mesh() {
    crate::assets::set_project_root(&root());
    let Ok((s, _)) = Scene::load(&root().join("Assets/Scenes/MeadowDemo.scene")) else {
        return;
    };
    let Some(rocks) = s.find_by_name("Rocks") else {
        panic!("MeadowDemo has no Rocks");
    };
    let rocks = s.children(rocks);
    assert!((20..=150).contains(&rocks.len()), "{}", rocks.len());
    for r in rocks {
        let group = s
            .world
            .get::<&crate::scene::LodGroup>(r)
            .expect("LOD Group");
        assert_eq!(group.lods.len(), 4);
        for (i, level) in group.lods.iter().enumerate() {
            let e = level.renderers[0];
            let mesh = s.world.get::<&MeshFilter>(e).unwrap().mesh.clone().unwrap();
            assert_eq!(crate::models::is_virtual(&mesh), i == 0, "{mesh:?}");
        }
    }
}
