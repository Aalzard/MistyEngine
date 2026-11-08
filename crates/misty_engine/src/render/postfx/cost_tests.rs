//! GPU cost of the round 17 effects on PostFxDemo at 1920×1080 (ignored; timing, prints only):
//!
//! `cargo test --release -p misty_engine --lib postfx_cost -- --ignored --nocapture --test-threads=1`
//!
//! Each configuration renders 60 frames to settle (the camera turning 0.5° a frame, so motion
//! blur has work), then the pass timer's median over 40 frames of the effect's own passes, and
//! the whole-frame GPU time against the same view with every effect off. The decal rows add the
//! screen-space prepass the decals turn on (the demo has no other screen-space effect).
//! `postfx_screenshots` writes `docs/screenshots/gfx/postfx_*.png` (converged; the motion blur
//! shots with the camera turning).

use glam::{Quat, Vec3};

use crate::quality::QualityLevel;
use crate::render::gpu_tests::headless;
use crate::render::{DecalDraw, Renderer};
use crate::render_scene::GameView;
use crate::scene::{DecalProjector, DofMode, PostFxQuality};

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn demo() -> GameView {
    crate::assets::set_project_root(&root());
    let path = root().join("Assets/Scenes/PostFxDemo.scene");
    let (scene, _) = crate::scene::Scene::load(&path).expect("PostFxDemo");
    let materials = crate::assets::AssetCache::open(&root());
    GameView::new(&scene, &materials).expect("camera")
}

/// `n` dirt and crack decals scattered over the yard in front of the camera.
fn scattered(n: usize) -> Vec<DecalDraw> {
    (0..n)
        .map(|i| {
            let (x, z) = ((i % 10) as f32 * 1.2 - 5.4, (i / 10) as f32 * 1.6 - 2.0);
            DecalDraw {
                object: glam::Mat4::from_rotation_translation(
                    Quat::from_rotation_x(90f32.to_radians()) * Quat::from_rotation_z(i as f32),
                    Vec3::new(x, 0.5, z),
                ),
                decal: DecalProjector {
                    size: [1.4, 1.4, 1.0],
                    base_map: Some(
                        if i % 2 == 0 {
                            "Assets/Decals/Dirt.png"
                        } else {
                            "Assets/Decals/Cracks.png"
                        }
                        .into(),
                    ),
                    normal_map: (i % 2 == 1).then(|| "Assets/Decals/Cracks_Normal.png".into()),
                    ..Default::default()
                },
            }
        })
        .collect()
}

struct Config {
    name: String,
    view: GameView,
    /// Pass labels that make up the effect's cost.
    passes: &'static [&'static str],
}

fn off(view: &mut GameView) {
    view.post.depth_of_field.active = false;
    view.post.motion_blur.active = false;
    view.post.auto_exposure.active = false;
    view.environment.decals.clear();
}

fn configs() -> Vec<Config> {
    let mut out = Vec::new();
    let mut add = |name: String, passes, f: &dyn Fn(&mut GameView)| {
        let mut view = demo();
        off(&mut view);
        f(&mut view);
        out.push(Config { name, view, passes });
    };
    add("all off".into(), &[], &|_| {});
    for hq in [false, true] {
        add(
            format!("DOF Gaussian{}", if hq { " HQ" } else { "" }),
            &["Depth of field"],
            &|v| {
                let d = &mut v.post.depth_of_field;
                d.active = true;
                d.mode = DofMode::Gaussian;
                d.gaussian_start = 8.0;
                d.gaussian_end = 25.0;
                d.high_quality_sampling = hq;
            },
        );
    }
    for q in PostFxQuality::ALL {
        add(
            format!("DOF Bokeh {}", q.label()),
            &["Depth of field"],
            &|v| {
                let d = &mut v.post.depth_of_field;
                d.active = true;
                d.mode = DofMode::Bokeh;
                d.quality = q;
            },
        );
    }
    for samples in [8, 16, 32] {
        add(
            format!("Motion Blur {samples} samples"),
            &["Motion blur"],
            &|v| {
                v.post.motion_blur.active = true;
                v.post.motion_blur.sample_count = samples;
            },
        );
    }
    add("Auto Exposure".into(), &["Auto exposure"], &|v| {
        v.post.auto_exposure.active = true;
    });
    for n in [10, 200] {
        add(
            format!("Decals x{n}"),
            &["Decals", "AO prepass + GTAO"],
            &|v| v.environment.decals = scattered(n),
        );
    }
    out
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
}

