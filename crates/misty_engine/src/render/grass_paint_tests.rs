//! Painting grass in the editor: a stroke must show under the brush within a frame or two and
//! never hide the blades already there. A timing harness (ignored, prints) paints a stroke of
//! a dense grass species across a terrain the way the editor's Paint Details brush does and
//! measures how long the painted blades take to show:
//! `cargo test --release -p misty_engine --lib grass_paint_latency -- --ignored --nocapture`

use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Instant;

use glam::{Quat, Vec2, Vec3};

use super::{Asked, Chunk, Done, Grass, Job, Pool, Queue, Upload};
use crate::render::culling::Aabb;
use crate::terrain::store::Region;

use crate::camera::{Camera, Projection};
use crate::quality::QualityLevel;
use crate::render::environment::Environment;
use crate::render::gpu_tests::headless;
use crate::render::post::PostSettings;
use crate::render::{Frame, LightData, Renderer};
use crate::scene::{LightType, Shadows};
use crate::terrain::brush::{Brush, PAINT_RATE};
use crate::terrain::detail_layers::{self, DetailLayer, DetailTarget};
use crate::terrain::{TerrainData, TerrainDraw, store};

/// The reported layer: 1000 blades/m², 0.2–0.6 m tall, 4 cm wide.
fn dense_species() -> DetailLayer {
    DetailLayer {
        density: 1000.0,
        height: [0.2, 0.6],
        width: 0.04,
        bend: 0.87,
        dryness: 0.47,
        tint: 0.0,
        seed_heads: 0.28,
        ..DetailLayer::grass("Grass 1")
    }
}

/// A default-sized terrain (200 m, 256 splat texels) with the dense species painted over a
/// square `side` metres wide around (100, 100) (store key from `name`).
pub(super) fn painted_terrain(name: &str, side: f32) -> String {
    let mut d = TerrainData::default();
    d.details.enabled = true;
    d.detail_layers = vec![dense_species()];
    let n = d.alphamap_resolution;
    let texel = d.size.x / n as f32;
    let mut map = vec![0u8; n * n];
    for tz in 0..n {
        for tx in 0..n {
            let p = (Vec2::new(tx as f32, tz as f32) + 0.5) * texel;
            if (p - Vec2::splat(100.0)).abs().max_element() < side * 0.5 {
                map[tz * n + tx] = 255;
            }
        }
    }
    d.detail_layers[0].map = map;
    let key = std::env::temp_dir()
        .join(format!(
            "misty_grass_paint_{name}_{}.terrain",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    store::insert(&key, d);
    key
}

fn draw_of(key: &str) -> TerrainDraw {
    let (data, version) = store::get(key).expect("inserted");
    TerrainDraw {
        key: key.to_owned(),
        version,
        data,
        position: Vec3::ZERO,
        cast_shadows: true,
    }
}

/// One frame of the Paint Details brush at `at` (as `terrain_tools::apply` does).
pub(super) fn stroke(key: &str, at: Vec2, dt: f32, erase: bool) {
    let b = Brush {
        center: at,
        size: 25.0,
        strength: 0.5,
        falloff: 0.7,
    };
    let region = store::get(key).map(|(d, _)| b.region(&d)).expect("loaded");
    let k = (PAINT_RATE * b.strength * dt).min(1.0);
    store::modify_region(key, region, |d| {
        detail_layers::paint(d, &b, k, DetailTarget::Layer(0), 1.0, erase)
    });
}

fn sun() -> LightData {
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.3, -0.7, 0.6).normalize(),
        color: [1.0; 3],
        intensity: 1.6,
        range: 10.0,
        spot_angle: 30.0,
        shadows: Shadows::Soft,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    }
}

/// The scene camera above the painted square, looking down at the stroke.
fn camera() -> Camera {
    Camera {
        position: Vec3::new(100.0, 14.0, 80.0),
        rotation: Quat::from_rotation_x(50f32.to_radians()),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.05,
        far: 500.0,
    }
}

