//! Grass of one terrain: blades are scattered per 16 m chunk around the camera (only within
//! the draw distance), kept in small instance buffers and dropped again when far away or when
//! the terrain under them changes. Scattering runs on worker threads ([`pool`]): the render
//! thread only hands out jobs (nearest chunk first) and uploads finished chunks within a
//! small time budget per frame, so moving the camera never stalls a frame. A chunk that
//! needs more blades keeps drawing its coarser blades until the finer scatter arrives.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use wgpu::util::DeviceExt;

use super::culling::{Aabb, Frustum};
use super::grass_cull::{self, Arena, CullFrame, CullPipelines, JobDraw, KINDS, PlannedKind};
use super::grass_pool::{BladePool, Slot};
use crate::sprites::compress::generated::{self, Ctx, Layers, Made, Role, Spec as TexSpec};
use crate::sprites::compress::{Compressed, Mode, encode_level};
use crate::terrain::TerrainData;
use crate::terrain::details::{self, Blade, DetailSettings};
use crate::terrain::flowers::FlowerSettings;
use crate::terrain::store::Region;

/// Chunk side in metres.
const CHUNK: f32 = 16.0;
/// Render-thread time per frame for uploading finished chunks (the first piece always goes).
pub(super) const UPLOAD_BUDGET: Duration = Duration::from_millis(2);
/// Blades written per piece of an upload (~0.6 MB of blades and 0.13 MB of cull records):
/// a big chunk goes up over several frames, so no single frame spends long on it.
const UPLOAD_PIECE: usize = 8 * 1024;
/// Scatter jobs handed to the workers at once per terrain (more would just queue up and go
/// stale while the camera moves on).
const MAX_IN_FLIGHT: usize = 6;
/// Jobs for edited chunks handed out on top of that (a brush covers up to nine chunks): they
/// never wait for room behind the camera's jobs, and the workers take them first.
const MAX_EDIT_IN_FLIGHT: usize = 9;
/// Render-thread time per frame for uploading edited chunks (painted grass shows within a
/// frame or two; the camera's chunks keep [`UPLOAD_BUDGET`]).
const EDIT_UPLOAD_BUDGET: Duration = Duration::from_millis(6);
/// How long chunks near a camera stay after its last frame: the editor's Scene and Game
/// views share the renderer, and each view's frame must not drop the other's chunks.
const CAMERA_MEMORY: Duration = Duration::from_millis(1500);
/// Vertices of one blade (grass_blade.wgsl: four segments and a tip).
pub(super) const BLADE_VERTICES: u32 = 10;
/// Share of a shadow caster range where the grass casters start to fade out.
pub(super) const CASTER_FADE: f32 = 0.6;
/// Vertices of a coarse far blade (three segments), drawn from vertex 100 on.
const COARSE_VERTICES: u32 = 8;
/// Chunks further than this many LOD distances draw coarse blades.
const COARSE_FROM: f32 = 1.5;
/// Fewest blades a far chunk keeps (share of full density).
const MIN_KEEP: f32 = 0.012;
/// Far blades grow at most this much wider to cover the ground of the ones left out.
const MAX_WIDEN: f32 = 3.0;

/// Stream chunks over frames (the default). Off, [`Grass::update`] waits for every chunk the
/// camera needs before returning, so a render is complete on its first frame (tests,
/// screenshots).
static STREAMING: AtomicBool = AtomicBool::new(true);

/// Cull grass on the GPU when the device can (the default); off, every blade of a chunk's
/// prefix goes to the vertex stage (A/B timings in tests).
static GPU_CULL: AtomicBool = AtomicBool::new(true);

#[cfg(test)]
pub(super) fn set_grass_gpu_cull(on: bool) {
    GPU_CULL.store(on, Ordering::Relaxed);
}

/// Build grass chunks over several frames on worker threads (true, the default) or wait for
/// them in the frame that needs them (deterministic renders: tests, screenshots).
pub fn set_grass_streaming(on: bool) {
    STREAMING.store(on, Ordering::Relaxed);
}

pub fn grass_streaming() -> bool {
    STREAMING.load(Ordering::Relaxed)
}

/// Share of blades drawn at `dist` metres: all up to the LOD distance, then falling with the
/// square of the distance (blades grow wider to cover the same ground, see grass.wgsl).
pub(super) fn keep_at(dist: f32, lod_distance: f32) -> f32 {
    let r = lod_distance.max(1.0) / dist.max(1e-3);
    (r * r).clamp(MIN_KEEP, 1.0)
}

/// Coarsest density level that still has `keep` of the blades.
fn level_for(keep: f32) -> u32 {
    let mut l = 0;
    while l < details::MAX_LEVEL && details::level_keep(l + 1) >= keep {
        l += 1;
    }
    l
}

/// Where a chunk's blades are (storage: the vertex stage fetches them through a draw's list).
enum Store {
    /// A run in the terrain's blade pages (grass_pool.rs).
    Pool(Slot),
    /// A buffer of its own (no page could take them).
    Own(wgpu::Buffer),
}

struct Chunk {
    /// The blades (None: the chunk has none).
    store: Option<Store>,
    count: u32,
    /// Density level the blades were scattered at.
    level: u32,
    /// [`Grass::edits`] the blades were scattered with.
    stamp: u64,
    /// Terrain-local bounds of the blades.
    bounds: Aabb,
    /// Where the blades' cull records start in the arena (None = not culled on the GPU).
    arena: Option<u32>,
    /// Group 3 of the grass pipelines when the chunk binds its blades on its own (the CPU
    /// path, or a buffer of its own): these blades and the list of drawn ones (with the
    /// generations of the list buffer and the pages it was made for).
    group: Option<((u64, u64), wgpu::BindGroup)>,
}

impl Chunk {
    fn pooled(&self) -> Option<Slot> {
        match &self.store {
            Some(Store::Pool(slot)) => Some(*slot),
            _ => None,
        }
    }
}

/// A chunk's job on the workers: the level and the edits asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Asked {
    level: u32,
    stamp: u64,
}

/// A finished scatter going up to the GPU piece by piece ([`UPLOAD_PIECE`] blades a time);
/// the chunk is swapped in when the last piece is written.
struct Upload {
    done: Done,
    store: Option<Store>,
    /// Cull records' offset in the arena.
    arena: Option<u32>,
    /// Blades written so far.
    written: usize,
}

/// Chunks within `range` metres only, `share` of their blades, coarse blades only.
#[derive(Clone, Copy)]
pub(super) struct Limit {
    pub range: f32,
    pub share: f32,
    pub coarse: bool,
}

/// A chunk to scatter on a worker thread.
struct Job {
    data: Arc<TerrainData>,
    key: (u32, u32),
    level: u32,
    /// [`Grass::generation`] when handed out; results of older generations are dropped.
    generation: u64,
    /// [`Grass::edits`] when handed out: the edits `data` already has.
    stamp: u64,
    /// The chunk is being edited: it goes before the camera's jobs, its rows scattered in
    /// parallel.
    edit: bool,
    reply: mpsc::Sender<Done>,
}

/// A scattered chunk, waiting to be uploaded.
struct Done {
    key: (u32, u32),
    level: u32,
    generation: u64,
    stamp: u64,
    blades: Vec<Blade>,
    /// Worker time of the scatter.
    scatter_ms: f32,
}

/// Scatter jobs waiting for a worker: chunks being edited (under the brush) go before the
/// ones the camera asks for.
#[derive(Default)]
struct Queue {
    edits: VecDeque<Job>,
    camera: VecDeque<Job>,
}

