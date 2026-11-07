//! The study renderer: its own targets, pipelines and passes for the ground, the blades (each
//! variant's way), the sun's cascades, TAA and tonemapping. Timed per pass with the
//! engine's pass timer.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec2, Vec3, Vec4};
use wgpu::util::DeviceExt;

use super::field::{BLADE_VERTICES, COARSE_VERTICES, Field};
use crate::render::pass_timer::PassTimer;
use crate::render::terrain_grass::GrassParams;
use crate::terrain::TerrainData;

pub const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const VELOCITY: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
const SHADOW_SIZE: u32 = 2048;
/// Radius of the sun's first cascade (the one with grass casters).
const CASCADE0: f32 = 24.0;
const CASCADE1: f32 = 170.0;
pub const CACHE_TEXELS: u32 = 1024;
/// Variant G: metres per texel of the two cache levels.
const CACHE_STEP: [f32; 2] = [1.0 / 32.0, 0.25];
pub const CACHE_SLICES: u32 = 4;
/// Ground mesh: half extent and spacing (m) around the camera.
const GROUND_HALF: f32 = 320.0;
const GROUND_STEP: f32 = 1.0;

const COMMON: &str = include_str!("shaders/study_common.wgsl");
const FIELD: &str = include_str!("shaders/study_field.wgsl");
const BLADE: &str = include_str!("shaders/study_blade.wgsl");
const GRASS: &str = include_str!("shaders/study_grass.wgsl");
const GROUND: &str = include_str!("shaders/study_ground.wgsl");
const RESOLVE: &str = include_str!("shaders/study_resolve.wgsl");
const SWR: &str = include_str!("shaders/study_swr.wgsl");
const SHADOW: &str = include_str!("shaders/study_shadow.wgsl");
const POST: &str = include_str!("shaders/study_post.wgsl");
const CACHE: &str = include_str!("shaders/study_cache.wgsl");

/// Fragment counters (the counting build): helper lanes have an empty sample mask and no
/// side effects, so each real lane adds 12 / (real lanes in its quad); the sum / 12 is the
/// number of quads launched.
const COUNT_ON: &str = "
fn count_quad(base: u32, mask: u32) {
    let real = select(0u, 1u, mask != 0u);
    let q = real + quadSwapX(real) + quadSwapY(real) + quadSwapDiagonal(real);
    if (real == 1u) {
        atomicAdd(&counters[base], 1u);
        atomicAdd(&counters[base + 1u], 12u / max(q, 1u));
    }
}
fn count_add(slot: u32, n: u32) {
    atomicAdd(&counters[slot], n);
}
";
const COUNT_OFF: &str = "
fn count_quad(base: u32, mask: u32) {}
fn count_add(slot: u32, n: u32) {}
";

/// Kinds of shader module (`module_source`).
pub const KINDS: [&str; 7] = [
    "grass", "ground", "resolve", "swr", "shadow", "post", "cache",
];

/// The WGSL of one kind of module (`count` = the counting build).
pub fn module_source(kind: &str, count: bool) -> String {
    let extra = match kind {
        "grass" => GRASS.to_string(),
        "ground" => format!(
            "{GRASS}
{GROUND}"
        ),
        "resolve" => format!(
            "{GRASS}
{RESOLVE}"
        ),
        "swr" => format!(
            "{GRASS}
{SWR}"
        ),
        "shadow" => SHADOW.to_string(),
        "post" => POST.to_string(),
        "cache" => CACHE.to_string(),
        _ => unreachable!(),
    };
    // The counters' atomics would turn early depth testing off; the lit pass writes no depth,
    // so forcing it keeps the count to the fragments the timing build really shades.
    let (extra, count) = if count {
        (
            extra.replace(
                "@fragment
fn fs_grass(",
                "@fragment @early_depth_test(force)
fn fs_grass(",
            ),
            COUNT_ON,
        )
    } else {
        (extra, COUNT_OFF)
    };
    format!(
        "{COMMON}
{FIELD}
{BLADE}
{extra}
{count}"
    )
}

/// Counter slots (see the shaders).
pub mod slot {
    pub const DEPTH_LANES: usize = 0;
    pub const DEPTH_QUADS12: usize = 1;
    pub const DEPTH_KEPT: usize = 2;
    pub const LIT_LANES: usize = 4;
    pub const LIT_QUADS12: usize = 5;
    pub const LIT_KEPT: usize = 6;
    pub const LIT_SAMPLES: usize = 7;
    pub const VIS_LANES: usize = 8;
    pub const VIS_QUADS12: usize = 9;
    pub const VIS_KEPT: usize = 10;
    pub const RESOLVED: usize = 11;
    pub const SWR_PIXELS: usize = 12;
    pub const SWR_BLADES: usize = 13;
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Path {
    /// Depth prepass + lit pass (the live renderer's way) at `samples` per pixel.
    Forward { samples: u32 },
    /// Visibility buffer: blade id raster, then shading once per pixel (C: thin blades by
    /// the compute rasteriser; F: quarter-rate shading).
    Vis { swr: bool, quarter: bool },
    /// B4 / B2: the visibility buffer at 4× / 2× MSAA (coverage as a sample mask), each
    /// distinct blade of a pixel shaded once.
    VisMs { samples: u32 },
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub name: &'static str,
    pub label: &'static str,
    pub path: Path,
    /// D: clump factor (1 = off).
    pub clump: f32,
    /// E: blades end and the far field starts here (m).
    pub far_field: Option<f32>,
    /// G: the lighting cache.
    pub cache: bool,
    /// H: (split from m, widening of the split blades).
    pub split: Option<(f32, f32)>,
    /// C: thin-blade threshold in pixels.
    pub swr_px: f32,
}

impl Variant {
    pub const fn forward(name: &'static str, label: &'static str) -> Self {
        Self {
            name,
            label,
            path: Path::Forward { samples: 4 },
            clump: 1.0,
            far_field: None,
            cache: false,
            split: None,
            swr_px: 1.5,
        }
    }
}

/// The scene's light (study form).
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    /// Direction the sunlight travels.
    pub sun_dir: Vec3,
    /// Linear colour × intensity.
    pub sun: Vec3,
    pub shadows: bool,
    pub sky: Vec3,
    pub equator: Vec3,
    pub ground: Vec3,
    pub ambient: f32,
    pub fog: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StudyUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    view_proj_nj: [[f32; 4]; 4],
    sun_vp0: [[f32; 4]; 4],
    sun_vp1: [[f32; 4]; 4],
    camera: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky: [f32; 4],
    equator: [f32; 4],
    ground: [f32; 4],
    fog: [f32; 4],
    screen: [f32; 4],
    frame: [u32; 4],
    params: [f32; 4],
    cache0: [f32; 4],
    cache1: [f32; 4],
}

/// One camera view of a frame.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub eye: Vec3,
    pub view_proj: Mat4,
    /// One pixel's angle (radians).
    pub pixel: f32,
}

struct Targets {
    size: (u32, u32),
    samples: u32,
    color: wgpu::TextureView,
    glow: wgpu::TextureView,
    velocity: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// Single-sample colour + depth (= `color` / `depth` when `samples` is 1).
    color1: wgpu::TextureView,
    depth1: wgpu::TextureView,
    vis: wgpu::TextureView,
    /// B4's 4× visibility targets: blade id and point on the card (= `vis` when
    /// single-sample).
    vis_ms: wgpu::TextureView,
    vis_uv_ms: wgpu::TextureView,
    swr: wgpu::Buffer,
    history: [wgpu::TextureView; 2],
    ldr: wgpu::Texture,
}

pub struct Study {
    pub terrain: Arc<TerrainData>,
    pub origin: Vec3,
    layouts: Layouts,
    atlas: (wgpu::TextureView, wgpu::Sampler),
    study_buf: wgpu::Buffer,
    grass_buf: wgpu::Buffer,
    caster_grass_buf: wgpu::Buffer,
    caster_bufs: [wgpu::Buffer; 2],
    shadow_layers: [wgpu::TextureView; 2],
    cache_tex: wgpu::TextureView,
    cache_ground: wgpu::Buffer,
    counters: wgpu::Buffer,
    counters_read: wgpu::Buffer,
    /// Group 0 for normal passes, for the shadow passes (dummy shadow map) and the cache
    /// update (dummy cache).
    group0: wgpu::BindGroup,
    group0_shadow: wgpu::BindGroup,
    group0_cache: wgpu::BindGroup,
    ground_vb: wgpu::Buffer,
    ground_ib: wgpu::Buffer,
    ground_indices: u32,
    blades: Option<(wgpu::Buffer, wgpu::Buffer, Vec<super::field::ChunkDraw>)>,
    grass_group: Option<wgpu::BindGroup>,
    caster_grass_group: Option<wgpu::BindGroup>,
    pipelines: HashMap<String, Pipeline>,
    targets: Option<Targets>,
    pub sun_vp: [Mat4; 2],
    pub cache_origin: [Vec2; 2],
    prev_view_proj: Mat4,
    last_uniform: Option<StudyUniform>,
    pub count: bool,
    pub lighting: Lighting,
}

enum Pipeline {
    Render(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}

struct Layouts {
    study: wgpu::BindGroupLayout,
    grass: wgpu::BindGroupLayout,
    caster: wgpu::BindGroupLayout,
    resolve: wgpu::BindGroupLayout,
    resolve4: wgpu::BindGroupLayout,
    swr: wgpu::BindGroupLayout,
    msaa: wgpu::BindGroupLayout,
    taa: wgpu::BindGroupLayout,
    cache: wgpu::BindGroupLayout,
}

fn entry(
    binding: u32,
    vis: wgpu::ShaderStages,
    ty: wgpu::BindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty,
        count: None,
    }
}

fn tex(
    sample_type: wgpu::TextureSampleType,
    dim: wgpu::TextureViewDimension,
    ms: bool,
) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type,
        view_dimension: dim,
        multisampled: ms,
    }
}

fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn uniform() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn storage_tex(format: wgpu::TextureFormat, dim: wgpu::TextureViewDimension) -> wgpu::BindingType {
    wgpu::BindingType::StorageTexture {
        access: wgpu::StorageTextureAccess::WriteOnly,
        format,
        view_dimension: dim,
    }
}

impl Layouts {
    fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        use wgpu::TextureSampleType as T;
        use wgpu::TextureViewDimension as D;
        let all = S::VERTEX | S::FRAGMENT | S::COMPUTE;
        let fc = S::FRAGMENT | S::COMPUTE;
        let float = T::Float { filterable: true };
        let make = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        Self {
            study: make(
                "study",
                &[
                    entry(0, all, uniform()),
                    entry(1, all, tex(T::Depth, D::D2Array, false)),
                    entry(
                        2,
                        all,
                        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    ),
                    entry(3, all, tex(float, D::D2, false)),
                    entry(
                        4,
                        all,
                        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    ),
                    entry(5, all, tex(float, D::D2, false)),
                    entry(6, all, tex(float, D::D3, false)),
                    entry(7, all, tex(float, D::D2Array, false)),
                    entry(
                        8,
                        all,
                        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    ),
                    entry(9, fc, storage(false)),
                ],
            ),
            grass: make(
                "study grass",
                &[
                    entry(0, all, uniform()),
                    entry(1, all, tex(float, D::D2, false)),
                    entry(
                        2,
                        all,
                        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    ),
                    entry(3, all, storage(true)),
                ],
            ),
            caster: make("study caster", &[entry(0, S::VERTEX, uniform())]),
            resolve: make(
                "study resolve",
                &[
                    entry(0, S::COMPUTE, tex(T::Uint, D::D2, false)),
                    entry(1, S::COMPUTE, tex(T::Depth, D::D2, false)),
                    entry(2, S::COMPUTE, storage_tex(HDR, D::D2)),
                    entry(3, S::COMPUTE, storage(true)),
                ],
            ),
            resolve4: make(
                "study resolve4",
                &[
                    entry(2, S::COMPUTE, storage_tex(HDR, D::D2)),
                    entry(
                        4,
                        S::COMPUTE,
                        tex(T::Float { filterable: false }, D::D2, true),
                    ),
                    entry(5, S::COMPUTE, tex(T::Depth, D::D2, true)),
                    entry(
                        6,
                        S::COMPUTE,
                        tex(T::Float { filterable: false }, D::D2, true),
                    ),
                    entry(
                        7,
                        S::COMPUTE,
                        tex(T::Float { filterable: false }, D::D2, true),
                    ),
                ],
            ),
            swr: make(
                "study swr",
                &[
                    entry(0, S::COMPUTE, tex(T::Depth, D::D2, false)),
                    entry(1, S::COMPUTE, storage(false)),
                    entry(2, S::COMPUTE, storage(true)),
                ],
            ),
            msaa: make(
                "study msaa",
                &[
                    entry(
                        0,
                        S::FRAGMENT,
                        tex(T::Float { filterable: false }, D::D2, true),
                    ),
                    entry(1, S::FRAGMENT, tex(T::Depth, D::D2, true)),
                ],
            ),
            taa: make(
                "study taa",
                &[
                    entry(
                        0,
                        S::COMPUTE,
                        tex(T::Float { filterable: false }, D::D2, false),
                    ),
                    entry(1, S::COMPUTE, tex(T::Depth, D::D2, false)),
                    entry(2, S::COMPUTE, tex(float, D::D2, false)),
                    entry(3, S::COMPUTE, storage_tex(HDR, D::D2)),
                    entry(
                        4,
                        S::COMPUTE,
                        storage_tex(wgpu::TextureFormat::Rgba8Unorm, D::D2),
                    ),
                ],
            ),
            cache: make(
                "study cache",
                &[
                    entry(
                        0,
                        S::COMPUTE,
                        storage_tex(wgpu::TextureFormat::Rgba8Unorm, D::D2Array),
                    ),
                    entry(1, S::COMPUTE, storage(true)),
                ],
            ),
        }
    }
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    (w, h): (u32, u32),
    layers: u32,
    format: wgpu::TextureFormat,
    samples: u32,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// Tileable value-noise clouds (0..1), `n`².
