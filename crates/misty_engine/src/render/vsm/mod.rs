//! Virtual shadow maps for the sun (Quality ▸ Shadows ▸ Virtual). The technique of the published
//! virtual shadow map / clipmapped page table talks, written from their descriptions:
//!
//! - **Clipmap** ([`clipmap`]): levels of 8192 × 8192 texels around the camera, level 0 with a
//!   texel of 1/1024 m (× Shadow Resolution's texels per pixel), each next level twice as
//!   coarse, up to a coarse "safety" level that covers the whole shadow distance. 128 × 128
//!   texel pages on a grid fixed in light space.
//! - **Page marking** (vsm_mark.wgsl): after the main pass a compute pass reads the depth
//!   buffer (grass blades included: their depth pass writes it), and for every pixel within
//!   the shadow distance marks the page of the level its footprint picks, plus the coarser
//!   levels the soft-shadow search and filter and the grass's taps sample, with neighbours
//!   near page borders. The bits are read back asynchronously (a frame or two later).
//! - **Page cache** ([`cache`]): wanted pages (marked within the last second, plus the safety
//!   level's disk) get a physical page in the pool; static pages stay cached across frames.
//!   Only new pages, pages under a caster that moved, appeared or vanished, and everything
//!   after the light turns are rendered. Least recently wanted pages are evicted when the pool
//!   is full.
//! - **Rendering** ([`rects`]): this frame's pages are grouped per level into rectangles,
//!   culled and drawn with the ordinary caster pipelines (meshes, terrain, trees, virtual
//!   geometry) into a 2048² scratch atlas, then copied page by page into the pool. Grass is
//!   not a cached caster (it sways): it keeps the near-camera grass shadow map.
//! - **Sampling** (shadows.wgsl `vsm_*`): the lit shaders pick the same level, look the page
//!   up in the page table (falling back to coarser levels while a page isn't rendered yet) and
//!   compare in the pool; the soft shadows run their blocker search and filter at the level
//!   whose texel suits the kernel, each tap looked up on its own so kernels cross page borders.

pub(super) mod cache;
pub(super) mod clipmap;
pub(super) mod fog;
mod passes;
pub(super) mod rects;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use bytemuck::{Pod, Zeroable};
use glam::{IVec2, Mat4, Vec3};

use cache::{PageCache, PageKey};
use clipmap::{Clipmap, MAX_LEVELS, PAGE, PAGES};
use rects::PageRect;

use super::SHADOW_FORMAT;
use super::culling::Aabb;
use super::shadows::VIEW_STRIDE;

/// Shared level / page math of the lit shaders and the marking pass.
pub(super) const COMMON_SRC: &str = include_str!("shaders/vsm_common.wgsl");
const MARK_SRC: &str = include_str!("shaders/vsm_mark.wgsl");
const COPY_SRC: &str = include_str!("shaders/vsm_copy.wgsl");

/// Pages on a side of the scratch atlas (2048² texels).
pub const SCRATCH_PAGES: i32 = rects::MAX_RECT;
/// Most rectangles drawn per frame (view uniform slots).
pub const MAX_RECTS: usize = 64;
/// Most scratch atlas fills per frame.
pub const MAX_ROUNDS: usize = 2;
/// Most pages rendered per frame (the rest wait for the next frames, sampled from coarser
/// levels meanwhile).
pub const PAGE_BUDGET: usize = 384;
/// Pages under swaying tree crowns are rendered again up to this texel size (metres): finer
/// levels than this show the leaves' sway, coarser ones keep a cached pose.
pub const SWAY_TEXEL: f32 = 0.04;
/// Most swaying pages rendered again per frame (round-robin, oldest first).
pub const SWAY_BUDGET: usize = 96;
/// The sun may turn this far (radians) from the light the pages were drawn for before a new
/// set starts filling in the background (shadows lag the sun by at most about this much).
pub const SUN_START: f32 = 0.1 * std::f32::consts::PI / 180.0;
/// A set filling in the background starts over when the sun turns this far from it.
pub const SUN_RESTART: f32 = 1.0 * std::f32::consts::PI / 180.0;
/// Further than this from the shown set, everything starts afresh at once.
pub const SUN_RESET: f32 = 5.0 * std::f32::consts::PI / 180.0;
/// Most pages of the background set rendered per frame.
pub const PENDING_BUDGET: usize = 128;
/// A marked page stays wanted this many frames.
const KEEP_FRAMES: u64 = 90;
/// Pages a pool row holds.
const POOL_ROW: u32 = 32;
/// Mark bits per level (one per page table slot).
const MARK_WORDS: usize = (PAGES * PAGES) as usize / 32;
/// Readback buffers in flight.
const READBACKS: usize = 3;
/// Table entry flag: rendered this frame (debug views only).
const FLAG_RENDERED: u32 = 1 << 16;

/// Request the neighbours of marked pages too (on by default; off for A/B measurements).
static GUARD_BAND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_guard_band(on: bool) {
    GUARD_BAND.store(on, Ordering::Relaxed);
}

fn guard_band() -> bool {
    GUARD_BAND.load(Ordering::Relaxed)
}