/// Chunks (16 m) a 25 m brush at `at` touches.
fn brushed_chunks(at: Vec2) -> Vec<(u32, u32)> {
    let lo = ((at - 12.5) / 16.0).floor().max(Vec2::ZERO);
    let hi = ((at + 12.5) / 16.0).floor();
    let mut out = Vec::new();
    for z in lo.y as u32..=hi.y as u32 {
        for x in lo.x as u32..=hi.x as u32 {
            out.push((x, z));
        }
    }
    out
}

#[test]
#[ignore = "timing, prints only"]
fn grass_paint_latency() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let key = painted_terrain("latency", 64.0);
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &QualityLevel::default());
    let target = r.create_target(&device, 960, 540);
    crate::render::set_grass_streaming(true);
    let cam = camera();
    let environment = Environment {
        grass_time: Some(3.0),
        ..Default::default()
    };
    let lights = [sun()];
    let render = |r: &mut Renderer| {
        let terrains = [draw_of(&key)];
        let frame = Frame {
            camera: &cam,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &terrains,
            water: &[],
            shaded: true,
            lights: &lights,
        };
        let t = Instant::now();
        r.render(&device, &queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        t.elapsed().as_secs_f32() * 1000.0
    };
    // Settle: every chunk in view scattered and uploaded.
    let t0 = Instant::now();
    let mut frames = 0;
    loop {
        render(&mut r);
        frames += 1;
        if r.stats().grass_pending == 0 && frames > 3 {
            break;
        }
        assert!(t0.elapsed().as_secs() < 600, "never settled");
    }
    eprintln!(
        "settled: {frames} frames, {:.0} ms, resident {:?}",
        t0.elapsed().as_secs_f32() * 1000.0,
        r.terrains.grass_resident()
    );
    // The stroke: 60 frames at 60 fps across the painted square's edge, erasing first (so
    // there is something to paint back) then painting.
    let dt = 1.0 / 60.0;
    let path = |i: usize| Vec2::new(70.0 + i as f32 * 1.0, 100.0);
    let mut worst_frame = 0.0f32;
    let mut hidden_frames = 0;
    let mut lag_frames: Vec<usize> = Vec::new();
    // Per stroke frame: its time, the chunk under the brush and the edits made so far; the
    // frame its blades first include those edits gives the brush's lag.
    let mut edits: Vec<(Instant, (u32, u32), u64)> = Vec::new();
    let mut seen: Vec<(Instant, (u32, u32), u64)> = Vec::new();
    let stroke_start = Instant::now();
    for pass in [true, false] {
        let mut pending_since: Option<usize> = None;
        for i in 0..60 {
            let at = path(i);
            let before: Vec<_> = brushed_chunks(at)
                .into_iter()
                .map(|c| (c, r.terrains.grass_chunk(&key, c)))
                .collect();
            stroke(&key, at, dt, pass);
            let painted = Instant::now();
            let ms = render(&mut r);
            worst_frame = worst_frame.max(ms);
            let under = ((at.x / 16.0) as u32, (at.y / 16.0) as u32);
            if let Some((_, total)) = r.terrains.grass_stamps(&key, under) {
                edits.push((painted, under, total));
            }
            for c in brushed_chunks(at) {
                if let Some((stamp, _)) = r.terrains.grass_stamps(&key, c) {
                    seen.push((Instant::now(), c, stamp));
                }
            }
            // A chunk that had blades and has none now: the slice disappeared.
            let gone = before.iter().any(|(c, had)| {
                had.is_some_and(|h| h.0 > 0) && r.terrains.grass_chunk(&key, *c).is_none()
            });
            hidden_frames += usize::from(gone);
            let fresh = r.terrains.grass_fresh(&key, &brushed_chunks(at));
            match (fresh, pending_since) {
                (true, Some(s)) => {
                    lag_frames.push(i - s);
                    pending_since = None;
                }
                (false, None) => pending_since = Some(i),
                _ => {}
            }
        }
        if let Some(s) = pending_since {
            lag_frames.push(60 - s);
        }
    }
    let stroke_ms = stroke_start.elapsed().as_secs_f32() * 1000.0;
    // After the stroke: frames until the painted chunks hold the latest scatter.
    let end = Instant::now();
    let mut after = 0;
    loop {
        render(&mut r);
        after += 1;
        if r.stats().grass_pending == 0 {
            break;
        }
        assert!(
            end.elapsed().as_secs() < 600,
            "never settled after the stroke"
        );
    }
    let settle_ms = end.elapsed().as_secs_f32() * 1000.0;
    let lag_max = lag_frames.iter().copied().max().unwrap_or(0);
    // Brush lag: from painting a dab to the chunk under it drawing blades that include it.
    let mut lags: Vec<f32> = (edits.iter())
        .filter_map(|(t, c, e)| {
            let shown = seen
                .iter()
                .find(|(st, sc, ss)| st >= t && sc == c && ss >= e)?;
            Some(shown.0.duration_since(*t).as_secs_f32() * 1000.0)
        })
        .collect();
    lags.sort_by(f32::total_cmp);
    if !lags.is_empty() {
        eprintln!(
            "brush lag (dab to its blades drawn, chunk under the brush): median {:.0} ms,              90% {:.0} ms, max {:.0} ms over {} of {} dabs",
            lags[lags.len() / 2],
            lags[lags.len() * 9 / 10],
            lags[lags.len() - 1],
            lags.len(),
            edits.len()
        );
    }
    eprintln!(
        "stroke: 120 frames in {stroke_ms:.0} ms (worst frame {worst_frame:.1} ms), \
         {hidden_frames} frames with a painted chunk hidden, under-brush lag max {lag_max} \
         frames ({} stale spans); after release: {after} frames, {settle_ms:.0} ms to settle",
        lag_frames.len()
    );
    crate::render::set_grass_streaming(false);
    store::unload(&key);
}

