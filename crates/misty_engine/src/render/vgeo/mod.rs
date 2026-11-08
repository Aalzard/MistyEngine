//! Misty Virtual Geometry on the GPU (prototype, off by default: Quality ▸ Virtual Geometry).
//!
//! Meshes imported with Virtual Geometry on are built into clusters and a cluster hierarchy
//! ([`crate::vgeo`]) and kept in one shared pool: the source vertices, every cluster's
//! triangles and the cluster records. Each frame the draws of those meshes become instances;
//! compute passes (vgeo_cull.wgsl) pick, per instance and per view, the clusters whose level
//! of detail fits the pixel error threshold and that pass the frustum and Hi-Z tests, and write
//! them into a visible list with the indirect draw arguments of each batch (one per material
//! texture set). The main pass draws them by vertex pulling with the ordinary lit shader.
//!
//! Views: the main camera in two phases (instances visible last frame, then the rest against
//! the Hi-Z pyramid built from the first phase's depth, like render/gpu_culling), then each
//! shadow map (frustum only, level of detail chosen for the main camera), up to
//! [`MAX_SHADOW_VIEWS`] (cascades, local lights, virtual shadow map rectangles). The first
//! phase's list also feeds the depth + normal prepass of the screen-space effects.
//!
//! Level of detail: each view decides every candidate group once (vgeo_cull.wgsl
//! `cs_groups`), and clusters read their own group's and their parent group's decision, so the
//! two sides of a group border always agree (crack-free by construction).
//!
//! Materials: draws of one virtual mesh with the same transform (the primitives of a glTF mesh,
//! one per material) become one instance whose material slots name each part's draw and batch;
//! a cluster goes to its part's slot. Opaque and Cutout (alpha-tested) batches are drawn by
//! their own pipelines in every pass (main, visibility raster, shadows, prepass).

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::culling::Frustum;
use super::gpu_culling::TargetState;
use super::pipelines::{self, DEPTH_SRC, Depth, MESH_SRC, Pipelines, Spec};
use super::{Frame, SHADOW_FORMAT};
use crate::camera::Projection;
use crate::scene::MeshRef;
use crate::vgeo::VirtualMesh;

pub(super) const CULL_SRC: &str = concat!(
    include_str!("../shaders/draw.wgsl"),
    "\n",
    include_str!("vgeo_cull.wgsl")
);
const FETCH_SRC: &str = include_str!("vgeo_fetch.wgsl");
const MESH_ENTRY_SRC: &str = include_str!("vgeo_mesh.wgsl");
const DEPTH_ENTRY_SRC: &str = include_str!("vgeo_depth.wgsl");
const PREPASS_ENTRY_SRC: &str = include_str!("vgeo_prepass.wgsl");

/// Visible-list entries of a main view (× 128 triangles at most each) and of a shadow view,
/// shared equally by the batches. (Entry × 384 is the indexed path's base vertex: it must stay
/// below 2³¹ for all views.)
const MAIN_VISIBLE: u32 = 1 << 18;
const SHADOW_VISIBLE: u32 = 1 << 16;
/// Chunks of 64 candidate clusters, and of 64 groups to decide, per main / shadow view.
const MAIN_CHUNKS: u32 = 1 << 17;
const SHADOW_CHUNKS: u32 = 1 << 15;
const MAIN_GROUP_CHUNKS: u32 = 1 << 16;
const SHADOW_GROUP_CHUNKS: u32 = 1 << 14;
/// Auto visibility buffer: on once the main camera draws at least this many triangles per pixel
/// and material batch (each batch's resolve reads every pixel's ids; the visibility buffer only
/// pays when the triangles are small enough for quad overshading to cost more), off again below
/// [`VIS_OFF_TRIANGLES`] (hysteresis: no flicker between the paths).
const VIS_ON_TRIANGLES: f32 = 0.25;
const VIS_OFF_TRIANGLES: f32 = 0.18;
/// Auto occlusion: the main camera takes two phases around a Hi-Z pyramid (occlusion culling)
/// from this many instances on, or when GPU culling builds the pyramid anyway; fewer instances
/// are culled in one phase (frustum and level of detail only), which keeps the main pass whole.
/// (The 3000-rock field is 0.6 ms faster in one phase: the pyramid, the split main pass and the
/// second culling cost more than the few rocks hidden behind others save.)
pub(super) const OCCLUSION_MIN_INSTANCES: usize = 16384;
/// Shadow maps with virtual geometry casters (in plan order: cascades, local lights, virtual
/// shadow map rectangles). The lists grow with the views a frame uses.
pub const MAX_SHADOW_VIEWS: usize = 32;
const MAX_VIEWS: usize = 2 + MAX_SHADOW_VIEWS;
/// Counters per view at the start of `control` (vgeo_cull.wgsl).
const COUNTERS: usize = 8;
/// A slot record's draw when the object doesn't draw that part.
const NO_DRAW: u32 = u32::MAX;
/// Words per vertex in the pool: position (3 × f32), normal and tangent (octahedral 2 × 16
/// bits each, the bitangent sign in the tangent's lowest bit), uv (2 × f32), lightmap uv
/// (2 × unorm16). [`super::mesh::Vertex`] is 14 floats.
const VERTEX_WORDS: usize = 8;
const WORKGROUP: u32 = 64;
/// Words of `control` before the limits: the counters and 8 dispatch words per view
/// (`DISPATCH_BASE` in vgeo_cull.wgsl = the counters' words).
const CONTROL_HEADER: usize = MAX_VIEWS * (COUNTERS + 8);

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuCluster {
    bounds: [f32; 4],
    /// Mesh-local groups ([`crate::vgeo::NO_GROUP`] = none): the one that made it, the one it
    /// was simplified into, and the latter's error (the binary search's key).
    group: u32,
    parent_group: u32,
    parent_error: f32,
    /// Triangles (8 bits) | level << 8 (6 bits) | material slot << 14.
    packed: u32,
    /// Its own vertex list (source vertex ids) and local indices (16-bit, in index units).
    vertex_first: u32,
    local_first: u32,
    /// Normal cone: octahedral axis and cutoff ([`crate::vgeo::Cluster::backfacing`]).
    cone_axis: u32,
    cone_cutoff: f32,
}

/// A mesh record (a = bounds, b = first cluster, clusters, first group record, groups) or a
/// group record (a = its sphere, b.x = its error's bits): vgeo_cull.wgsl `Record`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuRecord {
    a: [f32; 4],
    b: [u32; 4],
}

/// An instance (draw, mesh record, first slot record, bit 0 = casts shadows) or a material slot
/// record (draw or [`NO_DRAW`], batch, bit 0 = casts shadows, 0): vgeo_cull.wgsl.
type GpuInstance = [u32; 4];

/// Where one view's entries are: in `lists` (and, for the visible list, `cluster_args`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Region {
    visible_first: u32,
    visible: u32,
    chunk_first: u32,
    chunks: u32,
    group_first: u32,
    groups: u32,
}

/// Visible entries, cluster chunks and group chunks of view `v` (0, 1 = the main camera).
fn view_sizes(v: usize) -> (u32, u32, u32) {
    match v < 2 {
        true => (MAIN_VISIBLE, MAIN_CHUNKS, MAIN_GROUP_CHUNKS),
        false => (SHADOW_VISIBLE, SHADOW_CHUNKS, SHADOW_GROUP_CHUNKS),
    }
}

/// The regions of `views` views: every visible list first (one index space for the draw
/// arguments), then every view's cluster chunks, then its group chunks. Returns them and the
/// entries of `lists`.
fn regions(views: usize) -> (Vec<Region>, u32) {
    let mut out: Vec<Region> = Vec::with_capacity(views);
    let mut at = 0;
    for v in 0..views {
        let (visible, chunks, groups) = view_sizes(v);
        out.push(Region {
            visible_first: at,
            visible,
            chunk_first: 0,
            chunks,
            group_first: 0,
            groups,
        });
        at += visible;
    }
    for r in &mut out {
        r.chunk_first = at;
        at += r.chunks;
        r.group_first = at;
        at += r.groups;
    }
    (out, at)
}

/// Visible-list entries of `views` views (the draw arguments' index space).
fn visible_entries(views: usize) -> u32 {
    (0..views).map(|v| view_sizes(v).0).sum()
}

/// The pass a virtual geometry draw belongs to (Frame Debugger events).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VgeoPass {
    /// Clusters drawn lit directly in the main pass (or a debug view).
    Lit,
    /// The visibility raster's ids.
    Raster,
    /// The material resolve (one fullscreen draw per batch and layer).
    Resolve,
    Shadow,
    Prepass,
}

/// One batch draw of virtual geometry: what [`Gate`] is asked about.
#[derive(Clone, Copy, Debug)]
pub(super) struct VgeoDraw {
    pub pass: VgeoPass,
    /// The view whose visible list it draws (0, 1 = main phases; 2.. = shadow maps).
    pub view: usize,
    pub batch: usize,
    /// A draw of the batch (its material).
    pub draw: usize,
    pub cutout: bool,
}

