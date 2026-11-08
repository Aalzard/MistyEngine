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
use crate::sprites::compress::generated::{self, Ctx, Layers, Role, Spec as TexSpec};
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
    include_str!("vsm/shaders/vsm_common.wgsl"),
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
/// The mesh LOD trees cast shadows with into a shadow map whose texel is `texel` metres: the
/// full mesh where texels resolve twigs, the first simplified level further out, impostors only
/// where a texel is wider than twigs anyway (impostor crowns barely shadow themselves). Chosen
/// per map (cascade or virtual shadow map page), not from the camera's distance: a tree
/// crossing its LOD distance used to switch its shadow in cascade 0 at once (no cross-fade),
/// and the shadow jumped while the player walked.
pub(super) fn shadow_lod(texel: f32) -> u8 {
    if texel <= 0.03 {
        0
    } else if texel < 0.2 {
        1
    } else {
        2
    }
}

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

/// The bind group layouts the tree pipelines use (owned, so a worker can build them).
struct TreeLayouts {
    view: wgpu::BindGroupLayout,
    mesh: wgpu::BindGroupLayout,
    shadow_view: wgpu::BindGroupLayout,
    draws: wgpu::BindGroupLayout,
    tree: wgpu::BindGroupLayout,
}

impl TreeLayouts {
    fn of(p: &super::pipelines::Pipelines, tree: &wgpu::BindGroupLayout) -> Self {
        Self {
            view: p.view_layout.clone(),
            mesh: p.mesh_layout.clone(),
            shadow_view: p.shadow_view_layout.clone(),
            draws: p.draws_layout.clone(),
            tree: tree.clone(),
        }
    }
}

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
    /// `samples`: the main pass's MSAA samples (passed in: a worker may build these).
    fn new(device: &wgpu::Device, l: &TreeLayouts, samples: u32) -> Self {
        let layout = &l.tree;
        let mesh = vertex_layouts();
        let imp = [instance_layout()];
        let back = Some(wgpu::Face::Back);
        let lit = [&l.view, &l.mesh, layout];
        let main_pass = |spec: Spec| build(device, Spec { samples, ..spec });
        let main = [
            main_pass(main_spec(
                "tree bark",
                &lit,
                "vs_tree",
                "fs_tree_bark",
                &mesh,
                back,
                false,
            )),
            main_pass(main_spec(
                "tree leaves",
                &lit,
                "vs_tree",
                "fs_tree_leaves",
                &mesh,
                None,
                true,
            )),
            main_pass(main_spec(
                "tree impostor",
                &lit,
                "vs_tree_impostor",
                "fs_tree_impostor",
                &imp,
                None,
                true,
            )),
        ];
        let casters = [&l.shadow_view, &l.draws, layout];
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
        let pre = [&l.view, &l.draws, layout];
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
    /// Meshes and textures.
    bytes: u64,
    texture_bytes: u64,
    /// Texture cache keys being encoded in the background (uploaded as RGBA8 meanwhile).
    waiting: Vec<String>,
}

