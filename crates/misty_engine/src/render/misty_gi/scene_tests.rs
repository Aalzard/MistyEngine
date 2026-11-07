//! Misty GI on the sample scenes (all ignored; run on purpose):
//! `cargo test -p misty_engine misty_gi_scene -- --ignored --nocapture --test-threads=1`
//!
//! - `misty_gi_scene_pass_times`: GPU time per stage at 1920×1080 (High quality level) on
//!   CyclesRoomDemo and MeadowDemo, Misty GI off / Low / Medium / High.
//! - `misty_gi_scene_meadow_shots`: MeadowDemo with and without Misty GI
//!   (`docs/screenshots/gfx/gi_meadow_<shot>_<off|misty>.png`), including a "grove" shot from
//!   beside the tree nearest the scene camera (sky occlusion under the canopy).
//! - `misty_gi_scene_meadow_options`: the ambient options for the user (off, on, on with the
//!   ambient lowered) and the grove with virtual shadow maps. (Made with the scene's ambient
//!   at 1.0; since option C shipped the scene's ambient is 0.6 and the variants scale that.)
//! - `misty_gi_scene_meadow_levels`: MeadowDemo as shipped, GPU frame time at every quality
//!   level with Global Illumination at the level's tier and Off.
//! - `misty_gi_scene_meadow_shipped`: MeadowDemo before and after the realtime GI default.

use std::path::PathBuf;

use crate::quality::QualityLevel;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::terrain_grass::gpu_tests::{Shot, breakdown, meadow_view};
use crate::render::{MistyGiSettings, Renderer};
use crate::render_scene::GameView;
use crate::scene::{MistyGlobalIlluminationOverride, Scene, ScreenSpaceQuality};

/// Builds one of the views a test measures.
type ViewFn = fn() -> GameView;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

/// CyclesRoomDemo as saved (realtime light, baked room probes, SSGI + SSR).
fn cycles_view() -> GameView {
    crate::assets::set_project_root(&root());
    let (scene, _) =
        Scene::load(&root().join("Assets/Scenes/CyclesRoomDemo.scene")).expect("CyclesRoomDemo");
    let materials = crate::assets::AssetCache::open(&root());
    GameView::new(&scene, &materials).expect("camera")
}

/// MeadowDemo at midday, the camera 7 m from the tree nearest the scene camera, looking at its
/// foot.
fn grove_view() -> GameView {
    grove_view_at(Shot::WideMidday)
}

/// The grove shot in the low golden-hour sun.
fn golden_grove_view() -> GameView {
    grove_view_at(Shot::WideGolden)
}

fn grove_view_at(shot: Shot) -> GameView {
    let mut v = meadow_view(shot);
    let t = v.terrains.first().expect("terrain").clone();
    let d = &t.data;
    let cam = v.camera.position;
    let foot = |i: &crate::terrain::trees::TreeInstance| {
        let (u, w) = (i.position.x, i.position.y);
        let r = d.heightmap_resolution - 1;
        let h = d.height((u * r as f32) as usize, (w * r as f32) as usize);
        t.position + glam::Vec3::new(u * d.size.x, h * d.size.y, w * d.size.z)
    };
    let tree = d
        .trees
        .instances
        .iter()
        .map(foot)
        .min_by(|a, b| a.distance(cam).total_cmp(&b.distance(cam)))
        .expect("trees");
    let away = (cam - tree).with_y(0.0).normalize_or(glam::Vec3::Z);
    let eye = tree + away * 7.0 + glam::Vec3::Y * 1.7;
    let dir = (tree + glam::Vec3::Y * 1.0 - eye).normalize();
    v.camera.position = eye;
    v.camera.rotation = glam::Quat::from_rotation_y(dir.x.atan2(dir.z))
        * glam::Quat::from_rotation_x(-dir.y.asin());
    v
}

