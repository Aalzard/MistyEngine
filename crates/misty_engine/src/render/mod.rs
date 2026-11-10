//! wgpu renderer: sky, lit meshes with shadows, grid, particles, gizmo lines and selection outline.

pub mod ao;
pub mod area;
mod batch;
mod cascades;
mod clouds;
pub mod color_filter;
pub mod contact_shadows;
pub mod culling;
pub mod custom_shader;
pub(crate) mod decals;
mod draw_prep;
pub mod environment;
pub mod fast_paths;
mod finish;
mod frame_debug;
mod frame_lists;
mod glow;
mod gpu_culling;
mod gpu_timer;
mod grass_cull;
mod grass_near_shadow;
mod grass_pool;
mod grass_vis;
mod hdri_sky;
mod light2d;
mod lightmap_gpu;
pub mod lod;
pub mod material_v2;
mod memory_items;
pub mod mesh;
mod misty_gi;
mod motion;
pub mod overrides;
pub mod particles;
mod pass_timer;
mod pipeline_cache;
mod pipelines;
pub mod post;
mod postfx;
mod prebuilt;
mod prepass;
pub mod probes;
mod runtime_tex;
mod scene_copy;
mod screen_space;
mod screen_trace;
mod shadow_denoise;
mod shadows;
mod skinning;
mod soft_shadows;
pub mod static_batch;
pub mod stats;
pub mod surface;
mod taa;
mod target;
mod terrain;
mod terrain_grass;
mod terrain_layers;
mod terrain_lod;
pub mod texture;
mod trees;
mod trees_cull;
pub mod ui;
mod variants;
pub(crate) mod vfx;
mod vgeo;
mod volumetrics;
mod vsm;
mod water;
mod wireframe;

pub use crate::terrain::TerrainDraw;
pub use ao::AoSettings;
pub use area::AreaShape;
pub use contact_shadows::ContactShadowSettings;
pub use decals::DecalDraw;
pub use environment::{Environment, Sun, Wind};
pub use frame_debug::{
    CaptureRequest, DrawLabel, EventKind, EventTarget, FrameCapture, FrameEvent, TargetInfo,
    TargetView,
};
pub use glow::GlowSource;
pub use grass_vis::{grass_vis_enabled, set_grass_vis};
pub use misty_gi::{MistyGiSettings, mesh_sdf_bakes_pending, mesh_sdf_cache_memory};
pub use particles::{ParticleBatch, ParticleInstance};
pub(crate) use pipeline_cache::pipeline_cache;
pub use pipeline_cache::{open_pipeline_cache, pipeline_cache_features, save_pipeline_cache};
pub use pipelines::{cache_shader_modules, pipelines_built};
pub use post::PostSettings;
pub use screen_trace::{SsgiSettings, SsrSettings};
pub use soft_shadows::LightShadow;
pub use stats::{FrameHistory, RenderMemory, RenderOptions, RenderStats};
pub use surface::{RenderingMode, ShaderKind, SortingKey};
pub use target::RenderTarget;
use target::TargetLayouts;
pub use terrain_grass::{GrassStats, grass_streaming, set_grass_streaming};
pub use vgeo::{VgeoChoice, VgeoDebug, VgeoStats};
pub use volumetrics::{FogDebug, FogSettings, LocalFog, SunShaftSettings, VolumetricLight};
pub use vsm::{VsmDebug, VsmStats};
pub use wireframe::{WirePath, Wireframe};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Projection};
use crate::models;
use crate::scene::{LightType, MeshRef, PrimitiveMesh, Shadows};

/// Final image shown in the editor.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Scene rendering format: values above 1 survive for bloom.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// MSAA samples of the main pass for the pipelines and targets made from now on: the
/// samples of the renderer at work (each `Renderer` keeps its own and sets this before it
/// builds anything; Quality ▸ Anti-Aliasing, 2× or 4×).
static SAMPLES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(4);

pub(crate) fn sample_count() -> u32 {
    SAMPLES.load(std::sync::atomic::Ordering::Relaxed)
}

fn use_samples(samples: u32) {
    SAMPLES.store(samples, std::sync::atomic::Ordering::Relaxed);
}

/// The main pass's MSAA samples for a quality level on `device`: its Anti-Aliasing.
/// Disabled renders 2× for now (the depth / velocity readers and the resolve need
/// multisampled targets); 2× needs the adapter's own format features (WebGPU guarantees
/// only 1× and 4× for the HDR targets), else 4×.
pub fn msaa_samples(device: &wgpu::Device, quality: &crate::quality::QualityLevel) -> u32 {
    let two = device
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
    match quality.msaa_samples() {
        n if n >= 4 || !two => 4,
        _ => 2,
    }
}

const MAX_LIGHTS: usize = 8;

/// Device features to request when the adapter has them: timestamp queries for GPU timing
/// (see [`Renderer::with_queue`]), indirect draws with an instance offset (GPU culling) and
/// the adapter's own format capabilities (2× MSAA of the HDR targets is not guaranteed by
/// WebGPU, only 1× and 4×), indirect draw counts (virtual geometry's indexed cluster draws),
/// BC texture compression and the wireframe draw modes' edges ([`wireframe`]).
pub fn timestamp_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    wireframe::wireframe_features(adapter)
        | adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
            | wgpu::Features::INDIRECT_FIRST_INSTANCE
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            // Virtual geometry draws each cluster as an indexed sub-draw, counted on the GPU,
            // and its visibility buffer names the triangle of each sample.
            | wgpu::Features::MULTI_DRAW_INDIRECT_COUNT
            | wgpu::Features::PRIMITIVE_INDEX
            // Block-compressed material textures (sprites::compress); without it they upload
            // uncompressed.
            | wgpu::Features::TEXTURE_COMPRESSION_BC)
}

/// `base` with the per-stage binding limits raised to what the adapter has (capped): the
/// lit fragment stage is close to WebGPU's guaranteed 16 sampled textures and 8 storage
/// buffers, and virtual geometry's material resolve adds its id targets and pool.
pub fn device_limits(adapter: &wgpu::Adapter, base: wgpu::Limits) -> wgpu::Limits {
    let has = adapter.limits();
    wgpu::Limits {
        max_sampled_textures_per_shader_stage: base
            .max_sampled_textures_per_shader_stage
            .max(has.max_sampled_textures_per_shader_stage.min(32)),
        max_storage_buffers_per_shader_stage: base
            .max_storage_buffers_per_shader_stage
            .max(has.max_storage_buffers_per_shader_stage.min(16)),
        ..base
    }
}

/// World plane the editor grid lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridPlane {
    /// Normal +X (side views).
    YZ,
    /// Normal +Y (default ground grid).
    XZ,
    /// Normal +Z (2D mode, front/back views).
    XY,
}

impl GridPlane {
    fn axis(self) -> usize {
        match self {
            GridPlane::YZ => 0,
            GridPlane::XZ => 1,
            GridPlane::XY => 2,
        }
    }
}

/// One gizmo line vertex; colour is sRGB with alpha.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// Surface parameters of a draw (from a material asset).
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialParams {
    /// sRGB base colour + alpha.
    pub color: [f32; 4],
    pub metallic: f32,
    pub smoothness: f32,
    /// sRGB emission colour.
    pub emission: [f32; 3],
    /// Linear multiplier on the emission colour (HDR: values above 1 glow through bloom).
    pub emission_intensity: f32,
    /// Project-relative albedo texture (sRGB), multiplied by `color`.
    pub albedo_map: Option<String>,
    /// Project-relative tangent-space normal map (linear).
    pub normal_map: Option<String>,
    pub normal_scale: f32,
    pub tiling: [f32; 2],
    pub offset: [f32; 2],
    /// UV rotation in degrees about the texture centre (per-object override; materials use 0).
    pub uv_rotation: f32,
    /// Unlit shaders: colour × albedo texture, no lighting or shadows.
    pub unlit: bool,
    pub mode: RenderingMode,
    /// Cutout: pixels with alpha below this are discarded.
    pub alpha_cutoff: f32,
    /// Cull Off.
    pub double_sided: bool,
    /// Smoothness from the albedo alpha (else from the metallic map's alpha).
    pub smoothness_from_albedo: bool,
    /// R = metallic, A = smoothness (linear). With a map, `metallic` is 1 and `smoothness` scales A.
    pub metallic_map: Option<String>,
    /// G = ambient occlusion (linear), applied to ambient light and reflections.
    pub occlusion_map: Option<String>,
    pub occlusion_strength: f32,
    /// Multiplied by the emission colour (sRGB).
    pub emission_map: Option<String>,
    /// A `.shader` asset replaces the built-in shading (see [`custom_shader`]).
    pub custom: Option<custom_shader::CustomMaterial>,
    /// Sprite-Lit-Default: unlit sprites and tiles multiplied by the 2D lights of their
    /// Sorting Layer (only while the scene has 2D lights, see [`crate::light2d`]).
    pub lit_2d: bool,
    /// Per-object: baked lightmap layer and uv2 scale-offset (see [`crate::lightmap`]).
    pub lightmap: Option<crate::lightmap::DrawLightmap>,
    /// Per-object: ambient light comes from the baked light probes (see
    /// [`crate::light_probes`]); set by the draw list for non-static Blend Probes renderers.
    pub light_probes: bool,
    /// Per-object (Mesh Renderer ▸ Contribute Global Illumination): the surface sends its
    /// light to others through Screen Space GI. Off for meshes that only show a light's
    /// emitter (the light already lights the scene; tracing its glow too counts it twice).
    pub contribute_gi: bool,
    /// Optional Standard features (detail maps, parallax, clear coat, translucency,
    /// anisotropy, specular tint): `None` for materials that use none of them, which then
    /// draw with the plain Standard variants (see [`material_v2`]).
    pub extras: Option<std::sync::Arc<material_v2::LitExtras>>,
}

impl Default for MaterialParams {
    fn default() -> Self {
        Self {
            color: [1.0; 4],
            metallic: 0.0,
            smoothness: 0.5,
            emission: [0.0; 3],
            emission_intensity: 0.0,
            albedo_map: None,
            normal_map: None,
            normal_scale: 1.0,
            tiling: [1.0, 1.0],
            offset: [0.0, 0.0],
            uv_rotation: 0.0,
            unlit: false,
            mode: RenderingMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            smoothness_from_albedo: false,
            metallic_map: None,
            occlusion_map: None,
            occlusion_strength: 1.0,
            emission_map: None,
            custom: None,
            lit_2d: false,
            lightmap: None,
            light_probes: false,
            contribute_gi: true,
            extras: None,
        }
    }
}

impl MaterialParams {
    /// Render queue: the custom shader's, else the rendering mode's.
    pub fn render_queue(&self) -> u32 {
        self.custom
            .as_ref()
            .map_or(self.mode.render_queue(), |c| c.queue)
    }

    pub fn pipeline_key(&self) -> surface::PipelineKey {
        surface::PipelineKey {
            unlit: self.unlit,
            mode: self.mode,
            double_sided: self.double_sided,
            features: self.features(),
        }
    }

    /// The optional features this material's lit variant compiles ([`material_v2::feature`]
    /// bits; 0 for unlit and `.shader` materials).
    pub fn features(&self) -> u8 {
        match &self.extras {
            Some(e) if !self.unlit && self.custom.is_none() => e.features,
            _ => 0,
        }
    }

    /// Group 2 of a feature variant (`None` = the plain Standard set, [`Self::textures`]).
    pub fn lit_set(&self) -> Option<material_v2::LitSet> {
        let extras = self.extras.as_ref().filter(|_| self.features() != 0)?;
        Some(material_v2::LitSet::new(self.textures(), extras))
    }

    /// Shadow caster variant; `None` = casts no shadow (unlit and blended materials).
    pub fn shadow_key(&self) -> Option<surface::ShadowKey> {
        (!self.unlit && !self.mode.is_blended()).then_some(surface::ShadowKey {
            cutout: self.mode == RenderingMode::Cutout,
            double_sided: self.double_sided,
        })
    }

