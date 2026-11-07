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
//! shadow map (frustum only, level of detail chosen for the main camera). The first phase's
//! list also feeds the depth + normal prepass of the screen-space effects.

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

/// Visible-list entries per view (× 128 triangles at most each), shared equally by the batches.
/// (Entry × 384 is the indexed path's base vertex: it must stay below 2³¹ for all views.)
const VISIBLE_PER_VIEW: u32 = 1 << 18;
/// Chunks of 64 candidate clusters per view.
const CHUNKS_PER_VIEW: u32 = 1 << 17;
/// Shadow maps with virtual geometry casters (the sun's cascades come first).
pub(super) const MAX_SHADOW_VIEWS: usize = 6;
const MAX_VIEWS: usize = 2 + MAX_SHADOW_VIEWS;
/// Words per vertex in the pool: position (3 × f32), normal and tangent (octahedral 2 × 16
/// bits each, the bitangent sign in the tangent's lowest bit), uv (2 × f32), lightmap uv
/// (2 × unorm16). [`super::mesh::Vertex`] is 14 floats.
const VERTEX_WORDS: usize = 8;
const WORKGROUP: u32 = 64;
/// Words of `control` before the limits: 4 counters and 4 dispatch words per view
/// (`DISPATCH_BASE` in vgeo_cull.wgsl = the counters' words).
const CONTROL_HEADER: usize = MAX_VIEWS * 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuCluster {
    bounds: [f32; 4],
    lod: [f32; 4],
    parent: [f32; 4],
    error: f32,
    parent_error: f32,
    triangles: u32,
    /// Its own vertex list (source vertex ids) and local indices (16-bit, in index units).
    vertex_first: u32,
    local_first: u32,
    /// Normal cone: octahedral axis and cutoff ([`crate::vgeo::Cluster::backfacing`]).
    cone_axis: u32,
    cone_cutoff: f32,
    pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMeshInfo {
    bounds: [f32; 4],
    first: u32,
    count: u32,
    pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuInstance {
    draw: u32,
    mesh: u32,
    batch: u32,
    flags: u32,
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
    /// Virtual meshes in the pool and their source triangles.
    pub meshes: u32,
    pub source_triangles: u64,
    /// GPU memory of the pool (vertices, local indices, cluster vertices, cluster records).
    pub pool_bytes: u64,
}

pub(super) struct VirtualGeometry {
    /// Quality ▸ Virtual Geometry.
    pub enabled: bool,
    /// Largest error on screen, in pixels.
    pub threshold: f32,
    /// Colour every cluster (debug view).
    pub debug: bool,
    /// This frame draws virtual meshes through clusters (set before the draw prep).
    pub on: bool,
    /// Mesh key → index in the pool (`None` = not a virtual mesh).
    pool_index: HashMap<String, Option<u32>>,
    generation: u64,
    /// [`crate::models::virtual_build::ready_generation`] the pool was laid out at.
    ready: u64,
    vertices: Vec<u32>,
    clusters: Vec<GpuCluster>,
    meshes: Vec<GpuMeshInfo>,
    /// Per cluster: its vertices' source ids, and its triangles as indices into that list.
    cluster_vertices: Vec<u32>,
    local_indices: Vec<u16>,
    source_triangles: u64,
    pool_dirty: bool,
    gpu: Option<Gpu>,
    frame: Option<FrameState>,
    last_instances: usize,
}

struct Gpu {
    cull_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
    cs_instances: wgpu::ComputePipeline,
    cs_prepare: wgpu::ComputePipeline,
    cs_clusters: wgpu::ComputePipeline,
    cs_finish: wgpu::ComputePipeline,
    main: wgpu::RenderPipeline,
    debug: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    prepass: wgpu::RenderPipeline,
    /// Draw each cluster as an indexed sub-draw with the GPU's count
    /// (`MULTI_DRAW_INDIRECT_COUNT`); else 384 pulled vertices per cluster.
    indexed: bool,
    /// Vertices, local indices (16-bit; the index buffer, read as storage by the pulled path),
    /// clusters, meshes, cluster vertices.
    pool: Option<[wgpu::Buffer; 5]>,
    instances: wgpu::Buffer,
    /// Per instance: visible last frame, drawn by the first phase.
    instance_state: wgpu::Buffer,
    /// Counters, dispatch arguments, limits and draw arguments (vgeo_cull.wgsl).
    control: wgpu::Buffer,
    /// The cluster passes' workgroups, copied out of `control` (a buffer bound for writing
    /// can't also be the indirect buffer of a dispatch).
    dispatch: wgpu::Buffer,
    /// Every view's visible list, then every view's chunks.
    lists: wgpu::Buffer,
    /// Per visible-list entry: that cluster's indexed draw arguments.
    cluster_args: wgpu::Buffer,
    uniforms: Vec<wgpu::Buffer>,
    instance_capacity: usize,
    batch_capacity: usize,
}

struct FrameState {
    views: usize,
    /// Shadow slot of view `2 + i`.
    shadow_slots: Vec<usize>,
    /// A draw of each batch (its material's texture set).
    batches: Vec<usize>,
    instances: u32,
    cull_groups: Vec<wgpu::BindGroup>,
    render_group: wgpu::BindGroup,
}

/// Everything [`VirtualGeometry::begin_frame`] needs from the renderer.
pub(super) struct FrameInputs<'a> {
    pub frame: &'a Frame<'a>,
    pub virtual_mesh: &'a [Option<u32>],
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
            debug: false,
            on: false,
            pool_index: HashMap::new(),
            generation: u64::MAX,
            ready: 0,
            vertices: Vec::new(),
            clusters: Vec::new(),
            meshes: Vec::new(),
            cluster_vertices: Vec::new(),
            local_indices: Vec::new(),
            source_triangles: 0,
            pool_dirty: false,
            gpu: None,
            frame: None,
            last_instances: usize::MAX,
        }
    }
}

