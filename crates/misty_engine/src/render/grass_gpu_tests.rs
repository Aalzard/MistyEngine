//! MeadowDemo grass: comparison shots, GPU cost and ground visibility (all ignored; run on
//! purpose in release):
//! `cargo test --release -p misty_engine grass_ -- --ignored --nocapture --test-threads=1`
//! `MISTY_GFX_STEP=<name>` names the shots `docs/screenshots/gfx/grass_<name>_<shot>.png`.

use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec3;

use crate::quality::QualityLevel;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{RenderTarget, Renderer};
use crate::render_scene::GameView;
use crate::scene::{Light, Scene, Transform};
use crate::terrain::TerrainDraw;

const SCENE: &str = "Assets/Scenes/MeadowDemo.scene";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

#[derive(Clone, Copy, Debug)]
pub(in crate::render) enum Shot {
    /// The scene camera: standing in the meadow, low sun ahead (backlit).
    WideGolden,
    /// Crouching, looking down into the grass a few metres ahead, backlit.
    CloseGolden,
    /// The scene camera with a high sun behind the camera.
    WideMidday,
    CloseMidday,
}

impl Shot {
    pub const ALL: [Shot; 4] = [
        Shot::WideGolden,
        Shot::CloseGolden,
        Shot::WideMidday,
        Shot::CloseMidday,
    ];

    fn name(self) -> &'static str {
        match self {
            Shot::WideGolden => "wide_golden",
            Shot::CloseGolden => "close_golden",
            Shot::WideMidday => "wide_midday",
            Shot::CloseMidday => "close_midday",
        }
    }
}

/// MeadowDemo as saved, re-posed for `shot`.
pub(in crate::render) fn meadow_scene(shot: Shot) -> Scene {
    crate::assets::set_project_root(&root());
    let (mut s, _) = Scene::load(&root().join(SCENE)).expect("MeadowDemo.scene");
    let close = matches!(shot, Shot::CloseGolden | Shot::CloseMidday);
    let midday = matches!(shot, Shot::WideMidday | Shot::CloseMidday);
    // The camera that renders: the First Person Player's (the scene's own Main Camera is
    // inactive since the player was placed), else the Main Camera.
    let cam = s
        .find_by_name("Camera")
        .filter(|&e| s.world.get::<&crate::scene::Camera>(e).is_ok())
        .or_else(|| s.find_by_name("Main Camera"));
    if close && let Some(cam) = cam {
        // Crouching, looking down into the grass (positive pitch looks down). The player's
        // camera sits under unrotated head-bob parents: its local offset is a world offset.
        let local = s
            .world
            .get::<&Transform>(cam)
            .map(|t| t.position)
            .unwrap_or_default();
        s.set_transform(
            cam,
            Transform::from_position(local - Vec3::Y * 0.8).with_euler(Vec3::new(16.0, 8.0, 0.0)),
        );
    }
    if midday && let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 200.0, 0.0))
                .with_euler(Vec3::new(62.0, 35.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.97, 0.92];
        l.intensity = 2.6;
    }
    s
}

pub(in crate::render) fn meadow_view(shot: Shot) -> GameView {
    let scene = meadow_scene(shot);
    let materials = crate::assets::AssetCache::open(&root());
    let mut view = GameView::new(&scene, &materials).expect("camera");
    // A fixed wind clock: renders compared with each other show the same blades.
    view.environment.grass_time = Some(3.0);
    view
}

/// The view's terrains with `edit` applied to their data (own store key, so the renderer
/// keeps separate GPU data).
pub(super) fn edited_terrains(
    view: &GameView,
    tag: &str,
    edit: impl Fn(&mut crate::terrain::TerrainData),
) -> Vec<TerrainDraw> {
    view.terrains
        .iter()
        .map(|t| {
            let mut d = (*t.data).clone();
            edit(&mut d);
            TerrainDraw {
                key: format!("{}#{tag}", t.key),
                data: Arc::new(d),
                ..t.clone()
            }
        })
        .collect()
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    r: &mut Renderer,
    target: &RenderTarget,
    view: &GameView,
    terrains: Option<&[TerrainDraw]>,
    frames: usize,
) -> Vec<u8> {
    let mut frame = view.frame();
    if let Some(t) = terrains {
        frame.terrains = t;
    }
    for _ in 0..frames {
        r.render(device, queue, target, &frame);
    }
    read_back(device, queue, target)
}

