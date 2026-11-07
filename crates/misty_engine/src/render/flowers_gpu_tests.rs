//! Wildflowers on the GPU: flowers draw in the grass (run with the other GPU tests), plus
//! MeadowDemo flower shots and the flowers' GPU cost (ignored; run on purpose in release):
//! `cargo test --release -p misty_engine --lib flower_ -- --ignored --nocapture --test-threads=1`
//! `MISTY_GFX_STEP=<name>` names the shots `docs/screenshots/gfx/flowers_<name>_<shot>.png`;
//! `MISTY_FLOWERS=0` renders without flowers (the "before" shots).

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Vec2, Vec3};

use crate::camera::{Camera, Projection};
use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::environment::Environment;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::trees::gpu_tests::{Shot, meadow_view};
use crate::render_scene::GameView;
use crate::terrain::flowers::{self, FlowerSettings};
use crate::terrain::{TerrainData, TerrainDraw, details};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

/// Share of reddish pixels (poppies) in an RGBA image.
fn red_share(rgba: &[u8]) -> f32 {
    let px = rgba.len() / 4;
    let red = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] as f32 > 90.0 && p[0] as f32 > 1.8 * p[1] as f32)
        .count();
    red as f32 / px as f32
}

/// A flat field of short grass full of poppies draws them (red where there were none), and the
/// flowers sway: two wind times give different images.
#[test]
fn flowers_draw_in_the_grass() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    crate::assets::set_project_root(&root());
    let mut d = TerrainData::new(65, Vec3::new(64.0, 10.0, 64.0), 64);
    d.details.enabled = true;
    d.details.density = 40.0;
    d.details.height = [0.05, 0.1];
    d.details.rules.breakup = 0.0;
    details::generate(&mut d);
    let poppy = FlowerSettings::wildflowers("Assets/Flowers")
        .into_iter()
        .find(|k| k.name == "Poppy")
        .unwrap();
    d.flowers = FlowerSettings {
        enabled: true,
        kinds: vec![poppy],
        density: 20.0,
        coverage: 1.0,
        ..Default::default()
    };
    let field = |flowers: bool, key: &str| TerrainDraw {
        key: key.into(),
        version: 1,
        data: Arc::new(TerrainData {
            flowers: FlowerSettings {
                enabled: flowers,
                ..d.flowers.clone()
            },
            ..d.clone()
        }),
        position: Vec3::new(-32.0, 0.0, -32.0),
        cast_shadows: true,
    };
    let camera = Camera {
        position: Vec3::new(0.0, 1.6, -4.0),
        rotation: glam::Quat::from_rotation_x(25f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.05,
        far: 500.0,
    };
    let (w, h) = (320, 180);
    let render = |terrains: &[TerrainDraw], time: f32| {
        let mut r = Renderer::with_queue(&device, &queue);
        let target = r.create_target(&device, w, h);
        let environment = Environment {
            grass_time: Some(time),
            ..Default::default()
        };
        let lights = [crate::render::LightData {
            kind: crate::scene::LightType::Directional,
            position: Vec3::ZERO,
            direction: Vec3::new(0.3, -1.0, 0.4).normalize(),
            color: [1.0; 3],
            intensity: 2.0,
            range: 10.0,
            spot_angle: 30.0,
            shadows: crate::scene::Shadows::Hard,
            contact_shadows: false,
            area: Default::default(),
            shadow: Default::default(),
            volumetric: Default::default(),
        }];
        let frame = crate::render::Frame {
            camera: &camera,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &environment,
            post: Default::default(),
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains,
            shaded: true,
            lights: &lights,
            water: &[],
        };
        for _ in 0..4 {
            r.render(&device, &queue, &target, &frame);
        }
        read_back(&device, &queue, &target)
    };
    let without = render(&[field(false, "flower test bare")], 1.0);
    let with = render(&[field(true, "flower test poppies")], 1.0);
    let later = render(&[field(true, "flower test poppies")], 2.5);
    if let Ok(dir) = std::env::var("MISTY_FLOWER_DEBUG") {
        for (name, img) in [("without", &without), ("with", &with)] {
            image::RgbaImage::from_raw(w, h, img.clone())
                .unwrap()
                .save(PathBuf::from(&dir).join(format!("flowers_test_{name}.png")))
                .unwrap();
        }
    }
    let (a, b) = (red_share(&without), red_share(&with));
    assert!(a < 0.002, "red without flowers: {a}");
    assert!(b > 0.01, "poppies: {b} of the pixels red");
    let moved = with
        .iter()
        .zip(&later)
        .filter(|(x, y)| x.abs_diff(**y) > 24)
        .count();
    assert!(
        moved > 50,
        "the wind moves the flowers ({moved} channels changed)"
    );
}

/// Terrain-local spot with the most flowers (of kind slot `kind`, any for None) in `radius`
/// metres of `around` (4 m cells).
fn densest_patch(d: &TerrainData, around: Vec2, radius: f32, kind: Option<u32>) -> Vec2 {
    let mut best = (0, around);
    let lo = around - radius;
    let cells = (radius * 2.0 / 4.0) as i32;
    for z in 0..cells {
        for x in 0..cells {
            let p = lo + Vec2::new(x as f32, z as f32) * 4.0;
            let n = flowers::scatter(d, p, p + 4.0)
                .iter()
                .filter(|b| kind.is_none_or(|k| flowers::card_of(b).is_some_and(|c| c.0 == k)))
                .count();
            if n > best.0 {
                best = (n, p + 2.0);
            }
        }
    }
    best.1
}