/// Scene view ▸ Virtual Shadow Map debug views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VsmDebug {
    #[default]
    Off,
    /// Each clipmap level in its own colour, page borders drawn.
    Levels,
    /// Cached pages green, pages rendered this frame red, pages still missing (sampled from a
    /// coarser level) magenta.
    Cache,
    /// Each physical page in its own colour (the page table's mapping).
    PageTable,
}

impl VsmDebug {
    fn index(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Levels => 1.0,
            Self::Cache => 2.0,
            Self::PageTable => 3.0,
        }
    }
}

/// `VsmParams` in vsm_common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct VsmUniform {
    /// xyz = light-space x axis (world), w = 1 when on.
    pub light_x: [f32; 4],
    /// xyz = light-space y axis, w = level count.
    pub light_y: [f32; 4],
    /// xyz = light direction (light-space z), w = 1 / depth range (m).
    pub light_z: [f32; 4],
    /// x = texel of level 0 (m), y = pixel footprint per metre of distance, z = the camera's
    /// depth (0..1), w = footprint floor (orthographic views, m).
    pub info: [f32; 4],
    /// x = debug view, y = PCSS search level, z = safety level, w = shadow distance.
    pub misc: [f32; 4],
    /// xy = 1 / pool size (texels), z = pages per pool row, w = 0.
    pub pool: [f32; 4],
    /// Per level: xy = window origin relative to the camera (`q` metres), z = 1 / texel,
    /// w = texel.
    pub levels: [[f32; 4]; MAX_LEVELS],
    /// Per level: xy = window origin page mod 64.
    pub wrap: [[f32; 4]; MAX_LEVELS],
}

/// Counters of the last frame (Stats, perf harness).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VsmStats {
    pub levels: u32,
    /// Pages requested by pixels / the safety level.
    pub wanted: u32,
    /// Pages holding a physical page.
    pub resident: u32,
    /// Pages rendered this frame (new + invalidated).
    pub rendered: u32,
    /// Of those, invalidated by moving casters or the terrain's level of detail.
    pub invalidated: u32,
    /// Rectangles drawn this frame.
    pub rects: u32,
    pub evicted: u32,
    /// Wanted pages without room in the pool.
    pub overflow: u32,
    /// Pages still waiting for the render budget.
    pub pending: u32,
    /// The whole cache was dropped (light turned, depth range changed).
    pub reset: bool,
    /// Pages under swaying tree crowns rendered again this frame.
    pub swaying: u32,
    /// Pages rendered for the set filling in the background (the sun turned).
    pub catching_up: u32,
    /// The background set took over this frame.
    pub switched: bool,
    /// Pages marked by the frame read back now that weren't rendered when it drew (it
    /// sampled them from a coarser level).
    pub late: u32,
}

/// One rectangle of pages drawn this frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct RectDraw {
    pub rect: PageRect,
    pub view_proj: Mat4,
    /// Scratch atlas fill and first page there.
    pub round: usize,
    pub at: IVec2,
    /// Tree level of detail for the level's texel.
    pub tree_lod: u8,
    /// Pixels per metre (virtual geometry's level of detail).
    pub texels_per_metre: f32,
}

/// This frame's virtual shadow map work.
pub(super) struct VsmFrame {
    pub uniform: VsmUniform,
    pub rects: Vec<RectDraw>,
    /// Scratch fills; per fill the instance range of its page copies.
    pub rounds: Vec<std::ops::Range<u32>>,
    /// The light the shown pages were drawn for (the sun may have turned a little since).
    pub light_dir: Vec3,
}

/// Inputs of [`Vsm::prepare`].
pub(super) struct VsmInput<'a> {
    pub camera: &'a crate::camera::Camera,
    pub sun_dir: Vec3,
    /// Tan of half the sun's angular size × softness (`light_params` x).
    pub sun_size: f32,
    pub target_height: u32,
    pub settings: &'a crate::quality::ShadowSettings,
    /// World bounds of every shadow caster (depth range).
    pub casters: &'a [(Vec3, Vec3)],
    /// World bounds of casters that moved, appeared or vanished since last frame.
    pub changed: &'a [Aabb],
    /// Something changed every page (terrain edited, …).
    pub reset: bool,
    pub frame: u64,
    pub debug: VsmDebug,
    /// Most rectangles this frame (virtual geometry has a few shadow views only).
    pub max_rects: usize,
    /// World bounds of tree crowns near the camera: their leaves sway, so the fine pages
    /// under them are rendered again round-robin.
    pub sway: &'a [Aabb],
}

/// Light direction, depth range, finest texel and level count a set of pages is drawn for;
/// `set` (0 or 1) tells its pages from the other set's while one replaces the other.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Basis {
    dir: Vec3,
    z: (f32, f32),
    finest: f32,
    levels: usize,
    set: u8,
}

/// A basis placed around this frame's camera.
struct Gen {
    basis: Basis,
    view: Mat4,
    map: Clipmap,
}

impl Gen {
    fn new(basis: Basis, camera: Vec3) -> Self {
        let view = clipmap::light_view(basis.dir);
        let map = Clipmap::new(&view, camera, basis.levels, basis.finest);
        Self { basis, view, map }
    }

