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

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Vec3};

use super::{Frame, RenderTarget};

mod mesh_sdf;
mod scene;
mod settings;

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
        cache: None,
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

/// The renderer's part: pipelines and the atlases shared by every view.
#[derive(Default)]
pub(in crate::render) struct MistyGi {
    pipelines: Option<Pipelines>,
    meshes: Option<mesh_sdf::MeshSdfs>,
    terrains: Option<TerrainAtlas>,
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

/// A view's clipmaps, probes and buffers.
struct TargetState {
    sizes: Sizes,
    full: (u32, u32),
    gather_size: (u32, u32),
    sdf: Volume,
    emission: Volume,
    radiance: Volume,
    normals: Volume,
    gather: wgpu::TextureView,
    /// Glossy reflections at the gather texels (rgb, a = confidence).
    reflect: wgpu::TextureView,
    probes: wgpu::Buffer,
    params: wgpu::Buffer,
    instances: wgpu::Buffer,
    lists: wgpu::Buffer,
    lights: wgpu::Buffer,
    /// What each level was last composed from (None = never).
    hashes: Vec<Option<u64>>,
    frame: u32,
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

impl TargetState {
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
        let probe_count = sizes.levels as u64 * (sizes.probes as u64).pow(3);
        Self {
            sizes,
            full,
            gather_size,
            sdf: Volume::new(device, "misty gi sdf", &sizes, VOLUME_FORMAT),
            emission: Volume::new(device, "misty gi emission", &sizes, VOLUME_FORMAT),
            radiance: Volume::new(device, "misty gi radiance", &sizes, VOLUME_FORMAT),
            normals: Volume::new(device, "misty gi cache normals", &sizes, NORMAL_FORMAT),
            gather: image("misty gi gather"),
            reflect: image("misty gi reflections"),
            probes: storage(device, "misty gi probes", probe_count * 64),
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty gi params"),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            instances: storage(device, "misty gi instances", 192 * 64),
            lists: storage(device, "misty gi lists", 4 * 1024),
            lights: storage(device, "misty gi lights", 96 * scene::MAX_LIGHTS as u64),
            hashes: vec![None; sizes.levels as usize],
            frame: 0,
        }
    }
}

/// Per-target slot (inside [`RenderTarget`]).
#[derive(Default)]
pub(crate) struct TargetSlot(Mutex<Option<TargetState>>);

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
        let pipelines = self.pipelines.get_or_insert_with(|| Pipelines::new(device));
        let meshes = self
            .meshes
            .get_or_insert_with(|| mesh_sdf::MeshSdfs::new(device));
        let terrains = self
            .terrains
            .get_or_insert_with(|| TerrainAtlas::new(device));
        let sizes = settings.sizes();
        let full = target.size();
        let mut slot = target.misty_gi.0.lock().ok()?;
        if slot
            .as_ref()
            .is_none_or(|s| s.sizes != sizes || s.full != full)
        {
            *slot = Some(TargetState::new(device, sizes, full));
        }
        let state = slot.as_mut()?;

