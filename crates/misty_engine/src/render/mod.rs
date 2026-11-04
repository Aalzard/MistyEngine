//! wgpu renderer: sky, lit meshes with shadows, grid, particles, gizmo lines and selection outline.

mod batch;
mod cascades;
pub mod culling;
pub mod environment;
mod frame_lists;
mod gpu_timer;
pub mod mesh;
pub mod particles;
mod pipelines;
pub mod post;
pub mod probes;
mod shadows;
mod soft_shadows;
pub mod stats;
pub mod surface;
mod target;
mod terrain;
pub mod texture;
pub mod ui;
mod variants;

pub use crate::terrain::TerrainDraw;
pub use environment::{Environment, Sun};
pub use particles::{ParticleBatch, ParticleInstance};
pub use post::PostSettings;
pub use soft_shadows::LightShadow;
pub use stats::{RenderMemory, RenderOptions, RenderStats};
pub use surface::{RenderingMode, ShaderKind, SortingKey};
pub use target::RenderTarget;
use target::TargetLayouts;

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
pub const SAMPLE_COUNT: u32 = 4;
const MAX_LIGHTS: usize = 8;

/// Device features to request when the adapter has them: timestamp queries for GPU timing
/// (see [`Renderer::with_queue`]).
pub fn timestamp_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & wgpu::Features::TIMESTAMP_QUERY
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
            unlit: false,
            mode: RenderingMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            smoothness_from_albedo: false,
            metallic_map: None,
            occlusion_map: None,
            occlusion_strength: 1.0,
            emission_map: None,
        }
    }
}

