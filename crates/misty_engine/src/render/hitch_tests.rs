//! Frame-time hitches: MeadowDemo rendered with a scripted camera that stands still, turns
//! 180° and dollies in and out, every frame timed on the CPU (the render call) and the GPU
//! (timestamp queries) with the profiler scopes of the slow frames.
//! `cargo test --release -p misty_engine hitch -- --ignored --nocapture --test-threads=1`

use glam::Quat;

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::headless;
use crate::render::terrain_grass::gpu_tests::{Shot, meadow_view};
use crate::render_scene::GameView;

/// Frames of the scripted run.
pub(super) const FRAMES: usize = 300;
/// Frames left out of the statistics (first renders build GPU resources).
pub(super) const WARMUP: usize = 30;

/// The camera at frame `i`: still, then a 180° turn, then a dolly 12 m in and back out.
pub(super) fn scripted(base: &crate::camera::Camera, i: usize) -> crate::camera::Camera {
    let mut cam = *base;
    let turn = ((i as f32 - 60.0) / 120.0).clamp(0.0, 1.0);
    cam.rotation = Quat::from_rotation_y(turn * std::f32::consts::PI) * base.rotation;
    let dolly = if i < 180 {
        0.0
    } else if i < 240 {
        (i - 180) as f32 / 60.0
    } else {
        1.0 - (i - 240) as f32 / 60.0
    };
    cam.position = base.position + cam.forward() * dolly * 12.0;
    cam
}

#[derive(Clone, Debug, Default)]
pub(super) struct FrameTime {
    /// Wall-clock time of the frame: the render call and the wait for the GPU to finish it.
    pub wall_ms: f32,
    /// CPU time of the render call alone.
    pub cpu_ms: f32,
    pub gpu_ms: Option<f32>,
    /// Render-thread time spent uploading grass chunks, and the time already spent when the
    /// last chunk started (the budget check).
    pub grass_upload_ms: f32,
    pub grass_upload_last_start_ms: f32,
    /// Pipelines built so far (a rise after the first frames is a shader compile mid-run).
    pub pipelines_built: u64,
    /// Longest profiler scopes of the render (name, ms), by total time.
    pub scopes: Vec<(String, f32)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Summary {
    pub median: f32,
    pub p95: f32,
    pub p99: f32,
    pub max: f32,
    /// Frames above 2.5 × the median.
    pub hitches: usize,
}

pub(super) fn summary(times: &[f32]) -> Summary {
    let mut sorted: Vec<f32> = times.iter().copied().filter(|t| t.is_finite()).collect();
    sorted.sort_by(f32::total_cmp);
    let q = |f: f32| {
        sorted
            .get(((sorted.len() as f32 * f) as usize).min(sorted.len().saturating_sub(1)))
            .copied()
            .unwrap_or(f32::NAN)
    };
    let median = q(0.5);
    Summary {
        median,
        p95: q(0.95),
        p99: q(0.99),
        max: sorted.last().copied().unwrap_or(f32::NAN),
        hitches: sorted.iter().filter(|&&t| t > median * 2.5).count(),
    }
}

/// Render the scripted run; one entry per frame.
pub(super) fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    (w, h): (u32, u32),
) -> Vec<FrameTime> {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_target(device, w, h);
    // The real thing: grass chunks stream in over frames (headless() turned it off).
    crate::render::set_grass_streaming(true);
    crate::profiler::set_enabled(true);
    crate::profiler::clear();
    let base = view.camera;
    let mut out = Vec::with_capacity(FRAMES);
    let first = crate::profiler::frame_index();
    for i in 0..FRAMES + 4 {
        let started = std::time::Instant::now();
        let cam = scripted(&base, i.min(FRAMES - 1));
        let mut frame = view.frame();
        frame.camera = &cam;
        r.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let cpu_ms = r.stats().cpu_ms;
        crate::profiler::end_frame();
        out.push(FrameTime {
            wall_ms: started.elapsed().as_secs_f32() * 1000.0,
            cpu_ms,
            grass_upload_ms: r.stats().grass_upload_ms,
            grass_upload_last_start_ms: r.stats().grass_upload_last_start_ms,
            pipelines_built: crate::render::pipelines_built(),
            ..Default::default()
        });
    }
    // GPU times arrive a frame or two later; scopes come from the recorded frames (the
    // history keeps the last 300, so the first few of the run are gone).
    crate::profiler::with_frames(|frames| {
        for f in frames {
            let i = (f.index - first) as usize;
            let Some(t) = out.get_mut(i) else {
                continue;
            };
            t.gpu_ms = f.gpu_ms;
            let tree = crate::profiler::HierarchyTree::build(f);
            let mut scopes: Vec<(String, f32)> = tree
                .nodes
                .iter()
                .skip(1)
                .filter(|n| n.self_ms > 0.05)
                .map(|n| (n.name.clone(), n.self_ms))
                .collect();
            scopes.sort_by(|a, b| b.1.total_cmp(&a.1));
            scopes.truncate(6);
            t.scopes = scopes;
        }
    });
    crate::profiler::set_enabled(false);
    crate::render::set_grass_streaming(false);
    out.truncate(FRAMES);
    out
}

