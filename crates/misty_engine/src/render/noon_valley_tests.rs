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
    "hero", "sun", "away", "left", "forest", "down", "treeline", "inside", "inside2",
];

fn views() -> [View; 9] {
    [
        view("hero", 0.0, 2.0, Vec3::ZERO),
        // Yaw replaced by the sun's bearing; looking up at it.
        view("sun", 0.0, 32.0, Vec3::ZERO),
        view("away", 180.0, 1.0, Vec3::ZERO),
        view("left", -90.0, 1.0, Vec3::ZERO),
        view("forest", 62.0, 14.0, Vec3::ZERO),
        view("down", 10.0, -38.0, Vec3::new(0.0, -0.7, 0.0)),
        view("treeline", 30.0, 3.0, Vec3::new(40.0, 0.0, 200.0)),
        // Inside the wood along the meadow's far edge: among the trunks, the canopy overhead.
        view("inside", 70.0, -8.0, Vec3::new(-60.0, 0.0, 530.0)),
        view("inside2", -25.0, 2.0, Vec3::new(-160.0, 0.0, 570.0)),
    ]
}

fn scene() -> Scene {
    crate::assets::set_project_root(&root());
    let (s, _) = Scene::load(&root().join(SCENE)).expect("NoonValley.scene");
    s
}

/// The scene's view re-posed for `name` (None = unknown name).
pub(in crate::render) fn noon_view(name: &str) -> Option<GameView> {
    noon_view_with(name, |_| {})
}