impl VirtualGeometry {
    /// Pool index of a mesh drawn through clusters this frame.
    pub fn mesh_index(&self, r: &MeshRef) -> Option<u32> {
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
            self.vertices.clear();
            self.clusters.clear();
            self.meshes.clear();
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
            // Built at import (or still building: then the ordinary mesh path draws it).
            let index = crate::models::virtual_mesh(&d.mesh).and_then(|built| {
                let m = crate::models::mesh(&d.mesh)?;
                Some(self.add_to_pool(&m.vertices, &built))
            });
            self.pool_index.insert(key.clone(), index);
        }
        if self.pool_dirty
            && let Some(gpu) = self.gpu.as_mut()
        {
            let init = |label, contents: &[u8], usage| {
                let padded;
                let contents = if contents.len() < 16 {
                    padded = [0u8; 16];
                    &padded[..]
                } else {
                    contents
                };
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage,
                })
            };
            let storage = wgpu::BufferUsages::STORAGE;
            let cast = |v: &[u32]| bytemuck::cast_slice::<u32, u8>(v).to_vec();
            // 16-bit indices, padded to whole words (read as `array<u32>` by the shaders).
            let mut local: Vec<u8> = bytemuck::cast_slice(&self.local_indices).to_vec();
            local.resize(local.len().next_multiple_of(4), 0);
            gpu.pool = Some([
                init("misty vgeo vertices", &cast(&self.vertices), storage),
                init(
                    "misty vgeo local indices",
                    &local,
                    storage | wgpu::BufferUsages::INDEX,
                ),
                init(
                    "misty vgeo clusters",
                    bytemuck::cast_slice(&self.clusters),
                    storage,
                ),
                init(
                    "misty vgeo meshes",
                    bytemuck::cast_slice(&self.meshes),
                    storage,
                ),
                init(
                    "misty vgeo cluster vertices",
                    &cast(&self.cluster_vertices),
                    storage,
                ),
            ]);
            self.pool_dirty = false;
        }
    }

    fn add_to_pool(&mut self, vertices: &[super::mesh::Vertex], m: &VirtualMesh) -> u32 {
        let base = (self.vertices.len() / VERTEX_WORDS) as u32;
        for v in vertices {
            self.vertices.extend_from_slice(&pack_vertex(v));
        }
        let first = self.clusters.len() as u32;
        let sphere = |s: crate::vgeo::Sphere| [s.center.x, s.center.y, s.center.z, s.radius];
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
                lod: sphere(c.lod),
                parent: sphere(c.parent),
                error: c.error,
                parent_error: c.parent_error,
                triangles: c.triangles,
                vertex_first,
                local_first,
                cone_axis: octahedral(c.cone_axis),
                cone_cutoff: c.cone_cutoff,
                pad: 0,
            });
        }
        self.meshes.push(GpuMeshInfo {
            bounds: sphere(m.bounds),
            first,
            count: m.clusters.len() as u32,
            pad: [0; 2],
        });
        self.source_triangles += m.source_triangles as u64;
        self.pool_dirty = true;
        self.meshes.len() as u32 - 1
    }

    /// GPU bytes of the pool.
    fn pool_bytes(&self) -> u64 {
        (self.vertices.len() * 4
            + self.local_indices.len().next_multiple_of(2) * 2
            + self.cluster_vertices.len() * 4
            + self.clusters.len() * std::mem::size_of::<GpuCluster>()
            + self.meshes.len() * std::mem::size_of::<GpuMeshInfo>()) as u64
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
        let Some(pool) = gpu.pool.as_ref() else {
            return;
        };
        let frame = input.frame;
        // Instances in draw order (stable between frames: last frame's visibility applies).
        let mut batch_of: HashMap<usize, u32> = HashMap::new();
        let mut batches = Vec::new();
        let mut instances = Vec::new();
        for (i, m) in input.virtual_mesh.iter().enumerate() {
            let Some(mesh) = *m else {
                continue;
            };
            let key = (input.batch_key)(i);
            let batch = *batch_of.entry(key).or_insert_with(|| {
                batches.push(i);
                batches.len() as u32 - 1
            });
            instances.push(GpuInstance {
                draw: i as u32,
                mesh,
                batch,
                flags: frame.meshes[i].cast_shadows as u32,
            });
        }
        if instances.is_empty() {
            return;
        }
        let n = instances.len();
        let b = batches.len();
        let views = 2 + input.shadow_matrices.len().min(MAX_SHADOW_VIEWS);
        // Grow the per-instance and per-batch buffers.
        if n > gpu.instance_capacity {
            let cap = n.next_power_of_two();
            let bytes = cap as u64 * 4;
            gpu.instances = storage(
                device,
                "misty vgeo instances",
                cap as u64 * 16,
                wgpu::BufferUsages::empty(),
            );
            gpu.instance_state = storage(
                device,
                "misty vgeo instance state",
                2 * bytes,
                wgpu::BufferUsages::empty(),
            );
            gpu.instance_capacity = cap;
            self.last_instances = usize::MAX;
        }
        if b > gpu.batch_capacity {
            let cap = b.next_power_of_two();
            gpu.control = control_buffer(device, cap);
            gpu.batch_capacity = cap;
        }
        queue.write_buffer(&gpu.instances, 0, bytemuck::cast_slice(&instances));
        if n != self.last_instances {
            // A new set of instances starts as "all visible" (drawn in the first phase).
            let state: Vec<u32> = (0..2 * n).map(|i| (i % 2 == 0) as u32).collect();
            queue.write_buffer(&gpu.instance_state, 0, bytemuck::cast_slice(&state));
            self.last_instances = n;
        }
        let (limits, args) = control_layout(gpu.batch_capacity);
        queue.write_buffer(&gpu.control, 0, &[0u8; MAX_VIEWS * 16]);
        // Each batch gets an equal share of a view's visible list (the indexed draw needs its
        // region's offset on the CPU).
        let per_batch = VISIBLE_PER_VIEW / b as u32;

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
        let uniform = |view: usize, matrix: &Mat4, mode: u32, mips: u32| ViewUniform {
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
                view as u32 * VISIBLE_PER_VIEW,
                per_batch,
                MAX_VIEWS as u32 * VISIBLE_PER_VIEW + view as u32 * CHUNKS_PER_VIEW,
            ],
            caps: [CHUNKS_PER_VIEW, 0, limits, args],
            eye: view_eye(matrix),
        };
        let mut uniforms = vec![
            uniform(0, &input.view_proj, 0, 0),
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
        for (u, buffer) in uniforms.iter().zip(&gpu.uniforms) {
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(u));
        }

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
                        entry(3, &pool[3]),
                        entry(4, &pool[2]),
                        entry(5, &gpu.instance_state),
                        entry(6, &gpu.lists),
                        entry(7, &gpu.control),
                        entry(8, &gpu.cluster_args),
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
                entry(0, &pool[0]),
                entry(1, &pool[1]),
                entry(2, &pool[2]),
                entry(3, &gpu.lists),
                entry(4, &pool[4]),
            ],
        });
        self.frame = Some(FrameState {
            views,
            shadow_slots: (0..views - 2).collect(),
            batches,
            instances: n as u32,
            cull_groups,
            render_group,
        });
    }

    /// Is there anything to draw this frame?
    pub fn active(&self) -> bool {
        self.frame.is_some()
    }

    /// Run the culling passes of views `first..last` (0, 1 = main phases; 2.. = shadow maps).
    pub fn cull(&self, encoder: &mut wgpu::CommandEncoder, first: usize, last: usize) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        let last = last.min(f.views);
        if first >= last {
            return;
        }
        let batches = f.batches.len() as u32;
        let label = Some("misty vgeo culling");
        for v in first..last {
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label,
                    timestamp_writes: None,
                });
                pass.set_bind_group(0, &f.cull_groups[v], &[]);
                pass.set_pipeline(&gpu.cs_instances);
                pass.dispatch_workgroups(f.instances.div_ceil(WORKGROUP), 1, 1);
                pass.set_pipeline(&gpu.cs_prepare);
                pass.dispatch_workgroups(1, 1, 1);
            }
            let at = (v * 16) as u64;
            encoder.copy_buffer_to_buffer(
                &gpu.control,
                (MAX_VIEWS * 16) as u64 + at,
                &gpu.dispatch,
                at,
                16,
            );
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label,
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &f.cull_groups[v], &[]);
            pass.set_pipeline(&gpu.cs_clusters);
            pass.dispatch_workgroups_indirect(&gpu.dispatch, at);
            pass.set_pipeline(&gpu.cs_finish);
            pass.dispatch_workgroups(batches.div_ceil(WORKGROUP), 1, 1);
        }
    }

    /// Shadow views to cull (after the main phases).
    pub fn shadow_views(&self) -> usize {
        self.frame.as_ref().map_or(0, |f| f.views - 2)
    }

    fn draw_view<'a>(
        &self,
        pass: &mut wgpu::RenderPass,
        view: usize,
        group2: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
    ) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        pass.set_bind_group(3, &f.render_group, &[]);
        let b = f.batches.len();
        let args = control_layout(gpu.batch_capacity).1 as usize * 4;
        let per_batch = VISIBLE_PER_VIEW as usize / b;
        if gpu.indexed
            && let Some(pool) = gpu.pool.as_ref()
        {
            pass.set_index_buffer(pool[1].slice(..), wgpu::IndexFormat::Uint16);
        }
        for (i, &draw) in f.batches.iter().enumerate() {
            let Some(group) = group2(draw) else {
                continue;
            };
            pass.set_bind_group(2, group, &[]);
            // The batch's cluster count (instance count of its pulled-vertex arguments).
            let count = args + (view * b + i) * 16;
            if gpu.indexed {
                // One indexed sub-draw per cluster of the batch's region, as many as it drew.
                let first = view * VISIBLE_PER_VIEW as usize + i * per_batch;
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
        pass: &mut wgpu::RenderPass,
        phase: usize,
        material: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
    ) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        pass.set_pipeline(if self.debug { &gpu.debug } else { &gpu.main });
        self.draw_view(pass, phase, material);
    }

    /// Shadow map `slot` (groups 0 and 1 already bound by the shadow pass).
    pub fn draw_shadow(&self, pass: &mut wgpu::RenderPass, slot: usize, empty: &wgpu::BindGroup) {
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return;
        };
        let Some(i) = f.shadow_slots.iter().position(|s| *s == slot) else {
            return;
        };
        pass.set_pipeline(&gpu.shadow);
        self.draw_view(pass, 2 + i, &|_| Some(empty));
    }

    /// The screen-space effects' prepass: the first phase's clusters.
    pub fn draw_prepass(&self, pass: &mut wgpu::RenderPass, empty: &wgpu::BindGroup) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if self.frame.is_none() {
            return;
        }
        pass.set_pipeline(&gpu.prepass);
        self.draw_view(pass, 0, &|_| Some(empty));
    }

    /// What the last frame drew (blocking readback: measurements only).
    pub fn read_stats(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> VgeoStats {
        let mut stats = VgeoStats {
            meshes: self.meshes.len() as u32,
            source_triangles: self.source_triangles,
            pool_bytes: self.pool_bytes(),
            ..Default::default()
        };
        let (Some(gpu), Some(f)) = (self.gpu.as_ref(), self.frame.as_ref()) else {
            return stats;
        };
        stats.instances = f.instances;
        let size = (MAX_VIEWS * 16) as u64;
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
        for v in 0..f.views {
            let c = &words[v * 4..v * 4 + 4];
            if v < 2 {
                stats.triangles += c[1] as u64;
                stats.clusters += c[2];
            } else {
                stats.shadow_triangles += c[1] as u64;
            }
            stats.dropped += c[3];
        }
        stats
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
        let render_entries: Vec<_> = (0..5).map(|b| storage_entry(b, true, S::VERTEX)).collect();
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
                cache: None,
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
        let debug = pipelines::build(
            device,
            main_spec(
                entry("vs_cluster_debug", "vs_cluster_debug_indexed"),
                "fs_cluster_debug",
            ),
        );

        let depth_src = format!("{DEPTH_SRC}\n{FETCH_SRC}\n{DEPTH_ENTRY_SRC}");
        let shadow = pipelines::build(
            device,
            Spec {
                vs: entry("vs_cluster_depth", "vs_cluster_depth_indexed"),
                fs: None,
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
                ..Spec::new(
                    "vgeo shadow",
                    &depth_src,
                    &[
                        &p.shadow_view_layout,
                        &p.draws_layout,
                        &p.empty_layout,
                        &render_layout,
                    ],
                )
            },
        );
        let prepass_src = format!(
            "{}\n{FETCH_SRC}\n{PREPASS_ENTRY_SRC}",
            super::screen_space::PREPASS_SRC
        );
        let prepass = pipelines::build(
            device,
            Spec {
                vs: entry("vs_cluster_prepass", "vs_cluster_prepass_indexed"),
                fs: Some("fs_normal"),
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
                ..Spec::new(
                    "vgeo prepass",
                    &prepass_src,
                    &[
                        &p.view_layout,
                        &p.draws_layout,
                        &p.empty_layout,
                        &render_layout,
                    ],
                )
            },
        );

        let none = wgpu::BufferUsages::empty();
        let uniforms = (0..MAX_VIEWS)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("misty vgeo view"),
                    size: std::mem::size_of::<ViewUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let views = MAX_VIEWS as u64;
        Self {
            cs_instances: compute("cs_instances"),
            cs_prepare: compute("cs_prepare"),
            cs_clusters: compute("cs_clusters"),
            cs_finish: compute("cs_finish"),
            cull_layout,
            render_layout,
            indexed,
            main,
            debug,
            shadow,
            prepass,
            pool: None,
            instances: storage(device, "misty vgeo instances", 16, none),
            instance_state: storage(device, "misty vgeo instance state", 16, none),
            control: control_buffer(device, 1),
            dispatch: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty vgeo dispatch"),
                size: views * 16,
                usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            lists: storage(
                device,
                "misty vgeo lists",
                views * (VISIBLE_PER_VIEW + CHUNKS_PER_VIEW) as u64 * 8,
                none,
            ),
            cluster_args: storage(
                device,
                "misty vgeo cluster args",
                views * VISIBLE_PER_VIEW as u64 * 20,
                wgpu::BufferUsages::INDIRECT,
            ),
            uniforms,
            instance_capacity: 0,
            batch_capacity: 0,
        }
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
                        (i < plan.cascades.len()).then(|| 0.5 * size * m.row(0).truncate().length())
                    })
                    .collect(),
                state,
                batch_key: &batch_key,
            },
        );
    }

    /// Draw every cluster in its own colour (Misty Virtual Geometry debug view).
    pub fn set_virtual_geometry_debug(&mut self, on: bool) {
        self.vgeo.debug = on;
    }

    /// What virtual geometry drew last frame: clusters, triangles per view (blocking GPU
    /// readback, for measurements).
    pub fn virtual_geometry_stats(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> VgeoStats {
        self.vgeo.read_stats(device, queue)
    }
}

#[cfg(test)]
mod gpu_tests;

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
        assert_eq!(super::MAX_VIEWS * 4, 32);
        assert!(super::CULL_SRC.contains("const DISPATCH_BASE: u32 = 32u;"));
        assert_eq!(std::mem::size_of::<super::GpuCluster>(), 80);
        // The indexed path's base vertex (entry × 384) fits an i32 for every view.
        assert!((super::MAX_VIEWS as u64 * super::VISIBLE_PER_VIEW as u64) * 384 < i32::MAX as u64);
        assert_eq!(std::mem::size_of::<super::GpuMeshInfo>(), 32);
        assert_eq!(std::mem::size_of::<super::GpuInstance>(), 16);
        // mat4 + 6 planes + 6 vec4s.
        assert_eq!(std::mem::size_of::<super::ViewUniform>(), 64 + 96 + 96);
    }
}
