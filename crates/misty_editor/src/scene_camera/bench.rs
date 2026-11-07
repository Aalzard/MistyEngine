//! Scene camera flythrough benchmark (measuring only).
//!
//! `MISTY_CAMERA_BENCH=<frames>` with a screenshot run flies the Scene camera forward while
//! turning (the same smoothing path as right mouse + W), then prints frame pacing, CPU and
//! GPU times, grass uploads and the input-to-GPU-done latency over `<frames>` frames:
//!
//! `MISTY_SCREENSHOT=out.png MISTY_SCENE=Assets/Scenes/MeadowDemo.scene MISTY_TAB=Scene
//! MISTY_CAMERA_BENCH=600 MISTY_CAPTURE_FRAME=800 misty SampleProject`
//!
//! The latency is from the start of the editor frame (where the input is read) until the GPU
//! finished that frame's Scene view image; a polling thread notices it within ~0.25 ms. The
//! window's present and the display scan-out come on top.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};

use super::{Drag, NavInput, SceneCamera};

/// Frames before measuring (shaders, grass streaming in at the start view).
const WARM_UP: u32 = 90;
/// Turn rate while flying, degrees per second.
const TURN: f32 = 30.0;

#[derive(Default)]
struct Record {
    interval_ms: f32,
    ui_ms: f32,
    scene_cpu_ms: f32,
    gpu_ms: Option<f32>,
    grass_upload_ms: f32,
    /// Upload time already spent when the frame's last chunk started.
    grass_last_start_ms: f32,
    grass_pending: u32,
    /// Camera eye movement this frame / interval (m/s).
    speed: f32,
}

#[derive(Default)]
struct Bench {
    frames: u32,
    measure: u32,
    started: Option<Instant>,
    current: Record,
    records: Vec<Record>,
    /// Frame intervals with the camera still (the second half of the warm-up).
    still_ms: Vec<f32>,
    latencies: Arc<Mutex<Vec<f32>>>,
    eye: Option<Vec3>,
    /// Seconds flown so far.
    flown: f32,
    reported: bool,
    poller: bool,
}

fn bench() -> Option<&'static Mutex<Bench>> {
    static B: OnceLock<Option<Mutex<Bench>>> = OnceLock::new();
    B.get_or_init(|| {
        let n = std::env::var("MISTY_CAMERA_BENCH")
            .ok()?
            .parse::<u32>()
            .ok()?;
        Some(Mutex::new(Bench {
            measure: n.max(10),
            ..Default::default()
        }))
    })
    .as_ref()
}

fn measuring(b: &Bench) -> bool {
    b.frames > WARM_UP && b.frames <= WARM_UP + b.measure
}

/// Start of an editor frame.
pub fn frame_start(ctx: &egui::Context) {
    let Some(b) = bench() else { return };
    let mut b = b.lock().unwrap();
    b.frames += 1;
    b.started = Some(Instant::now());
    b.current = Record {
        interval_ms: ctx.input(|i| i.unstable_dt) * 1000.0,
        ..Default::default()
    };
    ctx.request_repaint();
}

/// End of an editor frame (before egui paints and presents).
pub fn frame_end() {
    let Some(b) = bench() else { return };
    let mut b = b.lock().unwrap();
    if let Some(t) = b.started {
        b.current.ui_ms = t.elapsed().as_secs_f32() * 1000.0;
    }
    if measuring(&b) {
        let r = std::mem::take(&mut b.current);
        b.records.push(r);
    } else if b.frames > WARM_UP / 2 && b.frames <= WARM_UP {
        let ms = b.current.interval_ms;
        b.still_ms.push(ms);
    }
    if b.frames == WARM_UP + b.measure && !b.reported {
        b.reported = true;
        report(&b);
    }
}

/// Drive the camera instead of the mouse and keyboard; true while the bench runs.
pub fn drive(cam: &mut SceneCamera, view_height: f32, dt: f32) -> bool {
    let Some(b) = bench() else { return false };
    let mut b = b.lock().unwrap();
    if b.frames <= WARM_UP {
        b.eye = Some(cam.position());
        return true;
    }
    cam.drag = Drag::Look;
    let input = NavInput {
        turn: Vec2::new(TURN * dt / super::LOOK_SPEED, 0.0),
        fly: Vec3::Z,
        ..Default::default()
    };
    cam.update(&input, view_height, dt);
    b.flown += dt;
    let eye = cam.position();
    // Speed per frame once the acceleration has reached full speed (after 2 s).
    if let Some(prev) = b.eye.replace(eye)
        && b.flown > 2.5
    {
        b.current.speed = (eye - prev).length() / (b.current.interval_ms / 1000.0).max(1e-4);
    }
    true
}

/// The Scene view image was just submitted: note the renderer's times and when the GPU is
/// done with it.
pub fn scene_rendered(gpu: &crate::viewport::Gpu) {
    let Some(b) = bench() else { return };
    let mut b = b.lock().unwrap();
    let stats = gpu.renderer.stats();
    b.current.scene_cpu_ms = stats.cpu_ms;
    b.current.grass_upload_ms = stats.grass_upload_ms;
    b.current.grass_last_start_ms = stats.grass_upload_last_start_ms;
    b.current.grass_pending = stats.grass_pending;
    b.current.gpu_ms = gpu.renderer.gpu_ms();
    if !b.poller {
        b.poller = true;
        let device = gpu.state.device.clone();
        std::thread::spawn(move || {
            loop {
                let _ = device.poll(misty_engine::wgpu::PollType::Poll);
                std::thread::sleep(Duration::from_micros(250));
            }
        });
    }
    if measuring(&b)
        && let Some(start) = b.started
    {
        let out = b.latencies.clone();
        gpu.state.queue.on_submitted_work_done(move || {
            out.lock()
                .unwrap()
                .push(start.elapsed().as_secs_f32() * 1000.0);
        });
    }
}