    /// The textures bound at group 2.
    pub fn textures(&self) -> texture::TextureSet {
        texture::TextureSet {
            albedo: self.albedo_map.clone(),
            normal: self.normal_map.clone(),
            metallic: self.metallic_map.clone(),
            occlusion: self.occlusion_map.clone(),
            emission: self.emission_map.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshDraw {
    pub mesh: MeshRef,
    pub model: Mat4,
    pub material: MaterialParams,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// Draw the orange selection outline around this mesh.
    pub outline: bool,
    /// Sorting Layer / Order in Layer (sprites); meshes use the default.
    pub sorting: surface::SortingKey,
    /// Level of a LOD Group (`None` = always drawn); the renderer picks levels per camera.
    pub lod: Option<lod::LodRange>,
}

#[derive(Clone, Copy, Debug)]
pub struct LightData {
    pub kind: LightType,
    pub position: Vec3,
    /// Direction the light travels (the light's forward axis).
    pub direction: Vec3,
    /// sRGB colour.
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    /// Full cone angle in degrees.
    pub spot_angle: f32,
    pub shadows: Shadows,
    /// Source size and biases (soft shadows).
    pub shadow: LightShadow,
    /// Volumetric fog multiplier and shadow dimmer.
    pub volumetric: VolumetricLight,
    /// Screen-space contact shadows (used when the view's Contact Shadows are on).
    pub contact_shadows: bool,
    /// Rectangle lights: size, axes, sides and the emitter quad.
    pub area: AreaShape,
}

/// How the main pass is split: GPU culling draws it in two phases (`Some(0)`, `Some(1)`)
/// around the Hi-Z build; `prepass` = depth is already drawn; `scene_copy` = see the field.
#[derive(Clone, Copy)]
struct MainSplit<'a> {
    phase: Option<usize>,
    culled: Option<&'a gpu_culling::CulledBatches>,
    prepass: bool,
    /// The transparent part follows a copy of the opaque scene (water, scene-reading shaders).
    scene_copy: bool,
    /// SSGI / SSR: the opaque scene is also copied into their lit colour history.
    trace: Option<&'a screen_trace::TraceFrame>,
    /// The lit grass goes through the visibility buffer (grass_vis.rs): the last phase ends
    /// after the grass depth, the visibility raster and resolve run, and the rest of the
    /// main pass starts with the grass composite.
    vis: bool,
}

/// Everything needed to draw one view.
pub struct Frame<'a> {
    pub camera: &'a Camera,
    pub grid: Option<GridPlane>,
    /// Draw the environment's skybox (else clear to `background`).
    pub skybox: bool,
    /// sRGB background colour used when the skybox is off.
    pub background: [f32; 3],
    /// Sky, ambient, reflections and fog (Lighting window).
    pub environment: &'a Environment,
    /// Bloom, grading, tonemapping and vignette.
    pub post: PostSettings,
    pub lines: &'a [LineVertex],
    /// Translucent triangles drawn over the scene (depth-tested, e.g. the nav mesh overlay).
    pub overlay: &'a [LineVertex],
    pub meshes: &'a [MeshDraw],
    /// Particle systems with live particles (drawn after opaque meshes).
    pub particles: &'a [ParticleBatch],
    /// Terrains (lit, shadowed, drawn with the opaque meshes).
    pub terrains: &'a [TerrainDraw],
    /// Draw shaded meshes (false in the Wireframe draw mode; outlines still work).
    pub shaded: bool,
    pub lights: &'a [LightData],
    /// Water surfaces (oceans, lakes, rivers), drawn after the opaque scene.
    pub water: &'a [crate::water::WaterDraw],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    grid: [f32; 4],
    background: [f32; 4],
    environment: [[f32; 4]; 8],
    post: [[f32; 4]; 6],
    /// Last frame's unjittered view-projection (motion vectors, see [`motion`]).
    prev_view_proj: [[f32; 4]; 4],
    /// xy = this frame's jitter in NDC, zw = target size.
    motion: [f32; 4],
    /// Camera rotation (quaternion) and the tangents of half the field of view (x, y): view
    /// rays without inverting the view-projection (see [`view_ray`]).
    view_ray: [[f32; 4]; 2],
}

/// One `draws[]` entry (320 bytes). Its layout (and the only WGSL copy of it) is documented
/// in shaders/draw.wgsl; new per-draw values go there and here, at the end.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    color: [f32; 4],
    params: [f32; 4],
    emission: [f32; 4],
    uv_st: [f32; 4],
    /// x = rendering mode index, y = alpha cutoff, z = occlusion strength,
    /// w = 1 when smoothness comes from the albedo alpha.
    surface: [f32; 4],
    /// x = cos, y = sin of the UV rotation, z = LOD cross-fade, w = 2D-lit layer flag
    /// ([`light2d::draw_flag`]).
    extra: [f32; 4],
    /// Lightmap uv = uv2 × xy + zw.
    lightmap_st: [f32; 4],
    /// x = lightmap layer + 1 (0 = not lightmapped).
    lightmap: [f32; 4],
    /// Last frame's object → world (motion vectors; the same as `model` when still).
    prev_model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightItem {
    pos_range: [f32; 4],
    dir_type: [f32; 4],
    color: [f32; 4],
    spot: [f32; 4],
    shadow: [f32; 4],
    /// Rectangle: xyz = half-width axis, w = 1 when two-sided.
    area_x: [f32; 4],
    /// Rectangle: xyz = half-height axis.
    area_y: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightsUniform {
    info: [f32; 4],
    shadow_view_proj: [[f32; 4]; 4],
    sky: [f32; 4],
    equator: [f32; 4],
    ground: [f32; 4],
    items: [LightItem; MAX_LIGHTS],
    local_view_proj: [[[f32; 4]; 4]; shadows::MAX_LOCAL_LAYERS],
}

struct GpuMesh {
    /// Primitives share one vertex/index buffer pair (so GPU-culled batches of different
    /// primitives can be one multi-draw); imported meshes have their own.
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Same id = same buffers.
    buffer_id: u64,
    first_index: u32,
    base_vertex: i32,
    count: u32,
    vertex_count: u32,
    /// Local bounds (culling, shadow fitting).
    bounds: culling::Aabb,
    /// Vertex colours (RGBA8, parallel to `vertices`) when the mesh has them; custom-shader
    /// draws of meshes without bind the renderer's white buffer instead.
    colors: Option<wgpu::Buffer>,
}

pub struct Renderer {
    pipelines: pipelines::Pipelines,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,
    shadows: shadows::ShadowMaps,
    /// Reflection probe atlas, lists and BRDF LUT (mesh group bindings 20–24).
    probes: probes::gpu::ProbeGpu,
    /// Baked lightmaps (mesh group bindings 25–26).
    lightmaps: lightmap_gpu::LightmapGpu,
    /// Group 2 of the terrain pass (no bindings).
    empty_group: wgpu::BindGroup,
    lights_buffer: wgpu::Buffer,
    /// 2D lights and shadow maps (storage, mesh group binding 30, see [`light2d`]).
    lights_2d: wgpu::Buffer,
    /// Rectangle lights' LTC lookup table (storage, mesh group binding 7).
    ltc_table: wgpu::Buffer,
    draws_buffer: wgpu::Buffer,
    draws_capacity: usize,
    /// Instance → draw slot indices (see [`batch`]).
    slots_buffer: wgpu::Buffer,
    slots_capacity: usize,
    mesh_bind_group: wgpu::BindGroup,
    draws_bind_group: wgpu::BindGroup,
    line_buffer: wgpu::Buffer,
    line_capacity: usize,
    /// Line and overlay vertices written this frame (capped to the largest buffer allowed).
    lines_drawn: u32,
    overlay_drawn: u32,
    particle_instances: particles::InstanceBuffer,
    terrains: terrain::TerrainCache,
    /// Terrain trees (render/trees.rs).
    trees: trees::TreeRenderer,
    /// `.shader` materials (created on first use: needs the queue).
    custom: Option<custom_shader::CustomShaders>,
    /// Opaque scene copies for water and scene-sampling shaders.
    scene_copies: scene_copy::SceneCopies,
    water: water::WaterRenderer,
    meshes: Vec<GpuMesh>,
    /// Imported meshes by "path#index" (None = could not be loaded), uploaded on first use.
    imported: std::collections::HashMap<String, Option<GpuMesh>>,
    /// [`models::generation`] the imported meshes were uploaded at.
    imported_generation: u64,
    textures: texture::TextureCache,
    /// Linear clamp sampler for post-processing.
    post_sampler: wgpu::Sampler,
    options: RenderOptions,
    quality: crate::quality::QualityLevel,
    /// Temporal anti-aliasing histories per render target.
    taa: taa::Taa,
    /// Last frame's camera, draws and grass clock per target (motion vectors).
    motion: motion::Motion,
    stats: RenderStats,
    /// GPU time per render (when the device supports timestamp queries).
    gpu_timer: Option<gpu_timer::GpuTimer>,
    /// GPU time per part of a render: blocking (benchmarks, see [`Renderer::set_pass_timing`])
    /// or, while the Profiler records, read back later for its GPU module.
    pass_timer: Option<pass_timer::PassTimer>,
    /// Name of the view being rendered, for the Profiler's GPU module ("Game", "Scene").
    view_label: std::borrow::Cow<'static, str>,
    /// GPU culling pipelines and buffers (`None` when the device can't draw indirectly with
    /// an instance offset).
    culler: Option<gpu_culling::GpuCuller>,
    /// Batches of the last GPU-culled frame.
    last_gpu_batches: usize,
    /// Misty Virtual Geometry: cluster culling and drawing of virtual meshes (render/vgeo).
    vgeo: vgeo::VirtualGeometry,
    prepass: prepass::Prepass,
    // Per-frame scratch kept to avoid reallocating.
    batcher: batch::Batcher,
    shadow_batchers: Vec<batch::Batcher>,
    slots_scratch: Vec<u32>,
    /// Froxel fog, height fog and sun shafts.
    volumetrics: volumetrics::Volumetrics,
    /// Volumetric clouds (trace, temporal resolve, shadow map).
    clouds: clouds::Clouds,
    /// The view group must be rebuilt (fog volume resized) before this render's passes.
    view_group_dirty: bool,
    /// Ambient occlusion and contact shadows (depth/normal prepass, GTAO passes).
    screen_space: screen_space::ScreenSpace,
    /// Screen-space GI and reflections (trace passes, lit colour history).
    screen_trace: screen_trace::ScreenTrace,
    shadow_denoise: shadow_denoise::ShadowDenoise,
    /// Light halos and lens flares.
    glows: glow::Glows,
    /// Auto Exposure, Depth of Field and Motion Blur (after the temporal resolve).
    postfx: postfx::PostFx,
    /// Decal Projectors (decal resolve after the screen-space prepass).
    decals: decals::Decals,
    /// White vertex colours for custom-shader draws of meshes without colours (as many as the
    /// largest base vertex + vertex count drawn so far).
    white_colors: Option<wgpu::Buffer>,
    /// HDRI sky cubes and uniform (view group bindings 4–7).
    sky: hdri_sky::SkyGpu,
    /// Posed Skinned Mesh Renderers (GPU skinning output, see [`skinning`]).
    skins: skinning::Skins,
    /// Visual Effect particles (GPU simulation and indirect draws, see [`vfx`]).
    effects: vfx::Effects,
    /// Frame Debugger recorder (`None` = not capturing; every hook is then a no-op).
    capture: frame_debug::Capture,
    /// Replay display pipelines (made on the first replay).
    debug_blit: Option<frame_debug::DebugBlit>,
    /// The view-projection the last render drew with (jitter included), for tests.
    #[cfg(test)]
    last_view_proj: Mat4,
    /// The sun's cascades and near grass shadow map slices of the last render, for tests.
    #[cfg(test)]
    last_shadow_views: (
        Vec<cascades::Cascade>,
        Option<[Mat4; grass_near_shadow::SLICES]>,
    ),
    /// MSAA samples of the main pass this renderer was built for (see [`sample_count`]).
    samples: u32,
    /// The sun's virtual shadow map (Quality ▸ Shadows ▸ Virtual, see [`vsm`]).
    vsm: vsm::Vsm,
    /// Page table and pool bound while the virtual shadow map is off.
    vsm_dummy: vsm::VsmDummy,
    /// Renders so far (the virtual shadow map's request ages).
    vsm_frame: u64,
    /// Grass visibility buffer pipelines (made with the first grass; `Some(None)` = this
    /// device refused them and the grass keeps the lit pass).
    grass_vis: Option<Option<grass_vis::VisPipelines>>,
    /// The grass visibility pipelines being built on a worker (see `prebuilt`).
    grass_vis_prebuilt: prebuilt::Prebuilt<Option<grass_vis::VisPipelines>>,
    /// How this device draws the wireframe draw modes' edges (`None` = it can't: the editor
    /// draws CPU lines), the next render's request, this render's, and whether this render is
    /// the Wireframe mode (geometry for its depth only, see [`wireframe`]).
    wire_path: Option<WirePath>,
    wire_next: Option<Wireframe>,
    wire: Option<Wireframe>,
    wire_only: bool,
    /// Built on the first wireframe render.
    wire_pipelines: Option<wireframe::WirePipelines>,
}

impl Renderer {
    /// Renderer without GPU timing (see [`Renderer::with_queue`]).
    pub fn new(device: &wgpu::Device) -> Self {
        let pipelines = pipelines::Pipelines::new(device);
        let taa = taa::Taa::new(device, &pipelines.view_layout);
        let uniform = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let view_buffer = uniform("misty view", std::mem::size_of::<ViewUniform>());
        let lights_buffer = uniform("misty lights", std::mem::size_of::<LightsUniform>());
        let volumetrics = volumetrics::Volumetrics::new(device);
        let sky = hdri_sky::SkyGpu::new(device);
        let clouds = clouds::Clouds::new(device);
        let view_bind_group = volumetrics::view_group(
            device,
            &pipelines.view_layout,
            &view_buffer,
            &volumetrics,
            &sky,
            &clouds,
        );
        let shadows = shadows::ShadowMaps::new(device, &pipelines.shadow_view_layout);
        let probes = probes::gpu::ProbeGpu::new(device);
        let lightmaps = lightmap_gpu::LightmapGpu::new(device);
        let screen_space = screen_space::ScreenSpace::new(device);
        let screen_trace = screen_trace::ScreenTrace::new(device);
        let shadow_denoise = shadow_denoise::ShadowDenoise::new(device);

        let draws_capacity = 256;
        let draws_buffer = draws_buffer(device, draws_capacity);
        let slots_capacity = 1024;
        let slots_buffer = slots_buffer(device, slots_capacity);
        let ltc_table = area::table_buffer(device);
        let lights_2d = light2d::buffer(device);
        let vsm_dummy = vsm::VsmDummy::new(device);
        let (mesh_bind_group, draws_bind_group) = draw_groups(
            device,
            &pipelines,
            [&lights_buffer, &ltc_table, &lights_2d],
            [&draws_buffer, &slots_buffer],
            &shadows,
            &probes,
            &lightmaps,
            [
                &screen_space.off_buffer,
                &screen_trace.off_buffer,
                shadow_denoise.off_buffer(),
            ],
            [
                &vsm_dummy.table_view,
                &vsm_dummy.pool_view,
                &vsm_dummy.pool_view,
            ],
        );

        let meshes = GpuMesh::primitives(device);

        let line_capacity = 1024;
        let post_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty post sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut renderer = Self {
            post_sampler,
            view_buffer,
            view_bind_group,
            shadows,
            probes,
            lightmaps,
            empty_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty empty group"),
                layout: &pipelines.empty_layout,
                entries: &[],
            }),
            lights_buffer,
            lights_2d,
            ltc_table,
            draws_buffer,
            draws_capacity,
            slots_buffer,
            slots_capacity,
            mesh_bind_group,
            draws_bind_group,
            line_buffer: line_buffer(device, line_capacity),
            line_capacity,
            lines_drawn: 0,
            overlay_drawn: 0,
            particle_instances: particles::InstanceBuffer::new(device),
            terrains: terrain::TerrainCache::new(device),
            trees: trees::TreeRenderer::new(device),
            custom: None,
            scene_copies: scene_copy::SceneCopies::new(device),
            water: Default::default(),
            meshes,
            imported: Default::default(),
            imported_generation: models::generation(),
            textures: texture::TextureCache::new(device),
            pipelines,
            options: RenderOptions::default(),
            // High: the default level, with the 2048² shadow map created above.
            // TAA starts off: it is on once an app applies a level that wants it
            // (repeated renders of a bare renderer stay identical).
            quality: crate::quality::QualityLevel {
                temporal_aa: false,
                ..Default::default()
            },
            taa,
            motion: Default::default(),
            stats: RenderStats::default(),
            gpu_timer: None,
            pass_timer: None,
            view_label: std::borrow::Cow::Borrowed(""),
            culler: gpu_culling::supported(device).then(|| gpu_culling::GpuCuller::new(device)),
            last_gpu_batches: 0,
            vgeo: Default::default(),
            prepass: Default::default(),
            batcher: Default::default(),
            shadow_batchers: Vec::new(),
            slots_scratch: Vec::new(),
            volumetrics,
            clouds,
            view_group_dirty: false,
            screen_space,
            screen_trace,
            shadow_denoise,
            glows: Default::default(),
            postfx: Default::default(),
            decals: Default::default(),
            white_colors: None,
            sky,
            skins: Default::default(),
            effects: Default::default(),
            capture: None,
            debug_blit: None,
            #[cfg(test)]
            last_view_proj: Mat4::IDENTITY,
            #[cfg(test)]
            last_shadow_views: Default::default(),
            samples: sample_count(),
            grass_vis: None,
            grass_vis_prebuilt: Default::default(),
            wire_path: WirePath::of(device),
            wire_next: None,
            wire: None,
            wire_only: false,
            wire_pipelines: None,
            vsm: Default::default(),
            vsm_dummy,
            vsm_frame: 0,
        };
        // Streaming (editor, player): compile what a scene's first frame would otherwise
        // wait for on worker threads now (see `prebuilt`).
        if grass_streaming() {
            renderer.trees.prebuild(device, &renderer.pipelines);
            renderer.terrains.prebuild(device);
            renderer.grass_vis_prebuilt =
                grass_vis::VisPipelines::prebuild(device, &renderer.pipelines);
            terrain_grass::prewarm_atlas();
            clouds::noise::prewarm_blue_noise();
        }
        renderer
    }

    /// The virtual shadow map's page table (or the dummy while it is off).
    fn vsm_table_view(&self) -> &wgpu::TextureView {
        self.vsm
            .gpu
            .as_ref()
            .map_or(&self.vsm_dummy.table_view, |g| &g.table_view)
    }

    /// The virtual shadow map's physical page pool (or the dummy while it is off).
    fn vsm_pool_view(&self) -> &wgpu::TextureView {
        self.vsm
            .gpu
            .as_ref()
            .map_or(&self.vsm_dummy.pool_view, |g| &g.pool_view)
    }

    /// The virtual shadow map's near layer (or the dummy while it is off).
    fn vsm_near_view(&self) -> &wgpu::TextureView {
        self.vsm
            .gpu
            .as_ref()
            .map_or(&self.vsm_dummy.pool_view, |g| &g.near_view)
    }

    /// Rebuild the lit pass's group 1 (shadow maps or buffers changed).
    fn rebuild_draw_groups(&mut self, device: &wgpu::Device) {
        (self.mesh_bind_group, self.draws_bind_group) = draw_groups(
            device,
            &self.pipelines,
            [&self.lights_buffer, &self.ltc_table, &self.lights_2d],
            [&self.draws_buffer, &self.slots_buffer],
            &self.shadows,
            &self.probes,
            &self.lightmaps,
            [
                &self.screen_space.off_buffer,
                &self.screen_trace.off_buffer,
                self.shadow_denoise.off_buffer(),
            ],
            [
                self.vsm_table_view(),
                self.vsm_pool_view(),
                self.vsm_near_view(),
            ],
        );
    }

    /// Scene view ▸ Virtual Shadow debug view (levels, cached pages, page table) for the next
    /// render only.
    pub fn set_vsm_debug(&mut self, debug: VsmDebug) {
        self.vsm.debug = debug;
    }

    /// Page counts of the last render's virtual shadow map (None = cascaded shadows).
    pub fn vsm_stats(&self) -> Option<VsmStats> {
        self.vsm.gpu.as_ref().map(|_| self.vsm.stats)
    }

    /// Point the view group at the current fog volume, HDRI sky cubes and cloud images.
    fn rebuild_view_group(&mut self, device: &wgpu::Device) {
        self.view_bind_group = volumetrics::view_group(
            device,
            &self.pipelines.view_layout,
            &self.view_buffer,
            &self.volumetrics,
            &self.sky,
            &self.clouds,
        );
        self.view_group_dirty = false;
    }

    /// Renderer that also measures GPU time per render when the device was created with
    /// `TIMESTAMP_QUERY` (see [`timestamp_features`]).
    pub fn with_queue(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mut renderer = Self {
            gpu_timer: gpu_timer::GpuTimer::new(device, queue),
            ..Self::new(device)
        };
        crate::profiler::set_gpu_timing(if pass_timer::supported(device) {
            crate::profiler::GpuTiming::Passes
        } else if renderer.gpu_timer.is_some() {
            crate::profiler::GpuTiming::FrameOnly
        } else {
            crate::profiler::GpuTiming::Unsupported
        });
        if grass_streaming() {
            let view_layout = &renderer.pipelines.view_layout;
            (renderer.volumetrics).prebuild(device, queue, view_layout);
            renderer.clouds.prebuild(device, queue);
        }
        renderer
    }

    /// Culling / instancing switches.
    pub fn options(&self) -> RenderOptions {
        self.options
    }

    pub fn set_options(&mut self, options: RenderOptions) {
        self.options = options;
    }

    /// The quality level the renderer draws with (see [`crate::quality`]).
    pub fn quality(&self) -> &crate::quality::QualityLevel {
        &self.quality
    }

    /// Use a quality level: resizes the directional shadow map when its resolution changes.
    /// Effect settings (cascades, soft shadows, AO, fog, probes) are read from
    /// [`Renderer::quality`] by the passes that implement them.
    pub fn set_quality(&mut self, device: &wgpu::Device, quality: &crate::quality::QualityLevel) {
        // Quality ▸ Anti-Aliasing: another sample count rebuilds the renderer (pipelines and
        // GPU caches; meshes, textures and grass load again over the next frames). Targets
        // made before no longer fit (`target_fits`): the hosts make new ones.
        let samples = msaa_samples(device, quality);
        if samples != self.samples {
            use_samples(samples);
            let mut fresh = Self::new(device);
            fresh.gpu_timer = self.gpu_timer.take();
            fresh.pass_timer = self.pass_timer.take();
            fresh.options = self.options;
            fresh.capture = self.capture.take();
            *self = fresh;
        }
        use_samples(self.samples);
        if !quality.temporal() {
            self.taa.clear();
        }
        // Virtual shadow maps replace the sun's cascades (small layers stay for the fog, filled
        // from the virtual map).
        let virtual_sun = quality.shadow_mode == crate::quality::ShadowMode::Virtual;
        let size = if virtual_sun {
            vsm::fog::FOG_SIZE
        } else {
            quality.shadow_map_size()
        };
        let near = quality.grass_shadow_map_size();
        let mut rebuild = false;
        if size != self.shadows.directional_size || near != self.shadows.grass_near.size {
            let layout = &self.pipelines.shadow_view_layout;
            self.shadows = shadows::ShadowMaps::with_size(device, layout, size, near);
            rebuild = true;
        }
        rebuild |= if virtual_sun {
            let pages = quality.shadow_pool.pages();
            self.vsm.ensure_gpu(device, &self.pipelines, pages)
        } else {
            self.vsm.release()
        };
        if rebuild {
            self.rebuild_draw_groups(device);
        }
        // Quality ▸ LOD Bias scales every LOD Group's switch heights.
        self.options.lod_bias = quality.lod_bias;
        self.vgeo.enabled = quality.virtual_geometry;
        self.vgeo.threshold = quality.virtual_geometry_error;
        self.quality = quality.clone();
    }

