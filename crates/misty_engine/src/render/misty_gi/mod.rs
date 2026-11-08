//! Misty GI: fully dynamic global illumination without baking (design: docs/pending/misty-gi.md).
//!
//! Per view, around the camera: a clipmap of global signed distance fields composed from the
//! frame's shapes (`scene`, `mesh_sdf`), a radiance cache in the band around their surfaces lit
//! by every light (with SDF soft shadows) plus last frame's indirect light, and a clipmap of
//! irradiance probes that trace the SDF and read the cache. Each render:
//!
//! 1. **Scene SDF**: recompose the levels whose box moved or whose shapes changed.
//! 2. **Surface cache**: relight the band voxels of every level.
//! 3. **Probe trace**: one probe in `stride` traces its rays, projects them onto SH L1 and
//!    blends them into its history.
//! 4. **Gather**: irradiance at every half- (or quarter-) resolution prepass pixel, which the
//!    screen-trace compose upsamples into the lit shaders' GI texel (with SSGI on top).
//!
//! Off (the default) nothing is allocated and nothing runs.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Vec3};

use super::{Frame, RenderTarget};

mod mesh_sdf;
mod scene;
mod settings;

pub use mesh_sdf::pending as mesh_sdf_bakes_pending;
#[cfg(test)]
pub(crate) use mesh_sdf::{
    LIBRARY_DIR as MESH_SDF_DIR, cache_reads as mesh_sdf_cache_reads,
    forget_all as forget_mesh_sdfs, test_serial as mesh_sdf_test_serial,
};
pub use settings::MistyGiSettings;
use settings::Sizes;

const COMMON_SRC: &str = include_str!("shaders/gi_common.wgsl");
const SDF_SRC: &str = include_str!("shaders/sdf.wgsl");
const SURFACE_SRC: &str = include_str!("shaders/surface.wgsl");
const PROBES_SRC: &str = include_str!("shaders/probes.wgsl");
const APPLY_SRC: &str = include_str!("shaders/apply.wgsl");

/// Clipmap levels the uniform has room for.
const MAX_LEVELS: usize = 6;
/// Share of a probe's history kept per update (with one update every `stride` frames).
const HYSTERESIS: f32 = 0.85;
/// Levels recomposed per frame at most (finest first; the others wait a frame).
const RECOMPOSE_PER_FRAME: usize = 2;
/// A level has settled when its shapes and placement, every coarser level's, the lights and the
/// sky have all stayed the same for this many updates (2 s at 60 FPS: the probes have taken
/// 32–64 updates since the last change and the bounces have converged). Its probes that have
/// had `SETTLED_UPDATES` (32) updates in their cell then trace one turn in [`SETTLED_RATE`]
/// (probes.wgsl), and with every level settled the surface cache relights one update in
/// `SETTLED_RATE / 2`. Any change brings the full rate back at once.
const SETTLE_FRAMES: u32 = 128;
/// See [`SETTLE_FRAMES`] (probes.wgsl has the same constants).
const SETTLED_RATE: u32 = 8;
/// Terrain height atlas side (texels) and the largest region one terrain gets.
const TERRAIN_ATLAS: u32 = 1024;
const TERRAIN_REGION: u32 = 512;
const VOLUME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Snorm;
const GATHER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LevelUniform {
    sdf: [f32; 4],
    probe: [f32; 4],
    flags: [f32; 4],
}

/// `GiParams` in gi_common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    info: [f32; 4],
    info2: [f32; 4],
    info3: [f32; 4],
    camera: [f32; 4],
    sky: [f32; 4],
    equator: [f32; 4],
    ground: [f32; 4],
    rot0: [f32; 4],
    rot1: [f32; 4],
    rot2: [f32; 4],
    inv_view_proj: [[f32; 4]; 4],
    size: [f32; 4],
    levels: [LevelUniform; MAX_LEVELS],
}