/// The scatter workers, shared by every terrain: half the cores (1–4), fed through a queue.
/// A worker scatters one chunk at a time, its rows spread over every core (rayon).
struct Pool {
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl Pool {
    fn send(&self, job: Job) {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if job.edit {
            q.edits.push_back(job);
        } else {
            q.camera.push_back(job);
        }
        drop(q);
        self.wake.notify_one();
    }

    fn take(&self) -> Job {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(job) = q.edits.pop_front().or_else(|| q.camera.pop_front()) {
                return job;
            }
            q = self.wake.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// Threads that scatter the rows of edited chunks: every core but two (the render and UI
/// threads keep theirs while painting). The camera's chunks are scattered one per worker,
/// as before, so streaming never crowds the render thread.
fn row_threads() -> Option<&'static rayon::ThreadPool> {
    static THREADS: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    THREADS
        .get_or_init(|| {
            let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
            rayon::ThreadPoolBuilder::new()
                .num_threads(cores.saturating_sub(2).max(1))
                .thread_name(|i| format!("misty grass rows {i}"))
                .build()
                .ok()
        })
        .as_ref()
}

/// Scatter one job (on a worker thread).
fn scatter_job(job: Job) {
    let started = Instant::now();
    let lo = Vec2::new(job.key.0 as f32, job.key.1 as f32) * CHUNK;
    let (data, level) = (&job.data, job.level);
    let blades = match row_threads().filter(|_| job.edit) {
        Some(threads) => {
            threads.install(|| details::scatter_level_parallel(data, lo, lo + CHUNK, level))
        }
        None => details::scatter_level(data, lo, lo + CHUNK, level),
    };
    let done = Done {
        key: job.key,
        level: job.level,
        generation: job.generation,
        stamp: job.stamp,
        blades,
        scatter_ms: started.elapsed().as_secs_f32() * 1000.0,
    };
    // The terrain may be gone by now; nobody is waiting then.
    let _ = job.reply.send(done);
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    static STARTED: std::sync::Once = std::sync::Once::new();
    let shared = POOL.get_or_init(|| Pool {
        queue: Mutex::new(Queue::default()),
        wake: Condvar::new(),
    });
    STARTED.call_once(|| {
        let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 4));
        for i in 0..workers {
            let spawned = std::thread::Builder::new()
                .name(format!("misty grass scatter {i}"))
                .spawn(move || {
                    loop {
                        scatter_job(shared.take());
                    }
                });
            if let Err(e) = spawned {
                eprintln!("grass scatter worker: {e}");
            }
        }
    });
    shared
}

/// What the last [`Grass::update`] did (Stats overlay, tests).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GrassStats {
    /// Chunks handed to the workers / uploaded this frame.
    pub scheduled: u32,
    pub uploaded: u32,
    /// Chunks still being scattered or waiting for upload.
    pub pending: u32,
    /// Render-thread time spent uploading this frame.
    pub upload_ms: f32,
    /// Time already spent when the last piece of the frame started uploading: always under
    /// the budget (the budget decides whether another piece starts, not how long it takes).
    pub upload_last_start_ms: f32,
    /// Longest single upload piece this frame (a piece is at most [`UPLOAD_PIECE`] blades).
    pub upload_piece_ms: f32,
    /// Of that, the longest wait for room in the blade pages and the cull arena.
    pub upload_alloc_ms: f32,
    /// Longest worker scatter among the chunks uploaded this frame.
    pub scatter_ms: f32,
}

pub(super) struct Grass {
    chunks: HashMap<(u32, u32), Chunk>,
    /// Settings the blades were scattered with (look-only fields cleared).
    key: Option<ScatterKey>,
    /// Bumped whenever the blades must be scattered afresh (older results are dropped).
    generation: u64,
    /// Chunks handed to the workers and not back yet (one job per chunk at a time while it
    /// is edited: changes made meanwhile go out with its next job).
    in_flight: HashMap<(u32, u32), Asked>,
    /// Edits so far ([`Grass::invalidate`] calls); blades carry the count they were scattered
    /// with.
    edits: u64,
    /// Chunks changed since their blades were scattered, with the count of their last edit.
    /// They keep drawing their old blades until newer ones are uploaded.
    dirty: HashMap<(u32, u32), u64>,
    /// Cameras of recent frames (terrain-local) and when they were last seen: chunks near
    /// any of them stay.
    cameras: Vec<(Vec3, Instant)>,
    /// The workers' replies (the receiver is locked because the renderer is shared across
    /// threads while culling).
    results: (mpsc::Sender<Done>, Mutex<mpsc::Receiver<Done>>),
    /// Scattered chunks waiting for the upload budget (nearest first).
    ready: VecDeque<Done>,
    /// The chunk going up now (in pieces over frames).
    uploading: Option<Upload>,
    /// Recent time of one upload piece (ms): a piece only starts when it fits in what is
    /// left of the frame's budget (the first of a frame always goes).
    piece_ms: f32,
    pub stats: GrassStats,
    /// Share of the blades drawn at every distance (Quality ▸ Grass Density; the rest grow
    /// wider to cover the same ground).
    density: f32,
    /// Cull records of every chunk (grass_cull.rs; made with the first chunk).
    arena: Option<Arena>,
    /// The blades of every chunk (grass_pool.rs; made with the first chunk).
    pool: Option<BladePool>,
    /// Group 3 per blade page for the culled passes (with the list and page generations it
    /// was made for): every pooled chunk of a page draws with it.
    page_groups: Vec<((u64, u64), wgpu::BindGroup)>,
    /// This frame's passes and their culling.
    cull: Option<CullFrame>,
    /// 0, 1, 2, ...: the list of the drawn blades when culling runs on the CPU (no indirect
    /// draws with a first instance on this device): every blade of the prefix.
    identity: Option<(u32, wgpu::Buffer)>,
}

impl Default for Grass {
    fn default() -> Self {
        Self {
            chunks: HashMap::new(),
            key: None,
            generation: 0,
            in_flight: HashMap::new(),
            edits: 0,
            dirty: HashMap::new(),
            cameras: Vec::new(),
            results: {
                let (tx, rx) = mpsc::channel();
                (tx, Mutex::new(rx))
            },
            ready: VecDeque::new(),
            uploading: None,
            piece_ms: 0.0,
            stats: GrassStats::default(),
            density: 1.0,
            arena: None,
            pool: None,
            page_groups: Vec::new(),
            cull: None,
            identity: None,
        }
    }
}

/// What the blades were scattered from: the grass settings (look-only fields cleared), the
/// flowers, the number of trees (flowers gather around them) and the seed.
type ScatterKey = (
    DetailSettings,
    FlowerSettings,
    usize,
    u32,
    Vec<crate::terrain::detail_layers::DetailLayer>,
);

/// Settings that change where blades are and how big (colours and wind are uniforms).
fn scatter_key(d: &TerrainData) -> ScatterKey {
    let mut s = d.details.clone();
    s.healthy_color = [0.0; 3];
    s.dry_color = [0.0; 3];
    s.wind_strength = 0.0;
    s.wind_speed = 0.0;
    s.draw_distance = 0.0;
    s.seed_color = [0.0; 3];
    s.translucency = 0.0;
    s.tip_dryness = 0.0;
    s.lod_distance = 0.0;
    s.lush_color = [0.0; 3];
    s.lush_amount = 0.0;
    s.dry_amount = 0.0;
    s.color_variation = 0.0;
    s.ground_tint = 0.0;
    s.cast_shadows = false;
    s.wind_direction = 0.0;
    s.gust_strength = 0.0;
    s.gust_scale = 0.0;
    s.turbulence = 0.0;
    // (Not the painted map: painting invalidates the region it changed.)
    let f = &d.flowers;
    let f = FlowerSettings {
        enabled: f.enabled,
        kinds: f.kinds.clone(),
        density: f.density,
        coverage: f.coverage,
        patch_scale: f.patch_scale,
        scattered: f.scattered,
        edges: f.edges,
        lod_distance: 0.0,
        map: Vec::new(),
    };
    // Grass species (their maps are painted by region, like the terrain grass's).
    let species = (d.detail_layers.iter())
        .filter(|l| l.kind == crate::terrain::detail_layers::DetailKind::Grass)
        .map(|l| l.settings())
        .collect();
    (s, f, d.trees.instances.len(), d.rule_seed, species)
}

/// Chunk `(x, z)` is within `r` metres of the camera (terrain-local).
fn near(x: u32, z: u32, camera: Vec3, r: f32) -> bool {
    let c = (Vec2::new(x as f32, z as f32) + 0.5) * CHUNK;
    c.distance(Vec2::new(camera.x, camera.z)) < r + CHUNK
}

/// Level a chunk needs: from its nearest point to the camera; `density` = the quality
/// level's share of the blades.
fn wanted(x: u32, z: u32, camera: Vec3, lod: f32, density: f32) -> u32 {
    let lo = Vec2::new(x as f32, z as f32) * CHUNK;
    let p = Vec2::new(camera.x, camera.z).clamp(lo, lo + CHUNK);
    let dist = Vec3::new(p.x - camera.x, 0.0, p.y - camera.z).length();
    level_for(keep_at(dist, lod) * density)
}