/// Asked before each batch draw (false = skip it): the Frame Debugger records it and stops
/// the frame at the chosen event; otherwise always true.
pub(super) type Gate<'g> = dyn Fn(&VgeoDraw) -> bool + 'g;

/// Debug views of virtual geometry (Scene view draw modes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VgeoDebug {
    #[default]
    Off,
    /// Every cluster in its own colour.
    Clusters,
    /// The level each cluster comes from (blue = source triangles … magenta = coarsest).
    Levels,
    /// Layers of rasterized cluster triangles, additive (bright = overdraw).
    Overdraw,
    /// Each material slot in its own colour.
    Materials,
}

impl VgeoDebug {
    /// Its number in the shaders (vgeo_mesh.wgsl `VgDebugInfo`).
    fn code(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::Clusters => 1,
            Self::Levels => 2,
            Self::Overdraw => 3,
            Self::Materials => 4,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    view_proj: [[f32; 4]; 4],
    planes: [[f32; 4]; 6],
    camera: [f32; 4],
    params: [f32; 4],
    info: [u32; 4],
    region: [u32; 4],
    caps: [u32; 4],
    /// This view's own eye (w = 1) or, orthographic, its forward direction (w = 0): the
    /// clusters' back-face test.
    eye: [f32; 4],
    /// First group chunk entry, group chunk capacity.
    groups: [u32; 4],
}

/// How a frame picks a virtual geometry option: by its cost (Auto, the default) or always on or
/// off (A/B measurements, tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VgeoChoice {
    #[default]
    Auto,
    On,
    Off,
}

impl From<bool> for VgeoChoice {
    fn from(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

/// What the last frame drew (read back on request, for measurements).
#[derive(Clone, Copy, Debug, Default)]
pub struct VgeoStats {
    pub instances: u32,
    /// Main camera, both phases.
    pub clusters: u32,
    pub triangles: u64,
    /// Every shadow map together.
    pub shadow_triangles: u64,
    /// Clusters left out because a visible list was full.
    pub dropped: u32,
    /// Instance views left out because their view had no room to decide their groups.
    pub dropped_instances: u32,
    /// Virtual meshes in the pool and their source triangles.
    pub meshes: u32,
    pub source_triangles: u64,
    /// GPU memory of the pool (vertices, local indices, cluster vertices, cluster records).
    pub pool_bytes: u64,
    /// GPU memory of the per-view visible lists, chunks and draw arguments.
    pub list_bytes: u64,
    /// The main pass drew the clusters through the visibility buffer.
    pub visibility: bool,
    /// The main camera took two phases around a Hi-Z pyramid (occlusion culling).
    pub occlusion: bool,
}

pub(super) struct VirtualGeometry {
    /// Quality ▸ Virtual Geometry.
    pub enabled: bool,
    /// Largest error on screen, in pixels.
    pub threshold: f32,
    /// Debug view (colours instead of the material), every render.
    pub debug: VgeoDebug,
    /// Debug view of the next render only (the Scene view's draw mode).
    pub debug_once: Option<VgeoDebug>,
    /// This render's debug view.
    pub view_debug: VgeoDebug,
    /// Draw the main pass's clusters through the visibility buffer (render/vgeo/vis.rs) where
    /// the device can, or lit directly; Auto picks the visibility buffer once the triangles
    /// get small (see [`VIS_ON_TRIANGLES`]).
    pub visibility: VgeoChoice,
    /// The last Auto visibility decision (hysteresis).
    vis_auto: bool,
    /// Two main phases around a Hi-Z pyramid (occlusion culling) or one; Auto: two from
    /// [`OCCLUSION_MIN_INSTANCES`] instances on, or when GPU culling builds the pyramid anyway.
    pub occlusion: VgeoChoice,
    /// The visibility resolve's second layer: two objects of one batch meeting inside a pixel
    /// get a shade each (otherwise the first one's; edges against everything else keep their
    /// MSAA coverage either way).
    pub edge_layer: bool,
    /// This frame draws virtual meshes through clusters (set before the draw prep).
    pub on: bool,
    /// Mesh key → (index in the pool, material slot) (`None` = not a virtual mesh).
    pool_index: HashMap<String, Option<(u32, u32)>>,
    /// Key of the mesh holding a build (a glTF mesh's first primitive) → index in the pool.
    source_index: HashMap<String, u32>,
    generation: u64,
    /// [`crate::models::virtual_build::ready_generation`] the pool was laid out at.
    ready: u64,
    vertices: Vec<u32>,
    clusters: Vec<GpuCluster>,
    /// Mesh records (their first group relative to `groups`) and group records.
    meshes: Vec<GpuRecord>,
    groups: Vec<GpuRecord>,
    /// Material slots of each pool mesh.
    mesh_materials: Vec<u32>,
    /// Per cluster: its vertices' source ids, and its triangles as indices into that list.
    cluster_vertices: Vec<u32>,
    local_indices: Vec<u16>,
    source_triangles: u64,
    pool_dirty: bool,
    gpu: Option<Gpu>,
    frame: Option<FrameState>,
    last_instances: usize,
    /// The per-view counters copied out each frame and read back when ready (never waited
    /// for): the Stats box and the Profiler.
    readback: Vec<Readback>,
    /// The last counters read back (a frame or two old).
    latest: VgeoStats,
}

/// One staging buffer of the counter readback.
struct Readback {
    buffer: wgpu::Buffer,
    /// 0 = free, 1 = copied this frame, 2 = mapping, 3 = mapped.
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    views: usize,
    instances: u32,
    visibility: bool,
    occlusion: bool,
}

struct Gpu {
    cull_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
    cs_instances: wgpu::ComputePipeline,
    cs_prepare: wgpu::ComputePipeline,
    cs_groups: wgpu::ComputePipeline,
    cs_clusters: wgpu::ComputePipeline,
    cs_finish: wgpu::ComputePipeline,
    main: wgpu::RenderPipeline,
    main_cutout: wgpu::RenderPipeline,
    debug: wgpu::RenderPipeline,
    overdraw: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    shadow_cutout: wgpu::RenderPipeline,
    prepass: wgpu::RenderPipeline,
    prepass_cutout: wgpu::RenderPipeline,
    /// Draw each cluster as an indexed sub-draw with the GPU's count
    /// (`MULTI_DRAW_INDIRECT_COUNT`); else 384 pulled vertices per cluster.
    indexed: bool,
    pool: Option<Pool>,
    /// The instances, then their material slot records.
    instances: wgpu::Buffer,
    /// Per instance: visible last frame, drawn by the first phase, its first group chunk and
    /// first decided group in the view being culled.
    instance_state: wgpu::Buffer,
    /// Counters, dispatch arguments, limits and draw arguments (vgeo_cull.wgsl).
    control: wgpu::Buffer,
    /// The cluster passes' workgroups, copied out of `control` (a buffer bound for writing
    /// can't also be the indirect buffer of a dispatch).
    dispatch: wgpu::Buffer,
    /// Every view's visible list, then every view's cluster and group chunks ([`regions`]).
    lists: wgpu::Buffer,
    /// Per visible-list entry: that cluster's indexed draw arguments.
    cluster_args: wgpu::Buffer,
    /// Per view: its index (the first view of a culling dispatch run, cull binding 0).
    uniforms: Vec<wgpu::Buffer>,
    /// Every view's [`ViewUniform`] (cull binding 10).
    views: wgpu::Buffer,
    /// Views the lists and uniforms are laid out for (they grow on demand).
    view_capacity: usize,
    /// Visible entries per view and per batch (vgeo_vis.wgsl `VgInfo`).
    info: wgpu::Buffer,
    /// The visibility buffer's pipelines and targets (None: this device draws clusters lit).
    vis: Option<vis::Vis>,
    /// Records (instances and slots) the instance buffer holds; instances the state holds.
    instance_capacity: usize,
    state_capacity: usize,
    /// Views the instance state is laid out for.
    state_views: usize,
    batch_capacity: usize,
}

/// Sections of the pool buffer (index into [`Pool::sections`]).
const VERTICES: usize = 0;
/// 16-bit local indices: the index buffer of the indexed path, read as storage elsewhere.
const LOCAL: usize = 1;
const CLUSTERS: usize = 2;
/// Mesh records, then group records.
const MESHES: usize = 3;
const CLUSTER_VERTICES: usize = 4;

/// The whole pool in one buffer (the visibility resolve reads it through a single binding:
/// the lit fragment stage already uses 7 of the 8 storage buffers WebGPU guarantees).
pub(super) struct Pool {
    buffer: wgpu::Buffer,
    /// (byte offset, byte size) of the vertices, local indices, clusters, meshes and cluster
    /// vertices, each aligned for a storage binding.
    sections: [(u64, u64); 5],
}

impl Pool {
    fn new(device: &wgpu::Device, parts: [&[u8]; 5]) -> Self {
        let align = device.limits().min_storage_buffer_offset_alignment.max(16) as u64;
        let mut bytes = Vec::new();
        let mut sections = [(0u64, 0u64); 5];
        for (i, part) in parts.iter().enumerate() {
            let at = (bytes.len() as u64).next_multiple_of(align);
            bytes.resize(at as usize, 0);
            bytes.extend_from_slice(part);
            // Never an empty binding; whole words.
            let size = (part.len() as u64).max(16).next_multiple_of(4);
            bytes.resize((at + size) as usize, 0);
            sections[i] = (at, size);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty vgeo pool"),
            contents: &bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDEX,
        });
        Self { buffer, sections }
    }

    fn binding(&self, section: usize) -> wgpu::BindingResource<'_> {
        let (offset, size) = self.sections[section];
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.buffer,
            offset,
            size: wgpu::BufferSize::new(size),
        })
    }

    /// Word offsets of the sections (the visibility resolve's view of the whole buffer).
    fn word_offsets(&self) -> [u32; 5] {
        self.sections.map(|(at, _)| (at / 4) as u32)
    }
}