/// The four stages (each its own compute pass, timed separately).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::render) enum Stage {
    Sdf,
    Surface,
    Probes,
    Gather,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Sdf, Stage::Surface, Stage::Probes, Stage::Gather];

    /// Pass timer / Frame Debugger name.
    pub fn label(self) -> &'static str {
        match self {
            Stage::Sdf => "Misty GI: scene SDF",
            Stage::Surface => "Misty GI: surface cache",
            Stage::Probes => "Misty GI: probe trace",
            Stage::Gather => "Misty GI: gather",
        }
    }
}

struct Pipelines {
    sdf: wgpu::ComputePipeline,
    surface: wgpu::ComputePipeline,
    probes: wgpu::ComputePipeline,
    apply: wgpu::ComputePipeline,
    sampler: wgpu::Sampler,
}

/// A shader's source; `MISTY_GI_SHADERS=<dir>` reads it from that folder instead (tuning
/// without rebuilding the engine).
fn source(name: &str, built_in: &str) -> String {
    std::env::var_os("MISTY_GI_SHADERS")
        .and_then(|dir| std::fs::read_to_string(std::path::Path::new(&dir).join(name)).ok())
        .unwrap_or_else(|| built_in.to_owned())
}

fn pipeline(device: &wgpu::Device, label: &str, file: &str, entry: &str) -> wgpu::ComputePipeline {
    let src = match file {
        "sdf.wgsl" => SDF_SRC,
        "surface.wgsl" => SURFACE_SRC,
        "probes.wgsl" => PROBES_SRC,
        _ => APPLY_SRC,
    };
    let code = format!(
        "{}\n{}",
        source("gi_common.wgsl", COMMON_SRC),
        source(file, src)
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(code.into()),
    });
    // The layout comes from the bindings the entry point uses (each kernel a subset).
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: crate::render::pipeline_cache(device).as_ref(),
    })
}

impl Pipelines {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            sdf: pipeline(device, "misty gi sdf", "sdf.wgsl", "cs_sdf"),
            surface: pipeline(device, "misty gi surface", "surface.wgsl", "cs_surface"),
            probes: pipeline(device, "misty gi probes", "probes.wgsl", "cs_probes"),
            apply: pipeline(device, "misty gi apply", "apply.wgsl", "cs_apply"),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty gi sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            }),
        }
    }
}

/// Terrain heights (normalised, R16Float) of every traced terrain.
struct TerrainAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// (store key, version, region).
    slots: Vec<(String, u64, scene::TerrainRegion)>,
}

impl TerrainAtlas {
    fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty gi terrain heights"),
            size: wgpu::Extent3d {
                width: TERRAIN_ATLAS,
                height: TERRAIN_ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {
            texture,
            view,
            slots: Vec::new(),
        }
    }

    /// The region of a terrain, (re)uploading its heights when they changed.
    fn region(
        &mut self,
        queue: &wgpu::Queue,
        t: &crate::terrain::TerrainDraw,
    ) -> Option<scene::TerrainRegion> {
        if let Some((_, version, r)) = self.slots.iter().find(|s| s.0 == t.key)
            && *version == t.version
        {
            return Some(*r);
        }
        let index = match self.slots.iter().position(|s| s.0 == t.key) {
            Some(i) => i,
            None if self.slots.len() < 4 => self.slots.len(),
            None => return None,
        };
        let d = &t.data;
        let res = d.heightmap_resolution.max(2) as u32;
        // Every n-th sample, so one terrain fits its quarter of the atlas.
        let step = res.div_ceil(TERRAIN_REGION).max(1);
        let cols = (res - 1) / step + 1;
        let rows = cols;
        let mut halves = Vec::with_capacity((cols * rows) as usize);
        for z in 0..rows {
            for x in 0..cols {
                let h = d.height((x * step) as usize, (z * step) as usize);
                halves.push(half::f16::from_f32(h).to_bits());
            }
        }
        let region = scene::TerrainRegion {
            x: (index as u32 % 2) * TERRAIN_REGION,
            y: (index as u32 / 2) * TERRAIN_REGION,
            columns: cols,
            rows,
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.x,
                    y: region.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&halves),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cols * 2),
                rows_per_image: Some(rows),
            },
            wgpu::Extent3d {
                width: cols,
                height: rows,
                depth_or_array_layers: 1,
            },
        );
        let slot = (t.key.clone(), t.version, region);
        if index < self.slots.len() {
            self.slots[index] = slot;
        } else {
            self.slots.push(slot);
        }
        Some(region)
    }
}

