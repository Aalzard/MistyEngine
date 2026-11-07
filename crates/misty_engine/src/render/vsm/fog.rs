//! The volumetric fog and sun shafts read the sun's shadow from cascade-shaped maps
//! (volumetrics/shadow_access.rs). With the virtual shadow map on, the cascades are not drawn:
//! each layer is filled from the virtual map instead (vsm_fog.wgsl), a texel lookup per
//! cascade texel, so the fog keeps working unchanged at a fraction of a cascade's cost.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use super::clipmap;
use crate::render::SHADOW_FORMAT;
use crate::render::cascades::{Cascade, MAX_CASCADES};

const FOG_SRC: &str = include_str!("shaders/vsm_fog.wgsl");

/// Side of each fog cascade layer in VSM mode (texels).
pub const FOG_SIZE: u32 = 1024;

/// `FogLayer` in vsm_fog.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct FogLayer {
    rect: [f32; 4],
    depth: [f32; 4],
}

pub(in crate::render) struct FogResolve {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    layers: wgpu::Buffer,
}

impl FogResolve {
    pub fn new(device: &wgpu::Device, p: &crate::render::pipelines::Pipelines) -> Self {
        use wgpu::ShaderStages as S;
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty vsm fog"),
            entries: &[
                entry(
                    0,
                    S::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    S::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(2, S::FRAGMENT, uniform),
                entry(3, S::FRAGMENT, uniform),
            ],
        });
        let source = format!("{}\n{FOG_SRC}", super::COMMON_SRC);
        use crate::render::pipelines::{Depth, Spec, build};
        let pipeline = build(
            device,
            Spec {
                vs: "vs_vsm_fog",
                fs: Some("fs_vsm_fog"),
                depth: Some(Depth {
                    format: SHADOW_FORMAT,
                    write: true,
                    compare: wgpu::CompareFunction::Always,
                    bias: Default::default(),
                }),
                color: None,
                glow: None,
                samples: 1,
                ..Spec::new("vsm fog cascades", &source, &[&p.empty_layout, &layout])
            },
        );
        let layers = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vsm fog layers"),
            size: (std::mem::size_of::<FogLayer>() * MAX_CASCADES) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            layers,
        }
    }

    /// Fill the fog's cascade layers (`views`) from the virtual shadow map.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        gpu: &super::VsmGpu,
        cascades: &[Cascade],
        sun_dir: Vec3,
        camera: Vec3,
        views: &[wgpu::TextureView],
    ) {
        let light = clipmap::light_view(sun_dir);
        let cam = light.transform_point3(camera);
        let mut layers = [FogLayer::default(); MAX_CASCADES];
        for (layer, c) in layers.iter_mut().zip(cascades) {
            // The cascade's box in light space: its projection's inverse.
            let proj_inv = (c.view_proj * light.inverse()).inverse();
            let lo = proj_inv.project_point3(Vec3::new(-1.0, -1.0, 0.0));
            let hi = proj_inv.project_point3(Vec3::new(1.0, 1.0, 1.0));
            let side = hi.x - lo.x;
            layer.rect = [lo.x - cam.x, -(hi.y - cam.y), side, FOG_SIZE as f32];
            layer.depth = [cam.z - lo.z, 1.0 / (hi.z - lo.z).max(1e-3), c.texel, 0.0];
        }
        queue.write_buffer(&self.layers, 0, bytemuck::cast_slice(&layers));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vsm fog"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&gpu.table_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&gpu.pool_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: gpu.mark_params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.layers.as_entire_binding(),
                },
            ],
        });
        for (i, view) in views.iter().enumerate().take(cascades.len()) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty vsm fog cascade"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view,
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..3, i as u32..i as u32 + 1);
        }
    }
}
