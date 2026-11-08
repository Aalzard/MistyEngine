//! StreamingDemo flown across with the renderer, every frame timed: the game update (World
//! Partition streaming with physics and scripts for the objects that arrive), gathering the
//! view (draw cache, terrains, HLOD proxies), the render call and the wait for the GPU. Frames
//! where cells finished loading or unloading are compared with the rest, so a hitch caused by
//! streaming (main thread or render thread: new tiles uploaded, draw lists relaid) shows.
//! `cargo test --release -p misty_engine --lib streaming_flythrough_frames -- --ignored --nocapture --test-threads=1`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use glam::Vec3;

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::headless;
use crate::render_scene::GameView;
use crate::scene::{Scene, Scripts, Transform};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

#[derive(Clone, Copy, Default)]
struct Frame {
    update_ms: f32,
    streaming_ms: f32,
    gather_ms: f32,
    render_ms: f32,
    wall_ms: f32,
    /// Cells finished loading or unloading this frame.
    events: u64,
    loaded: usize,
    pending: usize,
}

fn quantile(v: &[f32], q: f32) -> f32 {
    let mut s: Vec<f32> = v.to_vec();
    s.sort_by(f32::total_cmp);
    s.get(((s.len() as f32 * q) as usize).min(s.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

fn line(name: &str, v: &[f32]) -> String {
    format!(
        "{name}: median {:.2} p95 {:.2} p99 {:.2} max {:.2} ms",
        quantile(v, 0.5),
        quantile(v, 0.95),
        quantile(v, 0.99),
        v.iter().copied().fold(0.0f32, f32::max)
    )
}

#[test]
#[ignore = "GPU bench over the StreamingDemo sample; run on purpose"]
fn streaming_flythrough_frames() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter");
        return;
    };
    let root = root();
    crate::assets::set_project_root(&root);
    // The real thing: grass chunks and tree models stream in on workers.
    crate::render::set_grass_streaming(true);
    let Ok((mut scene, _)) = Scene::load(&root.join("Assets/Scenes/StreamingDemo.scene")) else {
        eprintln!("StreamingDemo scene missing");
        return;
    };
    let cam = crate::world_partition::main_camera(&scene).unwrap();
    let _ = scene.world.remove_one::<Scripts>(cam);
    let start = Vec3::new(-1800.0, 260.0, -1800.0);
    let end = Vec3::new(1800.0, 260.0, 1800.0);
    let place = |s: &mut Scene, p: Vec3| {
        s.set_transform(
            cam,
            Transform::from_position(p).with_euler(Vec3::new(12.0, 45.0, 0.0)),
        );
    };
    place(&mut scene, start);
    let settings = crate::settings::ProjectSettings::default();
    let (audio, _) = crate::audio::Audio::null(root.clone());
    let mut rt =
        crate::runtime::Runtime::start_with_audio(&mut scene, root.clone(), &settings, audio);
    let materials = crate::assets::AssetCache::open(&root);
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &QualityLevel::high());
    let (w, h) = (1280, 720);
    let target = r.create_target(&device, w, h);
    let mut cache = crate::draw_cache::DrawCache::new();
    let mut time = crate::runtime::Time::default();
    let dt = 1.0 / 60.0;
    let mut step =
        |scene: &mut Scene, rt: &mut crate::runtime::Runtime, time: &mut crate::runtime::Time| {
            let began = Instant::now();
            time.advance(dt);
            rt.update(scene, time, crate::scripting::Input::default());
            let update_ms = began.elapsed().as_secs_f32() * 1000.0;
            let g = Instant::now();
            let view = GameView::cached(scene, &materials, &mut cache).expect("camera");
            let gather_ms = g.elapsed().as_secs_f32() * 1000.0;
            let rr = Instant::now();
            r.render(&device, &queue, &target, &view.frame());
            let render_ms = rr.elapsed().as_secs_f32() * 1000.0;
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            let s = rt.streaming().stats().clone();
            let _ = rt.take_logs();
            Frame {
                update_ms,
                streaming_ms: s.last_ms,
                gather_ms,
                render_ms,
                wall_ms: began.elapsed().as_secs_f32() * 1000.0,
                events: s.loads + s.unloads,
                loaded: s.loaded,
                pending: s.pending(),
            }
        };
    // The start area loads and the first renders build their GPU resources (a loading screen).
    let t0 = Instant::now();
    loop {
        let f = step(&mut scene, &mut rt, &mut time);
        if f.loaded > 0 && f.pending == 0 && t0.elapsed() > Duration::from_secs(5) {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(120),
            "start area never loaded"
        );
    }
    for _ in 0..120 {
        step(&mut scene, &mut rt, &mut time);
    }
    // Still frames (no streaming) for the baseline, then the flight at 120 m/s.
    let still: Vec<Frame> = (0..120)
        .map(|_| step(&mut scene, &mut rt, &mut time))
        .collect();
    let speed = 120.0;
    let frames = (start.distance(end) / (speed * dt)).ceil() as usize;
    let mut flight = Vec::with_capacity(frames);
    let mut prev_events = still.last().map_or(0, |f| f.events);
    for i in 0..=frames {
        place(&mut scene, start.lerp(end, i as f32 / frames as f32));
        let mut f = step(&mut scene, &mut rt, &mut time);
        let total = f.events;
        f.events = total - prev_events;
        prev_events = total;
        flight.push(f);
    }
    let pick = |v: &[Frame], f: fn(&Frame) -> f32| v.iter().map(f).collect::<Vec<f32>>();
    eprintln!(
        "StreamingDemo flythrough, {w}x{h} High, 120 m/s, {} frames:",
        flight.len()
    );
    eprintln!("  still  {}", line("wall", &pick(&still, |f| f.wall_ms)));
    eprintln!("  flight {}", line("wall", &pick(&flight, |f| f.wall_ms)));
    eprintln!(
        "  flight {}",
        line(
            "streaming (main thread)",
            &pick(&flight, |f| f.streaming_ms)
        )
    );
    eprintln!(
        "  flight {}",
        line("game update", &pick(&flight, |f| f.update_ms))
    );
    eprintln!(
        "  flight {}",
        line("gather view", &pick(&flight, |f| f.gather_ms))
    );
    eprintln!(
        "  flight {}",
        line("render call", &pick(&flight, |f| f.render_ms))
    );
    let with: Vec<Frame> = flight.iter().copied().filter(|f| f.events > 0).collect();
    let without: Vec<Frame> = flight.iter().copied().filter(|f| f.events == 0).collect();
    eprintln!(
        "  frames where cells finished loading/unloading ({}): {}",
        with.len(),
        line("wall", &pick(&with, |f| f.wall_ms))
    );
    eprintln!(
        "  other frames ({}): {}",
        without.len(),
        line("wall", &pick(&without, |f| f.wall_ms))
    );
    let median = quantile(&pick(&flight, |f| f.wall_ms), 0.5);
    let mut slow: Vec<(usize, Frame)> = flight
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, f)| f.wall_ms > 16.0 && f.wall_ms > median * 1.5)
        .collect();
    slow.sort_by(|a, b| b.1.wall_ms.total_cmp(&a.1.wall_ms));
    for (i, f) in slow.iter().take(12) {
        eprintln!(
            "  slow frame {i:4}: wall {:.2} = update {:.2} (streaming {:.2}) + gather {:.2} + render {:.2} ms, {} cell events, {} loaded, {} pending",
            f.wall_ms,
            f.update_ms,
            f.streaming_ms,
            f.gather_ms,
            f.render_ms,
            f.events,
            f.loaded,
            f.pending
        );
    }
    let stats = rt.streaming().stats();
    eprintln!(
        "  cells: {} loads, {} unloads, max streaming frame {:.2} ms, {} frames over the {:.1} ms budget",
        stats.loads, stats.unloads, stats.max_ms, stats.over_budget, stats.budget_ms
    );
    crate::render::set_grass_streaming(false);
    let worst = pick(&flight, |f| f.streaming_ms)
        .into_iter()
        .fold(0.0f32, f32::max);
    assert!(worst < 16.0, "streaming main-thread frame of {worst:.2} ms");
}