struct FrameState {
    views: usize,
    /// Where each view's entries are.
    regions: Vec<Region>,
    /// Shadow slot of view `2 + i`.
    shadow_slots: Vec<usize>,
    /// A draw of each batch (its material's texture set) and whether it is Cutout.
    batches: Vec<usize>,
    cutout: Vec<bool>,
    instances: u32,
    cull_groups: Vec<wgpu::BindGroup>,
    render_group: wgpu::BindGroup,
    /// This frame draws the main pass's clusters through the visibility buffer.
    vis: Option<vis::VisFrame>,
    /// The main camera is culled in one phase (view 0, frustum and level of detail only).
    single: bool,
}

/// Everything [`VirtualGeometry::begin_frame`] needs from the renderer.
pub(super) struct FrameInputs<'a> {
    pub frame: &'a Frame<'a>,
    /// Per draw: (pool mesh, material slot) when it draws through clusters.
    pub virtual_mesh: &'a [Option<(u32, u32)>],
    pub draws: &'a wgpu::Buffer,
    pub view_proj: Mat4,
    pub size: (u32, u32),
    pub shadow_matrices: Vec<Mat4>,
    /// Per shadow map: shadow-map pixels per world unit for the sun's cascades (their level
    /// of detail is chosen in their own texels), `None` for local lights (the camera's).
    pub shadow_texels: Vec<Option<f32>>,
    pub state: &'a TargetState,
    /// Texture set of each draw's material, as an address (batches group by it).
    pub batch_key: &'a dyn Fn(usize) -> usize,
    /// Is the draw's material Cutout (alpha-tested)?
    pub cutout: &'a dyn Fn(usize) -> bool,
    /// GPU culling builds the Hi-Z pyramid this frame anyway.
    pub gpu_culling: bool,
}

fn storage(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    extra: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | extra,
        mapped_at_creation: false,
    })
}

/// The eye of a view-projection matrix: the point clip space maps to (0, 0, z, 0), as
/// (x, y, z, 1); for an orthographic matrix the direction depth grows along, as (x, y, z, 0).
fn view_eye(view_proj: &Mat4) -> [f32; 4] {
    let e = view_proj.inverse() * glam::Vec4::new(0.0, 0.0, 1.0, 0.0);
    let xyz = e.truncate();
    if e.w.abs() > 1e-6 * xyz.length() {
        (xyz / e.w).extend(1.0).to_array()
    } else {
        xyz.normalize_or_zero().extend(0.0).to_array()
    }
}

/// The batch draw a view's clusters go through, its pipeline and group 2 already set.
type BatchSetup<'s> = dyn FnMut(&mut wgpu::RenderPass<'_>, usize, usize, bool) -> bool + 's;

/// Word offsets in `control` of the limits and draw arguments for `batches` batches per view.
fn control_layout(batches: usize) -> (u32, u32) {
    let limits = CONTROL_HEADER;
    let args = limits + MAX_VIEWS * batches;
    (limits as u32, args as u32)
}

fn control_buffer(device: &wgpu::Device, batches: usize) -> wgpu::Buffer {
    let words = control_layout(batches).1 as usize + MAX_VIEWS * batches * 4;
    storage(
        device,
        "misty vgeo control",
        words as u64 * 4,
        wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC,
    )
}

/// Two components in [-1, 1] as 16-bit signed normalized values (WGSL `unpack2x16snorm`).
fn pack_snorm(x: f32, y: f32) -> u32 {
    let q = |v: f32| ((v.clamp(-1.0, 1.0) * 32767.0).round() as i16) as u16 as u32;
    q(x) | (q(y) << 16)
}

/// A direction in octahedral encoding, 2 × 16 bits (decoded by `vg_octahedral` in
/// vgeo_fetch.wgsl).
fn octahedral(n: Vec3) -> u32 {
    let sum = n.x.abs() + n.y.abs() + n.z.abs();
    if sum < 1e-20 {
        return pack_snorm(0.0, 0.0);
    }
    let n = n / sum;
    let sign = |v: f32| if v >= 0.0 { 1.0 } else { -1.0 };
    let (x, y) = match n.z < 0.0 {
        true => ((1.0 - n.y.abs()) * sign(n.x), (1.0 - n.x.abs()) * sign(n.y)),
        false => (n.x, n.y),
    };
    pack_snorm(x, y)
}

/// A pool vertex ([`VERTEX_WORDS`]; decoded by `vg_vertex` in vgeo_fetch.wgsl).
fn pack_vertex(v: &super::mesh::Vertex) -> [u32; VERTEX_WORDS] {
    let p = v.position.map(f32::to_bits);
    let t = Vec3::from_slice(&v.tangent[..3]);
    // The bitangent sign replaces the lowest bit of the tangent's x.
    let tangent = (octahedral(t) & !1) | (v.tangent[3] < 0.0) as u32;
    let unorm = |f: f32| (f.clamp(0.0, 1.0) * 65535.0).round() as u32;
    [
        p[0],
        p[1],
        p[2],
        octahedral(Vec3::from(v.normal)),
        tangent,
        v.uv[0].to_bits(),
        v.uv[1].to_bits(),
        unorm(v.uv2[0]) | (unorm(v.uv2[1]) << 16),
    ]
}

impl Default for VirtualGeometry {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 1.0,
            debug: VgeoDebug::Off,
            debug_once: None,
            view_debug: VgeoDebug::Off,
            visibility: VgeoChoice::Auto,
            vis_auto: false,
            occlusion: VgeoChoice::Auto,
            edge_layer: false,
            on: false,
            pool_index: HashMap::new(),
            source_index: HashMap::new(),
            generation: u64::MAX,
            ready: 0,
            vertices: Vec::new(),
            clusters: Vec::new(),
            meshes: Vec::new(),
            groups: Vec::new(),
            mesh_materials: Vec::new(),
            cluster_vertices: Vec::new(),
            local_indices: Vec::new(),
            source_triangles: 0,
            pool_dirty: false,
            gpu: None,
            frame: None,
            last_instances: usize::MAX,
            readback: Vec::new(),
            latest: VgeoStats::default(),
        }
    }
}

impl VirtualGeometry {
    /// Pool index and material slot of a mesh drawn through clusters this frame.
    pub fn mesh_index(&self, r: &MeshRef) -> Option<(u32, u32)> {
        if !self.on {
            return None;
        }
        match r {
            MeshRef::Asset(key) => self.pool_index.get(key).copied().flatten(),
            MeshRef::Primitive(_) => None,
        }
    }