/// Wall, CPU and GPU summaries of the frames after the warmup.
pub(super) fn summarize(times: &[FrameTime]) -> [Summary; 3] {
    let wall: Vec<f32> = times[WARMUP..].iter().map(|t| t.wall_ms).collect();
    let cpu: Vec<f32> = times[WARMUP..].iter().map(|t| t.cpu_ms).collect();
    let gpu: Vec<f32> = times[WARMUP..].iter().filter_map(|t| t.gpu_ms).collect();
    [summary(&wall), summary(&cpu), summary(&gpu)]
}

fn line(name: &str, s: &Summary) -> String {
    format!(
        "{name} median {:.2} p95 {:.2} p99 {:.2} max {:.2} ms ({} hitches)",
        s.median, s.p95, s.p99, s.max, s.hitches
    )
}

fn report(label: &str, times: &[FrameTime]) -> [Summary; 3] {
    let [w, c, g] = summarize(times);
    eprintln!("{label}:");
    eprintln!("  {}", line("wall", &w));
    eprintln!("  {}", line("cpu ", &c));
    eprintln!("  {}", line("gpu ", &g));
    for (i, t) in times.iter().enumerate().skip(WARMUP) {
        let slow_wall = t.wall_ms > w.median * 2.0;
        let slow_cpu = t.cpu_ms > c.median * 2.0;
        let slow_gpu = t.gpu_ms.is_some_and(|ms| ms > g.median * 2.0);
        if slow_wall || slow_cpu || slow_gpu {
            let scopes: Vec<String> = t
                .scopes
                .iter()
                .map(|(n, ms)| format!("{n} {ms:.2}"))
                .collect();
            eprintln!(
                "  frame {i:3}: wall {:.2} cpu {:.2} gpu {} ms  [{}]",
                t.wall_ms,
                t.cpu_ms,
                t.gpu_ms.map_or("-".to_owned(), |g| format!("{g:.2}")),
                scopes.join(", ")
            );
        }
    }
    [w, c, g]
}

/// A sample scene as saved, seen by its camera.
fn demo_view(scene: &str) -> GameView {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    crate::assets::set_project_root(&root);
    let path = root.join(format!("Assets/Scenes/{scene}.scene"));
    let (s, _) = crate::scene::Scene::load(&path).expect("sample scene");
    let materials = crate::assets::AssetCache::open(&root);
    GameView::new(&s, &materials).expect("camera")
}

/// Frames after the warmup whose wall time was over 2.5× the median while their GPU time was
/// ordinary: a stall of ours, not the GPU being shared with another program (the user's
/// editor, the other tests) in that frame.
fn stalls(times: &[FrameTime]) -> Vec<(usize, f32)> {
    let [wall, _, gpu] = summarize(times);
    times
        .iter()
        .enumerate()
        .skip(WARMUP)
        .filter(|(_, t)| t.wall_ms > wall.median * 2.5)
        .filter(|(_, t)| t.gpu_ms.is_none_or(|g| g < gpu.median * 2.0))
        .map(|(i, t)| (i, t.wall_ms))
        .collect()
}

