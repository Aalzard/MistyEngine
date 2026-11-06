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
use crate::terrain::TerrainData;
use crate::terrain::details::{self, Blade, DetailSettings};
use crate::terrain::store::Region;

/// Chunk side in metres.
const CHUNK: f32 = 16.0;
/// Render-thread time per frame for uploading finished chunks (the first one always goes).
pub(super) const UPLOAD_BUDGET: Duration = Duration::from_millis(2);
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

struct Chunk {
    buffer: Option<wgpu::Buffer>,
    count: u32,
    /// Density level the blades were scattered at.
    level: u32,
    /// Terrain-local bounds of the blades.
    bounds: Aabb,
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
    /// Time already spent when the last chunk of the frame started uploading: always under
    /// the budget (the budget decides whether another chunk starts, not how long it takes).
    pub upload_last_start_ms: f32,
    /// Longest worker scatter among the chunks uploaded this frame.
    pub scatter_ms: f32,
}

pub(super) struct Grass {
    chunks: HashMap<(u32, u32), Chunk>,
    /// Settings the blades were scattered with (look-only fields cleared).
    key: Option<(DetailSettings, u32)>,
    /// Bumped whenever the blades must be scattered afresh (older results are dropped).
    generation: u64,
    /// Chunks handed to the workers and not uploaded yet: the level asked for.
    in_flight: HashMap<(u32, u32), u32>,
    /// The workers' replies (the receiver is locked because the renderer is shared across
    /// threads while culling).
    results: (mpsc::Sender<Done>, Mutex<mpsc::Receiver<Done>>),
    /// Scattered chunks waiting for the upload budget (nearest first).
    ready: VecDeque<Done>,
    pub stats: GrassStats,
    /// Share of the blades drawn at every distance (Quality ▸ Grass Density; the rest grow
    /// wider to cover the same ground).
    density: f32,
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
            stats: GrassStats::default(),
            density: 1.0,
        }
    }
}

