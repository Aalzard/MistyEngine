//! Grass visibility buffer: the lit grass pass is replaced by three steps (shaders in
//! grass_vis.wgsl and grass_composite.wgsl), the way deferred / visibility-buffer renderers
//! shade dense foliage:
//!
//! 1. **Raster** (own render pass after the grass depth): the lit grass's blades and fans,
//!    same lists, order, depth test, coverage and alpha to coverage, write only which blade
//!    each MSAA sample shows (a page / index id) and which triangle of its card.
//! 2. **Resolve** (compute): per pixel, each distinct blade fragment among the samples is
//!    shaded once with the lit grass's own lighting (`grass_lit`, `shade_grass`), its
//!    attributes interpolated from the card's triangle exactly as the rasteriser does
//!    (perspective-correct barycentrics at the pixel centre, quad gradients for the atlas),
//!    the blade evaluated again at the previous wind time for the motion vector.
//! 3. **Composite** (first draw of the rest of the main pass, per sample): every sample a
//!    blade owns gets that blade's colour, coverage alpha and motion vector, as the lit pass
//!    wrote them; samples without a blade keep the terrain / trees. The grid, transparents,
//!    particles, effects and gizmos then draw and resolve as before.
//!
//! The lit pass launched ~15 M fragment lanes at 1080p to colour ~2 M visible samples (2×2
//! quads around thin leaves, one shade per covering blade per pixel under MSAA, overlapping
//! flat fans); the resolve shades each visible blade sample once (docs/pending/grass-study.md,
//! grass-vis.md). The sample count is the main pass's (Quality ▸ Anti-Aliasing).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::grass_pool::{BladePool, MAX_PAGES};
use super::pipelines::{self, Pipelines};

/// Triangle, facing and coverage (alpha to coverage reads target 0's alpha).
const CARD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The blade id as raw float bits (core WebGPU has no multisampled integer targets).
const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// Resolve output per slot (colour, coverage as four halves).
const OUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
/// Resolve output per pixel: the sample → slot table, sample 0's motion vector.
const PIXEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// Terrains with grass one frame can resolve (3 id bits).
pub(super) const MAX_TERRAINS: usize = 8;
/// Bytes between the per-terrain resolve uniforms.
const FRAME_STRIDE: u64 = 256;