/// Squared chunk distance from the camera's chunk (ordering key).
fn chunk_dist(k: &(u32, u32), camera: Vec3) -> i64 {
    let (cx, cz) = ((camera.x / CHUNK) as i64, (camera.z / CHUNK) as i64);
    (k.0 as i64 - cx).pow(2) + (k.1 as i64 - cz).pow(2)
}

impl Grass {
    /// Blades of a changed region (`None` = everything) are out of date: their chunks are
    /// scattered again from the latest data, edited chunks before any other, and keep drawing
    /// their old blades until the new ones are uploaded (painting never opens a hole). A
    /// chunk edited again while its scatter runs waits for that one (it is shown, then the
    /// latest state goes out): a stroke never restarts work, it only adds the next round.
    pub fn invalidate(&mut self, d: &TerrainData, region: Option<Region>) {
        let Some(r) = region else {
            self.outdate_all();
            return;
        };
        if r.is_empty() {
            return;
        }
        self.edits += 1;
        let stamp = self.edits;
        let size = Vec2::new(d.size.x, d.size.z);
        let lo = Vec2::from(r.min) * size;
        let hi = Vec2::from(r.max) * size;
        // Chunks touching the region (its edges included).
        let first = ((lo / CHUNK).floor() - 1.0).max(Vec2::ZERO);
        let last = (hi / CHUNK).floor().min(size / CHUNK);
        if last.x < first.x || last.y < first.y {
            return;
        }
        for z in first.y as u32..=last.y as u32 {
            for x in first.x as u32..=last.x as u32 {
                let c0 = Vec2::new(x as f32, z as f32) * CHUNK;
                let c1 = c0 + CHUNK;
                if !(c1.x < lo.x || c1.y < lo.y || c0.x > hi.x || c0.y > hi.y) {
                    self.dirty.insert((x, z), stamp);
                }
            }
        }
    }

    /// Every chunk held or on its way is out of date (it keeps drawing until replaced).
    fn outdate_all(&mut self) {
        self.edits += 1;
        let stamp = self.edits;
        let keys: Vec<(u32, u32)> = (self.chunks.keys())
            .chain(self.in_flight.keys())
            .chain(self.ready.iter().map(|r| &r.key))
            .chain(self.uploading.as_ref().map(|u| &u.done.key))
            .copied()
            .collect();
        for k in keys {
            self.dirty.insert(k, stamp);
        }
    }

    /// Drop the chunks `keep` refuses (their blades and cull records go back to the pages
    /// and the arena).
    fn keep_chunks(&mut self, mut keep: impl FnMut((u32, u32)) -> bool) {
        let (arena, pool) = (&mut self.arena, &mut self.pool);
        self.chunks.retain(|k, c| {
            let kept = keep(*k);
            if !kept {
                free_chunk(arena, pool, c.store.as_ref(), c.arena, c.count);
            }
            kept
        });
    }

    /// Drop every scatter under way or waiting: the next update asks for them again.
    fn restart(&mut self) {
        self.generation += 1;
        self.in_flight.clear();
        self.dirty.clear();
        self.ready.clear();
        if let Some(up) = self.uploading.take() {
            self.abort_upload(up);
        }
    }

    /// Give back the room of an upload that will not finish.
    fn abort_upload(&mut self, up: Upload) {
        let n = up.done.blades.len() as u32;
        free_chunk(
            &mut self.arena,
            &mut self.pool,
            up.store.as_ref(),
            up.arena,
            n,
        );
    }

    /// The edit count chunk `k`'s blades are behind (None: up to date).
    fn outdated(&self, k: (u32, u32)) -> Option<u64> {
        let edit = *self.dirty.get(&k)?;
        self.chunks
            .get(&k)
            .is_none_or(|c| c.stamp < edit)
            .then_some(edit)
    }

    /// Chunks within the draw distance that are missing, coarser than the camera needs or
    /// behind an edit (and not already asked for): edited chunks first, then nearest first.
    /// With each whether it is an edit.
    fn stale_chunks(&self, d: &TerrainData, camera: Vec3) -> Vec<((u32, u32), bool)> {
        let range = d.details.draw_distance.max(1.0);
        let lod = d.details.lod_distance;
        let per_x = (d.size.x / CHUNK).ceil() as i64;
        let per_z = (d.size.z / CHUNK).ceil() as i64;
        let (cx, cz) = ((camera.x / CHUNK) as i64, (camera.z / CHUNK) as i64);
        let reach = (range / CHUNK).ceil() as i64 + 1;
        let mut todo: Vec<((u32, u32), bool)> = Vec::new();
        for z in (cz - reach).max(0)..(cz + reach + 1).min(per_z) {
            for x in (cx - reach).max(0)..(cx + reach + 1).min(per_x) {
                let k = (x as u32, z as u32);
                if !near(k.0, k.1, camera, range) {
                    continue;
                }
                let want = wanted(k.0, k.1, camera, lod, self.density);
                let edit = self.outdated(k);
                let stale = edit.is_some() || self.chunks.get(&k).is_none_or(|c| c.level > want);
                if !stale {
                    continue;
                }
                // Already on its way: a result at least this fine with these edits, or (while
                // edited) any job, whose result shows before the next round goes out.
                let covers =
                    |level: u32, stamp: u64| level <= want && edit.is_none_or(|e| stamp >= e);
                let asked = (self.in_flight.get(&k))
                    .is_some_and(|a| edit.is_some() || covers(a.level, a.stamp))
                    || (self.ready.iter()).any(|r| r.key == k && covers(r.level, r.stamp))
                    || (self.uploading.as_ref())
                        .is_some_and(|u| u.done.key == k && covers(u.done.level, u.done.stamp));
                if !asked {
                    todo.push((k, edit.is_some()));
                }
            }
        }
        todo.sort_by_key(|(k, edit)| (!*edit, chunk_dist(k, camera)));
        todo
    }