/// [`noon_view`] of the scene after `edit`.
fn noon_view_with(name: &str, edit: impl Fn(&mut Scene)) -> Option<GameView> {
    // `pose` (in MISTY_NOON_VIEWS) with `MISTY_NOON_POSE=yaw,pitch,right,ahead[,up]`: any spot
    // (`up` metres above the ground there).
    let pose = (name == "pose")
        .then(|| std::env::var("MISTY_NOON_POSE").ok())
        .flatten()
        .map(|s| {
            s.split(',')
                .filter_map(|v| v.trim().parse::<f32>().ok())
                .collect::<Vec<_>>()
        })
        .filter(|p| p.len() >= 4)
        .map(|p| {
            view(
                "pose",
                p[0],
                p[1],
                Vec3::new(p[2], p.get(4).copied().unwrap_or(0.0), p[3]),
            )
        });
    let v = pose.or_else(|| views().into_iter().find(|v| v.name == name))?;
    let mut scene = scene();
    edit(&mut scene);
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
    let mut eye =
        gv.camera.position + flat_right * v.offset.x + Vec3::Y * v.offset.y + Vec3::Z * v.offset.z;
    if v.offset.x != 0.0 || v.offset.z != 0.0 {
        // Stand on the ground there (eye height as at the start).
        let start_ground = ground_at(&gv, gv.camera.position);
        if name.starts_with("inside") {
            eye = clear_spot(&gv, eye);
        }
        let here = ground_at(&gv, eye);
        eye.y += here - start_ground;
    }
    gv.camera.position = eye;
    gv.camera.rotation = Quat::from_mat3(&Mat3::from_cols(right, up, fwd));
    // `MISTY_NOON_GRASS=density:500,lod:5,draw:140,width:0.05,billboards:60`: the grass (and
    // tree rendering) with these settings (cost against look, without regenerating the terrain).
    if let Ok(edits) = std::env::var("MISTY_NOON_GRASS") {
        let t = &mut gv.terrains[0];
        let mut d = (*t.data).clone();
        // `treekeep:0.5,treefrom:150`: keep that share of the trees farther than `treefrom`
        // metres (default 150), the kept ones bigger by 1/√keep (the canopy as closed).
        let num = |key: &str| {
            (edits.split(','))
                .filter_map(|e| e.split_once(':'))
                .find(|(k, _)| *k == key)
                .and_then(|(_, v)| v.parse::<f32>().ok())
        };
        if let Some(keep) = num("treekeep") {
            let from = num("treefrom").unwrap_or(150.0);
            let (pos, size) = (t.position, d.size);
            let eye = gv.camera.position;
            let grow = 1.0 / keep.max(0.01).sqrt();
            let mut n = 0u32;
            d.trees.instances.retain_mut(|i| {
                n = n.wrapping_add(1);
                let w =
                    glam::Vec2::new(pos.x + i.position.x * size.x, pos.z + i.position.y * size.z);
                if w.distance(glam::Vec2::new(eye.x, eye.z)) < from {
                    return true;
                }
                let h = (n.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32;
                i.height_scale *= grow.sqrt();
                i.width_scale *= grow;
                h < keep
            });
        }
        for e in edits.split(',') {
            let Some((k, v)) = e.split_once(':') else {
                continue;
            };
            let Ok(v) = v.parse::<f32>() else { continue };
            match k {
                "density" => d.details.density = v,
                "lod" => d.details.lod_distance = v,
                "draw" => d.details.draw_distance = v,
                "width" => d.details.width = v,
                "shadows" => d.details.cast_shadows = v > 0.5,
                "flowers" => d.flowers.lod_distance = v,
                "billboards" => d.trees.billboard_start = v,
                "treelod" => d.trees.lod_distance = v,
                "shadowdetail" => d.trees.shadow_detail = v,
                "treefade" => d.trees.lod_fade = v,
                "treekeep" | "treefrom" => {}
                _ => eprintln!("(MISTY_NOON_GRASS: {k} unknown)"),
            }
        }
        t.key = format!("{}#grass{edits}", t.key);
        t.data = std::sync::Arc::new(d);
    }
    // `MISTY_NOON_AE=off`: eye adaptation off (the before / after shots).
    if std::env::var("MISTY_NOON_AE").is_ok_and(|v| v == "off") {
        gv.post.auto_exposure.active = false;
    }
    Some(gv)
}

/// The spot within 12 m of `p` (XZ) farthest from any tree or bush (up to 2.5 m clear; nearer
/// spots win ties): inside the wood the camera stands between the trunks, a big one close by
/// as in the reference, not in a bush.
fn clear_spot(gv: &GameView, p: Vec3) -> Vec3 {
    let t = &gv.terrains[0];
    let d = &t.data;
    let small: Vec<bool> = (d.trees.prototypes.iter())
        .map(|p| p.tree.contains("Fern") || p.tree.contains("Groundcover"))
        .collect();
    let trunks: Vec<glam::Vec2> = (d.trees.instances.iter())
        .filter(|i| !small.get(i.prototype as usize).copied().unwrap_or(false))
        .map(|i| {
            glam::Vec2::new(
                t.position.x + i.position.x * d.size.x,
                t.position.z + i.position.y * d.size.z,
            )
        })
        .filter(|q| q.distance(glam::Vec2::new(p.x, p.z)) < 20.0)
        .collect();
    let mut best = (f32::MIN, p);
    for iz in -12..=12 {
        for ix in -12..=12 {
            let o = glam::Vec2::new(ix as f32, iz as f32);
            if o.length() > 12.0 {
                continue;
            }
            let q = glam::Vec2::new(p.x, p.z) + o;
            let clear = trunks.iter().map(|t| t.distance(q)).fold(2.5f32, f32::min);
            let score = clear - 0.02 * o.length();
            if score > best.0 {
                best = (score, Vec3::new(q.x, p.y, q.y));
            }
        }
    }
    best.1
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

/// GPU tree culling with and without its occlusion test, and off (CPU lists), interleaved
/// round by round so the machine's load affects all alike (`MISTY_AB_ROUNDS`, default 3;
/// `MISTY_NOON_VIEWS`, default `hero,inside`; `MISTY_QUALITY`, default `high`): median GPU
/// frame, CPU render, triangles and the tree parts per mode.
#[test]
#[ignore = "timing, prints only"]
fn noon_valley_cull_ab() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::render::fast_paths::set("tree_gpu_cull", true);
            crate::render::fast_paths::set("tree_occlusion", false);
        }
    }
    let _restore = Restore;
    let (w, h) = size();
    let rounds: usize = std::env::var("MISTY_AB_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let names = std::env::var("MISTY_NOON_VIEWS").unwrap_or_else(|_| "hero,inside".into());
    const PARTS: [&str; 4] = [
        "Tree culling",
        "Shadow maps (cascades, local)",
        "Screen-space prepass",
        "Main: trees",
    ];
    if let Some(v) = noon_view("hero") {
        breakdown(&device, &queue, &v, &QualityLevel::low(), (w, h));
    }
    let median = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    for name in names.split(',') {
        let Some(view) = noon_view(name) else {
            continue;
        };
        for (level, quality) in levels("high") {
            // [mode][gpu, cpu, parts..., M triangles]
            let mut runs: [Vec<Vec<f32>>; 3] = [Vec::new(), Vec::new(), Vec::new()];
            for _ in 0..rounds {
                for (mode, gpu, occlusion) in [(0, true, true), (1, true, false), (2, false, false)]
                {
                    crate::render::fast_paths::set("tree_gpu_cull", gpu);
                    crate::render::fast_paths::set("tree_occlusion", occlusion);
                    let b = breakdown(&device, &queue, &view, &quality, (w, h));
                    let mut row = vec![b.gpu, b.cpu];
                    row.extend(PARTS.iter().map(|p| b.part(p)));
                    row.push(b.stats.triangles as f32 / 1e6);
                    runs[mode].push(row);
                }
            }
            for (mode, label) in [
                (0, "gpu cull + occlusion"),
                (1, "gpu cull, no occlusion"),
                (2, "cpu lists"),
            ] {
                let col = |i: usize| median(runs[mode].iter().map(|r| r[i]).collect());
                eprintln!(
                    "== ab {name} {level} {w}x{h} {label}: GPU {:.2} ms, CPU render {:.2} ms, {:.1} M triangles (counted)",
                    col(0),
                    col(1),
                    col(2 + PARTS.len())
                );
                for (i, p) in PARTS.iter().enumerate() {
                    eprintln!("   {p:<32} {:6.2} ms", col(2 + i));
                }
            }
        }
    }
}

