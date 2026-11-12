//! Light glows: halos around point / spot lights, procedural lens flares and the Glow Spread
//! halos of Rectangle lights (see [`spread`]).
//!
//! Sources come from the scene through [`Environment::glows`](super::Environment). Each render
//! [`Renderer::prepare_glows`] builds the instances (spot facing, fog response, screen-edge
//! fades) and [`Renderer::glow_passes`] draws them additively onto the HDR and glow images
//! right after the main pass (before sun shafts and bloom). Occlusion is read from the main
//! pass's depth in the vertex shader, so the main pass keeps its depth while glows are on.

mod fog;
mod instances;
pub(crate) mod spread;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod spread_tests;
#[cfg(test)]
mod tests;

use glam::Vec3;

use super::pipelines::{self, Spec};
use super::{HDR_FORMAT, RenderTarget, Renderer};
use crate::scene::{LensFlare, LightGlow, LightHalo, LightType};
use instances::GlowInstance;
use spread::SpreadInstance;
pub use spread::emitter_feed;

const SRC: &str = include_str!("glow.wgsl");

/// A light with a halo and/or a lens flare, gathered from the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct GlowSource {
    pub kind: LightType,
    pub position: Vec3,
    /// Direction the light travels.
    pub direction: Vec3,
    /// Full cone angle in degrees (spot lights).
    pub spot_angle: f32,
    /// sRGB.
    pub color: [f32; 3],
    pub intensity: f32,
    /// Light ▸ Glow (Halo brightness, spread and soft core).
    pub glow: LightGlow,
    pub halo: Option<LightHalo>,
    pub flare: Option<LensFlare>,
}

/// Pipelines and the instance buffer (made on the first render with glows).
struct GlowGpu {
    layout: wgpu::BindGroupLayout,
    halo: wgpu::RenderPipeline,
    flare: wgpu::RenderPipeline,
    spread: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    capacity: usize,
    spread_buffer: wgpu::Buffer,
    spread_capacity: usize,
}

#[derive(Default)]
pub(crate) struct Glows {
    gpu: Option<GlowGpu>,
    /// Instances of the current render: halos first, then flare elements.
    halos: u32,
    flares: u32,
    /// Glow Spread halos of this render.
    spreads: u32,
}

impl Glows {
    /// The main pass must keep its depth (glow occlusion reads it).
    pub fn keeps_depth(&self) -> bool {
        self.halos + self.flares + self.spreads > 0
    }
}

fn instance_buffer<T>(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty glow instances"),
        size: (capacity * std::mem::size_of::<T>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl GlowGpu {
    fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty glow"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        });
        let buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GlowInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x4,
                1 => Float32x4,
                2 => Float32x4
            ],
        }];
        // Additive in colour; alpha (coverage) untouched.
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let spread_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SpreadInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x4,
                1 => Float32x4,
                2 => Float32x4,
                3 => Float32x4,
                4 => Float32x4,
                5 => Float32x4
            ],
        }];
        let layouts = [view_layout, &layout];
        let build = |name, vs, fs, buffers: &[wgpu::VertexBufferLayout]| {
            pipelines::build(
                device,
                Spec {
                    vs,
                    fs: Some(fs),
                    buffers,
                    depth: None,
                    samples: 1,
                    blend: Some(add),
                    color: Some(HDR_FORMAT),
                    glow: Some(true),
                    ..Spec::new(name, SRC, &layouts)
                },
            )
        };
        let capacity = 64;
        let spread_capacity = 8;
        Self {
            halo: build("light halos", "vs_halo", "fs_halo", &buffers),
            flare: build("lens flares", "vs_flare", "fs_flare", &buffers),
            spread: build(
                "light glow spread",
                "vs_spread",
                "fs_spread",
                &spread_buffers,
            ),
            layout,
            buffer: instance_buffer::<GlowInstance>(device, capacity),
            capacity,
            spread_buffer: instance_buffer::<SpreadInstance>(device, spread_capacity),
            spread_capacity,
        }
    }
}

impl Renderer {
    /// Build and upload this render's glow instances (after the view uniform is written).
    pub(super) fn prepare_glows(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &super::Frame,
    ) {
        let g = &mut self.glows;
        g.halos = 0;
        g.flares = 0;
        g.spreads = 0;
        let spreading = frame
            .lights
            .iter()
            .any(|l| l.kind == LightType::Rectangle && l.area.glow.spread > 0.0);
        if frame.environment.glows.is_empty() && !spreading {
            return;
        }
        let gain = spread::bloom_gain(&frame.post, target.bloom.len());
        let list = instances::build(frame, target.aspect(), self.quality.lens_flares, gain);
        if list.is_empty() {
            return;
        }
        let gpu = g
            .gpu
            .get_or_insert_with(|| GlowGpu::new(device, &self.pipelines.view_layout));
        let all: Vec<GlowInstance> = list.halos.iter().chain(&list.flares).copied().collect();
        if all.len() > gpu.capacity {
            gpu.capacity = all.len().next_power_of_two();
            gpu.buffer = instance_buffer::<GlowInstance>(device, gpu.capacity);
        }
        if list.spreads.len() > gpu.spread_capacity {
            gpu.spread_capacity = list.spreads.len().next_power_of_two();
            gpu.spread_buffer = instance_buffer::<SpreadInstance>(device, gpu.spread_capacity);
        }
        if !all.is_empty() {
            queue.write_buffer(&gpu.buffer, 0, bytemuck::cast_slice(&all));
        }
        if !list.spreads.is_empty() {
            queue.write_buffer(&gpu.spread_buffer, 0, bytemuck::cast_slice(&list.spreads));
        }
        g.halos = list.halos.len() as u32;
        g.flares = list.flares.len() as u32;
        g.spreads = list.spreads.len() as u32;
    }

    /// Glow Spread halos, halos, then flares, onto the resolved HDR and glow images (after the main pass).
    pub(super) fn glow_passes(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
    ) {
        let g = &self.glows;
        let Some(gpu) = g.gpu.as_ref().filter(|_| g.keeps_depth()) else {
            return;
        };
        if !super::frame_debug::whole(
            &self.capture,
            &["Halos & Flares"],
            || super::main_target(target),
            super::frame_debug::EventTarget::Hdr,
            || super::frame_debug::FrameEvent {
                instances: g.halos + g.flares + g.spreads,
                vertices: 6,
                blend: "One One (additive)".to_owned(),
                ..super::frame_debug::events::post_event(
                    format!(
                        "Draw {} Halos, {} Flares, {} Glow Spreads",
                        g.halos, g.flares, g.spreads
                    ),
                    "glow.wgsl",
                )
            },
        ) {
            return;
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty glow group"),
            layout: &gpu.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(target.depth_view()),
            }],
        });
        let attach = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty glow pass"),
            color_attachments: &[attach(target.hdr_view()), attach(target.glow_view())],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, &group, &[]);
        if g.spreads > 0 {
            pass.set_pipeline(&gpu.spread);
            pass.set_vertex_buffer(0, gpu.spread_buffer.slice(..));
            pass.draw(0..6, 0..g.spreads);
        }
        pass.set_vertex_buffer(0, gpu.buffer.slice(..));
        if g.halos > 0 {
            pass.set_pipeline(&gpu.halo);
            pass.draw(0..6, 0..g.halos);
        }
        if g.flares > 0 {
            pass.set_pipeline(&gpu.flare);
            pass.draw(0..6, g.halos..g.halos + g.flares);
        }
    }
}