    /// Build (or take from the cache) the virtual meshes this frame draws and upload the pool.
    pub fn prepare_meshes(&mut self, device: &wgpu::Device, frame: &Frame) {
        let generation = crate::models::generation();
        if generation != self.generation {
            self.generation = generation;
            self.pool_index.clear();
            self.source_index.clear();
            self.vertices.clear();
            self.clusters.clear();
            self.meshes.clear();
            self.groups.clear();
            self.mesh_materials.clear();
            self.cluster_vertices.clear();
            self.local_indices.clear();
            self.source_triangles = 0;
            self.pool_dirty = true;
        }
        // Builds finished on a worker: look again at the meshes drawn the ordinary way.
        let ready = crate::models::virtual_build::ready_generation();
        if ready != self.ready {
            self.ready = ready;
            self.pool_index.retain(|_, v| v.is_some());
        }
        for d in frame.meshes {
            let MeshRef::Asset(key) = &d.mesh else {
                continue;
            };
            if self.pool_index.contains_key(key) {
                continue;
            }
            // Built at import (or still building: then the ordinary mesh path draws it). The
            // primitives of a glTF mesh share their first one's build.
            let index = crate::models::virtual_mesh(&d.mesh).and_then(|built| {
                let MeshRef::Asset(source) = &built.source else {
                    return None;
                };
                let pool = match self.source_index.get(source) {
                    Some(&p) => p,
                    None => {
                        let vertices = crate::models::virtual_vertices(&built.source)?;
                        let p = self.add_to_pool(&vertices, &built.mesh);
                        self.source_index.insert(source.clone(), p);
                        p
                    }
                };
                Some((pool, built.slot))
            });
            self.pool_index.insert(key.clone(), index);
        }
        if self.pool_dirty
            && let Some(gpu) = self.gpu.as_mut()
        {
            // Mesh records first, then the groups (each mesh's first group made absolute).
            let n = self.meshes.len() as u32;
            let mut records = self.meshes.clone();
            for r in &mut records {
                r.b[2] += n;
            }
            records.extend_from_slice(&self.groups);
            gpu.pool = Some(Pool::new(
                device,
                [
                    bytemuck::cast_slice(&self.vertices),
                    bytemuck::cast_slice(&self.local_indices),
                    bytemuck::cast_slice(&self.clusters),
                    bytemuck::cast_slice(&records),
                    bytemuck::cast_slice(&self.cluster_vertices),
                ],
            ));
            self.pool_dirty = false;
        }
    }

    fn add_to_pool(&mut self, vertices: &[super::mesh::Vertex], m: &VirtualMesh) -> u32 {
        let base = (self.vertices.len() / VERTEX_WORDS) as u32;
        for v in vertices {
            self.vertices.extend_from_slice(&pack_vertex(v));
        }
        let first = self.clusters.len() as u32;
        let group_first = self.groups.len() as u32;
        let sphere = |s: crate::vgeo::Sphere| [s.center.x, s.center.y, s.center.z, s.radius];
        for g in &m.groups {
            self.groups.push(GpuRecord {
                a: sphere(g.sphere),
                b: [g.error.to_bits(), 0, 0, 0],
            });
        }
        for (i, c) in m.clusters.iter().enumerate() {
            // The cluster's own vertex list (first use order) and local indices into it.
            let vertex_first = self.cluster_vertices.len() as u32;
            let local_first = self.local_indices.len() as u32;
            let mut local: HashMap<u32, u16> = HashMap::new();
            for &v in m.triangles(i) {
                let next = local.len() as u16;
                let l = *local.entry(v).or_insert_with(|| {
                    self.cluster_vertices.push(v + base);
                    next
                });
                self.local_indices.push(l);
            }
            self.clusters.push(GpuCluster {
                bounds: sphere(c.bounds),
                group: c.group,
                parent_group: c.parent_group,
                parent_error: c.parent_error,
                packed: c.triangles | (c.level.min(63) << 8) | (c.material << 14),
                vertex_first,
                local_first,
                cone_axis: octahedral(c.cone_axis),
                cone_cutoff: c.cone_cutoff,
            });
        }
        self.meshes.push(GpuRecord {
            a: sphere(m.bounds),
            b: [
                first,
                m.clusters.len() as u32,
                group_first,
                m.groups.len() as u32,
            ],
        });
        self.mesh_materials.push(m.materials.max(1));
        self.source_triangles += m.source_triangles as u64;
        self.pool_dirty = true;
        self.meshes.len() as u32 - 1
    }

    /// GPU bytes of the pool and the per-view lists (Profiler Memory module).
    pub fn memory_bytes(&self) -> u64 {
        self.pool_bytes() + self.gpu.as_ref().map_or(0, Gpu::list_bytes)
    }

    /// GPU bytes of the pool.
    fn pool_bytes(&self) -> u64 {
        (self.vertices.len() * 4
            + self.local_indices.len().next_multiple_of(2) * 2
            + self.cluster_vertices.len() * 4
            + self.clusters.len() * std::mem::size_of::<GpuCluster>()
            + (self.meshes.len() + self.groups.len()) * std::mem::size_of::<GpuRecord>())
            as u64
    }

    /// Make the pipelines and fixed buffers on first use.
    pub fn ensure_gpu(&mut self, device: &wgpu::Device, p: &Pipelines) {
        if self.gpu.is_some() {
            return;
        }
        self.gpu = Some(Gpu::new(device, p));
        self.pool_dirty = true;
    }