/// MeadowDemo on High with the scripted camera: no frame longer than 2.5× the median after
/// the warmup (grass chunks stream in on worker threads; a run that stalls gets one more
/// try, since the test suite runs on every core at once), and the render thread never starts
/// uploading a chunk past its budget.
#[test]
fn scripted_meadow_run_has_no_hitches() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = meadow_view(Shot::WideGolden);
    let budget = super::terrain_grass::UPLOAD_BUDGET.as_secs_f32() * 1000.0;
    let mut stalled = Vec::new();
    for attempt in 0..2 {
        let times = run(&device, &queue, &view, &QualityLevel::high(), (1280, 720));
        let [wall, cpu, gpu] = report(&format!("meadow high, attempt {attempt}"), &times);
        assert!(wall.median.is_finite() && cpu.median.is_finite() && gpu.median.is_finite());
        for (i, t) in times.iter().enumerate() {
            assert!(
                t.grass_upload_last_start_ms < budget,
                "frame {i}: a grass chunk started uploading {:.2} ms into the frame's budget of {budget} ms",
                t.grass_upload_last_start_ms
            );
        }
        stalled = stalls(&times);
        if stalled.is_empty() {
            return;
        }
    }
    panic!("frames over 2.5× the median wall time (frame, ms): {stalled:?}");
}

/// Every pipeline a demo scene draws with exists after its first frames: moving the camera
/// (LOD cross-fades, new shadow casters, effects) compiles no shader mid-run.
#[test]
fn demo_scenes_compile_no_shader_after_their_first_frames() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    for scene in ["MeadowDemo", "CyclesRoomDemo", "VfxDemo"] {
        let view = demo_view(scene);
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &QualityLevel::high());
        let target = r.create_target(&device, 960, 540);
        let base = view.camera;
        let mut built = Vec::new();
        for i in 0..FRAMES {
            // The same turn and dolly, in fewer frames.
            let cam = scripted(&base, (i * 10).min(FRAMES - 1));
            let mut frame = view.frame();
            frame.camera = &cam;
            r.render_with_effects(&device, &queue, &target, &frame, &view.effects);
            built.push(crate::render::pipelines_built());
            if i >= 30 {
                break;
            }
        }
        let late: Vec<(usize, u64)> = built
            .windows(2)
            .enumerate()
            .skip(1)
            .filter(|(_, w)| w[1] > w[0])
            .map(|(i, w)| (i + 1, w[1] - w[0]))
            .collect();
        assert!(
            late.is_empty(),
            "{scene}: pipelines built after frame 1 (frame, count): {late:?}"
        );
    }
}

/// Per-frame CPU and GPU times of the scripted MeadowDemo run on High, with the scopes of
/// every slow frame (prints only).
#[test]
#[ignore = "timing, prints only"]
fn hitch_profile() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = meadow_view(Shot::WideGolden);
    for (label, quality) in [
        ("high", QualityLevel::high()),
        ("medium", QualityLevel::medium()),
    ] {
        let times = run(&device, &queue, &view, &quality, (1600, 900));
        report(label, &times);
        if let Ok(dir) = std::env::var("MISTY_HITCH_DUMP") {
            let mut text = String::new();
            for (i, t) in times.iter().enumerate() {
                let scopes: Vec<String> = t
                    .scopes
                    .iter()
                    .map(|(n, ms)| format!("{n} {ms:.2}"))
                    .collect();
                text += &format!(
                    "{i:3} wall {:7.2} cpu {:6.2} gpu {} pipelines {} grass upload {:.2}  [{}]\n",
                    t.wall_ms,
                    t.cpu_ms,
                    t.gpu_ms.map_or("-".to_owned(), |g| format!("{g:6.2}")),
                    t.pipelines_built,
                    t.grass_upload_ms,
                    scopes.join(", ")
                );
            }
            let path = std::path::Path::new(&dir).join(format!("hitch_{label}.txt"));
            std::fs::write(&path, text).unwrap();
            eprintln!("wrote {}", path.display());
        }
    }
}