fn pct(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

fn sorted(v: impl Iterator<Item = f32>) -> Vec<f32> {
    let mut v: Vec<f32> = v.collect();
    v.sort_by(f32::total_cmp);
    v
}

fn mean_cv(v: &[f32]) -> (f32, f32) {
    let n = v.len().max(1) as f32;
    let mean = v.iter().sum::<f32>() / n;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n;
    (mean, var.sqrt() / mean.max(1e-6))
}

fn report(b: &Bench) {
    let r = &b.records;
    // `MISTY_CAMERA_BENCH_CSV=<file>`: every measured frame.
    if let Some(path) = std::env::var_os("MISTY_CAMERA_BENCH_CSV") {
        let mut csv = String::from(
            "interval_ms,ui_ms,scene_cpu_ms,gpu_ms,grass_upload_ms,grass_last_start_ms,grass_pending,speed
",
        );
        for f in r {
            csv += &format!(
                "{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{:.3}
",
                f.interval_ms,
                f.ui_ms,
                f.scene_cpu_ms,
                f.gpu_ms.unwrap_or(f32::NAN),
                f.grass_upload_ms,
                f.grass_last_start_ms,
                f.grass_pending,
                f.speed
            );
        }
        let _ = std::fs::write(path, csv);
    }
    let interval = sorted(r.iter().map(|r| r.interval_ms));
    let med = pct(&interval, 0.5);
    let (mean, cv) = mean_cv(&interval);
    let hitches: Vec<&Record> = r.iter().filter(|r| r.interval_ms > med * 1.5).collect();
    let budget = 2.0;
    let with_upload = hitches
        .iter()
        .filter(|r| r.grass_upload_ms > budget)
        .count();
    let slow_chunk = r
        .iter()
        .filter(|r| r.grass_upload_ms - r.grass_last_start_ms > budget)
        .count();
    let ui = sorted(r.iter().map(|r| r.ui_ms));
    let wait = sorted(r.iter().map(|r| (r.interval_ms - r.ui_ms).max(0.0)));
    let cpu = sorted(r.iter().map(|r| r.scene_cpu_ms));
    let gpu = sorted(r.iter().filter_map(|r| r.gpu_ms));
    let uploads = sorted(r.iter().map(|r| r.grass_upload_ms).filter(|m| *m > 0.01));
    let pending = r.iter().filter(|r| r.grass_pending > 0).count();
    let speeds: Vec<f32> = r.iter().map(|r| r.speed).filter(|s| *s > 0.0).collect();
    let (speed, speed_cv) = mean_cv(&speeds);
    let lat = sorted(b.latencies.lock().unwrap().iter().copied());
    let still = sorted(b.still_ms.iter().copied());
    eprintln!(
        "camera bench over {} frames (flying, turning {TURN} deg/s):",
        r.len()
    );
    eprintln!(
        "  camera still (before flying): median {:.2} ms ({:.0} FPS) over {} frames",
        pct(&still, 0.5),
        1000.0 / pct(&still, 0.5),
        still.len()
    );
    eprintln!(
        "  frame interval: median {med:.2} ms ({:.0} FPS), mean {mean:.2}, p90 {:.2}, p99 {:.2}, max {:.2}, jitter (CV) {:.1} %",
        1000.0 / med,
        pct(&interval, 0.9),
        pct(&interval, 0.99),
        pct(&interval, 1.0),
        cv * 100.0
    );
    eprintln!(
        "  hitches (> 1.5x median): {} ({} with over 2 ms of grass upload that frame)",
        hitches.len(),
        with_upload
    );
    eprintln!("  frames where one grass chunk upload alone took over 2 ms: {slow_chunk}");
    eprintln!(
        "  editor ui() CPU: median {:.2} ms, p99 {:.2}; outside ui() (paint, present, wait): median {:.2} ms, p99 {:.2}",
        pct(&ui, 0.5),
        pct(&ui, 0.99),
        pct(&wait, 0.5),
        pct(&wait, 0.99)
    );
    eprintln!(
        "  Scene render CPU: median {:.2} ms, max {:.2}; GPU frame: median {:.2} ms, p99 {:.2}",
        pct(&cpu, 0.5),
        pct(&cpu, 1.0),
        pct(&gpu, 0.5),
        pct(&gpu, 0.99)
    );
    eprintln!(
        "  grass: uploads in {} frames (median {:.2} ms, max {:.2}), chunks pending in {} frames",
        uploads.len(),
        pct(&uploads, 0.5),
        pct(&uploads, 1.0),
        pending
    );
    eprintln!(
        "  input -> GPU done: median {:.1} ms ({:.2} frames), p90 {:.1} ms, max {:.1} ms",
        pct(&lat, 0.5),
        pct(&lat, 0.5) / med,
        pct(&lat, 0.9),
        pct(&lat, 1.0)
    );
    eprintln!(
        "  camera speed per frame: mean {speed:.2} m/s, variation (CV) {:.2} %",
        speed_cv * 100.0
    );
}