/// Per pixel: the camera ray meets the terrain where grass grows, within the grass draw
/// distance (CPU ray march over the heightfield).
fn grass_ground(view: &GameView, (w, h): (u32, u32)) -> Vec<bool> {
    let cam = &view.camera;
    let inv = cam.view_projection(w as f32 / h as f32).inverse();
    let mut out = vec![false; (w * h) as usize];
    for t in &view.terrains {
        let d = &t.data;
        let n = d.alphamap_resolution;
        if !d.details.enabled || d.detail_map.len() != n * n {
            continue;
        }
        let reach = d.details.draw_distance;
        for y in 0..h {
            for x in 0..w {
                let ndc = glam::Vec2::new(
                    (x as f32 + 0.5) / w as f32 * 2.0 - 1.0,
                    1.0 - (y as f32 + 0.5) / h as f32 * 2.0,
                );
                let far = inv.project_point3(ndc.extend(1.0));
                let dir = (far - cam.position).normalize();
                if dir.y > 0.02 {
                    continue;
                }
                let mut dist = 0.05;
                while dist < reach {
                    let p = cam.position + dir * dist - t.position;
                    if p.x < 0.0 || p.z < 0.0 || p.x >= d.size.x || p.z >= d.size.z {
                        break;
                    }
                    if p.y <= d.sample_height(p.x, p.z) {
                        let tx = ((p.x / d.size.x * n as f32) as usize).min(n - 1);
                        let tz = ((p.z / d.size.z * n as f32) as usize).min(n - 1);
                        out[(y * w + x) as usize] |= d.detail_map[tz * n + tx] > 128;
                        break;
                    }
                    dist += (dist * 0.004).max(0.02);
                }
            }
        }
    }
    out
}

/// Share of the grass ground's pixels where the ground shows between blades, out of the
/// terrain pixels showing ground where grass grows within its draw distance (mountains and
/// the field beyond the grass stay out). Two measures: `.0` = pixels where the image with
/// grass matches the image without blades (same ground, same light; the original measure,
/// which also counts blades that happen to match the ground's colour now that the ground is
/// tinted like the grass), `.1` = pixels that change by more than 12 levels when only the
/// ground's colour changes (ground tint off: soil instead of grass colour), with the same
/// blades in front.
pub(super) fn ground_visibility(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    (w, h): (u32, u32),
    shot: &str,
) -> (f32, f32) {
    // Grass shadows on the ground would count as blades.
    let quality = QualityLevel {
        temporal_aa: false,
        grass_shadows: crate::quality::Tier::Off,
        ..quality.clone()
    };
    let bare = edited_terrains(view, "bare", |d| d.details.density = 0.0);
    let soil = edited_terrains(view, "soil", |d| d.details.ground_tint = 0.0);
    let none: Vec<TerrainDraw> = Vec::new();
    let mut images = Vec::new();
    for terrains in [None, Some(&bare[..]), Some(&none[..]), Some(&soil[..])] {
        let mut r = Renderer::with_queue(device, queue);
        r.set_quality(device, &quality);
        let target = r.create_target(device, w, h);
        // Chunks are scattered one per frame at full density: let them all arrive.
        images.push(render(device, queue, &mut r, &target, view, terrains, 48));
    }
    let diff = |a: &[u8], b: &[u8], i: usize| {
        (0..3)
            .map(|c| (a[i * 4 + c] as i32 - b[i * 4 + c] as i32).abs())
            .max()
            .unwrap()
    };
    let grass_ground = grass_ground(view, (w, h));
    let (mut terrain, mut seen, mut swapped) = (0usize, 0usize, 0usize);
    // Red = ground showing (matches the bare render), blue = the ground's colour shows
    // through (tint swap), green = covered by blades.
    let mut mask = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        mask[i * 4 + 3] = 255;
        if grass_ground[i] && diff(&images[1], &images[2], i) > 10 {
            terrain += 1;
            let mut covered = true;
            if diff(&images[0], &images[1], i) <= 4 {
                seen += 1;
                mask[i * 4] = 255;
                covered = false;
            }
            if diff(&images[0], &images[3], i) > 12 {
                swapped += 1;
                mask[i * 4 + 2] = 255;
                covered = false;
            }
            if covered {
                mask[i * 4 + 1] = 160;
            }
        }
    }
    if let Ok(name) = std::env::var("MISTY_GFX_MASK") {
        // The ground alone (no blades), to compare its colour with the blades'.
        let path = screenshots().join(format!("grass_{name}_{shot}_bare.png"));
        image::RgbaImage::from_raw(w, h, images[1].clone())
            .unwrap()
            .save(path)
            .unwrap();
        let path = screenshots().join(format!("grass_{name}_{shot}_ground_mask.png"));
        image::RgbaImage::from_raw(w, h, mask)
            .unwrap()
            .save(path)
            .unwrap();
    }
    (
        seen as f32 / terrain.max(1) as f32,
        swapped as f32 / terrain.max(1) as f32,
    )
}

