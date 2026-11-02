//! wgpu renderer: sky, infinite grid and gizmo lines into an offscreen target.

mod pipelines;
mod target;

pub use target::RenderTarget;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Projection};

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SAMPLE_COUNT: u32 = 4;

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

/// Everything needed to draw one view.
pub struct Frame<'a> {
    pub camera: &'a Camera,
    pub grid: Option<GridPlane>,
    pub skybox: bool,
    /// sRGB background colour used when the skybox is off.
    pub background: [f32; 3],
    pub lines: &'a [LineVertex],
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

pub struct Renderer {
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,
    pipelines: pipelines::Pipelines,
    line_buffer: wgpu::Buffer,
    line_capacity: usize,
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let view_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty view uniform"),
            contents: bytemuck::bytes_of(&ViewUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let pipelines = pipelines::Pipelines::new(device);
        let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty view bind group"),
            layout: &pipelines.view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });
        let line_capacity = 1024;
        Self {
            view_buffer,
            view_bind_group,
            pipelines,
            line_buffer: line_buffer(device, line_capacity),
            line_capacity,
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
        queue.write_buffer(
            &self.view_buffer,
            0,
            bytemuck::bytes_of(&view_uniform(frame, target.aspect())),
        );

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
        {
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

            if frame.grid.is_some() {
                pass.set_pipeline(&self.pipelines.grid);
                pass.draw(0..3, 0..1);
            }

            if !frame.lines.is_empty() {
                pass.set_pipeline(&self.pipelines.lines);
                pass.set_vertex_buffer(0, self.line_buffer.slice(..));
                pass.draw(0..frame.lines.len() as u32, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

fn line_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty lines"),
        size: (capacity * std::mem::size_of::<LineVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
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
    let bg = frame.background.map(|c| c.powf(2.2));
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
