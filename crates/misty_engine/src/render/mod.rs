//! wgpu renderer: sky, lit meshes with shadows, grid, gizmo lines and selection outline.

pub mod mesh;
mod pipelines;
mod target;

pub use target::RenderTarget;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Projection};
use crate::scene::{LightType, PrimitiveMesh};

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
pub const SAMPLE_COUNT: u32 = 4;
const SHADOW_SIZE: u32 = 2048;
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialParams {
    /// sRGB base colour + alpha.
    pub color: [f32; 4],
    pub metallic: f32,
    pub smoothness: f32,
    /// sRGB emission colour (already multiplied by intensity).
    pub emission: [f32; 3],
}

impl Default for MaterialParams {
    fn default() -> Self {
        Self {
            color: [1.0; 4],
            metallic: 0.0,
            smoothness: 0.5,
            emission: [0.0; 3],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MeshDraw {
    pub mesh: PrimitiveMesh,
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
    pub shadows: bool,
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
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightItem {
    pos_range: [f32; 4],
    dir_type: [f32; 4],
    color: [f32; 4],
    spot: [f32; 4],
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
    shadow_view_buffer: wgpu::Buffer,
    shadow_view_bind_group: wgpu::BindGroup,
    lights_buffer: wgpu::Buffer,
    draws_buffer: wgpu::Buffer,
    draws_capacity: usize,
    mesh_bind_group: wgpu::BindGroup,
    draws_bind_group: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    line_buffer: wgpu::Buffer,
    line_capacity: usize,
    meshes: Vec<GpuMesh>,
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
        let shadow_view_buffer = uniform("misty shadow view", std::mem::size_of::<ViewUniform>());
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
        let shadow_view_bind_group = view_group(&shadow_view_buffer);

        let shadow_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("misty shadow map"),
                size: wgpu::Extent3d {
                    width: SHADOW_SIZE,
                    height: SHADOW_SIZE,
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
            .create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let draws_capacity = 256;
        let draws_buffer = draws_buffer(device, draws_capacity);
        let (mesh_bind_group, draws_bind_group) = draw_groups(
            device,
            &pipelines,
            &lights_buffer,
            &draws_buffer,
            &shadow_view,
            &shadow_sampler,
        );

        let meshes = PrimitiveMesh::ALL
            .iter()
            .map(|p| {
                let data = mesh::primitive(*p);
                GpuMesh {
                    vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(p.name()),
                        contents: bytemuck::cast_slice(&data.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(p.name()),
                        contents: bytemuck::cast_slice(&data.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    count: data.indices.len() as u32,
                }
            })
            .collect();

        let line_capacity = 1024;
        Self {
            view_buffer,
            view_bind_group,
            shadow_view_buffer,
            shadow_view_bind_group,
            lights_buffer,
            draws_buffer,
            draws_capacity,
            mesh_bind_group,
            draws_bind_group,
            shadow_view,
            shadow_sampler,
            line_buffer: line_buffer(device, line_capacity),
            line_capacity,
            meshes,
            pipelines,
        }
    }

    /// New offscreen target for a view.
    pub fn create_target(&self, device: &wgpu::Device, width: u32, height: u32) -> RenderTarget {
        RenderTarget::new(device, width, height, &self.pipelines.mask_layout)
    }

    fn gpu_mesh(&self, p: PrimitiveMesh) -> &GpuMesh {
        &self.meshes[PrimitiveMesh::ALL.iter().position(|m| *m == p).unwrap_or(0)]
    }

    /// Draw a frame into `target` and submit it.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
    ) {
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
                &self.shadow_view,
                &self.shadow_sampler,
            );
        }
        let draws: Vec<DrawUniform> = frame.meshes.iter().map(draw_uniform).collect();
        if !draws.is_empty() {
            queue.write_buffer(&self.draws_buffer, 0, bytemuck::cast_slice(&draws));
        }

        // Lights and the directional shadow caster.
        let shadow_vp = shadow_matrix(frame);
        let lights = lights_uniform(frame, shadow_vp);
        queue.write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights));
        if let Some(vp) = shadow_vp {
            let mut v = ViewUniform::zeroed();
            v.view_proj = vp.to_cols_array_2d();
            queue.write_buffer(&self.shadow_view_buffer, 0, bytemuck::bytes_of(&v));
        }

        if frame.lines.len() > self.line_capacity {
            self.line_capacity = frame.lines.len().next_power_of_two();
            self.line_buffer = line_buffer(device, self.line_capacity);
        }
        if !frame.lines.is_empty() {
            queue.write_buffer(&self.line_buffer, 0, bytemuck::cast_slice(frame.lines));
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty frame"),
        });
        if shadow_vp.is_some() {
            self.shadow_pass(&mut encoder, frame);
        }
        let any_outline = frame.meshes.iter().any(|m| m.outline);
        self.mask_pass(&mut encoder, target, frame);
        self.main_pass(&mut encoder, target, frame, any_outline);
        queue.submit([encoder.finish()]);
    }

