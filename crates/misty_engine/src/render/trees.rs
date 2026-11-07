//! Terrain trees: every placed tree drawn with GPU instancing, per prototype and level of
//! detail — the full mesh close up, the lighter mesh further out, the baked impostor far away
//! (dithered cross-fades between them, so nothing pops) — into the main pass, the screen-space
//! prepass and the sun's shadow cascades.
//!
//! Per frame the CPU sorts the trees of every terrain into instance lists (distance, frustum,
//! LOD) and uploads them into one buffer (32 bytes a tree); the meshes, textures and impostors
//! of a tree asset are uploaded once per build (`trees::store`). The wind comes from the
//! terrain's grass settings and the scene's Wind Zone, so trees and grass move with the same
//! gusts; the vertex stages evaluate it a frame ago too, for TAA's motion vectors.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::RenderStats;
use super::culling::{Aabb, Frustum};
use super::environment::Environment;
use super::pipelines::{Depth, Spec, build};
use crate::terrain::TerrainDraw;
use crate::terrain::trees::local_position;
use crate::trees::generate::{TreeMesh, TreeVertex};
use crate::trees::impostor;
use crate::trees::store::{self, Built};

/// The lit tree shader (same includes as the grass's, then the tree code).
const TREE_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/mesh.wgsl"),
    "\n",
    include_str!("shaders/shadows.wgsl"),
    "\n",
    include_str!("shadow_denoise/shaders/shadow_lookup.wgsl"),
    "\n",
    include_str!("area/area_light.wgsl"),
    "\n",
    include_str!("probes/probes.wgsl"),
    "\n",
    include_str!("shaders/screen_space.wgsl"),
    "\n",
    include_str!("screen_trace/shaders/screen_trace.wgsl"),
    "\n",
    include_str!("shaders/grass_field.wgsl"),
    "\n",
    include_str!("shaders/tree_vertex.wgsl"),
    "\n",
    include_str!("shaders/tree.wgsl")
);
/// Shadow casters.
const TREE_SHADOW_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/grass_field.wgsl"),
    "\n",
    include_str!("shaders/tree_vertex.wgsl"),
    "\n",
    include_str!("shaders/tree_shadow.wgsl")
);
/// Screen-space prepass (normals, depth, motion vectors).
const TREE_PREPASS_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/grass_field.wgsl"),
    "\n",
    include_str!("shaders/tree_vertex.wgsl"),
    "\n",
    include_str!("shaders/tree_prepass.wgsl")
);

/// Share of a LOD distance over which two levels cross-fade (on each side of it). Practically
/// off: a dithered fade read as a hatch pattern around the crowns (TAA's neighbourhood clipping
/// keeps it), and the levels are close enough that the switch hardly shows. The fade code
/// stays for a smaller-noise dither later.
const FADE: f32 = 0.0001;
/// Trees cast shadows into the cascades as: the camera's mesh LOD in cascade 0, at least LOD 1
/// further out (the shadow of a far crown needs no twigs).
const SHADOW_MIN_LOD: [u8; 4] = [0, 1, 1, 1];

/// One placed tree on the GPU (`TreeInstance` in tree_vertex.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct InstanceGpu {
    /// xyz = foot (world), w = turn (radians).
    foot: [f32; 4],
    /// x = height scale, y = width scale, z = colour variation, w = LOD fade.
    scale: [f32; 4],
}

/// `TreeParams` in tree_vertex.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ParamsGpu {
    origin: [f32; 4],
    wind: [f32; 4],
    gust: [f32; 4],
    leaf: [f32; 4],
    bark: [f32; 4],
    motion: [f32; 4],
    impostor: [f32; 4],
    light: [f32; 4],
}

fn vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const VERTEX: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x4,
        4 => Unorm8x4
    ];
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TreeVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX,
        },
        instance_layout(),
    ]
}

fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const INSTANCE: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![5 => Float32x4, 6 => Float32x4];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<InstanceGpu>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &INSTANCE,
    }
}

/// What a draw shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Part {
    Bark,
    Leaves,
    Impostor,
}

/// The tree pipelines: (bark, leaves, impostor) for the main pass, shadows and the prepass.
struct Pipelines {
    main: [wgpu::RenderPipeline; 3],
    shadow: [wgpu::RenderPipeline; 3],
    prepass: [wgpu::RenderPipeline; 3],
}