    /// Lay out this frame's instances, batches and views, and reset the counters.
    pub fn begin_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, input: FrameInputs) {
        self.frame = None;
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        if gpu.pool.is_none() {
            return;
        }
        let frame = input.frame;
        // Instances in draw order (stable between frames: last frame's visibility applies).
        // Draws of one pool mesh with the same transform are the parts of one object (a glTF
        // mesh's primitives): one instance, each draw in its material slot.
        let mut batch_of: HashMap<(usize, bool), u32> = HashMap::new();
        let mut batches = Vec::new();
        let mut cutout = Vec::new();
        // (draw, mesh, shadows, slot records) per instance.
        let mut objects: Vec<(u32, u32, bool, Vec<GpuInstance>)> = Vec::new();
        let mut object_of: HashMap<(u32, [u32; 16]), usize> = HashMap::new();
        for (i, m) in input.virtual_mesh.iter().enumerate() {
            let Some((mesh, slot)) = *m else {
                continue;
            };
            let slots = self.mesh_materials.get(mesh as usize).copied().unwrap_or(1);
            if slot >= slots {
                continue;
            }
            let key = ((input.batch_key)(i), (input.cutout)(i));
            let batch = *batch_of.entry(key).or_insert_with(|| {
                batches.push(i);
                cutout.push(key.1);
                batches.len() as u32 - 1
            });
            let shadows = frame.meshes[i].cast_shadows;
            let place = (
                mesh,
                frame.meshes[i].model.to_cols_array().map(f32::to_bits),
            );
            let o = match object_of.get(&place) {
                Some(&o) if objects[o].3[slot as usize][0] == NO_DRAW => o,
                _ => {
                    let empty = [NO_DRAW, 0, 0, 0];
                    objects.push((i as u32, mesh, false, vec![empty; slots as usize]));
                    object_of.insert(place, objects.len() - 1);
                    objects.len() - 1
                }
            };
            objects[o].2 |= shadows;
            objects[o].3[slot as usize] = [i as u32, batch, shadows as u32, 0];
        }
        if objects.is_empty() {
            return;
        }
        let n = objects.len();
        let mut instances: Vec<GpuInstance> = Vec::with_capacity(2 * n);
        let mut slot_records: Vec<GpuInstance> = Vec::new();
        for (draw, mesh, shadows, slots) in &objects {
            instances.push([
                *draw,
                *mesh,
                (n + slot_records.len()) as u32,
                *shadows as u32,
            ]);
            slot_records.extend_from_slice(slots);
        }
        instances.extend(slot_records);
        let b = batches.len();
        let views = 2 + input.shadow_matrices.len().min(MAX_SHADOW_VIEWS);
        // Grow the per-instance, per-batch and per-view buffers.
        if instances.len() > gpu.instance_capacity {
            let cap = instances.len().next_power_of_two();
            gpu.instances = storage(
                device,
                "misty vgeo instances",
                cap as u64 * 16,
                wgpu::BufferUsages::empty(),
            );
            gpu.instance_capacity = cap;
        }
        if b > gpu.batch_capacity {
            let cap = b.next_power_of_two();
            gpu.control = control_buffer(device, cap);
            gpu.batch_capacity = cap;
        }
        if views > gpu.view_capacity {
            gpu.grow_views(device, views.max(6));
        }
        // 4 words per instance, then 2 per instance and view (views are culled together).
        if n > gpu.state_capacity || gpu.view_capacity > gpu.state_views {
            let cap = n.next_power_of_two().max(gpu.state_capacity);
            gpu.instance_state = storage(
                device,
                "misty vgeo instance state",
                cap as u64 * (16 + 8 * gpu.view_capacity as u64),
                wgpu::BufferUsages::empty(),
            );
            gpu.state_capacity = cap;
            gpu.state_views = gpu.view_capacity;
            self.last_instances = usize::MAX;
        }
        queue.write_buffer(&gpu.instances, 0, bytemuck::cast_slice(&instances));
        if n != self.last_instances {
            // A new set of instances starts as "all visible" (drawn in the first phase).
            let state: Vec<u32> = (0..4 * n).map(|i| (i % 4 == 0) as u32).collect();
            queue.write_buffer(&gpu.instance_state, 0, bytemuck::cast_slice(&state));
            self.last_instances = n;
        }
        let (limits, args) = control_layout(gpu.batch_capacity);
        // The counters and the dispatch arguments (cs_prepare takes maxima into them).
        queue.write_buffer(&gpu.control, 0, &[0u8; CONTROL_HEADER * 4]);
        let (regions, _) = regions(gpu.view_capacity);
        // Each batch gets an equal share of a view's visible list (the indexed draw needs its
        // region's offset on the CPU).
        queue.write_buffer(
            &gpu.info,
            0,
            bytemuck::cast_slice(&[
                MAIN_VISIBLE,
                MAIN_VISIBLE / b as u32,
                self.view_debug.code(),
                0,
            ]),
        );

        // One main phase (frustum and level of detail only) or two around the Hi-Z pyramid.
        let single = match self.occlusion {
            VgeoChoice::On => false,
            VgeoChoice::Off => true,
            VgeoChoice::Auto => !input.gpu_culling && n < OCCLUSION_MIN_INSTANCES,
        };
        // Level of detail for the main camera, in every view.
        let camera = frame.camera;
        let (scale, ortho) = match camera.projection {
            Projection::Perspective { fov_y } => (
                input.size.1 as f32 / (2.0 * (fov_y.to_radians() * 0.5).tan()),
                false,
            ),
            Projection::Orthographic { size } => {
                (input.size.1 as f32 / (2.0 * size.max(1e-4)), true)
            }
        };
        let uniform = |view: usize, matrix: &Mat4, mode: u32, mips: u32| {
            let r = regions[view];
            ViewUniform {
                view_proj: matrix.to_cols_array_2d(),
                planes: Frustum::from_matrix(matrix).planes().map(|p| p.to_array()),
                camera: [
                    camera.position.x,
                    camera.position.y,
                    camera.position.z,
                    scale,
                ],
                params: [
                    self.threshold.max(0.05),
                    if ortho { 1.0 } else { 0.0 },
                    input.size.0 as f32,
                    input.size.1 as f32,
                ],
                info: [n as u32, mode, mips, b as u32],
                region: [
                    view as u32,
                    r.visible_first,
                    r.visible / b as u32,
                    r.chunk_first,
                ],
                caps: [r.chunks, gpu.state_capacity as u32, limits, args],
                eye: view_eye(matrix),
                groups: [r.group_first, r.groups, 0, 0],
            }
        };
        let mut uniforms = vec![
            uniform(0, &input.view_proj, if single { 3 } else { 0 }, 0),
            uniform(1, &input.view_proj, 1, input.state.mips()),
        ];
        for (i, m) in input
            .shadow_matrices
            .iter()
            .take(MAX_SHADOW_VIEWS)
            .enumerate()
        {
            let mut u = uniform(2 + i, m, 2, 0);
            // A cascade can't show less than a texel: the coarser of that and the camera's
            // level of detail (params.zw is the target size, unused without Hi-Z).
            u.params[2] = input.shadow_texels.get(i).copied().flatten().unwrap_or(0.0);
            u.params[3] = 0.0;
            uniforms.push(u);
        }
        queue.write_buffer(&gpu.views, 0, bytemuck::cast_slice(&uniforms));

        let Some(pool) = gpu.pool.as_ref() else {
            return;
        };
        fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        let cull_groups = (0..views)
            .map(|v| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("misty vgeo cull group"),
                    layout: &gpu.cull_layout,
                    entries: &[
                        entry(0, &gpu.uniforms[v]),
                        entry(1, input.draws),
                        entry(2, &gpu.instances),
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: pool.binding(MESHES),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: pool.binding(CLUSTERS),
                        },
                        entry(5, &gpu.instance_state),
                        entry(6, &gpu.lists),
                        entry(7, &gpu.control),
                        entry(8, &gpu.cluster_args),
                        entry(10, &gpu.views),
                        wgpu::BindGroupEntry {
                            binding: 9,
                            resource: wgpu::BindingResource::TextureView(
                                input.state.pyramid_view(),
                            ),
                        },
                    ],
                })
            })
            .collect();
        let render_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vgeo render group"),
            layout: &gpu.render_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pool.binding(VERTICES),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: pool.binding(LOCAL),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: pool.binding(CLUSTERS),
                },
                entry(3, &gpu.lists),
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: pool.binding(CLUSTER_VERTICES),
                },
                entry(5, &gpu.info),
            ],
        });
        let plain = self.view_debug == VgeoDebug::Off;
        // Auto: by the main camera's triangles a frame or two ago, per pixel and batch.
        let pixels = (input.size.0 * input.size.1) as f32 * b as f32;
        let triangles = self.latest.triangles as f32;
        self.vis_auto = match self.vis_auto {
            true => triangles >= VIS_OFF_TRIANGLES * pixels,
            false => triangles >= VIS_ON_TRIANGLES * pixels,
        };
        let wanted = match self.visibility {
            VgeoChoice::On => true,
            VgeoChoice::Off => false,
            VgeoChoice::Auto => self.vis_auto,
        };
        let vis = match wanted && plain && b <= vis::MAX_BATCHES {
            true => gpu
                .vis
                .as_mut()
                .map(|v| v.begin_frame(device, queue, pool, input.size, self.edge_layer)),
            false => None,
        };
        self.frame = Some(FrameState {
            views,
            regions,
            shadow_slots: (0..views - 2).collect(),
            batches,
            cutout,
            instances: n as u32,
            cull_groups,
            render_group,
            vis,
            single,
        });
    }

    /// The main camera's clusters are culled in one phase this frame (no Hi-Z occlusion):
    /// the main pass draws them all with the first phase's meshes and isn't split for them.
    pub fn single_phase(&self) -> bool {
        self.frame.as_ref().is_some_and(|f| f.single)
    }

    /// Is there anything to draw this frame?
    pub fn active(&self) -> bool {
        self.frame.is_some()
    }

    /// Run the culling passes of views `first..last` (0, 1 = main phases; 2.. = shadow maps);
    /// `gate(view)` false skips a view (the Frame Debugger stopped before it).
    pub fn cull(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        first: usize,
        last: usize,
        gate: &dyn Fn(usize) -> bool,
    ) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        let last = last.min(f.views);
        if first >= last {
            return;
        }
        let batches = f.batches.len() as u32;
        let label = Some("misty vgeo culling");
        // With one main phase there is no view 1.
        let first = if first == 1 && f.single { 2 } else { first };
        if first >= last {
            return;
        }
        // The views the Frame Debugger lets through (it stops a frame at one event: every
        // view after a refused one is refused too).
        let mut run = 0;
        for v in first..last {
            if !gate(v) {
                break;
            }
            run += 1;
        }
        if run == 0 {
            return;
        }
        // Every view of the run in one dispatch per pass (y or z = the view): five dispatches
        // and one copy for all the shadow maps instead of five and one per map.
        let views = run as u32;
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label,
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &f.cull_groups[first], &[]);
            pass.set_pipeline(&gpu.cs_instances);
            pass.dispatch_workgroups(f.instances.div_ceil(WORKGROUP), views, 1);
            pass.set_pipeline(&gpu.cs_prepare);
            pass.dispatch_workgroups(views, 1, 1);
        }
        // The run's cluster and group dispatches (in its first view's 8 words after the
        // counters).
        let at = (first * 32) as u64;
        encoder.copy_buffer_to_buffer(
            &gpu.control,
            (MAX_VIEWS * COUNTERS * 4) as u64 + at,
            &gpu.dispatch,
            at,
            32,
        );
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label,
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &f.cull_groups[first], &[]);
        // Every candidate group's decision first: the clusters read them.
        pass.set_pipeline(&gpu.cs_groups);
        pass.dispatch_workgroups_indirect(&gpu.dispatch, at + 16);
        pass.set_pipeline(&gpu.cs_clusters);
        pass.dispatch_workgroups_indirect(&gpu.dispatch, at);
        pass.set_pipeline(&gpu.cs_finish);
        pass.dispatch_workgroups(batches.div_ceil(WORKGROUP), views, 1);
    }

    /// Shadow views to cull (after the main phases).
    pub fn shadow_views(&self) -> usize {
        self.frame.as_ref().map_or(0, |f| f.views - 2)
    }

    /// Draw view `view`'s clusters, batch by batch; `setup(pass, batch, draw, cutout)` sets the
    /// batch's pipeline and group 2 (false = skip the batch), after `gate` agreed.
    fn draw_view(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        view: usize,
        kind: VgeoPass,
        gate: &Gate<'_>,
        setup: &mut BatchSetup<'_>,
    ) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        pass.set_bind_group(3, &f.render_group, &[]);
        let b = f.batches.len();
        let args = control_layout(gpu.batch_capacity).1 as usize * 4;
        let region = f.regions[view];
        let per_batch = (region.visible as usize) / b;
        if gpu.indexed
            && let Some(pool) = gpu.pool.as_ref()
        {
            let (at, size) = pool.sections[LOCAL];
            pass.set_index_buffer(pool.buffer.slice(at..at + size), wgpu::IndexFormat::Uint16);
        }
        for (i, &draw) in f.batches.iter().enumerate() {
            let cutout = f.cutout[i];
            let what = VgeoDraw {
                pass: kind,
                view,
                batch: i,
                draw,
                cutout,
            };
            if !gate(&what) || !setup(pass, i, draw, cutout) {
                continue;
            }
            // The batch's cluster count (instance count of its pulled-vertex arguments).
            let count = args + (view * b + i) * 16;
            if gpu.indexed {
                // One indexed sub-draw per cluster of the batch's region, as many as it drew.
                let first = region.visible_first as usize + i * per_batch;
                pass.multi_draw_indexed_indirect_count(
                    &gpu.cluster_args,
                    (first * 20) as u64,
                    &gpu.control,
                    (count + 4) as u64,
                    per_batch as u32,
                );
            } else {
                pass.draw_indirect(&gpu.control, count as u64);
            }
        }
    }

    /// Main pass: the clusters of phase `phase` (group 1 = the lit group, already bound).
    pub fn draw_main<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        phase: usize,
        material: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
        gate: &Gate<'_>,
    ) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let debug = self.view_debug;
        let mut setup = |pass: &mut wgpu::RenderPass<'_>, _: usize, draw: usize, cutout: bool| {
            let Some(group) = material(draw) else {
                return false;
            };
            pass.set_pipeline(match (debug, cutout) {
                (VgeoDebug::Overdraw, _) => &gpu.overdraw,
                (VgeoDebug::Off, false) => &gpu.main,
                (VgeoDebug::Off, true) => &gpu.main_cutout,
                _ => &gpu.debug,
            });
            pass.set_bind_group(2, group, &[]);
            true
        };
        self.draw_view(pass, phase, VgeoPass::Lit, gate, &mut setup);
    }

    /// Does the main pass draw this frame's clusters through the visibility buffer
    /// ([`Self::raster_visibility`] between two parts of the pass, then [`Self::resolve`])?
    pub fn visibility_active(&self) -> bool {
        self.frame.as_ref().is_some_and(|f| f.vis.is_some())
    }

    /// The visibility raster of phase `phase`'s clusters into the main pass's MSAA `depth`
    /// (its own render pass; groups: the main view, the draws).
    pub fn raster_visibility<'a>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        phase: usize,
        groups: [&wgpu::BindGroup; 3],
        material: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
        gate: &Gate<'_>,
    ) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        let (Some(vis), Some(vf)) = (gpu.vis.as_ref(), f.vis.as_ref()) else {
            return;
        };
        let mut pass = vis.begin_raster(encoder, vf, depth);
        for (i, g) in groups.iter().enumerate() {
            pass.set_bind_group(i as u32, *g, &[]);
        }
        let empty = groups[2];
        let mut setup = |pass: &mut wgpu::RenderPass<'_>, _: usize, draw: usize, cutout: bool| {
            // Cutout batches test their albedo alpha (group 2 = their texture set).
            let group = match cutout {
                true => material(draw),
                false => Some(empty),
            };
            let Some(group) = group else {
                return false;
            };
            vis.set_raster(pass, cutout);
            pass.set_bind_group(2, group, &[]);
            true
        };
        self.draw_view(&mut pass, phase, VgeoPass::Raster, gate, &mut setup);
    }

    /// The material resolve of the last visibility raster, in the main pass (groups 0 and 1
    /// bound).
    pub fn resolve<'a>(
        &self,
        pass: &mut wgpu::RenderPass,
        phase: usize,
        material: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
        gate: &Gate<'_>,
    ) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        if let (Some(vis), Some(vf)) = (gpu.vis.as_ref(), f.vis.as_ref()) {
            let keep = |batch: usize, draw: usize, cutout: bool| {
                gate(&VgeoDraw {
                    pass: VgeoPass::Resolve,
                    view: phase,
                    batch,
                    draw,
                    cutout,
                })
            };
            vis.resolve(pass, vf, &f.batches, &f.cutout, material, &keep);
        }
    }

    /// The pipeline and group 2 of a depth-only pass's batch: opaque ones `opaque` with the
    /// empty group, cutout ones `cutout` with their texture set.
    fn depth_setup<'s, 'm: 's>(
        opaque: &'s wgpu::RenderPipeline,
        cutout: &'s wgpu::RenderPipeline,
        empty: &'m wgpu::BindGroup,
        material: &'s (dyn Fn(usize) -> Option<&'m wgpu::BindGroup> + 's),
    ) -> impl FnMut(&mut wgpu::RenderPass<'_>, usize, usize, bool) -> bool + 's {
        move |pass: &mut wgpu::RenderPass<'_>, _: usize, draw: usize, is_cutout: bool| {
            let group = match is_cutout {
                true => material(draw),
                false => Some(empty),
            };
            let Some(group) = group else {
                return false;
            };
            pass.set_pipeline(if is_cutout { cutout } else { opaque });
            pass.set_bind_group(2, group, &[]);
            true
        }
    }

    /// Shadow map `slot` (groups 0 and 1 already bound by the shadow pass).
    pub fn draw_shadow<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: usize,
        empty: &'a wgpu::BindGroup,
        material: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
        gate: &Gate<'_>,
    ) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        let Some(i) = f.shadow_slots.iter().position(|s| *s == slot) else {
            return;
        };
        let mut setup = Self::depth_setup(&gpu.shadow, &gpu.shadow_cutout, empty, material);
        self.draw_view(pass, 2 + i, VgeoPass::Shadow, gate, &mut setup);
    }

    /// The screen-space effects' prepass: the first phase's clusters.
    pub fn draw_prepass<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        empty: &'a wgpu::BindGroup,
        material: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
        gate: &Gate<'_>,
    ) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if self.frame.is_none() {
            return;
        }
        let mut setup = Self::depth_setup(&gpu.prepass, &gpu.prepass_cutout, empty, material);
        self.draw_view(pass, 0, VgeoPass::Prepass, gate, &mut setup);
    }

    /// Copy this frame's per-view counters into a free staging buffer (read back later by
    /// [`Self::collect_stats`], never waited for).
    pub fn copy_stats(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        use std::sync::atomic::Ordering;
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        let size = (MAX_VIEWS * COUNTERS * 4) as u64;
        let free = self
            .readback
            .iter()
            .position(|r| r.state.load(Ordering::Acquire) == 0);
        let i = match free {
            Some(i) => i,
            // Three in flight at most: skip a frame rather than wait.
            None if self.readback.len() >= 3 => return,
            None => {
                self.readback.push(Readback {
                    buffer: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("misty vgeo counters readback"),
                        size,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    }),
                    state: Default::default(),
                    views: 0,
                    instances: 0,
                    visibility: false,
                    occlusion: false,
                });
                self.readback.len() - 1
            }
        };
        let r = &mut self.readback[i];
        encoder.copy_buffer_to_buffer(&gpu.control, 0, &r.buffer, 0, size);
        r.views = f.views;
        r.instances = f.instances;
        r.visibility = f.vis.is_some();
        r.occlusion = !f.single;
        r.state.store(1, Ordering::Release);
    }

    /// After the frame's submit: start mapping the counters it copied.
    pub fn stats_submitted(&self) {
        use std::sync::atomic::Ordering;
        for r in &self.readback {
            if r.state.load(Ordering::Acquire) == 1 {
                r.state.store(2, Ordering::Release);
                let state = r.state.clone();
                r.buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        state.store(if result.is_ok() { 3 } else { 0 }, Ordering::Release);
                    });
            }
        }
    }

    /// Take the newest counters that have arrived and return the latest known statistics
    /// (with the pool's and the lists' memory).
    pub fn collect_stats(&mut self) -> VgeoStats {
        use std::sync::atomic::Ordering;
        let mut newest = None;
        for r in &self.readback {
            if r.state.load(Ordering::Acquire) != 3 {
                continue;
            }
            let mut stats = VgeoStats {
                instances: r.instances,
                visibility: r.visibility,
                occlusion: r.occlusion,
                ..Default::default()
            };
            if let Ok(data) = r.buffer.slice(..).get_mapped_range() {
                stats.add_counters(bytemuck::cast_slice(&data[..]), r.views);
            }
            r.buffer.unmap();
            r.state.store(0, Ordering::Release);
            newest = Some(stats);
        }
        if let Some(s) = newest {
            self.latest = s;
        }
        if !self.on || self.frame.is_none() {
            self.latest = VgeoStats::default();
        }
        VgeoStats {
            meshes: self.meshes.len() as u32,
            source_triangles: self.source_triangles,
            pool_bytes: self.pool_bytes(),
            list_bytes: self.gpu.as_ref().map_or(0, Gpu::list_bytes),
            ..self.latest
        }
    }

    /// What the last frame drew (blocking readback: measurements only).
    pub fn read_stats(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> VgeoStats {
        let mut stats = VgeoStats {
            meshes: self.meshes.len() as u32,
            source_triangles: self.source_triangles,
            pool_bytes: self.pool_bytes(),
            list_bytes: self.gpu.as_ref().map_or(0, Gpu::list_bytes),
            ..Default::default()
        };
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return stats;
        };
        stats.instances = f.instances;
        stats.visibility = f.vis.is_some();
        stats.occlusion = !f.single;
        let size = (MAX_VIEWS * COUNTERS * 4) as u64;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vgeo stats readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&gpu.control, 0, &read, 0, size);
        queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let Ok(data) = slice.get_mapped_range() else {
            return stats;
        };
        let words: &[u32] = bytemuck::cast_slice(&data[..]);
        stats.add_counters(words, f.views);
        stats
    }
}