    fn draw_meshes(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        filter: impl Fn(&MeshDraw) -> bool,
    ) {
        for (i, d) in frame.meshes.iter().enumerate() {
            if !filter(d) {
                continue;
            }
            let m = self.gpu_mesh(d.mesh);
            pass.set_vertex_buffer(0, m.vertices.slice(..));
            pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..m.count, 0, i as u32..i as u32 + 1);
        }
    }

    fn shadow_pass(&self, encoder: &mut wgpu::CommandEncoder, frame: &Frame) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty shadow pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow_view,
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
        pass.set_bind_group(0, &self.shadow_view_bind_group, &[]);
        pass.set_bind_group(1, &self.draws_bind_group, &[]);
        self.draw_meshes(&mut pass, frame, |d| d.cast_shadows);
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
        self.draw_meshes(&mut pass, frame, |d| d.outline);
    }

    fn main_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        frame: &Frame,
        outline: bool,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty main pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.msaa_view(),
                depth_slice: None,
                resolve_target: Some(target.resolve_view()),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Discard,
                },
            })],
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
            self.draw_meshes(&mut pass, frame, |_| true);
        }

        if frame.grid.is_some() {
            pass.set_pipeline(&self.pipelines.grid);
            pass.draw(0..3, 0..1);
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
    shadow: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
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
                resource: wgpu::BindingResource::TextureView(shadow),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
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
    let e = srgb_to_linear(d.material.emission);
    DrawUniform {
        model: d.model.to_cols_array_2d(),
        normal: d.model.inverse().transpose().to_cols_array_2d(),
        color: d.material.color,
        params: [
            d.material.metallic,
            d.material.smoothness,
            if d.receive_shadows { 1.0 } else { 0.0 },
            0.0,
        ],
        emission: [e[0], e[1], e[2], 0.0],
    }
}

/// Orthographic light matrix around all shadow casters, for the first shadowed directional light.
fn shadow_matrix(frame: &Frame) -> Option<Mat4> {
    let light = frame
        .lights
        .iter()
        .find(|l| l.kind == LightType::Directional && l.shadows && l.intensity > 0.0)?;
    let casters: Vec<&MeshDraw> = frame.meshes.iter().filter(|m| m.cast_shadows).collect();
    if casters.is_empty() || !frame.shaded {
        return None;
    }
    // Bounding sphere of the casters' world bounds.
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for m in &casters {
        let half = m.mesh.half_extents();
        for corner in 0..8 {
            let s = Vec3::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            let p = m.model.transform_point3(half * s);
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let center = (lo + hi) * 0.5;
    let radius = ((hi - lo).length() * 0.5).max(1.0);
    let dir = light.direction.normalize_or(Vec3::NEG_Y);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let eye = center - dir * radius * 2.0;
    let view = glam::camera::lh::view::look_to_mat4(eye, dir, up);
    let proj = glam::camera::lh::proj::directx::orthographic(
        -radius,
        radius,
        -radius,
        radius,
        0.0,
        radius * 4.0,
    );
    Some(proj * view)
}

fn lights_uniform(frame: &Frame, shadow_vp: Option<Mat4>) -> LightsUniform {
    let mut u = LightsUniform::zeroed();
    let mut shadow_taken = false;
    let mut n = 0;
    for l in frame.lights.iter().take(MAX_LIGHTS) {
        if l.intensity <= 0.0 {
            continue;
        }
        let c = srgb_to_linear(l.color);
        let kind = match l.kind {
            LightType::Directional => 0.0,
            LightType::Point => 1.0,
            LightType::Spot => 2.0,
        };
        // Only the first shadowed directional light owns the shadow map.
        let casts =
            l.kind == LightType::Directional && l.shadows && !shadow_taken && shadow_vp.is_some();
        shadow_taken |= casts;
        let half = (l.spot_angle * 0.5).to_radians();
        u.items[n] = LightItem {
            pos_range: l.position.extend(l.range).to_array(),
            dir_type: l.direction.normalize_or(Vec3::Z).extend(kind).to_array(),
            color: [
                c[0] * l.intensity,
                c[1] * l.intensity,
                c[2] * l.intensity,
                casts as u8 as f32,
            ],
            spot: [half.cos(), (half * 0.8).cos(), 0.0, 0.0],
        };
        n += 1;
    }
    u.info = [
        n as f32,
        shadow_vp.is_some() as u8 as f32,
        frame.ambient_intensity,
        0.0,
    ];
    u.shadow_view_proj = shadow_vp.unwrap_or(Mat4::IDENTITY).to_cols_array_2d();
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
        assert_eq!(std::mem::size_of::<DrawUniform>(), 176);
        assert_eq!(std::mem::size_of::<LightItem>(), 64);
        assert_eq!(
            std::mem::size_of::<LightsUniform>(),
            16 + 64 + 48 + 64 * MAX_LIGHTS
        );
    }
}