fn part_index(p: Part) -> usize {
    match p {
        Part::Bark => 0,
        Part::Leaves => 1,
        Part::Impostor => 2,
    }
}

type Layouts<'a> = [&'a wgpu::BindGroupLayout; 3];

/// A main-pass tree pipeline (lit, depth written, motion vectors).
fn main_spec<'a>(
    name: &'a str,
    layouts: &'a Layouts<'a>,
    vs: &'a str,
    fs: &'a str,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
    cull: Option<wgpu::Face>,
    a2c: bool,
) -> Spec<'a> {
    Spec {
        vs,
        fs: Some(fs),
        buffers,
        cull,
        depth: Depth::main(true, wgpu::CompareFunction::Less),
        glow: Some(false),
        alpha_to_coverage: a2c,
        velocity: true,
        ..Spec::new(name, TREE_SRC, layouts)
    }
}

/// A shadow-caster pipeline (depth only, biased like the grass: clusters shade themselves).
fn shadow_spec<'a>(
    name: &'a str,
    layouts: &'a Layouts<'a>,
    vs: &'a str,
    fs: Option<&'a str>,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
) -> Spec<'a> {
    Spec {
        vs,
        fs,
        buffers,
        cull: None,
        depth: Some(Depth {
            format: super::SHADOW_FORMAT,
            write: true,
            compare: wgpu::CompareFunction::LessEqual,
            bias: wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.5,
                clamp: 0.0,
            },
        }),
        color: None,
        samples: 1,
        glow: None,
        ..Spec::new(name, TREE_SHADOW_SRC, layouts)
    }
}

/// A screen-space prepass pipeline (normals + motion vectors, single sample).
fn prepass_spec<'a>(
    name: &'a str,
    layouts: &'a Layouts<'a>,
    vs: &'a str,
    fs: &'a str,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
    cull: Option<wgpu::Face>,
) -> Spec<'a> {
    Spec {
        vs,
        fs: Some(fs),
        buffers,
        cull,
        depth: Depth::main(true, wgpu::CompareFunction::Less),
        color: Some(super::screen_space::NORMAL_FORMAT),
        samples: 1,
        glow: None,
        velocity: true,
        ..Spec::new(name, TREE_PREPASS_SRC, layouts)
    }
}

impl Pipelines {
    fn new(
        device: &wgpu::Device,
        p: &super::pipelines::Pipelines,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let mesh = vertex_layouts();
        let imp = [instance_layout()];
        let back = Some(wgpu::Face::Back);
        let lit = [&p.view_layout, &p.mesh_layout, layout];
        let main = [
            build(
                device,
                main_spec(
                    "tree bark",
                    &lit,
                    "vs_tree",
                    "fs_tree_bark",
                    &mesh,
                    back,
                    false,
                ),
            ),
            build(
                device,
                main_spec(
                    "tree leaves",
                    &lit,
                    "vs_tree",
                    "fs_tree_leaves",
                    &mesh,
                    None,
                    true,
                ),
            ),
            build(
                device,
                main_spec(
                    "tree impostor",
                    &lit,
                    "vs_tree_impostor",
                    "fs_tree_impostor",
                    &imp,
                    None,
                    true,
                ),
            ),
        ];
        let casters = [&p.shadow_view_layout, &p.draws_layout, layout];
        let shadow = [
            build(
                device,
                shadow_spec("tree bark shadow", &casters, "vs_tree_shadow", None, &mesh),
            ),
            build(
                device,
                shadow_spec(
                    "tree leaves shadow",
                    &casters,
                    "vs_tree_shadow",
                    Some("fs_tree_shadow_leaves"),
                    &mesh,
                ),
            ),
            build(
                device,
                shadow_spec(
                    "tree impostor shadow",
                    &casters,
                    "vs_tree_shadow_impostor",
                    Some("fs_tree_shadow_impostor"),
                    &imp,
                ),
            ),
        ];
        let pre = [&p.view_layout, &p.draws_layout, layout];
        let prepass = [
            build(
                device,
                prepass_spec(
                    "tree bark prepass",
                    &pre,
                    "vs_tree_prepass",
                    "fs_tree_prepass_bark",
                    &mesh,
                    back,
                ),
            ),
            build(
                device,
                prepass_spec(
                    "tree leaves prepass",
                    &pre,
                    "vs_tree_prepass",
                    "fs_tree_prepass_leaves",
                    &mesh,
                    None,
                ),
            ),
            build(
                device,
                prepass_spec(
                    "tree impostor prepass",
                    &pre,
                    "vs_tree_prepass_impostor",
                    "fs_tree_prepass_impostor",
                    &imp,
                    None,
                ),
            ),
        ];
        Self {
            main,
            shadow,
            prepass,
        }
    }
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    bark: Range<u32>,
    leaves: Range<u32>,
    vertex_count: u32,
}

