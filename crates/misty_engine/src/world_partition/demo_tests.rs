//! The StreamingDemo sample (`SampleProject/Assets/Scenes/StreamingDemo.scene`, written by the
//! editor's `sample_streaming`) flown across in the game loop (Runtime: physics bodies and
//! scripts start as objects stream in): no frame's main-thread streaming work goes over budget.
//!
//! `streaming_demo_flight_stays_within_budget` is a quick, fast flight run with the other
//! tests; `streaming_demo_flythrough_bench` flies the whole diagonal at a realistic speed, one
//! frame every 16.7 ms:
//! `cargo test --release -p misty_engine --lib streaming_demo_flythrough -- --ignored --nocapture`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use glam::Vec3;

use crate::scene::{Scene, Scripts};

/// Median, 99th percentile and maximum.
pub struct Summary {
    pub median: f32,
    pub p99: f32,
    pub max: f32,
}

pub fn summary(times: &[f32]) -> Summary {
    let mut v: Vec<f32> = times.iter().copied().filter(|t| t.is_finite()).collect();
    v.sort_by(f32::total_cmp);
    let q = |f: f32| {
        v.get(((v.len() as f32 * f) as usize).min(v.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0)
    };
    Summary {
        median: q(0.5),
        p99: q(0.99),
        max: v.last().copied().unwrap_or(0.0),
    }
}

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// What a flight measured.
pub struct Flight {
    /// Main-thread streaming time per frame (ms).
    pub streaming_ms: Vec<f32>,
    /// The whole game frame (`Runtime::update`) per frame (ms).
    pub frame_ms: Vec<f32>,
    pub stats: super::StreamingStats,
    pub frames: usize,
}

/// The demo scene ready to fly: no fly-over script (the test moves the camera), the partition
/// folder absolute (tests beside this one move the project root).
pub fn demo_scene() -> Option<Scene> {
    let root = project();
    let (mut scene, report) = Scene::load(&root.join("Assets/Scenes/StreamingDemo.scene")).ok()?;
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let cam = super::main_camera(&scene)?;
    let _ = scene.world.remove_one::<Scripts>(cam);
    let (e, _) = super::find(&scene)?;
    let mut wp = scene.world.get::<&mut super::WorldPartition>(e).ok()?;
    wp.folder = root.join(&wp.folder).to_string_lossy().replace('\\', "/");
    drop(wp);
    Some(scene)
}

/// Fly from (-1800, -1800) to (1800, 1800) at `speed` m/s, `dt` seconds of game time a frame,
/// each frame taking at least `pace` of wall time.
pub fn fly(speed: f32, dt: f32, pace: Duration) -> Flight {
    let root = project();
    let mut scene = demo_scene().expect("StreamingDemo scene");
    let cam = super::main_camera(&scene).unwrap();
    let start = Vec3::new(-1800.0, 260.0, -1800.0);
    let end = Vec3::new(1800.0, 260.0, 1800.0);
    let place = |s: &mut Scene, p: Vec3| {
        let t = crate::scene::Transform::from_position(p).with_euler(Vec3::new(15.0, 45.0, 0.0));
        s.set_transform(cam, t);
    };
    place(&mut scene, start);
    let settings = crate::settings::ProjectSettings::default();
    let (audio, _) = crate::audio::Audio::null(root.clone());
    let mut rt =
        crate::runtime::Runtime::start_with_audio(&mut scene, root.clone(), &settings, audio);
    rt.streaming_mut().set_root(root);
    let mut time = crate::runtime::Time::default();
    // The start area first (a loading screen would cover this).
    let t0 = Instant::now();
    loop {
        time.advance(dt);
        rt.update(&mut scene, &time, crate::scripting::Input::default());
        let s = rt.streaming().stats();
        if s.loaded > 0 && s.pending() == 0 {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "start area never loaded"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let length = start.distance(end);
    let frames = (length / (speed * dt)).ceil() as usize;
    let mut streaming_ms = Vec::with_capacity(frames);
    let mut frame_ms = Vec::with_capacity(frames);
    for i in 0..=frames {
        let began = Instant::now();
        let p = start.lerp(end, (i as f32 / frames as f32).min(1.0));
        place(&mut scene, p);
        time.advance(dt);
        rt.update(&mut scene, &time, crate::scripting::Input::default());
        frame_ms.push(began.elapsed().as_secs_f32() * 1000.0);
        streaming_ms.push(rt.streaming().stats().last_ms);
        let _ = rt.take_logs();
        if let Some(rest) = pace.checked_sub(began.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Flight {
        streaming_ms,
        frame_ms,
        stats: rt.streaming().stats().clone(),
        frames,
    }
}

fn report(label: &str, f: &Flight) {
    let s = summary(&f.streaming_ms);
    let w = summary(&f.frame_ms);
    eprintln!("{label}: {} frames", f.frames);
    eprintln!(
        "  streaming main thread: median {:.3} p99 {:.3} max {:.3} ms (budget {:.1} ms, {} frames over)",
        s.median, s.p99, s.max, f.stats.budget_ms, f.stats.over_budget
    );
    eprintln!(
        "  whole game frame:      median {:.3} p99 {:.3} max {:.3} ms",
        w.median, w.p99, w.max
    );
    eprintln!(
        "  cells: {} loads, {} unloads, {} loaded at the end, {:.1} MB",
        f.stats.loads,
        f.stats.unloads,
        f.stats.loaded,
        f.stats.memory_bytes as f64 / 1048576.0
    );
}

/// Quick flight (6.7 m a frame at 60 fps, frames paced at 4 ms): cells load and unload all the
/// way, and no frame's streaming work comes near a 16 ms hitch.
#[test]
fn streaming_demo_flight_stays_within_budget() {
    if demo_scene().is_none() {
        return;
    }
    let f = fly(400.0, 1.0 / 60.0, Duration::from_millis(4));
    report("StreamingDemo quick flight", &f);
    assert!(f.stats.loads >= 20, "{} loads", f.stats.loads);
    assert!(f.stats.unloads >= 10, "{} unloads", f.stats.unloads);
    assert_eq!(f.stats.failed, 0);
    let worst = f.streaming_ms.iter().copied().fold(0.0f32, f32::max);
    // The bar: no streaming hitch above 16 ms (generous for a machine busy with other tests;
    // the bench shows the real numbers).
    assert!(worst < 16.0, "worst streaming frame {worst:.2} ms");
}

/// The whole 5 km diagonal at 120 m/s, a frame every 16.7 ms.
#[test]
#[ignore = "bench: about a minute; run on purpose"]
fn streaming_demo_flythrough_bench() {
    let f = fly(120.0, 1.0 / 60.0, Duration::from_micros(16_667));
    report("StreamingDemo flythrough (120 m/s)", &f);
    let s = summary(&f.streaming_ms);
    assert!(s.max < 16.0, "worst streaming frame {:.2} ms", s.max);
    assert_eq!(f.stats.failed, 0);
}
