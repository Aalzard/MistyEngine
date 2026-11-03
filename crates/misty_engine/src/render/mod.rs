//! wgpu renderer: sky, lit meshes with shadows, grid, particles, gizmo lines and selection outline.

pub mod mesh;
pub mod particles;
mod pipelines;
mod shadows;
mod target;
pub mod texture;
pub mod ui;

pub use particles::{ParticleBatch, ParticleInstance};
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
}

/// Everything needed to draw one view.
pub struct Frame<'a> {
    pub camera: &'a Camera,
    pub grid: Option<GridPlane>,
    pub skybox: bool,
    /// sRGB background colour used when the skybox is off.
    pub background: [f32; 3],
    pub lines: &'a [LineVertex],
    pub meshes: &'a [MeshDraw],
    /// Particle systems with live particles (drawn after opaque meshes).
    pub particles: &'a [ParticleBatch],
    /// Draw shaded meshes (false in the Wireframe draw mode; outlines still work).
    pub shaded: bool,
    pub lights: &'a [LightData],
    pub ambient_intensity: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    grid: [f32; 4],
    background: [f32; 4],
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
}

pub struct Renderer {
    pipelines: pipelines::Pipelines,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,
    shadows: shadows::ShadowMaps,
    lights_buffer: wgpu::Buffer,
    draws_buffer: wgpu::Buffer,
    draws_capacity: usize,
    mesh_bind_group: wgpu::BindGroup,
    draws_bind_group: wgpu::BindGroup,
    line_buffer: wgpu::Buffer,
    line_capacity: usize,
    particle_instances: particles::InstanceBuffer,
    meshes: Vec<GpuMesh>,
    /// Imported meshes by "path#index" (None = could not be loaded), uploaded on first use.
    imported: std::collections::HashMap<String, Option<GpuMesh>>,
    /// [`models::generation`] the imported meshes were uploaded at.
    imported_generation: u64,
    textures: texture::TextureCache,
    /// Linear clamp sampler for post-processing.
    post_sampler: wgpu::Sampler,
}

impl Renderer {
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

        let draws_capacity = 256;
        let draws_buffer = draws_buffer(device, draws_capacity);
        let (mesh_bind_group, draws_bind_group) =
            draw_groups(device, &pipelines, &lights_buffer, &draws_buffer, &shadows);