    /// Frame Debugger event of virtual geometry's culling of view `v` (true = run it).
    fn vgeo_cull_event(&self, v: usize) -> bool {
        frame_debug::record(&self.capture, |_| frame_debug::events::vgeo_cull_event(v))
    }

    /// Frame Debugger gate of virtual geometry's batch draws (records each; true = draw it).
    fn vgeo_gate<'s>(&'s self, frame: &'s Frame<'_>) -> impl Fn(&vgeo::VgeoDraw) -> bool + 's {
        move |d| {
            frame_debug::record(&self.capture, |r| {
                frame_debug::events::vgeo_event(r, frame, d)
            })
        }
    }

    /// Counters of the last [`Renderer::render`] call.
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Record the next renders' passes and draw calls for the Frame Debugger (and stop after
    /// `request.stop_after`). GPU culling and GPU timing are off while capturing.
    pub fn begin_capture(&mut self, request: CaptureRequest) {
        self.capture = Some(std::sync::Mutex::new(frame_debug::Recorder::new(request)));
    }

    /// Stop capturing and return what was recorded since [`Renderer::begin_capture`].
    pub fn end_capture(&mut self) -> Option<FrameCapture> {
        let r = self.capture.take()?;
        Some(r.into_inner().unwrap_or_else(|e| e.into_inner()).finish())
    }

    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Record an event drawn outside [`Renderer::render`] (the Game view's canvas UI) under
    /// `path`; true = draw it (not past the replay stop).
    pub fn capture_event(&self, path: &[&str], event: FrameEvent) -> bool {
        let target = || frame_debug::target("Final Image", (0, 0), "Rgba8UnormSrgb");
        frame_debug::whole(&self.capture, path, target, EventTarget::Output, || event)
    }

    /// Time the last HDRI → cubemap conversion took (upload, GPU passes and the wait), in ms.
    pub fn hdri_build_ms(&self) -> Option<f32> {
        self.sky.last_build_ms
    }

    /// Latest GPU time of a render, when timestamp queries are supported.
    pub fn gpu_ms(&self) -> Option<f32> {
        self.gpu_timer.as_ref().and_then(|t| t.last_ms())
    }

    /// The target was made for this renderer's MSAA samples (else make a new one: Quality ▸
    /// Anti-Aliasing changed).
    pub fn target_fits(&self, target: &RenderTarget) -> bool {
        target.samples() == self.samples
    }

    /// Measure the GPU time of each part of every render (shadows, AO, main pass, grass, TAA,
    /// post...), waiting for the GPU at the end of each render: profiling only. Needs a device
    /// with `TIMESTAMP_QUERY_INSIDE_ENCODERS` (see [`timestamp_features`]); returns whether it
    /// is on.
    pub fn set_pass_timing(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        on: bool,
    ) -> bool {
        self.pass_timer = match (on, self.pass_timer.take()) {
            (true, Some(t)) if !t.is_profiler() => Some(t),
            (true, _) => pass_timer::PassTimer::new(device, queue),
            (false, _) => None,
        };
        self.pass_timer.is_some()
    }

    /// GPU milliseconds of each part of the last render (with [`Renderer::set_pass_timing`]):
    /// the time up to the end of the named part from the end of the previous one.
    pub fn pass_times(&self) -> &[(&'static str, f32)] {
        self.pass_timer.as_ref().map_or(&[], |t| t.last())
    }

    /// Name the views rendered next in the Profiler's GPU module ("Game", "Scene").
    pub fn set_view_label(&mut self, label: impl Into<std::borrow::Cow<'static, str>>) {
        self.view_label = label.into();
    }

    /// While the Profiler records, time every part of each render for its GPU module (read
    /// back a frame or two later, no waiting); collect the renders that finished. Not while
    /// the Frame Debugger captures, nor next to a blocking pass timer.
    fn sync_profiler_timing(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let want = crate::profiler::enabled() && self.capture.is_none() && self.gpu_timer.is_some();
        match self.pass_timer.as_mut() {
            Some(t) if t.is_profiler() => {
                t.poll(device);
                if !want {
                    self.pass_timer = None;
                }
            }
            Some(_) => {}
            None if want => self.pass_timer = pass_timer::PassTimer::new_async(device, queue),
            None => {}
        }
    }

    /// End of a timed part of the render (between passes).
    fn mark(&self, encoder: &mut wgpu::CommandEncoder, label: &'static str) {
        if let Some(t) = &self.pass_timer {
            t.mark(encoder, label);
        }
    }

    /// End of a timed part inside a render pass.
    fn mark_pass(&self, pass: &mut wgpu::RenderPass, label: &'static str) {
        if let Some(t) = &self.pass_timer {
            t.mark_pass(pass, label);
        }
    }

    /// GPU bytes of the textures the renderer makes itself: tree maps (bark, leaves,
    /// impostors), terrain layer arrays and grass atlases (block-compressed or RGBA8).
    pub fn made_texture_bytes(&self) -> [(&'static str, u64); 3] {
        [
            ("Trees", self.trees.texture_bytes()),
            ("Terrain layers", self.terrains.layer_bytes()),
            ("Grass and flowers", self.terrains.atlas_bytes()),
        ]
    }

    /// GPU resources held (Profiler Memory module).
    pub fn memory(&self) -> RenderMemory {
        let imported = self.imported.values().flatten();
        let meshes = self.meshes.iter().chain(imported.clone());
        let mesh_bytes: u64 = meshes.map(GpuMesh::bytes).sum::<u64>() + self.skins.bytes();
        let (terrains, terrain_bytes) = self.terrains.memory();
        // Tree assets count with the terrains; the textures the renderer makes (tree maps,
        // terrain layer arrays, grass atlases) with the textures.
        let (_, tree_bytes) = self.trees.memory();
        let made = self.made_texture_bytes();
        let made_total = made.iter().map(|m| m.1).sum::<u64>();
        let terrain_bytes = (terrain_bytes + tree_bytes).saturating_sub(made_total);
        let (textures, texture_bytes) = self.textures.memory();
        let buffers = [
            &self.draws_buffer,
            &self.slots_buffer,
            &self.line_buffer,
            &self.view_buffer,
            &self.lights_buffer,
            &self.lights_2d,
        ];
        let shadow_bytes = 4
            * (self.shadows.directional_size.pow(2) as u64 * cascades::MAX_CASCADES as u64
                + shadows::LOCAL_SIZE.pow(2) as u64 * shadows::MAX_LOCAL_LAYERS as u64);
        let vgeo_bytes = self.vgeo.memory_bytes();
        RenderMemory {
            meshes: self.meshes.len() + imported.count() + terrains,
            textures,
            virtual_geometry_bytes: vgeo_bytes,
            buffer_bytes: mesh_bytes
                + vgeo_bytes
                + terrain_bytes
                + buffers.iter().map(|b| b.size()).sum::<u64>()
                + self.particle_instances.buffer.size()
                + self.effects.bytes(),
            texture_bytes: texture_bytes
                + made_total
                + shadow_bytes
                + self.probes.bytes()
                + self.lightmaps.bytes()
                + self.sky.bytes()
                + self.volumetrics.texture_bytes()
                + self.clouds.texture_bytes()
                + self.taa.bytes()
                + self.vsm.bytes()
                + self.postfx.bytes()
                + self.decals.bytes(),
        }
    }

    /// Reload textures, models, shaders (and Shader Graphs) and effect graphs whose files changed
    /// on disk (the editor calls this in Edit mode). Returns the reloaded paths.
    pub fn reload_changed_assets(&mut self) -> Vec<String> {
        let mut changed = self.textures.poll_changes();
        if !changed.is_empty()
            && let Some(c) = self.custom.as_mut()
        {
            c.textures_changed();
        }
        changed.extend(models::poll_changes());
        changed.extend(crate::shader_asset::poll_changes());
        changed.extend(crate::vfx::library::poll_changes());
        self.decals.reload(&changed);
        changed
    }

    /// Shader compile errors since the last call (Console lines with file and line).
    pub fn take_shader_errors(&mut self) -> Vec<String> {
        self.custom
            .as_mut()
            .map(|c| c.take_errors())
            .unwrap_or_default()
    }

    /// Texture load errors since the last call.
    pub fn take_warnings(&mut self) -> Vec<String> {
        let mut warnings = self.textures.take_warnings();
        warnings.extend(self.decals.take_warnings());
        warnings
    }

    /// Upload imported meshes and build texture bind groups and shader variants used by this
    /// frame (`lod` = each draw's LOD state, which can pick the cutout variants).
    fn prepare_assets(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        lod: &[lod::LodState],
    ) {
        self.textures.sync_runtime(device, queue);
        let generation = models::generation();
        if generation != self.imported_generation {
            self.imported.clear();
            self.imported_generation = generation;
        }
        let poses = frame.meshes.iter().filter_map(|d| match &d.mesh {
            MeshRef::Asset(k) if crate::skinning::is_pose_key(k) => Some(k.as_str()),
            _ => None,
        });
        self.skins.prepare(device, queue, poses);
        for (d, state) in frame.meshes.iter().zip(lod) {
            if let MeshRef::Asset(key) = &d.mesh
                && !self.imported.contains_key(key)
                && !crate::skinning::is_pose_key(key)
            {
                let gpu = models::mesh(&d.mesh)
                    .filter(|m| !m.indices.is_empty())
                    .map(|m| {
                        let mut gpu = GpuMesh::new(device, key, &m);
                        if let Some((lo, hi)) = models::bounds(&d.mesh) {
                            gpu.bounds = culling::Aabb::new(lo, hi);
                        }
                        gpu
                    });
                self.imported.insert(key.clone(), gpu);
            }
            if frame.shaded
                && let Some(c) = &d.material.custom
            {
                let custom = self
                    .custom
                    .get_or_insert_with(|| custom_shader::CustomShaders::new(device, queue));
                custom.prepare(device, queue, &self.pipelines, &mut self.textures, c);
                // White vertex colours reaching this mesh's vertices.
                let reach = self
                    .gpu_mesh(&d.mesh)
                    .filter(|m| m.colors.is_none())
                    .map(|m| (m.base_vertex.max(0) as u64 + m.vertex_count as u64) * 4);
                if let Some(bytes) = reach
                    && self.white_colors.as_ref().is_none_or(|b| b.size() < bytes)
                {
                    let size = bytes.next_power_of_two().max(4096);
                    self.white_colors = Some(device.create_buffer_init(
                        &wgpu::util::BufferInitDescriptor {
                            label: Some("misty white vertex colours"),
                            contents: &vec![255u8; size as usize],
                            usage: wgpu::BufferUsages::VERTEX,
                        },
                    ));
                }
            } else if frame.shaded {
                let set = d.material.textures();
                let layout = &self.pipelines.material_layout;
                self.textures.prepare_set(device, queue, layout, &set);
                // Feature variants have their own group 2 (the plain set above still serves
                // their shadow, prepass and debug draws).
                if let Some(lit) = d.material.lit_set() {
                    let layout = &self.pipelines.lit_layout;
                    self.textures.prepare_lit(device, queue, layout, &lit);
                }
                let key = draw_prep::pipeline_key(d, *state);
                self.pipelines.ensure_mesh(device, key);
                if self.options.depth_prepass {
                    self.prepass.ensure(device, &self.pipelines, key);
                }
                // A LOD Group's levels cross-fade with the cutout variants: build them now
                // rather than in the frame the first fade starts (a shader compile mid-run).
                if d.lod.is_some() {
                    let fading = draw_prep::pipeline_key(d, lod::LodState::Fading(0.5));
                    self.pipelines.ensure_mesh(device, fading);
                    if self.options.depth_prepass {
                        self.prepass.ensure(device, &self.pipelines, fading);
                    }
                }
            }
            if let Some(key) = draw_prep::shadow_key(d, *state) {
                self.pipelines.ensure_shadow(device, key);
                if key.cutout {
                    let set = d.material.textures();
                    let layout = &self.pipelines.material_layout;
                    self.textures.prepare_set(device, queue, layout, &set);
                }
            }
            if d.lod.is_some()
                && let Some(key) = draw_prep::shadow_key(d, lod::LodState::Fading(0.5))
            {
                self.pipelines.ensure_shadow(device, key);
            }
        }
        if !frame.terrains.is_empty() {
            // Group 2 of the terrain pipelines is unused; any material group fits.
            let layout = &self.pipelines.material_layout;
            self.textures.prepare(device, queue, layout, None, None);
            // Quality ▸ Grass Ground Cover: the undergrowth painted on the ground, no fans.
            let cover = self.quality.grass_ground_cover;
            (self.terrains).set_ground_cover(cover.then(|| self.quality.grass_density()));
            self.terrains.prepare(
                device,
                queue,
                &self.pipelines.terrain_layout,
                &mut self.textures,
                frame.terrains,
                frame.environment,
            );
        }
        for b in frame.particles {
            self.textures.prepare(
                device,
                queue,
                &self.pipelines.material_layout,
                b.texture.as_deref(),
                None,
            );
        }
    }

    /// New offscreen target for a view.
    pub fn create_target(&self, device: &wgpu::Device, width: u32, height: u32) -> RenderTarget {
        use_samples(self.samples);
        let layouts = TargetLayouts {
            mask: &self.pipelines.mask_layout,
            post: &self.pipelines.post_layout,
            composite: &self.pipelines.composite_layout,
            sampler: &self.post_sampler,
        };
        RenderTarget::new(device, (width, height), 1.0, &layouts)
    }

    /// A Scene / Game view or player target: its final image is `width` × `height`, and the
    /// scene renders at the active quality level's Upscaling share of that (native when
    /// Upscaling is Off). Thumbnails, previews and probe captures use
    /// [`Renderer::create_target`] (always native: one-off renders have no history).
    pub fn create_view_target(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> RenderTarget {
        let layouts = TargetLayouts {
            mask: &self.pipelines.mask_layout,
            post: &self.pipelines.post_layout,
            composite: &self.pipelines.composite_layout,
            sampler: &self.post_sampler,
        };
        let scale = self.quality.upscale_ratio();
        RenderTarget::new(device, (width, height), scale, &layouts)
    }

    /// `target` (made by [`Renderer::create_view_target`]) still matches a `width` ×
    /// `height` view at the active quality level's Upscaling share and this renderer's MSAA
    /// samples ([`Renderer::target_fits`]); false = make a new one.
    pub fn view_target_fits(&self, target: &RenderTarget, width: u32, height: u32) -> bool {
        let wanted =
            target::render_size((width.max(1), height.max(1)), self.quality.upscale_ratio());
        target.output_size() == (width.max(1), height.max(1))
            && target.size() == wanted
            && self.target_fits(target)
    }

    fn gpu_mesh(&self, r: &MeshRef) -> Option<&GpuMesh> {
        match r {
            MeshRef::Primitive(p) => {
                Some(&self.meshes[PrimitiveMesh::ALL.iter().position(|m| m == p).unwrap_or(0)])
            }
            MeshRef::Asset(key) if crate::skinning::is_pose_key(key) => self.skins.get(key),
            MeshRef::Asset(key) => self.imported.get(key)?.as_ref(),
        }
    }

    /// Draw a frame into `target` and submit it. Rectangle lights with Show Emitter on add
    /// their emissive quads to the frame's meshes.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
    ) {
        use_samples(self.samples);
        if !self.target_fits(target) {
            // (Made before Quality ▸ Anti-Aliasing changed: the host makes a new one.)
            return;
        }
        self.begin_wireframe();
        // Quality ▸ Global Illumination caps Misty GI whatever the scene or volume asks for.
        let mut post = frame.post;
        post.misty_gi = post.misty_gi.capped(self.quality.global_illumination);
        let frame = &if self.wire_only {
            // The Wireframe mode: the geometry renders for the depth that hides edges behind
            // it, unlit and without what draws no depth; its colour is painted over with the
            // flat background (no post-processing on it).
            Frame {
                shaded: true,
                skybox: false,
                post: PostSettings::default(),
                lights: &[],
                water: &[],
                particles: &[],
                ..*frame
            }
        } else {
            Frame { post, ..*frame }
        };
        // Frame Debugger ▸ Emission: every material unlit, showing only its emission.
        let emissive: Vec<MeshDraw>;
        let frame = &match frame_debug::view(&self.capture) {
            frame_debug::TargetView::Emission => {
                emissive = frame
                    .meshes
                    .iter()
                    .map(frame_debug::emission_only)
                    .collect();
                Frame {
                    meshes: &emissive,
                    skybox: false,
                    background: [0.0; 3],
                    ..*frame
                }
            }
            _ => Frame { ..*frame },
        };
        let emitters = if frame.shaded {
            area::emitter_draws(frame.lights)
        } else {
            Vec::new()
        };
        if emitters.is_empty() {
            return self.render_frame(device, queue, target, frame);
        }
        let meshes: Vec<MeshDraw> = frame.meshes.iter().cloned().chain(emitters).collect();
        let with_emitters = Frame {
            meshes: &meshes,
            ..*frame
        };
        self.render_frame(device, queue, target, &with_emitters);
    }

    /// [`Renderer::render`] with Visual Effects: simulates the visible ones that have a new
    /// step (once per step, whichever view renders first) and draws them after transparent
    /// geometry.
    pub fn render_with_effects(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
        effects: &[crate::vfx::EffectDraw],
    ) {
        if frame.shaded && !effects.is_empty() {
            let view_proj = frame.camera.view_projection(target.aspect());
            let cull = self.options.culling;
            self.effects.prepare(
                device,
                queue,
                &self.pipelines,
                &mut self.textures,
                effects,
                &view_proj,
                frame.camera.position,
                cull,
            );
        }
        self.render(device, queue, target, frame);
        self.effects.end_frame();
    }

    fn render_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
    ) {
        crate::profile_scope!("Render", crate::profiler::Category::Rendering);
        let started = std::time::Instant::now();
        let mut stats = RenderStats::default();
        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.poll(device);
        }
        self.sync_profiler_timing(device, queue);
        // Virtual geometry: meshes built into clusters draw through the cluster path (it needs
        // indirect draws with an instance offset, like GPU culling). While capturing, the
        // Frame Debugger lists its culling and batch draws (vgeo_gate). Prepared before the
        // LOD states: a LOD Group whose LOD 0 draws as clusters keeps LOD 0.
        self.vgeo.on = self.vgeo.enabled && self.culler.is_some() && frame.shaded;
        self.vgeo.view_debug = self.vgeo.debug_once.take().unwrap_or(self.vgeo.debug);
        if self.vgeo.on {
            crate::profile_scope!("Render.VirtualGeometry");
            self.vgeo.ensure_gpu(device, &self.pipelines);
            self.vgeo.prepare_meshes(device, frame);
        }
        let lod = draw_prep::lod_states(frame, self.options.lod_bias, |m| {
            self.vgeo.mesh_index(m).is_some()
        });
        {
            crate::profile_scope!("Render.PrepareAssets");
            self.prepare_assets(device, queue, frame, &lod);
        }
        let aspect = target.aspect();
        // Temporal anti-aliasing jitters the camera by a sub-pixel offset each frame (of an
        // internal pixel when the target is upscaled: the upscaler is TAA at a lower
        // resolution, so it runs whatever Quality ▸ Temporal Anti-aliasing says).
        let upscaled = target.upscaled();
        let taa_on = (self.quality.temporal_aa || upscaled) && frame.shaded && !self.wire_only;
        let jitter = if taa_on {
            self.taa.jitter(target.id, target.render_scale())
        } else {
            glam::Vec2::ZERO
        };
        // Textures sample the mip level the output resolution would have picked.
        let mip_bias = mip_bias(target);
        let view_proj = frame.camera.view_projection(aspect);
        // Motion vectors: last frame's camera, draws and grass clock.
        let grass_time = self.terrains.grass_time(frame.environment);
        let motion = self.motion.begin(target.id, frame, view_proj, grass_time);
        let mut view = view_uniform(
            frame,
            aspect,
            jitter,
            target.size(),
            motion.prev_view_proj,
            mip_bias,
        );
        // view_tan.w (unused elsewhere): the wireframe edges' opacity ([`wireframe`]).
        view.view_ray[1][3] = self.wire.map_or(0.0, |w| w.color[3]);
        queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&view));
        #[cfg(test)]
        {
            self.last_view_proj = Mat4::from_cols_array_2d(
                &view_uniform(
                    frame,
                    aspect,
                    jitter,
                    target.size(),
                    motion.prev_view_proj,
                    mip_bias,
                )
                .view_proj,
            );
        }
        // HDRI sky: cubes built when the file changes, the uniform every render.
        if self.sky.prepare(device, queue, frame.environment) {
            self.rebuild_view_group(device);
        }
        let scene_copy = self.needs_scene_copy(frame);
        self.prepare_scene_inputs(device, queue, target, frame, scene_copy);
        // (Before culling: decals in view turn the screen-space prepass on.)
        self.prepare_decals(device, queue, target, frame);
        if scene_copy && let Some(c) = self.scene_copies.current(target.size()) {
            self.effects.scene_depth(
                device,
                c.id,
                &c.depth_view,
                view_proj,
                frame.camera.position,
                target.size(),
            );
        }

        // Culling and batching for the camera and every shadow map.
        // The active Quality level's shadows, fitted to the map size actually allocated.
        let shadow_settings = crate::quality::ShadowSettings {
            map_size: self.shadows.directional_size,
            ..self.quality.shadow_settings()
        };
        let (mut lists, plan, probes_rebuilt, near_sun) = {
            crate::profile_scope!("Render.Culling");
            let prep = self.draw_prep(frame, lod, &motion.prev_draws);
            let terrains = {
                crate::profile_scope!("Render.TerrainBounds");
                self.terrain_bounds(frame)
            };
            // Reflection probes: atlas uploads and each draw's probe list.
            let rebuilt = {
                crate::profile_scope!("Render.Probes");
                self.probes
                    .prepare(device, queue, frame, &prep.bounds, &terrains)
            };
            // Baked lightmaps (texture array upload).
            let rebuilt =
                (self.lightmaps).prepare(device, queue, &frame.environment.lightmaps) || rebuilt;
            let casters = Self::caster_bounds(frame, &prep.bounds, &terrains);
            let mut plan = {
                crate::profile_scope!("Render.ShadowPlan");
                shadows::plan_with(frame, &casters, aspect, &shadow_settings)
            };
            let grass_layout = &self.pipelines.grass_layout;
            let camera = frame.camera.position;
            // The sun that has the cascades gets the near grass shadow map too.
            let near_sun = (self.shadows.grass_near.size > 1 && plan.has_sun())
                .then(|| {
                    frame
                        .lights
                        .iter()
                        .find(|l| {
                            l.kind == crate::scene::LightType::Directional
                                && l.shadows != crate::scene::Shadows::None
                                && l.intensity > 0.0
                        })
                        .map(|l| l.direction)
                })
                .flatten();
            let first = frame.meshes.len();
            // (Before the frame lists: the virtual shadow map's pages follow the terrain chunks'
            // detail.)
            (self.terrains).set_shadow_reach(shadow_settings.distance.min(frame.camera.far));
            (self.terrains).select(
                device,
                queue,
                camera,
                &view_proj,
                frame.terrains,
                first,
                grass_layout,
                frame.environment,
                self.quality.grass_shadow_casters(),
                // The internal pixel: blades thinner than ~1 px are widened to it. (Sizing them
                // by the output pixel under upscaling measured hazier, not sharper.)
                grass_screen(frame, target.size().1, taa_on, motion.grass_dt),
                near_sun.is_some(),
                grass_time,
                self.quality.grass_density(),
            );
            {
                crate::profile_scope!("Render.VirtualShadowMap");
                self.prepare_vsm(device, queue, frame, target, &mut plan, &prep, &casters);
            }
            let lists = {
                crate::profile_scope!("Render.FrameLists");
                self.frame_lists(frame, &view_proj, &plan, prep, &terrains, &mut stats)
            };
            let vsm_views: Vec<(Mat4, u8)> = (self.vsm.frame.iter())
                .flat_map(|f| f.rects.iter().map(|r| (r.view_proj, r.tree_lod)))
                .collect();
            // Last frame's depth, while the view has hardly changed.
            let tree_hiz = target.tree_hiz.lock().unwrap_or_else(|e| e.into_inner());
            let occlusion = (tree_hiz.as_ref())
                .filter(|_| fast_paths::tree_occlusion())
                .filter(|h| h.usable(target.size(), camera, frame.camera.forward()))
                .map(|h| trees_cull::TreeOcclusion {
                    pyramid: h.state.pyramid_view(),
                    view_proj: h.view_proj,
                    mips: h.state.mips(),
                    size: target.size(),
                });
            let tree_frame = trees::TreeFrame {
                occlusion: occlusion.as_ref(),
                terrains: frame.terrains,
                camera,
                view_proj: &view_proj,
                cascades: &plan.cascades,
                environment: frame.environment,
                time: grass_time,
                dt: if taa_on { motion.grass_dt } else { 0.0 },
                taa: taa_on,
                lod_bias: self.quality.lod_bias,
                // Towards the sun that has the cascades (impostor shadows face it).
                sun: frame
                    .lights
                    .iter()
                    .find(|l| {
                        l.kind == crate::scene::LightType::Directional
                            && l.shadows != crate::scene::Shadows::None
                            && l.intensity > 0.0
                    })
                    .map(|l| -l.direction),
                vsm_views: &vsm_views,
                near: (self.vsm.frame.as_ref())
                    .and_then(|f| f.near)
                    .map(|n| (n.view_proj, n.radius)),
                // The radius the near layer has when on (its grass reach or the tree radius).
                near_reach: match &self.vsm.frame {
                    Some(_) => self
                        .terrains
                        .grass_caster_range()
                        .map_or(vsm::NEAR_RADIUS, |r| r.max(vsm::NEAR_RADIUS)),
                    None => 0.0,
                },
            };
            (self.trees).prepare(device, queue, &self.pipelines, &tree_frame);
            drop(tree_hiz);
            (lists, plan, rebuilt, near_sun)
        };
        // The near-camera grass shadow map follows the camera (off: its uniform says so).
        let near_sun = near_sun.filter(|_| self.terrains.has_grass_near_shadow());
        let grass_near = (self.shadows.grass_near).prepare(queue, near_sun, frame.camera.position);
        #[cfg(test)]
        {
            self.last_shadow_views = (plan.cascades.clone(), grass_near);
        }
        // Virtual shadow maps leave grass out of their cached pages: the grass casters past the
        // near map go into the virtual map's near layer instead.
        let grass_band = (self.vsm.frame.as_ref())
            .and_then(|f| f.near)
            .filter(|n| n.grass)
            .map(|n| n.view_proj);
        // GPU culling: opaque batches are built from every candidate and culled on the GPU.
        let culled = self.gpu_culling_active(frame).then(|| {
            let culled = self.gpu_batches(frame, &lists, lists.slots.len() as u32);
            lists.opaque.clear();
            culled
        });
        let extra_slots = culled.as_ref().map_or(0, |c| c.slots_needed());
        self.upload_draws(device, queue, frame, &lists, extra_slots, probes_rebuilt);
        if let Some(c) = &culled {
            self.upload_culling(device, queue, target, frame, &lists, c, &view_proj);
            self.last_gpu_batches = c.batches.len();
        }
        let gpu_culled = culled.is_some();
        self.begin_vgeo(
            device, queue, target, frame, &lists, &view_proj, &plan, gpu_culled,
        );
        self.prepare_wireframe(device);
        self.prepare_shadow_mask(device, queue, target, frame, &plan, &shadow_settings);
        let screen = self.prepare_screen_space(device, queue, target, frame);
        if screen.is_none() {
            self.cancel_shadow_mask();
        }
        // This frame's grass passes, culled on the GPU before the first of them: the sun
        // cascades that start within the grass casters' range, the near grass shadow map, the
        // AO prepass and the camera's grass.
        if self.terrains.has_grass() {
            let grass_range = self.terrains.grass_caster_range();
            let mut cascades: Vec<(usize, glam::Mat4)> = (plan.cascades.iter().enumerate())
                .filter(|(slot, _)| {
                    let start = slot.checked_sub(1).map_or(0.0, |p| plan.cascades[p].far);
                    grass_range.is_some_and(|r| start < r)
                })
                .map(|(slot, c)| (slot, c.view_proj))
                .collect();
            // (No cascades with the virtual shadow map: its near layer takes the first lane.)
            if let Some(m) = grass_band {
                cascades.push((0, m));
            }
            let prepass = screen
                .as_ref()
                .and_then(|_| screen_space::grass_prepass_range(self.quality.ambient_occlusion));
            // The grass visibility raster draws the standing blades from the depth lane
            // (`grass_vis_passes`), so the lit lane needs only the flat fans.
            let fans_only = fast_paths::grass_vis_depth()
                && fast_paths::sky_late()
                && self.capture.is_none()
                && grass_vis::grass_vis_enabled()
                && frame.shaded;
            self.terrains.plan_grass(
                device,
                queue,
                &self.pipelines.grass_blades_layout,
                frame.terrains,
                &cascades,
                grass_near.as_ref(),
                prepass,
                fans_only,
            );
        }

        // Lights and their shadow maps (cascades first, then local layers).
        let soft = shadow_settings.soft != crate::quality::Tier::Off;
        let lights = lights_uniform(frame, &plan, soft);
        queue.write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights));
        light2d::upload(queue, &self.lights_2d, &frame.environment.lights_2d);
        self.write_shadow_uniforms(queue, frame, &plan, &shadow_settings, target.size());
        self.prepare_volumetrics(device, queue, target, frame, &plan);
        self.prepare_glows(device, queue, target, frame);
        self.prepare_clouds(device, queue, target, frame);

        // Overlay triangles follow the lines in the same buffer, which never grows past the
        // device's largest buffer: overlays are kept whole, lines are cut to what fits.
        let max_vertices = (device.limits().max_buffer_size as usize
            / std::mem::size_of::<LineVertex>())
        .max(1024);
        let overlay = &frame.overlay[..frame.overlay.len().min(max_vertices)];
        let lines = &frame.lines[..frame.lines.len().min(max_vertices - overlay.len())];
        self.lines_drawn = lines.len() as u32;
        self.overlay_drawn = overlay.len() as u32;
        let line_vertices = lines.len() + overlay.len();
        if line_vertices > self.line_capacity {
            self.line_capacity = line_vertices.next_power_of_two().min(max_vertices);
            self.line_buffer = line_buffer(device, self.line_capacity);
        }
        if !lines.is_empty() {
            queue.write_buffer(&self.line_buffer, 0, bytemuck::cast_slice(lines));
        }
        if !overlay.is_empty() {
            let offset = std::mem::size_of_val(lines) as u64;
            queue.write_buffer(&self.line_buffer, offset, bytemuck::cast_slice(overlay));
        }

        let particle_draws = self.upload_particles(device, queue, frame, &lists.particles);
        (self.postfx).prepare(
            device,
            queue,
            &self.pipelines.view_layout,
            target,
            frame,
            motion.prev_view_proj,
            motion.continuous,
        );
        // No GPU timing while capturing: a replay may skip the pass that ends the timing.
        let timing = match self.capture {
            Some(_) => None,
            None => self.gpu_timer.as_mut().and_then(|t| t.begin()),
        };
        frame_debug::set_size(&self.capture, target.size());

        crate::profile_scope!("Render.Encode");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty frame"),
        });
        let any_outline = frame.meshes.iter().any(|m| m.outline);
        if let Some(t) = &self.pass_timer {
            t.begin(&mut encoder);
        }
        // First pass: starts the GPU timing.
        self.mask_pass(&mut encoder, target, frame, timing);
        self.mark(&mut encoder, "Selection mask");
        self.terrains
            .encode_grass_cull(frame.terrains, &mut encoder);
        self.mark(&mut encoder, "Grass culling");
        self.trees.encode_cull(device, &mut encoder);
        self.mark(&mut encoder, "Tree culling");
        if frame.shaded
            && !frame.water.is_empty()
            && frame_debug::whole(
                &self.capture,
                &["Water", "Simulation"],
                TargetInfo::default,
                EventTarget::None,
                || FrameEvent::new(EventKind::Compute, "Dispatch Water FFT"),
            )
        {
            self.water.simulate(&mut encoder);
        }
        self.mark(&mut encoder, "Water simulation");
        self.vgeo
            .cull(&mut encoder, 2, 2 + self.vgeo.shadow_views(), &|v| {
                self.vgeo_cull_event(v)
            });
        self.mark(&mut encoder, "Virtual geometry: shadow culling");
        {
            crate::profile_scope!("Render.Encode.Shadows");
            self.shadow_passes(&mut encoder, frame, &plan, &lists, &mut stats);
        }
        self.mark(&mut encoder, "Shadow maps (cascades, local)");
        {
            crate::profile_scope!("Render.Encode.VirtualShadowMap");
            self.vsm_passes(&mut encoder, frame, &plan, &lists, &mut stats);
            // The fog and sun shafts read cascade layers: fill them from the virtual map.
            if let (Some(fog), Some(gpu), Some(dir), true) = (
                &self.vsm.fog,
                &self.vsm.gpu,
                // The light the shown pages were drawn for (it may trail the sun a little).
                self.vsm.frame.as_ref().map(|f| f.light_dir),
                self.volumetrics.reads_sun_shadow(),
            ) {
                fog.encode(
                    device,
                    queue,
                    &mut encoder,
                    gpu,
                    &plan.fog_cascades,
                    dir,
                    frame.camera.position,
                    &self.shadows.cascade_views,
                );
            }
        }
        self.mark(&mut encoder, "Virtual shadow map pages");
        if let Some(view_proj) = grass_near {
            self.grass_near_shadow_pass(&mut encoder, frame, &view_proj, &mut stats);
        }
        self.mark(&mut encoder, "Grass near shadow map");
        self.cloud_passes(device, &mut encoder);
        self.mark(&mut encoder, "Clouds");
        self.froxel_passes(device, &mut encoder, target);
        self.mark(&mut encoder, "Froxel fog");
        // The first phase's clusters (also drawn by the screen-space prepass below).
        self.vgeo
            .cull(&mut encoder, 0, 1, &|v| self.vgeo_cull_event(v));
        self.mark(&mut encoder, "Virtual geometry: culling (first phase)");
        // The AO / contact shadow prepass (own normal + depth targets) is separate from the
        // optional main depth prepass below; GPU culling is off while it runs (see
        // `gpu_culling_active`), so it always sees the CPU-culled opaque batches.
        if let Some(ss) = &screen
            && frame_debug::whole(
                &self.capture,
                &["Ambient Occlusion"],
                || {
                    frame_debug::target(
                        "Normals + Depth",
                        target.size(),
                        "Rgba8Unorm / Depth32Float",
                    )
                },
                EventTarget::None,
                || FrameEvent {
                    shader: "ao_prepass.wgsl, ao.wgsl (GTAO, denoise, resolve)".to_owned(),
                    ..FrameEvent::new(EventKind::Compute, "Depth Normal Prepass + GTAO")
                },
            )
        {
            self.screen_space_passes(&mut encoder, frame, &lists, ss, &mut stats);
            self.mark(&mut encoder, "GTAO");
            self.decal_pass(device, queue, &mut encoder, ss);
            self.mark(&mut encoder, "Decals");
            // The virtual shadow map marks its pages from this single-sampled depth (cheaper
            // than the main pass's multisampled one, which then needn't be kept for it).
            if self.vsm.wants_marks() {
                let (depth, _, _) = ss.views();
                let size = target.size();
                (self.vsm).encode_mark(
                    device,
                    &mut encoder,
                    &self.view_bind_group,
                    depth,
                    size,
                    false,
                );
                self.mark(&mut encoder, "Virtual shadow map marking");
            }
            let (depth, normals, velocity) = ss.views();
            self.shadow_mask_passes(device, &mut encoder, depth, normals, velocity);
            self.mark(&mut encoder, "Shadow mask / denoise");
        }
        let trace = screen.as_ref().and_then(|s| s.trace.as_ref());
        if let Some(t) = trace {
            self.misty_gi_passes(&mut encoder, t);
            self.screen_trace_passes(&mut encoder, target, t);
        }
        self.mark(&mut encoder, "SSGI / SSR");
        // Without the screen-space prepass the marking reads the main pass's multisampled depth,
        // which then must be stored and decompressed: every other frame is enough (marked pages
        // stay wanted for a second, the guard band covers the frame between).
        if self.vsm.wants_marks() && self.vsm_frame % 2 == 1 {
            self.vsm.skip_marks();
        }
        let prepass = self.prepass_active(frame);
        if prepass {
            self.depth_prepass(&mut encoder, target.depth_view(), frame, &lists, &mut stats);
        }
        self.mark(&mut encoder, "Depth prepass");
        let split = MainSplit {
            phase: None,
            culled: None,
            prepass,
            scene_copy,
            trace: trace.filter(|t| t.needs_history),
            vis: self.grass_vis_active(device, queue, target, frame),
        };
        // GPU culling and virtual geometry draw the main pass in two phases around the Hi-Z
        // pyramid of the first phase's depth (virtual geometry only when it has enough
        // instances for occlusion culling to pay: otherwise its one phase draws with the
        // opaque meshes of an unsplit main pass).
        let vgeo = self.vgeo.active() && !self.vgeo.single_phase();
        match (&culled, self.culler.as_ref()) {
            (_, Some(culler)) if culled.is_some() || vgeo => {
                let state = target.culling.lock().unwrap_or_else(|e| e.into_inner());
                let state = state.as_ref().expect("made by upload_culling / begin_vgeo");
                let slots = &self.slots_buffer;
                if let Some(c) = &culled {
                    culler.dispatch(device, &mut encoder, state, c, slots, 0);
                }
                let first = MainSplit {
                    phase: Some(0),
                    culled: culled.as_ref(),
                    ..split
                };
                let parts = (&lists, &particle_draws[..]);
                self.main_pass(
                    device,
                    &mut encoder,
                    target,
                    frame,
                    any_outline,
                    parts,
                    &mut stats,
                    first,
                );
                culler.build_pyramid(device, &mut encoder, state, target.depth_view());
                if let Some(c) = &culled {
                    culler.dispatch(device, &mut encoder, state, c, slots, 1);
                }
                self.vgeo
                    .cull(&mut encoder, 1, 2, &|v| self.vgeo_cull_event(v));
                self.mark(&mut encoder, "Hi-Z + culling (second phase)");
                let rest = MainSplit {
                    phase: Some(1),
                    ..first
                };
                self.main_pass(
                    device,
                    &mut encoder,
                    target,
                    frame,
                    any_outline,
                    parts,
                    &mut stats,
                    rest,
                );
            }
            _ => {
                let parts = (&lists, &particle_draws[..]);
                self.main_pass(
                    device,
                    &mut encoder,
                    target,
                    frame,
                    any_outline,
                    parts,
                    &mut stats,
                    split,
                );
                // GPU-culled trees: this frame's depth as a Hi-Z pyramid, for next frame's
                // occlusion test.
                if let Some(culler) = self.culler.as_ref().filter(|_| {
                    self.trees.gpu_culled() && target.samples() > 1 && fast_paths::tree_occlusion()
                }) {
                    let mut h = target.tree_hiz.lock().unwrap_or_else(|e| e.into_inner());
                    let size = target.size();
                    let fresh = || trees_cull::TreeHiz {
                        state: gpu_culling::TargetState::new(device, size),
                        view_proj,
                        camera: frame.camera.position,
                        forward: frame.camera.forward(),
                    };
                    if !h.as_ref().is_some_and(|h| h.state.fits(size)) {
                        *h = Some(fresh());
                    }
                    let h = h.as_mut().expect("made above");
                    culler.build_pyramid(device, &mut encoder, &h.state, target.depth_view());
                    h.view_proj = view_proj;
                    h.camera = frame.camera.position;
                    h.forward = frame.camera.forward();
                    self.mark(&mut encoder, "Tree Hi-Z");
                }
            }
        }
        self.mark(&mut encoder, "Main pass (rest)");
        if self.vsm.wants_marks() {
            let depth = target.depth_view();
            (self.vsm).encode_mark(
                device,
                &mut encoder,
                &self.view_bind_group,
                depth,
                target.size(),
                sample_count() > 1,
            );
        }
        self.mark(&mut encoder, "Virtual shadow map marking");
        self.glow_passes(device, &mut encoder, target);
        self.mark(&mut encoder, "Glows");
        self.sun_shaft_passes(device, &mut encoder, target);
        self.mark(&mut encoder, "Sun shafts");
        let resolved = taa_on
            && frame_debug::record(&self.capture, |_| {
                let name = match upscaled {
                    true => "Temporal Upscaling",
                    false => "Temporal Anti-aliasing",
                };
                frame_debug::events::post_event(name.to_owned(), "taa.wgsl")
            });
        if resolved {
            self.taa.resolve(
                device,
                &mut encoder,
                &taa::Resolve {
                    view_group: &self.view_bind_group,
                    target: target.id,
                    current: (target.hdr_texture(), target.hdr_view()),
                    output: (target.post_hdr_view(), target.output_size()),
                    jitter,
                    depth: target.depth_view(),
                    velocity: target.velocity_view(),
                    view_proj,
                    prev_view_proj: motion.prev_view_proj,
                    continuous: motion.continuous,
                    camera: frame.camera,
                    settings: taa::TaaSettings {
                        blend: self.quality.taa_blend,
                        sharpness: self.quality.taa_sharpness,
                    },
                },
            );
        } else if upscaled {
            // No temporal resolve this frame (wireframe, frame debugger replay): stretch.
            let (input, output) = (target.hdr_view(), target.post_hdr_view());
            (self.taa).stretch(device, &mut encoder, &self.view_bind_group, input, output);
        }
        self.mark(&mut encoder, "TAA");
        let composite = self.postfx_passes(device, &mut encoder, target);
        let composite = composite.as_ref().unwrap_or(&target.composite_group);
        self.bloom_passes(&mut encoder, target, composite, timing);
        self.mark(&mut encoder, "Bloom, tonemap, finish");
        if let Some(t) = &self.pass_timer {
            t.resolve(&mut encoder);
        }
        match frame_debug::view(&self.capture) {
            v if v.needs_prepass() || v == frame_debug::TargetView::Gi => {
                self.frame_debug_view(device, &mut encoder, target, v, screen.as_ref());
            }
            _ => {
                if let Some(display) = frame_debug::stop_display(&self.capture) {
                    self.frame_debug_blit(device, &mut encoder, target, display);
                }
            }
        }
        if let (Some(timer), Some(slot)) = (self.gpu_timer.as_ref(), timing) {
            timer.resolve(&mut encoder, slot);
        }
        self.vgeo.copy_stats(device, &mut encoder);
        {
            crate::profile_scope!("Render.Submit");
            let commands = {
                crate::profile_scope!("Render.Submit.Finish");
                encoder.finish()
            };
            crate::profile_scope!("Render.Submit.Queue");
            queue.submit([commands]);
        }
        self.vgeo.stats_submitted();
        self.trees.cull_submitted();
        let vg = self.vgeo.collect_stats();
        stats.vgeo_instances = vg.instances;
        stats.vgeo_clusters = vg.clusters;
        stats.vgeo_triangles = vg.triangles;
        stats.vgeo_shadow_triangles = vg.shadow_triangles;
        stats.vgeo_memory = vg.pool_bytes + vg.list_bytes;
        if let (Some(timer), Some(slot)) = (self.gpu_timer.as_ref(), timing) {
            timer.submitted(slot);
        }
        self.vsm.submitted();
        self.vsm_frame += 1;
        if let Some(t) = self.pass_timer.as_mut() {
            if t.is_profiler() {
                let view = match self.view_label.is_empty() {
                    true => format!("{}×{}", target.size().0, target.size().1).into(),
                    false => self.view_label.clone(),
                };
                t.submitted(crate::profiler::frame_index(), view);
            } else {
                t.read(device);
            }
        }
        self.slots_scratch = lists.slots;
        let grass = self.terrains.grass_stats();
        stats.grass_pending = grass.pending;
        stats.loading = (self.terrains.layers_pending()
            + self.trees.loading()
            + misty_gi::mesh_sdf_bakes_pending()) as u32;
        stats.grass_upload_ms = grass.upload_ms;
        stats.grass_upload_last_start_ms = grass.upload_last_start_ms;
        stats.grass_upload_piece_ms = grass.upload_piece_ms;
        stats.grass_upload_alloc_ms = grass.upload_alloc_ms;
        self.motion
            .end(target.id, std::mem::take(&mut lists.prep.motion));
        stats.cpu_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.stats = stats;
    }

    /// Shader globals, the scene copy and water inputs for this render.
    fn prepare_scene_inputs(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
        split: bool,
    ) {
        let copy = split.then(|| self.scene_copies.get(device, target.size()));
        if let Some(c) = self.custom.as_mut() {
            c.write_globals(queue, target.size());
            if let Some(copy) = copy {
                c.scene_group(device, copy.id, &copy.color_view, &copy.depth_view);
            }
        }
        if frame.shaded && !frame.water.is_empty() {
            self.water
                .prepare(device, queue, &self.pipelines, frame, target.size(), copy);
        } else {
            // A render without water draws none: the last plan (a deleted surface, the
            // underwater pass of a camera that was under it) must not carry over.
            self.water.forget();
        }
    }

    /// Write the per-draw values and the slot lists, growing the buffers when needed
    /// (`extra_slots` = room after the lists for GPU culling's output).
    fn upload_draws(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        lists: &frame_lists::FrameLists,
        extra_slots: usize,
        rebuild: bool,
    ) {
        let slots = &lists.slots[..];
        let draw_count = frame.meshes.len() + frame.terrains.len();
        let grow_draws = draw_count > self.draws_capacity;
        let slots_needed = slots.len() + extra_slots;
        let grow_slots = slots_needed > self.slots_capacity;
        if grow_draws {
            self.draws_capacity = draw_count.next_power_of_two();
            self.draws_buffer = draws_buffer(device, self.draws_capacity);
        }
        if grow_slots {
            self.slots_capacity = slots_needed.next_power_of_two();
            self.slots_buffer = slots_buffer(device, self.slots_capacity);
        }
        if grow_draws || grow_slots || rebuild {
            self.rebuild_draw_groups(device);
        }
        let meshes = &lists.prep.uniforms;
        if !meshes.is_empty() {
            queue.write_buffer(&self.draws_buffer, 0, bytemuck::cast_slice(meshes));
        }
        let terrains: Vec<DrawUniform> = frame.terrains.iter().map(terrain::draw_uniform).collect();
        if !terrains.is_empty() {
            let offset = std::mem::size_of_val(&meshes[..]) as u64;
            queue.write_buffer(&self.draws_buffer, offset, bytemuck::cast_slice(&terrains));
        }
        if !slots.is_empty() {
            queue.write_buffer(&self.slots_buffer, 0, bytemuck::cast_slice(slots));
        }
    }

    /// Sort the visible particles back to front and upload them as instances.
    fn upload_particles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        visible: &[bool],
    ) -> Vec<particles::ParticleDraw> {
        let depth = camera_depth(frame.camera);
        let (instances, draws) = particles::order_filtered(frame.particles, |i| visible[i], depth);
        self.particle_instances.upload(device, queue, &instances);
        draws
    }

    fn draw_particles(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        draws: &[particles::ParticleDraw],
        stats: &mut RenderStats,
    ) {
        pass.set_vertex_buffer(0, self.particle_instances.buffer.slice(..));
        for d in draws {
            let b = &frame.particles[d.batch];
            let Some(group) = self.textures.get(b.texture.as_deref(), None) else {
                continue;
            };
            let n = d.instances.len() as u32;
            if !frame_debug::record(&self.capture, |_| {
                frame_debug::events::particle_event(d.batch, n, b.additive, b.texture.as_deref())
            }) {
                continue;
            }
            pass.set_pipeline(if b.additive {
                &self.pipelines.particles_additive
            } else {
                &self.pipelines.particles_alpha
            });
            pass.set_bind_group(1, group, &[]);
            pass.draw(0..6, d.instances.clone());
            // A system is one draw of camera-facing quads.
            let n = d.instances.len() as u64;
            stats.batches += 1;
            stats.set_pass += 1;
            stats.triangles += 2 * n;
            stats.vertices += 4 * n;
        }
    }

    /// Fullscreen pass into `view` (clear or load), sampling `group`.
    fn fullscreen(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
        clear: bool,
        timestamps: Option<wgpu::RenderPassTimestampWrites>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty post pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Bright-pass → downsample chain → additive upsample chain → composite into the output
    /// image (the last pass: ends the GPU timing).
    fn bloom_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        composite_group: &wgpu::BindGroup,
        timing: Option<usize>,
    ) {
        let levels = &target.bloom;
        let p = &self.pipelines;
        // Post-processing runs at the output resolution.
        let (w, h) = target.output_size();
        let level_size = |i: usize| ((w >> (i + 1)).max(1), (h >> (i + 1)).max(1));
        let step = |name: String, shader: &str, i: usize| {
            let target =
                || frame_debug::target(&format!("Bloom Mip {i}"), level_size(i), "Rgba16Float");
            let display = EventTarget::Bloom(i as u32);
            frame_debug::whole(&self.capture, &["Post", "Bloom"], target, display, || {
                frame_debug::events::post_event(name, shader)
            })
        };
        if step("Bloom Prefilter".to_owned(), "bloom.wgsl fs_prefilter", 0) {
            self.fullscreen(
                encoder,
                &levels[0].view,
                &p.bloom_prefilter,
                &target.hdr_group,
                true,
                None,
            );
        }
        for i in 1..levels.len() {
            if step(format!("Bloom Downsample {i}"), "bloom.wgsl fs_down", i) {
                self.fullscreen(
                    encoder,
                    &levels[i].view,
                    &p.bloom_down,
                    &levels[i - 1].group,
                    true,
                    None,
                );
            }
        }
        for i in (1..levels.len()).rev() {
            if step(
                format!("Bloom Upsample {i}"),
                "bloom.wgsl fs_up (additive)",
                i - 1,
            ) {
                self.fullscreen(
                    encoder,
                    &levels[i - 1].view,
                    &p.bloom_up,
                    &levels[i].group,
                    false,
                    None,
                );
            }
        }
        // FXAA / sharpening read the composite from a second image and write the output.
        let finish = p.finish.pick(&self.quality);
        let composite_to = match finish {
            Some(_) => target.pre_finish_view(),
            None => target.output_view(),
        };
        let composite_display = match finish {
            Some(_) => EventTarget::PreFinish,
            None => EventTarget::Output,
        };
        let final_image = || frame_debug::target("Final Image", (w, h), "Rgba8UnormSrgb");
        let composite = frame_debug::whole(
            &self.capture,
            &["Post", "Tonemap"],
            final_image,
            composite_display,
            || {
                frame_debug::events::post_event(
                    "Composite (bloom, grading, tonemap, vignette)".to_owned(),
                    "composite.wgsl fs_main",
                )
            },
        );
        let finish_allowed = finish.is_some()
            && frame_debug::whole(
                &self.capture,
                &["Post", "Anti-aliasing / Sharpen"],
                final_image,
                EventTarget::Output,
                || frame_debug::events::post_event("FXAA / Sharpen".to_owned(), "finish.wgsl"),
            );
        if !composite {
            return;
        }
        self.fullscreen(
            encoder,
            composite_to,
            &p.composite,
            composite_group,
            true,
            if finish.is_none() {
                self.timestamps(timing, false, true)
            } else {
                None
            },
        );
        if let Some(pipeline) = finish.filter(|_| finish_allowed) {
            self.fullscreen(
                encoder,
                target.output_view(),
                pipeline,
                &target.pre_finish_group,
                true,
                self.timestamps(timing, false, true),
            );
        }
    }

    /// Timestamp writes of the timed render's first or last pass.
    fn timestamps(
        &self,
        timing: Option<usize>,
        start: bool,
        end: bool,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let (timer, slot) = (self.gpu_timer.as_ref()?, timing?);
        Some(timer.writes(slot, start, end))
    }

    /// Draw one mesh (draw slot `i`).
    fn draw_mesh(&self, pass: &mut wgpu::RenderPass, i: usize, d: &MeshDraw) {
        if let Some(m) = self.gpu_mesh(&d.mesh) {
            m.draw(pass, i as u32..i as u32 + 1);
        }
    }

    /// Draw main-pass batches with their material's pipeline and textures (`prepass` = Opaque
    /// draws test against the depth prepass; `scene` = scene copy id when the opaque scene was
    /// copied, custom shaders' group 3).
    #[allow(clippy::too_many_arguments)]
    fn draw_batches(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        lists: &frame_lists::FrameLists,
        batches: &[batch::Batch],
        prepass: bool,
        scene: Option<u64>,
        stats: &mut RenderStats,
    ) {
        use frame_debug::events::{BatchContext, ListKind};
        let kind = match prepass || !std::ptr::eq(batches, &lists.transparent[..]) {
            true => ListKind::Opaque,
            false => ListKind::Transparent,
        };
        let ctx = self
            .capture
            .is_some()
            .then(|| BatchContext::new(frame, batches, self.options.instancing, kind));
        let (mut pipeline_at, mut group_at) = (0, 0);
        for b in batches {
            let d = &frame.meshes[b.draw];
            let key = lists.prep.pipeline[b.draw];
            let resources = match &d.material.custom {
                Some(_) => self.material_resources(&d.material),
                None => {
                    let pipeline = match prepass && prepass::applies(key) {
                        true => self.prepass.shade_variant(key),
                        false => self.pipelines.mesh_variant(key),
                    };
                    pipeline.zip(self.main_group(&d.material))
                }
            };
            let (Some((pipeline, group)), Some(m)) = (resources, self.gpu_mesh(&d.mesh)) else {
                continue;
            };
            let after_prepass = prepass && prepass::applies(key);
            if !frame_debug::record(&self.capture, |r| {
                frame_debug::events::main_event(r, frame, b, m, ctx.as_ref(), key, after_prepass)
            }) {
                continue;
            }
            if batch::addr(pipeline) != pipeline_at {
                pipeline_at = batch::addr(pipeline);
                pass.set_pipeline(pipeline);
                stats.set_pass += 1;
                if d.material.custom.is_some()
                    && let Some(c) = self.custom.as_ref()
                {
                    pass.set_bind_group(3, c.scene_group_for(scene), &[]);
                }
            }
            if batch::addr(group) != group_at {
                group_at = batch::addr(group);
                pass.set_bind_group(2, group, &[]);
                stats.texture_binds += 1;
            }
            if d.material.custom.is_some() {
                match m.colors.as_ref().or(self.white_colors.as_ref()) {
                    Some(colors) => pass.set_vertex_buffer(1, colors.slice(..)),
                    None => continue,
                }
            }
            m.draw(pass, b.first..b.first + b.count);
            stats.draw(b.count, m.count, m.vertex_count);
        }
    }

    /// Main-pass pipeline and group 2 of a material (built-in variant or `.shader`).
    fn material_resources(
        &self,
        m: &MaterialParams,
    ) -> Option<(&wgpu::RenderPipeline, &wgpu::BindGroup)> {
        match &m.custom {
            Some(c) => self.custom.as_ref()?.get(c),
            None => Some((
                self.pipelines.mesh_variant(m.pipeline_key())?,
                self.main_group(m)?,
            )),
        }
    }

    /// Group 2 of a built-in material's main-pass variant: its textures, or for a feature
    /// variant its textures, feature maps and values.
    pub(super) fn main_group(&self, m: &MaterialParams) -> Option<&wgpu::BindGroup> {
        match m.lit_set() {
            Some(lit) => self.textures.get_lit(&lit),
            None => self.textures.get_set(&m.textures()),
        }
    }

    /// The transparent queue samples the opaque scene (water, scene-reading shaders).
    fn needs_scene_copy(&self, frame: &Frame) -> bool {
        if !frame.shaded || self.wire_only {
            return false;
        }
        if !frame.water.is_empty() || self.effects.needs_scene() {
            return true;
        }
        let Some(custom) = self.custom.as_ref() else {
            return false;
        };
        frame.meshes.iter().any(|d| {
            d.material
                .custom
                .as_ref()
                .is_some_and(|c| c.queue >= 3000 && custom.uses_scene(c))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn shadow_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        target: &wgpu::TextureView,
        slot: usize,
        lists: &frame_lists::FrameLists,
        list: Option<&frame_lists::ShadowList>,
        grass: bool,
        trees: Option<usize>,
        chunks: Option<&culling::Frustum>,
        stats: &mut RenderStats,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty shadow pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target,
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
        let Some(list) = list else {
            return;
        };
        let offset = (slot as u64 * shadows::VIEW_STRIDE) as u32;
        pass.set_bind_group(0, &self.shadows.view_bind_group, &[offset]);
        pass.set_bind_group(1, &self.draws_bind_group, &[]);
        let casters = Casters {
            grass,
            trees,
            chunks,
        };
        self.draw_casters(&mut pass, frame, slot, lists, list, casters, stats);
    }

    /// The casters of shadow map `slot` into an open pass (groups 0 and 1 bound by the
    /// caller): meshes, virtual geometry, grass (`grass`), trees (their list `trees`) and the
    /// terrains (only the chunks meeting `chunks` when given).
    #[allow(clippy::too_many_arguments)]
    fn draw_casters(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        slot: usize,
        lists: &frame_lists::FrameLists,
        list: &frame_lists::ShadowList,
        casters: Casters,
        stats: &mut RenderStats,
    ) {
        let Casters {
            grass,
            trees,
            chunks,
        } = casters;
        let mut pipeline_at = 0;
        let ctx = self.capture.is_some().then(|| {
            let kind = frame_debug::events::ListKind::Shadow;
            let instancing = self.options.instancing;
            frame_debug::events::BatchContext::new(frame, &list.batches, instancing, kind)
        });
        for b in &list.batches {
            let d = &frame.meshes[b.draw];
            let Some(key) = lists.prep.shadow[b.draw] else {
                continue;
            };
            let (Some(pipeline), Some(m)) =
                (self.pipelines.shadow_variant(key), self.gpu_mesh(&d.mesh))
            else {
                continue;
            };
            if !frame_debug::record(&self.capture, |r| {
                let shader = frame_debug::events::shadow_label(key);
                let depth = "LessEqual, write on (slope bias)";
                let e = frame_debug::events::mesh_event;
                e(
                    r,
                    frame,
                    b,
                    m,
                    ctx.as_ref(),
                    shader,
                    depth,
                    None,
                    key.double_sided,
                )
            }) {
                continue;
            }
            if key.cutout {
                let Some(group) = self.textures.get_set(&d.material.textures()) else {
                    continue;
                };
                pass.set_bind_group(2, group, &[]);
            }
            if batch::addr(pipeline) != pipeline_at {
                pipeline_at = batch::addr(pipeline);
                pass.set_pipeline(pipeline);
                stats.set_pass += 1;
            }
            m.draw(pass, b.first..b.first + b.count);
            stats.draw(b.count, m.count, m.vertex_count);
        }
        // Virtual geometry casters (culled for this map, detail chosen for the camera).
        if self.vgeo.active() {
            let textures = &self.textures;
            let material = |draw: usize| textures.get_set(&frame.meshes[draw].material.textures());
            self.vgeo.draw_shadow(
                pass,
                slot,
                &self.empty_group,
                &material,
                &self.vgeo_gate(frame),
            );
        }
        // Grass near the camera (the first cascades only; group 2 = the grass casters).
        if grass
            && frame_debug::record(&self.capture, |_| {
                frame_debug::events::post_event(
                    "Grass Shadow Casters".to_owned(),
                    "grass_shadow.wgsl",
                )
            })
        {
            pass.set_pipeline(&self.pipelines.grass_shadow);
            stats.set_pass += 1;
            (self.terrains).draw_grass_shadow(pass, frame.terrains, slot, stats);
        }
        // Trees into the sun's cascades and virtual pages (group 2 = the tree's textures).
        if let Some(t) = trees.filter(|_| self.trees.has_trees()) {
            self.trees.draw_shadow(pass, t, stats);
        }
        let first = frame.meshes.len();
        // Terrain casters pull heights from their own textures (group 3; group 2 unused).
        let Some(material) = self.textures.get(None, None) else {
            return;
        };
        let terrains = self.terrain_events(frame, |i| list.terrains[i], "Terrain Shadow Caster");
        pass.set_pipeline(&self.pipelines.terrain_shadow);
        pass.set_bind_group(2, material, &[]);
        match chunks {
            Some(f) => {
                (self.terrains).draw_in(pass, frame.terrains, first, |i| terrains[i], f, stats)
            }
            None => self
                .terrains
                .draw(pass, frame.terrains, first, false, |i| terrains[i], stats),
        }
    }

    /// Every blade near the camera into the near-camera grass shadow map (one pass per
    /// slice: the sharp inner square, then the outer one).
    fn grass_near_shadow_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        view_proj: &[glam::Mat4; grass_near_shadow::SLICES],
        stats: &mut RenderStats,
    ) {
        let near = &self.shadows.grass_near;
        for i in 0..view_proj.len() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty grass near shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &near.slice_views[i],
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
            let offset = grass_near_shadow::GrassNearShadow::view_offset(i);
            pass.set_bind_group(0, &near.view_group, &[offset]);
            pass.set_bind_group(1, &self.draws_bind_group, &[]);
            pass.set_pipeline(&self.pipelines.grass_shadow);
            // The inner slice may use only part of its layer.
            let side = near.slice_size(i) as f32;
            pass.set_viewport(0.0, 0.0, side, side, 0.0, 1.0);
            stats.set_pass += 1;
            (self.terrains).draw_grass_near_shadow(&mut pass, frame.terrains, i, stats);
            drop(pass);
            if i == 0 {
                self.mark(encoder, "Grass near shadow map (inner)");
            }
        }
    }

    fn mask_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        frame: &Frame,
        timing: Option<usize>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty selection mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.mask_view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: self.timestamps(timing, true, false),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipelines.mask);
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, &self.draws_bind_group, &[]);
        if frame.meshes.iter().any(|d| d.outline) {
            let mask = || frame_debug::target("Selection Mask", target.size(), "R8Unorm");
            frame_debug::pass(&self.capture, &["Selection Mask"], mask, EventTarget::None);
        }
        for (i, d) in frame.meshes.iter().enumerate() {
            if d.outline
                && frame_debug::record(&self.capture, |r| {
                    frame_debug::events::simple_draw(r, i, d, "mask.wgsl (selection mask)")
                })
            {
                self.draw_mesh(&mut pass, i, d);
            }
        }
    }

    /// Begin an MSAA main pass. `clear` = colour cleared (else loaded), `clear_depth` likewise;
    /// `resolve` = resolve into the HDR / glow images at its end; `store` / `store_depth` keep
    /// the MSAA images for a later pass.
    #[allow(clippy::too_many_arguments)]
    fn begin_main<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        target: &'e RenderTarget,
        clear: bool,
        clear_depth: bool,
        resolve: bool,
        store: bool,
        store_depth: bool,
    ) -> wgpu::RenderPass<'e> {
        let op = |keep: bool| match keep {
            true => wgpu::StoreOp::Store,
            false => wgpu::StoreOp::Discard,
        };
        let color = |view, resolve_view| wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: resolve.then_some(resolve_view),
            ops: wgpu::Operations {
                load: match clear {
                    true => wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    false => wgpu::LoadOp::Load,
                },
                store: op(store),
            },
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty main pass"),
            color_attachments: &[
                Some(color(target.msaa_view(), target.hdr_view())),
                // Emission only; drives bloom.
                Some(color(target.glow_msaa_view(), target.glow_view())),
                // Motion vectors (no resolve: TAA reads sample 0, like the depth).
                Some(wgpu::RenderPassColorAttachment {
                    view: target.velocity_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match clear {
                            true => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            false => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: match clear_depth {
                        true => wgpu::LoadOp::Clear(1.0),
                        false => wgpu::LoadOp::Load,
                    },
                    store: op(store_depth),
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass
    }

    /// The sky after the opaque scene: on the far plane, depth tested, so only the samples
    /// nothing covers shade it (see `main_pass`).
    fn draw_sky_late(&self, pass: &mut wgpu::RenderPass) {
        pass.set_pipeline(&self.pipelines.sky_late);
        pass.draw(0..3, 0..1);
        self.mark_pass(pass, "Main: sky (late)");
    }

    /// The frame's terrains in the main pass (group 1 bound by the caller).
    #[allow(clippy::too_many_arguments)]
    fn draw_terrain(
        &self,
        pass: &mut wgpu::RenderPass,
        target: &RenderTarget,
        frame: &Frame,
        lists: &frame_lists::FrameLists,
        pipeline: &wgpu::RenderPipeline,
        label: &'static str,
        stats: &mut RenderStats,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(2, &self.empty_group, &[]);
        let first = frame.meshes.len();
        let main = || main_target(target);
        frame_debug::pass(&self.capture, &["Terrain"], main, EventTarget::Hdr);
        let visible = self.terrain_events(frame, |i| lists.terrains[i], "Draw Terrain");
        self.terrains
            .draw(pass, frame.terrains, first, true, |i| visible[i], stats);
        self.mark_pass(pass, label);
    }

    /// One part of the main pass. GPU culling splits it into two phases around the Hi-Z build
    /// (`split.phase`); the last phase draws sky, opaque meshes, terrains and the grid, then
    /// (after the scene copy when `split.scene_copy`) water, transparent meshes, particles,
    /// overlays, lines and the selection outline.
    #[allow(clippy::too_many_arguments)]
    fn main_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        frame: &Frame,
        outline: bool,
        (lists, particle_draws): (&frame_lists::FrameLists, &[particles::ParticleDraw]),
        stats: &mut RenderStats,
        split: MainSplit,
    ) {
        crate::profile_scope!("Render.Encode.Main");
        let (first, last) = (split.phase != Some(1), split.phase != Some(0));
        let copy = last && (split.scene_copy || split.trace.is_some());
        // Sun shafts and glow occlusion read the depth after the main pass.
        // So does the virtual shadow map's page marking (unless it marked the screen-space
        // prepass's depth already), and depth of field and motion blur.
        let keep_depth = self.volumetrics.keeps_depth()
            || self.glows.keeps_depth()
            || self.vsm.wants_marks()
            || self.postfx.keeps_depth()
            || ui::keeps_depth();
        // The virtual geometry view this part draws: its phase, or with one main phase all of
        // it (view 0) where the opaque meshes draw.
        let single = self.vgeo.single_phase();
        let vgeo_view = match split.phase {
            Some(1) if single => None,
            Some(p) => Some(p),
            None if single => Some(0),
            None => None,
        }
        .filter(|_| self.vgeo.active());
        // Virtual geometry's visibility raster splits the phase's pass in two around it.
        let vis = vgeo_view.is_some() && self.vgeo.visibility_active();
        // The grass visibility buffer ends the last part after the grass depth (kept for the
        // grass visibility raster), the rest follows below.
        let grass_vis = last && split.vis;
        let mut grass_vis_drawn = false;
        // The sky after the opaque scene, on the far plane and depth tested: it then shades
        // only the samples nothing covers instead of the whole screen. Same picture (it was
        // overdrawn there); drawn before anything that writes colour without depth (the lit
        // grass, the grid, transparents). The Frame Debugger keeps the drawing order.
        let sky_late = fast_paths::sky_late() && self.capture.is_none();
        let mut sky_drawn = !sky_late;
        // With the grass visibility buffer the standing blades write the grass depth in its
        // raster: this part ends with the terrain's depth alone, and the terrain is shaded
        // (and the sky drawn) after the raster, tested against the blades' depth (see
        // `grass_vis_passes`).
        let vis_depth = grass_vis
            && sky_late
            && fast_paths::grass_vis_depth()
            && frame.shaded
            && !frame.terrains.is_empty()
            && self.terrains.has_grass();
        {
            let mut pass = self.begin_main(
                encoder,
                target,
                first,
                first && !split.prepass,
                last && !vis && !grass_vis,
                !last || copy || vis || grass_vis,
                !last || copy || keep_depth || vis || grass_vis,
            );
            if first {
                let main = || main_target(target);
                frame_debug::pass(&self.capture, &["Sky"], main, EventTarget::Hdr);
                let what = match split.prepass {
                    true => "Clear (color)",
                    false => "Clear (color, depth)",
                };
                frame_debug::record(&self.capture, |_| FrameEvent::new(EventKind::Clear, what));
                if !sky_late
                    && frame_debug::record(&self.capture, |_| {
                        frame_debug::events::post_event("Draw Sky".to_owned(), "sky.wgsl fs_main")
                    })
                {
                    pass.set_pipeline(&self.pipelines.sky);
                    pass.draw(0..3, 0..1);
                }
            }

            self.mark_pass(&mut pass, "Main: sky");
            // Opaque and Cutout meshes, front to back (or this phase's GPU-culled batches),
            // then this phase's virtual geometry clusters. Without GPU culling the CPU-culled
            // batches all draw in the first phase.
            if let Some(phase) = split.phase {
                pass.set_bind_group(1, self.lit_group(), &[]);
                if let Some(culled) = split.culled {
                    let main = || main_target(target);
                    let leaf = if phase == 0 {
                        "First Phase"
                    } else {
                        "Second Phase"
                    };
                    let path = ["Opaque (GPU-driven)", leaf];
                    frame_debug::pass(&self.capture, &path, main, EventTarget::Hdr);
                    self.draw_culled(&mut pass, frame, lists, culled, phase, stats);
                } else if phase == 0 && frame.shaded && !lists.opaque.is_empty() {
                    let opaque = &lists.opaque;
                    self.draw_batches(&mut pass, frame, lists, opaque, split.prepass, None, stats);
                }
            }
            if split.phase.is_none() && frame.shaded && !lists.opaque.is_empty() {
                let main = || main_target(target);
                frame_debug::pass(&self.capture, &["Opaque"], main, EventTarget::Hdr);
                pass.set_bind_group(1, self.lit_group(), &[]);
                let opaque = &lists.opaque;
                self.draw_batches(&mut pass, frame, lists, opaque, split.prepass, None, stats);
            }
            if let Some(phase) = vgeo_view {
                pass.set_bind_group(1, self.lit_group(), &[]);
                self.mark_pass(&mut pass, "Main: opaque meshes");
                let textures = &self.textures;
                let material =
                    |draw: usize| textures.get_set(&frame.meshes[draw].material.textures());
                if vis {
                    drop(pass);
                    let groups = [
                        &self.view_bind_group,
                        &self.draws_bind_group,
                        &self.empty_group,
                    ];
                    self.vgeo.raster_visibility(
                        encoder,
                        target.depth_view(),
                        phase,
                        groups,
                        &material,
                        &self.vgeo_gate(frame),
                    );
                    self.mark(encoder, "Virtual geometry: visibility raster");
                    pass = self.begin_main(
                        encoder,
                        target,
                        false,
                        false,
                        last && !grass_vis,
                        !last || copy || grass_vis,
                        !last || copy || keep_depth || grass_vis,
                    );
                    pass.set_bind_group(1, self.lit_group(), &[]);
                    self.vgeo
                        .resolve(&mut pass, phase, &material, &self.vgeo_gate(frame));
                    self.mark_pass(&mut pass, "Main: virtual geometry (resolve)");
                } else {
                    self.vgeo
                        .draw_main(&mut pass, phase, &material, &self.vgeo_gate(frame));
                    self.mark_pass(&mut pass, "Main: virtual geometry");
                }
            }
            if !last {
                return;
            }

            self.mark_pass(&mut pass, "Main: opaque meshes");
            if frame.shaded && !frame.terrains.is_empty() {
                pass.set_bind_group(1, self.lit_group(), &[]);
                // The terrain after the trees and the grass's depth: its costly layer shading
                // then skips the pixels they hide completely (dense turf close up). Same
                // picture: every sample the grass depth wins is later written by the grass
                // (lit pass or composite), and the trees' depth hides the same terrain either
                // way. The Frame Debugger keeps the drawing order (its replay stops at an
                // event).
                let late = fast_paths::terrain_late() && self.capture.is_none();
                if !late && !vis_depth {
                    let terrain = &self.pipelines.terrain;
                    self.draw_terrain(
                        &mut pass,
                        target,
                        frame,
                        lists,
                        terrain,
                        "Main: terrain",
                        stats,
                    );
                }
                // Trees before the grass: the grass's depth pass then skips what they hide.
                if self.trees.has_trees()
                    && frame_debug::record(&self.capture, |_| {
                        frame_debug::events::post_event("Draw Trees".to_owned(), "tree.wgsl")
                    })
                {
                    self.trees.draw_main(&mut pass, stats);
                }
                let grass = self.terrains.has_grass()
                    && frame_debug::record(&self.capture, |_| {
                        frame_debug::events::grass_event(self.terrains.grass_indirect())
                    });
                if vis_depth {
                    self.mark_pass(&mut pass, "Main: trees");
                    // (Drawn again when shaded: counted once.)
                    let depth = &self.pipelines.terrain_depth;
                    let mut uncounted = RenderStats::default();
                    let label = "Main: terrain depth";
                    self.draw_terrain(
                        &mut pass,
                        target,
                        frame,
                        lists,
                        depth,
                        label,
                        &mut uncounted,
                    );
                } else if grass {
                    // Depth first (alpha tested), then the shading once per covered sample:
                    // dense turf overlaps each pixel many times.
                    self.mark_pass(&mut pass, "Main: trees");
                    pass.set_pipeline(&self.pipelines.grass_depth);
                    let depth = grass_cull::DEPTH;
                    self.terrains
                        .draw_grass(&mut pass, frame.terrains, depth, stats);
                    self.mark_pass(&mut pass, "Main: grass depth");
                }
                if late && !vis_depth {
                    let terrain = &self.pipelines.terrain;
                    self.draw_terrain(
                        &mut pass,
                        target,
                        frame,
                        lists,
                        terrain,
                        "Main: terrain",
                        stats,
                    );
                }
                if !sky_drawn && !vis_depth {
                    self.draw_sky_late(&mut pass);
                    sky_drawn = true;
                }
                if grass {
                    if grass_vis {
                        grass_vis_drawn = true;
                    } else {
                        pass.set_pipeline(&self.pipelines.grass);
                        if self.terrains.lit_fans_only() {
                            // The lit lane holds only the fans this frame (planned for the
                            // visibility buffer, which then could not run): blades first.
                            let standing = grass_cull::DEPTH;
                            (self.terrains).draw_grass(&mut pass, frame.terrains, standing, stats);
                        }
                        let lit = grass_cull::LIT;
                        self.terrains
                            .draw_grass(&mut pass, frame.terrains, lit, stats);
                        self.mark_pass(&mut pass, "Main: grass lit");
                    }
                }
            }
            if !sky_drawn && !vis_depth {
                self.draw_sky_late(&mut pass);
            }

            if !grass_vis {
                self.main_grid(&mut pass, target, frame);
                if !copy {
                    self.main_tail(
                        &mut pass,
                        target,
                        frame,
                        outline,
                        lists,
                        particle_draws,
                        None,
                        stats,
                    );
                    return;
                }
            }
        }
        if grass_vis {
            // Which blade each sample shows, each visible blade shaded once, then the rest of
            // the main pass starting with the shaded blades written into their samples.
            if grass_vis_drawn {
                self.grass_vis_passes(device, encoder, target, frame, vis_depth, stats);
            }
            let mut pass = self.begin_main(
                encoder,
                target,
                false,
                false,
                true,
                copy,
                copy || keep_depth,
            );
            if vis_depth {
                // The terrain where no standing blade hides it, then the sky.
                pass.set_bind_group(1, self.lit_group(), &[]);
                let shade = &self.pipelines.terrain_shade;
                self.draw_terrain(
                    &mut pass,
                    target,
                    frame,
                    lists,
                    shade,
                    "Main: terrain",
                    stats,
                );
                self.draw_sky_late(&mut pass);
            }
            if grass_vis_drawn {
                self.grass_vis_composite(&mut pass, target);
            }
            self.main_grid(&mut pass, target, frame);
            if !copy {
                self.main_tail(
                    &mut pass,
                    target,
                    frame,
                    outline,
                    lists,
                    particle_draws,
                    None,
                    stats,
                );
                return;
            }
        }
        // The opaque scene, sampled by water refraction and scene-reading shaders.
        let main = || main_target(target);
        if split.scene_copy {
            frame_debug::pass(&self.capture, &["Scene Copy"], main, EventTarget::Hdr);
            frame_debug::record(&self.capture, |_| {
                FrameEvent::new(EventKind::Copy, "Copy Opaque Color + Depth")
            });
            self.scene_copies.record(device, encoder, target);
        }
        // Next render's SSGI / SSR read this frame's lit opaque scene.
        if let Some(t) = split.trace {
            self.screen_trace_history(encoder, target, t);
        }
        let scene = split
            .scene_copy
            .then(|| self.scene_copies.current(target.size()).map(|c| c.id))
            .flatten();
        let mut pass = self.begin_main(encoder, target, false, false, true, false, keep_depth);
        if frame.shaded && !frame.water.is_empty() {
            frame_debug::pass(&self.capture, &["Water"], main, EventTarget::Hdr);
        }
        if frame.shaded
            && !frame.water.is_empty()
            && frame_debug::record(&self.capture, |_| {
                frame_debug::events::post_event("Draw Water Surfaces".to_owned(), "water.wgsl")
            })
        {
            pass.set_bind_group(1, self.lit_group(), &[]);
            self.water.draw(&mut pass, frame, stats);
        }
        self.main_tail(
            &mut pass,
            target,
            frame,
            outline,
            lists,
            particle_draws,
            scene,
            stats,
        );
    }

    /// The editor grid (after the opaque scene, before transparents).
    fn main_grid(&self, pass: &mut wgpu::RenderPass, target: &RenderTarget, frame: &Frame) {
        self.draw_wire_background(pass);
        if frame.grid.is_some() {
            let main = || main_target(target);
            frame_debug::pass(&self.capture, &["Gizmos"], main, EventTarget::Hdr);
        }
        if frame.grid.is_some()
            && frame_debug::record(&self.capture, |_| {
                frame_debug::events::post_event("Draw Grid".to_owned(), "grid.wgsl")
            })
        {
            pass.set_pipeline(&self.pipelines.grid);
            pass.draw(0..3, 0..1);
        }
    }

    /// This render's lit grass goes through the visibility buffer: switched on, the device
    /// has the pipelines and the target's images, and every terrain's grass can be named by
    /// visibility ids this frame (else the lit pass draws it, as before).
    fn grass_vis_active(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
    ) -> bool {
        if !grass_vis::grass_vis_enabled()
            || !frame.shaded
            || frame.terrains.is_empty()
            || !self.terrains.has_grass()
            || !self.terrains.grass_vis_ready(frame.terrains)
        {
            return false;
        }
        if self.grass_vis.is_none() {
            crate::profile_scope!("GrassVis.Pipelines");
            let built = (self.grass_vis_prebuilt.take())
                .unwrap_or_else(|| grass_vis::VisPipelines::new(device, &self.pipelines));
            self.grass_vis = Some(built);
        }
        let Some(Some(pipes)) = &self.grass_vis else {
            return false;
        };
        let images = target
            .grass_vis
            .get(device, pipes, target.size(), target.samples());
        if !matches!(&*images, Some(Some(_))) {
            return false;
        }
        let terrains = self.terrains.grass_vis_terrains(frame.terrains);
        grass_vis::write_frame(queue, pipes, target.size(), &terrains);
        true
    }

    /// The grass visibility raster (after the main pass's first part, which kept its depth)
    /// and the resolve that shades each visible blade once. `depth`: the main pass left the
    /// grass depth out, so the standing blades write it here (in the grass depth pass's order
    /// and test, the last one at the nearest depth keeping a sample, as before), then the flat
    /// fans test against it without writing. The only difference to the lit grass's order: a
    /// fan now always shows over a standing blade behind it (before, a blade drawn after the
    /// fan covered it).
    fn grass_vis_passes(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        frame: &Frame,
        depth: bool,
        stats: &mut RenderStats,
    ) {
        let Some(Some(pipes)) = &self.grass_vis else {
            return;
        };
        let images = target
            .grass_vis
            .get(device, pipes, target.size(), target.samples());
        let Some(Some(t)) = &*images else {
            return;
        };
        {
            let mut pass = grass_vis::begin_raster(encoder, t, target.depth_view());
            pass.set_bind_group(0, &self.view_bind_group, &[]);
            pass.set_bind_group(1, self.lit_group(), &[]);
            let lit = grass_cull::LIT;
            if depth {
                pass.set_pipeline(pipes.raster_standing());
                let standing = grass_cull::DEPTH;
                self.terrains
                    .draw_grass(&mut pass, frame.terrains, standing, stats);
                // (With the ground cover there are no fans: the lit lane holds only blades.)
                if self.terrains.draws_fans() {
                    pass.set_pipeline(pipes.raster_fans());
                    self.terrains
                        .draw_grass(&mut pass, frame.terrains, lit, stats);
                }
            } else {
                pass.set_pipeline(pipes.raster());
                self.terrains
                    .draw_grass(&mut pass, frame.terrains, lit, stats);
            }
        }
        self.mark(encoder, "Grass visibility");
        let terrains = self.terrains.grass_vis_terrains(frame.terrains);
        grass_vis::resolve(
            device,
            encoder,
            pipes,
            t,
            &self.view_bind_group,
            self.lit_group(),
            &terrains,
        );
        self.mark(encoder, "Grass shading");
    }

    /// Write the resolved blades into their samples (first draw of the main pass's rest).
    fn grass_vis_composite(&self, pass: &mut wgpu::RenderPass, target: &RenderTarget) {
        let Some(Some(pipes)) = &self.grass_vis else {
            return;
        };
        let Some(group) = target.grass_vis.composite_group() else {
            return;
        };
        pass.set_pipeline(pipes.composite());
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..3, 0..1);
        self.mark_pass(pass, "Main: grass composite");
    }

    /// The main pass after opaque geometry; `scene` = scene copy id for scene-reading shaders.
    #[allow(clippy::too_many_arguments)]
    fn main_tail(
        &self,
        pass: &mut wgpu::RenderPass,
        target: &RenderTarget,
        frame: &Frame,
        outline: bool,
        lists: &frame_lists::FrameLists,
        particle_draws: &[particles::ParticleDraw],
        scene: Option<u64>,
        stats: &mut RenderStats,
    ) {
        let main = || main_target(target);
        // Fade / Transparent meshes back to front, after the grid (they don't write depth).
        let shaded = frame.shaded && !self.wire_only;
        if shaded && !lists.transparent.is_empty() {
            frame_debug::pass(&self.capture, &["Transparent"], main, EventTarget::Hdr);
            pass.set_bind_group(1, self.lit_group(), &[]);
            let transparent = &lists.transparent;
            self.draw_batches(pass, frame, lists, transparent, false, scene, stats);
        }

        // After the grid: particles don't write depth, so the grid would draw over them.
        if frame.shaded && !particle_draws.is_empty() {
            frame_debug::pass(&self.capture, &["Particles"], main, EventTarget::Hdr);
            self.draw_particles(pass, frame, particle_draws, stats);
        }
        if shaded
            && (!self.effects.has_draws()
                || frame_debug::whole(
                    &self.capture,
                    &["Visual Effects"],
                    main,
                    EventTarget::Hdr,
                    frame_debug::events::effects_event,
                ))
        {
            self.effects.draw(pass, &self.textures, scene, stats);
        }

        let wire = self.wire.is_some();
        if !frame.overlay.is_empty() || !frame.lines.is_empty() || outline || wire {
            frame_debug::pass(&self.capture, &["Gizmos"], main, EventTarget::Hdr);
        }
        let gizmo = |name: &str, shader: &str| {
            frame_debug::record(&self.capture, |_| {
                frame_debug::events::post_event(name.to_owned(), shader)
            })
        };
        if wire && gizmo("Draw Wireframe", "wireframe.wgsl") {
            self.draw_wireframe(pass, frame, lists);
        }
        if self.overlay_drawn > 0 && gizmo("Draw Overlay Triangles", "lines.wgsl") {
            let first = self.lines_drawn;
            pass.set_pipeline(&self.pipelines.overlay);
            pass.set_vertex_buffer(0, self.line_buffer.slice(..));
            pass.draw(first..first + self.overlay_drawn, 0..1);
        }

        if self.lines_drawn > 0 && gizmo("Draw Gizmo Lines", "lines.wgsl") {
            pass.set_pipeline(&self.pipelines.lines);
            pass.set_vertex_buffer(0, self.line_buffer.slice(..));
            pass.draw(0..self.lines_drawn, 0..1);
        }

        if outline && gizmo("Draw Selection Outline", "outline.wgsl") {
            pass.set_pipeline(&self.pipelines.outline);
            pass.set_bind_group(1, &target.mask_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

impl GpuMesh {
    fn new(device: &wgpu::Device, label: &str, data: &mesh::MeshData) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let (lo, hi) = data.bounds();
        Self {
            buffer_id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&data.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&data.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            first_index: 0,
            base_vertex: 0,
            count: data.indices.len() as u32,
            vertex_count: data.vertices.len() as u32,
            bounds: culling::Aabb::new(lo, hi),
            colors: (!data.colors.is_empty() && data.colors.len() == data.vertices.len()).then(
                || {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytemuck::cast_slice(&data.colors),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
                },
            ),
        }
    }

    /// Every primitive, in [`PrimitiveMesh::ALL`] order, in one shared buffer pair.
    fn primitives(device: &wgpu::Device) -> Vec<Self> {
        let mut all = mesh::MeshData::default();
        let mut ranges = Vec::new();
        for p in PrimitiveMesh::ALL {
            let data = mesh::primitive(p);
            ranges.push((all.indices.len() as u32, all.vertices.len() as i32, data));
            all.vertices.extend_from_slice(&data.vertices);
            all.indices.extend_from_slice(&data.indices);
        }
        let shared = Self::new(device, "misty primitives", &all);
        PrimitiveMesh::ALL
            .iter()
            .zip(ranges)
            .map(|(p, (first_index, base_vertex, data))| Self {
                vertices: shared.vertices.clone(),
                indices: shared.indices.clone(),
                buffer_id: shared.buffer_id,
                first_index,
                base_vertex,
                count: data.indices.len() as u32,
                vertex_count: data.vertices.len() as u32,
                // The same box picking and shadow fitting use.
                bounds: culling::Aabb::new(-p.half_extents(), p.half_extents()),
                colors: None,
            })
            .collect()
    }

    /// Draw `instances` (entries of the slots buffer).
    fn draw(&self, pass: &mut wgpu::RenderPass, instances: std::ops::Range<u32>) {
        self.bind(pass);
        let first = self.first_index;
        pass.draw_indexed(first..first + self.count, self.base_vertex, instances);
    }

    fn bind(&self, pass: &mut wgpu::RenderPass) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
    }

    /// Bytes of this mesh's own vertices and indices.
    fn bytes(&self) -> u64 {
        self.vertex_count as u64 * std::mem::size_of::<mesh::Vertex>() as u64
            + self.count as u64 * 4
    }
}

/// The main pass's attachments (Frame Debugger details).
fn main_target(target: &RenderTarget) -> TargetInfo {
    frame_debug::target(
        "HDR Color (MSAA ×4) + Emission + Motion + Depth",
        target.size(),
        "Rgba16Float / Rg16Float / Depth32Float",
    )
}

fn draws_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty draws"),
        size: (capacity * std::mem::size_of::<DrawUniform>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn slots_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty draw slots"),
        size: (capacity * std::mem::size_of::<u32>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// What [`Renderer::draw_casters`] draws besides the meshes and virtual geometry.
struct Casters<'a> {
    /// Grass shadow casters (sun cascades within the casters' range).
    grass: bool,
    /// The trees' shadow list to draw (cascades, then virtual shadow map rectangles).
    trees: Option<usize>,
    /// Only the terrain chunks meeting this frustum (the shadow map's own view).
    chunks: Option<&'a culling::Frustum>,
}

/// Group 1 of the lit mesh pass and of the shadow/mask passes; `lights` = the lights uniform
/// and the LTC table; `buffers` = draws, slots; `screen` = screen-space texels (AO, contact
/// shadows) and the screen-space trace texels (SSGI, SSR).
#[allow(clippy::too_many_arguments)]
fn draw_groups(
    device: &wgpu::Device,
    p: &pipelines::Pipelines,
    lights: [&wgpu::Buffer; 3],
    buffers: [&wgpu::Buffer; 2],
    shadows: &shadows::ShadowMaps,
    probes: &probes::gpu::ProbeGpu,
    lightmaps: &lightmap_gpu::LightmapGpu,
    [screen, trace, shadow_mask]: [&wgpu::Buffer; 3],
    [vsm_table, vsm_pool, vsm_near]: [&wgpu::TextureView; 3],
) -> (wgpu::BindGroup, wgpu::BindGroup) {
    let [lights, ltc, lights_2d] = lights;
    let [draws, slots] = buffers;
    let [p0, p1, p2, p3, p4] = probes.entries();
    let [l0, l1] = lightmaps.entries();
    let mesh = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty mesh group"),
        layout: &p.mesh_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: lights.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: draws.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&shadows.cascade_array_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&shadows.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&shadows.local_array_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: slots.as_entire_binding(),
            },
            // Soft-shadow parameters (shadows.wgsl).
            wgpu::BindGroupEntry {
                binding: 6,
                resource: shadows.params_buffer.as_entire_binding(),
            },
            // Rectangle lights' LTC table (area_light.wgsl).
            wgpu::BindGroupEntry {
                binding: 7,
                resource: ltc.as_entire_binding(),
            },
            // Near-camera grass shadow map (grass_near_shadow.rs).
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&shadows.grass_near.view),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: shadows.grass_near.uniform.as_entire_binding(),
            },
            // The sun's virtual shadow map (render/vsm).
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(vsm_table),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::TextureView(vsm_pool),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::TextureView(vsm_near),
            },
            p0,
            p1,
            p2,
            p3,
            p4,
            l0,
            l1,
            wgpu::BindGroupEntry {
                binding: 16,
                resource: screen.as_entire_binding(),
            },
            // Screen-space GI and reflection texels (screen_trace).
            wgpu::BindGroupEntry {
                binding: 17,
                resource: trace.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: shadow_denoise::BINDING,
                resource: shadow_mask.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: light2d::BINDING,
                resource: lights_2d.as_entire_binding(),
            },
        ],
    });
    let draws_only = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty draws group"),
        layout: &p.draws_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 1,
                resource: draws.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: slots.as_entire_binding(),
            },
        ],
    });
    (mesh, draws_only)
}

