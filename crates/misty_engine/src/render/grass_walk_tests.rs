//! Long walks through a meadow (round 35): the camera moves like a player on foot for
//! thousands of frames (out, round a block, back, turning round, circling) while every frame
//! is timed and what the grass holds is logged. Frame time and memory must stay flat however
//! far the player walks.
//!
//! The measuring run (ignored; in release):
//! `cargo test --release -p misty_engine --lib grass_walk_report -- --ignored --nocapture --test-threads=1`
//!
//! - `MISTY_WALK_SCENE` (MeadowDemo), `MISTY_QUALITY` (high), `MISTY_SIZE=WxH` (1920x1080)
//! - `MISTY_WALK_SPEED` m/s (5), `MISTY_WALK_YAW` degrees (the scene camera's heading),
//!   `MISTY_WALK_START=x,z` terrain-local metres (the scene camera's place)
//! - `MISTY_WALK_SCALE` scales every distance of the script (1)
//! - `MISTY_WALK_PACE=0`: frames as fast as they render (default: one every 1/60 s, so the
//!   scatter workers get the time they have in the editor)
//! - `MISTY_WALK_CSV=<file>`: one line per second of the walk

use std::time::{Duration, Instant};

use glam::{Quat, Vec2, Vec3};

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::headless;
use crate::render::terrain_grass::GrassLoad;
use crate::render_scene::GameView;

/// Eye height of the walker above the ground (m).
const EYE: f32 = 1.7;
/// Frames per second of the walk (the distance a frame moves).
const FPS: f32 = 60.0;

/// One part of a walk.
#[derive(Clone, Copy, Debug)]
pub(super) enum Step {
    /// Stand for this many frames.
    Stand(usize),
    /// Walk straight ahead: metres, metres per second.
    Walk(f32, f32),
    /// Turn on the spot: degrees (to the right), frames.
    Turn(f32, usize),
    /// Walk a circle to the right: radius, laps, metres per second.
    Circle(f32, f32, f32),
    /// Stand and measure (a named row of the report).
    Mark(&'static str),
}

/// Where the walker is at one frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pose {
    /// Terrain-local metres (x, z).
    pub at: Vec2,
    /// Heading in radians (0 = +z, turning right).
    pub yaw: f32,
    /// Metres walked so far.
    pub walked: f32,
    /// A measurement starts at this frame.
    pub mark: Option<&'static str>,
}

/// The walker's pose at every frame of `script`, from `start` heading `yaw`.
pub(super) fn poses(script: &[Step], start: Vec2, yaw: f32) -> Vec<Pose> {
    let mut p = Pose {
        at: start,
        yaw,
        walked: 0.0,
        mark: None,
    };
    let mut out = Vec::new();
    let ahead = |yaw: f32| Vec2::new(yaw.sin(), yaw.cos());
    for step in script {
        match *step {
            Step::Stand(frames) => out.extend(std::iter::repeat_n(p, frames)),
            Step::Walk(metres, speed) => {
                let frames = (metres / speed * FPS).ceil().max(1.0) as usize;
                let per = metres / frames as f32;
                for _ in 0..frames {
                    p.at += ahead(p.yaw) * per;
                    p.walked += per;
                    out.push(p);
                }
            }
            Step::Turn(degrees, frames) => {
                let per = degrees.to_radians() / frames.max(1) as f32;
                for _ in 0..frames.max(1) {
                    p.yaw += per;
                    out.push(p);
                }
            }
            Step::Circle(radius, laps, speed) => {
                let metres = std::f32::consts::TAU * radius * laps;
                let frames = (metres / speed * FPS).ceil().max(1.0) as usize;
                let per = metres / frames as f32;
                for _ in 0..frames {
                    p.yaw += per / radius;
                    p.at += ahead(p.yaw) * per;
                    p.walked += per;
                    out.push(p);
                }
            }
            Step::Mark(name) => {
                out.push(Pose {
                    mark: Some(name),
                    ..p
                });
            }
        }
    }
    out
}

