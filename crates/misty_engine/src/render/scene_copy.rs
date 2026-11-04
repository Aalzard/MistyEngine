//! Copy of the opaque scene for the transparent queue: after opaque geometry the main pass is
//! split, the resolved colour is copied and the MSAA depth is resolved (sample 0) into a float
//! texture. Water refraction/shore foam and custom shaders reading `scene_color` / `scene_depth`
//! sample these.

use std::collections::HashMap;

use super::pipelines::{self, Spec};
use super::{HDR_FORMAT, RenderTarget};

/// Format of the depth copy (device depth, 0 near .. 1 far).
pub const DEPTH_COPY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

pub struct SceneCopy {
    /// Unique per copy (bind groups that use it are cached by this).
    pub id: u64,
    pub color: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
}

/// Copies per target size (the editor renders several views a frame).
pub struct SceneCopies {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    copies: HashMap<(u32, u32), SceneCopy>,
    next_id: u64,
}

impl SceneCopies {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty depth copy"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        });
        let pipeline = pipelines::build(
            device,
            Spec {
                depth: None,
                samples: 1,
                color: Some(DEPTH_COPY_FORMAT),
                glow: None,
                ..Spec::new(
                    "depth copy",
                    include_str!("shaders/depth_copy.wgsl"),
                    &[&layout],
                )
            },
        );
        Self {
            layout,
            pipeline,
            copies: HashMap::new(),
            next_id: 1,
        }
    }

    /// The copy textures for a target size (created on first use).
    pub fn get(&mut self, device: &wgpu::Device, size: (u32, u32)) -> &SceneCopy {
        if self.copies.len() > 4 && !self.copies.contains_key(&size) {
            self.copies.clear();
        }
        let id = &mut self.next_id;
        self.copies.entry(size).or_insert_with(|| {
            let make = |label, format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let sampled = wgpu::TextureUsages::TEXTURE_BINDING;
            let color = make(
                "misty scene colour copy",
                HDR_FORMAT,
                sampled | wgpu::TextureUsages::COPY_DST,
            );
            let depth = make(
                "misty scene depth copy",
                DEPTH_COPY_FORMAT,
                sampled | wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            *id += 1;
            SceneCopy {
                id: *id,
                color_view: color.create_view(&Default::default()),
                color,
                depth_view: depth.create_view(&Default::default()),
            }
        })
    }

    pub fn current(&self, size: (u32, u32)) -> Option<&SceneCopy> {
        self.copies.get(&size)
    }

    /// Copy the resolved colour and resolve the depth of `target` into its copy.
    pub fn record(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
    ) {
        let Some(copy) = self.current(target.size()) else {
            return;
        };
        encoder.copy_texture_to_texture(
            target.hdr_view().texture().as_image_copy(),
            copy.color.as_image_copy(),
            copy.color.size(),
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty depth copy"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(target.depth_view()),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty depth copy"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &copy.depth_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}