fn line_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty lines"),
        size: (capacity * std::mem::size_of::<LineVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Sort distance of a world point: along the view axis when orthographic, else squared distance.
/// Grass view values: x = angle of one pixel (radians, 0 = orthographic), y = 1 with TAA,
/// z = seconds the grass clock advanced since last frame (wind motion vectors).
fn grass_screen(frame: &Frame, height: u32, taa: bool, dt: f32) -> [f32; 3] {
    let pixel = match frame.camera.projection {
        crate::camera::Projection::Perspective { fov_y } => {
            2.0 * (fov_y.to_radians() * 0.5).tan() / height.max(1) as f32
        }
        crate::camera::Projection::Orthographic { .. } => 0.0,
    };
    // The wind's motion vectors only matter to TAA (the other temporal passes reproject the
    // grass with the camera alone, as they always did): without it the vertex stage skips
    // the second blade evaluation.
    let dt = if taa { dt } else { 0.0 };
    [pixel, if taa { 1.0 } else { 0.0 }, dt]
}

fn camera_depth(cam: &Camera) -> impl Fn(Vec3) -> f32 + use<> {
    let (eye, forward) = (cam.position, cam.forward());
    let ortho = matches!(cam.projection, Projection::Orthographic { .. });
    move |p| {
        if ortho {
            (p - eye).dot(forward)
        } else {
            p.distance_squared(eye)
        }
    }
}

pub(crate) fn srgb_to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| v.max(0.0).powf(2.2))
}