/// The report's walk: out 500 m, a look back, round a 500 × 250 m block and out again (2 km),
/// a look back, 300 m back and three circles. `scale` scales the distances.
pub(super) fn long_walk(speed: f32, scale: f32) -> Vec<Step> {
    let m = |metres: f32| metres * scale;
    vec![
        Step::Stand(180),
        Step::Mark("start"),
        Step::Walk(m(500.0), speed),
        Step::Mark("500 m"),
        Step::Turn(180.0, 45),
        Step::Mark("500 m, looking back"),
        Step::Turn(180.0, 45),
        Step::Turn(90.0, 30),
        Step::Walk(m(250.0), speed),
        Step::Turn(90.0, 30),
        Step::Walk(m(500.0), speed),
        Step::Turn(90.0, 30),
        Step::Walk(m(250.0), speed),
        Step::Turn(90.0, 30),
        Step::Walk(m(500.0), speed),
        Step::Mark("2 km"),
        Step::Turn(180.0, 45),
        Step::Mark("2 km, looking back"),
        Step::Walk(m(300.0), speed),
        Step::Circle(m(25.0).max(8.0), 3.0, speed),
        Step::Mark("end"),
    ]
}

/// The process's private bytes and working set (Windows; None elsewhere).
#[cfg(windows)]
fn process_memory() -> Option<(u64, u64)> {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged_pool: usize,
        quota_paged_pool: usize,
        quota_peak_non_paged_pool: usize,
        quota_non_paged_pool: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    // SAFETY: the pseudo handle needs no closing; `c` is a correctly sized, writable struct.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then_some((c.pagefile_usage as u64, c.working_set as u64))
}

#[cfg(not(windows))]
fn process_memory() -> Option<(u64, u64)> {
    None
}

/// What was held at one moment of a walk.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Held {
    /// The grass of every terrain, summed.
    pub grass: GrassLoad,
    /// The lit grass went through the visibility buffer (else the slower lit pass drew it).
    pub vis: bool,
    /// The renderer's own count of its buffers and textures (bytes).
    pub buffers: u64,
    pub textures: u64,
    /// GPU memory the device's allocator holds (bytes; 0 = not reported).
    pub gpu_allocated: u64,
    pub gpu_reserved: u64,
    /// The process's private bytes and working set.
    pub private: u64,
    pub working: u64,
    pub draws: u32,
    pub triangles: u64,
}

fn held(r: &Renderer, device: &wgpu::Device, frame: &crate::render::Frame) -> Held {
    let mut grass = GrassLoad::default();
    for (_, g) in r.terrains.grass_load() {
        grass.pieces += g.pieces;
        grass.blades += g.blades;
        for (a, b) in grass.by_level.iter_mut().zip(g.by_level) {
            *a += b;
        }
        grass.pages += g.pages;
        grass.page_blades += g.page_blades;
        grass.page_free += g.page_free;
        grass.page_longest_free = grass.page_longest_free.max(g.page_longest_free);
        grass.coarser = grass.coarser.max(g.coarser);
        grass.unculled += g.unculled;
        grass.records += g.records;
        grass.records_free += g.records_free;
        grass.in_flight += g.in_flight;
        grass.ready += g.ready;
        grass.cameras += g.cameras;
        grass.dirty += g.dirty;
        grass.culled |= g.culled;
        for (a, b) in grass.drawn.iter_mut().zip(g.drawn) {
            *a += b;
        }
        grass.bytes += g.bytes;
        grass.cull_bytes += g.cull_bytes;
    }
    let memory = r.memory();
    let (private, working) = process_memory().unwrap_or_default();
    let report = device.generate_allocator_report();
    let stats = r.stats();
    Held {
        grass,
        vis: crate::render::grass_vis::grass_vis_enabled()
            && r.terrains.has_grass()
            && r.terrains.grass_vis_ready(frame.terrains),
        buffers: memory.buffer_bytes,
        textures: memory.texture_bytes,
        gpu_allocated: report.as_ref().map_or(0, |a| a.total_allocated_bytes),
        gpu_reserved: report.as_ref().map_or(0, |a| a.total_reserved_bytes),
        private,
        working,
        draws: stats.batches,
        triangles: stats.triangles,
    }
}