/// Coverage-keeping mip chain of an alpha-tested atlas with `cols` × `rows` cells: each level's
/// alpha is scaled per cell so as many texels pass 0.5 as in the full image (thin leaves and
/// far crowns don't fade away with distance).
pub(super) fn coverage_mips(
    image: image::RgbaImage,
    cols: u32,
    rows: u32,
) -> Vec<image::RgbaImage> {
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

/// Write a grayscale map (its red channel, resized to `dst`'s size) into `channel` of `dst`,
/// or `fallback` everywhere when the map is missing or can't be read.
fn pack_channel(dst: &mut image::RgbaImage, rel: &str, channel: usize, fallback: u8) {
    let src = (!rel.is_empty())
        .then(|| super::texture::load_image(&crate::assets::absolute(rel)).ok())
        .flatten()
        .map(|img| {
            if img.dimensions() == dst.dimensions() {
                img
            } else {
                image::imageops::resize(
                    &img,
                    dst.width(),
                    dst.height(),
                    image::imageops::FilterType::Triangle,
                )
            }
        });
    match src {
        Some(src) => {
            for (d, s) in dst.pixels_mut().zip(src.pixels()) {
                d[channel] = s[0];
            }
        }
        None => {
            for d in dst.pixels_mut() {
                d[channel] = fallback;
            }
        }
    }
}

/// A tree's textures (decoded, packed and mip-mapped on any thread), ready to upload.
pub(super) struct TreeTextures {
    built: Arc<Built>,
    /// Bark, bark normal, leaves, leaf normal, impostor albedo, impostor normal: blocks
    /// (`Library/TextureCache`) or RGBA8 mip chains.
    pub(super) textures: [Layers; 6],
}

impl TreeTextures {
    /// Load the asset's textures, pack them and build every mip chain, or read their blocks
    /// from the texture cache (a hundred milliseconds or so for 2K textures: on a worker thread
    /// while streaming). Formats (`compress::generated`): colour with alpha (bark + height,
    /// leaf and impostor cutouts) and the packed normal maps BC7, by the Compression setting of
    /// the albedo / normal map file.
    pub(super) fn build(built: Arc<Built>, ctx: &Ctx) -> Self {
        let a = &built.asset;
        let m = &built.model;
        // The material maps ride in the spare channels (one texture pair per draw): bark
        // albedo alpha = height; normal maps = (x, y, roughness, occlusion / translucency),
        // z rebuilt in the shader.
        let mut bark = texture_or(&a.bark_texture, false);
        pack_channel(&mut bark, &a.bark_height, 3, 128);
        let mut bark_n = texture_or(&a.bark_normal, true);
        pack_channel(&mut bark_n, &a.bark_roughness, 2, 222);
        pack_channel(&mut bark_n, &a.bark_occlusion, 3, 255);
        let mut leaf_n = texture_or(&a.leaf_normal, true);
        pack_channel(&mut leaf_n, &a.leaf_roughness, 2, 150);
        pack_channel(&mut leaf_n, &a.leaf_translucency, 3, 255);
        let leaf = texture_or(&a.leaf_texture, false);
        let colour = |rel: &str| TexSpec {
            recipe: "tree box srgb",
            format: ctx.format(Role::ColorAlpha, rel),
            srgb: true,
            coverage: false,
        };
        let packed = |rel: &str| TexSpec {
            recipe: "tree box linear",
            format: ctx.format(Role::Packed, rel),
            srgb: false,
            coverage: false,
        };
        let srgb_mips = |i| super::texture::mip_chain(i, true);
        let linear_mips = |i| super::texture::mip_chain(i, false);
        let leaves = if leaf.width() >= 64 {
            let spec = TexSpec {
                recipe: "tree coverage 2x2",
                coverage: true,
                ..colour(&a.leaf_texture)
            };
            generated::make_one(leaf, &spec, ctx, |i| coverage_mips(i, 2, 2))
        } else {
            let spec = TexSpec {
                recipe: "tree one level",
                coverage: true,
                ..colour(&a.leaf_texture)
            };
            generated::make_one(leaf, &spec, ctx, |i| vec![i])
        };
        let impostor = TexSpec {
            recipe: "tree impostor coverage",
            coverage: true,
            ..colour("")
        };
        let textures = [
            generated::make_one(bark, &colour(&a.bark_texture), ctx, srgb_mips),
            generated::make_one(bark_n, &packed(&a.bark_normal), ctx, linear_mips),
            leaves,
            generated::make_one(leaf_n, &packed(&a.leaf_normal), ctx, linear_mips),
            generated::make_one(m.impostor.albedo.clone(), &impostor, ctx, |i| {
                coverage_mips(i, impostor::VIEWS, 1)
            }),
            generated::make_one(m.impostor.normal.clone(), &packed(""), ctx, linear_mips),
        ];
        Self { built, textures }
    }

    /// Keys still being encoded in the background (the textures are RGBA8 meanwhile).
    fn waiting(&self) -> Vec<String> {
        self.textures
            .iter()
            .flat_map(|t| t.waiting.iter().cloned())
            .collect()
    }
}

/// Make sure the texture cache under the context's project holds the blocks of the tree
/// asset at `path` (builds: the model is built here, as a player builds it); returns the
/// project-relative cache entries.
pub(crate) fn bake_textures(path: &str, ctx: &Ctx) -> Result<Vec<String>, String> {
    let asset = crate::trees::TreeAsset::load(&crate::assets::absolute(path))?;
    let t = TreeTextures::build(Arc::new(store::build(asset)), ctx);
    Ok(t.textures.iter().flat_map(Layers::entries).collect())
}

impl GpuTree {
    /// Upload a built tree: meshes and textures.
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, t: TreeTextures) -> Self {
        let waiting = t.waiting();
        let TreeTextures { built, textures } = t;
        let m = &built.model;
        let lods = [
            GpuMesh::new(device, &m.lods[0]),
            GpuMesh::new(device, &m.lods[1]),
        ];
        let mut bytes = lods.iter().map(GpuMesh::bytes).sum::<u64>();
        let mut texture_bytes = 0;
        let textures = textures.map(|t| {
            let (view, b) =
                super::texture::upload_made(device, queue, "misty tree texture", &t.layers, false);
            texture_bytes += b;
            view
        });
        bytes += texture_bytes;
        Self {
            built,
            lods,
            textures,
            bytes,
            texture_bytes,
            waiting,
        }
    }

    /// Whether textures that went up uncompressed have their blocks in the cache now.
    fn blocks_ready(&self) -> bool {
        !self.waiting.is_empty() && !generated::waiting(&self.waiting)
    }
}