    /// Per level, the pages whose columns meet the world box `b`.
    fn columns(&self, b: &Aabb) -> Vec<(IVec2, IVec2)> {
        let lb = b.transformed(&self.view);
        let (lo, hi) = (
            clipmap::q_of(Vec3::new(lb.min.x, lb.max.y, 0.0)),
            clipmap::q_of(Vec3::new(lb.max.x, lb.min.y, 0.0)),
        );
        (0..self.basis.levels)
            .map(|l| self.map.page_range(l, lo, hi))
            .collect()
    }
}

/// The clipmaps (and their set) one frame marked pages for: the shown set, then the one
/// filling in the background.
struct Marked {
    bases: Vec<(Clipmap, u8)>,
    frame: u64,
}

struct Readback {
    buffer: wgpu::Buffer,
    /// 0 free, 1 waiting for the map, 2 mapped.
    state: Arc<AtomicU8>,
    map: Option<Marked>,
}

/// GPU objects (made when the mode first turns on).
pub(super) struct VsmGpu {
    pub pool_view: wgpu::TextureView,
    pool_size: (u32, u32),
    pub table_view: wgpu::TextureView,
    table: wgpu::Texture,
    pub scratch_view: wgpu::TextureView,
    pub view_buffer: wgpu::Buffer,
    pub view_group: wgpu::BindGroup,
    pub copy: wgpu::RenderPipeline,
    copy_group: wgpu::BindGroup,
    copy_info: wgpu::Buffer,
    pub copy_instances: wgpu::Buffer,
    /// Marking from the main depth, and from a single-sampled depth.
    mark: [(wgpu::ComputePipeline, wgpu::BindGroupLayout); 2],
    pub(super) mark_params: wgpu::Buffer,
    marks: wgpu::Buffer,
    readbacks: Vec<Readback>,
    capacity: u32,
}

/// Dummy table and pool bound while the mode is off.
pub(super) struct VsmDummy {
    pub table_view: wgpu::TextureView,
    pub pool_view: wgpu::TextureView,
}

impl VsmDummy {
    pub fn new(device: &wgpu::Device) -> Self {
        let (_, table_view) = table_texture(device, 1);
        let pool = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty vsm pool (off)"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            table_view,
            pool_view: pool.create_view(&Default::default()),
        }
    }
}

fn table_texture(device: &wgpu::Device, side: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let layers = if side == 1 { 1 } else { MAX_LEVELS as u32 };
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("misty vsm page table"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let v = t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    (t, v)
}

impl VsmGpu {
    fn new(device: &wgpu::Device, p: &super::pipelines::Pipelines, pages: u32) -> Self {
        let rows = pages.div_ceil(POOL_ROW);
        let pool_size = (POOL_ROW * PAGE, rows * PAGE);
        let depth = |label, (w, h): (u32, u32)| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SHADOW_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let pool = depth("misty vsm pool", pool_size);
        let scratch_side = SCRATCH_PAGES as u32 * PAGE;
        let scratch = depth("misty vsm scratch", (scratch_side, scratch_side));
        let (table, table_view) = table_texture(device, PAGES as u32);
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vsm views"),
            size: VIEW_STRIDE * MAX_RECTS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vsm view group"),
            layout: &p.shadow_view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &view_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(VIEW_STRIDE),
                }),
            }],
        });
        use wgpu::ShaderStages as S;
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let depth_2d = |ms| wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: ms,
        };
        // Copy: scratch depth + pool size.
        let copy_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty vsm copy"),
            entries: &[
                entry(0, S::FRAGMENT, depth_2d(false)),
                entry(1, S::VERTEX, uniform),
            ],
        });
        let copy_info = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vsm copy info"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scratch_view = scratch.create_view(&Default::default());
        let copy_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vsm copy"),
            layout: &copy_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scratch_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: copy_info.as_entire_binding(),
                },
            ],
        });
        let instance_layout = [wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Uint32x4],
        }];
        use super::pipelines::{Depth, Spec, build};
        let copy = build(
            device,
            Spec {
                vs: "vs_vsm_copy",
                fs: Some("fs_vsm_copy"),
                buffers: &instance_layout,
                depth: Some(Depth {
                    format: SHADOW_FORMAT,
                    write: true,
                    compare: wgpu::CompareFunction::Always,
                    bias: Default::default(),
                }),
                color: None,
                glow: None,
                samples: 1,
                ..Spec::new("vsm copy", COPY_SRC, &[&p.empty_layout, &copy_layout])
            },
        );
        let copy_instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vsm copies"),
            size: 16 * (PAGE_BUDGET as u64 + MAX_RECTS as u64 * 4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Marking: a depth buffer (the main pass's multisampled one, or the screen-space
        // prepass's single-sampled one when it runs), parameters, mark bits.
        let make_mark = |ms: bool| {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("misty vsm mark"),
                entries: &[
                    entry(0, S::COMPUTE, depth_2d(ms)),
                    entry(1, S::COMPUTE, uniform),
                    entry(
                        2,
                        S::COMPUTE,
                        wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                    ),
                ],
            });
            let src = if ms {
                MARK_SRC.to_owned()
            } else {
                MARK_SRC.replace("texture_depth_multisampled_2d", "texture_depth_2d")
            };
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("misty vsm mark"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}
{COMMON_SRC}
{src}",
                        super::pipelines::COMMON
                    )
                    .into(),
                ),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("misty vsm mark"),
                bind_group_layouts: &[Some(&p.view_layout), Some(&layout)],
                immediate_size: 0,
            });
            super::pipelines::note_pipeline();
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("misty vsm mark"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("cs_vsm_mark"),
                compilation_options: Default::default(),
                cache: None,
            });
            (pipeline, layout)
        };
        let mark = [make_mark(super::sample_count() > 1), make_mark(false)];
        let mark_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vsm mark params"),
            // The shown set's parameters, then the background set's.
            size: 2 * std::mem::size_of::<VsmUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mark_bytes = (2 * MARK_WORDS * MAX_LEVELS * 4) as u64;
        let marks = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vsm marks"),
            size: mark_bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let readbacks = (0..READBACKS)
            .map(|_| Readback {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("misty vsm marks readback"),
                    size: mark_bytes,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(0)),
                map: None,
            })
            .collect();
        Self {
            pool_view: pool.create_view(&Default::default()),
            pool_size,
            table_view,
            table,
            scratch_view,
            view_buffer,
            view_group,
            copy,
            copy_group,
            copy_info,
            copy_instances,
            mark,
            mark_params,
            marks,
            readbacks,
            capacity: rows * POOL_ROW,
        }
    }

    /// Bytes on the GPU (Memory module).
    pub fn bytes(&self) -> u64 {
        let pool = self.pool_size.0 as u64 * self.pool_size.1 as u64 * 4;
        let scratch = (SCRATCH_PAGES as u64 * PAGE as u64).pow(2) * 4;
        let table = (PAGES * PAGES) as u64 * MAX_LEVELS as u64 * 4;
        pool + scratch + table
    }
}