/// Light ▸ Intensity from Sun Height before / after: the hero view with the sun at a few
/// heights, the option off and on, written as `sun_<off|on>_<degrees>.png` into
/// `MISTY_NOON_DIR` (High, `MISTY_SIZE`, default 960×540).
#[test]
#[ignore = "writes comparison renders; run on purpose"]
fn noon_valley_sun_heights() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = PathBuf::from(std::env::var("MISTY_NOON_DIR").expect("MISTY_NOON_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h) = std::env::var("MISTY_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((960, 540));
    for on in [false, true] {
        for elevation in [6.0f32, 15.0, 35.0, 72.0] {
            let view = noon_view_with("hero", |s| {
                let sun = s.find_by_name("Directional Light").expect("sun");
                s.set_transform(
                    sun,
                    crate::scene::Transform::from_position(Vec3::new(0.0, 200.0, 0.0))
                        .with_euler(Vec3::new(elevation, 25.0, 0.0)),
                );
                s.world
                    .get::<&mut crate::scene::Light>(sun)
                    .unwrap()
                    .intensity_from_sun_height = on;
            })
            .expect("hero");
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &QualityLevel::high());
            let target = r.create_view_target(&device, w, h);
            let frame = view.frame();
            for _ in 0..48 {
                r.render(&device, &queue, &target, &frame);
            }
            let img = image::RgbaImage::from_raw(w, h, read_back(&device, &queue, &target))
                .expect("read back");
            let name = format!("sun_{}_{elevation:.0}.png", if on { "on" } else { "off" });
            img.save(dir.join(&name)).unwrap();
            eprintln!("wrote {name}");
        }
    }
}