/// Block until the view's tree assets are built (they build on worker threads; until then
/// neither the renderer nor Misty GI has them).
fn wait_for_trees(v: &GameView) {
    for _ in 0..200 {
        let ready = v.terrains.iter().all(|t| {
            t.data
                .trees
                .prototypes
                .iter()
                .all(|p| crate::trees::store::get(&p.tree).is_some())
        });
        if ready {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// The grove view with the terrain's grass switched off (own store key, so the renderer keeps
/// separate terrain data): the ground under the canopy is the terrain, which reads the GI.
fn bare_grove_view() -> GameView {
    let mut v = grove_view();
    v.terrains = v
        .terrains
        .iter()
        .map(|t| {
            let mut d = (*t.data).clone();
            d.details.enabled = false;
            d.details.density = 0.0;
            crate::terrain::TerrainDraw {
                key: format!("{}#misty-gi-bare", t.key),
                data: std::sync::Arc::new(d),
                ..t.clone()
            }
        })
        .collect();
    v
}

fn with_misty(mut v: GameView, quality: Option<ScreenSpaceQuality>) -> GameView {
    wait_for_trees(&v);
    v.post.misty_gi = match quality {
        Some(quality) => MistyGiSettings::from_override(&MistyGlobalIlluminationOverride {
            active: true,
            quality,
            ..Default::default()
        }),
        None => MistyGiSettings::default(),
    };
    v
}

#[test]
#[ignore = "timing, prints only"]
fn misty_gi_scene_pass_times() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = (1920, 1080);
    let quality = QualityLevel::high();
    let scenes: [(&str, ViewFn); 2] = [
        ("CyclesRoomDemo", cycles_view),
        ("MeadowDemo", || meadow_view(Shot::WideGolden)),
    ];
    // A first run warms the GPU clocks up.
    breakdown(&device, &queue, &cycles_view(), &QualityLevel::low(), size);
    for (name, view) in scenes {
        let configs = [
            ("off", None),
            ("Low", Some(ScreenSpaceQuality::Low)),
            ("Medium", Some(ScreenSpaceQuality::Medium)),
            ("High", Some(ScreenSpaceQuality::High)),
        ];
        for (label, q) in configs {
            let v = with_misty(view(), q);
            let b = breakdown(&device, &queue, &v, &quality, size);
            let misty: Vec<(&str, f32)> = b
                .parts
                .iter()
                .filter(|(l, _)| l.starts_with("Misty GI"))
                .copied()
                .collect();
            let total: f32 = misty.iter().map(|p| p.1).sum();
            eprintln!(
                "{name} {label}: frame {:.2} ms, Misty GI {total:.2} ms ({}), SSGI/SSR {:.2} ms",
                b.gpu,
                misty
                    .iter()
                    .map(|(l, m)| format!("{} {m:.2}", l.trim_start_matches("Misty GI: ")))
                    .collect::<Vec<_>>()
                    .join(", "),
                b.part("SSGI / SSR"),
            );
        }
    }
}

#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn misty_gi_scene_meadow_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (1280, 720);
    // Grass blades and trees have their own lighting shaders, which don't read the GI term
    // yet: "grove_bare" switches the grass off so the ground under the canopy shows Misty GI.
    let shots: [(ViewFn, &str, f32); 5] = [
        (grove_view, "grove", 1.0),
        (golden_grove_view, "grove_golden", 1.0),
        (bare_grove_view, "grove_bare", 1.0),
        (|| meadow_view(Shot::WideMidday), "wide_midday", 1.0),
        (|| meadow_view(Shot::WideGolden), "wide_golden", 1.0),
    ];
    for (view, name, grass) in shots {
        for (label, q) in [("off", None), ("misty", Some(ScreenSpaceQuality::High))] {
            let v = with_misty(view(), q);
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(
                &device,
                &QualityLevel {
                    grass_density: grass,
                    ..QualityLevel::high()
                },
            );
            let target = r.create_target(&device, w, h);
            let frame = v.frame();
            for _ in 0..90 {
                r.render(&device, &queue, &target, &frame);
            }
            let rgba = read_back(&device, &queue, &target);
            let path = screenshots().join(format!("gi_meadow_{name}_{label}.png"));
            image::RgbaImage::from_raw(w, h, rgba)
                .expect("size")
                .save(&path)
                .expect("png");
            eprintln!("wrote {}", path.display());
        }
    }
}

/// MeadowDemo ambient options for the user (`gi_meadow_opt_<shot>_<variant>.png`): Misty GI
/// off; on with the scene's ambient; on with the ambient (Lighting ▸ Environment ▸ Intensity)
/// lowered to 0.6; on with the volume's GI Intensity at 1.6 (only the traced light scales). The grove shots also
/// render with virtual shadow maps (`_vsm`) to compare Misty GI's sun with their shadows. The
/// scene file is not changed.
#[test]
#[ignore = "writes option renders into docs/screenshots/gfx; run on purpose"]
fn misty_gi_scene_meadow_options() {
    use crate::quality::ShadowMode;
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (1280, 720);
    let shots: [(ViewFn, &str); 4] = [
        (|| meadow_view(Shot::WideMidday), "wide"),
        (grove_view, "grove"),
        (|| meadow_view(Shot::WideGolden), "wide_golden"),
        (golden_grove_view, "grove_golden"),
    ];
    // (label, Misty GI intensity (0 = off), ambient intensity, shadow mode)
    let variants = [
        ("off", 0.0, 1.0, ShadowMode::Cascaded),
        ("misty", 1.0, 1.0, ShadowMode::Cascaded),
        ("misty_amb60", 1.0, 0.6, ShadowMode::Cascaded),
        ("misty_gi160", 1.6, 1.0, ShadowMode::Cascaded),
        ("off_vsm", 0.0, 1.0, ShadowMode::Virtual),
        ("misty_vsm", 1.0, 1.0, ShadowMode::Virtual),
    ];
    for (view, name) in shots {
        for (label, gi, ambient, shadows) in variants {
            if shadows == ShadowMode::Virtual && !name.starts_with("grove") {
                continue;
            }
            let mut v = with_misty(view(), (gi > 0.0).then_some(ScreenSpaceQuality::High));
            v.post.misty_gi.intensity = gi;
            v.environment.settings.environment_lighting.intensity *= ambient;
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(
                &device,
                &QualityLevel {
                    shadow_mode: shadows,
                    ..QualityLevel::high()
                },
            );
            let target = r.create_target(&device, w, h);
            let frame = v.frame();
            for _ in 0..90 {
                r.render(&device, &queue, &target, &frame);
                if shadows == ShadowMode::Virtual {
                    // (Virtual shadow pages arrive a frame after they are asked for.)
                    let _ = device.poll(wgpu::PollType::wait_indefinitely());
                }
            }
            let rgba = read_back(&device, &queue, &target);
            let path = screenshots().join(format!("gi_meadow_opt_{name}_{label}.png"));
            image::RgbaImage::from_raw(w, h, rgba)
                .expect("size")
                .save(&path)
                .expect("png");
            eprintln!("wrote {}", path.display());
        }
    }
}

/// The editor's case: a Game view and a Scene view a few metres from it, both 1920×1080 at
/// High, rendered in turn. GPU time of the pair with the Scene view secondary (it gathers
/// from the Game view's clipmaps) and with it updating clipmaps of its own.
#[test]
#[ignore = "timing, prints only"]
fn misty_gi_scene_shared_views() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    for (name, view) in [
        ("MeadowDemo", (|| meadow_view(Shot::WideGolden)) as ViewFn),
        ("CyclesRoomDemo", cycles_view),
    ] {
        let game = with_misty(view(), Some(ScreenSpaceQuality::High));
        let mut scene = with_misty(view(), Some(ScreenSpaceQuality::High));
        scene.camera.position += glam::Vec3::new(1.5, 0.8, -1.0);
        let mut times = Vec::new();
        for secondary in [false, true] {
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &QualityLevel::high());
            let a = r.create_view_target(&device, 1920, 1080);
            let b = r.create_view_target(&device, 1920, 1080);
            b.set_misty_gi_secondary(secondary);
            let (fa, fb) = (game.frame(), scene.frame());
            // The timer's reading at a render is the previous render's: after the Game view's
            // render it holds the Scene view's time and the other way round, so read after each.
            let (mut game_ms, mut scene_ms) = (Vec::new(), Vec::new());
            for i in 0..160 {
                r.render(&device, &queue, &a, &fa);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if i >= 60 {
                    scene_ms.extend(r.gpu_ms());
                }
                r.render(&device, &queue, &b, &fb);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if i >= 60 {
                    game_ms.extend(r.gpu_ms());
                }
            }
            let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
            let (g, sc) = (mean(&game_ms), mean(&scene_ms));
            eprintln!("{name}: Game view {g:.2} ms, Scene view {sc:.2} ms");
            let pair = g + sc;
            eprintln!(
                "{name}: Game + Scene view {:.2} ms per pair, Scene view {} ({} worlds)",
                pair,
                if secondary {
                    "secondary"
                } else {
                    "own clipmaps"
                },
                r.screen_trace.misty.worlds.len()
            );
            times.push(pair);
        }
        eprintln!(
            "{name}: sharing saves {:.2} ms per editor frame",
            times[0] - times[1]
        );
    }
}