    /// Scatter missing chunks near the camera (terrain-local) and drop far ones; `density`
    /// = the quality level's share of the blades (0..1).
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        d: &Arc<TerrainData>,
        camera: Vec3,
        density: f32,
    ) {
        crate::profile_scope!("Terrain.Grass");
        self.stats = GrassStats::default();
        self.density = density.clamp(MIN_KEEP, 1.0);
        // New settings (a slider being dragged): every chunk is scattered again, drawing its
        // old blades meanwhile.
        let key = scatter_key(d);
        if self.key.as_ref() != Some(&key) {
            if self.key.is_some() {
                self.outdate_all();
            }
            self.key = Some(key);
        }
        if !crate::terrain::detail_layers::draws_grass(d) {
            self.keep_chunks(|_| false);
            self.restart();
            return;
        }
        let range = d.details.draw_distance.max(1.0);
        let lod = d.details.lod_distance;
        let now = Instant::now();
        (self.cameras)
            .retain(|(c, t)| now.duration_since(*t) < CAMERA_MEMORY && c.distance(camera) > 1.0);
        self.cameras.push((camera, now));
        let cameras: Vec<Vec3> = self.cameras.iter().map(|c| c.0).collect();
        let near_any = |x: u32, z: u32, r: f32| cameras.iter().any(|c| near(x, z, *c, r));
        // Far chunks go, and edited ones past the draw distance (they show nothing there and
        // are scattered afresh when a camera comes back); chunks coarser than wanted stay
        // until their finer scatter lands.
        let edited_far: Vec<(u32, u32)> = (self.dirty.keys())
            .filter(|k| !near_any(k.0, k.1, range) && self.outdated(**k).is_some())
            .copied()
            .collect();
        self.keep_chunks(|k| near_any(k.0, k.1, range * 1.5) && !edited_far.contains(&k));
        self.in_flight
            .retain(|(x, z), _| near_any(*x, *z, range * 1.5));
        self.dirty.retain(|(x, z), _| near_any(*x, *z, range * 1.5));

        // Finished scatters: keep the ones still worth uploading, edits and nearest first.
        self.collect(camera, &cameras, range);
        self.upload(device, queue, grass_streaming());

        // Hand out stale chunks, as many as the workers can take: edited ones first, on top of
        // the camera's share.
        for (k, edit) in self.stale_chunks(d, camera) {
            let cap = if edit {
                MAX_IN_FLIGHT + MAX_EDIT_IN_FLIGHT
            } else {
                MAX_IN_FLIGHT
            };
            if self.in_flight.len() >= cap {
                if edit {
                    continue;
                }
                break;
            }
            let asked = Asked {
                level: wanted(k.0, k.1, camera, lod, self.density),
                stamp: self.edits,
            };
            let job = Job {
                data: Arc::clone(d),
                key: k,
                level: asked.level,
                generation: self.generation,
                stamp: asked.stamp,
                edit,
                reply: self.results.0.clone(),
            };
            pool().send(job);
            self.in_flight.insert(k, asked);
            self.stats.scheduled += 1;
        }
        if !grass_streaming() {
            // Wait for everything asked for and upload it all now; more rounds while chunks
            // are still missing (one round covers `MAX_IN_FLIGHT` chunks).
            while !self.in_flight.is_empty() {
                // (A worker that died would never answer: give up rather than hang.)
                let reply = self
                    .results
                    .1
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .recv_timeout(Duration::from_secs(30));
                let Ok(done) = reply else {
                    break;
                };
                self.accept(done, &cameras, range);
            }
            self.upload(device, queue, false);
            if !self.stale_chunks(d, camera).is_empty() {
                let stats = self.stats;
                self.update(device, queue, d, camera, density);
                self.stats.scheduled += stats.scheduled;
                self.stats.uploaded += stats.uploaded;
                self.stats.upload_ms += stats.upload_ms;
            }
        }
        // (Edited chunks not handed out yet only wait when the workers are full: then
        // `in_flight` counts.)
        self.stats.pending =
            (self.in_flight.len() + self.ready.len() + self.uploading.is_some() as usize) as u32;
    }

    /// Take the workers' results: those of the current generation that still improve a chunk
    /// within reach wait for upload, edits and nearest first.
    fn collect(&mut self, camera: Vec3, cameras: &[Vec3], range: f32) {
        let replies: Vec<Done> = {
            let rx = self.results.1.lock().unwrap_or_else(|e| e.into_inner());
            rx.try_iter().collect()
        };
        for done in replies {
            self.accept(done, cameras, range);
        }
        if self.ready.len() > 1 {
            let dirty = &self.dirty;
            self.ready
                .make_contiguous()
                .sort_by_key(|d| (!dirty.contains_key(&d.key), chunk_dist(&d.key, camera)));
        }
    }

    /// One worker result: queued for upload when it improves on what the chunk holds or has
    /// on its way (newer edits, or the same edits finer), replacing older results of the
    /// chunk not uploaded yet.
    fn accept(&mut self, done: Done, cameras: &[Vec3], range: f32) {
        if done.generation != self.generation {
            return;
        }
        let asked = Asked {
            level: done.level,
            stamp: done.stamp,
        };
        // Only chunks still asked for (a job dropped when its chunk went out of reach may
        // carry data edited since). A finer job for the same chunk may have been handed out
        // since: that one stays in flight.
        match self.in_flight.get(&done.key) {
            None => return,
            Some(a) if *a == asked => {
                self.in_flight.remove(&done.key);
            }
            Some(_) => {}
        }
        let better = |level: u32, stamp: u64| {
            done.stamp > stamp || (done.stamp == stamp && done.level < level)
        };
        let improves = self
            .chunks
            .get(&done.key)
            .is_none_or(|c| better(c.level, c.stamp));
        let (x, z) = done.key;
        let near_any = |r: f32| cameras.iter().any(|c| near(x, z, *c, r));
        // (An outdated chunk past the draw distance is dropped, not uploaded.)
        let behind =
            (self.dirty.get(&done.key)).is_some_and(|e| done.stamp < *e) && !near_any(range);
        if !improves || behind || !near_any(range * 1.5) {
            return;
        }
        // Results of the chunk still waiting: the better one stays.
        if (self.ready.iter()).any(|r| r.key == done.key && !better(r.level, r.stamp)) {
            return;
        }
        self.ready.retain(|r| r.key != done.key);
        if let Some(up) = self.uploading.take_if(|u| u.done.key == done.key) {
            if better(up.done.level, up.done.stamp) {
                self.abort_upload(up);
            } else {
                self.uploading = Some(up);
                return;
            }
        }
        self.ready.push_back(done);
    }

    /// Upload finished chunks piece by piece ([`UPLOAD_PIECE`] blades at a time): within
    /// [`UPLOAD_BUDGET`] ([`EDIT_UPLOAD_BUDGET`] while edited chunks go up; always at least
    /// one piece) when `budgeted`, else all of them. A chunk replaces its older version only
    /// once all its blades are on the GPU.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, budgeted: bool) {
        let started = Instant::now();
        let mut first = true;
        loop {
            let piece_start = Instant::now();
            let spent = started.elapsed().as_secs_f32() * 1000.0;
            // (While an edited chunk waits, the chunk going up ahead of it goes faster too.)
            let edit = (self.uploading.as_ref().map(|u| u.done.key))
                .into_iter()
                .chain(self.ready.front().map(|d| d.key))
                .any(|k| self.dirty.contains_key(&k));
            let budget = if edit {
                EDIT_UPLOAD_BUDGET
            } else {
                UPLOAD_BUDGET
            };
            let budget_ms = budget.as_secs_f32() * 1000.0;
            if budgeted && !first && spent + self.piece_ms > budget_ms {
                break;
            }
            first = false;
            if self.uploading.is_none() {
                let Some(done) = self.ready.pop_front() else {
                    break;
                };
                self.stats.scatter_ms = self.stats.scatter_ms.max(done.scatter_ms);
                let up = self.begin_upload(device, queue, done);
                let alloc_ms = piece_start.elapsed().as_secs_f32() * 1000.0;
                self.stats.upload_alloc_ms = self.stats.upload_alloc_ms.max(alloc_ms);
                self.uploading = Some(up);
            }
            let up = self.uploading.as_mut().expect("set above");
            let total = up.done.blades.len();
            let end = (up.written + UPLOAD_PIECE).min(total);
            let piece = &up.done.blades[up.written..end];
            if let (Some(Store::Pool(slot)), Some(pool)) = (&up.store, &self.pool) {
                pool.write(queue, *slot, up.written as u32, piece);
            }
            if let (Some(at), Some(arena)) = (up.arena, &self.arena) {
                arena.write(queue, at, up.written as u32, piece);
            }
            up.written = end;
            if up.written >= total {
                let up = self.uploading.take().expect("set above");
                self.finish_upload(up);
            }
            let piece_ms = piece_start.elapsed().as_secs_f32() * 1000.0;
            self.stats.upload_piece_ms = self.stats.upload_piece_ms.max(piece_ms);
            // (A slow piece raises the estimate at once, fast ones lower it gently.)
            self.piece_ms = piece_ms.max(self.piece_ms * 0.9 + piece_ms * 0.1);
            let elapsed = started.elapsed();
            if budgeted && elapsed >= budget {
                break;
            }
            // The next piece, if any, starts now: under the budget by construction.
            self.stats.upload_last_start_ms = elapsed.as_secs_f32() * 1000.0;
        }
        self.stats.upload_ms += started.elapsed().as_secs_f32() * 1000.0;
    }

    /// Room for a scattered chunk's blades (a run in the pages, else a buffer of its own,
    /// written whole) and its cull records.
    fn begin_upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, done: Done) -> Upload {
        let n = done.blades.len() as u32;
        let store = (n > 0).then(|| {
            let pool = self.pool.get_or_insert_with(|| BladePool::new(device));
            match pool.alloc(device, queue, n) {
                Some(slot) => Store::Pool(slot),
                None => Store::Own(
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("misty grass blades"),
                        contents: bytemuck::cast_slice(&done.blades),
                        usage: wgpu::BufferUsages::STORAGE,
                    }),
                ),
            }
        });
        let arena = match (&store, grass_cull_supported(device)) {
            (Some(_), true) => self
                .arena
                .get_or_insert_with(|| Arena::new(device))
                .reserve(device, queue, n),
            _ => None,
        };
        Upload {
            done,
            store,
            arena,
            written: 0,
        }
    }

    /// Swap a fully uploaded chunk in (its older version's blades go back).
    fn finish_upload(&mut self, up: Upload) {
        let done = up.done;
        let lo = Vec2::new(done.key.0 as f32, done.key.1 as f32) * CHUNK;
        let blades = &done.blades;
        let top = blades
            .iter()
            .map(|b| b.position[1] + b.position[3])
            .fold(f32::MIN, f32::max);
        let bottom = blades
            .iter()
            .map(|b| b.position[1])
            .fold(f32::MAX, f32::min);
        let bounds = Aabb::new(
            Vec3::new(lo.x - 1.0, bottom - 1.0, lo.y - 1.0),
            Vec3::new(lo.x + CHUNK + 1.0, top + 1.0, lo.y + CHUNK + 1.0),
        );
        let old = self.chunks.insert(
            done.key,
            Chunk {
                store: up.store,
                count: blades.len() as u32,
                level: done.level,
                stamp: done.stamp,
                bounds,
                arena: up.arena,
                group: None,
            },
        );
        if let Some(old) = old {
            free_chunk(
                &mut self.arena,
                &mut self.pool,
                old.store.as_ref(),
                old.arena,
                old.count,
            );
        }
        // Up to date with its edits: no longer dirty.
        if (self.dirty.get(&done.key)).is_some_and(|e| *e <= done.stamp) {
            self.dirty.remove(&done.key);
        }
        self.stats.uploaded += 1;
    }

    /// The chunks a pass draws, nearest first, and how many of each one's key-sorted blades:
    /// far chunks draw only the share their nearest point keeps (the vertex shader shrinks
    /// away the rest blade by blade). `limit` keeps only the chunks within a range, a share of
    /// their blades (shadow casters, the prepass). With each chunk its arena offset.
    ///
    /// A blade shrunk away to nothing still has a width: its card collapses onto the root as
    /// a small flat fan, and in the chunks around the camera most drawn blades are such fans
    /// (they keep the ground between the standing blades covered). Only the lit pass draws
    /// them; the depth pass, the prepass and the shadow casters skip them (grass_blade.wgsl
    /// `grass_flat`, and the GPU culling before them).
    fn pass_chunks(
        &self,
        origin: Vec3,
        camera: Vec3,
        lod_distance: f32,
        frustum: &Frustum,
        limit: Option<Limit>,
    ) -> Vec<(JobDraw, u32, u32)> {
        // Nearest chunks first: their blades fill the depth buffer, so the many blades
        // behind them fail the depth test before shading.
        let mut order: Vec<(f32, (u32, u32), &Chunk)> = self
            .chunks
            .iter()
            .filter_map(|(k, c)| {
                c.store.as_ref()?;
                let world = Aabb::new(c.bounds.min + origin, c.bounds.max + origin);
                let dist = camera.clamp(world.min, world.max).distance(camera);
                let near = limit.is_none_or(|l| dist < l.range);
                (near && frustum.intersects(&world)).then_some((dist, *k, c))
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let share = limit.map_or(1.0, |l| l.share);
        let coarse_only = limit.is_some_and(|l| l.coarse);
        order
            .into_iter()
            .filter_map(|(dist, key, c)| {
                let keep = keep_at(dist, lod_distance) * self.density * share
                    / details::level_keep(c.level);
                let count = ((c.count as f32 * keep.min(1.0)).ceil() as u32).min(c.count);
                if count == 0 {
                    return None;
                }
                let (first_vertex, verts) = if coarse_only || dist > lod_distance * COARSE_FROM {
                    (100, COARSE_VERTICES)
                } else {
                    (0, BLADE_VERTICES)
                };
                let draw = JobDraw {
                    key,
                    job: 0,
                    count,
                    verts,
                    first_vertex,
                };
                let blades = c.pooled().map_or(0, Slot::list_base);
                Some((draw, c.arena.unwrap_or(0), blades))
            })
            .collect()
    }

    /// Lay out this frame's grass passes (`passes[k]` = pass k's culling tests, view and
    /// chunk limit; None = not drawn) and the groups their chunks draw with.
    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        pipelines: &CullPipelines,
        origin: Vec3,
        camera: Vec3,
        lod_distance: f32,
        passes: &[Option<(grass_cull::Kind, Frustum, Option<Limit>)>; KINDS],
    ) {
        // (Every chunk needs its records: a full arena falls back to the CPU path.)
        let gpu = GPU_CULL.load(Ordering::Relaxed)
            && grass_cull_supported(device)
            && self.arena.is_some()
            && (self.chunks.values()).all(|c| c.store.is_none() || c.arena.is_some());
        let planned: [Option<PlannedKind>; KINDS] = std::array::from_fn(|k| {
            let (kind, frustum, limit) = passes[k].as_ref()?;
            let draws = self.pass_chunks(origin, camera, lod_distance, frustum, *limit);
            Some(PlannedKind { kind: *kind, draws })
        });
        let cull = self.cull.get_or_insert_with(|| CullFrame::new(device));
        cull.plan(device, queue, pipelines, self.arena.as_ref(), &planned, gpu);
        let pages = self.pool.as_ref().map_or(0, |p| p.generation);
        // The list buffer the groups bind: the culled lists, else 0, 1, 2, ...
        let (generation, list) = if gpu {
            (cull.indices_generation, cull.indices().clone())
        } else {
            let longest = self.chunks.values().map(|c| c.count).max().unwrap_or(0);
            if self.identity.as_ref().is_none_or(|(n, _)| *n < longest) {
                let n = longest.next_power_of_two().max(1024);
                let ids: Vec<u32> = (0..n).collect();
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("misty grass identity list"),
                    contents: bytemuck::cast_slice(&ids),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                self.identity = Some((n, buffer));
            }
            // (Identity lists count their generations down from u64::MAX, culled ones up.)
            let (n, buffer) = self.identity.as_ref().expect("just made");
            (u64::MAX - *n as u64, buffer.clone())
        };
        let key = (generation, pages);
        // Culled passes: one group per page for all its chunks (the lists hold page indices).
        if gpu && let Some(pool) = &self.pool {
            self.page_groups.truncate(pool.pages());
            for page in 0..pool.pages() {
                if self.page_groups.get(page).is_some_and(|(k, _)| *k == key) {
                    continue;
                }
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("misty grass page group"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: pool.buffer(page as u32).as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: list.as_entire_binding(),
                        },
                    ],
                });
                match self.page_groups.get_mut(page) {
                    Some(g) => *g = (key, group),
                    None => self.page_groups.push((key, group)),
                }
            }
        }
        // The CPU path binds each chunk's run of blades on its own (and a chunk with a buffer
        // of its own always does).
        for d in cull.per_kind.iter().flatten() {
            let Some(c) = self.chunks.get_mut(&d.key) else {
                continue;
            };
            if c.group.as_ref().is_some_and(|(g, _)| *g == key) {
                continue;
            }
            let blades = match (&c.store, &self.pool) {
                (Some(Store::Own(buffer)), _) => buffer.as_entire_buffer_binding(),
                (Some(Store::Pool(slot)), Some(pool)) if !gpu => pool.binding(*slot, c.count),
                _ => continue,
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty grass blades group"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(blades),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: list.as_entire_binding(),
                    },
                ],
            });
            c.group = Some((key, group));
        }
    }

    /// This frame's lit grass can go through the visibility buffer: it is culled on the GPU
    /// and every chunk it draws lives in the blade pages (the visibility ids name blades by
    /// page and index, grass_vis.rs).
    pub fn vis_ready(&self) -> bool {
        let Some(cull) = &self.cull else {
            return true;
        };
        let lit = &cull.per_kind[grass_cull::LIT];
        lit.is_empty()
            || (cull.culled()
                && lit.iter().all(|d| {
                    self.chunks
                        .get(&d.key)
                        .is_some_and(|c| c.pooled().is_some())
                }))
    }

    /// The blade pages (the visibility resolve reads blades by id from them).
    pub fn pages(&self) -> Option<&BladePool> {
        self.pool.as_ref()
    }

    /// Blades held (chunks, total) (tests).
    #[cfg(test)]
    pub fn resident(&self) -> (usize, u64) {
        let blades = self.chunks.values().map(|c| c.count as u64).sum();
        (self.chunks.len(), blades)
    }

    /// Blades and density level of chunk `k` (None: not held) (tests).
    #[cfg(test)]
    pub fn chunk(&self, k: (u32, u32)) -> Option<(u32, u32)> {
        self.chunks.get(&k).map(|c| (c.count, c.level))
    }

    /// Chunk `k` holds blades of the latest data (tests).
    #[cfg(test)]
    pub fn fresh(&self, k: (u32, u32)) -> bool {
        self.chunks.contains_key(&k) && self.outdated(k).is_none()
    }

    #[cfg(test)]
    pub fn read_cull_counts(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<[(u64, u64); KINDS]> {
        Some(self.cull.as_ref()?.read_counts(device, queue))
    }

    /// Record this frame's culling (before the first grass pass).
    pub fn encode_cull(&self, pipelines: &CullPipelines, encoder: &mut wgpu::CommandEncoder) {
        if let Some(cull) = &self.cull {
            cull.encode(pipelines, encoder);
        }
    }

    /// This frame's blades are culled on the GPU and drawn indirectly.
    pub fn indirect(&self) -> bool {
        self.cull.as_ref().is_some_and(|c| c.culled())
    }

    /// Draw pass `kind` as planned (pipeline and groups 0–2 already bound).
    pub fn draw(&self, pass: &mut wgpu::RenderPass, kind: usize, stats: &mut super::RenderStats) {
        let Some(cull) = &self.cull else {
            return;
        };
        let indirect = cull.culled();
        // (Consecutive chunks of one page share its group: bound once.)
        let mut bound: Option<*const wgpu::BindGroup> = None;
        for d in &cull.per_kind[kind] {
            let Some(c) = self.chunks.get(&d.key) else {
                continue;
            };
            let group = match c.pooled() {
                Some(slot) if indirect => self.page_groups.get(slot.page as usize).map(|g| &g.1),
                _ => c.group.as_ref().map(|g| &g.1),
            };
            let Some(group) = group else {
                continue;
            };
            if bound != Some(group as *const _) {
                pass.set_bind_group(3, group, &[]);
                bound = Some(group as *const _);
            }
            if indirect {
                pass.draw_indirect(cull.args(), d.job as u64 * 16);
            } else {
                pass.draw(d.first_vertex..d.first_vertex + d.verts, 0..d.count);
            }
            stats.draw(d.count, (d.verts - 2) * 3, d.verts);
        }
    }

    pub fn bytes(&self) -> u64 {
        let own: u64 = (self.chunks.values())
            .filter(|c| matches!(c.store, Some(Store::Own(_))))
            .map(|c| c.count as u64 * std::mem::size_of::<details::Blade>() as u64)
            .sum();
        own + self.pool.as_ref().map_or(0, BladePool::bytes)
            + self.arena.as_ref().map_or(0, Arena::bytes)
            + self.cull.as_ref().map_or(0, CullFrame::bytes)
    }
}

