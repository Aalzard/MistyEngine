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
    /// CPU time of the render call alone (wall clock: a descheduled thread counts too).
    pub cpu_ms: f32,
    /// Work the render thread itself did in the frame: CPU time it was running, in the
    /// platform's units (TSC cycles on Windows, ns on Linux; only compared with other frames).
    /// Unlike the wall times it does not grow while the thread waits for a core.
    pub work: Option<u64>,
    /// Share of the machine's CPU time that was busy during the frame (0..1).
    pub busy: Option<f32>,
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
    // Recording and the frame counter are process-wide; the profiler's tests wait.
    let profiler = crate::profiler::test_serial();
    crate::profiler::set_enabled(true);
    crate::profiler::clear();
    let base = view.camera;
    let mut out = Vec::with_capacity(FRAMES);
    let first = crate::profiler::frame_index();
    for i in 0..FRAMES + 4 {
        let (work0, system0) = (os::thread_work(), os::system_times());
        let started = std::time::Instant::now();
        let cam = scripted(&base, i.min(FRAMES - 1));
        let mut frame = view.frame();
        frame.camera = &cam;
        r.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let wall_ms = started.elapsed().as_secs_f32() * 1000.0;
        let work = work0
            .zip(os::thread_work())
            .map(|(a, b)| b.saturating_sub(a));
        let busy = os::busy_between(system0, os::system_times());
        let cpu_ms = r.stats().cpu_ms;
        crate::profiler::end_frame();
        out.push(FrameTime {
            wall_ms,
            cpu_ms,
            work,
            busy,
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
    drop(profiler);
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
                "  frame {i:3}: wall {:.2} cpu {:.2} gpu {} ms, work {} x median, machine {} busy  [{}]",
                t.wall_ms,
                t.cpu_ms,
                t.gpu_ms.map_or("-".to_owned(), |g| format!("{g:.2}")),
                work_ratio(times, t).map_or("-".to_owned(), |r| format!("{r:.1}")),
                t.busy
                    .map_or("-".to_owned(), |b| format!("{:.0} %", b * 100.0)),
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

/// The machine counts as loaded during a frame when at least this share of its CPU time was
/// busy: then threads queue for cores, and a thread that is ready to run can wait tens of
/// milliseconds for one (the whole test suite runs on every core; other programs too).
const LOADED: f32 = 0.85;

/// A frame's render-thread work over the median of the frames after the warmup.
fn work_ratio(times: &[FrameTime], t: &FrameTime) -> Option<f32> {
    let work: Vec<f32> = times[WARMUP..]
        .iter()
        .filter_map(|t| t.work.map(|w| w as f32))
        .collect();
    let median = summary(&work).median;
    t.work.filter(|_| median > 0.0).map(|w| w as f32 / median)
}

/// Slow frames after the warmup: wall time over 2.5× the median while the GPU time was
/// ordinary (not the GPU being shared with another program in that frame). Each is a hitch
/// (`Err`) when the render thread itself worked over 2.5× its median (its CPU time, which
/// waiting for a core does not add to), or when it was off its core on a machine with cores
/// to spare (a wait of ours: a lock, a file, a blocking call). It is excused (`Ok`) when the
/// work was ordinary and the machine was loaded: the thread was ready but had no core.
fn stalls(times: &[FrameTime]) -> Vec<Result<String, String>> {
    let [wall, _, gpu] = summarize(times);
    times
        .iter()
        .enumerate()
        .skip(WARMUP)
        .filter(|(_, t)| t.wall_ms > wall.median * 2.5)
        .filter(|(_, t)| t.gpu_ms.is_none_or(|g| g < gpu.median * 2.0))
        .map(|(i, t)| {
            let ratio = work_ratio(times, t);
            let busy = t.busy.unwrap_or(0.0);
            let text = format!(
                "frame {i}: wall {:.1} ms (median {:.1}), render-thread work {} x median, machine {:.0} % busy",
                t.wall_ms,
                wall.median,
                ratio.map_or("?".to_owned(), |r| format!("{r:.1}")),
                busy * 100.0
            );
            match ratio {
                Some(r) if r <= 2.5 && busy >= LOADED => Ok(text),
                _ => Err(text),
            }
        })
        .collect()
}

/// MeadowDemo on High with the scripted camera: no frame longer than 2.5× the median after
/// the warmup (grass chunks stream in on worker threads), and the render thread never starts
/// uploading a chunk past its budget. Frames slowed by a loaded machine are told apart from
/// ours by the render thread's own CPU time (see [`stalls`]); they are printed, not failed.
/// A run with a hitch gets one more try; it fails when both have one.
#[test]
fn scripted_meadow_run_has_no_hitches() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = meadow_view(Shot::WideGolden);
    let budget = super::terrain_grass::UPLOAD_BUDGET.as_secs_f32() * 1000.0;
    let mut hitches = Vec::new();
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
        hitches.clear();
        for s in stalls(&times) {
            match s {
                Ok(excused) => eprintln!("  excused (machine loaded, ordinary work): {excused}"),
                Err(hitch) => hitches.push(hitch),
            }
        }
        if hitches.is_empty() {
            return;
        }
        eprintln!("  attempt {attempt}: {} hitches", hitches.len());
    }
    panic!("hitches in both attempts (the second's): {hitches:#?}");
}

use crate::test_cpu as os;

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

/// Opening MeadowDemo in the editor (streaming on) never freezes a frame: its rocks' Misty GI
/// distance fields (82k-triangle meshes, seconds each) bake on the background pool while the
/// frames go on (v0.2.1 froze the second frame for 23 s, the shadow lists' parallel loop
/// queued behind the bakes on rayon's global pool), and a later session reads them back from
/// `Library/MeshSdf` instead of baking again.
#[test]
fn meadow_demo_opens_without_a_frozen_frame() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // Frames over this fail (the first frame builds pipelines and is left out).
    const BUDGET_MS: f32 = 1500.0;
    let view = meadow_view(Shot::WideGolden);
    let open = |frames: usize, until_baked: bool| -> Vec<f32> {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &QualityLevel::high());
        let target = r.create_target(&device, 640, 360);
        let mut times = Vec::new();
        let opened = std::time::Instant::now();
        for i in 0.. {
            let started = std::time::Instant::now();
            r.render_with_effects(&device, &queue, &target, &view.frame(), &view.effects);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            times.push(started.elapsed().as_secs_f32() * 1000.0);
            let baking = crate::render::mesh_sdf_bakes_pending() > 0;
            if i + 1 >= frames && !(until_baked && baking) {
                break;
            }
            assert!(
                opened.elapsed().as_secs() < 1800,
                "mesh SDFs still pending after {} frames",
                times.len()
            );
        }
        times
    };
    // Forgetting every bake of the process: the mesh SDF tests that check the shared map wait.
    let _sdf = super::misty_gi::mesh_sdf_test_serial();
    crate::render::set_grass_streaming(true);
    super::misty_gi::forget_mesh_sdfs();
    // First launch (or a Library already holding the bakes): every frame goes on.
    let first = open(30, false);
    let worst = first[1..].iter().copied().fold(0.0, f32::max);
    eprintln!(
        "first open: frame 0 {:.0} ms, worst after {worst:.0} ms, bakes pending {}",
        first[0],
        crate::render::mesh_sdf_bakes_pending()
    );
    // Let the bakes land and write their cache files (first run on a machine: minutes on a
    // busy one; forgetting waits for every pending bake), then a new session.
    super::misty_gi::forget_mesh_sdfs();
    let library = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../SampleProject")
        .join(super::misty_gi::MESH_SDF_DIR);
    let cached = std::fs::read_dir(library).map_or(0, |d| d.count());
    let (hits, misses) = super::misty_gi::mesh_sdf_cache_reads();
    let started = std::time::Instant::now();
    // Frames until every rock's field has landed (read back on the background pool).
    let again = open(2, true);
    let reread = started.elapsed();
    let (hits_after, misses_after) = super::misty_gi::mesh_sdf_cache_reads();
    eprintln!(
        "second open: {reread:?}, {} frames, {} cache reads",
        again.len(),
        hits_after - hits
    );
    crate::render::set_grass_streaming(false);
    assert!(
        worst < BUDGET_MS,
        "opening MeadowDemo froze a frame for {worst:.0} ms (frames {first:?})"
    );
    assert!(
        cached >= 6,
        "the rocks' distance fields were not cached ({cached} files)"
    );
    // Counted, not timed: on a busy machine the second open's first frames alone can take
    // seconds, and a bake on below-normal-priority workers far longer.
    assert_eq!(
        misses_after,
        misses,
        "the second open baked again instead of reading the cache: {reread:?}, {} frames",
        again.len()
    );
    assert!(
        hits_after - hits >= 6,
        "the second open read {} distance fields from the cache, not every rock's",
        hits_after - hits
    );
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
