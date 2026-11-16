//! Any project's scene rendered from its saved views, without a window (all ignored; run on
//! purpose in release), for look checks and frame times of scenes outside `SampleProject`:
//!
//! - `project_scene_shots`: the scene `MISTY_SHOT_SCENE` (project-relative) of the project
//!   `MISTY_SHOT_PROJECT`, from every GameObject named in `MISTY_SHOT_VIEWS` (comma separated;
//!   the view takes that object's position and rotation, e.g. switched-off cameras kept in the
//!   scene as bookmarks; default: the scene's own camera, named `camera`), at every level of
//!   `MISTY_QUALITY` (default `high`), `MISTY_SIZE` (default 1920×1080), 64 frames, written
//!   as `<level>_<view>.png` into `MISTY_SHOT_DIR`.
//! - `project_scene_times`: median GPU frame time and the GPU time of every part, per view
//!   and level.
//!
//! `MISTY_SHOT_AE=off` switches eye adaptation off; `MISTY_SHOT_TIME` sets the wind clock
//! (seconds, default 3); `MISTY_SHOT_WEATHER=<seconds>` runs the scene's Weather that long first;
//! `MISTY_SHOT_OFF=fog,clouds,shafts,terrain,meshes` leaves those out.

use std::path::PathBuf;

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::terrain_grass::gpu_tests::breakdown;
use crate::render_scene::GameView;
use crate::scene::Scene;

fn root() -> PathBuf {
    PathBuf::from(std::env::var("MISTY_SHOT_PROJECT").expect("MISTY_SHOT_PROJECT"))
}

/// File-name form of a view's name ("View Into Sun" → "view_into_sun").
fn slug(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// The scene's view from the GameObject `name` (`camera` = the scene's camera as it is).
fn view(name: &str) -> Option<GameView> {
    let root = root();
    crate::assets::set_project_root(&root);
    crate::asset_db::install(crate::asset_db::GuidDatabase::read(&root));
    let rel = std::env::var("MISTY_SHOT_SCENE").expect("MISTY_SHOT_SCENE");
    let (mut scene, _) = Scene::load(&root.join(rel)).expect("scene");
    // `MISTY_SHOT_WEATHER=120`: the scene's Weather run for that many seconds first (rain has
    // wetted the ground, snow has settled), as after that long in Play.
    if let Some(seconds) = std::env::var("MISTY_SHOT_WEATHER")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        let eye =
            crate::render_scene::game_camera(&scene).map_or(glam::Vec3::ZERO, |c| c.0.position);
        for _ in 0..(seconds / 0.25).ceil() as usize {
            crate::weather::update_scene(&mut scene, 0.25, eye);
        }
    }
    let materials = crate::assets::AssetCache::open(&root);
    let mut gv = GameView::new(&scene, &materials).expect("camera");
    let time = std::env::var("MISTY_SHOT_TIME")
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(3.0);
    gv.environment.grass_time = Some(time);
    if name.trim() != "camera" {
        let e = scene.find_by_name(name.trim())?;
        let (_, rotation, position) = scene.world_matrix(e).to_scale_rotation_translation();
        gv.camera.position = position;
        gv.camera.rotation = rotation;
    }
    if std::env::var("MISTY_SHOT_AE").is_ok_and(|v| v == "off") {
        gv.post.auto_exposure.active = false;
    }
    // `MISTY_SHOT_OFF=fog,clouds,shafts,terrain,meshes`: the view without these (to find
    // which part of a picture something comes from).
    for part in std::env::var("MISTY_SHOT_OFF")
        .unwrap_or_default()
        .split(',')
    {
        match part.trim() {
            "fog" => gv.post.fog = Default::default(),
            "clouds" => gv.post.clouds.active = false,
            "shafts" => gv.post.sun_shafts = Default::default(),
            "terrain" => gv.terrains.clear(),
            "meshes" => gv.meshes = Default::default(),
            "" => {}
            other => eprintln!("(MISTY_SHOT_OFF: {other} unknown)"),
        }
    }
    Some(gv)
}

fn views() -> Vec<String> {
    std::env::var("MISTY_SHOT_VIEWS")
        .unwrap_or_else(|_| "camera".into())
        .split(',')
        .map(str::to_owned)
        .collect()
}

fn levels() -> Vec<(String, QualityLevel)> {
    let names = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high".into());
    names
        .split(',')
        .filter_map(|n| {
            let q = match n {
                "low" => QualityLevel::low(),
                "medium" => QualityLevel::medium(),
                "high" => QualityLevel::high(),
                "ultra" => QualityLevel::ultra(),
                _ => return None,
            };
            Some((n.to_owned(), q))
        })
        .collect()
}

fn size() -> (u32, u32) {
    std::env::var("MISTY_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1920, 1080))
}

#[test]
#[ignore = "writes look-check renders of a project; run on purpose"]
fn project_scene_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = PathBuf::from(std::env::var("MISTY_SHOT_DIR").expect("MISTY_SHOT_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h) = size();
    for (level, quality) in levels() {
        for name in views() {
            let Some(view) = view(&name) else {
                eprintln!("(no GameObject named {name:?})");
                continue;
            };
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &quality);
            let target = r.create_view_target(&device, w, h);
            let frame = view.frame();
            for _ in 0..64 {
                r.render(&device, &queue, &target, &frame);
            }
            let img = image::RgbaImage::from_raw(w, h, read_back(&device, &queue, &target))
                .expect("read back");
            let path = dir.join(format!("{level}_{}.png", slug(&name)));
            img.save(&path).unwrap();
            eprintln!("wrote {}", path.display());
        }
    }
}

#[test]
#[ignore = "timing of a project's scene, prints only"]
fn project_scene_times() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = size();
    let names = views();
    // A first run warms the GPU clocks up.
    if let Some(v) = names.first().and_then(|n| view(n)) {
        breakdown(&device, &queue, &v, &QualityLevel::low(), (w, h));
    }
    for name in &names {
        let Some(v) = view(name) else {
            eprintln!("(no GameObject named {name:?})");
            continue;
        };
        for (level, quality) in levels() {
            let b = breakdown(&device, &queue, &v, &quality, (w, h));
            b.print(&format!(
                "{} {level} {w}x{h} ({:.0} FPS)",
                slug(name),
                1000.0 / b.gpu
            ));
        }
    }
}

/// Where a view's draws come from: the tree draws of every pass (per part and mesh level,
/// how many hold no tree) and the frame's counters, per view at High.
#[test]
#[ignore = "profile of a project's scene, prints only"]
fn project_scene_draws() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = size();
    for name in views() {
        let Some(v) = view(&name) else {
            continue;
        };
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &QualityLevel::high());
        let target = r.create_view_target(&device, w, h);
        let frame = v.frame();
        for _ in 0..12 {
            r.render(&device, &queue, &target, &frame);
        }
        // The last frame's terrain passes alone.
        let terrain = crate::render::terrain::TerrainCache::pass_draws();
        terrain.lock().unwrap().clear();
        r.render(&device, &queue, &target, &frame);
        let stats = r.stats();
        for (kind, draws) in terrain.lock().unwrap().iter() {
            eprintln!("   terrain {kind}: {draws} draws");
        }
        eprintln!(
            "== {}: {} draws, {} pipeline changes, {} mesh objects, CPU {:.2} ms",
            slug(&name),
            stats.batches,
            stats.set_pass,
            v.meshes.len(),
            stats.cpu_ms
        );
        for line in r.trees.draw_report() {
            eprintln!("   {line}");
        }
    }
}