/// Give a dropped chunk's blades and cull records back.
fn free_chunk(
    arena: &mut Option<Arena>,
    pool: &mut Option<BladePool>,
    store: Option<&Store>,
    records: Option<u32>,
    count: u32,
) {
    if let (Some(a), Some(at)) = (arena.as_mut(), records) {
        a.free(at, count);
    }
    if let (Some(p), Some(Store::Pool(slot))) = (pool.as_mut(), store) {
        p.free(*slot, count);
    }
}

/// Grass is culled on the GPU when the device draws indirect with a first instance (else
/// the CPU picks the chunks and the vertex stage tests every blade, as before).
pub(super) fn grass_cull_supported(device: &wgpu::Device) -> bool {
    device
        .features()
        .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
}

/// Grass uniform (see grass_blade.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct GrassParams {
    origin: [f32; 4],
    healthy: [f32; 4],
    dry: [f32; 4],
    wind: [f32; 4],
    /// rgb = linear seed-head colour, w = translucency.
    seed: [f32; 4],
    /// x = full-density distance, y = tip dryness, z = most widening, w = colour variation.
    look: [f32; 4],
    /// x = the terrain's draw slot (its reflection probes), y = 1 with the ground cover (the
    /// lit pass skips the flat fans), z = the terrain's number in the visibility ids, w = the
    /// flowers' LOD distance (f32 bits).
    draw: [u32; 4],
    /// rgb = linear lush colour, w = patch size.
    lush: [f32; 4],
    /// x = lush amount, y = dry amount, z = gust strength, w = turbulence.
    amounts: [f32; 4],
    /// xyz = camera position, w = gust wave length.
    camera: [f32; 4],
    /// x = caster range, y = caster share, z = 1 in shadow passes, w = ground tint (the
    /// share of the range where the casters start to fade out in shadow passes).
    shadow: [f32; 4],
    /// x = angle of one pixel (radians), y = 1 with temporal anti-aliasing, z = radius of
    /// the near-camera grass shadow map, w = seconds since last frame (wind motion vectors).
    screen: [f32; 4],
    /// x = shortest and y = tallest blade height of the turf (m), z = height variation,
    /// w = the quality level's share of the blades (the rest grow wider).
    canopy: [f32; 4],
}