/// The visibility buffer is used when it can be (the default); off, the grass draws with the
/// lit pass as before (A/B timings and look checks).
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_grass_vis(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn grass_vis_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The raster, resolve and composite pipelines (made with the first grass, at the renderer's
/// sample count).
pub(super) struct VisPipelines {
    raster: wgpu::RenderPipeline,
    classify: wgpu::ComputePipeline,
    jobs: wgpu::ComputePipeline,
    shade: wgpu::ComputePipeline,
    jobs_reset: wgpu::ComputePipeline,
    composite: wgpu::RenderPipeline,
    resolve_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    /// The resolve's per-terrain uniforms (`FRAME_STRIDE` apart).
    frame: wgpu::Buffer,
}

/// Run `f` inside a validation error scope: false when wgpu refused something it made (a
/// sample count or format this device can't do), so the grass keeps the lit pass.
fn accepted<T>(device: &wgpu::Device, f: impl FnOnce() -> T) -> Option<T> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = f();
    match block_on(scope.pop()) {
        Some(e) => {
            eprintln!("grass visibility buffer unavailable: {e}");
            None
        }
        None => Some(value),
    }
}

/// The bind group layouts the visibility pipelines use (owned, so a worker can build them).
struct VisLayouts {
    view_layout: wgpu::BindGroupLayout,
    mesh_layout: wgpu::BindGroupLayout,
    grass_layout: wgpu::BindGroupLayout,
    grass_blades_layout: wgpu::BindGroupLayout,
}

impl VisLayouts {
    fn of(p: &Pipelines) -> Self {
        Self {
            view_layout: p.view_layout.clone(),
            mesh_layout: p.mesh_layout.clone(),
            grass_layout: p.grass_layout.clone(),
            grass_blades_layout: p.grass_blades_layout.clone(),
        }
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

impl VisPipelines {
    /// None when the device refuses them (the lit pass stays).
    pub fn new(device: &wgpu::Device, p: &Pipelines) -> Option<Self> {
        let layouts = VisLayouts::of(p);
        accepted(device, || {
            Self::build(device, &layouts, super::sample_count())
        })
    }

    /// Start building them on a worker thread (streaming only; see `render::prebuilt`).
    pub(super) fn prebuild(
        device: &wgpu::Device,
        p: &Pipelines,
    ) -> super::prebuilt::Prebuilt<Option<Self>> {
        let (device, layouts) = (device.clone(), VisLayouts::of(p));
        let samples = super::sample_count();
        super::prebuilt::Prebuilt::spawn("misty grass visibility pipelines", move || {
            accepted(&device, || Self::build(&device, &layouts, samples))
        })
    }

    fn build(device: &wgpu::Device, p: &VisLayouts, samples: u32) -> Self {
        use wgpu::ShaderStages as S;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty grass visibility"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    pipelines::COMMON,
                    pipelines::GRASS_SRC,
                    include_str!("shaders/grass_vis.wgsl")
                )
                .into(),
            ),
        });
        let layouts = [
            Some(&p.view_layout),
            Some(&p.mesh_layout),
            Some(&p.grass_layout),
            Some(&p.grass_blades_layout),
        ];
        let raster_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty grass visibility"),
            bind_group_layouts: &layouts,
            immediate_size: 0,
        });
        pipelines::note_pipeline();
        let raster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("misty grass visibility"),
            layout: Some(&raster_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_grass_vis"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                ..Default::default()
            },
            // The lit grass's test: against the grass depth pass's depth, no writes (the
            // flat fans in front of it all draw, the later one showing).
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                alpha_to_coverage_enabled: samples > 1,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_grass_vis"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: CARD_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: ID_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: crate::render::pipeline_cache(device).as_ref(),
        });

        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: S::COMPUTE,
            ty,
            count: None,
        };
        let page = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let ms = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: true,
        };
        let out = |format, dim| wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: dim,
        };
        let rw = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty grass visibility resolve"),
            entries: &[
                entry(0, page),
                entry(2, page),
                entry(3, page),
                entry(4, page),
                entry(5, ms),
                entry(6, ms),
                entry(7, out(PIXEL_FORMAT, wgpu::TextureViewDimension::D2)),
                entry(8, out(OUT_FORMAT, wgpu::TextureViewDimension::D2Array)),
                entry(
                    9,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(10, rw),
                entry(11, rw),
                entry(12, out(PIXEL_FORMAT, wgpu::TextureViewDimension::D2)),
            ],
        });
        let resolve_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("misty grass visibility resolve"),
                bind_group_layouts: &[
                    Some(&p.view_layout),
                    Some(&p.mesh_layout),
                    Some(&p.grass_layout),
                    Some(&resolve_layout),
                ],
                immediate_size: 0,
            });
        let compute = |entry: &str| {
            pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&resolve_pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: crate::render::pipeline_cache(device).as_ref(),
            })
        };
        let classify = compute("cs_grass_classify");
        let jobs = compute("cs_grass_jobs");
        let shade = compute("cs_grass_shade");
        let jobs_reset = compute("cs_grass_jobs_reset");

        let read = |binding, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: S::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: dim,
                multisampled: false,
            },
            count: None,
        };
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty grass composite"),
            entries: &[
                read(0, wgpu::TextureViewDimension::D2),
                read(1, wgpu::TextureViewDimension::D2Array),
                read(2, wgpu::TextureViewDimension::D2),
            ],
        });
        // Writes colour + coverage alpha, the glow's coverage alpha and the motion vector of
        // each sample a blade owns (unblended, like the lit grass), tests nothing.
        let composite = pipelines::build(
            device,
            pipelines::Spec {
                fs: Some("fs_grass_composite"),
                glow: Some(true),
                velocity: true,
                ..pipelines::Spec::new(
                    "grass composite",
                    include_str!("shaders/grass_composite.wgsl"),
                    &[&p.view_layout, &composite_layout],
                )
            },
        );
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty grass visibility frame"),
            size: FRAME_STRIDE * MAX_TERRAINS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            raster,
            classify,
            jobs,
            shade,
            jobs_reset,
            composite,
            resolve_layout,
            composite_layout,
            frame,
        }
    }

    pub fn raster(&self) -> &wgpu::RenderPipeline {
        &self.raster
    }

    pub fn composite(&self) -> &wgpu::RenderPipeline {
        &self.composite
    }
}