        // The frame's shapes and lights.
        let list = scene::build(frame, &mut |m| meshes.brick(queue, m), &mut |t| {
            terrains.region(queue, t)
        });
        let camera = frame.camera.position;
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
            if state.hashes[k as usize] != Some(hash) {
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
        // A new view composes every level at once; later changes a few levels a frame.
        let fresh = state.hashes.iter().all(|h| h.is_none());
        let budget = if fresh {
            dirty.len()
        } else {
            RECOMPOSE_PER_FRAME
        };
        let recomposed = dirty.len().min(budget);
        for &(k, hash) in dirty.iter().take(budget) {
            levels[k].flags[0] = 1.0;
            state.hashes[k] = Some(hash);
        }

        // Buffers (grown to fit).
        let instance_bytes = std::mem::size_of_val(list.instances.as_slice()) as u64;
        if instance_bytes > state.instances.size() {
            state.instances = storage(
                device,
                "misty gi instances",
                instance_bytes.next_power_of_two(),
            );
        }
        if !list.instances.is_empty() {
            queue.write_buffer(&state.instances, 0, bytemuck::cast_slice(&list.instances));
        }
        let list_bytes = (lists.len() * 4) as u64;
        if list_bytes > state.lists.size() {
            state.lists = storage(device, "misty gi lists", list_bytes.next_power_of_two());
        }
        if !lists.is_empty() {
            queue.write_buffer(&state.lists, 0, bytemuck::cast_slice(&lists));
        }
        if !list.lights.is_empty() {
            queue.write_buffer(&state.lights, 0, bytemuck::cast_slice(&list.lights));
        }

        let env = frame.environment;
        let k = env.settings.environment_lighting.intensity.max(0.0);
        let [sky, equator, ground] = env.ambient();
        let rot = ray_rotation(state.frame);
        let row = |i: usize| Vec3::new(rot.x_axis[i], rot.y_axis[i], rot.z_axis[i]);
        let view_proj = frame.camera.view_projection(target.aspect());
        let params = Params {
            info: [
                sizes.levels as f32,
                sizes.dim as f32,
                sizes.probes as f32,
                (state.frame % 65536) as f32,
            ],
            info2: [
                sizes.rays as f32,
                sizes.stride as f32,
                (state.frame % sizes.stride) as f32,
                HYSTERESIS,
            ],
            info3: [
                list.lights.len() as f32,
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
                state.gather_size.0 as f32,
                state.gather_size.1 as f32,
            ],
            levels,
        };
        queue.write_buffer(&state.params, 0, bytemuck::bytes_of(&params));

        let sampler = wgpu::BindingResource::Sampler(&pipelines.sampler);
        let group = |p: &wgpu::ComputePipeline, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty gi group"),
                layout: &p.get_bind_group_layout(0),
                entries,
            })
        };
        let volume_groups = [
            state.sizes.dim.div_ceil(4),
            state.sizes.dim.div_ceil(4),
            (state.sizes.dim * state.sizes.levels).div_ceil(4),
        ];
        let mut steps = Vec::new();
        if recomposed > 0 {
            steps.push(Step {
                stage: Stage::Sdf,
                group: group(
                    &pipelines.sdf,
                    &[
                        entry(0, buf(&state.params)),
                        entry(2, res(&state.sdf.view)),
                        entry(4, res(&state.emission.view)),
                        entry(9, sampler.clone()),
                        entry(10, buf(&state.instances)),
                        entry(11, buf(&state.lists)),
                        entry(14, res(&meshes.view)),
                        entry(15, res(&terrains.view)),
                    ],
                ),
                groups: volume_groups,
            });
        }
        steps.push(Step {
            stage: Stage::Surface,
            group: group(
                &pipelines.surface,
                &[
                    entry(0, buf(&state.params)),
                    entry(1, res(&state.sdf.view)),
                    entry(3, res(&state.emission.view)),
                    entry(6, res(&state.radiance.view)),
                    entry(8, res(&state.normals.view)),
                    entry(9, sampler.clone()),
                    entry(12, buf(&state.lights)),
                    entry(13, buf(&state.probes)),
                ],
            ),
            groups: volume_groups,
        });
        let probe_count = sizes.levels * sizes.probes.pow(3);
        steps.push(Step {
            stage: Stage::Probes,
            group: group(
                &pipelines.probes,
                &[
                    entry(0, buf(&state.params)),
                    entry(1, res(&state.sdf.view)),
                    entry(5, res(&state.radiance.view)),
                    entry(7, res(&state.normals.view)),
                    entry(9, sampler.clone()),
                    entry(13, buf(&state.probes)),
                ],
            ),
            groups: [probe_count / sizes.stride, 1, 1],
        });
        steps.push(Step {
            stage: Stage::Gather,
            group: group(
                &pipelines.apply,
                &[
                    entry(0, buf(&state.params)),
                    entry(1, res(&state.sdf.view)),
                    entry(5, res(&state.radiance.view)),
                    entry(7, res(&state.normals.view)),
                    entry(9, sampler),
                    entry(13, buf(&state.probes)),
                    entry(16, res(depth_view)),
                    entry(17, res(normal_view)),
                    entry(18, res(&state.gather)),
                    entry(19, res(&state.reflect)),
                ],
            ),
            groups: [
                state.gather_size.0.div_ceil(8),
                state.gather_size.1.div_ceil(8),
                1,
            ],
        });
        state.frame = state.frame.wrapping_add(1);
        let mut world_header = [[0.0; 4]; WORLD_HEADER_VEC4S];
        world_header[0] = [sizes.levels as f32, sizes.probes as f32, 0.0, 0.0];
        for (k, l) in levels.iter().take(sizes.levels as usize).enumerate() {
            world_header[1 + k] = l.probe;
        }
        Some(MistyFrame {
            steps,
            gather: state.gather.clone(),
            scale: sizes.apply_scale,
            reflect: state.reflect.clone(),
            reflections: sizes.reflections,
            world_header,
            probes: state.probes.clone(),
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