/// Texture chains of one version of a tree asset, being built on a worker thread.
struct Pending {
    version: u64,
    /// (Behind a lock only so the renderer stays `Sync`.)
    rx: std::sync::Mutex<std::sync::mpsc::Receiver<TreeTextures>>,
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

/// A placed tree's key for the near layer's membership.
fn near_key(p: &Placed) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in [p.foot.x, p.foot.y, p.foot.z, p.yaw] {
        h = (h ^ v.to_bits() as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h ^ p.prototype as u64
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
    /// The virtual shadow map's page rectangles this frame (render/vsm): view-projection and
    /// the level of detail for their texel. Their lists follow the cascades'.
    pub vsm_views: &'a [(Mat4, u8)],
    /// The virtual shadow map's near layer (view-projection, half side): trees close to the
    /// camera go into it (their swaying leaves and limbs) instead of the cached pages. Its
    /// list follows the virtual shadow map's.
    pub near: Option<(Mat4, f32)>,
    /// Half side the near layer would have (0 = no virtual shadow map): trees within it are
    /// counted, so the layer is on before the first of them joins it.
    pub near_reach: f32,
}

pub(super) struct TreeRenderer {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: Option<Pipelines>,
    /// The pipelines being built on a worker since the renderer was made (streaming only).
    prebuilt: super::prebuilt::Prebuilt<Pipelines>,
    trees: HashMap<String, GpuTree>,
    /// Texture chains being built on worker threads, per tree asset (while streaming).
    pending: HashMap<String, Pending>,
    /// Tree assets of the last frame not drawn yet: being built or uploaded.
    loading: usize,
    /// A tree was uploaded this frame (one per frame while streaming: ~25 ms each, and with
    /// the models cached on disk every tree of a scene is ready in the same frame).
    uploaded: bool,
    terrains: HashMap<String, TerrainTrees>,
    instance_buffer: Option<wgpu::Buffer>,
    /// This frame's tree keys (index = `Batch::tree`), groups, and draws per pass.
    frame_trees: Vec<String>,
    /// Per (terrain, prototype): bark, leaves and impostor groups.
    groups: Vec<[wgpu::BindGroup; 3]>,
    /// Those groups and their parameter buffer per (tree asset, terrain), kept across frames
    /// (the parameters are rewritten each frame instead of a new buffer and groups).
    group_cache: HashMap<(String, String), CachedGroups>,
    /// Frames prepared (which cached groups were used this frame).
    stamp: u64,
    main: Vec<Batch>,
    shadows: Vec<Vec<Batch>>,
    /// Trees in the virtual shadow map's near layer (by placement), kept across frames for
    /// the hysteresis.
    near_set: std::collections::HashSet<u64>,
    /// Bounds of trees that entered or left the near layer since the virtual shadow map last
    /// asked (its pages under them render again).
    near_changes: Vec<Aabb>,
    /// Trees in the near layer this frame.
    near_count: usize,
    /// Trees within the near layer's reach this frame (it is needed).
    near_candidates: usize,
}

/// A tree joins the near layer when its box, seen from the light, lies within this share of
/// the layer's square and leaves it past [`NEAR_OUT`] (hysteresis, so a tree on the edge
/// doesn't flip every frame; its whole shadow column then lies in the layer).
const NEAR_IN: f32 = 0.9;
const NEAR_OUT: f32 = 1.0;

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
            prebuilt: Default::default(),
            trees: HashMap::new(),
            pending: HashMap::new(),
            loading: 0,
            uploaded: false,
            terrains: HashMap::new(),
            instance_buffer: None,
            frame_trees: Vec::new(),
            groups: Vec::new(),
            group_cache: HashMap::new(),
            stamp: 0,
            main: Vec::new(),
            shadows: Vec::new(),
            near_set: Default::default(),
            near_changes: Vec::new(),
            near_count: 0,
            near_candidates: 0,
        }
    }

    /// Bounds of the trees that entered or left the near layer since the last call.
    pub fn take_near_changes(&mut self) -> Vec<Aabb> {
        std::mem::take(&mut self.near_changes)
    }

    /// Trees within the near layer's reach last frame (it must be on).
    pub fn near_candidates(&self) -> usize {
        self.near_candidates
    }

    /// Trees drawn into the near layer last frame.
    pub fn near_count(&self) -> usize {
        self.near_count
    }

    /// Any tree to draw this frame.
    pub fn has_trees(&self) -> bool {
        !self.main.is_empty()
    }

    /// Start building the pipelines on a worker thread (streaming only), so the first frame
    /// of a scene with trees does not compile them.
    pub fn prebuild(&mut self, device: &wgpu::Device, pipelines: &super::pipelines::Pipelines) {
        let (device, layouts) = (device.clone(), TreeLayouts::of(pipelines, &self.layout));
        let samples = super::sample_count();
        self.prebuilt = super::prebuilt::Prebuilt::spawn("misty tree pipelines", move || {
            Pipelines::new(&device, &layouts, samples)
        });
    }

    /// Tree assets of the last frame still being built or prepared for upload.
    pub fn loading(&self) -> usize {
        self.loading
    }

    /// The uploaded model of `path` for `built`: uploads it when its textures are ready (built
    /// on a worker while streaming, so the frame never waits for image decoding), and keeps
    /// the previous version drawing meanwhile. None until a first version is uploaded.
    fn ensure(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &str,
        built: Arc<Built>,
    ) -> bool {
        // (Built again when its textures' blocks became ready: a cache hit then.)
        let current = self
            .trees
            .get(path)
            .is_some_and(|g| g.built.version == built.version && !g.blocks_ready());
        if current {
            return true;
        }
        let ctx = Ctx::runtime(
            device
                .features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
        );
        if !super::grass_streaming() {
            crate::profile_scope!("Trees.Upload");
            let t = TreeTextures::build(built, &ctx);
            self.trees
                .insert(path.to_owned(), GpuTree::new(device, queue, t));
            return true;
        }
        let ready = match self.pending.get_mut(path) {
            // Already uploaded one this frame: the others wait in their channels.
            Some(p) if p.version == built.version && self.uploaded => None,
            Some(p) if p.version == built.version => match p.rx.get_mut().map(|r| r.try_recv()) {
                Ok(Ok(t)) => Some(t),
                Ok(Err(std::sync::mpsc::TryRecvError::Empty)) => None,
                Ok(Err(std::sync::mpsc::TryRecvError::Disconnected)) | Err(_) => {
                    self.pending.remove(path);
                    None
                }
            },
            _ => {
                let (tx, rx) = std::sync::mpsc::channel();
                let version = built.version;
                let spawned = std::thread::Builder::new()
                    .name("misty tree textures".into())
                    .spawn(move || {
                        let _ = tx.send(TreeTextures::build(built, &ctx));
                    });
                if spawned.is_ok() {
                    let rx = std::sync::Mutex::new(rx);
                    self.pending
                        .insert(path.to_owned(), Pending { version, rx });
                }
                None
            }
        };
        if let Some(t) = ready {
            crate::profile_scope!("Trees.Upload");
            self.uploaded = true;
            self.pending.remove(path);
            self.trees
                .insert(path.to_owned(), GpuTree::new(device, queue, t));
        } else {
            self.loading += 1;
        }
        self.trees.contains_key(path)
    }

    /// Uploaded tree assets and their GPU bytes (Memory module).
    pub fn memory(&self) -> (usize, u64) {
        (self.trees.len(), self.trees.values().map(|t| t.bytes).sum())
    }

    /// GPU bytes of the uploaded trees' textures (bark, leaves, impostors).
    pub fn texture_bytes(&self) -> u64 {
        self.trees.values().map(|t| t.texture_bytes).sum()
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
        self.stamp += 1;
        self.frame_trees.clear();
        self.groups.clear();
        self.main.clear();
        self.shadows.clear();
        self.near_count = 0;
        self.near_candidates = 0;
        let mut near_now: std::collections::HashSet<u64> = Default::default();
        let shadow_views = f.cascades.len() + f.vsm_views.len() + f.near.is_some() as usize;
        self.shadows.resize(shadow_views, Vec::new());
        self.loading = 0;
        self.uploaded = false;
        if f.terrains.iter().all(|t| t.data.trees.instances.is_empty()) {
            return;
        }
        if self.pipelines.is_none() {
            crate::profile_scope!("Trees.Pipelines");
            let built = self.prebuilt.take().unwrap_or_else(|| {
                let layouts = TreeLayouts::of(pipelines, &self.layout);
                Pipelines::new(device, &layouts, super::sample_count())
            });
            self.pipelines = Some(built);
        }
        let frustum = Frustum::from_matrix(f.view_proj);
        let cascade_frusta: Vec<Frustum> = f
            .cascades
            .iter()
            .map(|c| Frustum::from_matrix(&c.view_proj))
            .collect();
        let vsm_frusta: Vec<(Frustum, u8)> = f
            .vsm_views
            .iter()
            .map(|(m, lod)| (Frustum::from_matrix(m), *lod))
            .collect();
        let shadow_reach = f.cascades.last().map_or(0.0, |c| c.far);
        // Instances of this frame: per (group, part, lod) for the camera and per cascade.
        let mut lists: Lists = HashMap::new();
        let mut shadow_lists: Vec<Lists> = vec![HashMap::new(); shadow_views];
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
                    if store::error(&p.tree).is_none() {
                        self.loading += 1;
                    }
                    protos.push(None);
                    continue;
                };
                if !self.ensure(device, queue, &p.tree, built) {
                    protos.push(None);
                    continue;
                }
                let tree = match self.frame_trees.iter().position(|k| *k == p.tree) {
                    Some(i) => i,
                    None => {
                        self.frame_trees.push(p.tree.clone());
                        self.frame_trees.len() - 1
                    }
                };
                let group = self.groups.len();
                let bind = {
                    crate::profile_scope!("Trees.BindGroup");
                    let gpu = &self.trees[&p.tree];
                    let params = tree_params(gpu, t, f);
                    let key = (p.tree.clone(), t.key.clone());
                    match self.group_cache.get_mut(&key) {
                        Some(c) if c.textures == gpu.textures => {
                            queue.write_buffer(&c.buffer, 0, bytemuck::bytes_of(&params));
                            c.stamp = self.stamp;
                            c.groups.clone()
                        }
                        _ => {
                            let c = CachedGroups::new(
                                device,
                                &self.layout,
                                &self.sampler,
                                gpu,
                                &params,
                                self.stamp,
                            );
                            let groups = c.groups.clone();
                            self.group_cache.insert(key, c);
                            groups
                        }
                    }
                };
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
                let m_bounds = (m.min, m.max);
                let scale = p.height.max(p.width);
                let center = p.foot + Vec3::Y * (m.max.y * 0.5 * p.height);
                let radius = m.bounding_radius() * scale;
                let dist = center.distance(f.camera);
                // Past the draw distance only the virtual shadow map's pages take the tree.
                let beyond = dist > draw_d + radius;
                if beyond && vsm_frusta.is_empty() {
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
                let visible = !beyond && frustum.intersects(&bounds);
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
                if s.cast_shadows && t.cast_shadows && !beyond && dist < shadow_reach + radius {
                    for (slot, cf) in cascade_frusta.iter().enumerate() {
                        if !cf.intersects(&bounds) {
                            continue;
                        }
                        let lod = shadow_lod(f.cascades[slot].texel) as usize;
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
                // Virtual shadow map pages: cached across frames, so whatever the camera's
                // distance (a page rendered while the tree was far stays valid when it comes
                // near), at the level of detail of the page's texel.
                if s.cast_shadows && t.cast_shadows && dist - radius < f.near_reach {
                    self.near_candidates += 1;
                }
                // Close to the camera: the near layer, redrawn every frame (the leaves sway),
                // instead of the cached pages.
                let near = f
                    .near
                    .filter(|_| s.cast_shadows && t.cast_shadows)
                    .filter(|(m, _)| {
                        let reach = if self.near_set.contains(&near_key(p)) {
                            NEAR_OUT
                        } else {
                            NEAR_IN
                        };
                        // Its box (any yaw) inside the layer's square, seen from the light.
                        let c = m_bounds
                            .0
                            .x
                            .abs()
                            .max(m_bounds.1.x.abs())
                            .max(m_bounds.0.z.abs())
                            .max(m_bounds.1.z.abs())
                            * p.width;
                        let (y0, y1) = (m_bounds.0.y * p.height, m_bounds.1.y * p.height);
                        (0..8).all(|k| {
                            let corner = p.foot
                                + Vec3::new(
                                    if k & 1 == 0 { -c } else { c },
                                    if k & 2 == 0 { y0 } else { y1 },
                                    if k & 4 == 0 { -c } else { c },
                                );
                            let ndc = m.project_point3(corner);
                            ndc.x.abs() < reach && ndc.y.abs() < reach
                        })
                    });
                if near.is_some() {
                    near_now.insert(near_key(p));
                    self.near_count += 1;
                    let list = f.cascades.len() + f.vsm_views.len();
                    for part in [Part::Bark, Part::Leaves] {
                        shadow_lists[list]
                            .entry((group, tree, part, 0))
                            .or_default()
                            .push(inst(0.0));
                    }
                }
                if s.cast_shadows && t.cast_shadows && near.is_none() {
                    for (j, (vf, lod)) in vsm_frusta.iter().enumerate() {
                        if !vf.intersects(&bounds) {
                            continue;
                        }
                        let lod = *lod as usize;
                        let parts: &[Part] = if lod == 2 {
                            &[Part::Impostor]
                        } else {
                            &[Part::Bark, Part::Leaves]
                        };
                        for &part in parts {
                            shadow_lists[f.cascades.len() + j]
                                .entry((group, tree, part, lod))
                                .or_default()
                                .push(inst(0.0));
                        }
                    }
                }
            }
        }
        // Groups no terrain drew with this frame go.
        let stamp = self.stamp;
        self.group_cache.retain(|_, c| c.stamp == stamp);
        // Trees that entered or left the near layer: the virtual shadow map's pages under them
        // render again (without or with them).
        if f.near.is_some() || !self.near_set.is_empty() {
            let changed: Vec<u64> = near_now
                .symmetric_difference(&self.near_set)
                .copied()
                .collect();
            if !changed.is_empty() {
                for t in f.terrains {
                    let Some(cache) = self.terrains.get(&t.key) else {
                        continue;
                    };
                    for p in cache
                        .placed
                        .iter()
                        .filter(|p| changed.contains(&near_key(p)))
                    {
                        // Generous bounds: the tree's height and width, any prototype.
                        let r = 20.0 * p.height.max(p.width).max(0.5);
                        self.near_changes.push(Aabb::new(
                            p.foot - Vec3::new(r, 1.0, r),
                            p.foot + Vec3::new(r, 2.0 * r, r),
                        ));
                    }
                }
            }
            self.near_set = near_now;
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

/// The parameters a tree of terrain `t` draws with this frame.
fn tree_params(gpu: &GpuTree, t: &TerrainDraw, f: &TreeFrame) -> ParamsGpu {
    let a = &gpu.built.asset;
    let m = &gpu.built.model;
    let [strength, speed, dx, dz, gusts, flutter] =
        super::terrain_grass::wind(&t.data.details, f.environment.wind.as_ref());
    let lin = |c: [f32; 3]| c.map(impostor::lin);
    let [lr, lg, lb] = lin(a.leaf_color);
    let [br, bg, bb] = lin(a.bark_color);
    let sun = f.sun.unwrap_or(Vec3::Y);
    ParamsGpu {
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
    }
}

/// Bark, leaves and impostor groups of one (tree asset, terrain) and their parameter buffer.
struct CachedGroups {
    buffer: wgpu::Buffer,
    groups: [wgpu::BindGroup; 3],
    /// The tree's textures the groups bind (a reloaded tree gets new groups).
    textures: [wgpu::TextureView; 6],
    /// Last frame they were used ([`TreeRenderer::stamp`]).
    stamp: u64,
}

impl CachedGroups {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        gpu: &GpuTree,
        params: &ParamsGpu,
        stamp: u64,
    ) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty tree params"),
            contents: bytemuck::bytes_of(params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let groups = std::array::from_fn(|part| {
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
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ];
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty tree group"),
                layout,
                entries: &entries,
            })
        });
        Self {
            buffer,
            groups,
            textures: gpu.textures.clone(),
            stamp,
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