/// One target's visibility and resolve images.
pub(super) struct VisTargets {
    size: (u32, u32),
    samples: u32,
    card: wgpu::TextureView,
    ids: wgpu::TextureView,
    table: wgpu::TextureView,
    vel: wgpu::TextureView,
    /// Shading jobs (one per pixel slot at most) and their count / indirect arguments.
    jobs: wgpu::Buffer,
    count: wgpu::Buffer,
    /// The shading dispatch's arguments, copied out of `count` (a buffer can't be bound
    /// for writing and used for the indirect dispatch at once).
    args: wgpu::Buffer,
    slots: wgpu::TextureView,
    /// The composite's inputs (group 1).
    pub composite_group: wgpu::BindGroup,
    /// GPU bytes of the images and buffers.
    #[cfg(test)]
    bytes: u64,
}

impl VisTargets {
    fn new(device: &wgpu::Device, pipes: &VisPipelines, size: (u32, u32), samples: u32) -> Self {
        let make = |label, format, samples, layers, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let storage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING;
        let card = make(
            "misty grass visibility card",
            CARD_FORMAT,
            samples,
            1,
            attach,
        );
        let ids = make("misty grass visibility ids", ID_FORMAT, samples, 1, attach);
        let table = make("misty grass resolve table", PIXEL_FORMAT, 1, 1, storage);
        let vel = make("misty grass resolve motion", PIXEL_FORMAT, 1, 1, storage);
        let slots = make("misty grass resolve slots", OUT_FORMAT, 1, samples, storage);
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let slots_view = slots.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let table_view = view(&table);
        let vel_view = view(&vel);
        let composite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty grass composite"),
            layout: &pipes.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&table_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&slots_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&vel_view),
                },
            ],
        });
        let pixels = size.0 as u64 * size.1 as u64;
        let jobs = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty grass shading jobs"),
            size: (pixels * samples as u64 * 4).max(16),
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let count = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty grass shading count"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let args = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty grass shading arguments"),
            size: 12,
            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            size,
            samples,
            card: view(&card),
            ids: view(&ids),
            table: table_view,
            vel: vel_view,
            jobs,
            count,
            args,
            slots: slots_view,
            composite_group,
            #[cfg(test)]
            bytes: pixels * (samples as u64 * (8 + 4 + 8 + 4) + 8),
        }
    }
}

/// A render target's visibility images (made on first use; None inside = this device can't).
#[derive(Default)]
pub(crate) struct TargetSlot(Mutex<Option<Option<VisTargets>>>);

impl TargetSlot {
    /// The images for `size` (the target's internal size) at `samples` (its main pass's),
    /// made or remade as needed; None when the device refuses them.
    pub(super) fn get(
        &self,
        device: &wgpu::Device,
        pipes: &VisPipelines,
        size: (u32, u32),
        samples: u32,
    ) -> std::sync::MutexGuard<'_, Option<Option<VisTargets>>> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let fits = match &*slot {
            Some(Some(t)) => t.size == size && t.samples == samples,
            Some(None) => true,
            None => false,
        };
        if !fits {
            *slot = Some(accepted(device, || {
                VisTargets::new(device, pipes, size, samples)
            }));
        }
        slot
    }

    /// GPU bytes of the images (a target's memory).
    #[cfg(test)]
    pub(super) fn bytes(&self) -> u64 {
        let slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        slot.as_ref()
            .and_then(|s| s.as_ref())
            .map_or(0, |t| t.bytes)
    }

    /// The composite's group for the images as they are.
    pub(super) fn composite_group(&self) -> Option<wgpu::BindGroup> {
        let slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        Some(slot.as_ref()?.as_ref()?.composite_group.clone())
    }
}