/// The built-in grass atlas: four clumps of thin field-grass blades (`tools/grass_atlas.py`)
/// and four photographed seed heads (timothy heads, open panicles) in 128×1024 cells, alpha
/// keyed (see grass.wgsl).
const ATLAS_PNG: &[u8] = include_bytes!("assets/grass_atlas.png");

/// Cells of the atlas (side by side).
const ATLAS_CELLS: u32 = 8;

/// Scale each cell's alpha in `mip` so as many texels pass the 0.5 alpha test as in the full
/// image (`coverage` per cell): plain box filtering would let thin blades fade away (or bloat)
/// in the distance. A binary search per cell (Castaño, "Computing Alpha Mipmaps").
fn keep_coverage(mip: &mut image::RgbaImage, coverage: &[f32]) {
    let (w, h) = mip.dimensions();
    let cell_w = (w / ATLAS_CELLS).max(1);
    for (c, &want) in coverage.iter().enumerate() {
        let x0 = (c as u32 * cell_w).min(w - 1);
        let x1 = (x0 + cell_w).min(w);
        let share = |scale: f32| {
            let mut n = 0;
            for y in 0..h {
                for x in x0..x1 {
                    n += u32::from(mip.get_pixel(x, y)[3] as f32 * scale > 127.5);
                }
            }
            n as f32 / ((x1 - x0) * h) as f32
        };
        let (mut lo, mut hi) = (0.25f32, 16.0f32);
        for _ in 0..12 {
            let mid = (lo * hi).sqrt();
            if share(mid) < want {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let scale = (lo * hi).sqrt();
        for y in 0..h {
            for x in x0..x1 {
                let p = mip.get_pixel_mut(x, y);
                p[3] = (p[3] as f32 * scale).round().min(255.0) as u8;
            }
        }
    }
}

/// The atlas mip chain (sRGB), each level keeping the blades' alpha-tested coverage.
fn atlas_mips(image: image::RgbaImage) -> Vec<image::RgbaImage> {
    let mut chain = super::texture::mip_chain(image, true);
    let (w, h) = chain[0].dimensions();
    let cell_w = w / ATLAS_CELLS;
    let coverage: Vec<f32> = (0..ATLAS_CELLS)
        .map(|c| {
            let mut n = 0;
            for y in 0..h {
                for x in c * cell_w..(c + 1) * cell_w {
                    n += u32::from(chain[0].get_pixel(x, y)[3] > 127);
                }
            }
            n as f32 / (cell_w * h) as f32
        })
        .collect();
    // Down to cells two texels wide; smaller levels are only seen as specks.
    for mip in chain.iter_mut().skip(1) {
        if mip.width() >= ATLAS_CELLS * 2 {
            keep_coverage(mip, &coverage);
        }
    }
    chain
}

/// The built-in atlas decoded with its coverage-keeping mips (RGBA8, sRGB). Made once per
/// process (40 ms of PNG decoding and filtering), on a worker thread when [`prewarm_atlas`]
/// ran first.
fn atlas_chain() -> &'static Vec<image::RgbaImage> {
    static CHAIN: std::sync::OnceLock<Vec<image::RgbaImage>> = std::sync::OnceLock::new();
    CHAIN.get_or_init(|| atlas_mips(atlas_image()))
}

fn atlas_image() -> image::RgbaImage {
    image::load_from_memory(ATLAS_PNG)
        .expect("built-in grass atlas")
        .into_rgba8()
}

/// How the built-in atlas is compressed: colour with alpha-tested coverage (BC7, sRGB).
fn atlas_spec(ctx: &Ctx) -> TexSpec<'static> {
    TexSpec {
        recipe: "grass atlas coverage 8 cells",
        format: ctx.format(Role::ColorAlpha, ""),
        srgb: true,
        coverage: true,
    }
}