/// Settings that change where blades are and how big (colours and wind are uniforms).
fn scatter_key(d: &TerrainData) -> (DetailSettings, u32) {
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
    (s, d.rule_seed)
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
            self.chunks.clear();
            return;
        };
        if r.is_empty() {
            return;
        }
        let size = Vec2::new(d.size.x, d.size.z);
        let lo = Vec2::from(r.min) * size;
        let hi = Vec2::from(r.max) * size;
        self.chunks.retain(|(x, z), _| {
            let c0 = Vec2::new(*x as f32, *z as f32) * CHUNK;
            let c1 = c0 + CHUNK;
            c1.x < lo.x || c1.y < lo.y || c0.x > hi.x || c0.y > hi.y
        });
    }

    /// Drop every scatter under way or waiting: the next update asks for them again.
    fn restart(&mut self) {
        self.generation += 1;
        self.in_flight.clear();
        self.ready.clear();
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
                    || self.ready.iter().any(|r| r.key == k && r.level <= want);
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
        d: &Arc<TerrainData>,
        camera: Vec3,
        density: f32,
    ) {
        crate::profile_scope!("Terrain.Grass");
        self.stats = GrassStats::default();
        self.density = density.clamp(MIN_KEEP, 1.0);
        let key = scatter_key(d);
        if self.key.as_ref() != Some(&key) {
            self.chunks.clear();
            self.restart();
            self.key = Some(key);
        }
        if !d.details.enabled || d.detail_map.is_empty() {
            self.chunks.clear();
            self.restart();
            return;
        }
        let range = d.details.draw_distance.max(1.0);
        let lod = d.details.lod_distance;
        // Far chunks go; chunks coarser than wanted stay until their finer scatter lands.
        self.chunks
            .retain(|(x, z), _| near(*x, *z, camera, range * 1.5));
        self.in_flight
            .retain(|(x, z), _| near(*x, *z, camera, range * 1.5));

        // Finished scatters: keep the ones still worth uploading, nearest first.
        self.collect(camera, range);
        self.upload(device, grass_streaming());

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
            self.upload(device, false);
            if !self.stale_chunks(d, camera).is_empty() {
                let stats = self.stats;
                self.update(device, d, camera, density);
                self.stats.scheduled += stats.scheduled;
                self.stats.uploaded += stats.uploaded;
                self.stats.upload_ms += stats.upload_ms;
            }
        }
        self.stats.pending = (self.in_flight.len() + self.ready.len()) as u32;
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

    /// Upload finished chunks: within [`UPLOAD_BUDGET`] (always at least one) when
    /// `budgeted`, else all of them.
    fn upload(&mut self, device: &wgpu::Device, budgeted: bool) {
        let started = Instant::now();
        while let Some(done) = self.ready.pop_front() {
            self.stats.scatter_ms = self.stats.scatter_ms.max(done.scatter_ms);
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
            let buffer = (!blades.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("misty grass blades"),
                    contents: bytemuck::cast_slice(blades),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            });
            let bounds = Aabb::new(
                Vec3::new(lo.x - 1.0, bottom - 1.0, lo.y - 1.0),
                Vec3::new(lo.x + CHUNK + 1.0, top + 1.0, lo.y + CHUNK + 1.0),
            );
            self.chunks.insert(
                done.key,
                Chunk {
                    buffer,
                    count: blades.len() as u32,
                    level: done.level,
                    bounds,
                },
            );
            self.stats.uploaded += 1;
            let elapsed = started.elapsed();
            if budgeted && elapsed >= UPLOAD_BUDGET {
                break;
            }
            // The next chunk, if any, starts now: under the budget by construction.
            self.stats.upload_last_start_ms = elapsed.as_secs_f32() * 1000.0;
        }
        self.stats.upload_ms += started.elapsed().as_secs_f32() * 1000.0;
    }

    /// Draw visible chunks (pipeline and groups 0–2 already bound), nearest first: far
    /// chunks draw only the share of their key-sorted blades their nearest point keeps (the
    /// vertex shader shrinks away the rest blade by blade). `limit` draws only the chunks
    /// within a range, a share of their blades (shadow casters, the prepass).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass,
        origin: Vec3,
        camera: Vec3,
        lod_distance: f32,
        frustum: &Frustum,
        limit: Option<Limit>,
        stats: &mut super::RenderStats,
    ) {
        // Nearest chunks first: their blades fill the depth buffer, so the many blades
        // behind them fail the depth test before shading.
        let mut order: Vec<(f32, &Chunk)> = self
            .chunks
            .values()
            .filter_map(|c| {
                c.buffer.as_ref()?;
                let world = Aabb::new(c.bounds.min + origin, c.bounds.max + origin);
                let dist = camera.clamp(world.min, world.max).distance(camera);
                let near = limit.is_none_or(|l| dist < l.range);
                (near && frustum.intersects(&world)).then_some((dist, c))
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let share = limit.map_or(1.0, |l| l.share);
        let coarse_only = limit.is_some_and(|l| l.coarse);
        for (dist, c) in order {
            let Some(buffer) = &c.buffer else {
                continue;
            };
            let keep =
                keep_at(dist, lod_distance) * self.density * share / details::level_keep(c.level);
            let count = ((c.count as f32 * keep.min(1.0)).ceil() as u32).min(c.count);
            if count == 0 {
                continue;
            }
            pass.set_vertex_buffer(0, buffer.slice(..));
            let verts = if coarse_only || dist > lod_distance * COARSE_FROM {
                pass.draw(100..100 + COARSE_VERTICES, 0..count);
                COARSE_VERTICES
            } else {
                pass.draw(0..BLADE_VERTICES, 0..count);
                BLADE_VERTICES
            };
            stats.draw(count, (verts - 2) * 3, verts);
        }
    }

    pub fn bytes(&self) -> u64 {
        self.chunks
            .values()
            .map(|c| c.count as u64 * std::mem::size_of::<details::Blade>() as u64)
            .sum()
    }
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
    /// x = the terrain's draw slot (its reflection probes), yzw unused.
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

/// Upload the grass atlas with coverage-keeping mips (sRGB) and its clamped trilinear,
/// anisotropic sampler (cards are seen at grazing angles).
pub(super) fn atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let image = image::load_from_memory(ATLAS_PNG)
        .expect("built-in grass atlas")
        .into_rgba8();
    let (w, h) = image.dimensions();
    let chain = atlas_mips(image);
    let data: Vec<u8> = chain
        .iter()
        .flat_map(|m| m.as_raw().iter().copied())
        .collect();
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
            mip_level_count: chain.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("misty grass sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 16,
        ..Default::default()
    });
    (view, sampler)
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