#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn grass_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let step = std::env::var("MISTY_GFX_STEP").unwrap_or_else(|_| "latest".into());
    let only = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
    // `MISTY_QUALITY=high,medium,low` renders each level (files `grass_<step>_<level>_<shot>`);
    // unset = High only, named as before. `MISTY_GFX_DIR` writes somewhere else.
    let levels = std::env::var("MISTY_QUALITY").ok();
    let dir = std::env::var("MISTY_GFX_DIR").map_or_else(|_| screenshots(), PathBuf::from);
    let (w, h) = (1600, 900);
    for name in levels.as_deref().unwrap_or("high").split(',') {
        let Some(mut quality) = level(name) else {
            continue;
        };
        for shot in Shot::ALL {
            if !only.is_empty() && !shot.name().contains(&only) {
                continue;
            }
            let view = meadow_view(shot);
            let mut r = Renderer::with_queue(&device, &queue);
            if std::env::var("MISTY_GRASS_SHADOWS").is_ok_and(|v| v == "0") {
                quality.grass_shadows = crate::quality::Tier::Off;
            }
            r.set_quality(&device, &quality);
            let target = r.create_target(&device, w, h);
            let rgba = render(&device, &queue, &mut r, &target, &view, None, 48);
            let file = match levels {
                Some(_) => format!("grass_{step}_{name}_{}.png", shot.name()),
                None => format!("grass_{step}_{}.png", shot.name()),
            };
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            image::RgbaImage::from_raw(w, h, rgba)
                .unwrap()
                .save(&path)
                .unwrap();
            eprintln!("wrote {}", path.display());
        }
    }
}

/// Median GPU frame time of MeadowDemo at 1920×1080 per quality level, and the share of the
/// ground showing between blades in the main shot.
#[test]
#[ignore = "timing, prints only"]
fn grass_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = meadow_view(Shot::WideGolden);
    let no_grass = edited_terrains(&view, "nograss", |d| d.details.enabled = false);
    for (name, quality) in [
        ("high", QualityLevel::high()),
        ("medium", QualityLevel::medium()),
        ("low", QualityLevel::low()),
    ] {
        for (label, terrains) in [("grass", None), ("no grass", Some(&no_grass[..]))] {
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &quality);
            let target = r.create_target(&device, 1920, 1080);
            let mut frame = view.frame();
            if let Some(t) = terrains {
                frame.terrains = t;
            }
            let mut times = Vec::new();
            for i in 0..160 {
                r.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if i >= 40
                    && let Some(ms) = r.gpu_ms()
                {
                    times.push(ms);
                }
            }
            times.sort_by(f32::total_cmp);
            let median = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
            let q = |f: f32| times.get((times.len() as f32 * f) as usize).copied();
            let tris = r.stats().triangles as f64 / 1e6;
            eprintln!(
                "{name:>6} {label:>8}: {median:.2} ms GPU (p25 {:.2}, p75 {:.2}), {tris:.1} M triangles",
                q(0.25).unwrap_or(f32::NAN),
                q(0.75).unwrap_or(f32::NAN)
            );
        }
    }
    if std::env::var("MISTY_GRASS_VARIANTS").is_ok() {
        let mut plain = meadow_view(Shot::WideGolden);
        for l in &mut plain.lights {
            l.contact_shadows = false;
        }
        let high = QualityLevel::high();
        let no_map = QualityLevel {
            grass_shadow_map: crate::quality::Tier::Off,
            ..high.clone()
        };
        let no_shadows = QualityLevel {
            grass_shadows: crate::quality::Tier::Off,
            ..no_map.clone()
        };
        let no_ao = QualityLevel {
            ambient_occlusion: crate::quality::Tier::Off,
            ..high.clone()
        };
        for (label, v, q) in [
            ("high", &view, &high),
            ("no contact shadows", &plain, &high),
            ("no near map", &view, &no_map),
            ("no grass shadows at all", &view, &no_shadows),
            ("no AO (no prepass grass)", &view, &no_ao),
        ] {
            let ms = gpu_median(&device, &queue, v, q, None);
            eprintln!("variant {label}: {ms:.2} ms GPU");
        }
    }
    let seen = ground_visibility(
        &device,
        &queue,
        &view,
        &QualityLevel::high(),
        (960, 540),
        "wide",
    );
    eprintln!(
        "ground visible (wide golden, high): {:.1}% (matches bare), {:.1}% (ground colour shows)",
        seen.0 * 100.0,
        seen.1 * 100.0
    );
    let close = meadow_view(Shot::CloseGolden);
    let seen = ground_visibility(
        &device,
        &queue,
        &close,
        &QualityLevel::high(),
        (960, 540),
        "close",
    );
    eprintln!(
        "ground visible (close golden, high): {:.1}% (matches bare), {:.1}% (ground colour shows)",
        seen.0 * 100.0,
        seen.1 * 100.0
    );
}