/// The built-in atlas under `ctx`: its blocks from the texture cache, else encoded now when
/// the context encodes, else RGBA8.
pub(super) fn atlas_layers(ctx: &Ctx) -> Layers {
    generated::make_one(atlas_image(), &atlas_spec(ctx), ctx, atlas_mips)
}

/// The built-in atlas's blocks of this run (None: not compressed, or still being encoded on a
/// worker in the editor: the RGBA8 atlas draws until then, see [`Atlas::outdated`]).
fn atlas_blocks(ctx: &Ctx) -> Option<&'static Compressed> {
    static BLOCKS: std::sync::OnceLock<Option<Compressed>> = std::sync::OnceLock::new();
    static STARTED: std::sync::Once = std::sync::Once::new();
    atlas_spec(ctx).format?;
    let make = |ctx: Ctx| match atlas_layers(&ctx).layers.pop() {
        Some(Made::Blocks(c)) => Some(c),
        _ => None,
    };
    if ctx.mode == Mode::Background {
        let ctx = Ctx {
            mode: Mode::Blocking,
            ..ctx.clone()
        };
        STARTED.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("misty grass atlas blocks".into())
                .spawn(move || {
                    BLOCKS.get_or_init(|| make(ctx));
                });
        });
        return BLOCKS.get().and_then(Option::as_ref);
    }
    BLOCKS.get_or_init(|| make(ctx.clone())).as_ref()
}

/// Start decoding the grass atlas on a worker thread (once per process).
pub(super) fn prewarm_atlas() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("misty grass atlas".into())
            .spawn(|| {
                atlas_chain();
            });
    });
}

/// Upload the grass atlas alone (a 2D texture) with coverage-keeping mips (sRGB) and its
/// clamped trilinear, anisotropic sampler (cards are seen at grazing angles): the grass study.
#[cfg(test)]
pub(super) fn atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let layer = Made::Rgba {
        chain: atlas_chain().clone(),
        srgb: true,
    };
    let (view, _) =
        super::texture::upload_made(device, queue, "misty grass atlas", &[layer], false);
    (view, atlas_sampler(device))
}

/// The flower kinds' textures in the layout of the grass atlas's flower layers: layer 1 =
/// albedo (four kinds side by side, each a column a quarter as wide as the atlas), layer 2 =
/// material (normal x, y, roughness, translucency; stored so the sRGB texture decodes them to
/// their linear values). Each with its mip chain (the albedo keeping its alpha-tested coverage
/// per head and plant cell).
pub(super) struct FlowerLayers {
    /// Albedo, material: blocks (both) or RGBA8 mip chains.
    pub(super) layers: Layers,
}

/// The textures a terrain's flower layers are made from (the cache key of [`flower_layers`]).
pub(super) fn flower_key(kinds: &[crate::terrain::flowers::FlowerKind]) -> Vec<String> {
    kinds
        .iter()
        .take(crate::terrain::flowers::MAX_KINDS)
        .flat_map(|k| {
            [
                k.texture.clone(),
                k.normal.clone(),
                k.roughness.clone(),
                k.translucency.clone(),
            ]
        })
        .collect()
}

/// Build the flower layers from the kinds' textures (decoding and filtering: tens of
/// milliseconds, on a worker thread while streaming). Missing textures leave their column
/// transparent (albedo) or flat (material).
pub(super) fn flower_layers(
    kinds: &[crate::terrain::flowers::FlowerKind],
    ctx: &Ctx,
) -> FlowerLayers {
    let (w, h) = atlas_chain()[0].dimensions();
    let cols = crate::terrain::flowers::MAX_KINDS as u32;
    let cw = w / cols;
    let load = |rel: &str| -> Option<image::RgbaImage> {
        if rel.is_empty() {
            return None;
        }
        let img = super::texture::load_image(&crate::assets::absolute(rel)).ok()?;
        Some(if img.dimensions() == (cw, h) {
            img
        } else {
            image::imageops::resize(&img, cw, h, image::imageops::FilterType::Triangle)
        })
    };
    let mut albedo = image::RgbaImage::new(w, h);
    // Linear material values first.
    let mut material = image::RgbaImage::from_pixel(w, h, image::Rgba([128, 128, 153, 128]));
    for (slot, k) in kinds.iter().take(cols as usize).enumerate() {
        let x0 = slot as u32 * cw;
        if let Some(img) = load(&k.texture) {
            image::imageops::replace(&mut albedo, &img, x0 as i64, 0);
        }
        let normal = load(&k.normal);
        let rough = load(&k.roughness);
        let trans = load(&k.translucency);
        for y in 0..h {
            for x in 0..cw {
                let m = material.get_pixel_mut(x0 + x, y);
                if let Some(n) = &normal {
                    let p = n.get_pixel(x, y);
                    m[0] = p[0];
                    m[1] = p[1];
                }
                if let Some(r) = &rough {
                    m[2] = r.get_pixel(x, y)[0];
                }
                if let Some(t) = &trans {
                    m[3] = t.get_pixel(x, y)[0];
                }
            }
        }
    }
    // The material is uploaded into an sRGB layer: store each value so it decodes to itself.
    let encode: Vec<u8> = (0..=255u32)
        .map(|v| {
            let c = v as f32 / 255.0;
            let e = if c <= 0.003_130_8 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (e * 255.0).round().clamp(0.0, 255.0) as u8
        })
        .collect();
    // (Alpha, the translucency, is linear in an sRGB format already.)
    for p in material.pixels_mut() {
        for c in 0..3 {
            p[c] = encode[p[c] as usize];
        }
    }
    // Compressed like the built-in layer (one array): BC7, by the kinds' Compression settings
    // (their albedo / normal map files).
    let kinds = &kinds[..kinds.len().min(cols as usize)];
    let albedo_spec = TexSpec {
        recipe: "flower albedo coverage 4x4",
        format: ctx.format_of_all(Role::ColorAlpha, kinds.iter().map(|k| k.texture.as_str())),
        srgb: true,
        coverage: true,
    };
    let material_spec = TexSpec {
        recipe: "flower material box srgb",
        format: ctx.format_of_all(Role::Packed, kinds.iter().map(|k| k.normal.as_str())),
        srgb: true,
        coverage: false,
    };
    let albedo = generated::make_one(albedo, &albedo_spec, ctx, |i| {
        super::trees::coverage_mips(i, cols, 4)
    });
    let material = generated::make_one(material, &material_spec, ctx, |i| {
        super::texture::mip_chain(i, true)
    });
    let mut layers = albedo;
    layers.layers.extend(material.layers);
    layers.keys.extend(material.keys);
    layers.waiting.extend(material.waiting);
    layers.layers = generated::same_kind(layers.layers);
    FlowerLayers { layers }
}

/// The cache entries of the built-in atlas and of a terrain's flower layers under `ctx` (a
/// build's), encoding what is missing.
pub(crate) fn bake_textures(
    kinds: Option<&[crate::terrain::flowers::FlowerKind]>,
    ctx: &Ctx,
) -> Vec<String> {
    let mut out = atlas_layers(ctx).entries();
    if let Some(kinds) = kinds {
        out.extend(flower_layers(kinds, ctx).layers.entries());
    }
    out
}

/// A grass atlas array on the GPU.
pub(super) struct Atlas {
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bytes: u64,
    /// Flower slices being encoded in the background (the array is RGBA8 meanwhile).
    waiting: Vec<String>,
    /// Uploaded RGBA8 while the built-in layer's blocks were still being made.
    wants_blocks: bool,
}

impl Atlas {
    /// Whether a compressed atlas can replace this one now (build it again: cache hits).
    pub fn outdated(&self) -> bool {
        if !self.waiting.is_empty() {
            return !generated::waiting(&self.waiting);
        }
        self.wants_blocks && atlas_blocks(&Ctx::runtime(true)).is_some()
    }