/// Scatter time of one fully painted 16 m chunk of the dense species at full density: one
/// thread (the camera's chunks) and the edited chunks' row threads. A painted chunk should
/// update well under 100 ms on a machine not busy with other work.
#[test]
#[ignore = "timing, prints only"]
fn grass_scatter_cost() {
    let key = painted_terrain("scatter", 64.0);
    let (d, _) = store::get(&key).expect("inserted");
    let lo = Vec2::new(96.0, 96.0);
    let median = |f: &dyn Fn() -> usize| {
        let mut times = Vec::new();
        let mut n = 0;
        for _ in 0..7 {
            let t = Instant::now();
            n = f();
            times.push(t.elapsed().as_secs_f32() * 1000.0);
        }
        times.sort_by(f32::total_cmp);
        (n, times[3])
    };
    let serial = || crate::terrain::details::scatter_level(&d, lo, lo + 16.0, 0).len();
    let threads = super::row_threads().expect("row threads");
    let parallel = || {
        threads
            .install(|| crate::terrain::details::scatter_level_parallel(&d, lo, lo + 16.0, 0))
            .len()
    };
    let (n, one) = median(&serial);
    let (_, rows) = median(&parallel);
    eprintln!(
        "scatter of one chunk: {n} blades, one thread {one:.1} ms, {} row threads {rows:.1} ms",
        threads.current_num_threads()
    );
    for t in [2, 4, 8] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build()
            .expect("pool");
        let (_, ms) = median(&|| {
            pool.install(|| crate::terrain::details::scatter_level_parallel(&d, lo, lo + 16.0, 0))
                .len()
        });
        eprintln!("  {t} threads: {ms:.1} ms");
    }
    store::unload(&key);
}

/// An edited chunk's rows scattered in parallel give the very blades, in the very order, of
/// the one-thread scatter (the golden shots and the far chunks' prefixes rely on it).
#[test]
fn edited_chunks_scatter_the_same_blades() {
    let key = painted_terrain("same", 64.0);
    let (d, _) = store::get(&key).expect("inserted");
    // A chunk on the painted square's edge: some texels painted, some not.
    let lo = Vec2::new(64.0, 80.0);
    let threads = super::row_threads().expect("row threads");
    let parallel =
        threads.install(|| crate::terrain::details::scatter_level_parallel(&d, lo, lo + 16.0, 0));
    let serial = crate::terrain::details::scatter_level(&d, lo, lo + 16.0, 0);
    assert!(serial.len() > 50_000, "{}", serial.len());
    assert!(serial == parallel, "the same blades in the same order");
    // And at a coarser level (a far chunk).
    let parallel =
        threads.install(|| crate::terrain::details::scatter_level_parallel(&d, lo, lo + 16.0, 3));
    assert!(crate::terrain::details::scatter_level(&d, lo, lo + 16.0, 3) == parallel);
    store::unload(&key);
}