impl GpuMesh {
    fn new(device: &wgpu::Device, m: &TreeMesh) -> Self {
        Self {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("misty tree vertices"),
                contents: bytemuck::cast_slice(&m.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("misty tree indices"),
                contents: bytemuck::cast_slice(&m.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            bark: m.bark.clone(),
            leaves: m.leaves.clone(),
            vertex_count: m.vertices.len() as u32,
        }
    }

    fn bytes(&self) -> u64 {
        self.vertices.size() + self.indices.size()
    }
}

/// A tree asset on the GPU.
struct GpuTree {
    built: Arc<Built>,
    lods: [GpuMesh; 2],
    /// Bark, bark normal, leaves, leaf normal, impostor, impostor normal.
    textures: [wgpu::TextureView; 6],
    bytes: u64,
}

/// Coverage-keeping mip chain of an alpha-tested atlas with `cols` × `rows` cells: each level's
/// alpha is scaled per cell so as many texels pass 0.5 as in the full image (thin leaves and
/// far crowns don't fade away with distance).
fn coverage_mips(image: image::RgbaImage, cols: u32, rows: u32) -> Vec<image::RgbaImage> {
    let mut chain = super::texture::mip_chain(image, true);
    let cells = |img: &image::RgbaImage| {
        let (iw, ih) = img.dimensions();
        let (cw, ch) = ((iw / cols).max(1), (ih / rows).max(1));
        (0..rows)
            .flat_map(move |r| (0..cols).map(move |c| (c * cw, r * ch, cw, ch)))
            .collect::<Vec<_>>()
    };
    let share = |img: &image::RgbaImage, (x0, y0, cw, ch): (u32, u32, u32, u32), scale: f32| {
        let mut n = 0u32;
        for y in y0..(y0 + ch).min(img.height()) {
            for x in x0..(x0 + cw).min(img.width()) {
                n += u32::from(img.get_pixel(x, y)[3] as f32 * scale > 127.5);
            }
        }
        n as f32 / (cw * ch) as f32
    };
    let want: Vec<f32> = cells(&chain[0])
        .into_iter()
        .map(|c| share(&chain[0], c, 1.0))
        .collect();
    for mip in chain.iter_mut().skip(1) {
        if mip.width() < cols * 4 || mip.height() < rows * 4 {
            break;
        }
        for (cell, &target) in cells(mip).into_iter().zip(&want) {
            let (mut lo, mut hi) = (0.25f32, 16.0f32);
            for _ in 0..10 {
                let mid = (lo * hi).sqrt();
                if share(mip, cell, mid) < target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let scale = (lo * hi).sqrt();
            let (x0, y0, cw, ch) = cell;
            for y in y0..(y0 + ch).min(mip.height()) {
                for x in x0..(x0 + cw).min(mip.width()) {
                    let p = mip.get_pixel_mut(x, y);
                    p[3] = (p[3] as f32 * scale).round().min(255.0) as u8;
                }
            }
        }
    }
    chain
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    chain: &[image::RgbaImage],
    srgb: bool,
) -> (wgpu::TextureView, u64) {
    let (w, h) = chain[0].dimensions();
    let data: Vec<u8> = chain
        .iter()
        .flat_map(|m| m.as_raw().iter().copied())
        .collect();
    let bytes = data.len() as u64;
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: chain.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    (texture.create_view(&Default::default()), bytes)
}

/// A texture of the asset, or a 1×1 stand-in (white, a flat normal).
fn texture_or(rel: &str, normal: bool) -> image::RgbaImage {
    if !rel.is_empty()
        && let Ok(img) = super::texture::load_image(&crate::assets::absolute(rel))
    {
        return img;
    }
    let p = if normal {
        [128, 128, 255, 255]
    } else {
        [255; 4]
    };
    image::RgbaImage::from_pixel(1, 1, image::Rgba(p))
}

impl GpuTree {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, built: Arc<Built>) -> Self {
        let a = &built.asset;
        let m = &built.model;
        let lods = [
            GpuMesh::new(device, &m.lods[0]),
            GpuMesh::new(device, &m.lods[1]),
        ];
        let plain =
            |rel: &str, normal: bool| super::texture::mip_chain(texture_or(rel, normal), !normal);
        let leaf = texture_or(&a.leaf_texture, false);
        let leaf = if leaf.width() >= 64 {
            coverage_mips(leaf, 2, 2)
        } else {
            vec![leaf]
        };
        let imp = coverage_mips(m.impostor.albedo.clone(), impostor::VIEWS, 1);
        let imp_n = super::texture::mip_chain(m.impostor.normal.clone(), false);
        let chains = [
            (plain(&a.bark_texture, false), true),
            (plain(&a.bark_normal, true), false),
            (leaf, true),
            (plain(&a.leaf_normal, true), false),
            (imp, true),
            (imp_n, false),
        ];
        let mut bytes = lods.iter().map(GpuMesh::bytes).sum::<u64>();
        let textures = chains.map(|(chain, srgb)| {
            let (view, b) = upload(device, queue, "misty tree texture", &chain, srgb);
            bytes += b;
            view
        });
        Self {
            built,
            lods,
            textures,
            bytes,
        }
    }
}

/// A placed tree in world space (cached per terrain version).
#[derive(Clone, Copy)]
struct Placed {
    foot: Vec3,
    yaw: f32,
    height: f32,
    width: f32,
    color: f32,
    prototype: u16,
}

struct TerrainTrees {
    version: u64,
    position: Vec3,
    placed: Vec<Placed>,
}

/// Instances of a frame per (bind group, tree, part, LOD).
type Lists = HashMap<(usize, usize, Part, usize), Vec<InstanceGpu>>;

/// An instanced draw of this frame.
#[derive(Clone, Debug)]
struct Batch {
    /// Bind group of (terrain, prototype).
    group: usize,
    tree: usize,
    part: Part,
    lod: usize,
    instances: Range<u32>,
}

/// Per-frame quality inputs.
pub(super) struct TreeFrame<'a> {
    pub terrains: &'a [TerrainDraw],
    pub camera: Vec3,
    pub view_proj: &'a Mat4,
    pub cascades: &'a [super::cascades::Cascade],
    pub environment: &'a Environment,
    pub time: f32,
    /// Seconds since last frame for the wind's motion vectors (0 = skip).
    pub dt: f32,
    pub taa: bool,
    pub lod_bias: f32,
    /// Direction towards the sun that casts the cascades.
    pub sun: Option<Vec3>,
}

pub(super) struct TreeRenderer {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: Option<Pipelines>,
    trees: HashMap<String, GpuTree>,
    terrains: HashMap<String, TerrainTrees>,
    instance_buffer: Option<wgpu::Buffer>,
    /// This frame's tree keys (index = `Batch::tree`), groups, and draws per pass.
    frame_trees: Vec<String>,
    /// Per (terrain, prototype): bark, leaves and impostor groups.
    groups: Vec<[wgpu::BindGroup; 3]>,
    main: Vec<Batch>,
    shadows: Vec<Vec<Batch>>,
}

impl TreeRenderer {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: S::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: S::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        entries.extend((1..=2).map(texture));
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: S::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty trees"),
            entries: &entries,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty tree sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        Self {
            layout,
            sampler,
            pipelines: None,
            trees: HashMap::new(),
            terrains: HashMap::new(),
            instance_buffer: None,
            frame_trees: Vec::new(),
            groups: Vec::new(),
            main: Vec::new(),
            shadows: Vec::new(),
        }
    }