#[test]
#[ignore = "timing, prints only"]
fn postfx_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let quality = QualityLevel::high();
    let mut rows = Vec::new();
    for c in configs() {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality);
        let target = r.create_view_target(&device, 1920, 1080);
        let base = c.view.camera;
        let mut view = c.view;
        let frame_at = |r: &mut Renderer, i: usize, view: &mut GameView| {
            view.camera.rotation =
                base.rotation * Quat::from_rotation_y((i as f32 * 0.5).to_radians());
            let frame = view.frame();
            r.render(&device, &queue, &target, &frame);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        };
        let mut gpu = Vec::new();
        for i in 0..100 {
            frame_at(&mut r, i, &mut view);
            if i >= 60 {
                gpu.extend(r.gpu_ms());
            }
        }
        let mut parts = Vec::new();
        let mut breakdown: Vec<(&str, Vec<f32>)> =
            c.passes.iter().map(|p| (*p, Vec::new())).collect();
        if r.set_pass_timing(&device, &queue, true) {
            for i in 100..140 {
                frame_at(&mut r, i, &mut view);
                let times = r.pass_times();
                let own = |l: &str| {
                    times
                        .iter()
                        .filter(|(t, _)| *t == l)
                        .map(|(_, ms)| ms)
                        .sum::<f32>()
                };
                parts.push(c.passes.iter().map(|p| own(p)).sum::<f32>());
                for (label, v) in &mut breakdown {
                    v.push(own(label));
                }
            }
        }
        let detail: Vec<String> = breakdown
            .iter_mut()
            .map(|(l, v)| format!("{l} {:.3}", median(v)))
            .collect();
        let row = (
            c.name,
            median(&mut gpu),
            median(&mut parts),
            detail.join(", "),
        );
        eprintln!(
            "{:<26} frame {:6.2} ms  effect {:6.3} ms  ({})",
            row.0, row.1, row.2, row.3
        );
        rows.push(row);
    }
    eprintln!("\n| Configuration | Frame GPU ms | Effect ms | Passes |\n|---|---|---|---|");
    for (name, frame, effect, detail) in rows {
        eprintln!("| {name} | {frame:.2} | {effect:.3} | {detail} |");
    }
}

/// Converged stills of PostFxDemo with each effect, into `docs/screenshots/gfx`.
#[test]
#[ignore = "writes screenshots into docs/screenshots/gfx; run on purpose"]
fn postfx_screenshots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx");
    let quality = QualityLevel::high();
    type Shot<'a> = (&'a str, &'a dyn Fn(&mut GameView), bool);
    let shots: [Shot; 6] = [
        ("postfx_off", &|v| off(v), false),
        (
            "postfx_decals",
            &|v| {
                let decals = std::mem::take(&mut v.environment.decals);
                off(v);
                v.environment.decals = decals;
            },
            false,
        ),
        ("postfx_dof_bokeh", &|_| {}, false),
        (
            "postfx_dof_gaussian",
            &|v| {
                let d = &mut v.post.depth_of_field;
                d.mode = DofMode::Gaussian;
                d.gaussian_start = 10.0;
                d.gaussian_end = 30.0;
                d.gaussian_max_radius = 1.5;
                v.post.motion_blur.active = false;
            },
            false,
        ),
        (
            "postfx_motion_blur",
            &|v| {
                v.post.depth_of_field.active = false;
                v.post.motion_blur.intensity = 1.0;
            },
            true,
        ),
        ("postfx_all", &|_| {}, true),
    ];
    for (name, f, turning) in shots {
        let mut view = demo();
        f(&mut view);
        if !turning {
            view.post.motion_blur.active = false;
        }
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality);
        let target = r.create_view_target(&device, 1920, 1080);
        let base = view.camera.rotation;
        for i in 0..64 {
            // Turning 1.5° a frame for the motion blur shots (the last frames only).
            let yaw = if turning && i > 56 {
                (i - 56) as f32 * 1.5
            } else {
                0.0
            };
            view.camera.rotation = base * Quat::from_rotation_y(yaw.to_radians());
            let frame = view.frame();
            r.render(&device, &queue, &target, &frame);
        }
        let px = crate::render::gpu_tests::read_back(&device, &queue, &target);
        let img = image::RgbaImage::from_raw(1920, 1080, px).expect("size");
        let path = dir.join(format!("{name}.png"));
        img.save(&path).expect("png");
        eprintln!("wrote {}", path.display());
    }
}