/// Tree count scaling: the hero view with the terrain's trees copied (jittered) up to
/// `MISTY_NOON_TREES` (default 250 000), GPU and CPU times on High (`MISTY_QUALITY`), with the
/// GPU tree culling on and (`MISTY_FAST_OFF=tree_gpu_cull`) off.
#[test]
#[ignore = "timing, prints only"]
fn noon_valley_stress() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let want: usize = std::env::var("MISTY_NOON_TREES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250_000);
    let mut view = noon_view("hero").expect("hero");
    let t = &mut view.terrains[0];
    let mut d = (*t.data).clone();
    let base = d.trees.instances.clone();
    let mut k = 0u32;
    while d.trees.instances.len() < want {
        let src = base[k as usize % base.len()];
        k += 1;
        let jx = crate::terrain::noise::hash(k as i32, 1, 0x57e5) - 0.5;
        let jz = crate::terrain::noise::hash(k as i32, 2, 0x57e5) - 0.5;
        let mut i = src;
        i.position += glam::Vec2::new(jx, jz) * (6.0 / d.size.x);
        i.position = i
            .position
            .clamp(glam::Vec2::splat(0.0), glam::Vec2::splat(1.0));
        d.trees.instances.push(i);
    }
    let n = d.trees.instances.len();
    t.key = format!("{}#stress{n}", t.key);
    t.data = std::sync::Arc::new(d);
    let (w, h) = size();
    for (level, quality) in levels("high") {
        let b = breakdown(&device, &queue, &view, &quality, (w, h));
        b.print(&format!("noon stress {n} trees {level} {w}x{h}"));
    }
}

/// GPU tree culling draws the same trees as the CPU lists: MeadowDemo's close oak and the
/// trees behind the player (meshes, impostors and their shadows) rendered both ways differ by
/// at most render-to-render noise.
#[test]
fn tree_gpu_cull_matches_the_cpu_lists() {
    use crate::render::trees::gpu_tests::{Shot as TreeShot, meadow_view};
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::render::fast_paths::set("tree_gpu_cull", true);
        }
    }
    let _restore = Restore;
    let (w, h) = (480, 270);
    for shot in [TreeShot::Close, TreeShot::Behind] {
        let view = meadow_view(shot);
        let mut images = Vec::new();
        for gpu in [false, true] {
            crate::render::fast_paths::set("tree_gpu_cull", gpu);
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &QualityLevel::medium());
            let target = r.create_target(&device, w, h);
            let frame = view.frame();
            for _ in 0..24 {
                r.render(&device, &queue, &target, &frame);
            }
            images.push(read_back(&device, &queue, &target));
        }
        let (a, b) = (&images[0], &images[1]);
        let mut sum = 0u64;
        let mut big = 0usize;
        for (x, y) in a.iter().zip(b) {
            let d = x.abs_diff(*y) as u64;
            sum += d;
            big += usize::from(d > 24);
        }
        let mean = sum as f64 / a.len() as f64;
        let share = big as f64 / a.len() as f64;
        eprintln!(
            "{}: mean {mean:.3}, {:.3} % over 24 levels",
            shot.name(),
            share * 100.0
        );
        assert!(
            mean < 1.0 && share < 0.01,
            "{}: mean {mean}, share {share}",
            shot.name()
        );
    }
}