        let meshes = PrimitiveMesh::ALL
            .iter()
            .map(|p| GpuMesh::new(device, p.name(), mesh::primitive(*p)))
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
            lights_buffer,
            draws_buffer,
            draws_capacity,
            mesh_bind_group,
            draws_bind_group,
            line_buffer: line_buffer(device, line_capacity),
            line_capacity,
            particle_instances: particles::InstanceBuffer::new(device),
            meshes,
            imported: Default::default(),
            imported_generation: models::generation(),
            textures: texture::TextureCache::new(device),
            pipelines,
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
                    .map(|m| GpuMesh::new(device, key, &m));
                self.imported.insert(key.clone(), gpu);
            }
            if frame.shaded {
                self.textures.prepare(
                    device,
                    queue,
                    &self.pipelines.material_layout,
                    d.material.albedo_map.as_deref(),
                    d.material.normal_map.as_deref(),
                );
            }
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
        self.prepare_assets(device, queue, frame);
        let aspect = target.aspect();
        queue.write_buffer(
            &self.view_buffer,
            0,
            bytemuck::bytes_of(&view_uniform(frame, aspect)),
        );

        // Per-draw data, grown on demand.
        if frame.meshes.len() > self.draws_capacity {
            self.draws_capacity = frame.meshes.len().next_power_of_two();
            self.draws_buffer = draws_buffer(device, self.draws_capacity);
            (self.mesh_bind_group, self.draws_bind_group) = draw_groups(
                device,
                &self.pipelines,
                &self.lights_buffer,
                &self.draws_buffer,
                &self.shadows,
            );
        }
        let draws: Vec<DrawUniform> = frame.meshes.iter().map(draw_uniform).collect();
        if !draws.is_empty() {
            queue.write_buffer(&self.draws_buffer, 0, bytemuck::cast_slice(&draws));
        }

        // Lights and their shadow maps (slot 0 = directional, 1.. = local layers).
        let plan = shadows::plan(frame);
        let lights = lights_uniform(frame, &plan);
        queue.write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights));
        let slots = plan
            .directional
            .iter()
            .map(|m| (0, *m))
            .chain(plan.local.iter().enumerate().map(|(i, m)| (i + 1, *m)));
        for (slot, m) in slots {
            let mut v = ViewUniform::zeroed();
            v.view_proj = m.to_cols_array_2d();
            queue.write_buffer(
                &self.shadows.view_buffer,
                slot as u64 * shadows::VIEW_STRIDE,
                bytemuck::bytes_of(&v),
            );
        }

        if frame.lines.len() > self.line_capacity {
            self.line_capacity = frame.lines.len().next_power_of_two();
            self.line_buffer = line_buffer(device, self.line_capacity);
        }
        if !frame.lines.is_empty() {
            queue.write_buffer(&self.line_buffer, 0, bytemuck::cast_slice(frame.lines));
        }

        let particle_draws = self.upload_particles(device, queue, frame);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty frame"),
        });
        if plan.directional.is_some() {
            self.shadow_pass(&mut encoder, frame, &self.shadows.directional_view, 0);
        }
        for layer in 0..plan.local.len() {
            self.shadow_pass(
                &mut encoder,
                frame,
                &self.shadows.local_layer_views[layer],
                layer + 1,
            );
        }
        let any_outline = frame.meshes.iter().any(|m| m.outline);
        self.mask_pass(&mut encoder, target, frame);
        self.main_pass(&mut encoder, target, frame, any_outline, &particle_draws);
        self.bloom_passes(&mut encoder, target);
        queue.submit([encoder.finish()]);
    }

    /// Sort the frame's particles back to front and upload them as instances.
    fn upload_particles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> Vec<particles::ParticleDraw> {
        let cam = frame.camera;
        let (eye, forward) = (cam.position, cam.forward());
        let ortho = matches!(cam.projection, Projection::Orthographic { .. });
        let (instances, draws) = particles::order(frame.particles, |p| {
            if ortho {
                (p - eye).dot(forward)
            } else {
                p.distance_squared(eye)
            }
        });
        self.particle_instances.upload(device, queue, &instances);
        draws
    }

    fn draw_particles(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        draws: &[particles::ParticleDraw],
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
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Bright-pass → downsample chain → additive upsample chain → composite into the output image.
    fn bloom_passes(&self, encoder: &mut wgpu::CommandEncoder, target: &RenderTarget) {
        let levels = &target.bloom;
        let p = &self.pipelines;
        self.fullscreen(
            encoder,
            &levels[0].view,
            &p.bloom_prefilter,
            &target.hdr_group,
            true,
        );
        for i in 1..levels.len() {
            self.fullscreen(
                encoder,
                &levels[i].view,
                &p.bloom_down,
                &levels[i - 1].group,
                true,
            );
        }
        for i in (1..levels.len()).rev() {
            self.fullscreen(
                encoder,
                &levels[i - 1].view,
                &p.bloom_up,
                &levels[i].group,
                false,
            );
        }
        self.fullscreen(
            encoder,
            target.output_view(),
            &p.composite,
            &target.composite_group,
            true,
        );
    }

    /// Draw the meshes passing `filter`; `textured` binds each material's textures at group 2.
    fn draw_meshes(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        textured: bool,
        filter: impl Fn(&MeshDraw) -> bool,
    ) {
        for (i, d) in frame.meshes.iter().enumerate() {
            if !filter(d) {
                continue;
            }
            let Some(m) = self.gpu_mesh(&d.mesh) else {
                continue;
            };
            if textured {
                let Some(group) = self.textures.get(
                    d.material.albedo_map.as_deref(),
                    d.material.normal_map.as_deref(),
                ) else {
                    continue;
                };
                pass.set_bind_group(2, group, &[]);
            }
            pass.set_vertex_buffer(0, m.vertices.slice(..));
            pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..m.count, 0, i as u32..i as u32 + 1);
        }
    }

    fn shadow_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        target: &wgpu::TextureView,
        slot: usize,
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
        pass.set_pipeline(&self.pipelines.shadow);
        let offset = (slot as u64 * shadows::VIEW_STRIDE) as u32;
        pass.set_bind_group(0, &self.shadows.view_bind_group, &[offset]);
        pass.set_bind_group(1, &self.draws_bind_group, &[]);
        self.draw_meshes(&mut pass, frame, false, |d| d.cast_shadows);
    }

    fn mask_pass(&self, encoder: &mut wgpu::CommandEncoder, target: &RenderTarget, frame: &Frame) {
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
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipelines.mask);
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, &self.draws_bind_group, &[]);
        self.draw_meshes(&mut pass, frame, false, |d| d.outline);
    }

    fn main_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        frame: &Frame,
        outline: bool,
        particle_draws: &[particles::ParticleDraw],
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

        if frame.shaded && !frame.meshes.is_empty() {
            pass.set_pipeline(&self.pipelines.mesh);
            pass.set_bind_group(1, &self.mesh_bind_group, &[]);
            self.draw_meshes(&mut pass, frame, true, |_| true);
        }

        if frame.grid.is_some() {
            pass.set_pipeline(&self.pipelines.grid);
            pass.draw(0..3, 0..1);
        }

        // After the grid: particles don't write depth, so the grid would draw over them.
        if frame.shaded && !particle_draws.is_empty() {
            self.draw_particles(&mut pass, frame, particle_draws);
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
        }
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

