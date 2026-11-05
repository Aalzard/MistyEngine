//! Replay display: after a frame stopped at an event, copy that event's render target into the
//! output image (tonemapped HDR, an 8-bit copy, or a shadow map's depth).

use super::EventTarget;
use crate::render::{COLOR_FORMAT, RenderTarget, Renderer};

pub(crate) struct DebugBlit {
    tonemap: wgpu::RenderPipeline,
    copy: wgpu::RenderPipeline,
    depth: wgpu::RenderPipeline,
    depth_layout: wgpu::BindGroupLayout,
}

fn pipeline(
    device: &wgpu::Device,
    name: &str,
    source: &str,
    fs: &str,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(name),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(name),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl DebugBlit {
    fn new(device: &wgpu::Device, post_layout: &wgpu::BindGroupLayout) -> Self {
        let color = include_str!("debug_blit.wgsl");
        let depth_src = include_str!("debug_depth.wgsl");
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty frame debug depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        Self {
            tonemap: pipeline(
                device,
                "frame debug tonemap",
                color,
                "fs_tonemap",
                post_layout,
            ),
            copy: pipeline(device, "frame debug copy", color, "fs_copy", post_layout),
            depth: pipeline(
                device,
                "frame debug depth",
                depth_src,
                "fs_depth",
                &depth_layout,
            ),
            depth_layout,
        }
    }
}

impl Renderer {
    /// Show `display` (the stopped event's image) in `target`'s output.
    pub(in crate::render) fn frame_debug_blit(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        display: EventTarget,
    ) {
        if display == EventTarget::Output {
            return;
        }
        let post_layout = &self.pipelines.post_layout;
        let blit = self
            .debug_blit
            .get_or_insert_with(|| DebugBlit::new(device, post_layout));
        let sample_group = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty frame debug source"),
                layout: post_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.post_sampler),
                    },
                ],
            })
        };
        let owned;
        let (pipeline, group): (Option<&wgpu::RenderPipeline>, Option<&wgpu::BindGroup>) =
            match display {
                EventTarget::Hdr => {
                    owned = sample_group(target.hdr_view());
                    (Some(&blit.tonemap), Some(&owned))
                }
                EventTarget::Bloom(i) => match target.bloom.get(i as usize) {
                    Some(level) => (Some(&blit.tonemap), Some(&level.group)),
                    None => (None, None),
                },
                EventTarget::PreFinish => (Some(&blit.copy), Some(&target.pre_finish_group)),
                EventTarget::Shadow { local, layer } => {
                    let views = match local {
                        true => &self.shadows.local_layer_views,
                        false => &self.shadows.cascade_views,
                    };
                    match views.get(layer as usize) {
                        Some(view) => {
                            owned = device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("misty frame debug shadow"),
                                layout: &blit.depth_layout,
                                entries: &[wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(view),
                                }],
                            });
                            (Some(&blit.depth), Some(&owned))
                        }
                        None => (None, None),
                    }
                }
                EventTarget::None | EventTarget::Output => (None, None),
            };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty frame debug display"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.output_view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.02,
                        g: 0.02,
                        b: 0.025,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let (Some(pipeline), Some(group)) = (pipeline, group) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
