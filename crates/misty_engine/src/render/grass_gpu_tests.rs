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
    if close && let Some(cam) = s.find_by_name("Main Camera") {
        // Crouching, looking down into the grass (positive pitch looks down).
        let p = s.world_position(cam);
        s.set_transform(
            cam,
            Transform::from_position(p - Vec3::Y * 0.8).with_euler(Vec3::new(16.0, 8.0, 0.0)),
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
    GameView::new(&scene, &materials).expect("camera")
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

/// Share of the grass ground's pixels where the ground shows between blades: pixels where the
/// image with grass matches the image without blades (same ground, same light), out of the
/// terrain pixels showing ground where grass grows within its draw distance (mountains and
/// the field beyond the grass stay out).
pub(super) fn ground_visibility(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    (w, h): (u32, u32),
) -> f32 {
    // Grass shadows on the ground would count as blades.
    let quality = QualityLevel {
        temporal_aa: false,
        grass_shadows: crate::quality::Tier::Off,
        ..quality.clone()
    };
    let bare = edited_terrains(view, "bare", |d| d.details.density = 0.0);
    let none: Vec<TerrainDraw> = Vec::new();
    let mut images = Vec::new();
    for terrains in [None, Some(&bare[..]), Some(&none[..])] {
        let mut r = Renderer::with_queue(device, queue);
        r.set_quality(device, &quality);
        let target = r.create_target(device, w, h);
        images.push(render(device, queue, &mut r, &target, view, terrains, 6));
    }
    let diff = |a: &[u8], b: &[u8], i: usize| {
        (0..3)
            .map(|c| (a[i * 4 + c] as i32 - b[i * 4 + c] as i32).abs())
            .max()
            .unwrap()
    };
    let grass_ground = grass_ground(view, (w, h));
    let (mut terrain, mut seen) = (0usize, 0usize);
    // Red = ground showing, green = covered by blades.
    let mut mask = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        mask[i * 4 + 3] = 255;
        if grass_ground[i] && diff(&images[1], &images[2], i) > 10 {
            terrain += 1;
            if diff(&images[0], &images[1], i) <= 4 {
                seen += 1;
                mask[i * 4] = 255;
            } else {
                mask[i * 4 + 1] = 160;
            }
        }
    }
    if let Ok(name) = std::env::var("MISTY_GFX_MASK") {
        let path = screenshots().join(format!("grass_{name}_ground_mask.png"));
        image::RgbaImage::from_raw(w, h, mask)
            .unwrap()
            .save(path)
            .unwrap();
    }
    seen as f32 / terrain.max(1) as f32
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
    let (w, h) = (1600, 900);
    for shot in Shot::ALL {
        if !only.is_empty() && !shot.name().contains(&only) {
            continue;
        }
        let view = meadow_view(shot);
        let mut r = Renderer::with_queue(&device, &queue);
        let mut quality = QualityLevel::high();
        if std::env::var("MISTY_GRASS_SHADOWS").is_ok_and(|v| v == "0") {
            quality.grass_shadows = crate::quality::Tier::Off;
        }
        r.set_quality(&device, &quality);
        let target = r.create_target(&device, w, h);
        let rgba = render(&device, &queue, &mut r, &target, &view, None, 24);
        let path = screenshots().join(format!("grass_{step}_{}.png", shot.name()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_raw(w, h, rgba)
            .unwrap()
            .save(&path)
            .unwrap();
        eprintln!("wrote {}", path.display());
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
    let seen = ground_visibility(&device, &queue, &view, &QualityLevel::high(), (960, 540));
    eprintln!("ground visible (wide golden, high): {:.1}%", seen * 100.0);
    let close = meadow_view(Shot::CloseGolden);
    let seen = ground_visibility(&device, &queue, &close, &QualityLevel::high(), (960, 540));
    eprintln!("ground visible (close golden, high): {:.1}%", seen * 100.0);
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
        panels.push(render(&device, &queue, &mut r, &target, &view, None, 24));
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
    // Off | on (High: cascades + the near map).
    strip(&[0, 2], "grass_shadows_on_vs_off.png");
    // The previous step (cascade casters only) | the near map.
    strip(&[1, 2], "grass_shadows_near_map.png");
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