/// Median, 95 % and worst of some times (ms).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Times {
    pub median: f32,
    pub p95: f32,
    pub max: f32,
}

fn times(v: &[f32]) -> Times {
    let mut v: Vec<f32> = v.iter().copied().filter(|t| t.is_finite()).collect();
    v.sort_by(f32::total_cmp);
    let q = |f: f32| {
        v.get(((v.len() as f32 * f) as usize).min(v.len().saturating_sub(1)))
            .copied()
            .unwrap_or(f32::NAN)
    };
    Times {
        median: q(0.5),
        p95: q(0.95),
        max: v.last().copied().unwrap_or(f32::NAN),
    }
}

/// One second of a walk.
#[derive(Clone, Copy, Debug)]
pub(super) struct Second {
    pub frame: usize,
    pub walked: f32,
    /// Render call + wait for the GPU, the render call alone, the GPU's own time.
    pub wall: Times,
    pub cpu: Times,
    pub gpu: Times,
    /// Grass chunks uploaded in the second and the most waiting at once.
    pub uploaded: u32,
    pub pending: u32,
    pub held: Held,
}

/// A measurement standing still at a named place of the walk.
#[derive(Clone, Debug)]
pub(super) struct Marked {
    pub name: &'static str,
    pub walked: f32,
    pub wall: Times,
    pub cpu: Times,
    pub gpu: Times,
    pub held: Held,
    /// Median GPU time of each part of the frame.
    pub parts: Vec<(&'static str, f32)>,
}

pub(super) struct WalkRun {
    pub seconds: Vec<Second>,
    pub marks: Vec<Marked>,
}

/// How a walk is rendered.
#[derive(Clone, Copy, Debug)]
pub(super) struct WalkSetup {
    pub size: (u32, u32),
    /// One frame every 1/60 s at most (the workers get the time they have in the editor).
    pub paced: bool,
    /// Frames a measurement stands before it is timed, and frames timed.
    pub settle: usize,
    pub timed: usize,
    /// Time the parts of the frame at every measurement.
    pub parts: bool,
}

/// Walk `script` over the view's first terrain from `from` (terrain-local metres, heading in
/// radians), grass streaming as in the editor.
pub(super) fn walk(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    setup: WalkSetup,
    script: &[Step],
    (start, yaw): (Vec2, f32),
) -> WalkRun {
    let terrain = view.terrains.first().expect("a terrain");
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_view_target(device, setup.size.0, setup.size.1);
    crate::render::set_grass_streaming(true);
    let camera_at = |p: &Pose| {
        let mut cam = view.camera;
        let ground = terrain.data.sample_height(p.at.x, p.at.y);
        cam.position = terrain.position + Vec3::new(p.at.x, ground + EYE, p.at.y);
        cam.rotation = Quat::from_rotation_y(p.yaw);
        cam
    };
    let mut out = WalkRun {
        seconds: Vec::new(),
        marks: Vec::new(),
    };
    let (mut wall, mut cpu, mut gpu) = (Vec::new(), Vec::new(), Vec::new());
    let (mut uploaded, mut pending) = (0, 0);
    let frame_time = Duration::from_secs_f32(1.0 / FPS);
    // One rendered frame: (wall, cpu, gpu) ms.
    let render = |r: &mut Renderer, cam: &crate::camera::Camera| {
        let started = Instant::now();
        let mut frame = view.frame();
        frame.camera = cam;
        r.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let wall = started.elapsed().as_secs_f32() * 1000.0;
        if setup.paced {
            std::thread::sleep(frame_time.saturating_sub(started.elapsed()));
        }
        (wall, r.stats().cpu_ms, r.gpu_ms().unwrap_or(f32::NAN))
    };
    for (i, pose) in poses(script, start, yaw).iter().enumerate() {
        let cam = camera_at(pose);
        if let Some(name) = pose.mark {
            for _ in 0..setup.settle {
                render(&mut r, &cam);
            }
            let (mut w, mut c, mut g) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..setup.timed {
                let t = render(&mut r, &cam);
                w.push(t.0);
                c.push(t.1);
                g.push(t.2);
            }
            let mut frame = view.frame();
            frame.camera = &cam;
            let held = held(&r, device, &frame);
            let mut parts: Vec<(&'static str, Vec<f32>)> = Vec::new();
            if setup.parts && r.set_pass_timing(device, queue, true) {
                for _ in 0..30 {
                    render(&mut r, &cam);
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
                r.set_pass_timing(device, queue, false);
            }
            out.marks.push(Marked {
                name,
                walked: pose.walked,
                wall: times(&w),
                cpu: times(&c),
                gpu: times(&g),
                held,
                parts: (parts.into_iter())
                    .map(|(l, v)| (l, times(&v).median))
                    .collect(),
            });
            continue;
        }
        let t = render(&mut r, &cam);
        wall.push(t.0);
        cpu.push(t.1);
        gpu.push(t.2);
        let grass = r.terrains.grass_stats();
        uploaded += grass.uploaded;
        pending = pending.max(grass.pending);
        if wall.len() == FPS as usize {
            let mut frame = view.frame();
            frame.camera = &cam;
            out.seconds.push(Second {
                frame: i,
                walked: pose.walked,
                wall: times(&wall),
                cpu: times(&cpu),
                gpu: times(&gpu),
                uploaded,
                pending,
                held: held(&r, device, &frame),
            });
            wall.clear();
            cpu.clear();
            gpu.clear();
            (uploaded, pending) = (0, 0);
        }
    }
    crate::render::set_grass_streaming(false);
    out
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn levels(g: &GrassLoad) -> String {
    let l: Vec<String> = g.by_level.iter().map(|n| n.to_string()).collect();
    l.join("/")
}

/// One line of what the grass held.
fn grass_line(h: &Held) -> String {
    let g = &h.grass;
    format!(
        "{} pieces (levels {}), {:.2} M blades, {} pages {:.0} MB ({:.2} M free, longest run {:.2} M), {} levels coarser for lack of room, {} without cull records, culled on the GPU: {}, visibility buffer: {}",
        g.pieces,
        levels(g),
        g.blades as f64 / 1e6,
        g.pages,
        mb(g.page_blades * super::grass_pool::BLADE_BYTES),
        g.page_free as f64 / 1e6,
        g.page_longest_free as f64 / 1e6,
        g.coarser,
        g.unculled,
        g.culled,
        h.vis,
    )
}

fn print_mark(m: &Marked) {
    eprintln!(
        "== {} ({:.0} m walked): wall {:.2} ms ({:.0} FPS), CPU render {:.2} ms, GPU {:.2} ms",
        m.name,
        m.walked,
        m.wall.median,
        1000.0 / m.wall.median,
        m.cpu.median,
        m.gpu.median
    );
    eprintln!("   grass: {}", grass_line(&m.held));
    eprintln!(
        "   grass GPU {:.0} MB; renderer buffers {:.0} MB, textures {:.0} MB; device allocator {:.0} MB used / {:.0} MB reserved; process private {:.0} MB, working set {:.0} MB; {} draws, {:.1} M triangles",
        mb(m.held.grass.bytes),
        mb(m.held.buffers),
        mb(m.held.textures),
        mb(m.held.gpu_allocated),
        mb(m.held.gpu_reserved),
        mb(m.held.private),
        mb(m.held.working),
        m.held.draws,
        m.held.triangles as f64 / 1e6,
    );
    let drawn: Vec<String> = (m.held.grass.drawn.iter())
        .map(|n| format!("{:.2}", *n as f64 / 1e6))
        .collect();
    eprintln!(
        "   blades tested per pass (M: lit, depth, prepass, 4 cascades, 2 near): {}",
        drawn.join(" ")
    );
    let mut parts = m.parts.clone();
    parts.sort_by(|a, b| b.1.total_cmp(&a.1));
    let list: Vec<String> = (parts.iter())
        .filter(|p| p.1 >= 0.15)
        .map(|(l, ms)| format!("{l} {ms:.2}"))
        .collect();
    if !list.is_empty() {
        eprintln!("   GPU parts (ms): {}", list.join(", "));
    }
}

fn env<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok()?.parse().ok()
}

/// A sample scene as saved, seen by its camera.
pub(super) fn scene_view(scene: &str) -> GameView {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    crate::assets::set_project_root(&root);
    let path = root.join(format!("Assets/Scenes/{scene}.scene"));
    let (s, _) = crate::scene::Scene::load(&path).expect("sample scene");
    let materials = crate::assets::AssetCache::open(&root);
    GameView::new(&s, &materials).expect("camera")
}

/// The measuring run: every second of the walk and every measurement, printed.
#[test]
#[ignore = "timing, prints only; minutes long"]
fn grass_walk_report() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let scene = std::env::var("MISTY_WALK_SCENE").unwrap_or_else(|_| "MeadowDemo".into());
    let view = scene_view(&scene);
    let quality = match std::env::var("MISTY_QUALITY").as_deref() {
        Ok("low") => QualityLevel::low(),
        Ok("medium") => QualityLevel::medium(),
        Ok("ultra") => QualityLevel::ultra(),
        _ => QualityLevel::high(),
    };
    let size = std::env::var("MISTY_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((1920, 1080));
    let terrain = view.terrains.first().expect("a terrain");
    let d = &terrain.data;
    let local = view.camera.position - terrain.position;
    let start = std::env::var("MISTY_WALK_START")
        .ok()
        .and_then(|s| {
            let (x, z) = s.split_once(',')?;
            Some(Vec2::new(x.trim().parse().ok()?, z.trim().parse().ok()?))
        })
        .unwrap_or(Vec2::new(local.x, local.z));
    let forward = view.camera.forward();
    let yaw = env::<f32>("MISTY_WALK_YAW").map_or(forward.x.atan2(forward.z), f32::to_radians);
    let speed = env("MISTY_WALK_SPEED").unwrap_or(5.0);
    let scale = env("MISTY_WALK_SCALE").unwrap_or(1.0);
    let script = long_walk(speed, scale);
    let path = poses(&script, start, yaw);
    // How much of the path is over grass, and whether it leaves the terrain.
    let n = d.alphamap_resolution;
    let mut over = (0usize, 0usize, 0usize);
    for p in &path {
        let inside =
            p.at.cmpge(Vec2::ZERO).all() && p.at.cmplt(Vec2::new(d.size.x, d.size.z)).all();
        over.0 += usize::from(!inside);
        if inside && d.detail_map.len() == n * n {
            let tx = ((p.at.x / d.size.x * n as f32) as usize).min(n - 1);
            let tz = ((p.at.y / d.size.z * n as f32) as usize).min(n - 1);
            over.1 += usize::from(d.detail_map[tz * n + tx] > 0);
        }
        over.2 += 1;
    }
    let s = &d.details;
    eprintln!(
        "{scene}: terrain {} × {} m at {:?}; grass {} blades/m², draw distance {} m, LOD distance {} m; walk from ({:.0}, {:.0}) heading {:.0}°, {} m/s, {} frames, {:.0} % of them over grass, {} outside the terrain",
        d.size.x,
        d.size.z,
        terrain.position,
        s.density,
        s.draw_distance,
        s.lod_distance,
        start.x,
        start.y,
        yaw.to_degrees(),
        speed,
        path.len(),
        over.1 as f32 * 100.0 / over.2.max(1) as f32,
        over.0,
    );
    let limits = device.limits();
    eprintln!(
        "device: storage binding {} MB, buffer {} MB; quality {}, {}x{}",
        limits.max_storage_buffer_binding_size >> 20,
        limits.max_buffer_size >> 20,
        std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high".into()),
        size.0,
        size.1
    );
    let setup = WalkSetup {
        size,
        paced: env::<u32>("MISTY_WALK_PACE") != Some(0),
        settle: 120,
        timed: 120,
        parts: true,
    };
    let run = walk(
        &device,
        &queue,
        &view,
        &quality,
        setup,
        &script,
        (start, yaw),
    );
    eprintln!("every tenth second of the walk:");
    for s in run.seconds.iter().step_by(10) {
        let g = &s.held.grass;
        eprintln!(
            "  {:6.0} m: wall {:5.2} (p95 {:5.2}, max {:6.2}) cpu {:5.2} gpu {:5.2} ms | {:4} pieces {:5.2} M blades, {} pages, {} coarser, {:3} unculled, vis {:5} | {:3} pending, {:3} uploaded | grass {:4.0} MB, device {:5.0} MB, private {:5.0} MB",
            s.walked,
            s.wall.median,
            s.wall.p95,
            s.wall.max,
            s.cpu.median,
            s.gpu.median,
            g.pieces,
            g.blades as f64 / 1e6,
            g.pages,
            g.coarser,
            g.unculled,
            s.held.vis,
            s.pending,
            s.uploaded,
            mb(g.bytes),
            mb(s.held.gpu_allocated),
            mb(s.held.private),
        );
    }
    for m in &run.marks {
        print_mark(m);
    }
    if let Ok(path) = std::env::var("MISTY_WALK_CSV") {
        let mut csv = String::from(
            "frame,walked_m,wall_ms,wall_p95_ms,wall_max_ms,cpu_ms,gpu_ms,pieces,blades,pages,page_blades,page_free,longest_free,coarser,unculled,culled,vis,pending,uploaded,in_flight,ready,cameras,grass_bytes,buffers,textures,gpu_allocated,gpu_reserved,private,working,draws,triangles,lit_tested\n",
        );
        for s in &run.seconds {
            let g = &s.held.grass;
            csv.push_str(&format!(
                "{},{:.1},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                s.frame,
                s.walked,
                s.wall.median,
                s.wall.p95,
                s.wall.max,
                s.cpu.median,
                s.gpu.median,
                g.pieces,
                g.blades,
                g.pages,
                g.page_blades,
                g.page_free,
                g.page_longest_free,
                g.coarser,
                g.unculled,
                u8::from(g.culled),
                u8::from(s.held.vis),
                s.pending,
                s.uploaded,
                g.in_flight,
                g.ready,
                g.cameras,
                g.bytes,
                s.held.buffers,
                s.held.textures,
                s.held.gpu_allocated,
                s.held.gpu_reserved,
                s.held.private,
                s.held.working,
                s.held.draws,
                s.held.triangles,
                g.drawn[0],
            ));
        }
        std::fs::write(&path, csv).expect("csv");
        eprintln!("wrote {path}");
    }
}

#[test]
fn poses_follow_the_script() {
    let script = [
        Step::Stand(3),
        Step::Walk(10.0, 5.0),
        Step::Mark("far"),
        Step::Turn(180.0, 10),
        Step::Walk(10.0, 5.0),
        Step::Circle(10.0, 1.0, 5.0),
    ];
    let path = poses(&script, Vec2::new(100.0, 100.0), 0.0);
    // 3 standing, 120 walking out, the mark, 10 turning, 120 back, the circle.
    let circle = (std::f32::consts::TAU * 10.0 / 5.0 * FPS).ceil() as usize;
    assert_eq!(path.len(), 3 + 120 + 1 + 10 + 120 + circle);
    let far = path.iter().find(|p| p.mark.is_some()).unwrap();
    assert!((far.at - Vec2::new(100.0, 110.0)).length() < 1e-3);
    assert!((far.walked - 10.0).abs() < 1e-3);
    // Back at the start after the walk back, and again after the circle.
    let back = path[3 + 120 + 1 + 10 + 120 - 1];
    assert!((back.at - Vec2::new(100.0, 100.0)).length() < 1e-2);
    let end = path.last().unwrap();
    assert!((end.at - Vec2::new(100.0, 100.0)).length() < 0.2);
    assert!((end.walked - (20.0 + std::f32::consts::TAU * 10.0)).abs() < 1e-2);
}
