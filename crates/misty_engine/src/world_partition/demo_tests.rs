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
use crate::test_cpu as cpu;

/// The start area fails to load when no cell made progress for this long.
const STALLED: Duration = Duration::from_secs(120);
/// A frame whose main-thread streaming work is over this is a hitch.
const HITCH_MS: f32 = 16.0;
/// The machine counts as loaded during a frame when at least this share of its CPU time was
/// busy (threads then queue for cores; see `render::hitch_tests`).
const LOADED: f32 = 0.85;

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
    /// CPU time the main thread ran during each frame's `Runtime::update` (ms; `None` where
    /// the OS can't tell). Unlike the wall times it does not grow while the thread waits for
    /// a core on a loaded machine.
    pub work_ms: Vec<Option<f32>>,
    /// Share of the machine's CPU time that was busy during each frame, and over the flight.
    pub busy: Vec<Option<f32>>,
    pub flight_busy: Option<f32>,
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
    // The start area first (a loading screen would cover this). Cells load on worker threads
    // that a busy machine slows down, so this waits as long as loading moves on and gives up
    // only when nothing changed for a long time.
    let mut progress = Instant::now();
    let mut last = (0, 0, 0);
    loop {
        time.advance(dt);
        rt.update(&mut scene, &time, crate::scripting::Input::default());
        let s = rt.streaming().stats();
        if s.loaded > 0 && s.pending() == 0 {
            break;
        }
        let now = (s.loaded, s.reading, s.adding);
        if now != last {
            last = now;
            progress = Instant::now();
        }
        assert!(
            progress.elapsed() < STALLED,
            "start area stopped loading: no progress for {} s ({} loaded, {} reading, {} waiting, {} adding, {} failed)",
            STALLED.as_secs(),
            s.loaded,
            s.reading,
            s.waiting,
            s.adding,
            s.failed
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let length = start.distance(end);
    let frames = (length / (speed * dt)).ceil() as usize;
    let mut streaming_ms = Vec::with_capacity(frames);
    let mut frame_ms = Vec::with_capacity(frames);
    let mut work_ms = Vec::with_capacity(frames);
    let mut busy = Vec::with_capacity(frames);
    let rate = cpu::work_per_ms();
    let flight_start = cpu::system_times();
    for i in 0..=frames {
        let began = Instant::now();
        let p = start.lerp(end, (i as f32 / frames as f32).min(1.0));
        place(&mut scene, p);
        time.advance(dt);
        let (work0, system0) = (cpu::thread_work(), cpu::system_times());
        rt.update(&mut scene, &time, crate::scripting::Input::default());
        let work = work0.zip(cpu::thread_work()).zip(rate);
        work_ms.push(work.map(|((a, b), r)| (b.saturating_sub(a) as f64 / r) as f32));
        busy.push(cpu::busy_between(system0, cpu::system_times()));
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
        work_ms,
        busy,
        flight_busy: cpu::busy_between(flight_start, cpu::system_times()),
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
        "  whole game frame: median {:.3} p99 {:.3} max {:.3} ms",
        w.median, w.p99, w.max
    );
    eprintln!(
        "  cells: {} loads, {} unloads, {} loaded at the end, {:.1} MB",
        f.stats.loads,
        f.stats.unloads,
        f.stats.loaded,
        f.stats.memory_bytes as f64 / 1048576.0
    );
    if let Some((what, ms)) = &f.stats.slowest {
        eprintln!("  slowest single step: {what} ({ms:.2} ms)");
    }
    let (hits, misses) = crate::physics::shape_cache::counts();
    eprintln!("  Mesh Collider shapes built ahead: {hits}, on the main thread: {misses}");
}

/// Frames whose streaming took over [`HITCH_MS`] of wall time. Each is a hitch (`Err`) unless
/// the main thread did less than that much work in the whole game frame (its CPU time, which
/// waiting for a core does not add to) while the machine was loaded: then the streaming work
/// itself was short and the thread only waited for a core (`Ok`, printed). A slow frame on a
/// machine with cores to spare is ours either way (a lock, a file read, a blocking call).
pub fn hitches(f: &Flight) -> Vec<Result<String, String>> {
    f.streaming_ms
        .iter()
        .enumerate()
        .filter(|(_, ms)| **ms >= HITCH_MS)
        .map(|(i, ms)| {
            let work = f.work_ms.get(i).copied().flatten();
            let busy = f.busy.get(i).copied().flatten().or(f.flight_busy);
            let text = format!(
                "frame {i}: streaming {ms:.1} ms, whole frame {:.1} ms wall / {} ms main-thread CPU, machine {} busy",
                f.frame_ms[i],
                work.map_or("?".to_owned(), |w| format!("{w:.1}")),
                busy.map_or("?".to_owned(), |b| format!("{:.0} %", b * 100.0)),
            );
            match (work, busy) {
                (Some(w), Some(b)) if w < HITCH_MS && b >= LOADED => Ok(text),
                _ => Err(text),
            }
        })
        .collect()
}

/// Quick flight (6.7 m a frame at 60 fps, frames paced at 4 ms): cells load and unload all the
/// way, and no frame's streaming work comes near a 16 ms hitch. Wall-clock spikes from a
/// machine busy with other tests are told apart from ours by the main thread's CPU time (see
/// [`hitches`]); a flight with a real hitch gets two more tries.
#[test]
fn streaming_demo_flight_stays_within_budget() {
    if demo_scene().is_none() {
        return;
    }
    let mut found = Vec::new();
    for round in 1..=3 {
        let f = fly(400.0, 1.0 / 60.0, Duration::from_millis(4));
        report(&format!("StreamingDemo quick flight {round}"), &f);
        assert!(f.stats.loads >= 20, "{} loads", f.stats.loads);
        assert!(f.stats.unloads >= 10, "{} unloads", f.stats.unloads);
        assert_eq!(f.stats.failed, 0);
        found.clear();
        for h in hitches(&f) {
            match h {
                Ok(excused) => eprintln!("  excused (machine loaded, little work): {excused}"),
                Err(hitch) => found.push(hitch),
            }
        }
        if found.is_empty() {
            return;
        }
        let (what, ms) = f.stats.slowest.clone().unwrap_or_default();
        found.push(format!("slowest single step: {what} ({ms:.2} ms)"));
    }
    panic!("streaming hitches over {HITCH_MS} ms in all three flights (the last's): {found:#?}");
}

/// The rule that excuses a slow frame: little main-thread work on a loaded machine only.
#[test]
fn slow_streaming_frames_are_excused_only_on_a_loaded_machine() {
    let flight = |work: Option<f32>, busy: Option<f32>| Flight {
        streaming_ms: vec![1.0, 40.0],
        frame_ms: vec![2.0, 45.0],
        work_ms: vec![Some(1.0), work],
        busy: vec![Some(0.5), busy],
        flight_busy: None,
        stats: super::StreamingStats::default(),
        frames: 2,
    };
    let verdict = |f: &Flight| {
        let h = hitches(f);
        assert_eq!(h.len(), 1, "only the slow frame: {h:?}");
        h[0].is_ok()
    };
    assert!(verdict(&flight(Some(3.0), Some(0.97))), "waited for a core");
    assert!(
        !verdict(&flight(Some(30.0), Some(0.97))),
        "real work on a loaded machine"
    );
    assert!(
        !verdict(&flight(Some(3.0), Some(0.3))),
        "a wait of ours on an idle machine"
    );
    assert!(
        !verdict(&flight(None, Some(0.97))),
        "no CPU time: wall time counts"
    );
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

/// A player build packs the cells, their terrain tiles, the HLOD proxies and their material:
/// the World Partition's cell list stands for its files.
#[test]
fn a_build_of_the_demo_packs_every_cell_tile_and_proxy() {
    let root = project();
    if !root.join("Assets/Scenes/StreamingDemo.scene").exists() {
        return;
    }
    let deps = crate::asset_deps::collect(&root, &["Assets/Scenes/StreamingDemo.scene".into()]);
    let has = |p: &str| deps.files.contains(p);
    assert!(has("Assets/Scenes/StreamingDemo/Cells/Cell_3_4.scene"));
    assert!(has("Assets/Scenes/StreamingDemo/Terrain/Tile_3_4.terrain"));
    assert!(has("Assets/Scenes/StreamingDemo/HLOD/Cell_3_4.hlod"));
    assert!(has("Assets/Scenes/StreamingDemo/HLOD/HLOD.mat"));
    assert!(has("Assets/Scenes/StreamingDemo/HLOD/HLOD.shader"));
    assert!(has("Assets/Trees/Oak.tree"));
    assert!(
        has("Assets/Rocks/Prefabs/Boulder Large.prefab")
            || has("Assets/Rocks/Meshes/Boulder Large LODs.glb")
    );
    let cells = deps
        .files
        .iter()
        .filter(|f| f.contains("/Cells/") && f.ends_with(".scene"))
        .count();
    assert_eq!(cells, 64);
    assert!(deps.missing.is_empty(), "{:?}", deps.missing);
}