fn draw_groups(
    device: &wgpu::Device,
    p: &pipelines::Pipelines,
    lights: &wgpu::Buffer,
    draws: &wgpu::Buffer,
    shadows: &shadows::ShadowMaps,
) -> (wgpu::BindGroup, wgpu::BindGroup) {
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
                resource: wgpu::BindingResource::TextureView(&shadows.directional_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&shadows.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&shadows.local_array_view),
            },
        ],
    });
    let draws_only = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty draws group"),
        layout: &p.draws_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 1,
            resource: draws.as_entire_binding(),
        }],
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

fn srgb_to_linear(c: [f32; 3]) -> [f32; 3] {
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
    let (t, o) = (d.material.tiling, d.material.offset);
    DrawUniform {
        model: d.model.to_cols_array_2d(),
        normal: d.model.inverse().transpose().to_cols_array_2d(),
        color: d.material.color,
        params: [
            d.material.metallic,
            d.material.smoothness,
            if d.receive_shadows { 1.0 } else { 0.0 },
            normal_scale,
        ],
        emission: [e[0], e[1], e[2], mirror],
        uv_st: [t[0], t[1], o[0], o[1]],
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
    u.info = [
        n as f32,
        plan.directional_texel,
        frame.ambient_intensity,
        0.0,
    ];
    u.shadow_view_proj = plan
        .directional
        .unwrap_or(Mat4::IDENTITY)
        .to_cols_array_2d();
    for (i, m) in plan.local.iter().enumerate() {
        u.local_view_proj[i] = m.to_cols_array_2d();
    }
    // Ambient matches the procedural sky gradient (sky / horizon / ground).
    let amb = |c: [f32; 3], k: f32| {
        let l = srgb_to_linear(c);
        [l[0] * k, l[1] * k, l[2] * k, 0.0]
    };
    u.sky = amb([0.40, 0.55, 0.76], 0.45);
    u.equator = amb([0.76, 0.79, 0.83], 0.28);
    u.ground = amb([0.32, 0.30, 0.29], 0.3);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_struct_sizes_match_wgsl() {
        assert_eq!(std::mem::size_of::<DrawUniform>(), 192);
        assert_eq!(std::mem::size_of::<LightItem>(), 80);
        assert_eq!(
            std::mem::size_of::<LightsUniform>(),
            16 + 64 + 48 + 80 * MAX_LIGHTS + 64 * shadows::MAX_LOCAL_LAYERS
        );
        assert!(std::mem::size_of::<ViewUniform>() as u64 <= shadows::VIEW_STRIDE);
    }
}