impl VgeoStats {
    /// Add the per-view counters at the start of `control` (`views` views).
    fn add_counters(&mut self, words: &[u32], views: usize) {
        for v in 0..views.min(words.len() / COUNTERS) {
            let c = &words[v * COUNTERS..(v + 1) * COUNTERS];
            if v < 2 {
                self.triangles += c[1] as u64;
                self.clusters += c[2];
            } else {
                self.shadow_triangles += c[1] as u64;
            }
            self.dropped += c[3];
            self.dropped_instances += c[5];
        }
    }
}

impl Gpu {
    fn new(device: &wgpu::Device, p: &Pipelines) -> Self {
        use wgpu::ShaderStages as S;
        let storage_entry = |binding, read_only, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut cull_entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: S::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        for binding in 1..=4 {
            cull_entries.push(storage_entry(binding, true, S::COMPUTE));
        }
        for binding in 5..=8 {
            cull_entries.push(storage_entry(binding, false, S::COMPUTE));
        }
        cull_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 10,
            visibility: S::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
        cull_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 9,
            visibility: S::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        let cull_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty vgeo cull"),
            entries: &cull_entries,
        });
        let mut render_entries: Vec<_> =
            (0..5).map(|b| storage_entry(b, true, S::VERTEX)).collect();
        // Visible entries per view and per batch (the visibility raster's batch ids).
        render_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 5,
            visibility: S::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
        let render_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty vgeo render"),
            entries: &render_entries,
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty vgeo cull"),
            source: wgpu::ShaderSource::Wgsl(CULL_SRC.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty vgeo cull"),
            bind_group_layouts: &[Some(&cull_layout)],
            immediate_size: 0,
        });
        let compute = |entry| {
            super::pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: crate::render::pipeline_cache(device).as_ref(),
            })
        };

        // Indexed sub-draws per cluster where the device can take their count from the GPU.
        let indexed = device
            .features()
            .contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT);
        let entry = |name: &'static str, ix: &'static str| if indexed { ix } else { name };
        let mesh_src = format!("{MESH_SRC}\n{FETCH_SRC}\n{MESH_ENTRY_SRC}");
        let mesh_layouts = [
            &p.view_layout,
            &p.mesh_layout,
            &p.material_layout,
            &render_layout,
        ];
        let main_spec = |vs, fs| Spec {
            vs,
            fs: Some(fs),
            cull: Some(wgpu::Face::Back),
            depth: Depth::main(true, wgpu::CompareFunction::Less),
            glow: Some(true),
            velocity: true,
            ..Spec::new("vgeo mesh", &mesh_src, &mesh_layouts)
        };
        let main = pipelines::build(
            device,
            main_spec(entry("vs_cluster", "vs_cluster_indexed"), "fs_main"),
        );
        let main_cutout = pipelines::build(
            device,
            main_spec(entry("vs_cluster", "vs_cluster_indexed"), "fs_cutout"),
        );
        let debug_vs = entry("vs_cluster_debug", "vs_cluster_debug_indexed");
        let debug = pipelines::build(device, main_spec(debug_vs, "fs_cluster_debug"));
        // Overdraw: every layer adds up (no depth test, additive colour).
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let overdraw = pipelines::build(
            device,
            Spec {
                depth: Depth::main(false, wgpu::CompareFunction::Always),
                blend: Some(wgpu::BlendState {
                    color: add,
                    alpha: add,
                }),
                glow: Some(false),
                ..main_spec(debug_vs, "fs_cluster_overdraw")
            },
        );

        let depth_src = format!("{DEPTH_SRC}\n{FETCH_SRC}\n{DEPTH_ENTRY_SRC}");
        let shadow_build = |vs, fs, group2: &[&wgpu::BindGroupLayout]| {
            pipelines::build(
                device,
                Spec {
                    vs,
                    fs,
                    cull: Some(wgpu::Face::Back),
                    depth: Some(Depth {
                        format: SHADOW_FORMAT,
                        write: true,
                        compare: wgpu::CompareFunction::LessEqual,
                        bias: wgpu::DepthBiasState {
                            constant: 1,
                            slope_scale: 1.0,
                            clamp: 0.0,
                        },
                    }),
                    color: None,
                    samples: 1,
                    glow: None,
                    ..Spec::new("vgeo shadow", &depth_src, group2)
                },
            )
        };
        let shadow_layouts = [
            &p.shadow_view_layout,
            &p.draws_layout,
            &p.empty_layout,
            &render_layout,
        ];
        let shadow_cutout_layouts = [
            &p.shadow_view_layout,
            &p.draws_layout,
            &p.material_layout,
            &render_layout,
        ];
        let shadow = shadow_build(
            entry("vs_cluster_depth", "vs_cluster_depth_indexed"),
            None,
            &shadow_layouts,
        );
        let shadow_cutout = shadow_build(
            entry("vs_cluster_depth_uv", "vs_cluster_depth_uv_indexed"),
            Some("fs_shadow_cutout"),
            &shadow_cutout_layouts,
        );
        let prepass_src = format!(
            "{}\n{FETCH_SRC}\n{PREPASS_ENTRY_SRC}",
            super::screen_space::PREPASS_SRC
        );
        let prepass_build = |fs, group2: &[&wgpu::BindGroupLayout]| {
            pipelines::build(
                device,
                Spec {
                    vs: entry("vs_cluster_prepass", "vs_cluster_prepass_indexed"),
                    fs: Some(fs),
                    cull: Some(wgpu::Face::Back),
                    depth: Some(Depth {
                        format: super::DEPTH_FORMAT,
                        write: true,
                        compare: wgpu::CompareFunction::Less,
                        bias: Default::default(),
                    }),
                    color: Some(super::screen_space::NORMAL_FORMAT),
                    samples: 1,
                    glow: None,
                    velocity: true,
                    ..Spec::new("vgeo prepass", &prepass_src, group2)
                },
            )
        };
        let prepass_layouts = [
            &p.view_layout,
            &p.draws_layout,
            &p.empty_layout,
            &render_layout,
        ];
        let prepass_cutout_layouts = [
            &p.view_layout,
            &p.draws_layout,
            &p.material_layout,
            &render_layout,
        ];
        let prepass = prepass_build("fs_normal", &prepass_layouts);
        let prepass_cutout = prepass_build("fs_normal_cutout", &prepass_cutout_layouts);

        let none = wgpu::BufferUsages::empty();
        let vis = vis::Vis::new(device, p, &render_layout, indexed);
        Self {
            cs_instances: compute("cs_instances"),
            cs_prepare: compute("cs_prepare"),
            cs_groups: compute("cs_groups"),
            cs_clusters: compute("cs_clusters"),
            cs_finish: compute("cs_finish"),
            cull_layout,
            render_layout,
            indexed,
            main,
            main_cutout,
            debug,
            overdraw,
            shadow,
            shadow_cutout,
            prepass,
            prepass_cutout,
            pool: None,
            instances: storage(device, "misty vgeo instances", 16, none),
            instance_state: storage(device, "misty vgeo instance state", 16, none),
            control: control_buffer(device, 1),
            dispatch: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty vgeo dispatch"),
                size: MAX_VIEWS as u64 * 32,
                usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            lists: storage(device, "misty vgeo lists", 16, none),
            cluster_args: storage(
                device,
                "misty vgeo cluster args",
                16,
                wgpu::BufferUsages::INDIRECT,
            ),
            uniforms: Vec::new(),
            views: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty vgeo views"),
                size: (MAX_VIEWS * std::mem::size_of::<ViewUniform>()) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            view_capacity: 0,
            info: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty vgeo info"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            vis,
            instance_capacity: 0,
            state_capacity: 0,
            state_views: 0,
            batch_capacity: 0,
        }
    }

    /// Lay the lists, draw arguments and view uniforms out for `views` views.
    fn grow_views(&mut self, device: &wgpu::Device, views: usize) {
        let views = views.min(MAX_VIEWS);
        let (_, entries) = regions(views);
        self.lists = storage(
            device,
            "misty vgeo lists",
            entries as u64 * 8,
            wgpu::BufferUsages::empty(),
        );
        self.cluster_args = storage(
            device,
            "misty vgeo cluster args",
            visible_entries(views) as u64 * 20,
            wgpu::BufferUsages::INDIRECT,
        );
        while self.uniforms.len() < views {
            let v = self.uniforms.len() as u32;
            self.uniforms.push(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("misty vgeo view range"),
                    contents: bytemuck::cast_slice(&[v, 0, 0, 0]),
                    usage: wgpu::BufferUsages::UNIFORM,
                }),
            );
        }
        self.view_capacity = views;
    }

    /// GPU bytes of the per-view lists and draw arguments.
    fn list_bytes(&self) -> u64 {
        self.lists.size() + self.cluster_args.size()
    }
}

