//! Misty GI on the sample scenes (all ignored; run on purpose):
//! `cargo test -p misty_engine misty_gi_scene -- --ignored --nocapture --test-threads=1`
//!
//! - `misty_gi_scene_pass_times`: GPU time per stage at 1920×1080 (High quality level) on
//!   CyclesRoomDemo and MeadowDemo, Misty GI off / Low / Medium / High.
//! - `misty_gi_scene_meadow_shots`: MeadowDemo with and without Misty GI
//!   (`docs/screenshots/gfx/gi_meadow_<shot>_<off|misty>.png`), including a "grove" shot from
//!   beside the tree nearest the scene camera (sky occlusion under the canopy).

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
    let mut v = meadow_view(Shot::WideMidday);
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
    let shots: [(ViewFn, &str, f32); 4] = [
        (grove_view, "grove", 1.0),
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