/// MeadowDemo crouched in the grass with a low golden sun from the right and a cube a few
/// metres ahead, its shadow falling across the grass.
fn shadow_proof_view() -> GameView {
    let mut s = meadow_scene(Shot::WideGolden);
    let cam = s.find_by_name("Main Camera").expect("camera");
    let eye = s.world_position(cam) - Vec3::Y * 0.7;
    // (positive pitch looks down)
    s.set_transform(
        cam,
        Transform::from_position(eye).with_euler(Vec3::new(14.0, 0.0, 0.0)),
    );
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 200.0, 0.0))
                .with_euler(Vec3::new(30.0, 245.0, 0.0)),
        );
    }
    // No cloud shadow over the spot (the proof is about the grass's own shadows).
    if let Some(fx) = s.find_by_name("Global Volume")
        && let Ok(mut v) = s.world.get::<&mut crate::scene::PostProcessVolume>(fx)
    {
        v.clouds.shadow_opacity = 0.0;
    }
    let materials = crate::assets::AssetCache::open(&root());
    let mut view = GameView::new(&s, &materials).expect("camera");
    let ground = view
        .terrains
        .first()
        .map(|t| {
            t.position.y
                + t.data
                    .sample_height(eye.x - 0.6 - t.position.x, eye.z + 3.5 - t.position.z)
        })
        .unwrap_or(0.0);
    let mut meshes = (*view.meshes).clone();
    meshes.push(crate::render::MeshDraw {
        mesh: crate::scene::PrimitiveMesh::Cube.into(),
        model: glam::Mat4::from_scale_rotation_translation(
            Vec3::splat(0.8),
            glam::Quat::from_rotation_y(0.4),
            Vec3::new(eye.x - 0.4, ground + 0.36, eye.z + 3.5),
        ),
        material: crate::render::MaterialParams::default(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    });
    view.meshes = Arc::new(meshes);
    view
}

/// `docs/screenshots/gfx/grass_shadows_on_vs_off.png`: the same close golden-hour shot with
/// grass shadows off (left) and on (right), and the GPU cost of the casters at 1920×1080.
#[test]
#[ignore = "writes a comparison render into docs/screenshots/gfx; run on purpose"]
fn grass_shadow_proof() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = shadow_proof_view();
    let (w, h) = (960, 720);
    use crate::quality::Tier;
    let setups = [
        ("off", Tier::Off, Tier::Off),
        ("cascades", Tier::High, Tier::Off),
        ("near map", Tier::Medium, Tier::Medium),
    ];
    let mut panels = Vec::new();
    for (_, casters, map) in setups {
        let mut r = Renderer::with_queue(&device, &queue);
        let quality = QualityLevel {
            grass_shadows: casters,
            grass_shadow_map: map,
            ..QualityLevel::high()
        };
        r.set_quality(&device, &quality);
        let target = r.create_target(&device, w, h);
        panels.push(render(&device, &queue, &mut r, &target, &view, None, 48));
    }
    let strip = |names: &[usize], file: &str| {
        let mut out =
            image::RgbaImage::new(w * names.len() as u32 + 8 * (names.len() as u32 - 1), h);
        for (k, &i) in names.iter().enumerate() {
            let img = image::RgbaImage::from_raw(w, h, panels[i].clone()).unwrap();
            image::imageops::replace(&mut out, &img, (k as u32 * (w + 8)) as i64, 0);
        }
        let path = screenshots().join(file);
        out.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
    };
    // `MISTY_GFX_STEP=<name>` names the files `grass_<name>_shadows_*.png`.
    let prefix =
        std::env::var("MISTY_GFX_STEP").map_or(String::from("grass_"), |s| format!("grass_{s}_"));
    // Off | on (High: cascades + the near map).
    strip(&[0, 2], &format!("{prefix}shadows_on_vs_off.png"));
    // The previous step (cascade casters only) | the near map.
    strip(&[1, 2], &format!("{prefix}shadows_near_map.png"));
    // Cost at 1080p of the main shot, High with grass shadows off / on (a first run warms
    // the GPU clocks up).
    let view = meadow_view(Shot::WideGolden);
    gpu_median(&device, &queue, &view, &QualityLevel::high(), None);
    for (casters, map) in [
        (Tier::Off, Tier::Off),
        (Tier::Medium, Tier::Off),
        (Tier::Medium, Tier::Low),
        (Tier::Medium, Tier::Medium),
        (Tier::Medium, Tier::High),
        (Tier::High, Tier::High),
    ] {
        let quality = QualityLevel {
            grass_shadows: casters,
            grass_shadow_map: map,
            ..QualityLevel::high()
        };
        let ms = gpu_median(&device, &queue, &view, &quality, None);
        eprintln!(
            "high, grass casters {}, near map {} ({} px): {ms:.2} ms GPU",
            casters.label(),
            map.label(),
            quality.grass_shadow_map_size()
        );
    }
}