/// The view's terrains without flowers (own store key).
fn without_flowers(view: &GameView) -> Vec<TerrainDraw> {
    view.terrains
        .iter()
        .map(|t| TerrainDraw {
            key: format!("{}#noflowers", t.key),
            data: Arc::new(TerrainData {
                flowers: FlowerSettings::default(),
                ..(*t.data).clone()
            }),
            ..t.clone()
        })
        .collect()
}

/// A camera at `eye` looking at `at` (no roll).
fn look_at(view: &mut GameView, eye: Vec3, at: Vec3, fov: f32) {
    let d = (at - eye).normalize();
    view.camera.position = eye;
    view.camera.rotation = glam::Quat::from_rotation_y(d.x.atan2(d.z))
        * glam::Quat::from_rotation_x(-d.y.clamp(-1.0, 1.0).asin());
    view.camera.projection = Projection::Perspective { fov_y: fov };
    view.camera.near = 0.05;
}

/// MeadowDemo views of the flowers: the player's start view and turned round, a patch close
/// up (standing at its edge) and a patch 40–60 m away.
fn flower_views() -> Vec<(&'static str, GameView)> {
    let wide = meadow_view(Shot::Wide);
    let t = &wide.terrains[0];
    let eye = wide.camera.position;
    let local = Vec2::new(eye.x - t.position.x, eye.z - t.position.z);
    let ground = |p: Vec2| t.position.y + t.data.sample_height(p.x, p.y);
    let world = |p: Vec2| Vec3::new(p.x + t.position.x, ground(p), p.y + t.position.z);
    let close_up = |near: Vec2| {
        let dir = (near - local).normalize_or(Vec2::Y);
        let stand = near - dir * 3.0;
        let mut v = meadow_view(Shot::Wide);
        look_at(
            &mut v,
            world(stand) + Vec3::Y * 1.6,
            world(near) + Vec3::Y * 0.15,
            55.0,
        );
        v
    };
    let close = close_up(densest_patch(&t.data, local, 28.0, None));
    // Poppies (kind slot 1 of the sample wildflowers).
    let poppies = close_up(densest_patch(&t.data, local, 60.0, Some(1)));
    // The densest patch in a ring 40–60 m out, seen from the player's eye height.
    let mut far_spot = (0, local + Vec2::Y * 50.0);
    for k in 0..24 {
        let a = k as f32 / 24.0 * std::f32::consts::TAU;
        for r in [40.0, 50.0, 60.0] {
            let p = local + Vec2::new(a.cos(), a.sin()) * r;
            let n = flowers::scatter(&t.data, p - 4.0, p + 4.0).len();
            if n > far_spot.0 {
                far_spot = (n, p);
            }
        }
    }
    let mut far = meadow_view(Shot::Wide);
    look_at(
        &mut far,
        world(local) + Vec3::Y * 1.7,
        world(far_spot.1) + Vec3::Y * 0.3,
        50.0,
    );
    vec![
        ("wide", wide),
        ("behind", meadow_view(Shot::Behind)),
        ("patch_close", close),
        ("patch_poppies", poppies),
        ("patch_far", far),
    ]
}

fn shot_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    terrains: Option<&[TerrainDraw]>,
    (w, h): (u32, u32),
) -> image::RgbaImage {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_target(device, w, h);
    let mut frame = view.frame();
    if let Some(t) = terrains {
        frame.terrains = t;
    }
    for _ in 0..48 {
        r.render(device, queue, &target, &frame);
    }
    image::RgbaImage::from_raw(w, h, read_back(device, queue, &target)).unwrap()
}

/// `docs/screenshots/gfx/flowers_<step>_{wide,behind,patch_close,patch_far}.png` (1600×900,
/// High; `MISTY_GFX_SHOT` picks shots by name, `MISTY_FLOWERS=0` leaves the flowers out).
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn flower_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let step = std::env::var("MISTY_GFX_STEP").unwrap_or_else(|_| "latest".into());
    let only = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
    let bare = std::env::var("MISTY_FLOWERS").is_ok_and(|v| v == "0");
    std::fs::create_dir_all(screenshots()).unwrap();
    for (name, view) in flower_views() {
        if !only.is_empty() && !name.contains(&only) {
            continue;
        }
        let none = without_flowers(&view);
        let terrains = bare.then_some(&none[..]);
        let img = shot_image(
            &device,
            &queue,
            &view,
            &QualityLevel::high(),
            terrains,
            (1600, 900),
        );
        let path = screenshots().join(format!("flowers_{step}_{name}.png"));
        img.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
    }
}

/// Median GPU frame time of MeadowDemo at 1920×1080 per quality level with and without its
/// flowers (the difference is what the flowers cost): the start view and the far patch view.
#[test]
#[ignore = "timing, prints only"]
fn flower_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let rounds: usize = std::env::var("MISTY_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    for (name, view) in flower_views() {
        if name != "wide" && name != "patch_far" {
            continue;
        }
        let none = without_flowers(&view);
        for (level, quality) in [
            ("high", QualityLevel::high()),
            ("medium", QualityLevel::medium()),
            ("low", QualityLevel::low()),
        ] {
            // Interleaved rounds, the quietest kept (the GPU is shared).
            let mut best = [f32::MAX; 2];
            for _ in 0..rounds {
                for (i, terrains) in [None, Some(&none[..])].into_iter().enumerate() {
                    let mut r = Renderer::with_queue(&device, &queue);
                    r.set_quality(&device, &quality);
                    let target = r.create_target(&device, 1920, 1080);
                    let mut frame = view.frame();
                    if let Some(t) = terrains {
                        frame.terrains = t;
                    }
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
                    let median = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
                    best[i] = best[i].min(median);
                }
            }
            eprintln!(
                "{name:>11} {level:>6}: flowers {:.2} ms, no flowers {:.2} ms => flowers cost {:.2} ms",
                best[0],
                best[1],
                best[0] - best[1],
            );
        }
    }
}
