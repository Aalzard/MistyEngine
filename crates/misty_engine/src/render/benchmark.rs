//! StressDemo benchmark (ignored; run in release):
//! `cargo test --release -p misty_engine stress_benchmark -- --ignored --nocapture`
//! Each configuration turns one optimisation off (or on) against the defaults.

use glam::{Quat, Vec3};

use super::gpu_tests::headless;
use super::*;
use crate::draw_cache::DrawCache;
use crate::render_scene::GameView;

struct Config {
    name: &'static str,
    scene: &'static str,
    options: RenderOptions,
    /// Keep a [`DrawCache`] between frames (else gather from scratch like before).
    cached: bool,
}

fn configs() -> Vec<Config> {
    let on = RenderOptions::default();
    let demo = "Assets/Scenes/StressDemo.scene";
    let row = |name, options, cached| Config {
        name,
        scene: demo,
        options,
        cached,
    };
    let off = RenderOptions {
        culling: false,
        instancing: false,
        ..on
    };
    vec![
        row(
            "baseline (no culling, no instancing, full gather)",
            off,
            false,
        ),
        row(
            "culling + instancing, full gather (previous best)",
            on,
            false,
        ),
        row("+ cached gather, parallel prep (default)", on, true),
        row(
            "  without instancing",
            RenderOptions {
                instancing: false,
                ..on
            },
            true,
        ),
        row(
            "  without culling",
            RenderOptions {
                culling: false,
                ..on
            },
            true,
        ),
        row(
            "  + depth prepass",
            RenderOptions {
                depth_prepass: true,
                ..on
            },
            true,
        ),
        row(
            "  + GPU culling (frustum + Hi-Z, indirect)",
            RenderOptions {
                gpu_culling: true,
                ..on
            },
            true,
        ),
        Config {
            name: "StressStatic: static batching (play mode)",
            scene: "Assets/Scenes/StressStatic.scene",
            options: on,
            cached: true,
        },
    ]
}

#[test]
#[ignore = "benchmark: renders StressDemo for a few hundred frames"]
fn stress_benchmark() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let materials = crate::assets::AssetCache::open(&root);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, 1280, 720);
    for config in configs() {
        let Ok((scene, _)) = crate::scene::Scene::load(&root.join(config.scene)) else {
            println!("{}: {} missing, skipped", config.name, config.scene);
            continue;
        };
        renderer.set_options(config.options);
        let mut cache = DrawCache::new();
        cache.set_static_batching(config.scene.contains("Static"));
        let frames = 120;
        let (mut gather, mut cpu, mut gpu, mut total) = (0.0, 0.0, 0.0, 0.0);
        let mut stats = RenderStats::default();
        for i in 0..frames + 10 {
            let start = std::time::Instant::now();
            let view = if config.cached {
                GameView::cached(&scene, &materials, &mut cache)
            } else {
                GameView::new(&scene, &materials)
            };
            let mut view = view.expect("camera");
            // Orbit like the demo's camera script.
            let angle = i as f32 * 0.02;
            view.camera.position = Vec3::new(angle.sin() * 70.0, 30.0, -angle.cos() * 70.0);
            view.camera.rotation = Quat::from_rotation_y(-angle) * Quat::from_rotation_x(0.38);
            let gathered = start.elapsed().as_secs_f32() * 1000.0;
            renderer.render(&device, &queue, &target, &view.frame());
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 10 {
                gather += gathered;
                cpu += renderer.stats().cpu_ms;
                gpu += renderer.gpu_ms().unwrap_or(0.0);
                total += start.elapsed().as_secs_f32() * 1000.0;
                stats = renderer.stats();
            }
        }
        let n = frames as f32;
        println!(
            "{}: {:.1} fps | frame {:.2} ms = gather {:.2} + render CPU {:.2} + GPU wait (GPU {:.2}) | \
             batches {} (saved {}), tris {}k, visible {}, culled {}, shadow casters {}",
            config.name,
            1000.0 / (total / n),
            total / n,
            gather / n,
            cpu / n,
            gpu / n,
            stats.batches,
            stats.saved_by_batching,
            stats.triangles / 1000,
            stats.visible,
            stats.culled,
            stats.shadow_casters,
        );
    }
}