impl super::Renderer {
    /// Lay out this frame's virtual geometry (after the draws are uploaded): instances,
    /// batches, the main camera's two phases and the shadow maps.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::render) fn begin_vgeo(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &super::RenderTarget,
        frame: &Frame,
        lists: &super::frame_lists::FrameLists,
        view_proj: &Mat4,
        plan: &super::shadows::ShadowPlan,
        gpu_culling: bool,
    ) {
        if !self.vgeo.on || !lists.prep.virtual_mesh.iter().any(Option::is_some) {
            self.vgeo.frame = None;
            return;
        }
        let size = target.size();
        let mut state = target.culling.lock().unwrap_or_else(|e| e.into_inner());
        if !state.as_ref().is_some_and(|s| s.fits(size)) {
            *state = Some(TargetState::new(device, size));
        }
        let Some(state) = state.as_ref() else {
            return;
        };
        let textures = &self.textures;
        let batch_key = |i: usize| {
            textures
                .get_set(&frame.meshes[i].material.textures())
                .map_or(0, super::batch::addr)
        };
        let cutout = |i: usize| lists.prep.pipeline[i].mode == super::RenderingMode::Cutout;
        self.vgeo.begin_frame(
            device,
            queue,
            FrameInputs {
                frame,
                virtual_mesh: &lists.prep.virtual_mesh,
                draws: &self.draws_buffer,
                view_proj: *view_proj,
                size,
                shadow_matrices: plan.matrices().copied().collect(),
                shadow_texels: plan
                    .matrices()
                    .enumerate()
                    .map(|(i, m)| {
                        // Orthographic: row 0 maps world x to -1..1 over the map's width.
                        let size = self.shadows.directional_size as f32;
                        if i < plan.cascades.len() {
                            return Some(0.5 * size * m.row(0).truncate().length());
                        }
                        // Virtual shadow map rectangles: their level's texel.
                        let rects = self.vsm.frame.as_ref().map(|f| &f.rects[..]);
                        let j = i.checked_sub(plan.vsm_first_slot())?;
                        rects?.get(j).map(|r| r.texels_per_metre)
                    })
                    .collect(),
                state,
                batch_key: &batch_key,
                cutout: &cutout,
                gpu_culling,
            },
        );
    }

    /// Draw virtual geometry's main-pass clusters through the visibility buffer (the default,
    /// where the device can) or lit directly (A/B measurements).
    /// `true` / `false` force it; [`VgeoChoice::Auto`] (the default) picks the visibility
    /// buffer once the clusters' triangles get small for the pixels and material batches.
    pub fn set_virtual_geometry_visibility(&mut self, choice: impl Into<VgeoChoice>) {
        self.vgeo.visibility = choice.into();
    }

    /// Cull the main camera's clusters in two phases around a Hi-Z pyramid (occlusion
    /// culling, `true`) or in one (`false`); [`VgeoChoice::Auto`] (the default) takes two
    /// from [`OCCLUSION_MIN_INSTANCES`] instances on or when GPU culling is on.
    pub fn set_virtual_geometry_occlusion(&mut self, choice: impl Into<VgeoChoice>) {
        self.vgeo.occlusion = choice.into();
    }

    /// Draw every cluster in its own colour (Misty Virtual Geometry debug view).
    pub fn set_virtual_geometry_debug(&mut self, on: bool) {
        self.vgeo.debug = match on {
            true => VgeoDebug::Clusters,
            false => VgeoDebug::Off,
        };
    }

    /// Virtual geometry's debug view for the next render only (the Scene view's draw modes;
    /// Off = the materials).
    pub fn set_virtual_geometry_debug_view(&mut self, view: VgeoDebug) {
        self.vgeo.debug_once = Some(view);
    }

    /// What virtual geometry drew last frame: clusters, triangles per view (blocking GPU
    /// readback, for measurements).
    pub fn virtual_geometry_stats(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> VgeoStats {
        self.vgeo.read_stats(device, queue)
    }
}