/// `draws[]` entry of a mesh draw; `lod_fade` = its LOD cross-fade (0 = none),
/// `prev_model` = last frame's model matrix (motion vectors).
fn draw_uniform(d: &MeshDraw, lod_fade: f32, prev_model: Mat4) -> DrawUniform {
    // Colour to linear first, then scale: intensity is linear brightness.
    let e = srgb_to_linear(d.material.emission).map(|c| c * d.material.emission_intensity);
    // Mirrored transforms flip the tangent frame's handedness.
    let mirror = if d.model.determinant() < 0.0 {
        -1.0
    } else {
        1.0
    };
    // No normal map: scale 0 keeps the vertex normal exactly.
    let normal_scale = if d.material.normal_map.is_some() {
        d.material.normal_scale
    } else {
        0.0
    };
    let m = &d.material;
    let (t, o) = (m.tiling, m.offset);
    // A metallic map replaces the slider: metallic = map R.
    let metallic = if m.metallic_map.is_some() {
        1.0
    } else {
        m.metallic
    };
    // No rotation gives exactly (1, 0), so draw_uv returns the plain tiled uv.
    let (sin, cos) = m.uv_rotation.to_radians().sin_cos();
    DrawUniform {
        model: d.model.to_cols_array_2d(),
        normal: d.model.inverse().transpose().to_cols_array_2d(),
        color: m.color,
        params: [
            metallic,
            m.smoothness,
            if d.receive_shadows { 1.0 } else { 0.0 },
            normal_scale,
        ],
        emission: [e[0], e[1], e[2], mirror],
        uv_st: [t[0], t[1], o[0], o[1]],
        surface: [
            m.mode.index() as f32,
            m.alpha_cutoff,
            if m.occlusion_map.is_some() {
                m.occlusion_strength
            } else {
                0.0
            },
            if m.smoothness_from_albedo { 1.0 } else { 0.0 },
        ],
        extra: [
            cos,
            sin,
            lod_fade,
            light2d::draw_flag(m.lit_2d, d.sorting.layer),
        ],
        lightmap_st: m.lightmap.map_or([1.0, 1.0, 0.0, 0.0], |l| l.scale_offset),
        lightmap: [
            m.lightmap.map_or(0.0, |l| l.index as f32 + 1.0),
            if m.contribute_gi { 0.0 } else { 1.0 },
            0.0,
            0.0,
        ],
        prev_model: prev_model.to_cols_array_2d(),
    }
}