impl MaterialParams {
    pub fn pipeline_key(&self) -> surface::PipelineKey {
        surface::PipelineKey {
            unlit: self.unlit,
            mode: self.mode,
            double_sided: self.double_sided,
        }
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

#[derive(Clone, Debug)]
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
}

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
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightItem {
    pos_range: [f32; 4],
    dir_type: [f32; 4],
    color: [f32; 4],
    spot: [f32; 4],
    shadow: [f32; 4],
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
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    vertex_count: u32,
    /// Local bounds (culling, shadow fitting).
    bounds: culling::Aabb,
}

pub struct Renderer {
    pipelines: pipelines::Pipelines,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,
    shadows: shadows::ShadowMaps,
    /// Reflection probe atlas, lists and BRDF LUT (mesh group bindings 20–24).
    probes: probes::gpu::ProbeGpu,
    /// Group 2 of the terrain pass (no bindings).
    empty_group: wgpu::BindGroup,
    lights_buffer: wgpu::Buffer,
    draws_buffer: wgpu::Buffer,
    draws_capacity: usize,
    /// Instance → draw slot indices (see [`batch`]).
    slots_buffer: wgpu::Buffer,
    slots_capacity: usize,
    mesh_bind_group: wgpu::BindGroup,
    draws_bind_group: wgpu::BindGroup,
    line_buffer: wgpu::Buffer,
    line_capacity: usize,
    particle_instances: particles::InstanceBuffer,
    terrains: terrain::TerrainCache,
    meshes: Vec<GpuMesh>,
    /// Imported meshes by "path#index" (None = could not be loaded), uploaded on first use.
    imported: std::collections::HashMap<String, Option<GpuMesh>>,
    /// [`models::generation`] the imported meshes were uploaded at.
    imported_generation: u64,
    textures: texture::TextureCache,
    /// Linear clamp sampler for post-processing.
    post_sampler: wgpu::Sampler,
    options: RenderOptions,
    stats: RenderStats,
    /// GPU time per render (when the device supports timestamp queries).
    gpu_timer: Option<gpu_timer::GpuTimer>,
    // Per-frame scratch kept to avoid reallocating.
    batcher: batch::Batcher,
    slots_scratch: Vec<u32>,
    draws_scratch: Vec<DrawUniform>,
}

impl Renderer {
    /// Renderer without GPU timing (see [`Renderer::with_queue`]).
    pub fn new(device: &wgpu::Device) -> Self {
        let pipelines = pipelines::Pipelines::new(device);
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
        let view_group = |buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty view group"),
                layout: &pipelines.view_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let view_bind_group = view_group(&view_buffer);
        let shadows = shadows::ShadowMaps::new(device, &pipelines.shadow_view_layout);
        let probes = probes::gpu::ProbeGpu::new(device);

        let draws_capacity = 256;
        let draws_buffer = draws_buffer(device, draws_capacity);
        let slots_capacity = 1024;
        let slots_buffer = slots_buffer(device, slots_capacity);
        let (mesh_bind_group, draws_bind_group) = draw_groups(
            device,
            &pipelines,
            &lights_buffer,
            [&draws_buffer, &slots_buffer],
            &shadows,
            &probes,
        );

        let meshes = PrimitiveMesh::ALL
            .iter()
            .map(|p| {
                let mut m = GpuMesh::new(device, p.name(), mesh::primitive(*p));
                // The same box picking and shadow fitting use.
                m.bounds = culling::Aabb::new(-p.half_extents(), p.half_extents());
                m
            })
            .collect();

        let line_capacity = 1024;
        let post_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty post sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            post_sampler,
            view_buffer,
            view_bind_group,
            shadows,
            probes,
            empty_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty empty group"),
                layout: &pipelines.empty_layout,
                entries: &[],
            }),
            lights_buffer,
            draws_buffer,
            draws_capacity,
            slots_buffer,
            slots_capacity,
            mesh_bind_group,
            draws_bind_group,
            line_buffer: line_buffer(device, line_capacity),
            line_capacity,
            particle_instances: particles::InstanceBuffer::new(device),
            terrains: terrain::TerrainCache::new(device),
            meshes,
            imported: Default::default(),
            imported_generation: models::generation(),
            textures: texture::TextureCache::new(device),
            pipelines,
            options: RenderOptions::default(),
            stats: RenderStats::default(),
            gpu_timer: None,
            batcher: Default::default(),
            slots_scratch: Vec::new(),
            draws_scratch: Vec::new(),
        }
    }

    /// Renderer that also measures GPU time per render when the device was created with
    /// `TIMESTAMP_QUERY` (see [`timestamp_features`]).
    pub fn with_queue(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            gpu_timer: gpu_timer::GpuTimer::new(device, queue),
            ..Self::new(device)
        }
    }

    /// Culling / instancing switches.
    pub fn options(&self) -> RenderOptions {
        self.options
    }

    pub fn set_options(&mut self, options: RenderOptions) {
        self.options = options;
    }

    /// Counters of the last [`Renderer::render`] call.
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Latest GPU time of a render, when timestamp queries are supported.
    pub fn gpu_ms(&self) -> Option<f32> {
        self.gpu_timer.as_ref().and_then(|t| t.last_ms())
    }

    /// GPU resources held (Profiler Memory module).
    pub fn memory(&self) -> RenderMemory {
        let imported = self.imported.values().flatten();
        let meshes = self.meshes.iter().chain(imported.clone());
        let mesh_bytes: u64 = meshes.map(GpuMesh::bytes).sum();
        let (terrains, terrain_bytes) = self.terrains.memory();
        let (textures, texture_bytes) = self.textures.memory();
        let buffers = [
            &self.draws_buffer,
            &self.slots_buffer,
            &self.line_buffer,
            &self.view_buffer,
            &self.lights_buffer,
        ];
        let shadow_bytes = 4
            * (shadows::DIRECTIONAL_SIZE.pow(2) as u64 * cascades::MAX_CASCADES as u64
                + shadows::LOCAL_SIZE.pow(2) as u64 * shadows::MAX_LOCAL_LAYERS as u64);
        RenderMemory {
            meshes: self.meshes.len() + imported.count() + terrains,
            textures,
            buffer_bytes: mesh_bytes
                + terrain_bytes
                + buffers.iter().map(|b| b.size()).sum::<u64>()
                + self.particle_instances.buffer.size(),
            texture_bytes: texture_bytes + shadow_bytes + self.probes.bytes(),
        }
    }

    /// Reload textures and models whose files changed on disk (the editor calls this in Edit mode).
    pub fn reload_changed_assets(&mut self) {
        self.textures.poll_changes();
        models::poll_changes();
    }

    /// Texture load errors since the last call.
    pub fn take_warnings(&mut self) -> Vec<String> {
        self.textures.take_warnings()
    }

    /// Upload imported meshes and build texture bind groups used by this frame.
    fn prepare_assets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        let generation = models::generation();
        if generation != self.imported_generation {
            self.imported.clear();
            self.imported_generation = generation;
        }
        for d in frame.meshes {
            if let MeshRef::Asset(key) = &d.mesh
                && !self.imported.contains_key(key)
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
            if frame.shaded {
                let set = d.material.textures();
                let layout = &self.pipelines.material_layout;
                self.textures.prepare_set(device, queue, layout, &set);
                self.pipelines
                    .ensure_mesh(device, d.material.pipeline_key());
            }
            if d.cast_shadows
                && let Some(key) = d.material.shadow_key()
            {
                self.pipelines.ensure_shadow(device, key);
                if key.cutout {
                    let set = d.material.textures();
                    let layout = &self.pipelines.material_layout;
                    self.textures.prepare_set(device, queue, layout, &set);
                }
            }
        }
        if !frame.terrains.is_empty() {
            let opaque_caster = surface::ShadowKey {
                cutout: false,
                double_sided: false,
            };
            self.pipelines.ensure_shadow(device, opaque_caster);
            self.terrains.prepare(
                device,
                queue,
                &self.pipelines.terrain_layout,
                &mut self.textures,
                frame.terrains,
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
        let layouts = TargetLayouts {
            mask: &self.pipelines.mask_layout,
            post: &self.pipelines.post_layout,
            composite: &self.pipelines.composite_layout,
            sampler: &self.post_sampler,
        };
        RenderTarget::new(device, width, height, &layouts)
    }

    fn gpu_mesh(&self, r: &MeshRef) -> Option<&GpuMesh> {
        match r {
            MeshRef::Primitive(p) => {
                Some(&self.meshes[PrimitiveMesh::ALL.iter().position(|m| m == p).unwrap_or(0)])
            }
            MeshRef::Asset(key) => self.imported.get(key)?.as_ref(),
        }
    }

    /// Draw a frame into `target` and submit it.
    pub fn render(
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
        {
            crate::profile_scope!("Render.PrepareAssets");
            self.prepare_assets(device, queue, frame);
        }
        let aspect = target.aspect();
        queue.write_buffer(
            &self.view_buffer,
            0,
            bytemuck::bytes_of(&view_uniform(frame, aspect)),
        );

        // Culling and batching for the camera and every shadow map.
        let shadow_settings = crate::quality::shadows();
        let (lists, plan, probes_rebuilt) = {
            crate::profile_scope!("Render.Culling");
            let meshes = self.mesh_bounds(frame);
            let terrains = self.terrain_bounds(frame);
            // Reflection probes: atlas uploads and each draw's probe list.
            let rebuilt = self
                .probes
                .prepare(device, queue, frame, &meshes, &terrains);
            let casters = Self::caster_bounds(frame, &meshes, &terrains);
            let plan = shadows::plan_with(frame, &casters, aspect, &shadow_settings);
            let view_proj = frame.camera.view_projection(aspect);
            let lists =
                self.frame_lists(frame, &view_proj, &plan, (&meshes, &terrains), &mut stats);
            (lists, plan, rebuilt)
        };
        self.upload_draws(device, queue, frame, &lists.slots, probes_rebuilt);

        // Lights and their shadow maps (cascades first, then local layers).
        let lights = lights_uniform(frame, &plan);
        queue.write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights));
        self.write_shadow_uniforms(queue, frame, &plan, &shadow_settings, target.size());

        // Overlay triangles follow the lines in the same buffer.
        let line_vertices = frame.lines.len() + frame.overlay.len();
        if line_vertices > self.line_capacity {
            self.line_capacity = line_vertices.next_power_of_two();
            self.line_buffer = line_buffer(device, self.line_capacity);
        }
        if !frame.lines.is_empty() {
            queue.write_buffer(&self.line_buffer, 0, bytemuck::cast_slice(frame.lines));
        }
        if !frame.overlay.is_empty() {
            let offset = std::mem::size_of_val(frame.lines) as u64;
            queue.write_buffer(
                &self.line_buffer,
                offset,
                bytemuck::cast_slice(frame.overlay),
            );
        }

        let particle_draws = self.upload_particles(device, queue, frame, &lists.particles);
        let timing = self.gpu_timer.as_mut().and_then(|t| t.begin());

        crate::profile_scope!("Render.Encode");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty frame"),
        });
        let any_outline = frame.meshes.iter().any(|m| m.outline);
        // First pass: starts the GPU timing.
        self.mask_pass(&mut encoder, target, frame, timing);
        self.shadow_passes(&mut encoder, frame, &plan, &lists.shadows, &mut stats);
        self.main_pass(
            &mut encoder,
            target,
            frame,
            any_outline,
            &lists,
            &particle_draws,
            &mut stats,
        );
        self.bloom_passes(&mut encoder, target, timing);
        if let (Some(timer), Some(slot)) = (self.gpu_timer.as_ref(), timing) {
            timer.resolve(&mut encoder, slot);
        }
        queue.submit([encoder.finish()]);
        if let (Some(timer), Some(slot)) = (self.gpu_timer.as_ref(), timing) {
            timer.submitted(slot);
        }
        self.slots_scratch = lists.slots;
        stats.cpu_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.stats = stats;
    }

    /// Write the per-draw values and the slot lists, growing the buffers when needed.
    fn upload_draws(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        slots: &[u32],
        rebuild: bool,
    ) {
        let draw_count = frame.meshes.len() + frame.terrains.len();
        let grow_draws = draw_count > self.draws_capacity;
        let grow_slots = slots.len() > self.slots_capacity;
        if grow_draws {
            self.draws_capacity = draw_count.next_power_of_two();
            self.draws_buffer = draws_buffer(device, self.draws_capacity);
        }
        if grow_slots {
            self.slots_capacity = slots.len().next_power_of_two();
            self.slots_buffer = slots_buffer(device, self.slots_capacity);
        }
        if grow_draws || grow_slots || rebuild {
            (self.mesh_bind_group, self.draws_bind_group) = draw_groups(
                device,
                &self.pipelines,
                &self.lights_buffer,
                [&self.draws_buffer, &self.slots_buffer],
                &self.shadows,
                &self.probes,
            );
        }
        let mut draws = std::mem::take(&mut self.draws_scratch);
        draws.clear();
        draws.extend(frame.meshes.iter().map(draw_uniform));
        draws.extend(frame.terrains.iter().map(terrain::draw_uniform));
        if !draws.is_empty() {
            queue.write_buffer(&self.draws_buffer, 0, bytemuck::cast_slice(&draws));
        }
        if !slots.is_empty() {
            queue.write_buffer(&self.slots_buffer, 0, bytemuck::cast_slice(slots));
        }
        self.draws_scratch = draws;
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
        timing: Option<usize>,
    ) {
        let levels = &target.bloom;
        let p = &self.pipelines;
        self.fullscreen(
            encoder,
            &levels[0].view,
            &p.bloom_prefilter,
            &target.hdr_group,
            true,
            None,
        );
        for i in 1..levels.len() {
            self.fullscreen(
                encoder,
                &levels[i].view,
                &p.bloom_down,
                &levels[i - 1].group,
                true,
                None,
            );
        }
        for i in (1..levels.len()).rev() {
            self.fullscreen(
                encoder,
                &levels[i - 1].view,
                &p.bloom_up,
                &levels[i].group,
                false,
                None,
            );
        }
        self.fullscreen(
            encoder,
            target.output_view(),
            &p.composite,
            &target.composite_group,
            true,
            self.timestamps(timing, false, true),
        );
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

    /// Draw main-pass batches with their material's pipeline and textures.
    fn draw_batches(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        batches: &[batch::Batch],
        stats: &mut RenderStats,
    ) {
        let (mut pipeline_at, mut group_at) = (0, 0);
        for b in batches {
            let d = &frame.meshes[b.draw];
            let (Some(pipeline), Some(group), Some(m)) = (
                self.pipelines.mesh_variant(d.material.pipeline_key()),
                self.textures.get_set(&d.material.textures()),
                self.gpu_mesh(&d.mesh),
            ) else {
                continue;
            };
            if batch::addr(pipeline) != pipeline_at {
                pipeline_at = batch::addr(pipeline);
                pass.set_pipeline(pipeline);
                stats.set_pass += 1;
            }
            if batch::addr(group) != group_at {
                group_at = batch::addr(group);
                pass.set_bind_group(2, group, &[]);
                stats.texture_binds += 1;
            }
            m.draw(pass, b.first..b.first + b.count);
            stats.draw(b.count, m.count, m.vertex_count);
        }
    }

    fn shadow_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        target: &wgpu::TextureView,
        slot: usize,
        list: Option<&frame_lists::ShadowList>,
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
        let mut pipeline_at = 0;
        for b in &list.batches {
            let d = &frame.meshes[b.draw];
            let Some(key) = d.material.shadow_key() else {
                continue;
            };
            let (Some(pipeline), Some(m)) =
                (self.pipelines.shadow_variant(key), self.gpu_mesh(&d.mesh))
            else {
                continue;
            };
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
            m.draw(&mut pass, b.first..b.first + b.count);
            stats.draw(b.count, m.count, m.vertex_count);
        }
        let first = frame.meshes.len();
        let terrain_caster = surface::ShadowKey {
            cutout: false,
            double_sided: false,
        };
        let Some(pipeline) = self.pipelines.shadow_variant(terrain_caster) else {
            return;
        };
        pass.set_pipeline(pipeline);
        self.terrains.draw(
            &mut pass,
            frame.terrains,
            first,
            false,
            |i| list.terrains[i],
            stats,
        );
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
        for (i, d) in frame.meshes.iter().enumerate() {
            if d.outline {
                self.draw_mesh(&mut pass, i, d);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn main_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        frame: &Frame,
        outline: bool,
        lists: &frame_lists::FrameLists,
        particle_draws: &[particles::ParticleDraw],
        stats: &mut RenderStats,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty main pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: target.msaa_view(),
                    depth_slice: None,
                    resolve_target: Some(target.hdr_view()),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                }),
                // Emission only; drives bloom.
                Some(wgpu::RenderPassColorAttachment {
                    view: target.glow_msaa_view(),
                    depth_slice: None,
                    resolve_target: Some(target.glow_view()),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.view_bind_group, &[]);

        pass.set_pipeline(&self.pipelines.sky);
        pass.draw(0..3, 0..1);

        // Opaque and Cutout meshes, front to back.
        if frame.shaded && !lists.opaque.is_empty() {
            pass.set_bind_group(1, &self.mesh_bind_group, &[]);
            self.draw_batches(&mut pass, frame, &lists.opaque, stats);
        }

        if frame.shaded && !frame.terrains.is_empty() {
            pass.set_pipeline(&self.pipelines.terrain);
            pass.set_bind_group(1, &self.mesh_bind_group, &[]);
            pass.set_bind_group(2, &self.empty_group, &[]);
            let first = frame.meshes.len();
            self.terrains.draw(
                &mut pass,
                frame.terrains,
                first,
                true,
                |i| lists.terrains[i],
                stats,
            );
        }

        if frame.grid.is_some() {
            pass.set_pipeline(&self.pipelines.grid);
            pass.draw(0..3, 0..1);
        }

        // Fade / Transparent meshes back to front, after the grid (they don't write depth).
        if frame.shaded && !lists.transparent.is_empty() {
            pass.set_bind_group(1, &self.mesh_bind_group, &[]);
            self.draw_batches(&mut pass, frame, &lists.transparent, stats);
        }

        // After the grid: particles don't write depth, so the grid would draw over them.
        if frame.shaded && !particle_draws.is_empty() {
            self.draw_particles(&mut pass, frame, particle_draws, stats);
        }

        if !frame.overlay.is_empty() {
            let first = frame.lines.len() as u32;
            pass.set_pipeline(&self.pipelines.overlay);
            pass.set_vertex_buffer(0, self.line_buffer.slice(..));
            pass.draw(first..first + frame.overlay.len() as u32, 0..1);
        }

        if !frame.lines.is_empty() {
            pass.set_pipeline(&self.pipelines.lines);
            pass.set_vertex_buffer(0, self.line_buffer.slice(..));
            pass.draw(0..frame.lines.len() as u32, 0..1);
        }

        if outline {
            pass.set_pipeline(&self.pipelines.outline);
            pass.set_bind_group(1, &target.mask_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

impl GpuMesh {
    fn new(device: &wgpu::Device, label: &str, data: &mesh::MeshData) -> Self {
        let (lo, hi) = data.bounds();
        Self {
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
            count: data.indices.len() as u32,
            vertex_count: data.vertices.len() as u32,
            bounds: culling::Aabb::new(lo, hi),
        }
    }

    /// Draw `instances` (entries of the slots buffer).
    fn draw(&self, pass: &mut wgpu::RenderPass, instances: std::ops::Range<u32>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, instances);
    }

    fn bytes(&self) -> u64 {
        self.vertices.size() + self.indices.size()
    }
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

/// Group 1 of the lit mesh pass and of the shadow/mask passes; `buffers` = draws, slots.
fn draw_groups(
    device: &wgpu::Device,
    p: &pipelines::Pipelines,
    lights: &wgpu::Buffer,
    buffers: [&wgpu::Buffer; 2],
    shadows: &shadows::ShadowMaps,
    probes: &probes::gpu::ProbeGpu,
) -> (wgpu::BindGroup, wgpu::BindGroup) {
    let [draws, slots] = buffers;
    let [p0, p1, p2, p3, p4] = probes.entries();
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
            p0,
            p1,
            p2,
            p3,
            p4,
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

fn draw_uniform(d: &MeshDraw) -> DrawUniform {
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
    }
}

fn lights_uniform(frame: &Frame, plan: &shadows::ShadowPlan) -> LightsUniform {
    let mut u = LightsUniform::zeroed();
    let mut n = 0;
    for (i, l) in frame.lights.iter().enumerate() {
        if n == MAX_LIGHTS || l.intensity <= 0.0 {
            continue;
        }
        let c = srgb_to_linear(l.color);
        let kind = match l.kind {
            LightType::Directional => 0.0,
            LightType::Point => 1.0,
            LightType::Spot => 2.0,
        };
        let half = (l.spot_angle * 0.5).to_radians();
        let (mode, layer) = match plan.light_layers.get(i).copied().flatten() {
            Some(layer) => (
                match l.shadows {
                    Shadows::None => 0.0,
                    Shadows::Hard => 1.0,
                    Shadows::Soft => 2.0,
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
            spot: [half.cos(), (half * 0.8).cos(), 0.0, 0.0],
            shadow: [mode, layer, 0.0, 0.0],
        };
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

fn view_uniform(frame: &Frame, aspect: f32) -> ViewUniform {
    let cam = frame.camera;
    let vp = cam.view_projection(aspect);
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
        environment: frame.environment.pack(cam.far),
        post: frame.post.pack(),
    }
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_struct_sizes_match_wgsl() {
        assert_eq!(std::mem::size_of::<DrawUniform>(), 208);
        assert_eq!(std::mem::size_of::<LightItem>(), 80);
        assert_eq!(
            std::mem::size_of::<LightsUniform>(),
            16 + 64 + 48 + 80 * MAX_LIGHTS + 64 * shadows::MAX_LOCAL_LAYERS
        );
        // Matrices, camera/grid/background, 8 environment and 6 post vec4s (common.wgsl).
        assert_eq!(std::mem::size_of::<ViewUniform>(), 128 + 48 + 16 * 14);
        assert!(std::mem::size_of::<ViewUniform>() as u64 <= shadows::VIEW_STRIDE);
    }
}