fn cloud_noise(n: u32) -> Vec<u8> {
    let hash = |x: i32, y: i32, o: i32| {
        let mut h = (x.wrapping_mul(374_761_393)
            ^ y.wrapping_mul(668_265_263)
            ^ o.wrapping_mul(2_147_483_647)) as u32;
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        (h ^ (h >> 16)) as f32 / u32::MAX as f32
    };
    let noise = |x: f32, y: f32, period: i32, o: i32| {
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let s = |t: f32| t * t * (3.0 - 2.0 * t);
        let g = |a: i32, b: i32| hash(a.rem_euclid(period), b.rem_euclid(period), o);
        let top = g(ix, iy) + (g(ix + 1, iy) - g(ix, iy)) * s(fx);
        let bottom = g(ix, iy + 1) + (g(ix + 1, iy + 1) - g(ix, iy + 1)) * s(fx);
        top + (bottom - top) * s(fy)
    };
    let mut out = Vec::with_capacity((n * n) as usize);
    for y in 0..n {
        for x in 0..n {
            let mut v = 0.0;
            let mut amp = 0.5;
            let mut period = 4;
            for o in 0..4 {
                let f = period as f32 / n as f32;
                v += amp * noise(x as f32 * f, y as f32 * f, period, o);
                amp *= 0.5;
                period *= 2;
            }
            out.push((v / 0.9375 * 255.0) as u8);
        }
    }
    out
}