    /// Any tree to draw this frame.
    pub fn has_trees(&self) -> bool {
        !self.main.is_empty()
    }

    /// Uploaded tree assets and their GPU bytes (Memory module).
    pub fn memory(&self) -> (usize, u64) {
        (self.trees.len(), self.trees.values().map(|t| t.bytes).sum())
    }

    /// Bring tree assets up to date, sort this frame's trees into instance lists per pass and
    /// upload them.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &super::pipelines::Pipelines,
        f: &TreeFrame,
    ) {
        crate::profile_scope!("Trees.Prepare");
        self.frame_trees.clear();
        self.groups.clear();
        self.main.clear();
        self.shadows.clear();
        self.shadows.resize(f.cascades.len(), Vec::new());
        if f.terrains.iter().all(|t| t.data.trees.instances.is_empty()) {
            return;
        }
        if self.pipelines.is_none() {
            self.pipelines = Some(Pipelines::new(device, pipelines, &self.layout));
        }
        let frustum = Frustum::from_matrix(f.view_proj);
        let cascade_frusta: Vec<Frustum> = f
            .cascades
            .iter()
            .map(|c| Frustum::from_matrix(&c.view_proj))
            .collect();
        let shadow_reach = f.cascades.last().map_or(0.0, |c| c.far);
        // Instances of this frame: per (group, part, lod) for the camera and per cascade.
        let mut lists: Lists = HashMap::new();
        let mut shadow_lists: Vec<Lists> = vec![HashMap::new(); f.cascades.len()];
        for t in f.terrains {
            let s = &t.data.trees;
            if s.instances.is_empty() || s.prototypes.is_empty() {
                continue;
            }
            // World positions, recomputed when the terrain data or its position change.
            let cached = self
                .terrains
                .get(&t.key)
                .is_some_and(|c| c.version == t.version && c.position == t.position);
            if !cached {
                let placed = s
                    .instances
                    .iter()
                    .map(|i| Placed {
                        foot: t.position + local_position(&t.data, i),
                        yaw: i.rotation,
                        height: i.height_scale,
                        width: i.width_scale,
                        color: i.color,
                        prototype: i.prototype,
                    })
                    .collect();
                self.terrains.insert(
                    t.key.clone(),
                    TerrainTrees {
                        version: t.version,
                        position: t.position,
                        placed,
                    },
                );
            }
            // Prototypes of this terrain that are built and uploaded.
            let mut protos: Vec<Option<(usize, usize)>> = Vec::with_capacity(s.prototypes.len());
            for p in &s.prototypes {
                let Some(built) = store::get(&p.tree) else {
                    protos.push(None);
                    continue;
                };
                let stale = self
                    .trees
                    .get(&p.tree)
                    .is_none_or(|g| g.built.version != built.version);
                if stale {
                    self.trees
                        .insert(p.tree.clone(), GpuTree::new(device, queue, built));
                }
                let tree = match self.frame_trees.iter().position(|k| *k == p.tree) {
                    Some(i) => i,
                    None => {
                        self.frame_trees.push(p.tree.clone());
                        self.frame_trees.len() - 1
                    }
                };
                let group = self.groups.len();
                let bind = self.bind_group(device, &self.trees[&p.tree], t, f);
                self.groups.push(bind);
                protos.push(Some((group, tree)));
            }
            let lod_d = s.lod_distance * f.lod_bias.max(0.1);
            let bb_d = s.billboard_start.max(lod_d * 1.25) * f.lod_bias.max(0.1);
            let draw_d = s.draw_distance;
            let cache = &self.terrains[&t.key];
            for p in &cache.placed {
                let Some(Some((group, tree))) = protos.get(p.prototype as usize).copied() else {
                    continue;
                };
                let m = &self.trees[&self.frame_trees[tree]].built.model;
                let scale = p.height.max(p.width);
                let center = p.foot + Vec3::Y * (m.max.y * 0.5 * p.height);
                let radius = m.bounding_radius() * scale;
                let dist = center.distance(f.camera);
                if dist > draw_d + radius {
                    continue;
                }
                let bounds = Aabb::new(center - Vec3::splat(radius), center + Vec3::splat(radius));
                let inst = |fade: f32| InstanceGpu {
                    foot: [p.foot.x, p.foot.y, p.foot.z, p.yaw],
                    scale: [p.height, p.width, p.color, fade],
                };
                // LOD with cross-fades: (lod, fade) pairs (fade 0 = fully drawn).
                let mut chosen: [(usize, f32); 2] = [(usize::MAX, 0.0); 2];
                let fade_at = |d: f32, edge: f32| {
                    ((d - edge * (1.0 - FADE)) / (edge * 2.0 * FADE)).clamp(0.0, 1.0)
                };
                let a = fade_at(dist, lod_d);
                let b = fade_at(dist, bb_d);
                let far = fade_at(dist, draw_d * (1.0 - FADE));
                if a < 1.0 {
                    chosen[0] = (0, if a > 0.0 { 1.0 - a } else { 0.0 });
                    if a > 0.0 {
                        chosen[1] = (1, -a);
                    }
                } else if b < 1.0 {
                    chosen[0] = (1, if b > 0.0 { 1.0 - b } else { 0.0 });
                    if b > 0.0 {
                        chosen[1] = (2, -b);
                    }
                } else if far < 1.0 {
                    chosen[0] = (2, if far > 0.0 { 1.0 - far } else { 0.0 });
                }
                // `fade` > 0 keeps noise < fade (fading out), < 0 keeps noise >= 1 + fade.
                // A level that is fading out keeps its share 1 - x; the one fading in, x.
                let visible = frustum.intersects(&bounds);
                if visible {
                    for &(lod, fade) in chosen.iter().filter(|c| c.0 != usize::MAX) {
                        let fade = if fade == 0.0 {
                            0.0
                        } else {
                            fade.clamp(-0.999, 0.999)
                        };
                        let parts: &[Part] = if lod == 2 {
                            &[Part::Impostor]
                        } else {
                            &[Part::Bark, Part::Leaves]
                        };
                        for &part in parts {
                            lists
                                .entry((group, tree, part, lod))
                                .or_default()
                                .push(inst(fade));
                        }
                    }
                }
                // Shadows: within the cascades' reach, one LOD per cascade (no fades).
                if s.cast_shadows && t.cast_shadows && dist < shadow_reach + radius {
                    let cam_lod = if a < 0.5 {
                        0
                    } else if b < 0.5 {
                        1
                    } else {
                        2
                    };
                    for (slot, cf) in cascade_frusta.iter().enumerate() {
                        if !cf.intersects(&bounds) {
                            continue;
                        }
                        let lod = cam_lod.max(SHADOW_MIN_LOD[slot.min(3)] as usize);
                        let parts: &[Part] = if lod == 2 {
                            &[Part::Impostor]
                        } else {
                            &[Part::Bark, Part::Leaves]
                        };
                        for &part in parts {
                            shadow_lists[slot]
                                .entry((group, tree, part, lod))
                                .or_default()
                                .push(inst(0.0));
                        }
                    }
                }
            }
        }
        // One buffer for every list.
        let mut all: Vec<InstanceGpu> = Vec::new();
        let mut take = |map: Lists| {
            let mut keys: Vec<_> = map.into_iter().collect();
            // Bark before leaves before impostors (fewer pipeline switches), then by group.
            keys.sort_by_key(|((g, _, part, lod), _)| (*part, *lod, *g));
            keys.into_iter()
                .map(|((group, tree, part, lod), v)| {
                    let start = all.len() as u32;
                    all.extend(v);
                    Batch {
                        group,
                        tree,
                        part,
                        lod,
                        instances: start..all.len() as u32,
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut batches: Vec<Vec<Batch>> = std::iter::once(lists)
            .chain(shadow_lists)
            .map(&mut take)
            .collect();
        self.main = batches.remove(0);
        self.shadows = batches;
        if all.is_empty() {
            return;
        }
        let bytes = std::mem::size_of_val(all.as_slice()) as u64;
        if self
            .instance_buffer
            .as_ref()
            .is_none_or(|b| b.size() < bytes)
        {
            self.instance_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty tree instances"),
                size: bytes.next_power_of_two().max(4096),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(b) = &self.instance_buffer {
            queue.write_buffer(b, 0, bytemuck::cast_slice(&all));
        }
    }

    fn bind_group(
        &self,
        device: &wgpu::Device,
        gpu: &GpuTree,
        t: &TerrainDraw,
        f: &TreeFrame,
    ) -> [wgpu::BindGroup; 3] {
        let a = &gpu.built.asset;
        let m = &gpu.built.model;
        let [strength, speed, dx, dz, gusts, flutter] =
            super::terrain_grass::wind(&t.data.details, f.environment.wind.as_ref());
        let lin = |c: [f32; 3]| c.map(impostor::lin);
        let [lr, lg, lb] = lin(a.leaf_color);
        let [br, bg, bb] = lin(a.bark_color);
        let sun = f.sun.unwrap_or(Vec3::Y);
        let params = ParamsGpu {
            origin: [t.position.x, t.position.y, t.position.z, f.time],
            wind: [strength, speed, dx, dz],
            gust: [gusts, flutter, t.data.details.gust_scale.max(1.0), f.dt],
            leaf: [lr, lg, lb, a.translucency],
            bark: [br, bg, bb, a.height],
            motion: [
                a.wind.trunk_bend,
                a.wind.branch_sway,
                a.wind.leaf_flutter,
                a.leaf_variation,
            ],
            impostor: [
                m.impostor.width,
                m.impostor.height,
                m.impostor.bottom,
                impostor::VIEWS as f32,
            ],
            light: [sun.x, sun.y, sun.z, if f.taa { 1.0 } else { 0.0 }],
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty tree params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        std::array::from_fn(|part| {
            let entries = [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&gpu.textures[part * 2]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&gpu.textures[part * 2 + 1]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ];
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty tree group"),
                layout: &self.layout,
                entries: &entries,
            })
        })
    }

    fn draw_list(
        &self,
        pass: &mut wgpu::RenderPass,
        list: &[Batch],
        pipelines: &[wgpu::RenderPipeline; 3],
        stats: &mut RenderStats,
    ) {
        let Some(instances) = &self.instance_buffer else {
            return;
        };
        let mut bound: Option<Part> = None;
        for b in list {
            let Some(gpu) = self.frame_trees.get(b.tree).and_then(|k| self.trees.get(k)) else {
                continue;
            };
            if bound != Some(b.part) {
                pass.set_pipeline(&pipelines[part_index(b.part)]);
                stats.set_pass += 1;
                bound = Some(b.part);
            }
            pass.set_bind_group(2, &self.groups[b.group][part_index(b.part)], &[]);
            let count = b.instances.len() as u32;
            let size = std::mem::size_of::<InstanceGpu>() as u64;
            let slice =
                instances.slice(b.instances.start as u64 * size..b.instances.end as u64 * size);
            if b.part == Part::Impostor {
                pass.set_vertex_buffer(0, slice);
                pass.draw(0..6, 0..count);
                stats.draw(count, 6, 6);
                continue;
            }
            let mesh = &gpu.lods[b.lod.min(1)];
            let range = if b.part == Part::Bark {
                mesh.bark.clone()
            } else {
                mesh.leaves.clone()
            };
            if range.is_empty() {
                continue;
            }
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, slice);
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(range.clone(), 0, 0..count);
            stats.draw(count, range.len() as u32, mesh.vertex_count);
        }
    }

    /// This frame's trees into the main pass (groups 0 and 1 bound by the caller: the view and
    /// the lit group).
    pub fn draw_main(&self, pass: &mut wgpu::RenderPass, stats: &mut RenderStats) {
        if let Some(p) = &self.pipelines {
            self.draw_list(pass, &self.main, &p.main, stats);
        }
    }

    /// Shadow casters of cascade `slot` (groups 0 and 1 bound by the caller).
    pub fn draw_shadow(&self, pass: &mut wgpu::RenderPass, slot: usize, stats: &mut RenderStats) {
        if let (Some(p), Some(list)) = (&self.pipelines, self.shadows.get(slot)) {
            self.draw_list(pass, list, &p.shadow, stats);
        }
    }

    /// The screen-space prepass (groups 0 and 1 bound by the caller: the view and the draws).
    pub fn draw_prepass(&self, pass: &mut wgpu::RenderPass, stats: &mut RenderStats) {
        if let Some(p) = &self.pipelines {
            self.draw_list(pass, &self.main, &p.prepass, stats);
        }
    }
}

#[cfg(test)]
mod tests {
    use wgpu::naga;

    /// The tree shaders parse and validate (no GPU needed).
    #[test]
    fn tree_shaders_validate() {
        for (name, src) in [
            ("tree", super::TREE_SRC),
            ("tree shadow", super::TREE_SHADOW_SRC),
            ("tree prepass", super::TREE_PREPASS_SRC),
        ] {
            let full = format!("{}\n{src}", super::super::pipelines::COMMON);
            let module = naga::front::wgsl::parse_str(&full)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&full)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}

#[cfg(test)]
#[path = "trees_gpu_tests.rs"]
pub(super) mod gpu_tests;
