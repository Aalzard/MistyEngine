//! Visibility buffer of Misty Virtual Geometry: the main pass's clusters are drawn in two steps
//! instead of one lit draw.
//!
//! 1. **Raster** (vgeo_vis.wgsl): the visible clusters with the main pass's MSAA depth write
//!    only ids (cluster and triangle, object and batch) into two multisampled targets.
//! 2. **Resolve** (vgeo_resolve.wgsl): back in the main pass, a full-screen triangle per batch
//!    (and per layer with the edge layer) rebuilds one triangle's surface per pixel from the
//!    pool and lights it with the Standard shading, written to the samples it owns.
//!
//! Pixel-sized triangles are what virtual geometry draws, and the lit draw shades them in 2×2
//! quads per triangle per covered sample group: several times the work of one shade per pixel.
//! The id format follows the grass visibility buffer (grass-study.md B4): raw bits in R32Float
//! (core WebGPU can't multisample integer targets), bit 30 always set.
//!
//! Cutout batches rasterize with their albedo alpha test (their own pipeline, group 2 = their
//! texture set), so holes never reach the resolve.
//!
//! Needs `PRIMITIVE_INDEX` (the triangle of an indexed sub-draw) and the indexed path; without
//! them, or when the device refuses the pipelines, clusters draw lit as before.

use super::{CLUSTER_VERTICES, CLUSTERS, LOCAL, Pool, VERTICES};
use crate::render::pipelines::{self, Depth, MESH_SRC, Pipelines, Spec};

const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
const RASTER_SRC: &str = include_str!("vgeo_vis.wgsl");
const RESOLVE_SRC: &str = include_str!("vgeo_resolve.wgsl");
/// Batches the id format can name.
pub(super) const MAX_BATCHES: usize = 512;
/// Target sizes kept (Scene and Game views, thumbnails).
const KEEP_TARGETS: usize = 3;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ResolveUniform {
    offsets: [u32; 4],
    info: [u32; 4],
}

/// The id targets of one render target size.
struct Targets {
    size: (u32, u32),
    ids: wgpu::TextureView,
    draws: wgpu::TextureView,
    used: u64,
}

pub(super) struct Vis {
    raster: wgpu::RenderPipeline,
    raster_cutout: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    resolve_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    targets: Vec<Targets>,
    frame: u64,
}

/// This frame's targets and resolve bind group.
pub(super) struct VisFrame {
    target: usize,
    group: wgpu::BindGroup,
    /// Resolve draws per batch (2 with the edge layer).
    layers: u32,
}

/// Run `f` inside a validation error scope: `None` when wgpu refused something it made.
fn accepted<T>(device: &wgpu::Device, f: impl FnOnce() -> T) -> Option<T> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = f();
    match block_on(scope.pop()) {
        Some(e) => {
            eprintln!("virtual geometry visibility buffer unavailable: {e}");
            None
        }
        None => Some(value),
    }
}

/// Native error scopes resolve immediately; poll with a no-op waker.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

impl Vis {
    /// The pipelines, when this device can run them.
    pub fn new(
        device: &wgpu::Device,
        p: &Pipelines,
        render_layout: &wgpu::BindGroupLayout,
        indexed: bool,
    ) -> Option<Self> {
        if !indexed || !device.features().contains(wgpu::Features::PRIMITIVE_INDEX) {
            return None;
        }
        accepted(device, || Self::build(device, p, render_layout))
    }