/// The renderer's part: pipelines, the atlases shared by every view, and the views' world
/// state (clipmaps and probes), which a secondary view near a primary one shares.
#[derive(Default)]
pub(in crate::render) struct MistyGi {
    pipelines: Option<Pipelines>,
    meshes: Option<mesh_sdf::MeshSdfs>,
    terrains: Option<TerrainAtlas>,
    worlds: Vec<World>,
}

/// A 3D texture with its levels stacked along z.
struct Volume {
    view: wgpu::TextureView,
}

impl Volume {
    fn new(device: &wgpu::Device, label: &str, s: &Sizes, format: wgpu::TextureFormat) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: s.dim,
                height: s.dim,
                depth_or_array_layers: s.dim * s.levels,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&Default::default()),
        }
    }
}

/// A secondary view shares a primary view's world updated this recently...
const SHARE_WITHIN: Duration = Duration::from_millis(250);
/// ...and worlds no view has updated for this long are freed.
const WORLD_KEEP: Duration = Duration::from_secs(3);

/// Clipmaps, probes and buffers around one view's camera.
struct World {
    sizes: Sizes,
    sdf: Volume,
    emission: Volume,
    radiance: Volume,
    normals: Volume,
    probes: wgpu::Buffer,
    params: wgpu::Buffer,
    instances: wgpu::Buffer,
    lists: wgpu::Buffer,
    lights: wgpu::Buffer,
    /// What each level was last composed from (None = never).
    hashes: Vec<Option<u64>>,
    /// Updates since each level's shapes or placement last changed, and since the lights or
    /// the sky last changed (with their hash): see [`SETTLE_FRAMES`].
    level_stable: [u32; MAX_LEVELS],
    light_hash: u64,
    light_stable: u32,
    frame: u32,
    /// The view whose camera places it ([`TargetSlot`] id), whether that view is secondary,
    /// and when and where it last updated it.
    owner: u64,
    owner_secondary: bool,
    updated: Instant,
    camera: Vec3,
    /// Level placement and light count of the last update (what sharing views gather with).
    levels: [LevelUniform; MAX_LEVELS],
    light_count: u32,
}