/// Pops while the camera dollies (run on purpose): the camera moves `MISTY_DOLLY_STEP` metres
/// (default -0.25: backwards) along its view each frame for `MISTY_DOLLY_FRAMES` frames (default
/// 72) at `MISTY_QUALITY` (default `high`) and `MISTY_SIZE` (default 1280×720), once with the
/// LOD cross-fades off (every switch instant, as before) and once on. The scene:
/// `MISTY_DOLLY_SCENE=noon` (default; the view from `MISTY_NOON_VIEWS`, default `treeline`,
/// `pose` works) or `meadow` (MeadowDemo, the trees behind the player).
///
/// A pop is a step change between two frames on top of the smooth change of the camera's
/// motion, so it shows in the second difference `f(t-1) - 2 f(t) + f(t+1)` of each pixel
/// while the parallax of a steady dolly mostly cancels. Printed per run: pixels whose second
/// difference exceeds 64 / 255 (any channel), in total and in the worst frame. With
/// `MISTY_DOLLY_DIR` every frame is written as `<off|on>_<frame>.png`.
#[test]
#[ignore = "renders a camera dolly; run on purpose"]
fn lod_fade_dolly() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::render::lod::set_fades(true);
        }
    }
    let _restore = Restore;
    let env = |k: &str| std::env::var(k).ok();
    let (w, h) = env("MISTY_SIZE")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1280, 720));
    let frames: usize = env("MISTY_DOLLY_FRAMES")
        .and_then(|v| v.parse().ok())
        .unwrap_or(72);
    let step: f32 = env("MISTY_DOLLY_STEP")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-0.25);
    let dir = env("MISTY_DOLLY_DIR").map(PathBuf::from);
    if let Some(d) = &dir {
        std::fs::create_dir_all(d).unwrap();
    }
    let meadow = env("MISTY_DOLLY_SCENE").is_some_and(|s| s == "meadow");
    let name = env("MISTY_NOON_VIEWS").unwrap_or_else(|| "treeline".into());
    let make = || {
        if meadow {
            crate::render::trees::gpu_tests::meadow_view(
                crate::render::trees::gpu_tests::Shot::Behind,
            )
        } else {
            noon_view(&name).expect("view")
        }
    };
    let (level, quality) = levels("high").remove(0);
    for fades in [false, true] {
        crate::render::lod::set_fades(fades);
        let mut gv = make();
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality);
        let target = r.create_view_target(&device, w, h);
        for _ in 0..48 {
            r.render(&device, &queue, &target, &gv.frame());
        }
        let f = gv.camera.forward();
        let along = Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
        let mut images: Vec<Vec<u8>> = Vec::with_capacity(frames);
        for i in 0..frames {
            let before = gv.camera.position;
            let mut eye = before + along * step;
            eye.y += ground_at(&gv, eye) - ground_at(&gv, before);
            gv.camera.position = eye;
            r.render(&device, &queue, &target, &gv.frame());
            let px = read_back(&device, &queue, &target);
            if let Some(d) = &dir {
                let tag = if fades { "on" } else { "off" };
                image::RgbaImage::from_raw(w, h, px.clone())
                    .expect("read back")
                    .save(d.join(format!("{tag}_{i:03}.png")))
                    .unwrap();
            }
            images.push(px);
        }
        let mut per_frame = Vec::with_capacity(frames);
        for t in 1..frames.saturating_sub(1) {
            let (a, b, c) = (&images[t - 1], &images[t], &images[t + 1]);
            let mut n = 0usize;
            for i in (0..a.len()).step_by(4) {
                let jump = (0..3)
                    .map(|k| (a[i + k] as i32 - 2 * b[i + k] as i32 + c[i + k] as i32).abs())
                    .max()
                    .unwrap_or(0);
                n += usize::from(jump > 64);
            }
            per_frame.push(n);
        }
        let total: usize = per_frame.iter().sum();
        let (worst_at, worst) = per_frame
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, n)| *n)
            .unwrap_or((0, 0));
        let scene = if meadow { "meadow behind" } else { name.as_str() };
        eprintln!(
            "== dolly {scene} {level} {w}x{h}, fades {}: pop pixels total {total}, worst frame {} \
             ({worst}), mean {:.0} per frame",
            if fades { "on" } else { "off" },
            worst_at + 1,
            total as f32 / per_frame.len().max(1) as f32
        );
        eprintln!("   per frame: {per_frame:?}");
    }
}