/// Light type index of the shaders (`LightItem.dir_type.w`, `FroxelLight.dir_type.w`).
pub(crate) fn light_kind_index(kind: LightType) -> f32 {
    match kind {
        LightType::Directional => 0.0,
        LightType::Point => 1.0,
        LightType::Spot => 2.0,
        LightType::Rectangle => 3.0,
    }
}

/// `soft` = false draws Soft lights with hard shadows (Quality ▸ Soft Shadows Off).
fn lights_uniform(frame: &Frame, plan: &shadows::ShadowPlan, soft: bool) -> LightsUniform {
    let mut u = LightsUniform::zeroed();
    let mut n = 0;
    for (i, l) in frame.lights.iter().enumerate() {
        if n == MAX_LIGHTS || l.intensity <= 0.0 {
            continue;
        }
        let c = srgb_to_linear(l.color);
        let kind = light_kind_index(l.kind);
        let half = (l.spot_angle * 0.5).to_radians();
        let (mode, layer) = match plan.light_layers.get(i).copied().flatten() {
            Some(layer) => (
                match l.shadows {
                    Shadows::None => 0.0,
                    Shadows::Hard => 1.0,
                    Shadows::Soft if soft => 2.0,
                    Shadows::Soft => 1.0,
                },
                layer as f32,
            ),
            None => (0.0, -1.0),
        };
        u.items[n] = LightItem {
            pos_range: l.position.extend(l.range).to_array(),
            dir_type: l.direction.normalize_or(Vec3::Z).extend(kind).to_array(),
            color: [
                c[0] * l.intensity,
                c[1] * l.intensity,
                c[2] * l.intensity,
                0.0,
            ],
            // z = contact shadows (screen_space.wgsl).
            spot: [
                half.cos(),
                (half * 0.8).cos(),
                if l.contact_shadows { 1.0 } else { 0.0 },
                0.0,
            ],
            shadow: [mode, layer, 0.0, 0.0],
            area_x: [0.0; 4],
            area_y: [0.0; 4],
        };
        if l.kind == LightType::Rectangle {
            let (hx, hy) = l.area.half_axes();
            let sides = if l.area.two_sided { 1.0 } else { 0.0 };
            u.items[n].area_x = hx.extend(sides).to_array();
            u.items[n].area_y = hy.extend(0.0).to_array();
        }
        n += 1;
    }
    let env = &frame.environment.settings;
    // Cascade 0 stands in for the single sun map of older shader code.
    let first = plan.cascades.first();
    u.info = [
        n as f32,
        first.map_or(0.0, |c| c.texel),
        env.environment_lighting.intensity.max(0.0),
        env.reflection_intensity.max(0.0),
    ];
    u.shadow_view_proj = first
        .map_or(Mat4::IDENTITY, |c| c.view_proj)
        .to_cols_array_2d();
    for (i, m) in plan.local.iter().enumerate() {
        u.local_view_proj[i] = m.to_cols_array_2d();
    }
    // Ambient from the Lighting window's Environment Lighting source.
    let [sky, equator, ground] = frame.environment.ambient();
    u.sky = sky.extend(0.0).to_array();
    u.equator = equator.extend(0.0).to_array();
    u.ground = ground.extend(0.0).to_array();
    u
}