fn storage(device: &wgpu::Device, label: &str, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.max(256).next_multiple_of(256),
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty gi params"),
        size: std::mem::size_of::<Params>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl World {
    fn new(device: &wgpu::Device, sizes: Sizes, owner: u64, owner_secondary: bool) -> Self {
        let probe_count = sizes.levels as u64 * (sizes.probes as u64).pow(3);
        Self {
            sizes,
            sdf: Volume::new(device, "misty gi sdf", &sizes, VOLUME_FORMAT),
            emission: Volume::new(device, "misty gi emission", &sizes, VOLUME_FORMAT),
            radiance: Volume::new(device, "misty gi radiance", &sizes, VOLUME_FORMAT),
            normals: Volume::new(device, "misty gi cache normals", &sizes, NORMAL_FORMAT),
            probes: storage(device, "misty gi probes", probe_count * 64),
            params: params_buffer(device),
            instances: storage(device, "misty gi instances", 192 * 64),
            lists: storage(device, "misty gi lists", 4 * 1024),
            lights: storage(device, "misty gi lights", 96 * scene::MAX_LIGHTS as u64),
            hashes: vec![None; sizes.levels as usize],
            level_stable: [0; MAX_LEVELS],
            light_hash: 0,
            light_stable: 0,
            frame: 0,
            owner,
            owner_secondary,
            updated: Instant::now(),
            camera: Vec3::ZERO,
            levels: [LevelUniform::zeroed(); MAX_LEVELS],
            light_count: 0,
        }
    }

    /// Can a secondary view at `camera` gather from this world instead of keeping its own?
    /// A primary view updated it just now, and the camera is inside the inner half of its
    /// third-coarsest level, so the pixels near it still get fine levels (0.25 m at High).
    fn shareable(&self, id: u64, sizes: &Sizes, camera: Vec3, now: Instant) -> bool {
        self.owner != id
            && !self.owner_secondary
            && self.sizes == *sizes
            && now.duration_since(self.updated) < SHARE_WITHIN
            && self.camera.distance(camera) <= share_radius(sizes)
    }
}

/// How far a secondary view's camera may be from the primary's to share its world.
fn share_radius(s: &Sizes) -> f32 {
    let k = s.levels.saturating_sub(2);
    0.25 * s.dim as f32 * s.voxel * (1u32 << k) as f32
}

/// A view's own part: the gather images at its size and its parameters.
struct ViewState {
    sizes: Sizes,
    full: (u32, u32),
    gather_size: (u32, u32),
    gather: wgpu::TextureView,
    /// Glossy reflections at the gather texels (rgb, a = confidence).
    reflect: wgpu::TextureView,
    params: wgpu::Buffer,
}

impl ViewState {
    fn new(device: &wgpu::Device, sizes: Sizes, full: (u32, u32)) -> Self {
        let gather_size = (
            full.0.div_ceil(sizes.apply_scale),
            full.1.div_ceil(sizes.apply_scale),
        );
        let image = |label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: gather_size.0,
                        height: gather_size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: GATHER_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::STORAGE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self {
            sizes,
            full,
            gather_size,
            gather: image("misty gi gather"),
            reflect: image("misty gi reflections"),
            params: params_buffer(device),
        }
    }
}

/// Per-target slot (inside [`RenderTarget`]): the view's identity, whether it is secondary
/// (the editor's Scene view), and its gather images.
pub(crate) struct TargetSlot {
    id: u64,
    secondary: AtomicBool,
    view: Mutex<Option<ViewState>>,
}

impl Default for TargetSlot {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            secondary: AtomicBool::new(false),
            view: Mutex::new(None),
        }
    }
}

impl TargetSlot {
    /// A secondary view (the editor's Scene view) gathers from a primary view's clipmaps
    /// when its camera is near enough, instead of updating clipmaps of its own.
    pub fn set_secondary(&self, on: bool) {
        self.secondary.store(on, Ordering::Relaxed);
    }
}

/// One stage's work this render.
struct Step {
    stage: Stage,
    group: wgpu::BindGroup,
    groups: [u32; 3],
}

/// This render's Misty GI work and its result.
pub(in crate::render) struct MistyFrame {
    steps: Vec<Step>,
    /// Gather image (rgb = irradiance) at `scale` full pixels per texel.
    pub gather: wgpu::TextureView,
    pub scale: u32,
    /// Glossy reflections at the same texels (rgb, a = confidence) when `reflections`.
    pub reflect: wgpu::TextureView,
    pub reflections: bool,
    /// The probe clipmap the lit shaders' world lookup reads (`misty_world_irradiance` in
    /// screen_trace.wgsl): `world_header` then a copy of `probes` (`probe_bytes`).
    pub world_header: [[f32; 4]; WORLD_HEADER_VEC4S],
    pub probes: wgpu::Buffer,
    pub probe_bytes: u64,
}

/// Vec4s before the probes in the lit shaders' copy: (levels, probes per side), then one per
/// level (first probe cell, spacing), padded to `MAX_LEVELS`.
pub(in crate::render) const WORLD_HEADER_VEC4S: usize = 2 + MAX_LEVELS;

impl MistyFrame {
    /// The stage runs this render.
    pub fn has(&self, stage: Stage) -> bool {
        self.steps.iter().any(|s| s.stage == stage)
    }
}

/// What the surface cache and the probes' sky see of the lights and the environment.
fn lighting_hash(lights: &[scene::GiLight], sky: [Vec3; 3]) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write(bytemuck::cast_slice(lights));
    for c in sky {
        h.write(bytemuck::bytes_of(&c.to_array()));
    }
    h.finish()
}

