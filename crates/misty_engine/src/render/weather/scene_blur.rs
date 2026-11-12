//! A small blurred mip chain of the opaque scene copy for the rain (render/weather).
//!
//! Level 0 is half the internal resolution, each further level half the one before
//! ([`LEVELS`] in all). A drop is a lens: thick drops near the camera show the scene behind
//! them bent and out of focus (the low levels), thin streaks show the average light around
//! them (the top levels: bright against dark trees, invisible against the sky). Sampling
//! these small textures is cheaper than the full-size scene copy the streaks read before.

use std::collections::HashMap;

use super::super::HDR_FORMAT;
use super::super::pipelines::{self, Spec};

/// Mip levels of the chain (1/2 .. 1/64 of the internal resolution).
pub const LEVELS: u32 = 6;

const SRC: &str = include_str!("shaders/scene_blur.wgsl");

pub struct Chain {
    /// Every level (what the rain samples).
    pub view: wgpu::TextureView,
    levels: Vec<wgpu::TextureView>,
}

pub struct SceneBlur {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// Trilinear, clamped.
    pub sampler: wgpu::Sampler,
    chains: HashMap<(u32, u32), Chain>,
}

impl SceneBlur {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty weather scene blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layouts = [view_layout, &layout];
        let pipeline = pipelines::build(
            device,
            Spec {
                fs: Some("fs_blur"),
                depth: None,
                color: Some(HDR_FORMAT),
                samples: 1,
                glow: None,
                ..Spec::new("weather scene blur", SRC, &layouts)
            },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty weather scene blur"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            pipeline,
            sampler,
            chains: HashMap::new(),
        }
    }

    /// The chain for an internal target size (made by [`Self::record`]).
    pub fn current(&self, size: (u32, u32)) -> Option<&Chain> {
        self.chains.get(&size)
    }

    /// Fill the chain for `size` from `scene` (the opaque scene colour copy).
    pub fn record(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view_group: &wgpu::BindGroup,
        scene: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        if self.chains.len() > 4 && !self.chains.contains_key(&size) {
            self.chains.clear();
        }
        let chain = self.chains.entry(size).or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("misty weather scene blur"),
                size: wgpu::Extent3d {
                    width: (size.0 / 2).max(1 << (LEVELS - 1)),
                    height: (size.1 / 2).max(1 << (LEVELS - 1)),
                    depth_or_array_layers: 1,
                },
                mip_level_count: LEVELS,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            Chain {
                view: texture.create_view(&Default::default()),
                levels: (0..LEVELS)
                    .map(|l| {
                        texture.create_view(&wgpu::TextureViewDescriptor {
                            base_mip_level: l,
                            mip_level_count: Some(1),
                            ..Default::default()
                        })
                    })
                    .collect(),
            }
        });
        for (i, level) in chain.levels.iter().enumerate() {
            let source = if i == 0 { scene } else { &chain.levels[i - 1] };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty weather scene blur"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty weather scene blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: level,
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, view_group, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