/// The view uniform; `jitter` = sub-pixel camera offset in pixels of a `size` image (TAA),
/// `prev_view_proj` = last frame's unjittered view-projection (motion vectors).
/// Mip bias of material textures for `target`: log2 of its render scale (an upscaled target
/// samples the detail of the output resolution; the temporal resolve averages the extra
/// aliasing away), 0 at native resolution.
fn mip_bias(target: &RenderTarget) -> f32 {
    let (w, _) = target.size();
    let (ow, _) = target.output_size();
    if w >= ow {
        return 0.0;
    }
    (w as f32 / ow as f32).log2()
}

fn view_uniform(
    frame: &Frame,
    aspect: f32,
    jitter: glam::Vec2,
    size: (u32, u32),
    prev_view_proj: Mat4,
    mip_bias: f32,
) -> ViewUniform {
    let cam = frame.camera;
    let vp = taa::jittered(cam.view_projection(aspect), jitter, size);
    let ortho = matches!(cam.projection, Projection::Orthographic { .. });
    let (axis, lod_ref) = match frame.grid {
        Some(plane) => {
            let axis = plane.axis();
            let lod_ref = match cam.projection {
                Projection::Orthographic { size } => size,
                Projection::Perspective { .. } => cam.position[axis].abs(),
            };
            (axis, lod_ref)
        }
        None => (1, 1.0),
    };
    // Ortho eyes sit far behind the pivot, so fading by eye distance would hide the grid.
    let fade = if ortho {
        1.0e30
    } else {
        (lod_ref * 40.0).clamp(30.0, cam.far * 0.9)
    };
    let bg = srgb_to_linear(frame.background);
    let mut environment = frame.environment.pack(cam.far);
    if water::camera_underwater(frame) {
        // Under water the underwater pass is the fog; air fog would apply twice.
        environment[6][3] = 0.0;
    }
    ViewUniform {
        view_proj: vp.to_cols_array_2d(),
        inv_view_proj: vp.inverse().to_cols_array_2d(),
        camera_pos: cam
            .position
            .extend(if ortho { 1.0 } else { 0.0 })
            .to_array(),
        grid: [
            axis as f32,
            fade,
            if frame.grid.is_some() { 1.0 } else { 0.0 },
            lod_ref,
        ],
        background: [bg[0], bg[1], bg[2], if frame.skybox { 1.0 } else { 0.0 }],
        environment,
        post: frame.post.pack(),
        prev_view_proj: prev_view_proj.to_cols_array_2d(),
        motion: {
            let j = taa::jitter_ndc(jitter, size);
            [j.x, j.y, size.0 as f32, size.1 as f32]
        },
        view_ray: {
            let mut r = view_ray(cam, aspect);
            // view_tan.z: texture mip bias (upscaled targets, see `mip_bias`).
            r[1][2] = mip_bias;
            r
        },
    }
}