/// Where level `k` is centred: a quarter of its width ahead of the camera, so the camera sits
/// in its back half and what it looks at gets the finer levels (a camera outside a room
/// looking in would otherwise see the room only in coarse levels).
fn level_focus(s: &Sizes, k: u32, camera: Vec3, forward: Vec3) -> Vec3 {
    let width = s.dim as f32 * s.voxel * (1u32 << k) as f32;
    camera + forward * (0.25 * width)
}

/// World box (min corner) and voxel size of level `k` centred near `focus`.
fn level_box(s: &Sizes, k: u32, focus: Vec3) -> (Vec3, f32) {
    let v = s.voxel * (1u32 << k) as f32;
    let snap = 8.0 * v;
    let center = (focus / snap).floor() * snap;
    (center - Vec3::splat(s.dim as f32 * 0.5 * v), v)
}

/// First probe cell and spacing of level `k` centred near `focus`.
fn probe_grid(s: &Sizes, k: u32, focus: Vec3) -> (Vec3, f32) {
    let v = s.voxel * (1u32 << k) as f32;
    let spacing = s.dim as f32 * v / s.probes as f32;
    let first = (focus / spacing).floor() - Vec3::splat((s.probes / 2) as f32);
    (first, spacing)
}

/// Rotation of frame `n`'s probe rays (a low-discrepancy walk over axes and angles).
fn ray_rotation(n: u32) -> Mat3 {
    let f = n as f32;
    let axis = Vec3::new(
        (f * 0.754_877_7).fract() * 2.0 - 1.0,
        (f * 0.569_840_3).fract() * 2.0 - 1.0,
        (f * 0.367_879_4 + 0.5).fract() * 2.0 - 1.0,
    )
    .normalize_or(Vec3::Y);
    Mat3::from_axis_angle(axis, (f * 0.618_034).fract() * std::f32::consts::TAU)
}

fn res(r: &wgpu::TextureView) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::TextureView(r)
}

fn buf(b: &wgpu::Buffer) -> wgpu::BindingResource<'_> {
    b.as_entire_binding()
}

fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

