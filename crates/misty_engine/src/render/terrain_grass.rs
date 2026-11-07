//! Grass of one terrain: blades are scattered per 16 m chunk around the camera (only within
//! the draw distance), kept in small instance buffers and dropped again when far away or when
//! the terrain under them changes. Scattering runs on worker threads ([`pool`]): the render
//! thread only hands out jobs (nearest chunk first) and uploads finished chunks within a
//! small time budget per frame, so moving the camera never stalls a frame. A chunk that
//! needs more blades keeps drawing its coarser blades until the finer scatter arrives.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use wgpu::util::DeviceExt;

use super::culling::{Aabb, Frustum};
use super::grass_cull::{self, Arena, CullFrame, CullPipelines, JobDraw, KINDS, PlannedKind};
use super::grass_pool::{BladePool, Slot};
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
/// Vertices of one blade (grass_blade.wgsl: four segments and a tip).
pub(super) const BLADE_VERTICES: u32 = 10;
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
    reply: mpsc::Sender<Done>,
}

/// A scattered chunk, waiting to be uploaded.
struct Done {
    key: (u32, u32),
    level: u32,
    generation: u64,
    blades: Vec<Blade>,
    /// Worker time of the scatter.
    scatter_ms: f32,
}

/// The scatter workers, shared by every terrain: half the cores (1–4), fed through a channel.
struct Pool {
    jobs: Mutex<mpsc::Sender<Job>>,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 4));
        for i in 0..workers {
            let rx = Arc::clone(&rx);
            let spawned = std::thread::Builder::new()
                .name(format!("misty grass scatter {i}"))
                .spawn(move || {
                    loop {
                        let job = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                        let Ok(job) = job else {
                            return;
                        };
                        let started = Instant::now();
                        let lo = Vec2::new(job.key.0 as f32, job.key.1 as f32) * CHUNK;
                        let blades = details::scatter_level(&job.data, lo, lo + CHUNK, job.level);
                        let done = Done {
                            key: job.key,
                            level: job.level,
                            generation: job.generation,
                            blades,
                            scatter_ms: started.elapsed().as_secs_f32() * 1000.0,
                        };
                        // The terrain may be gone by now; nobody is waiting then.
                        let _ = job.reply.send(done);
                    }
                });
            if let Err(e) = spawned {
                eprintln!("grass scatter worker: {e}");
            }
        }
        Pool {
            jobs: Mutex::new(tx),
        }
    })
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
    /// Chunks handed to the workers and not uploaded yet: the level asked for.
    in_flight: HashMap<(u32, u32), u32>,
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
type ScatterKey = (DetailSettings, FlowerSettings, usize, u32);

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
    (s, f, d.trees.instances.len(), d.rule_seed)
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
    /// Forget blades of a changed region (`None` = everything). Scatters under way are
    /// dropped too (their result would show the old terrain).
    pub fn invalidate(&mut self, d: &TerrainData, region: Option<Region>) {
        self.restart();
        let Some(r) = region else {
            self.keep_chunks(|_| false);
            return;
        };
        if r.is_empty() {
            return;
        }
        let size = Vec2::new(d.size.x, d.size.z);
        let lo = Vec2::from(r.min) * size;
        let hi = Vec2::from(r.max) * size;
        self.keep_chunks(|(x, z)| {
            let c0 = Vec2::new(x as f32, z as f32) * CHUNK;
            let c1 = c0 + CHUNK;
            c1.x < lo.x || c1.y < lo.y || c0.x > hi.x || c0.y > hi.y
        });
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
        self.ready.clear();
        if let Some(up) = self.uploading.take() {
            let n = up.done.blades.len() as u32;
            free_chunk(
                &mut self.arena,
                &mut self.pool,
                up.store.as_ref(),
                up.arena,
                n,
            );
        }
    }

    /// Chunks within the draw distance that are missing or coarser than the camera needs
    /// (and not already asked for), nearest first.
    fn stale_chunks(&self, d: &TerrainData, camera: Vec3) -> Vec<(u32, u32)> {
        let range = d.details.draw_distance.max(1.0);
        let lod = d.details.lod_distance;
        let per_x = (d.size.x / CHUNK).ceil() as i64;
        let per_z = (d.size.z / CHUNK).ceil() as i64;
        let (cx, cz) = ((camera.x / CHUNK) as i64, (camera.z / CHUNK) as i64);
        let reach = (range / CHUNK).ceil() as i64 + 1;
        let mut todo: Vec<(u32, u32)> = Vec::new();
        for z in (cz - reach).max(0)..(cz + reach + 1).min(per_z) {
            for x in (cx - reach).max(0)..(cx + reach + 1).min(per_x) {
                let k = (x as u32, z as u32);
                let want = wanted(k.0, k.1, camera, lod, self.density);
                let stale = self.chunks.get(&k).is_none_or(|c| c.level > want);
                let asked = self.in_flight.get(&k).is_some_and(|l| *l <= want)
                    || self.ready.iter().any(|r| r.key == k && r.level <= want)
                    || (self.uploading.as_ref())
                        .is_some_and(|u| u.done.key == k && u.done.level <= want);
                if stale && !asked && near(k.0, k.1, camera, range) {
                    todo.push(k);
                }
            }
        }
        todo.sort_by_key(|k| chunk_dist(k, camera));
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
        let key = scatter_key(d);
        if self.key.as_ref() != Some(&key) {
            self.keep_chunks(|_| false);
            self.restart();
            self.key = Some(key);
        }
        if !d.details.enabled || d.detail_map.is_empty() {
            self.keep_chunks(|_| false);
            self.restart();
            return;
        }
        let range = d.details.draw_distance.max(1.0);
        let lod = d.details.lod_distance;
        // Far chunks go; chunks coarser than wanted stay until their finer scatter lands.
        self.keep_chunks(|(x, z)| near(x, z, camera, range * 1.5));
        self.in_flight
            .retain(|(x, z), _| near(*x, *z, camera, range * 1.5));

        // Finished scatters: keep the ones still worth uploading, nearest first.
        self.collect(camera, range);
        self.upload(device, queue, grass_streaming());

        // Hand out the nearest stale chunks, as many as the workers can take.
        let slots = MAX_IN_FLIGHT.saturating_sub(self.in_flight.len());
        for k in self.stale_chunks(d, camera).into_iter().take(slots) {
            let level = wanted(k.0, k.1, camera, lod, self.density);
            let job = Job {
                data: Arc::clone(d),
                key: k,
                level,
                generation: self.generation,
                reply: self.results.0.clone(),
            };
            let sent = pool()
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .send(job);
            if sent.is_ok() {
                self.in_flight.insert(k, level);
                self.stats.scheduled += 1;
            }
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
                if done.generation == self.generation {
                    self.in_flight.remove(&done.key);
                    self.ready.push_back(done);
                }
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
        self.stats.pending =
            (self.in_flight.len() + self.ready.len() + self.uploading.is_some() as usize) as u32;
    }

    /// Take the workers' results: those of the current generation that still improve a chunk
    /// within reach wait for upload, nearest first.
    fn collect(&mut self, camera: Vec3, range: f32) {
        let replies = self.results.1.lock().unwrap_or_else(|e| e.into_inner());
        while let Ok(done) = replies.try_recv() {
            if done.generation != self.generation {
                continue;
            }
            // (A finer job for the same chunk may have been handed out since: that one is
            // still in flight.)
            if self.in_flight.get(&done.key) == Some(&done.level) {
                self.in_flight.remove(&done.key);
            }
            let finer = self
                .chunks
                .get(&done.key)
                .is_none_or(|c| c.level > done.level);
            if finer && near(done.key.0, done.key.1, camera, range * 1.5) {
                self.ready.push_back(done);
            }
        }
        if self.ready.len() > 1 {
            self.ready
                .make_contiguous()
                .sort_by_key(|d| chunk_dist(&d.key, camera));
        }
    }

    /// Upload finished chunks piece by piece ([`UPLOAD_PIECE`] blades at a time): within
    /// [`UPLOAD_BUDGET`] (always at least one piece) when `budgeted`, else all of them. A
    /// chunk replaces its older version only once all its blades are on the GPU.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, budgeted: bool) {
        let started = Instant::now();
        let budget = UPLOAD_BUDGET.as_secs_f32() * 1000.0;
        let mut first = true;
        loop {
            let piece_start = Instant::now();
            let spent = started.elapsed().as_secs_f32() * 1000.0;
            if budgeted && !first && spent + self.piece_ms > budget {
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
            if budgeted && elapsed >= UPLOAD_BUDGET {
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
    /// x = caster range, y = caster share, z = 1 in shadow passes, w = ground tint.
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

/// The built-in atlas decoded with its coverage-keeping mips: (width, height, levels, texels).
/// Made once per process (40 ms of PNG decoding and filtering), on a worker thread when
/// [`prewarm_atlas`] ran first.
fn atlas_data() -> &'static (u32, u32, u32, Vec<u8>) {
    static DATA: std::sync::OnceLock<(u32, u32, u32, Vec<u8>)> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        let image = image::load_from_memory(ATLAS_PNG)
            .expect("built-in grass atlas")
            .into_rgba8();
        let (w, h) = image.dimensions();
        let chain = atlas_mips(image);
        let data = chain
            .iter()
            .flat_map(|m| m.as_raw().iter().copied())
            .collect();
        (w, h, chain.len() as u32, data)
    })
}

/// Start decoding the grass atlas on a worker thread (once per process).
pub(super) fn prewarm_atlas() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("misty grass atlas".into())
            .spawn(|| {
                atlas_data();
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
    let (w, h, levels, data) = atlas_data();
    let (w, h, levels) = (*w, *h, *levels);
    let texture = wgpu::util::DeviceExt::create_texture_with_data(
        device,
        queue,
        &wgpu::TextureDescriptor {
            label: Some("misty grass atlas"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        data,
    );
    let view = texture.create_view(&Default::default());
    (view, atlas_sampler(device))
}

/// The flower kinds' textures in the layout of the grass atlas's flower layers: layer 1 =
/// albedo (four kinds side by side, each a column a quarter as wide as the atlas), layer 2 =
/// material (normal x, y, roughness, translucency; stored so the sRGB texture decodes them to
/// their linear values). Each with its mip chain (the albedo keeping its alpha-tested coverage
/// per head and plant cell).
pub(super) struct FlowerLayers {
    albedo: Vec<image::RgbaImage>,
    material: Vec<image::RgbaImage>,
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
pub(super) fn flower_layers(kinds: &[crate::terrain::flowers::FlowerKind]) -> FlowerLayers {
    let (w, h, _, _) = atlas_data();
    let (w, h) = (*w, *h);
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
    FlowerLayers {
        albedo: super::trees::coverage_mips(albedo, cols, 4),
        material: super::texture::mip_chain(material, true),
    }
}

/// Upload the grass atlas as a three-layer array: the built-in grass (layer 0) and the flower
/// layers (transparent while `flowers` is None), with coverage-keeping mips (sRGB) and the
/// clamped trilinear, anisotropic sampler of [`atlas`].
pub(super) fn atlas_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    flowers: Option<&FlowerLayers>,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let (w, h, levels, grass) = atlas_data();
    let (w, h, levels) = (*w, *h, *levels);
    let mut data = grass.clone();
    match flowers {
        Some(f) if f.albedo.len() == levels as usize && f.material.len() == levels as usize => {
            for chain in [&f.albedo, &f.material] {
                for mip in chain {
                    data.extend_from_slice(mip.as_raw());
                }
            }
        }
        _ => data.resize(grass.len() * 3, 0),
    }
    let texture = wgpu::util::DeviceExt::create_texture_with_data(
        device,
        queue,
        &wgpu::TextureDescriptor {
            label: Some("misty grass atlas"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 3,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    (view, atlas_sampler(device))
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
                s.ground_tint.clamp(0.0, 1.0),
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
