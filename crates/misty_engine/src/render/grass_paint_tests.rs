//! Painting grass in the editor: a stroke must show under the brush within a frame or two and
//! never hide the blades already there. A timing harness (ignored, prints) paints a stroke of
//! a dense grass species across a terrain the way the editor's Paint Details brush does and
//! measures how long the painted blades take to show:
//! `cargo test --release -p misty_engine --lib grass_paint_latency -- --ignored --nocapture`

use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Instant;

use glam::{Quat, Vec2, Vec3};

use super::{
    Asked, Chunk, Done, Grass, Job, Pool, Queue, TILE_BIT, TileQueue, Upload, is_tile, tile_key,
    tiles_of,
};
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
    // Per dab: when it was painted, where, the edits made so far and how long until the
    // blades drawn at its centre include it.
    let mut dabs: Vec<(Instant, Vec2, u64, Option<f32>)> = Vec::new();
    let shown = |r: &Renderer, dabs: &mut Vec<(Instant, Vec2, u64, Option<f32>)>| {
        for (t, at, e, lag) in dabs.iter_mut().filter(|d| d.3.is_none()) {
            if r.terrains
                .grass_stamps_at(&key, *at)
                .is_some_and(|(s, _)| s >= *e)
            {
                *lag = Some(t.elapsed().as_secs_f32() * 1000.0);
            }
        }
    };
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
            if let Some((_, total)) = r.terrains.grass_stamps_at(&key, at) {
                dabs.push((painted, at, total, None));
            }
            shown(&r, &mut dabs);
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
        shown(&r, &mut dabs);
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
    let mut lags: Vec<f32> = dabs.iter().filter_map(|d| d.3).collect();
    lags.sort_by(f32::total_cmp);
    if !lags.is_empty() {
        eprintln!(
            "brush lag (dab to its blades drawn at the brush centre): median {:.0} ms, \
             90% {:.0} ms, max {:.0} ms over {} of {} dabs",
            lags[lags.len() / 2],
            lags[lags.len() * 9 / 10],
            lags[lags.len() - 1],
            lags.len(),
            dabs.len()
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
        hidden: false,
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

/// Upload `done` at once (no GPU: the logic tests hold no blades).
fn land(g: &mut Grass, done: Done) {
    g.finish_upload(Upload {
        done,
        store: None,
        arena: None,
        written: 0,
    });
}

/// A brush square over the middle of chunk (1, 1) of the small terrain: its tiles 5, 6, 9
/// and 10 (the 8 m square in its centre).
fn dab() -> Region {
    Region {
        min: [21.0 / 64.0; 2],
        max: [27.0 / 64.0; 2],
    }
}

#[test]
fn edits_keep_the_old_blades_drawing() {
    let d = small();
    let mut g = Grass::default();
    let camera = Vec3::new(24.0, 1.0, 24.0);
    for z in 0..4 {
        for x in 0..4 {
            g.chunks.insert((x, z), held(1000, 0));
        }
    }
    g.invalidate(&d, Some(dab()));
    assert_eq!(g.chunks.len(), 16, "no chunk dropped by the edit");
    assert_eq!(g.chunk((1, 1)), Some((1000, 0)), "old blades still drawn");
    assert!(
        g.tiled.contains(&(1, 1)),
        "the painted chunk switches to tiles"
    );
    assert!(!g.fresh((1, 1)));
    assert!(g.fresh((3, 3)), "far from the brush");
    let touched: Vec<u32> = (0..16)
        .filter(|t| g.dirty.contains_key(&tile_key((1, 1), *t)))
        .collect();
    assert_eq!(touched, vec![5, 6, 9, 10], "only the tiles under the brush");
    // All 16 tiles are scattered once (the touched ones first); until the last is up, the
    // whole run draws and the tiles stay hidden.
    let todo = g.stale_chunks(&d, camera);
    let tiles: Vec<(u32, u32)> = todo.iter().filter(|(_, e)| *e).map(|(k, _)| *k).collect();
    assert_eq!(tiles.len(), 16, "{todo:?}");
    assert!(
        [5, 6, 9, 10].contains(&(0..16).find(|t| tile_key((1, 1), *t) == tiles[0]).unwrap()),
        "a tile under the brush first"
    );
    let stamp = g.edits;
    for (i, t) in tiles.iter().enumerate() {
        g.in_flight.insert(*t, Asked { level: 0, stamp });
        let done = result(&g, *t, stamp, 10);
        g.accept(done, &[camera], 80.0);
        let done = g.ready.pop_front().expect("queued");
        land(&mut g, done);
        if i < 15 {
            assert_eq!(
                g.chunk((1, 1)),
                Some((1000, 0)),
                "whole run until all tiles are up"
            );
        }
    }
    assert_eq!(g.chunk((1, 1)), Some((160, 0)), "16 tiles of 10 blades now");
    assert!(!g.chunks.contains_key(&(1, 1)), "the whole run went back");
    assert!(g.fresh((1, 1)));
    // The next dab re-scatters only its four tiles; the others keep drawing as they are.
    g.invalidate(&d, Some(dab()));
    let todo = g.stale_chunks(&d, camera);
    assert_eq!(todo.len(), 4, "{todo:?}");
    assert_eq!(g.chunk((1, 1)), Some((160, 0)), "old tiles still drawn");
    // Everything (an undo, Fill / Clear): still nothing dropped.
    g.invalidate(&d, None);
    assert_eq!(g.chunk((1, 1)), Some((160, 0)));
    assert!(!g.fresh((3, 3)));
}

#[test]
fn repeated_edits_coalesce() {
    let d = small();
    let mut g = Grass::default();
    let camera = Vec3::new(24.0, 1.0, 24.0);
    g.tiled.insert((1, 1));
    for t in tiles_of((1, 1)) {
        g.chunks.insert(t, held(100, 0));
    }
    for z in 0..4 {
        for x in 0..4 {
            if (x, z) != (1, 1) {
                g.chunks.insert((x, z), held(1000, 0));
            }
        }
    }
    let t5 = tile_key((1, 1), 5);
    g.invalidate(&d, Some(dab()));
    // The tile under the brush goes first (before nearer camera work would).
    let todo = g.stale_chunks(&d, camera);
    assert_eq!(todo.len(), 4);
    assert!(todo.iter().all(|(k, e)| *e && is_tile(*k)));
    // Its job goes out; the stroke keeps editing it meanwhile.
    let first = Asked {
        level: 0,
        stamp: g.edits,
    };
    g.in_flight.insert(t5, first);
    assert!(
        !g.stale_chunks(&d, camera).iter().any(|(k, _)| *k == t5),
        "no second job while one with the latest edits runs"
    );
    // Edited again: the tile goes out again (its waiting job is replaced in the queue, see
    // `tile_jobs_waiting_are_replaced`).
    for _ in 0..10 {
        g.invalidate(&d, Some(dab()));
    }
    assert!(g.stale_chunks(&d, camera).contains(&(t5, true)));
    let latest = Asked {
        level: 0,
        stamp: g.edits,
    };
    g.in_flight.insert(t5, latest);
    // The older job's result (it was already running) still shows: progress while painting.
    let done = result(&g, t5, first.stamp, 10);
    g.accept(done, &[camera], 80.0);
    assert_eq!(g.ready.len(), 1);
    assert_eq!(
        g.in_flight.get(&t5),
        Some(&latest),
        "the newer job stays asked for"
    );
    // Its result replaces the older one still waiting for upload.
    let done = result(&g, t5, latest.stamp, 20);
    g.accept(done, &[camera], 80.0);
    assert_eq!(g.ready.len(), 1);
    assert_eq!(g.ready[0].stamp, latest.stamp);
    // A late result of a job no longer asked for (its chunk went out of reach) is dropped.
    let stale = result(&g, (2, 2), 0, 5);
    g.accept(stale, &[camera], 80.0);
    assert_eq!(g.ready.len(), 1);
    // A whole scatter of the tiled chunk older than its tiles' edits never replaces them.
    g.in_flight.insert((1, 1), Asked { level: 0, stamp: 0 });
    let old = result(&g, (1, 1), 0, 1000);
    g.accept(old, &[camera], 80.0);
    assert!(g.ready.iter().all(|r| r.key != (1, 1)));
}

/// The tile queue: the newest round first, nearest the brush first; a tile's waiting job is
/// replaced by a newer one (a stroke never queues behind its own stale work).
#[test]
fn tile_jobs_waiting_are_replaced() {
    let q = TileQueue::default();
    let data = Arc::new(small());
    let (tx, _rx) = mpsc::channel();
    let job = |x: u32, stamp: u64, order: i64| Job {
        data: Arc::clone(&data),
        key: (TILE_BIT | x, 0),
        level: 0,
        generation: 0,
        stamp,
        edit: true,
        owner: 7,
        order,
        first: x == 9,
        reply: tx.clone(),
    };
    q.send(job(1, 1, 0));
    q.send(job(2, 1, 5));
    q.send(job(3, 2, 9));
    // Tile 1 edited again: its waiting job is replaced, not queued twice.
    q.send(job(1, 3, 4));
    assert_eq!(q.jobs.lock().unwrap().len(), 3);
    // A tile of a chunk switching to tiles goes before them all.
    q.send(job(9, 1, 50));
    assert_eq!(q.take().key.0 & !TILE_BIT, 9);
    let order: Vec<(u32, u64)> = (0..3)
        .map(|_| {
            let j = q.take();
            (j.key.0 & !TILE_BIT, j.stamp)
        })
        .collect();
    assert_eq!(order, vec![(1, 3), (3, 2), (2, 1)]);
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
        owner: 1,
        order: 0,
        first: false,
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

/// A settings change (a slider dragged in the Inspector) keeps every chunk drawing and
/// re-scatters by distance to the camera; a brush meanwhile goes first.
#[test]
fn settings_changes_keep_blades_and_wait_their_turn() {
    let d = small();
    let mut g = Grass::default();
    let camera = Vec3::new(8.0, 1.0, 8.0);
    for z in 0..4 {
        for x in 0..4 {
            g.chunks.insert((x, z), held(1000, 0));
        }
    }
    g.outdate_all(false);
    assert_eq!(g.chunks.len(), 16, "nothing dropped");
    let todo = g.stale_chunks(&d, camera);
    assert_eq!(todo.len(), 16);
    assert!(todo.iter().all(|(_, edit)| !edit), "camera priority");
    assert_eq!(todo[0].0, (0, 0), "nearest the camera first");
    // Dragging on: each chunk keeps one job at a time (coalesced).
    g.in_flight.insert(
        (0, 0),
        Asked {
            level: 0,
            stamp: g.edits,
        },
    );
    g.outdate_all(false);
    assert!(!g.stale_chunks(&d, camera).iter().any(|(k, _)| *k == (0, 0)));
    // A brush stroke meanwhile jumps ahead.
    g.invalidate(&d, Some(dab()));
    let todo = g.stale_chunks(&d, camera);
    assert!(todo[0].1 && is_tile(todo[0].0), "{todo:?}");
}

/// Healthy↔Dry and Brightness of a species are looked up at draw time: they leave the
/// scatter alone (no chunk is scattered again); its height does not.
#[test]
fn species_colour_leaves_the_scatter_alone() {
    let mut d = small();
    d.detail_layers = vec![dense_species()];
    let key = super::scatter_key(&d);
    d.detail_layers[0].dryness = 0.9;
    d.detail_layers[0].tint = -0.6;
    assert!(super::scatter_key(&d) == key, "colour only: same scatter");
    d.detail_layers[0].height = [0.5, 0.9];
    assert!(super::scatter_key(&d) != key, "height: scattered again");
}

/// Through the renderer: a species colour edit shows on the next frame without scattering
/// anything.
#[test]
fn species_colour_edits_show_at_once() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let key = painted_terrain("colour", 24.0);
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(
        &device,
        &QualityLevel {
            temporal_aa: false,
            ..QualityLevel::default()
        },
    );
    let target = r.create_target(&device, 320, 200);
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
        r.render(&device, &queue, &target, &frame);
        crate::render::gpu_tests::read_back(&device, &queue, &target)
    };
    render(&mut r);
    let a = render(&mut r);
    let a2 = render(&mut r);
    store::modify_settings(&key, |d| {
        d.detail_layers[0].dryness = 1.0;
        d.detail_layers[0].tint = -1.0;
    });
    let b = render(&mut r);
    assert_eq!(
        r.terrains.grass_stats().scheduled,
        0,
        "nothing scattered again"
    );
    let diff = |x: &[u8], y: &[u8]| {
        x.iter()
            .zip(y)
            .map(|(p, q)| p.abs_diff(*q) as f64)
            .sum::<f64>()
            / x.len() as f64
    };
    let noise = diff(&a, &a2);
    let changed = diff(&a, &b);
    assert!(
        changed > noise * 10.0 + 0.05,
        "the grass took the new colour ({changed} vs {noise})"
    );
    store::unload(&key);
}