/// Signature of a caster for change detection: mesh fingerprint, model matrix bits and
/// shadow variant.
fn caster_signature(print: u64, model: &Mat4, key: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    print.hash(&mut h);
    key.hash(&mut h);
    for v in model.to_cols_array() {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

/// The sun's virtual shadow map: page cache, feedback and GPU objects.
#[derive(Default)]
pub(super) struct Vsm {
    pub gpu: Option<VsmGpu>,
    /// Fills the fog's cascade layers from the virtual map.
    pub fog: Option<fog::FogResolve>,
    cache: Option<PageCache>,
    /// The light the shown pages were rendered for.
    basis: Option<Basis>,
    /// A set of pages filling in the background for the light the sun has turned to, and
    /// whether this frame's pixels have marked it yet.
    pending: Option<(Basis, bool)>,
    /// Page table as uploaded (level-major, 64 × 64 per level).
    table: Vec<u32>,
    /// Last frame's casters: signature → (world bounds, count).
    casters: HashMap<u64, (Aabb, u32)>,
    /// Last frame's terrain chunk detail per terrain (key, version, (lod, mask) per chunk).
    chunks: super::terrain::ChunkStates,
    pub frame: Option<VsmFrame>,
    /// This frame's clipmaps (for the readback of their marks).
    marking: Option<Marked>,
    pub stats: VsmStats,
    pub debug: VsmDebug,
}

impl Vsm {
    /// Make (or remake for another pool size) the GPU objects; true when they changed.
    pub fn ensure_gpu(
        &mut self,
        device: &wgpu::Device,
        p: &super::pipelines::Pipelines,
        pages: u32,
    ) -> bool {
        let pages = pages.max(POOL_ROW);
        let fits = self
            .gpu
            .as_ref()
            .is_some_and(|g| g.capacity == pages.div_ceil(POOL_ROW) * POOL_ROW);
        if fits {
            return false;
        }
        let gpu = VsmGpu::new(device, p, pages);
        self.fog = Some(fog::FogResolve::new(device, p));
        self.cache = Some(PageCache::new(gpu.capacity));
        self.basis = None;
        self.pending = None;
        self.table.clear();
        self.gpu = Some(gpu);
        true
    }

    /// Drop the GPU objects and the cache (mode turned off).
    pub fn release(&mut self) -> bool {
        let had = self.gpu.is_some();
        *self = Self {
            debug: self.debug,
            ..Default::default()
        };
        had
    }

    pub fn bytes(&self) -> u64 {
        self.gpu.as_ref().map_or(0, VsmGpu::bytes)
    }

    /// Casters that moved, appeared or vanished since last frame (world bounds), from this
    /// frame's `(signature, bounds, always dirty)` list.
    pub fn caster_changes(&mut self, now: impl Iterator<Item = (u64, Aabb, bool)>) -> Vec<Aabb> {
        let mut current: HashMap<u64, (Aabb, u32)> = HashMap::with_capacity(self.casters.len());
        let mut changed = Vec::new();
        for (sig, b, always) in now {
            if always {
                changed.push(b);
            }
            let e = current.entry(sig).or_insert((b, 0));
            e.1 += 1;
        }
        for (sig, (b, n)) in &current {
            if self.casters.get(sig).is_none_or(|(_, m)| m != n) {
                changed.push(*b);
            }
        }
        for (sig, (b, n)) in &self.casters {
            if current.get(sig).is_none_or(|(_, m)| m != n) {
                changed.push(*b);
            }
        }
        self.casters = current;
        changed
    }

    /// Terrain chunks whose level of detail changed since last frame (their world bounds),
    /// and whether a terrain was added, edited or moved (every page then renders again).
    pub fn terrain_changes(
        &mut self,
        now: super::terrain::ChunkStates,
        bounds: impl Fn(usize, usize) -> Option<Aabb>,
    ) -> (Vec<Aabb>, bool) {
        let mut changed = Vec::new();
        let mut reset = now.len() != self.chunks.len();
        for (i, (key, version, chunks)) in now.iter().enumerate() {
            let Some((k, v, old)) = self.chunks.get(i) else {
                reset = true;
                continue;
            };
            if k != key || v != version || old.len() != chunks.len() {
                reset = true;
                continue;
            }
            for (c, (a, b)) in chunks.iter().zip(old).enumerate() {
                if a != b
                    && let Some(bb) = bounds(i, c)
                {
                    changed.push(bb);
                }
            }
        }
        self.chunks = now;
        (changed, reset)
    }

    /// Plan this frame: read finished feedback, place the clipmap, update the cache, choose the
    /// pages to render and group them, upload the page table and the rectangles' views.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, input: &VsmInput) {
        self.frame = None;
        let (Some(gpu), Some(cache)) = (self.gpu.as_mut(), self.cache.as_mut()) else {
            return;
        };
        let s = input.settings;
        let finest = clipmap::FINEST_TEXEL * s.texels_per_pixel.max(0.125);
        let distance = s.distance.min(input.camera.far).max(1.0);
        let safety = clipmap::safety_level(distance, finest);
        let levels = safety + 1;
        let dir = input.sun_dir.normalize_or(Vec3::NEG_Y);
        let z_now = clipmap::depth_range(&clipmap::light_view(dir), input.casters)
            .unwrap_or((-1000.0, 1000.0));
        let mut stats = VsmStats {
            levels: levels as u32,
            ..Default::default()
        };
        // Which light the pages are drawn for (see `Basis`): the active basis while the sun
        // stays within SUN_START of it; past that a pending basis fills its own pages in the
        // background and takes over once they are all rendered; past SUN_RESET (or anything
        // else changing every page) everything starts afresh.
        let fresh = |set: u8| Basis {
            dir,
            z: z_now,
            finest,
            levels,
            set,
        };
        let angle = |a: Vec3| a.dot(dir).clamp(-1.0, 1.0).acos();
        let reset = match self.basis {
            None => true,
            Some(b) => {
                input.reset
                    || b.finest != finest
                    || b.levels != levels
                    || angle(b.dir) > SUN_RESET
                    || (angle(b.dir) <= SUN_START && b.z != z_now)
            }
        };
        if reset {
            cache.clear();
            stats.reset = true;
            self.basis = Some(fresh(0));
            self.pending = None;
        }
        let active = self.basis.expect("set above");
        if angle(active.dir) > SUN_START {
            let restart = self
                .pending
                .is_none_or(|(p, _)| angle(p.dir) > SUN_RESTART || p.z != z_now);
            if restart {
                let set = active.set ^ 1;
                cache.drop_set(set);
                self.pending = Some((fresh(set), false));
            }
        } else if let Some((p, _)) = self.pending.take() {
            // Back within reach of the active basis: drop the half-built one.
            cache.drop_set(p.set);
        }
        let mut gens = vec![Gen::new(active, input.camera.position)];
        if let Some((p, _)) = self.pending {
            gens.push(Gen::new(p, input.camera.position));
        }

        // Feedback of earlier frames (non-blocking).
        let _ = device.poll(wgpu::PollType::Poll);
        for r in &mut gpu.readbacks {
            if r.state.load(Ordering::Acquire) != 2 {
                continue;
            }
            if let (Some(marked), Ok(data)) =
                (r.map.as_ref(), r.buffer.slice(..).get_mapped_range())
            {
                let words: &[u32] = bytemuck::cast_slice(&data);
                for (half, (m, set)) in marked.bases.iter().enumerate() {
                    // Only marks made for a basis still in use.
                    let Some(g) = gens.iter().find(|g| g.basis.set == *set) else {
                        continue;
                    };
                    if g.basis.levels != m.levels || g.basis.finest != m.finest {
                        continue;
                    }
                    if g.basis.set != active.set {
                        if let Some((_, marked)) = self.pending.as_mut() {
                            *marked = true;
                        }
                    }
                    let base = half * MARK_WORDS * MAX_LEVELS;
                    let bits = &words[base..base + MARK_WORDS * MAX_LEVELS];
                    for (l, level) in bits.chunks(MARK_WORDS).enumerate().take(m.levels) {
                        for (w, &bits) in level.iter().enumerate() {
                            let mut b = bits;
                            while b != 0 {
                                let i = w * 32 + b.trailing_zeros() as usize;
                                b &= b - 1;
                                let slot = IVec2::new(i as i32 % PAGES, i as i32 / PAGES);
                                let key = PageKey::of(*set, l, m.global_page(l, slot));
                                if *set == active.set && !cache.is_rendered(&key) {
                                    stats.late += 1;
                                }
                                cache.request(key, marked.frame);
                                // Guard band: the neighbours too, so a turning or moving
                                // view finds its next pages already rendered instead of
                                // waiting a frame for their marks (sampled coarse meanwhile).
                                if guard_band() {
                                    for dy in -1..=1 {
                                        for dx in -1..=1 {
                                            let p = key.page() + IVec2::new(dx, dy);
                                            if (dx, dy) != (0, 0) && m.contains(l, p) {
                                                cache
                                                    .request(PageKey::of(*set, l, p), marked.frame);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            r.buffer.unmap();
            r.map = None;
            r.state.store(0, Ordering::Release);
        }
        for g in &gens {
            let (set, map) = (g.basis.set, &g.map);
            // The safety level always covers the shadow distance.
            for p in clipmap::disk_pages(map, safety, distance) {
                cache.request(PageKey::of(set, safety, p), input.frame);
            }
            // Moving casters: their columns at every level.
            for b in input.changed {
                for (l, (a, c)) in g.columns(b).into_iter().enumerate() {
                    cache.invalidate(set, l, a, c);
                }
            }
        }
        // Swaying leaves: the pages fine enough to show the sway (texel up to SWAY_TEXEL) under
        // crowns near the camera are rendered again, oldest first, SWAY_BUDGET a frame.
        let g0 = &gens[0];
        let mut sway: Vec<PageKey> = Vec::new();
        for b in input.sway {
            let columns = g0.columns(b);
            for (l, (a, c)) in columns.into_iter().enumerate() {
                if g0.map.texel(l) > SWAY_TEXEL {
                    break;
                }
                for y in a.y..=c.y {
                    for x in a.x..=c.x {
                        sway.push(PageKey::of(active.set, l, IVec2::new(x, y)));
                    }
                }
            }
        }
        stats.swaying = cache.refresh(sway.into_iter(), SWAY_BUDGET) as u32;
        let all = cache.update(
            input.frame,
            KEEP_FRAMES,
            |k| {
                gens.iter()
                    .find(|g| g.basis.set == k.set)
                    .is_some_and(|g| g.map.contains(k.level as usize, k.page()))
            },
            usize::MAX,
        );
        // The active basis's pages first; the pending one's PENDING_BUDGET a frame.
        let mut todo: Vec<PageKey> = all
            .iter()
            .filter(|k| k.set == active.set)
            .copied()
            .collect();
        todo.extend(
            all.iter()
                .filter(|k| k.set != active.set)
                .take(PENDING_BUDGET),
        );
        todo.truncate(PAGE_BUDGET);
        let cstats = cache.stats;
        // Rectangles, at most `max_rects` and what the scratch fills hold.
        let mut rects = rects::group(&todo);
        rects.truncate(input.max_rects.min(MAX_RECTS));
        let placed = rects::pack(&rects, SCRATCH_PAGES);
        let mut draws: Vec<RectDraw> = Vec::new();
        let mut rendered_keys: Vec<PageKey> = Vec::new();
        let mut copies: Vec<(usize, [u32; 4])> = Vec::new();
        let pool_row = POOL_ROW;
        for (r, (round, at)) in rects.iter().zip(placed) {
            if round >= MAX_ROUNDS {
                continue;
            }
            let Some(g) = gens.iter().find(|g| g.basis.set == r.set) else {
                continue;
            };
            let texel = g.map.texel(r.level);
            draws.push(RectDraw {
                rect: *r,
                view_proj: g
                    .map
                    .rect_view_proj(&g.view, r.level, r.lo, r.hi(), g.basis.z),
                round,
                at,
                // The cascades' choice: full trees close up, the first simplified level
                // further out, impostors only where a texel is wider than twigs anyway
                // (impostor crowns barely shadow themselves).
                tree_lod: if texel <= 0.03 {
                    0
                } else if texel < 0.2 {
                    1
                } else {
                    2
                },
                texels_per_metre: 1.0 / texel,
            });
            for j in 0..r.size.y {
                for i in 0..r.size.x {
                    let key = PageKey::of(r.set, r.level, r.lo + IVec2::new(i, j));
                    let Some(phys) = cache.phys(&key) else {
                        continue;
                    };
                    let src = (at + IVec2::new(i, j)).as_uvec2() * PAGE;
                    let dst = glam::UVec2::new(phys % pool_row, phys / pool_row) * PAGE;
                    copies.push((round, [src.x, src.y, dst.x, dst.y]));
                    if cache.is_rendered(&key) {
                        stats.invalidated += 1;
                    }
                    cache.rendered(&key, input.frame);
                    rendered_keys.push(key);
                    stats.rendered += 1;
                    if r.set != active.set {
                        stats.catching_up += 1;
                    }
                }
            }
        }
        stats.rects = draws.len() as u32;
        stats.pending = (all.len() as u32).saturating_sub(stats.rendered);
        // The pending basis takes over once its pixels have marked it and every page it wants
        // is rendered.
        let mut gens = gens;
        if let Some((p, true)) = self.pending
            && cache.unrendered_wanted(p.set) == 0
        {
            cache.drop_set(active.set);
            self.basis = Some(p);
            self.pending = None;
            gens.swap(0, 1);
            gens.truncate(1);
            stats.switched = true;
        }
        copies.sort_by_key(|c| c.0);
        let mut rounds = Vec::new();
        for round in 0..MAX_ROUNDS {
            let start = copies.partition_point(|c| c.0 < round) as u32;
            let end = copies.partition_point(|c| c.0 <= round) as u32;
            if draws.iter().any(|d| d.round == round) {
                rounds.push(start..end);
            }
        }
        let instances: Vec<[u32; 4]> = copies.iter().map(|c| c.1).collect();
        if !instances.is_empty() {
            queue.write_buffer(&gpu.copy_instances, 0, bytemuck::cast_slice(&instances));
        }
        queue.write_buffer(
            &gpu.copy_info,
            0,
            bytemuck::cast_slice(&[
                1.0 / gpu.pool_size.0 as f32,
                1.0 / gpu.pool_size.1 as f32,
                0.0,
                0.0,
            ]),
        );
        // Views of the rectangles.
        for (j, d) in draws.iter().enumerate() {
            let mut v = super::ViewUniform::zeroed();
            v.view_proj = d.view_proj.to_cols_array_2d();
            let c = input.camera.position;
            v.camera_pos = [c.x, c.y, c.z, 0.0];
            queue.write_buffer(
                &gpu.view_buffer,
                j as u64 * VIEW_STRIDE,
                bytemuck::bytes_of(&v),
            );
        }
        // Page table: every rendered page of the active basis inside its level's window.
        let shown = &gens[0];
        let rendered_now: std::collections::HashSet<PageKey> = if input.debug != VsmDebug::Off {
            rendered_keys.iter().copied().collect()
        } else {
            Default::default()
        };
        let mut table = vec![0u32; MAX_LEVELS * (PAGES * PAGES) as usize];
        for (k, phys) in cache.addressable() {
            let l = k.level as usize;
            if k.set != shown.basis.set || !shown.map.contains(l, k.page()) {
                continue;
            }
            let w = Clipmap::wrap(k.page());
            let flag = if rendered_now.contains(k) {
                FLAG_RENDERED
            } else {
                0
            };
            table[l * (PAGES * PAGES) as usize + (w.y * PAGES + w.x) as usize] = (phys + 1) | flag;
        }
        let per = (PAGES * PAGES) as usize;
        for l in 0..MAX_LEVELS {
            let now = &table[l * per..(l + 1) * per];
            if self.table.len() == table.len() && self.table[l * per..(l + 1) * per] == *now {
                continue;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu.table,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: l as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(now),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(PAGES as u32 * 4),
                    rows_per_image: Some(PAGES as u32),
                },
                wgpu::Extent3d {
                    width: PAGES as u32,
                    height: PAGES as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.table = table;

        // The shaders' parameters: the active basis for the lit shaders, both for marking.
        let (pixel, floor) = match input.camera.projection {
            crate::camera::Projection::Perspective { fov_y } => (
                2.0 * (fov_y.to_radians() * 0.5).tan() / input.target_height.max(1) as f32,
                0.0,
            ),
            crate::camera::Projection::Orthographic { size } => {
                (0.0, 2.0 * size / input.target_height.max(1) as f32)
            }
        };
        let reach = input.sun_size * clipmap::MAX_BLOCKER_DISTANCE;
        let search = clipmap::kernel_level(reach, finest, levels);
        let uniform = |g: &Gen| {
            let inv = g.view.inverse();
            let axis = |v: Vec3| inv.transform_vector3(v).normalize_or_zero();
            let (lx, ly, lz) = (axis(Vec3::X), axis(Vec3::Y), axis(Vec3::Z));
            let range = (g.basis.z.1 - g.basis.z.0).max(1.0);
            let camera_depth = (g.map.camera.z - g.basis.z.0) / range;
            let mut u = VsmUniform {
                light_x: lx.extend(1.0).to_array(),
                light_y: ly.extend(levels as f32).to_array(),
                light_z: lz.extend(1.0 / range).to_array(),
                // The footprint is scaled like `finest`, so a level's texel is Shadow
                // Resolution's texels per pixel times the footprint it serves.
                info: [
                    finest,
                    pixel * s.texels_per_pixel,
                    camera_depth,
                    floor * s.texels_per_pixel,
                ],
                misc: [input.debug.index(), search as f32, safety as f32, distance],
                pool: [
                    1.0 / gpu.pool_size.0 as f32,
                    1.0 / gpu.pool_size.1 as f32,
                    POOL_ROW as f32,
                    0.0,
                ],
                levels: [[0.0; 4]; MAX_LEVELS],
                wrap: [[0.0; 4]; MAX_LEVELS],
            };
            for l in 0..levels {
                let o = g.map.origin_offset(l);
                let t = g.map.texel(l);
                u.levels[l] = [o.x, o.y, 1.0 / t, t];
                let w = Clipmap::wrap(g.map.origins[l]);
                u.wrap[l] = [w.x as f32, w.y as f32, 0.0, 0.0];
            }
            u
        };
        let u = uniform(&gens[0]);
        let marks = [u, gens.get(1).map_or(VsmUniform::zeroed(), uniform)];
        queue.write_buffer(&gpu.mark_params, 0, bytemuck::cast_slice(&marks));
        stats.wanted = cstats.wanted;
        stats.resident = cstats.resident;
        stats.evicted = cstats.evicted;
        stats.overflow = cstats.overflow;
        self.stats = stats;
        self.marking = Some(Marked {
            bases: gens.iter().map(|g| (g.map.clone(), g.basis.set)).collect(),
            frame: input.frame,
        });
        self.frame = Some(VsmFrame {
            uniform: u,
            rects: draws,
            rounds,
            light_dir: gens[0].basis.dir,
        });
    }

    /// Page copies from the scratch atlas into the pool (after each scratch fill).
    pub fn encode_copy(&self, encoder: &mut wgpu::CommandEncoder, instances: std::ops::Range<u32>) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty vsm page copy"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &gpu.pool_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if instances.is_empty() {
            return;
        }
        pass.set_pipeline(&gpu.copy);
        pass.set_bind_group(1, &gpu.copy_group, &[]);
        pass.set_vertex_buffer(0, gpu.copy_instances.slice(..));
        pass.draw(0..6, instances);
    }

    /// Leave this frame unmarked (the marks of the frame before stand; see `render_frame`).
    pub fn skip_marks(&mut self) {
        self.marking = None;
    }

    /// The pages this frame's pixels need are still to be marked (the main pass must keep
    /// its depth for it when no earlier depth was marked).
    pub fn wants_marks(&self) -> bool {
        self.marking.is_some()
    }

    /// Mark the pages this frame's pixels need from a depth buffer (the screen-space
    /// prepass's when it runs, else the main pass's) and copy the bits out.
    pub fn encode_mark(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view_group: &wgpu::BindGroup,
        depth: &wgpu::TextureView,
        size: (u32, u32),
        multisampled: bool,
    ) {
        let (Some(gpu), Some(map)) = (self.gpu.as_mut(), self.marking.take()) else {
            return;
        };
        let Some(slot) = gpu
            .readbacks
            .iter()
            .position(|r| r.state.load(Ordering::Acquire) == 0)
        else {
            return;
        };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vsm mark"),
            layout: &gpu.mark[!multisampled as usize].1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: gpu.mark_params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: gpu.marks.as_entire_binding(),
                },
            ],
        });
        encoder.clear_buffer(&gpu.marks, 0, None);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("misty vsm mark"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.mark[!multisampled as usize].0);
            pass.set_bind_group(0, view_group, &[]);
            pass.set_bind_group(1, &group, &[]);
            // 8 × 8 threads a group, one thread per 2 × 2 pixels.
            pass.dispatch_workgroups(size.0.div_ceil(16), size.1.div_ceil(16), 1);
        }
        let r = &mut gpu.readbacks[slot];
        encoder.copy_buffer_to_buffer(&gpu.marks, 0, &r.buffer, 0, r.buffer.size());
        r.state.store(1, Ordering::Release);
        r.map = Some(map);
    }

    /// Ask for the mark readbacks once the frame is submitted.
    pub fn submitted(&self) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        for r in &gpu.readbacks {
            if r.state
                .compare_exchange(1, 3, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let state = r.state.clone();
                r.buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                    state.store(if result.is_ok() { 2 } else { 0 }, Ordering::Release);
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_matches_wgsl() {
        // 6 vec4s, then two arrays of MAX_LEVELS vec4s (vsm_common.wgsl `VsmParams`).
        assert_eq!(
            std::mem::size_of::<VsmUniform>(),
            16 * 6 + 2 * 16 * MAX_LEVELS
        );
    }

    #[test]
    fn caster_changes_find_moved_new_and_removed_casters() {
        let mut v = Vsm::default();
        let b = |x: f32| Aabb::new(Vec3::splat(x), Vec3::splat(x + 1.0));
        let m = |x: f32| Mat4::from_translation(Vec3::splat(x));
        let sig = |x: f32| caster_signature(7, &m(x), 1);
        assert_eq!(
            v.caster_changes([(sig(0.0), b(0.0), false), (sig(5.0), b(5.0), false)].into_iter())
                .len(),
            2
        );
        // Unchanged: nothing.
        assert!(
            v.caster_changes([(sig(0.0), b(0.0), false), (sig(5.0), b(5.0), false)].into_iter())
                .is_empty()
        );
        // One moved: its old and new bounds.
        let moved =
            v.caster_changes([(sig(0.0), b(0.0), false), (sig(6.0), b(6.0), false)].into_iter());
        assert_eq!(moved.len(), 2);
        assert!(moved.contains(&b(5.0)) && moved.contains(&b(6.0)));
        // Skinned (always dirty) every frame.
        let skinned =
            v.caster_changes([(sig(0.0), b(0.0), false), (sig(6.0), b(6.0), true)].into_iter());
        assert_eq!(skinned, vec![b(6.0)]);
    }

    #[test]
    fn terrain_detail_changes_dirty_their_chunks() {
        let mut v = Vsm::default();
        let bounds = |_t: usize, c: usize| {
            Some(Aabb::new(
                Vec3::splat(c as f32),
                Vec3::splat(c as f32 + 1.0),
            ))
        };
        let (_, reset) = v.terrain_changes(vec![("t".into(), 1, vec![(0, 0), (1, 0)])], bounds);
        assert!(reset);
        let (changed, reset) =
            v.terrain_changes(vec![("t".into(), 1, vec![(0, 0), (2, 1)])], bounds);
        assert!(!reset);
        assert_eq!(changed.len(), 1);
        let (_, reset) = v.terrain_changes(vec![("t".into(), 2, vec![(0, 0), (2, 1)])], bounds);
        assert!(reset, "edited terrain");
    }
}