    fn build(device: &wgpu::Device, p: &Pipelines, render_layout: &wgpu::BindGroupLayout) -> Self {
        use wgpu::ShaderStages as S;
        let samples = crate::render::sample_count();
        // `enable` must open the module, before the common code.
        let source = format!(
            "enable primitive_index;\n{}\n{}\n{}\n{RASTER_SRC}",
            pipelines::COMMON,
            crate::render::screen_space::PREPASS_SRC,
            super::FETCH_SRC,
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty vgeo visibility"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let id_target = Some(wgpu::ColorTargetState {
            format: ID_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        });
        let raster_pipeline = |group2: &wgpu::BindGroupLayout, vs: &str, fs: &str| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("misty vgeo visibility"),
                bind_group_layouts: &[
                    Some(&p.view_layout),
                    Some(&p.draws_layout),
                    Some(group2),
                    Some(render_layout),
                ],
                immediate_size: 0,
            });
            pipelines::note_pipeline();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("misty vgeo visibility"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: crate::render::DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[id_target.clone(), id_target.clone()],
                }),
                multiview_mask: None,
                cache: crate::render::pipeline_cache(device).as_ref(),
            })
        };
        let raster = raster_pipeline(&p.empty_layout, "vs_cluster_vis", "fs_cluster_vis");
        let raster_cutout = raster_pipeline(
            &p.material_layout,
            "vs_cluster_vis_cutout",
            "fs_cluster_vis_cutout",
        );

        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let ms = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: true,
        };
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty vgeo resolve"),
            entries: &[
                entry(
                    0,
                    S::FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, S::FRAGMENT, ms),
                entry(2, S::FRAGMENT, ms),
                entry(
                    3,
                    S::FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let resolve_src = format!("{MESH_SRC}\n{RESOLVE_SRC}");
        let layouts = [
            &p.view_layout,
            &p.mesh_layout,
            &p.material_layout,
            &resolve_layout,
        ];
        let resolve = pipelines::build(
            device,
            Spec {
                vs: "vs_cluster_resolve",
                fs: Some("fs_cluster_resolve"),
                cull: None,
                // Inside the main pass: its depth (already holding the clusters) is untouched.
                depth: Depth::main(false, wgpu::CompareFunction::Always),
                glow: Some(true),
                velocity: true,
                ..Spec::new("vgeo resolve", &resolve_src, &layouts)
            },
        );
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty vgeo resolve"),
            size: std::mem::size_of::<ResolveUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            raster,
            raster_cutout,
            resolve,
            resolve_layout,
            uniform,
            targets: Vec::new(),
            frame: 0,
        }
    }

    /// This frame's id targets (made for `size` on first use) and resolve bind group.
    pub fn begin_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pool: &Pool,
        size: (u32, u32),
        edge_layer: bool,
    ) -> VisFrame {
        self.frame += 1;
        let samples = crate::render::sample_count();
        let target = match self.targets.iter().position(|t| t.size == size) {
            Some(i) => i,
            None => {
                if self.targets.len() >= KEEP_TARGETS {
                    let oldest = (0..self.targets.len())
                        .min_by_key(|&i| self.targets[i].used)
                        .unwrap_or(0);
                    self.targets.swap_remove(oldest);
                }
                let make = |label| {
                    device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d {
                                width: size.0.max(1),
                                height: size.1.max(1),
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: samples,
                            dimension: wgpu::TextureDimension::D2,
                            format: ID_FORMAT,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                };
                self.targets.push(Targets {
                    size,
                    ids: make("misty vgeo visibility ids"),
                    draws: make("misty vgeo visibility draws"),
                    used: 0,
                });
                self.targets.len() - 1
            }
        };
        self.targets[target].used = self.frame;
        let w = pool.word_offsets();
        let u = ResolveUniform {
            offsets: [w[VERTICES], w[LOCAL], w[CLUSTERS], w[CLUSTER_VERTICES]],
            info: [samples, edge_layer as u32, 0, 0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        let t = &self.targets[target];
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vgeo resolve"),
            layout: &self.resolve_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pool.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&t.ids),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&t.draws),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        VisFrame {
            target,
            group,
            layers: 1 + edge_layer as u32,
        }
    }

    /// Begin the raster: ids cleared (no virtual geometry), the main pass's depth kept.
    pub fn begin_raster<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        frame: &VisFrame,
        depth: &wgpu::TextureView,
    ) -> wgpu::RenderPass<'e> {
        let t = &self.targets[frame.target];
        let color = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty vgeo visibility"),
            color_attachments: &[color(&t.ids), color(&t.draws)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.raster);
        pass
    }

    /// The raster pipeline of an opaque or a cutout batch.
    pub fn set_raster(&self, pass: &mut wgpu::RenderPass, cutout: bool) {
        pass.set_pipeline(if cutout {
            &self.raster_cutout
        } else {
            &self.raster
        });
    }

    /// The resolve of `batches` batches (groups 0 and 1 bound by the main pass).
    pub fn resolve<'a>(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &VisFrame,
        batches: &[usize],
        cutout: &[bool],
        material: &dyn Fn(usize) -> Option<&'a wgpu::BindGroup>,
        keep: &dyn Fn(usize, usize, bool) -> bool,
    ) {
        pass.set_pipeline(&self.resolve);
        pass.set_bind_group(3, &frame.group, &[]);
        for (i, &draw) in batches.iter().enumerate() {
            let c = cutout.get(i).copied().unwrap_or(false);
            if !keep(i, draw, c) {
                continue;
            }
            let Some(group) = material(draw) else {
                continue;
            };
            pass.set_bind_group(2, group, &[]);
            // Instance = (batch × 2 + layer) × 2 + cutout (vgeo_resolve.wgsl).
            let c = c as u32;
            for layer in 0..frame.layers {
                let id = (2 * i as u32 + layer) * 2 + c;
                pass.draw(0..3, id..id + 1);
            }
        }
    }
}