    /// Keep this atlas as it is (building it again failed).
    pub fn keep(&mut self) {
        self.waiting.clear();
        self.wants_blocks = false;
    }
}

/// Upload the grass atlas as a three-layer array: the built-in grass (layer 0) and the flower
/// layers (transparent while `flowers` is None), with coverage-keeping mips (sRGB) and the
/// clamped trilinear, anisotropic sampler of [`atlas`]. Block-compressed (BC7) when the device,
/// the settings and the texture cache allow it; else RGBA8.
pub(super) fn atlas_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    flowers: Option<&FlowerLayers>,
) -> Atlas {
    let ctx = Ctx::runtime(
        device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
    );
    let chain = atlas_chain();
    let (size, levels) = (chain[0].dimensions(), chain.len() as u32);
    let fits = |m: &Made| m.size() == size && m.level_count() == levels;
    let flower_layers = flowers
        .map(|f| &f.layers)
        .filter(|l| l.layers.len() == 2 && l.layers.iter().all(fits));
    let waiting = flower_layers.map(|l| l.waiting.clone()).unwrap_or_default();
    let compressible = atlas_spec(&ctx).format.is_some()
        && flower_layers.is_none_or(|l| l.layers.iter().all(Made::is_compressed));
    let layers: Vec<Made> = match atlas_blocks(&ctx) {
        Some(grass) if compressible => {
            let flower = match flower_layers {
                Some(l) => l.layers.clone(),
                None => vec![transparent(grass); 2],
            };
            std::iter::once(Made::Blocks(grass.clone()))
                .chain(flower)
                .collect()
        }
        _ => {
            let flower = match flower_layers {
                Some(l) => l.layers.iter().cloned().map(Made::into_rgba).collect(),
                None => {
                    let empty: Vec<image::RgbaImage> = chain
                        .iter()
                        .map(|m| image::RgbaImage::new(m.width(), m.height()))
                        .collect();
                    let layer = Made::Rgba {
                        chain: empty,
                        srgb: true,
                    };
                    vec![layer; 2]
                }
            };
            let grass = Made::Rgba {
                chain: chain.clone(),
                srgb: true,
            };
            std::iter::once(grass).chain(flower).collect()
        }
    };
    let wants_blocks = compressible && !layers[0].is_compressed();
    let (view, bytes) =
        super::texture::upload_made(device, queue, "misty grass atlas", &layers, true);
    Atlas {
        view,
        sampler: atlas_sampler(device),
        bytes,
        waiting,
        wants_blocks,
    }
}

/// Transparent black blocks the shape of `like` (the empty flower layers).
fn transparent(like: &Compressed) -> Made {
    let block = encode_level(&image::RgbaImage::new(4, 4), like.format);
    let levels = like
        .levels
        .iter()
        .map(|l| block.repeat(l.len() / block.len()))
        .collect();
    Made::Blocks(Compressed {
        levels,
        ..like.clone()
    })
}

fn atlas_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("misty grass sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 16,
        ..Default::default()
    })
}

fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Wind blowing over the grass: the detail settings, scaled and turned by the scene's Wind
/// Zone when there is one.
pub(super) fn wind(s: &DetailSettings, zone: Option<&super::environment::Wind>) -> [f32; 6] {
    let a = s.wind_direction.to_radians();
    let mut dir = Vec2::new(a.sin(), a.cos());
    let (mut strength, mut speed, mut gusts, mut flutter) = (
        s.wind_strength,
        s.wind_speed * 5.0,
        s.gust_strength,
        s.turbulence,
    );
    if let Some(z) = zone {
        let d = Vec2::new(z.direction.x, z.direction.z);
        if d.length_squared() > 1e-6 {
            dir = d.normalize();
        }
        strength *= z.main;
        flutter *= z.turbulence;
        gusts *= z.pulse_magnitude;
        speed *= z.pulse_frequency;
    }
    [strength, speed, dir.x, dir.y, gusts, flutter]
}

impl GrassParams {
    /// Quality ▸ Grass Ground Cover: the lit grass leaves the flat fans out too (the terrain
    /// paints the undergrowth they drew).
    pub fn with_ground_cover(mut self, on: bool) -> Self {
        self.draw[1] = u32::from(on);
        self
    }

    /// Shadow passes: the share of the caster range where the casters start to fade out
    /// (grass_blade.wgsl `grass_drawn_height`; [`CASTER_FADE`] unless set).
    pub fn with_caster_fade(mut self, from: f32) -> Self {
        if self.shadow[2] > 0.5 {
            self.shadow[3] = from;
        }
        self
    }

    /// The terrain's number in the grass visibility ids (grass_vis.wgsl `grass_vis_id`).
    pub fn with_vis_index(mut self, index: u32) -> Self {
        self.draw[2] = index;
        self
    }

    /// The flowers' LOD distance (grass_blade.wgsl `grass_flower_keep`).
    pub fn with_flowers(mut self, lod_distance: f32) -> Self {
        self.draw[3] = lod_distance.max(1.0).to_bits();
        self
    }

    /// The culling tests of a pass drawn with this uniform through `view_proj` (the same
    /// numbers its vertex stage reads): `camera_pass` = the near plane counts, `skip_flat`
    /// = the pass leaves the flat fans out.
    pub fn cull_kind(
        &self,
        view_proj: &glam::Mat4,
        camera_pass: bool,
        skip_flat: bool,
    ) -> grass_cull::Kind {
        grass_cull::kind(
            view_proj,
            self.origin,
            self.camera,
            self.look[0],
            self.healthy[3],
            self.canopy[3],
            self.screen[0],
            self.shadow,
            self.screen[2],
            camera_pass,
            skip_flat,
        )
    }

    /// Uniform of one terrain's grass; `shadow` = caster range and share for shadow passes,
    /// `screen` = pixel angle, TAA on, the near shadow map's radius and the seconds since
    /// last frame ([`GrassParams::screen`]), `density` = the quality level's share of the blades.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        s: &DetailSettings,
        origin: Vec3,
        time: f32,
        draw: usize,
        camera: Vec3,
        zone: Option<&super::environment::Wind>,
        shadow: Option<(f32, f32)>,
        screen: [f32; 4],
        density: f32,
    ) -> Self {
        let far = s.draw_distance.max(1.0);
        let [hr, hg, hb] = s.healthy_color.map(lin);
        let [dr, dg, db] = s.dry_color.map(lin);
        let [sr, sg, sb] = s.seed_color.map(lin);
        let [lr, lg, lb] = s.lush_color.map(lin);
        let [strength, speed, dx, dz, gusts, flutter] = wind(s, zone);
        let (range, share) = shadow.unwrap_or((0.0, 1.0));
        Self {
            origin: [origin.x, origin.y, origin.z, time],
            healthy: [hr, hg, hb, far],
            dry: [dr, dg, db, far * 0.8],
            wind: [strength, speed, dx, dz],
            seed: [sr, sg, sb, s.translucency.clamp(0.0, 2.0)],
            look: [
                s.lod_distance.max(1.0),
                s.tip_dryness.clamp(0.0, 1.0),
                MAX_WIDEN,
                s.color_variation.clamp(0.0, 1.0),
            ],
            draw: [draw as u32, 0, 0, 0],
            lush: [lr, lg, lb, s.patch_scale.max(0.5)],
            amounts: [
                s.lush_amount.clamp(0.0, 1.0),
                s.dry_amount.clamp(0.0, 1.0),
                gusts.clamp(0.0, 2.0),
                flutter.clamp(0.0, 2.0),
            ],
            camera: [camera.x, camera.y, camera.z, s.gust_scale.max(1.0)],
            shadow: [
                range,
                share,
                if shadow.is_some() { 1.0 } else { 0.0 },
                if shadow.is_some() {
                    CASTER_FADE
                } else {
                    s.ground_tint.clamp(0.0, 1.0)
                },
            ],
            screen,
            canopy: [
                s.height[0].max(0.01),
                s.height[1].max(s.height[0]).max(0.02),
                s.height_variation.clamp(0.0, 1.0),
                density.clamp(MIN_KEEP, 1.0),
            ],
        }
    }
}

#[cfg(test)]
#[path = "grass_gpu_tests.rs"]
pub(super) mod gpu_tests;

#[cfg(test)]
#[path = "grass_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "grass_paint_tests.rs"]
mod paint_tests;