/// Median GPU frame time of `view` at 1920×1080 over 120 frames (after 40 to settle).
fn gpu_median(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    terrains: Option<&[TerrainDraw]>,
) -> f32 {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_target(device, 1920, 1080);
    let mut frame = view.frame();
    if let Some(t) = terrains {
        frame.terrains = t;
    }
    let mut times = Vec::new();
    for i in 0..160 {
        r.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if i >= 40
            && let Some(ms) = r.gpu_ms()
        {
            times.push(ms);
        }
    }
    times.sort_by(f32::total_cmp);
    times.get(times.len() / 2).copied().unwrap_or(f32::NAN)
}

/// One measured setup: median GPU frame time, median CPU time of `Renderer::render`, the
/// median GPU time of each part of the frame, and the last render's counters.
pub(in crate::render) struct Breakdown {
    pub gpu: f32,
    pub cpu: f32,
    pub parts: Vec<(&'static str, f32)>,
    pub stats: crate::render::RenderStats,
}

impl Breakdown {
    pub fn part(&self, label: &str) -> f32 {
        self.parts
            .iter()
            .find(|(l, _)| *l == label)
            .map_or(0.0, |p| p.1)
    }

    pub fn print(&self, name: &str) {
        eprintln!(
            "== {name}: GPU {:.2} ms, CPU render {:.2} ms, {:.1} M triangles, {} draws",
            self.gpu,
            self.cpu,
            self.stats.triangles as f64 / 1e6,
            self.stats.batches
        );
        let mut sum = 0.0;
        for (label, m) in &self.parts {
            sum += m;
            if *m >= 0.05 {
                eprintln!("   {label:<32} {m:6.2} ms");
            }
        }
        eprintln!("   {:<32} {sum:6.2} ms", "(sum of parts)");
    }
}

/// Measure `view` at `size` with `quality`: 60 frames to settle, 60 timed as a whole, 60
/// timed per part (pass timestamps).
pub(in crate::render) fn breakdown(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    (w, h): (u32, u32),
) -> Breakdown {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_target(device, w, h);
    let frame = view.frame();
    let mut totals = Vec::new();
    let mut cpu = Vec::new();
    for i in 0..120 {
        r.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if i >= 60 {
            if let Some(ms) = r.gpu_ms() {
                totals.push(ms);
            }
            cpu.push(r.stats().cpu_ms);
        }
    }
    let stats = r.stats();
    let mut parts: Vec<(&'static str, Vec<f32>)> = Vec::new();
    if r.set_pass_timing(device, queue, true) {
        for _ in 0..60 {
            r.render(device, queue, &target, &frame);
            let mut seen: Vec<(&'static str, f32)> = Vec::new();
            for &(label, ms) in r.pass_times() {
                match seen.iter_mut().find(|(l, _)| *l == label) {
                    Some(s) => s.1 += ms,
                    None => seen.push((label, ms)),
                }
            }
            for (label, ms) in seen {
                match parts.iter_mut().find(|(l, _)| *l == label) {
                    Some(p) => p.1.push(ms),
                    None => parts.push((label, vec![ms])),
                }
            }
        }
    }
    let median = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
    };
    Breakdown {
        gpu: median(totals),
        cpu: median(cpu),
        parts: parts.into_iter().map(|(l, v)| (l, median(v))).collect(),
        stats,
    }
}

fn level(name: &str) -> Option<QualityLevel> {
    Some(match name {
        "high" => QualityLevel::high(),
        "medium" => QualityLevel::medium(),
        "low" => QualityLevel::low(),
        "ultra" => QualityLevel::ultra(),
        _ => return None,
    })
}

/// GPU time per part of the frame (pass timestamps) and the CPU time of `Renderer::render`,
/// MeadowDemo per quality level at 1920×1080 (`MISTY_SIZE=1125x558` another size, e.g. the
/// editor's docked Game view; `MISTY_SCALED=1`: at the level's Render Scale, as the editor
/// and player draw it; `MISTY_GFX_SHOT=close` for the close shot; `MISTY_QUALITY=high,low`
/// picks levels).
#[test]
#[ignore = "timing, prints only"]
fn grass_pass_times() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let shot = match std::env::var("MISTY_GFX_SHOT").as_deref() {
        Ok("close") => Shot::CloseGolden,
        _ => Shot::WideGolden,
    };
    let view = meadow_view(shot);
    let levels = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high,medium,low".into());
    // A first run warms the GPU clocks up.
    breakdown(&device, &queue, &view, &QualityLevel::low(), (1920, 1080));
    for name in levels.split(',') {
        let Some(quality) = level(name) else {
            continue;
        };
        let scale = match std::env::var("MISTY_SCALED") {
            Ok(_) => quality.render_scale,
            Err(_) => 1.0,
        };
        let (w, h) = std::env::var("MISTY_SIZE")
            .ok()
            .and_then(|s| {
                let (w, h) = s.split_once('x')?;
                Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
            })
            .unwrap_or((1920.0, 1080.0));
        let size = ((w * scale) as u32, (h * scale) as u32);
        let b = breakdown(&device, &queue, &view, &quality, size);
        b.print(&format!("{name} {}x{}", size.0, size.1));
    }
}

/// The grass passes of MeadowDemo (High, 1920×1080) with one feature at a time turned off:
/// where the lit grass's time goes.
#[test]
#[ignore = "timing, prints only"]
fn grass_variants() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    use crate::quality::Tier;
    let view = meadow_view(Shot::WideGolden);
    let base = level(&std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high".into()))
        .unwrap_or_else(QualityLevel::high);
    let mut unshadowed = meadow_view(Shot::WideGolden);
    for l in &mut unshadowed.lights {
        l.shadows = crate::scene::Shadows::None;
    }
    let variants: Vec<(&str, &GameView, QualityLevel)> = vec![
        ("base", &view, base.clone()),
        (
            "no near grass map",
            &view,
            QualityLevel {
                grass_shadow_map: Tier::Off,
                ..base.clone()
            },
        ),
        ("sun without shadows", &unshadowed, base.clone()),
        (
            "AO off",
            &view,
            QualityLevel {
                ambient_occlusion: Tier::Off,
                ..base.clone()
            },
        ),
        (
            "volumetric fog off",
            &view,
            QualityLevel {
                volumetric_fog: Tier::Off,
                sun_shafts: Tier::Off,
                ..base.clone()
            },
        ),
        (
            "TAA off",
            &view,
            QualityLevel {
                temporal_aa: false,
                ..base.clone()
            },
        ),
        (
            "grass density 0.5",
            &view,
            QualityLevel {
                grass_density: 0.5,
                ..base.clone()
            },
        ),
        (
            "soft shadows off",
            &view,
            QualityLevel {
                soft_shadows: Tier::Off,
                shadow_temporal: false,
                shadow_denoise: false,
                ..base.clone()
            },
        ),
    ];
    breakdown(&device, &queue, &view, &base, (1920, 1080));
    for (name, v, q) in variants {
        let b = breakdown(&device, &queue, v, &q, (1920, 1080));
        eprintln!(
            "{name:<24} frame {:6.2}  terrain {:5.2}  grass depth {:5.2}  grass lit {:5.2}  near map {:5.2}  prepass {:5.2}  {:5.1} M tris",
            b.gpu,
            b.part("Main: terrain"),
            b.part("Main: grass depth"),
            b.part("Main: grass lit"),
            b.part("Grass near shadow map"),
            b.part("AO prepass + GTAO"),
            b.stats.triangles as f64 / 1e6
        );
    }
}