#[cfg(test)]
mod gpu_tests;
mod vis;

#[cfg(test)]
mod tests {
    use wgpu::naga;

    /// The culling and drawing shaders parse and validate.
    #[test]
    fn vgeo_shaders_validate() {
        let common = super::pipelines::COMMON;
        for (name, src) in [
            ("vgeo cull", super::CULL_SRC.to_owned()),
            (
                "vgeo mesh",
                format!(
                    "{common}\n{}\n{}\n{}",
                    super::MESH_SRC,
                    super::FETCH_SRC,
                    super::MESH_ENTRY_SRC
                ),
            ),
            (
                "vgeo depth",
                format!(
                    "{common}\n{}\n{}\n{}",
                    super::DEPTH_SRC,
                    super::FETCH_SRC,
                    super::DEPTH_ENTRY_SRC
                ),
            ),
            (
                "vgeo prepass",
                format!(
                    "{common}\n{}\n{}\n{}",
                    super::super::screen_space::PREPASS_SRC,
                    super::FETCH_SRC,
                    super::PREPASS_ENTRY_SRC
                ),
            ),
            (
                "vgeo visibility",
                format!(
                    "enable primitive_index;
{common}
{}
{}
{}",
                    super::super::screen_space::PREPASS_SRC,
                    super::FETCH_SRC,
                    include_str!("vgeo_vis.wgsl")
                ),
            ),
            (
                "vgeo resolve",
                format!(
                    "{common}
{}
{}",
                    super::MESH_SRC,
                    include_str!("vgeo_resolve.wgsl")
                ),
            ),
        ] {
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }

    #[test]
    fn gpu_structs_match_the_shaders() {
        // `DISPATCH_BASE` in vgeo_cull.wgsl.
        let base = super::MAX_VIEWS * super::COUNTERS;
        assert!(
            super::CULL_SRC.contains(&format!("const DISPATCH_BASE: u32 = {base}u;")),
            "DISPATCH_BASE should be {base}"
        );
        assert!(super::CULL_SRC.contains(&format!("MAX_VIEWS = {}", super::MAX_VIEWS)));
        // Every view's uniform in one array (views are culled together).
        assert!(super::CULL_SRC.contains(&format!(
            "var<uniform> views: array<View, {}>;",
            super::MAX_VIEWS
        )));
        assert_eq!(std::mem::size_of::<super::GpuCluster>(), 48);
        // vgeo_resolve.wgsl reads cluster records by words.
        let resolve = include_str!("vgeo_resolve.wgsl");
        assert!(resolve.contains("const VR_CLUSTER_WORDS: u32 = 12u;"));
        // The indexed path's base vertex (entry × 384) fits an i32 for every view.
        assert!(super::visible_entries(super::MAX_VIEWS) as u64 * 384 < i32::MAX as u64);
        assert_eq!(std::mem::size_of::<super::GpuRecord>(), 32);
        assert_eq!(std::mem::size_of::<super::GpuInstance>(), 16);
        // mat4 + 6 planes + 7 vec4s.
        assert_eq!(std::mem::size_of::<super::ViewUniform>(), 64 + 96 + 112);
    }

    #[test]
    fn view_regions_never_overlap() {
        let (regions, entries) = super::regions(super::MAX_VIEWS);
        let mut spans: Vec<(u32, u32)> = Vec::new();
        for r in &regions {
            spans.push((r.visible_first, r.visible));
            spans.push((r.chunk_first, r.chunks));
            spans.push((r.group_first, r.groups));
        }
        spans.sort_unstable();
        assert_eq!(spans[0].0, 0);
        for w in spans.windows(2) {
            assert_eq!(w[0].0 + w[0].1, w[1].0, "gap or overlap");
        }
        let last = spans[spans.len() - 1];
        assert_eq!(last.0 + last.1, entries);
        // Visible lists first: the draw arguments index them from 0.
        let visible: u32 = regions.iter().map(|r| r.visible).sum();
        assert_eq!(visible, super::visible_entries(super::MAX_VIEWS));
        assert!(regions.iter().all(|r| r.visible_first < visible));
        // Growing keeps the visible lists where they were.
        let (small, _) = super::regions(6);
        for (a, b) in small.iter().zip(&regions) {
            assert_eq!((a.visible_first, a.visible), (b.visible_first, b.visible));
        }
    }
}