/// CyclesRoomDemo with the cube sliding a little every frame: what a moving object costs (the
/// levels holding it recompose, two a frame at most).
#[test]
#[ignore = "timing, prints only"]
fn misty_gi_scene_moving_object_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = with_misty(cycles_view(), Some(ScreenSpaceQuality::High));
    let mut meshes: Vec<_> = view.meshes.as_ref().clone();
    let cube = meshes
        .iter()
        .position(|m| {
            (m.model.w_axis.truncate() - glam::Vec3::new(0.5, 0.35, 0.75)).length() < 0.01
        })
        .expect("the room's cube");
    let base = meshes[cube].model;
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &QualityLevel::high());
    let target = r.create_target(&device, 1920, 1080);
    let timing = r.set_pass_timing(&device, &queue, true);
    let mut sdf = Vec::new();
    let mut total = Vec::new();
    for i in 0..120 {
        let x = 0.3 * (i as f32 * 0.1).sin();
        meshes[cube].model = glam::Mat4::from_translation(glam::Vec3::new(x, 0.0, 0.0)) * base;
        let mut frame = view.frame();
        frame.meshes = &meshes;
        r.render(&device, &queue, &target, &frame);
        if timing && i >= 20 {
            let parts = r.pass_times();
            let sum = |p: &dyn Fn(&str) -> bool| -> f32 {
                parts.iter().filter(|(l, _)| p(l)).map(|(_, m)| m).sum()
            };
            sdf.push(sum(&|l| l == "Misty GI: scene SDF"));
            total.push(sum(&|l| l.starts_with("Misty GI")));
        }
    }
    let median = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
    };
    eprintln!(
        "moving cube: scene SDF {:.2} ms, Misty GI total {:.2} ms (median of 100 frames)",
        median(sdf),
        median(total)
    );
}