impl MistyGi {
    /// Set up this render's Misty GI (`None` when off). `normal_view` / `depth_view` = the
    /// prepass targets of this view.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
        normal_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) -> Option<MistyFrame> {
        let settings = frame.post.misty_gi;
        if !settings.active() {
            return None;
        }
        let MistyGi {
            pipelines,
            meshes,
            terrains,
            worlds,
        } = self;
        let pipelines = pipelines.get_or_insert_with(|| Pipelines::new(device));
        let meshes = meshes.get_or_insert_with(|| mesh_sdf::MeshSdfs::new(device));
        let terrains = terrains.get_or_insert_with(|| TerrainAtlas::new(device));
        let sizes = settings.sizes();
        let full = target.size();
        let slot = &target.misty_gi;
        let mut view_slot = slot.view.lock().ok()?;
        if view_slot
            .as_ref()
            .is_none_or(|s| s.sizes != sizes || s.full != full)
        {
            *view_slot = Some(ViewState::new(device, sizes, full));
        }
        let view = view_slot.as_mut()?;

        // The world this view gathers from: a primary view's when this one is secondary and
        // near it (nothing to update), else its own (updated around its camera).
        let now = Instant::now();
        worlds.retain(|w| now.duration_since(w.updated) < WORLD_KEEP);
        let camera = frame.camera.position;
        let secondary = slot.secondary.load(Ordering::Relaxed);
        let shared = secondary
            .then(|| {
                worlds
                    .iter()
                    .position(|w| w.shareable(slot.id, &sizes, camera, now))
            })
            .flatten();
        let index = match shared {
            Some(i) => i,
            None => match worlds.iter().position(|w| w.owner == slot.id) {
                Some(i) if worlds[i].sizes == sizes && worlds[i].owner_secondary == secondary => i,
                Some(i) => {
                    worlds[i] = World::new(device, sizes, slot.id, secondary);
                    i
                }
                None => {
                    worlds.push(World::new(device, sizes, slot.id, secondary));
                    worlds.len() - 1
                }
            },
        };
        let world = &mut worlds[index];
        let update = shared.is_none();

        let sampler = wgpu::BindingResource::Sampler(&pipelines.sampler);
        let group = |p: &wgpu::ComputePipeline, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty gi group"),
                layout: &p.get_bind_group_layout(0),
                entries,
            })
        };
        let env = frame.environment;
        let k = env.settings.environment_lighting.intensity.max(0.0);
        let [sky, equator, ground] = env.ambient();
        let view_proj = frame.camera.view_projection(target.aspect());
        // `frame_n` = the world's update count (ray rotation, stride phase).
        let params = |levels: [LevelUniform; MAX_LEVELS], frame_n: u32, lights: u32| {
            let rot = ray_rotation(frame_n);
            let row = |i: usize| Vec3::new(rot.x_axis[i], rot.y_axis[i], rot.z_axis[i]);
            Params {
                info: [
                    sizes.levels as f32,
                    sizes.dim as f32,
                    sizes.probes as f32,
                    (frame_n % 65536) as f32,
                ],
                info2: [
                    sizes.rays as f32,
                    sizes.stride as f32,
                    (frame_n % sizes.stride) as f32,
                    HYSTERESIS,
                ],
                info3: [
                    lights as f32,
                    sizes.visibility_steps as f32,
                    sizes.apply_scale as f32,
                    0.0,
                ],
                camera: camera
                    .extend(if sizes.reflections { 1.0 } else { 0.0 })
                    .to_array(),
                sky: (sky * k).extend(0.0).to_array(),
                equator: (equator * k).extend(0.0).to_array(),
                ground: (ground * k).extend(0.0).to_array(),
                rot0: row(0).extend(0.0).to_array(),
                rot1: row(1).extend(0.0).to_array(),
                rot2: row(2).extend(0.0).to_array(),
                inv_view_proj: view_proj.inverse().to_cols_array_2d(),
                size: [
                    full.0 as f32,
                    full.1 as f32,
                    view.gather_size.0 as f32,
                    view.gather_size.1 as f32,
                ],
                levels,
            }
        };
        let probe_count = sizes.levels * sizes.probes.pow(3);
        let mut steps = Vec::new();
        if update {
            // The frame's shapes and lights.
            let list = scene::build(frame, &mut |m| meshes.brick(queue, m), &mut |t| {
                terrains.region(queue, t)
            });
            let mut lists: Vec<u32> = Vec::new();
            let mut levels = [LevelUniform::zeroed(); MAX_LEVELS];
            let mut dirty = Vec::new();
            for k in 0..sizes.levels {
                let focus = level_focus(&sizes, k, camera, frame.camera.forward());
                let (origin, v) = level_box(&sizes, k, focus);
                let margin = Vec3::splat(8.0 * v);
                let hi = origin + Vec3::splat(sizes.dim as f32 * v);
                let indices = scene::cull(&list.instances, origin - margin, hi + margin);
                let hash = scene::level_hash(&list.instances, &indices, origin, 0);
                if world.hashes[k as usize] != Some(hash) {
                    dirty.push((k as usize, hash));
                }
                let (first, spacing) = probe_grid(&sizes, k, focus);
                levels[k as usize] = LevelUniform {
                    sdf: origin.extend(v).to_array(),
                    probe: first.extend(spacing).to_array(),
                    flags: [0.0, lists.len() as f32, indices.len() as f32, 0.0],
                };
                lists.extend(indices);
            }
            // A new world composes every level at once; later changes a few levels a frame.
            let fresh = world.hashes.iter().all(|h| h.is_none());
            let budget = if fresh {
                dirty.len()
            } else {
                RECOMPOSE_PER_FRAME
            };
            let recomposed = dirty.len().min(budget);
            for &(k, hash) in dirty.iter().take(budget) {
                levels[k].flags[0] = 1.0;
                world.hashes[k] = Some(hash);
            }
            // Settled levels (see `SETTLE_FRAMES`): flags.w = 1.
            let light_hash = lighting_hash(&list.lights, [sky * k, equator * k, ground * k]);
            world.light_stable = match light_hash == world.light_hash {
                true => world.light_stable.saturating_add(1),
                false => 0,
            };
            world.light_hash = light_hash;
            for k in 0..sizes.levels as usize {
                world.level_stable[k] = match dirty.iter().any(|&(d, _)| d == k) {
                    true => 0,
                    false => world.level_stable[k].saturating_add(1),
                };
            }
            let mut settled = super::fast_paths::gi_settle() && world.light_stable >= SETTLE_FRAMES;
            for k in (0..sizes.levels as usize).rev() {
                settled &= world.level_stable[k] >= SETTLE_FRAMES;
                levels[k].flags[3] = if settled { 1.0 } else { 0.0 };
            }
            // `settled` is now level 0's: every level has settled. The probe dispatch then holds
            // only the probes whose refresh turn it is (flags.w = 2, probes.wgsl).
            let relight = !settled || world.frame % (SETTLED_RATE / 2) == 0;
            let probe_groups = match settled {
                true => {
                    for l in levels.iter_mut().take(sizes.levels as usize) {
                        l.flags[3] = 2.0;
                    }
                    (probe_count / sizes.stride).div_ceil(SETTLED_RATE)
                }
                false => probe_count / sizes.stride,
            };

            // Buffers (grown to fit).
            let instance_bytes = std::mem::size_of_val(list.instances.as_slice()) as u64;
            if instance_bytes > world.instances.size() {
                world.instances = storage(
                    device,
                    "misty gi instances",
                    instance_bytes.next_power_of_two(),
                );
            }
            if !list.instances.is_empty() {
                queue.write_buffer(&world.instances, 0, bytemuck::cast_slice(&list.instances));
            }
            let list_bytes = (lists.len() * 4) as u64;
            if list_bytes > world.lists.size() {
                world.lists = storage(device, "misty gi lists", list_bytes.next_power_of_two());
            }
            if !lists.is_empty() {
                queue.write_buffer(&world.lists, 0, bytemuck::cast_slice(&lists));
            }
            if !list.lights.is_empty() {
                queue.write_buffer(&world.lights, 0, bytemuck::cast_slice(&list.lights));
            }
            let light_count = list.lights.len() as u32;
            queue.write_buffer(
                &world.params,
                0,
                bytemuck::bytes_of(&params(levels, world.frame, light_count)),
            );
            world.levels = levels;
            world.light_count = light_count;
            world.camera = camera;
            world.updated = now;

            let volume_groups = [
                sizes.dim.div_ceil(4),
                sizes.dim.div_ceil(4),
                (sizes.dim * sizes.levels).div_ceil(4),
            ];
            if recomposed > 0 {
                steps.push(Step {
                    stage: Stage::Sdf,
                    group: group(
                        &pipelines.sdf,
                        &[
                            entry(0, buf(&world.params)),
                            entry(2, res(&world.sdf.view)),
                            entry(4, res(&world.emission.view)),
                            entry(9, sampler.clone()),
                            entry(10, buf(&world.instances)),
                            entry(11, buf(&world.lists)),
                            entry(14, res(&meshes.view)),
                            entry(15, res(&terrains.view)),
                        ],
                    ),
                    groups: volume_groups,
                });
            }
            if relight {
                steps.push(Step {
                    stage: Stage::Surface,
                    group: group(
                        &pipelines.surface,
                        &[
                            entry(0, buf(&world.params)),
                            entry(1, res(&world.sdf.view)),
                            entry(3, res(&world.emission.view)),
                            entry(6, res(&world.radiance.view)),
                            entry(8, res(&world.normals.view)),
                            entry(9, sampler.clone()),
                            entry(12, buf(&world.lights)),
                            entry(13, buf(&world.probes)),
                        ],
                    ),
                    groups: volume_groups,
                });
            }
            steps.push(Step {
                stage: Stage::Probes,
                group: group(
                    &pipelines.probes,
                    &[
                        entry(0, buf(&world.params)),
                        entry(1, res(&world.sdf.view)),
                        entry(5, res(&world.radiance.view)),
                        entry(7, res(&world.normals.view)),
                        entry(9, sampler.clone()),
                        entry(13, buf(&world.probes)),
                    ],
                ),
                groups: [probe_groups, 1, 1],
            });
            world.frame = world.frame.wrapping_add(1);
        }
        // The view's gather, from the world's placement as last updated.
        queue.write_buffer(
            &view.params,
            0,
            bytemuck::bytes_of(&params(world.levels, world.frame, world.light_count)),
        );
        steps.push(Step {
            stage: Stage::Gather,
            group: group(
                &pipelines.apply,
                &[
                    entry(0, buf(&view.params)),
                    entry(1, res(&world.sdf.view)),
                    entry(5, res(&world.radiance.view)),
                    entry(7, res(&world.normals.view)),
                    entry(9, sampler),
                    entry(13, buf(&world.probes)),
                    entry(16, res(depth_view)),
                    entry(17, res(normal_view)),
                    entry(18, res(&view.gather)),
                    entry(19, res(&view.reflect)),
                ],
            ),
            groups: [
                view.gather_size.0.div_ceil(8),
                view.gather_size.1.div_ceil(8),
                1,
            ],
        });
        let mut world_header = [[0.0; 4]; WORLD_HEADER_VEC4S];
        world_header[0] = [sizes.levels as f32, sizes.probes as f32, 0.0, 0.0];
        for (k, l) in world.levels.iter().take(sizes.levels as usize).enumerate() {
            world_header[1 + k] = l.probe;
        }
        Some(MistyFrame {
            steps,
            gather: view.gather.clone(),
            scale: sizes.apply_scale,
            reflect: view.reflect.clone(),
            reflections: sizes.reflections,
            world_header,
            probes: world.probes.clone(),
            probe_bytes: probe_count as u64 * 64,
        })
    }

    /// Record one stage (its own compute pass).
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, f: &MistyFrame, stage: Stage) {
        let Some(p) = self.pipelines.as_ref() else {
            return;
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(stage.label()),
            timestamp_writes: None,
        });
        for step in f.steps.iter().filter(|s| s.stage == stage) {
            let pipeline = match step.stage {
                Stage::Sdf => &p.sdf,
                Stage::Surface => &p.surface,
                Stage::Probes => &p.probes,
                Stage::Gather => &p.apply,
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &step.group, &[]);
            let [x, y, z] = step.groups;
            pass.dispatch_workgroups(x, y, z);
        }
    }
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod scene_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::naga;

    fn validate(label: &str, src: &str) {
        let src = format!("{COMMON_SRC}\n{src}");
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(&src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{label}: {e:?}"));
    }

    #[test]
    fn shaders_validate() {
        validate("sdf", SDF_SRC);
        validate("surface", SURFACE_SRC);
        validate("probes", PROBES_SRC);
        validate("apply", APPLY_SRC);
        assert_eq!(
            std::mem::size_of::<Params>(),
            10 * 16 + 64 + 16 + 48 * MAX_LEVELS
        );
    }

    #[test]
    fn levels_nest_and_follow_the_camera() {
        let s = MistyGiSettings {
            enabled: true,
            ..Default::default()
        }
        .sizes();
        let cam = Vec3::new(3.3, 1.7, -8.2);
        let forward = Vec3::new(0.6, -0.48, 0.64);
        for k in 0..s.levels {
            let focus = level_focus(&s, k, cam, forward);
            let (lo, v) = level_box(&s, k, focus);
            let hi = lo + Vec3::splat(s.dim as f32 * v);
            assert!(cam.cmpgt(lo + 8.0 * v).all() && cam.cmplt(hi - 8.0 * v).all());
            let (first, spacing) = probe_grid(&s, k, focus);
            let plo = first * spacing;
            let phi = plo + Vec3::splat(s.probes as f32 * spacing);
            assert!(cam.cmpgt(plo).all() && cam.cmplt(phi).all());
        }
        // Small moves keep the box (no recompose).
        let a = level_box(&s, 2, cam).0;
        let b = level_box(&s, 2, cam + Vec3::splat(0.05)).0;
        assert_eq!(a, b);
    }
}