/// This frame's resolve uniforms (one per terrain dispatch): before the frame is encoded.
pub(super) fn write_frame(
    queue: &wgpu::Queue,
    pipes: &VisPipelines,
    size: (u32, u32),
    terrains: &[ResolveTerrain],
) {
    for (k, r) in terrains.iter().take(MAX_TERRAINS).enumerate() {
        let info = [r.index, 0, size.0, size.1];
        queue.write_buffer(
            &pipes.frame,
            k as u64 * FRAME_STRIDE,
            bytemuck::cast_slice(&info),
        );
    }
}

/// Begin the visibility raster: card and id targets cleared (no blade), the main pass's
/// depth tested and kept.
pub(super) fn begin_raster<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    t: &'e VisTargets,
    depth: &'e wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
    let clear = |view| {
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
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("misty grass visibility"),
        color_attachments: &[clear(&t.card), clear(&t.ids)],
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
    })
}

/// One terrain's part of the resolve: its grass group (2) and blade pages.
pub(super) struct ResolveTerrain<'a> {
    /// Id bits 24–26 (GrassParams `draw.z`).
    pub index: u32,
    pub grass_group: &'a wgpu::BindGroup,
    pub pages: &'a BladePool,
}

/// Shade every visible blade (one dispatch per terrain with grass, the first writing every
/// pixel's sample table). Groups 0 (view) and 1 (lighting) as for the lit grass.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipes: &VisPipelines,
    t: &VisTargets,
    view_group: &wgpu::BindGroup,
    lit_group: &wgpu::BindGroup,
    terrains: &[ResolveTerrain],
) {
    let (w, h) = t.size;
    let groups: Vec<wgpu::BindGroup> = (terrains.iter().take(MAX_TERRAINS).enumerate())
        .map(|(k, r)| {
            let page = |i: usize| {
                let i = if i < r.pages.pages() { i } else { 0 };
                r.pages.buffer(i as u32).as_entire_binding()
            };
            debug_assert!(r.pages.pages() <= MAX_PAGES);
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty grass visibility resolve"),
                layout: &pipes.resolve_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: page(0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: page(1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: page(2),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: page(3),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&t.card),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&t.ids),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(&t.table),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(&t.slots),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &pipes.frame,
                            offset: k as u64 * FRAME_STRIDE,
                            size: wgpu::BufferSize::new(16),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 10,
                        resource: t.jobs.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 11,
                        resource: t.count.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 12,
                        resource: wgpu::BindingResource::TextureView(&t.vel),
                    },
                ],
            })
        })
        .collect();
    for (r, group) in terrains.iter().zip(&groups) {
        let groups = [view_group, lit_group, r.grass_group, group];
        {
            let mut pass = begin_resolve(encoder, &pipes.classify, groups);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
            pass.set_pipeline(&pipes.jobs);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&t.count, 4, &t.args, 0, 12);
        let mut pass = begin_resolve(encoder, &pipes.shade, groups);
        pass.dispatch_workgroups_indirect(&t.args, 0);
        pass.set_pipeline(&pipes.jobs_reset);
        pass.dispatch_workgroups(1, 1, 1);
    }
}

/// A compute pass of the resolve with its pipeline and groups 0–3 set.
fn begin_resolve<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    groups: [&wgpu::BindGroup; 4],
) -> wgpu::ComputePass<'e> {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("misty grass visibility resolve"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    for (i, g) in groups.into_iter().enumerate() {
        pass.set_bind_group(i as u32, g, &[]);
    }
    pass
}