impl Study {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        terrain: Arc<TerrainData>,
        origin: Vec3,
        lighting: Lighting,
        count: bool,
    ) -> Self {
        let layouts = Layouts::new(device);
        let atlas = crate::render::terrain_grass::atlas(device, queue);
        let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let u = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let study_buf = buf(
            "study uniform",
            std::mem::size_of::<StudyUniform>() as u64,
            u,
        );
        let grass_buf = buf("study grass", std::mem::size_of::<GrassParams>() as u64, u);
        let caster_grass_buf = buf(
            "study grass casters",
            std::mem::size_of::<GrassParams>() as u64,
            u,
        );
        let caster_bufs = [buf("study caster 0", 64, u), buf("study caster 1", 64, u)];
        let shadow = texture(
            device,
            "study shadow",
            (SHADOW_SIZE, SHADOW_SIZE),
            2,
            DEPTH,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let layer = |i: u32| {
            shadow.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i,
                array_layer_count: Some(1),
                ..Default::default()
            })
        };
        let shadow_layers = [layer(0), layer(1)];
        let array_view = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let shadow_view = array_view(&shadow);
        let dummy_shadow = texture(
            device,
            "study dummy shadow",
            (1, 1),
            2,
            DEPTH,
            1,
            wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let dummy_shadow_view = array_view(&dummy_shadow);
        let clouds = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("study clouds"),
                size: wgpu::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &cloud_noise(256),
        );
        // Screen AO stand-in: a constant 0.85 (the fetch is the cost), 2048² covers 1080p.
        let ao = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("study ao"),
                size: wgpu::Extent3d {
                    width: 2048,
                    height: 2048,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &vec![217u8; 2048 * 2048],
        );
        // Froxel fog stand-in: faint warm in-scatter growing with depth, transmittance ~1.
        let mut fog = Vec::new();
        for z in 0..16u8 {
            let px = [z / 15, z / 15, z / 15, 255 - z * 2 / 3];
            for _ in 0..32 * 32 {
                fog.extend_from_slice(&px);
            }
        }
        let fog = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("study fog"),
                size: wgpu::Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_array_layers: 16,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &fog,
        );
        let cache = texture(
            device,
            "study light cache",
            (CACHE_TEXELS, CACHE_TEXELS),
            2,
            wgpu::TextureFormat::Rgba8Unorm,
            1,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let cache_tex = array_view(&cache);
        let dummy_cache = texture(
            device,
            "study dummy cache",
            (1, 1),
            2,
            wgpu::TextureFormat::Rgba8Unorm,
            1,
            wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let dummy_cache_view = array_view(&dummy_cache);
        let cache_ground = buf(
            "study cache ground",
            (2 * CACHE_TEXELS * CACHE_TEXELS * 4) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let counters = buf(
            "study counters",
            128,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        );
        let counters_read = buf(
            "study counters read",
            128,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("study shadow cmp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let repeat = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("study repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let clamp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("study clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let clouds_view = clouds.create_view(&Default::default());
        let ao_view = ao.create_view(&Default::default());
        let fog_view = fog.create_view(&Default::default());
        let group0 = |shadow_view: &wgpu::TextureView, cache_view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("study group 0"),
                layout: &layouts.study,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: study_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&cmp),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&clouds_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&repeat),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&ao_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&fog_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(cache_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::Sampler(&clamp),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: counters.as_entire_binding(),
                    },
                ],
            })
        };
        let group0_main = group0(&shadow_view, &cache_tex);
        let group0_shadow = group0(&dummy_shadow_view, &dummy_cache_view);
        let group0_cache = group0(&shadow_view, &dummy_cache_view);
        let ground_vb = buf("study ground vb", 16, wgpu::BufferUsages::VERTEX);
        let ground_ib = buf("study ground ib", 16, wgpu::BufferUsages::INDEX);
        Self {
            terrain,
            origin,
            layouts,
            atlas,
            study_buf,
            grass_buf,
            caster_grass_buf,
            caster_bufs,
            shadow_layers,
            cache_tex,
            cache_ground,
            counters,
            counters_read,
            group0: group0_main,
            group0_shadow,
            group0_cache,
            ground_vb,
            ground_ib,
            ground_indices: 0,
            blades: None,
            grass_group: None,
            caster_grass_group: None,
            pipelines: HashMap::new(),
            targets: None,
            sun_vp: [Mat4::IDENTITY; 2],
            cache_origin: [Vec2::ZERO; 2],
            prev_view_proj: Mat4::IDENTITY,
            last_uniform: None,
            count,
            lighting,
        }
    }

    /// The ground mesh around `eye` (terrain heights, bilinear like the blade roots).
    pub fn build_ground(&mut self, device: &wgpu::Device, eye: Vec3) {
        let d = &self.terrain;
        let local = eye - self.origin;
        let lo = Vec2::new(
            (local.x - GROUND_HALF).max(0.0),
            (local.z - GROUND_HALF).max(0.0),
        );
        let hi = Vec2::new(
            (local.x + GROUND_HALF).min(d.size.x),
            (local.z + GROUND_HALF).min(d.size.z),
        );
        let nx = ((hi.x - lo.x) / GROUND_STEP) as u32 + 1;
        let nz = ((hi.y - lo.y) / GROUND_STEP) as u32 + 1;
        let mut verts: Vec<[f32; 3]> = Vec::with_capacity((nx * nz) as usize);
        for z in 0..nz {
            for x in 0..nx {
                let p = lo + Vec2::new(x as f32, z as f32) * GROUND_STEP;
                let y = d.sample_height(p.x, p.y);
                let w = self.origin + Vec3::new(p.x, y, p.y);
                verts.push(w.into());
            }
        }
        let mut idx: Vec<u32> = Vec::with_capacity(((nx - 1) * (nz - 1) * 6) as usize);
        for z in 0..nz - 1 {
            for x in 0..nx - 1 {
                let i = z * nx + x;
                idx.extend_from_slice(&[i, i + nx, i + 1, i + 1, i + nx, i + nx + 1]);
            }
        }
        self.ground_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("study ground vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.ground_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("study ground ib"),
            contents: bytemuck::cast_slice(&idx),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.ground_indices = idx.len() as u32;
    }

    /// Upload a field's blades (vertex + storage) and its draws.
    pub fn set_field(&mut self, device: &wgpu::Device, field: &Field) {
        let blades = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("study blades"),
            contents: bytemuck::cast_slice(&field.blades),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
        });
        let drawn = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("study drawn"),
            contents: bytemuck::cast_slice(&field.drawn),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let group = |params: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("study grass group"),
                layout: &self.layouts.grass,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.atlas.0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.atlas.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: blades.as_entire_binding(),
                    },
                ],
            })
        };
        self.grass_group = Some(group(&self.grass_buf));
        self.caster_grass_group = Some(group(&self.caster_grass_buf));
        self.blades = Some((blades, drawn, field.draws.clone()));
    }

    fn module(&self, device: &wgpu::Device, kind: &str) -> wgpu::ShaderModule {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(kind),
            source: wgpu::ShaderSource::Wgsl(module_source(kind, self.count).into()),
        })
    }

    /// The override constants of `variant` that `kind`'s module declares.
    fn constants(
        kind: &str,
        v: &Variant,
        extra: &[(&'static str, f64)],
    ) -> Vec<(&'static str, f64)> {
        let b = |x: bool| if x { 1.0 } else { 0.0 };
        let mut c = vec![
            ("CACHE", b(v.cache)),
            ("SPLIT", b(v.split.is_some())),
            ("CLUMP", v.clump as f64),
        ];
        if matches!(kind, "grass" | "ground" | "resolve" | "swr") {
            let stochastic = matches!(v.path, Path::Forward { samples: 1 } | Path::Vis { .. });
            c.push(("STOCHASTIC", b(stochastic)));
            c.push(("SWR", b(matches!(v.path, Path::Vis { swr: true, .. }))));
            let ms = match v.path {
                Path::VisMs { samples } => samples,
                _ => 4,
            };
            c.push(("MS_SAMPLES", ms as f64));
        }
        if kind == "ground" {
            c.push(("FAR_FIELD", b(v.far_field.is_some())));
        }
        if kind == "resolve" {
            let (swr, quarter) = match v.path {
                Path::Vis { swr, quarter } => (swr, quarter),
                _ => (false, false),
            };
            c.push(("SWR_ON", b(swr)));
            c.push(("QUARTER", b(quarter)));
        }
        c.extend_from_slice(extra);
        c
    }

    fn layout(
        &self,
        device: &wgpu::Device,
        groups: &[Option<&wgpu::BindGroupLayout>],
    ) -> wgpu::PipelineLayout {
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("study"),
            bind_group_layouts: groups,
            immediate_size: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_pipeline(
        &self,
        device: &wgpu::Device,
        kind: &str,
        v: &Variant,
        vs: &str,
        fs: Option<&str>,
        groups: &[Option<&wgpu::BindGroupLayout>],
        buffers: &[wgpu::VertexBufferLayout],
        topology: wgpu::PrimitiveTopology,
        targets: &[Option<wgpu::ColorTargetState>],
        depth: Option<wgpu::DepthStencilState>,
        samples: u32,
        a2c: bool,
    ) -> wgpu::RenderPipeline {
        let module = self.module(device, kind);
        let layout = self.layout(device, groups);
        let constants = Self::constants(kind, v, &[]);
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(vs),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(vs),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                buffers: &buffers.iter().cloned().map(Some).collect::<Vec<_>>(),
            },
            primitive: wgpu::PrimitiveState {
                topology,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: depth,
            multisample: wgpu::MultisampleState {
                count: samples,
                alpha_to_coverage_enabled: a2c && samples > 1,
                ..Default::default()
            },
            fragment: fs.map(|fs| wgpu::FragmentState {
                module: &module,
                entry_point: Some(fs),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets,
            }),
            multiview_mask: None,
            cache: crate::render::pipeline_cache(device).as_ref(),
        })
    }

    fn compute_pipeline(
        &self,
        device: &wgpu::Device,
        kind: &str,
        v: &Variant,
        entry: &str,
        groups: &[Option<&wgpu::BindGroupLayout>],
    ) -> wgpu::ComputePipeline {
        let module = self.module(device, kind);
        let layout = self.layout(device, groups);
        let constants = Self::constants(kind, v, &[]);
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            },
            cache: crate::render::pipeline_cache(device).as_ref(),
        })
    }

    fn key(name: &str, v: &Variant) -> String {
        format!(
            "{name}|{:?}|{}|{}|{}|{}",
            v.path,
            v.clump,
            v.far_field.is_some(),
            v.cache,
            v.split.is_some()
        )
    }

    /// Build (once) every pipeline `v` needs.
    fn ensure_pipelines(&mut self, device: &wgpu::Device, v: &Variant) {
        if self.pipelines.contains_key(&Self::key("taa", v)) {
            return;
        }
        let blade_attrs = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x2
        ];
        let blade_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::terrain::details::Blade>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &blade_attrs,
        };
        let ground_attrs = wgpu::vertex_attr_array![0 => Float32x3];
        let ground_layout = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ground_attrs,
        };
        let l = &self.layouts;
        let g01 = [Some(&l.study), Some(&l.grass)];
        let depth = |write: bool, compare| {
            Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let target = |format, writes| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: writes,
            })
        };
        let all = wgpu::ColorWrites::ALL;
        let none = wgpu::ColorWrites::empty();
        let strip = wgpu::PrimitiveTopology::TriangleStrip;
        let list = wgpu::PrimitiveTopology::TriangleList;
        use wgpu::CompareFunction as C;
        let mut made: Vec<(String, Pipeline)> = Vec::new();
        match v.path {
            Path::Forward { samples } => {
                let mrt = [target(HDR, all), target(HDR, all), target(VELOCITY, all)];
                let mrt_depth = [
                    target(HDR, wgpu::ColorWrites::ALPHA),
                    target(HDR, none),
                    target(VELOCITY, none),
                ];
                made.push((
                    "ground".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "ground",
                        v,
                        "vs_ground",
                        Some("fs_ground_mrt"),
                        &g01,
                        std::slice::from_ref(&ground_layout),
                        list,
                        &mrt,
                        depth(true, C::Less),
                        samples,
                        false,
                    )),
                ));
                made.push((
                    "grass depth".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "grass",
                        v,
                        "vs_grass_depth",
                        Some("fs_grass_depth"),
                        &g01,
                        std::slice::from_ref(&blade_layout),
                        strip,
                        &mrt_depth,
                        depth(true, C::Less),
                        samples,
                        true,
                    )),
                ));
                made.push((
                    "grass lit".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "grass",
                        v,
                        "vs_grass",
                        Some("fs_grass"),
                        &g01,
                        std::slice::from_ref(&blade_layout),
                        strip,
                        &mrt,
                        depth(false, C::LessEqual),
                        samples,
                        true,
                    )),
                ));
                if samples > 1 {
                    made.push((
                        "msaa".into(),
                        Pipeline::Render(self.render_pipeline(
                            device,
                            "post",
                            v,
                            "vs_full",
                            Some("fs_msaa_resolve"),
                            &[Some(&l.study), None, Some(&l.msaa)],
                            &[],
                            list,
                            &[target(HDR, all)],
                            depth(true, C::Always),
                            1,
                            false,
                        )),
                    ));
                }
            }
            Path::Vis { swr, .. } => {
                made.push((
                    "ground".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "ground",
                        v,
                        "vs_ground",
                        Some("fs_ground"),
                        &g01,
                        std::slice::from_ref(&ground_layout),
                        list,
                        &[target(HDR, all)],
                        depth(true, C::Less),
                        1,
                        false,
                    )),
                ));
                made.push((
                    "vis".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "grass",
                        v,
                        "vs_vis",
                        Some("fs_vis"),
                        &g01,
                        std::slice::from_ref(&blade_layout),
                        strip,
                        &[target(wgpu::TextureFormat::Rg32Uint, all)],
                        depth(true, C::Less),
                        1,
                        false,
                    )),
                ));
                made.push((
                    "resolve".into(),
                    Pipeline::Compute(self.compute_pipeline(
                        device,
                        "resolve",
                        v,
                        "cs_resolve",
                        &[Some(&l.study), Some(&l.grass), Some(&l.resolve)],
                    )),
                ));
                if swr {
                    made.push((
                        "swr".into(),
                        Pipeline::Compute(self.compute_pipeline(
                            device,
                            "swr",
                            v,
                            "cs_swr",
                            &[Some(&l.study), Some(&l.grass), Some(&l.swr)],
                        )),
                    ));
                }
            }
            Path::VisMs { samples } => {
                made.push((
                    "ground".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "ground",
                        v,
                        "vs_ground",
                        Some("fs_ground"),
                        &g01,
                        std::slice::from_ref(&ground_layout),
                        list,
                        &[target(HDR, all)],
                        depth(true, C::Less),
                        samples,
                        false,
                    )),
                ));
                made.push((
                    "vis".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "grass",
                        v,
                        "vs_vis",
                        Some("fs_vis4"),
                        &g01,
                        std::slice::from_ref(&blade_layout),
                        strip,
                        &[
                            target(wgpu::TextureFormat::R32Float, all),
                            target(VELOCITY, all),
                        ],
                        depth(true, C::Less),
                        samples,
                        false,
                    )),
                ));
                made.push((
                    "msaa".into(),
                    Pipeline::Render(self.render_pipeline(
                        device,
                        "post",
                        v,
                        "vs_full",
                        Some("fs_msaa_resolve"),
                        &[Some(&l.study), None, Some(&l.msaa)],
                        &[],
                        list,
                        &[target(HDR, all)],
                        depth(true, C::Always),
                        1,
                        false,
                    )),
                ));
                made.push((
                    "resolve".into(),
                    Pipeline::Compute(self.compute_pipeline(
                        device,
                        "resolve",
                        v,
                        "cs_resolve4",
                        &[Some(&l.study), Some(&l.grass), Some(&l.resolve4)],
                    )),
                ));
            }
        }
        if v.cache {
            made.push((
                "cache".into(),
                Pipeline::Compute(self.compute_pipeline(
                    device,
                    "cache",
                    v,
                    "cs_cache",
                    &[Some(&l.study), Some(&l.grass), Some(&l.cache)],
                )),
            ));
        }
        made.push((
            "taa".into(),
            Pipeline::Compute(self.compute_pipeline(
                device,
                "post",
                v,
                "cs_taa",
                &[Some(&l.study), None, None, Some(&l.taa)],
            )),
        ));
        for (name, p) in made {
            self.pipelines.insert(Self::key(&name, v), p);
        }
    }

    fn render_pipe(&self, name: &str, v: &Variant) -> &wgpu::RenderPipeline {
        match self.pipelines.get(&Self::key(name, v)) {
            Some(Pipeline::Render(p)) => p,
            _ => panic!("no render pipeline {name}"),
        }
    }

    fn compute_pipe(&self, name: &str, v: &Variant) -> &wgpu::ComputePipeline {
        match self.pipelines.get(&Self::key(name, v)) {
            Some(Pipeline::Compute(p)) => p,
            _ => panic!("no compute pipeline {name}"),
        }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: (u32, u32), samples: u32) {
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.size == size && t.samples == samples)
        {
            return;
        }
        use wgpu::TextureUsages as U;
        let ra = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING;
        let view = |t: wgpu::Texture| t.create_view(&Default::default());
        let single = if samples == 1 {
            U::STORAGE_BINDING
        } else {
            U::empty()
        };
        let color = view(texture(
            device,
            "study color",
            size,
            1,
            HDR,
            samples,
            ra | single,
        ));
        let glow = view(texture(device, "study glow", size, 1, HDR, samples, ra));
        let velocity = view(texture(
            device,
            "study velocity",
            size,
            1,
            VELOCITY,
            samples,
            ra,
        ));
        let depth = view(texture(device, "study depth", size, 1, DEPTH, samples, ra));
        let (color1, depth1) = if samples > 1 {
            (
                view(texture(
                    device,
                    "study color1",
                    size,
                    1,
                    HDR,
                    1,
                    ra | U::STORAGE_BINDING,
                )),
                view(texture(device, "study depth1", size, 1, DEPTH, 1, ra)),
            )
        } else {
            (color.clone(), depth.clone())
        };
        let vis = view(texture(
            device,
            "study vis",
            size,
            1,
            wgpu::TextureFormat::Rg32Uint,
            1,
            ra,
        ));
        let (vis_ms, vis_uv_ms) = if samples > 1 {
            (
                view(texture(
                    device,
                    "study vis ms",
                    size,
                    1,
                    wgpu::TextureFormat::R32Float,
                    samples,
                    ra,
                )),
                view(texture(
                    device,
                    "study vis uv ms",
                    size,
                    1,
                    VELOCITY,
                    samples,
                    ra,
                )),
            )
        } else {
            (vis.clone(), vis.clone())
        };
        let swr = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("study swr"),
            size: size.0 as u64 * size.1 as u64 * 8,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hist = || {
            view(texture(
                device,
                "study history",
                size,
                1,
                HDR,
                1,
                U::STORAGE_BINDING | U::TEXTURE_BINDING,
            ))
        };
        let ldr = texture(
            device,
            "study ldr",
            size,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
            1,
            U::STORAGE_BINDING | U::COPY_SRC,
        );
        self.targets = Some(Targets {
            size,
            samples,
            color,
            glow,
            velocity,
            depth,
            color1,
            depth1,
            vis,
            vis_ms,
            vis_uv_ms,
            swr,
            history: [hist(), hist()],
            ldr,
        });
    }

    /// Render the sun's cascades around `eye` (ground everywhere, grass casters in cascade 0)
    /// and fill the lighting cache's ground heights. Once per camera position.
    pub fn bake_shadows(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        eye: Vec3,
        grass: &GrassParams,
    ) {
        let dir = self.lighting.sun_dir.normalize();
        let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let ground_y = self.origin.y
            + self
                .terrain
                .sample_height(eye.x - self.origin.x, eye.z - self.origin.z);
        let centre = Vec3::new(eye.x, ground_y, eye.z);
        let make = |r: f32| {
            let view = glam::camera::lh::view::look_to_mat4(centre - dir * 600.0, dir, up);
            let proj = glam::camera::lh::proj::directx::orthographic(-r, r, -r, r, 1.0, 1200.0);
            // Snap to texels so the map is stable.
            let vp = proj * view;
            let o = vp.project_point3(Vec3::ZERO);
            let texel = 2.0 / SHADOW_SIZE as f32;
            let snap = Vec3::new(
                (o.x / texel).round() * texel - o.x,
                (o.y / texel).round() * texel - o.y,
                0.0,
            );
            Mat4::from_translation(snap) * vp
        };
        self.sun_vp = [make(CASCADE0), make(CASCADE1)];
        for (i, vp) in self.sun_vp.iter().enumerate() {
            queue.write_buffer(
                &self.caster_bufs[i],
                0,
                bytemuck::bytes_of(&vp.to_cols_array()),
            );
        }
        queue.write_buffer(&self.caster_grass_buf, 0, bytemuck::bytes_of(grass));
        let v = Variant::forward("shadow", "");
        let key = Self::key("shadow grass", &v);
        if !self.pipelines.contains_key(&key) {
            let blade_attrs = wgpu::vertex_attr_array![
                0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x2
            ];
            let blade_layout = wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<crate::terrain::details::Blade>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &blade_attrs,
            };
            let ground_attrs = wgpu::vertex_attr_array![0 => Float32x3];
            let ground_layout = wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &ground_attrs,
            };
            let groups = [
                Some(&self.layouts.study),
                Some(&self.layouts.grass),
                Some(&self.layouts.caster),
            ];
            let depth = |slope: f32| {
                Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: slope,
                        clamp: 0.0,
                    },
                })
            };
            let grass_p = self.render_pipeline(
                device,
                "shadow",
                &v,
                "vs_grass_shadow",
                Some("fs_grass_shadow"),
                &groups,
                &[blade_layout],
                wgpu::PrimitiveTopology::TriangleStrip,
                &[],
                depth(2.5),
                1,
                false,
            );
            let ground_p = self.render_pipeline(
                device,
                "shadow",
                &v,
                "vs_ground_shadow",
                None,
                &groups,
                &[ground_layout],
                wgpu::PrimitiveTopology::TriangleList,
                &[],
                depth(1.5),
                1,
                false,
            );
            self.pipelines
                .insert(key.clone(), Pipeline::Render(grass_p));
            self.pipelines
                .insert(Self::key("shadow ground", &v), Pipeline::Render(ground_p));
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        for i in 0..2 {
            let group2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("study caster"),
                layout: &self.layouts.caster,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.caster_bufs[i].as_entire_binding(),
                }],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("study shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_layers[i],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.group0_shadow, &[]);
            pass.set_bind_group(1, self.caster_grass_group.as_ref().expect("field"), &[]);
            pass.set_bind_group(2, &group2, &[]);
            pass.set_pipeline(self.render_pipe("shadow ground", &v));
            pass.set_vertex_buffer(0, self.ground_vb.slice(..));
            pass.set_index_buffer(self.ground_ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.ground_indices, 0, 0..1);
            if i == 0 {
                pass.set_pipeline(self.render_pipe("shadow grass", &v));
                let (blades, _, draws) = self.blades.as_ref().expect("field");
                pass.set_vertex_buffer(0, blades.slice(..));
                for d in draws.iter().filter(|d| d.dist < CASCADE0 * 1.2) {
                    pass.draw(100..100 + COARSE_VERTICES, d.first..d.first + d.count);
                }
            }
        }
        queue.submit([encoder.finish()]);
        // Cache ground heights: two levels centred on the eye.
        let mut heights = Vec::with_capacity((2 * CACHE_TEXELS * CACHE_TEXELS) as usize);
        for (level, step) in CACHE_STEP.iter().enumerate() {
            let half = step * CACHE_TEXELS as f32 * 0.5;
            let o = Vec2::new(eye.x - half, eye.z - half);
            self.cache_origin[level] = o;
            for y in 0..CACHE_TEXELS {
                for x in 0..CACHE_TEXELS {
                    let p = o + (Vec2::new(x as f32, y as f32) + 0.5) * *step;
                    let local = p - Vec2::new(self.origin.x, self.origin.z);
                    heights.push(self.origin.y + self.terrain.sample_height(local.x, local.y));
                }
            }
        }
        queue.write_buffer(&self.cache_ground, 0, bytemuck::cast_slice(&heights));
    }

    fn cache_group(&self, device: &wgpu::Device) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("study cache"),
            layout: &self.layouts.cache,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.cache_tex),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.cache_ground.as_entire_binding(),
                },
            ],
        })
    }

    /// Variant G: refresh the whole lighting cache now (every slice; after a first frame of
    /// `v` set the uniforms).
    pub fn fill_cache(&self, device: &wgpu::Device, queue: &wgpu::Queue, v: &Variant) {
        let Some(u) = self.last_uniform else {
            return;
        };
        let group2 = self.cache_group(device);
        for r in 0..CACHE_SLICES {
            let mut u2 = u;
            u2.frame[0] = r;
            queue.write_buffer(&self.study_buf, 0, bytemuck::bytes_of(&u2));
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(self.compute_pipe("cache", v));
                pass.set_bind_group(0, &self.group0_cache, &[]);
                pass.set_bind_group(1, self.grass_group.as_ref().expect("field"), &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.dispatch_workgroups(
                    CACHE_TEXELS.div_ceil(8),
                    (CACHE_TEXELS / CACHE_SLICES).div_ceil(8),
                    2,
                );
            }
            queue.submit([enc.finish()]);
        }
        queue.write_buffer(&self.study_buf, 0, bytemuck::bytes_of(&u));
    }

    /// Zero the fragment counters.
    pub fn reset_counters(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.counters, 0, &[0u8; 128]);
    }

    pub fn read_counters(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> [u32; 32] {
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.counters, 0, &self.counters_read, 0, 128);
        queue.submit([enc.finish()]);
        let slice = self.counters_read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let mut out = [0u32; 32];
        out.copy_from_slice(bytemuck::cast_slice(
            &slice.get_mapped_range().expect("mapped"),
        ));
        self.counters_read.unmap();
        out
    }

    /// Render one frame of `v` from `view` (frame `index`: TAA jitter, H's split, F's
    /// anchor, G's slice). `grass` = this frame's grass uniform. Pass times go to `timer`.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        v: &Variant,
        view: &View,
        jitter: Vec2,
        index: u32,
        size: (u32, u32),
        grass: &GrassParams,
        cloud_clock: f32,
        timer: Option<&PassTimer>,
    ) {
        let samples = match v.path {
            Path::Forward { samples } => samples,
            Path::Vis { .. } => 1,
            Path::VisMs { samples } => samples,
        };
        self.ensure_pipelines(device, v);
        self.ensure_targets(device, size, samples);
        let lit = self.lighting;
        let jit = Mat4::from_translation(Vec3::new(
            jitter.x * 2.0 / size.0 as f32,
            -jitter.y * 2.0 / size.1 as f32,
            0.0,
        ));
        let vp = jit * view.view_proj;
        let prev = if index == 0 {
            view.view_proj
        } else {
            self.prev_view_proj
        };
        let u = StudyUniform {
            view_proj: vp.to_cols_array_2d(),
            inv_view_proj: vp.inverse().to_cols_array_2d(),
            prev_view_proj: prev.to_cols_array_2d(),
            view_proj_nj: view.view_proj.to_cols_array_2d(),
            sun_vp0: self.sun_vp[0].to_cols_array_2d(),
            sun_vp1: self.sun_vp[1].to_cols_array_2d(),
            camera: [view.eye.x, view.eye.y, view.eye.z, cloud_clock],
            sun_dir: lit
                .sun_dir
                .normalize()
                .extend(if lit.shadows { 1.0 } else { 0.0 })
                .into(),
            sun_color: lit.sun.extend(CASCADE0).into(),
            sky: lit.sky.extend(lit.ambient).into(),
            equator: lit.equator.extend(0.0).into(),
            ground: lit.ground.extend(0.0).into(),
            fog: lit.fog.into(),
            screen: [
                size.0 as f32,
                size.1 as f32,
                1.0 / size.0 as f32,
                1.0 / size.1 as f32,
            ],
            frame: [index, 1, CACHE_SLICES, 0],
            params: [
                v.swr_px,
                v.far_field.unwrap_or(1e9),
                v.split.map_or(1e9, |s| s.0),
                v.split.map_or(1.0, |s| s.1),
            ],
            cache0: [
                self.cache_origin[0].x,
                self.cache_origin[0].y,
                CACHE_STEP[0],
                CACHE_TEXELS as f32,
            ],
            cache1: [
                self.cache_origin[1].x,
                self.cache_origin[1].y,
                CACHE_STEP[1],
                CACHE_TEXELS as f32,
            ],
        };
        self.prev_view_proj = view.view_proj;
        self.last_uniform = Some(u);
        queue.write_buffer(&self.study_buf, 0, bytemuck::bytes_of(&u));
        queue.write_buffer(&self.grass_buf, 0, bytemuck::bytes_of(grass));
        let t = self.targets.as_ref().expect("targets");
        let (blades, drawn, draws) = self.blades.as_ref().expect("field");
        let grass_group = self.grass_group.as_ref().expect("field");
        let mut enc = device.create_command_encoder(&Default::default());
        if let Some(timer) = timer {
            timer.begin(&mut enc);
        }
        let mark = |enc: &mut wgpu::CommandEncoder, label: &'static str| {
            if let Some(timer) = timer {
                timer.mark(enc, label);
            }
        };
        let draw_blades = |pass: &mut wgpu::RenderPass| {
            pass.set_vertex_buffer(0, blades.slice(..));
            for d in draws {
                if d.coarse {
                    pass.draw(100..100 + COARSE_VERTICES, d.first..d.first + d.count);
                } else {
                    pass.draw(0..BLADE_VERTICES, d.first..d.first + d.count);
                }
            }
        };
        let draw_ground = |pass: &mut wgpu::RenderPass| {
            pass.set_vertex_buffer(0, self.ground_vb.slice(..));
            pass.set_index_buffer(self.ground_ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.ground_indices, 0, 0..1);
        };
        let groups_xy = |n: u32| n.div_ceil(8);
        if v.cache {
            let group2 = self.cache_group(device);
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(self.compute_pipe("cache", v));
            pass.set_bind_group(0, &self.group0_cache, &[]);
            pass.set_bind_group(1, grass_group, &[]);
            pass.set_bind_group(2, &group2, &[]);
            pass.dispatch_workgroups(
                groups_xy(CACHE_TEXELS),
                groups_xy(CACHE_TEXELS / CACHE_SLICES),
                2,
            );
            drop(pass);
            mark(&mut enc, "G cache update");
        }
        fn clear<V>(load: wgpu::LoadOp<V>) -> wgpu::Operations<V> {
            wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            }
        }
        let sky = wgpu::Color {
            r: lit.fog.x as f64,
            g: lit.fog.y as f64,
            b: lit.fog.z as f64,
            a: 1.0,
        };
        match v.path {
            Path::Forward { samples } => {
                {
                    fn att(
                        view: &wgpu::TextureView,
                        c: wgpu::Color,
                    ) -> Option<wgpu::RenderPassColorAttachment<'_>> {
                        Some(wgpu::RenderPassColorAttachment {
                            view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(c),
                                store: wgpu::StoreOp::Store,
                            },
                        })
                    }
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("study main"),
                        color_attachments: &[
                            att(&t.color, sky),
                            att(&t.glow, wgpu::Color::TRANSPARENT),
                            att(&t.velocity, wgpu::Color::TRANSPARENT),
                        ],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.depth,
                            depth_ops: Some(clear(wgpu::LoadOp::Clear(1.0))),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(1, grass_group, &[]);
                    pass.set_pipeline(self.render_pipe("ground", v));
                    draw_ground(&mut pass);
                    if let Some(timer) = timer {
                        timer.mark_pass(&mut pass, "ground");
                    }
                    pass.set_pipeline(self.render_pipe("grass depth", v));
                    draw_blades(&mut pass);
                    if let Some(timer) = timer {
                        timer.mark_pass(&mut pass, "grass depth");
                    }
                    pass.set_pipeline(self.render_pipe("grass lit", v));
                    draw_blades(&mut pass);
                    if let Some(timer) = timer {
                        timer.mark_pass(&mut pass, "grass lit");
                    }
                }
                if samples > 1 {
                    let group2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("study msaa"),
                        layout: &self.layouts.msaa,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&t.color),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&t.depth),
                            },
                        ],
                    });
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("study msaa resolve"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &t.color1,
                            depth_slice: None,
                            resolve_target: None,
                            ops: clear(wgpu::LoadOp::Clear(wgpu::Color::BLACK)),
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.depth1,
                            depth_ops: Some(clear(wgpu::LoadOp::Clear(1.0))),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(self.render_pipe("msaa", v));
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(2, &group2, &[]);
                    pass.draw(0..3, 0..1);
                    drop(pass);
                    mark(&mut enc, "msaa resolve");
                }
            }
            Path::Vis { swr, .. } => {
                {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("study ground"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &t.color1,
                            depth_slice: None,
                            resolve_target: None,
                            ops: clear(wgpu::LoadOp::Clear(sky)),
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.depth1,
                            depth_ops: Some(clear(wgpu::LoadOp::Clear(1.0))),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(1, grass_group, &[]);
                    pass.set_pipeline(self.render_pipe("ground", v));
                    draw_ground(&mut pass);
                }
                mark(&mut enc, "ground");
                {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("study vis"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &t.vis,
                            depth_slice: None,
                            resolve_target: None,
                            ops: clear(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)),
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.depth1,
                            depth_ops: Some(clear(wgpu::LoadOp::Load)),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(1, grass_group, &[]);
                    pass.set_pipeline(self.render_pipe("vis", v));
                    draw_blades(&mut pass);
                }
                mark(&mut enc, "grass vis raster");
                if swr {
                    enc.clear_buffer(&t.swr, 0, None);
                    let group2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("study swr"),
                        layout: &self.layouts.swr,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&t.depth1),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: t.swr.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: drawn.as_entire_binding(),
                            },
                        ],
                    });
                    let n = (drawn.size() / 4) as u32;
                    let groups = n.div_ceil(64);
                    let gx = groups.min(65535);
                    let gy = groups.div_ceil(gx);
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(self.compute_pipe("swr", v));
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(1, grass_group, &[]);
                    pass.set_bind_group(2, &group2, &[]);
                    pass.dispatch_workgroups(gx, gy, 1);
                    drop(pass);
                    mark(&mut enc, "grass swr");
                }
                let group2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("study resolve"),
                    layout: &self.layouts.resolve,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&t.vis),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&t.depth1),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&t.color1),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: t.swr.as_entire_binding(),
                        },
                    ],
                });
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(self.compute_pipe("resolve", v));
                pass.set_bind_group(0, &self.group0, &[]);
                pass.set_bind_group(1, grass_group, &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.dispatch_workgroups(groups_xy(size.0), groups_xy(size.1), 1);
                drop(pass);
                mark(&mut enc, "grass resolve");
            }
            Path::VisMs { .. } => {
                {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("study ground 4x"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &t.color,
                            depth_slice: None,
                            resolve_target: None,
                            ops: clear(wgpu::LoadOp::Clear(sky)),
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.depth,
                            depth_ops: Some(clear(wgpu::LoadOp::Clear(1.0))),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(1, grass_group, &[]);
                    pass.set_pipeline(self.render_pipe("ground", v));
                    draw_ground(&mut pass);
                }
                mark(&mut enc, "ground");
                {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("study vis 4x"),
                        color_attachments: &[
                            Some(wgpu::RenderPassColorAttachment {
                                view: &t.vis_ms,
                                depth_slice: None,
                                resolve_target: None,
                                ops: clear(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)),
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &t.vis_uv_ms,
                                depth_slice: None,
                                resolve_target: None,
                                ops: clear(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)),
                            }),
                        ],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.depth,
                            depth_ops: Some(clear(wgpu::LoadOp::Load)),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(1, grass_group, &[]);
                    pass.set_pipeline(self.render_pipe("vis", v));
                    draw_blades(&mut pass);
                }
                mark(&mut enc, "grass vis raster");
                // Ground colour and nearest depth to one sample (TAA's inputs), then the
                // grass samples shaded over it.
                {
                    let group2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("study msaa"),
                        layout: &self.layouts.msaa,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&t.color),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&t.depth),
                            },
                        ],
                    });
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("study msaa resolve"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &t.color1,
                            depth_slice: None,
                            resolve_target: None,
                            ops: clear(wgpu::LoadOp::Clear(wgpu::Color::BLACK)),
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.depth1,
                            depth_ops: Some(clear(wgpu::LoadOp::Clear(1.0))),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(self.render_pipe("msaa", v));
                    pass.set_bind_group(0, &self.group0, &[]);
                    pass.set_bind_group(2, &group2, &[]);
                    pass.draw(0..3, 0..1);
                }
                mark(&mut enc, "msaa resolve");
                let group2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("study resolve4"),
                    layout: &self.layouts.resolve4,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&t.color1),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&t.vis_ms),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&t.depth),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::TextureView(&t.color),
                        },
                        wgpu::BindGroupEntry {
                            binding: 7,
                            resource: wgpu::BindingResource::TextureView(&t.vis_uv_ms),
                        },
                    ],
                });
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(self.compute_pipe("resolve", v));
                pass.set_bind_group(0, &self.group0, &[]);
                pass.set_bind_group(1, grass_group, &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.dispatch_workgroups(groups_xy(size.0), groups_xy(size.1), 1);
                drop(pass);
                mark(&mut enc, "grass resolve");
            }
        }
        // TAA (history ping-pong) and tonemapping.
        let (hist_in, hist_out) = (
            &t.history[(index % 2) as usize],
            &t.history[((index + 1) % 2) as usize],
        );
        let ldr = t.ldr.create_view(&Default::default());
        let group3 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("study taa"),
            layout: &self.layouts.taa,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&t.color1),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&t.depth1),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(hist_in),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(hist_out),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&ldr),
                },
            ],
        });
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(self.compute_pipe("taa", v));
            pass.set_bind_group(0, &self.group0, &[]);
            pass.set_bind_group(3, &group3, &[]);
            pass.dispatch_workgroups(groups_xy(size.0), groups_xy(size.1), 1);
        }
        mark(&mut enc, "taa");
        if let Some(timer) = timer {
            timer.resolve(&mut enc);
        }
        queue.submit([enc.finish()]);
    }

    /// The last frame's tonemapped image (RGBA8, sRGB-encoded).
    pub fn read_image(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let t = self.targets.as_ref().expect("targets");
        let (w, h) = t.size;
        let row = (w * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("study readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            t.ldr.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("mapped");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let s = (y * row) as usize;
            out.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        out
    }
}