/// The camera's rotation and half-angle tangents, so shaders build exact view rays
/// (`view_ray` in common.wgsl). Unprojecting the far plane through the inverse
/// view-projection loses most of its precision when the near / far ratio is large (the
/// Scene view's dynamic clipping reaches 1 : 500 000): the ray then wobbles by up to a pixel
/// from one camera orientation to the next, and the sky (sun disc, clouds) shakes while
/// turning.
fn view_ray(cam: &Camera, aspect: f32) -> [[f32; 4]; 2] {
    let q = cam.rotation.normalize();
    let tan = match cam.projection {
        Projection::Perspective { fov_y } => {
            let t = (fov_y.to_radians() * 0.5).tan();
            [t * aspect.max(1e-4), t, 0.0, 0.0]
        }
        Projection::Orthographic { .. } => [0.0, 0.0, 1.0, 0.0],
    };
    [q.to_array(), tan]
}

#[cfg(test)]
mod benchmark;
#[cfg(test)]
mod flowers_gpu_tests;
#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod gpu_checks;
#[cfg(test)]
mod material_v2_gpu_tests;
/// Make sure project `root`'s texture cache holds the blocks of the textures the renderer makes
/// from the packed `files` for `platform`: tree maps and impostors (`.tree`), terrain layer
/// arrays, the grass atlas and flower layers (`.terrain`). Returns the cache entries
/// (project-relative, what a build packs) and warnings. The images are read from the open
/// project (`assets::absolute`), which a build's project is.
pub fn bake_made_textures(
    root: &std::path::Path,
    files: &[String],
    platform: &'static str,
) -> (Vec<String>, Vec<String>) {
    let ctx = crate::sprites::compress::generated::Ctx::bake(root, platform);
    let (mut entries, mut warnings) = (Vec::new(), Vec::new());
    for rel in files {
        if rel.ends_with(".tree") {
            match trees::bake_textures(rel, &ctx) {
                Ok(e) => entries.extend(e),
                Err(e) => warnings.push(format!("{rel}: {e}")),
            }
        } else if rel.ends_with(".terrain") {
            let d = match crate::terrain::TerrainData::load(&crate::assets::absolute(rel)) {
                Ok(d) => d,
                Err(e) => {
                    warnings.push(e);
                    continue;
                }
            };
            entries.extend(terrain_layers::bake_textures(&d.layers, &ctx));
            if d.details.enabled && !d.detail_map.is_empty() {
                let flowers = d.flowers.enabled && !d.flowers.kinds.is_empty();
                let kinds = flowers.then_some(&d.flowers.kinds[..]);
                entries.extend(terrain_grass::bake_textures(kinds, &ctx));
            }
        }
    }
    entries.sort();
    entries.dedup();
    (entries, warnings)
}

#[cfg(test)]
pub(crate) mod gpu_tests;
#[cfg(test)]
mod grass_study;
#[cfg(test)]
mod grass_vis_tests;
#[cfg(test)]
mod hitch_tests;
#[cfg(test)]
mod made_textures_tests;
#[cfg(test)]
mod meadow_perf_tests;
#[cfg(test)]
mod noon_valley_tests;
#[cfg(test)]
mod rocks_gpu_tests;
#[cfg(test)]
mod shadow_stutter_tests;
#[cfg(test)]
mod streaming_tests;
#[cfg(test)]
mod taa_gpu_tests;
#[cfg(test)]
mod terrain_gpu_tests;
#[cfg(test)]
mod upscale_gpu_tests;
#[cfg(test)]
mod wireframe_gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_struct_sizes_match_wgsl() {
        assert_eq!(std::mem::size_of::<DrawUniform>(), 320);
        assert_eq!(std::mem::size_of::<LightItem>(), 112);
        assert_eq!(
            std::mem::size_of::<LightsUniform>(),
            16 + 64 + 48 + 112 * MAX_LIGHTS + 64 * shadows::MAX_LOCAL_LAYERS
        );
        // Matrices, camera/grid/background, 8 environment and 6 post vec4s, the previous
        // view-projection, the motion vec4 and the view-ray pair (common.wgsl).
        assert_eq!(
            std::mem::size_of::<ViewUniform>(),
            128 + 48 + 16 * 14 + 64 + 16 + 32
        );
        assert!(std::mem::size_of::<ViewUniform>() as u64 <= shadows::VIEW_STRIDE);
    }
}