/// A chunk holding `count` blades scattered at density level 0 with edits `stamp` (no GPU
/// storage: the logic tests only look at what is held).
fn held(count: u32, stamp: u64) -> Chunk {
    Chunk {
        store: None,
        count,
        level: 0,
        stamp,
        bounds: Aabb::new(Vec3::ZERO, Vec3::ONE),
        arena: None,
        group: None,
    }
}

fn result(g: &Grass, key: (u32, u32), stamp: u64, blades: usize) -> Done {
    Done {
        key,
        level: 0,
        generation: g.generation,
        stamp,
        blades: vec![bytemuck::Zeroable::zeroed(); blades],
        scatter_ms: 0.0,
    }
}

/// A 64 m terrain with grass on (its maps are not read by the logic tests).
fn small() -> TerrainData {
    let mut d = TerrainData::new(65, Vec3::new(64.0, 10.0, 64.0), 64);
    d.details.enabled = true;
    d
}

#[test]
fn edits_keep_the_old_blades_drawing() {
    let d = small();
    let mut g = Grass::default();
    for z in 0..4 {
        for x in 0..4 {
            g.chunks.insert((x, z), held(1000, 0));
        }
    }
    // A brush square over chunk (1, 1) only.
    let r = Region {
        min: [20.0 / 64.0; 2],
        max: [28.0 / 64.0; 2],
    };
    g.invalidate(&d, Some(r));
    assert_eq!(g.chunks.len(), 16, "no chunk dropped by the edit");
    assert_eq!(g.chunk((1, 1)), Some((1000, 0)), "old blades still drawn");
    assert!(g.outdated((1, 1)).is_some());
    assert!(g.outdated((3, 3)).is_none(), "far from the brush");
    // Everything (an undo, Fill / Clear): still nothing dropped.
    g.invalidate(&d, None);
    assert_eq!(g.chunks.len(), 16);
    assert!(g.outdated((3, 3)).is_some());
    // The replacement swaps in once uploaded, and the chunk is up to date again.
    let stamp = g.edits;
    g.in_flight.insert((1, 1), Asked { level: 0, stamp });
    let done = result(&g, (1, 1), stamp, 0);
    g.accept(done, &[Vec3::new(24.0, 1.0, 24.0)], 80.0);
    assert_eq!(g.ready.len(), 1);
    let done = g.ready.pop_front().unwrap();
    g.finish_upload(Upload {
        done,
        store: None,
        arena: None,
        written: 0,
    });
    assert_eq!(
        g.chunk((1, 1)),
        Some((0, 0)),
        "erased: the new, empty scatter"
    );
    assert!(g.outdated((1, 1)).is_none());
}

#[test]
fn repeated_edits_coalesce() {
    let d = small();
    let mut g = Grass::default();
    let camera = Vec3::new(24.0, 1.0, 24.0);
    for z in 0..4 {
        for x in 0..4 {
            g.chunks.insert((x, z), held(1000, 0));
        }
    }
    let r = Region {
        min: [20.0 / 64.0; 2],
        max: [28.0 / 64.0; 2],
    };
    g.invalidate(&d, Some(r));
    // The edited chunk goes first (before nearer camera work would).
    let todo = g.stale_chunks(&d, camera);
    assert_eq!(todo.first(), Some(&((1, 1), true)), "{todo:?}");
    // Its job goes out; the stroke keeps editing it meanwhile.
    let first = Asked {
        level: 0,
        stamp: g.edits,
    };
    g.in_flight.insert((1, 1), first);
    for _ in 0..10 {
        g.invalidate(&d, Some(r));
        assert!(
            !g.stale_chunks(&d, camera).iter().any(|(k, _)| *k == (1, 1)),
            "no second job while one runs"
        );
    }
    // Its result (behind the latest edits) still shows: progress while painting.
    let done = result(&g, (1, 1), first.stamp, 10);
    g.accept(done, &[camera], 80.0);
    assert_eq!(g.ready.len(), 1);
    assert!(g.in_flight.is_empty());
    // Now one job with the latest edits goes out.
    let todo = g.stale_chunks(&d, camera);
    assert_eq!(todo, vec![((1, 1), true)]);
    let latest = Asked {
        level: 0,
        stamp: g.edits,
    };
    g.in_flight.insert((1, 1), latest);
    // Its result replaces the older one still waiting for upload.
    let done = result(&g, (1, 1), latest.stamp, 20);
    g.accept(done, &[camera], 80.0);
    assert_eq!(g.ready.len(), 1);
    assert_eq!(g.ready[0].stamp, latest.stamp);
    // A late result of a job no longer asked for (its chunk went out of reach) is dropped.
    let stale = result(&g, (2, 2), 0, 5);
    g.accept(stale, &[camera], 80.0);
    assert_eq!(g.ready.len(), 1);
}

