//! NoonValley: the fixed views every look change is judged on, and its GPU cost (all ignored;
//! run on purpose in release):
//!
//! - `noon_valley_shots`: every view (`MISTY_NOON_VIEWS=hero,sun,…`, default all) at every
//!   level of `MISTY_QUALITY` (default `high`), `MISTY_SIZE` (default 1920×1080), 64 frames,
//!   written as `<level>_<view>.png` into `MISTY_NOON_DIR`.
//! - `noon_valley_times`: median GPU frame time and the GPU time of every part, per view and
//!   level (`MISTY_NOON_VIEWS`, default `hero,away`; `MISTY_QUALITY`, default `high,ultra`).

use std::path::PathBuf;

use glam::{Mat3, Quat, Vec3};

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::terrain_grass::gpu_tests::breakdown;
use crate::render_scene::GameView;
use crate::scene::Scene;

const SCENE: &str = "Assets/Scenes/NoonValley.scene";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// A fixed view: yaw (degrees right of the scene camera's +Z), pitch (degrees up), the eye
/// moved (right, up, ahead) metres from the scene camera.
struct View {
    name: &'static str,
    yaw: f32,
    pitch: f32,
    offset: Vec3,
}

const fn view(name: &'static str, yaw: f32, pitch: f32, offset: Vec3) -> View {
    View {
        name,
        yaw,
        pitch,
        offset,
    }
}

/// The views: the reference composition, towards the sun, away from it, both sides, up at the
/// forest and cliffs, down into the grass, and along the treeline from closer in.
pub(in crate::render) const VIEWS: &[&str] = &[
    "hero", "sun", "away", "left", "forest", "down", "treeline",
];

fn views() -> [View; 7] {
    [
        view("hero", 0.0, 2.0, Vec3::ZERO),
        // Yaw replaced by the sun's bearing; looking up at it.
        view("sun", 0.0, 32.0, Vec3::ZERO),
        view("away", 180.0, 1.0, Vec3::ZERO),
        view("left", -90.0, 1.0, Vec3::ZERO),
        view("forest", 62.0, 14.0, Vec3::ZERO),
        view("down", 10.0, -38.0, Vec3::new(0.0, -0.7, 0.0)),
        view("treeline", 30.0, 3.0, Vec3::new(40.0, 0.0, 200.0)),
    ]
}

fn scene() -> Scene {
    crate::assets::set_project_root(&root());
    let (s, _) = Scene::load(&root().join(SCENE)).expect("NoonValley.scene");
    s
}

/// The scene's view re-posed for `name` (None = unknown name).
pub(in crate::render) fn noon_view(name: &str) -> Option<GameView> {
    let v = views().into_iter().find(|v| v.name == name)?;
    let scene = scene();
    let materials = crate::assets::AssetCache::open(&root());
    let mut gv = GameView::new(&scene, &materials).expect("camera");
    gv.environment.grass_time = Some(3.0);
    let mut yaw = v.yaw;
    if name == "sun" {
        let sun = scene.find_by_name("Directional Light").expect("sun");
        let to_sun = -scene.world_matrix(sun).transform_vector3(Vec3::Z);
        yaw = to_sun.x.atan2(to_sun.z).to_degrees();
    }
    let (yaw, pitch) = (yaw.to_radians(), v.pitch.to_radians());
    let fwd = Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    let right = Vec3::Y.cross(fwd).normalize();
    let up = fwd.cross(right);
    let flat_right = Vec3::new(1.0, 0.0, 0.0);
    let mut eye = gv.camera.position
        + flat_right * v.offset.x
        + Vec3::Y * v.offset.y
        + Vec3::Z * v.offset.z;
    if v.offset.x != 0.0 || v.offset.z != 0.0 {
        // Stand on the ground there (eye height as at the start).
        let start_ground = ground_at(&gv, gv.camera.position);
        let here = ground_at(&gv, eye);
        eye.y += here - start_ground;
    }
    gv.camera.position = eye;
    gv.camera.rotation = Quat::from_mat3(&Mat3::from_cols(right, up, fwd));
    Some(gv)
}

/// Terrain height under world `p` (the first terrain).
fn ground_at(gv: &GameView, p: Vec3) -> f32 {
    let t = &gv.terrains[0];
    t.position.y + t.data.sample_height(p.x - t.position.x, p.z - t.position.z)
}

fn levels(default: &str) -> Vec<(String, QualityLevel)> {
    let names = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| default.into());
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
#[ignore = "writes look-check renders; run on purpose"]
fn noon_valley_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = PathBuf::from(std::env::var("MISTY_NOON_DIR").expect("MISTY_NOON_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let names = std::env::var("MISTY_NOON_VIEWS").unwrap_or_else(|_| VIEWS.join(","));
    let (w, h) = size();
    for (level, quality) in levels("high") {
        for name in names.split(',') {
            let Some(view) = noon_view(name) else {
                eprintln!("(view {name} unknown)");
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
            let path = dir.join(format!("{level}_{name}.png"));
            img.save(&path).unwrap();
            eprintln!("wrote {}", path.display());
        }
    }
}

#[test]
#[ignore = "timing, prints only"]
fn noon_valley_times() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = size();
    let names = std::env::var("MISTY_NOON_VIEWS").unwrap_or_else(|_| "hero,away".into());
    // A first run warms the GPU clocks up.
    if let Some(v) = noon_view("hero") {
        breakdown(&device, &queue, &v, &QualityLevel::low(), (w, h));
    }
    for name in names.split(',') {
        let Some(v) = noon_view(name) else {
            continue;
        };
        for (level, quality) in levels("high,ultra") {
            let b = breakdown(&device, &queue, &v, &quality, (w, h));
            b.print(&format!(
                "noon {name} {level} {w}x{h} ({:.0} FPS)",
                1000.0 / b.gpu
            ));
        }
    }
}