/// MeadowDemo's shapes as Misty GI sees them: the terrain and two proxies per tree once the
/// tree assets are built.
#[test]
#[ignore = "diagnostics, prints only"]
fn misty_gi_scene_meadow_shapes() {
    let view = meadow_view(Shot::WideMidday);
    let mut frame = view.frame();
    frame.post.misty_gi.enabled = true;
    let region = super::scene::TerrainRegion {
        x: 0,
        y: 0,
        columns: 2,
        rows: 2,
    };
    for attempt in 0..60 {
        let list = super::scene::build(&frame, &mut |_| None, &mut |_| Some(region));
        let count = |k: f32| list.instances.iter().filter(|i| i.extent[3] == k).count();
        let trees: usize = view
            .terrains
            .iter()
            .map(|t| t.data.trees.instances.len())
            .sum();
        eprintln!(
            "attempt {attempt}: {} shapes: terrain {}, capsules {}, ellipsoids {}, boxes {}; {trees} trees placed",
            list.instances.len(),
            count(super::scene::KIND_TERRAIN),
            count(super::scene::KIND_CAPSULE),
            count(super::scene::KIND_ELLIPSOID),
            count(super::scene::KIND_BOX),
        );
        if count(super::scene::KIND_ELLIPSOID) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// MeadowDemo as shipped (Lighting ▸ Realtime Global Illumination on, Environment Intensity
/// 0.6): GPU frame time at 1920×1080 at every quality level, with Quality ▸ Global
/// Illumination at the level's tier and switched Off.
#[test]
#[ignore = "timing, prints only"]
fn misty_gi_scene_meadow_levels() {
    use crate::quality::Tier;
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = (1920, 1080);
    let v = meadow_view(Shot::WideGolden);
    wait_for_trees(&v);
    assert!(
        v.post.misty_gi.enabled,
        "MeadowDemo ships with realtime GI on"
    );
    breakdown(&device, &queue, &v, &QualityLevel::low(), size);
    for (level, quality) in [
        ("Low", QualityLevel::low()),
        ("Medium", QualityLevel::medium()),
        ("High", QualityLevel::high()),
        ("Ultra", QualityLevel::ultra()),
    ] {
        let off = QualityLevel {
            global_illumination: Tier::Off,
            ..quality.clone()
        };
        let (mut best_off, mut best_on) = (f32::MAX, f32::MAX);
        let mut misty = 0.0;
        for _ in 0..3 {
            best_off = best_off.min(breakdown(&device, &queue, &v, &off, size).gpu);
            let b = breakdown(&device, &queue, &v, &quality, size);
            if b.gpu < best_on {
                best_on = b.gpu;
                misty = b
                    .parts
                    .iter()
                    .filter(|(l, _)| l.starts_with("Misty GI"))
                    .map(|p| p.1)
                    .sum();
            }
        }
        eprintln!(
            "MeadowDemo {level} ({:?}): GI off {best_off:.2} ms, GI on {best_on:.2} ms \
             (+{:.2} ms, Misty GI passes {misty:.2} ms)",
            quality.global_illumination,
            best_on - best_off
        );
    }
}

/// MeadowDemo before and after the realtime GI default (`gi_meadow_shipped_<shot>_<before|
/// after>.png`): before = GI off, Environment Intensity 1.0 (the scene until now); after = the
/// scene as saved. High quality level, 1280×720.
#[test]
#[ignore = "writes renders into docs/screenshots/gfx; run on purpose"]
fn misty_gi_scene_meadow_shipped() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (1280, 720);
    let shots: [(ViewFn, &str); 2] = [
        (|| meadow_view(Shot::WideGolden), "wide"),
        (grove_view, "grove"),
    ];
    for (view, name) in shots {
        for label in ["before", "after"] {
            let mut v = view();
            wait_for_trees(&v);
            if label == "before" {
                v.post.misty_gi = MistyGiSettings::default();
                v.environment.settings.environment_lighting.intensity = 1.0;
            }
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &QualityLevel::high());
            let target = r.create_target(&device, w, h);
            let frame = v.frame();
            for _ in 0..90 {
                r.render(&device, &queue, &target, &frame);
            }
            let rgba = read_back(&device, &queue, &target);
            let path = screenshots().join(format!("gi_meadow_shipped_{name}_{label}.png"));
            image::RgbaImage::from_raw(w, h, rgba)
                .expect("size")
                .save(&path)
                .expect("png");
            eprintln!("wrote {}", path.display());
        }
    }
}