#[test]
fn edited_chunks_jump_the_workers_queue() {
    let pool = Pool {
        queue: Mutex::new(Queue::default()),
        wake: Condvar::new(),
    };
    let data = Arc::new(small());
    let (tx, _rx) = mpsc::channel();
    let job = |x: u32, edit: bool| Job {
        data: Arc::clone(&data),
        key: (x, 0),
        level: 0,
        generation: 0,
        stamp: 0,
        edit,
        reply: tx.clone(),
    };
    pool.send(job(1, false));
    pool.send(job(2, false));
    pool.send(job(3, true));
    let order: Vec<u32> = (0..3).map(|_| pool.take().key.0).collect();
    assert_eq!(order, vec![3, 1, 2]);
}

/// Through the renderer: painting a chunk keeps its blades drawn every frame, the frames
/// report pending grass work (the editor's views keep repainting while they do) and the
/// painted blades replace them within a few frames.
#[test]
fn painted_grass_shows_within_frames() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let key = painted_terrain("frames", 24.0);
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &QualityLevel::default());
    let target = r.create_target(&device, 320, 200);
    let cam = camera();
    let environment = Environment::default();
    let lights = [sun()];
    let render = |r: &mut Renderer| {
        let terrains = [draw_of(&key)];
        let frame = Frame {
            camera: &cam,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &terrains,
            water: &[],
            shaded: true,
            lights: &lights,
        };
        r.render(&device, &queue, &target, &frame);
    };
    // Settled (no streaming: every chunk in the first frame).
    render(&mut r);
    let chunk = (6, 6);
    let (before, _) = r
        .terrains
        .grass_chunk(&key, chunk)
        .expect("painted chunk held");
    assert!(before > 100_000, "{before}");
    assert_eq!(r.stats().grass_pending, 0);
    crate::render::set_grass_streaming(true);
    // Erase a dab in the chunk's middle.
    stroke(&key, Vec2::new(104.0, 104.0), 0.5, true);
    render(&mut r);
    let mut frames = 1;
    loop {
        let now = r.terrains.grass_chunk(&key, chunk);
        assert!(
            now.is_some(),
            "the painted chunk vanished on frame {frames}"
        );
        if r.terrains.grass_fresh(&key, &[chunk]) {
            break;
        }
        assert!(r.stats().grass_pending > 0, "work left but none reported");
        assert!(frames < 600, "the painted blades never arrived");
        std::thread::sleep(std::time::Duration::from_millis(2));
        render(&mut r);
        frames += 1;
    }
    let (after, _) = r.terrains.grass_chunk(&key, chunk).expect("held");
    assert!(after < before, "erased: {after} of {before}");
    // The brush's other chunks follow; then nothing is left to do and the views may idle.
    let brushed = brushed_chunks(Vec2::new(104.0, 104.0));
    while r.stats().grass_pending > 0 {
        assert!(frames < 1200, "grass work never finished");
        for c in &brushed {
            assert!(r.terrains.grass_chunk(&key, *c).is_some(), "{c:?} vanished");
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        render(&mut r);
        frames += 1;
    }
    assert!(r.terrains.grass_fresh(&key, &brushed));
    render(&mut r);
    assert_eq!(r.stats().grass_pending, 0);
    crate::render::set_grass_streaming(false);
    store::unload(&key);
}
