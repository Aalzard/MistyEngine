//! The Post-process Volume's camera effects, like Unity HDRP's overrides of the same names:
//! **Auto Exposure** (eye adaptation from a luminance histogram), **Depth of Field**
//! (Gaussian, or a physically based Bokeh lens) and **Motion Blur** (per object and camera,
//! from the motion vectors). All are off by default; while they are off nothing here runs and
//! the composite multiplies by an exposure of exactly 1, so images are what they were.
//!
//! They run after the temporal resolve (TAA or the upscaler) at the output resolution, in
//! HDRP's order — exposure measurement, depth of field, motion blur — before bloom and the
//! composite:
//! - the resolved image is converged and jitter-free, so the blurs neither flicker nor need
//!   their own temporal pass, and with the upscaler they work on the sharp output-resolution
//!   image (depth and motion vectors are read at the internal resolution by uv);
//! - Depth of Field and Motion Blur write into the view's chain images (one per effect that
//!   runs), and the composite reads the last one;
//! - Auto Exposure writes a multiplier into the target's exposure buffer, which the composite
//!   applies to the scene and bloom before grading (render/shaders/composite.wgsl).
//!
//! Depth of Field details are in shaders/dof.wgsl, Motion Blur in shaders/motion_blur.wgsl,
//! Auto Exposure in shaders/exposure.wgsl; the uniform and CPU copies of the math in
//! [`params`].

use std::collections::HashMap;
use std::time::Instant;

use glam::Mat4;
use wgpu::util::DeviceExt;

use super::frame_debug::{self, EventTarget};
use super::pipelines::{self, Spec};
use super::{Frame, HDR_FORMAT, RenderTarget, Renderer};
use crate::scene::DofMode;

pub(crate) mod params;

const PARAMS_SRC: &str = include_str!("shaders/postfx_params.wgsl");
pub(super) const DOF_SRC: &str = concat!(
    include_str!("shaders/postfx_params.wgsl"),
    "\n",
    include_str!("shaders/postfx_common.wgsl"),
    "\n",
    include_str!("shaders/dof.wgsl")
);
pub(super) const MB_SRC: &str = concat!(
    include_str!("shaders/postfx_params.wgsl"),
    "\n",
    include_str!("shaders/postfx_common.wgsl"),
    "\n",
    include_str!("shaders/motion_blur.wgsl")
);
const AE_SRC: &str = include_str!("shaders/exposure.wgsl");
/// Near / far CoC tiles and motion blur tiles.
const TILE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// Views not rendered for this many renders lose their images.
const STALE_AFTER: u64 = 600;
/// Bytes of the histogram (128 bins).
const BINS_BYTES: u64 = 128 * 4;
/// The exposure buffer while Auto Exposure is off: multiplier 1, not adapted.
pub(crate) const EXPOSURE_OFF: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

struct Pipes {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// 1×1 transparent black (the near field of Gaussian depth of field).
    black: wgpu::TextureView,
    dof_prefilter: wgpu::RenderPipeline,
    dof_tiles: wgpu::RenderPipeline,
    dof_dilate: wgpu::RenderPipeline,
    dof_gather: wgpu::RenderPipeline,
    dof_blur_h: wgpu::RenderPipeline,
    dof_blur_v: wgpu::RenderPipeline,
    dof_composite: wgpu::RenderPipeline,
    mb_tiles: wgpu::RenderPipeline,
    mb_neighbours: wgpu::RenderPipeline,
    mb_gather: wgpu::RenderPipeline,
    ae_layout: wgpu::BindGroupLayout,
    ae_histogram: wgpu::ComputePipeline,
    ae_average: wgpu::ComputePipeline,
}

struct Image {
    view: wgpu::TextureView,
    size: (u32, u32),
    format: wgpu::TextureFormat,
}

impl Image {
    fn new(
        device: &wgpu::Device,
        label: &str,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Self {
        let size = (size.0.max(1), size.1.max(1));
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self { view, size, format }
    }

    fn bytes(&self) -> u64 {
        let texel = if self.format == TILE_FORMAT { 4 } else { 8 };
        texel * self.size.0 as u64 * self.size.1 as u64
    }
}

/// Half-resolution images of Depth of Field.
struct DofImages {
    prefilter: Image,
    temp: Image,
    far: Image,
    near: Image,
    tiles: Image,
    dilated: Image,
}

impl DofImages {
    fn new(device: &wgpu::Device, out: (u32, u32)) -> Self {
        let half = (out.0.div_ceil(2), out.1.div_ceil(2));
        let tiles = (
            half.0.div_ceil(params::DOF_TILE),
            half.1.div_ceil(params::DOF_TILE),
        );
        let hdr = |label| Image::new(device, label, half, HDR_FORMAT);
        Self {
            prefilter: hdr("misty dof prefilter"),
            temp: hdr("misty dof blur"),
            far: hdr("misty dof far"),
            near: hdr("misty dof near"),
            tiles: Image::new(device, "misty dof tiles", tiles, TILE_FORMAT),
            dilated: Image::new(device, "misty dof tiles dilated", tiles, TILE_FORMAT),
        }
    }

    fn bytes(&self) -> u64 {
        [
            &self.prefilter,
            &self.temp,
            &self.far,
            &self.near,
            &self.tiles,
            &self.dilated,
        ]
        .iter()
        .map(|i| i.bytes())
        .sum()
    }
}

/// Per-view state.
struct TargetFx {
    out: (u32, u32),
    params: wgpu::Buffer,
    /// Output-resolution HDR images the blurs write (the first, then the second).
    chain: [Option<Image>; 2],
    dof: Option<DofImages>,
    /// Motion blur tiles and their neighbourhood maximum.
    mb: Option<[Image; 2]>,
    /// Auto Exposure histogram.
    bins: Option<wgpu::Buffer>,
    last: Option<Instant>,
    used: u64,
    /// The target's exposure buffer holds an adapted value (reset when Auto Exposure stops).
    exposure_on: bool,
}

/// What this render runs.
#[derive(Clone, Copy, Debug)]
struct FxFrame {
    target: u64,
    dof: Option<DofMode>,
    motion_blur: bool,
    exposure: bool,
}

/// The renderer's part (pipelines are built the first time an effect turns on).
#[derive(Default)]
pub(super) struct PostFx {
    pipes: Option<Pipes>,
    targets: HashMap<u64, TargetFx>,
    renders: u64,
    frame: Option<FxFrame>,
}

impl PostFx {
    /// Set up this render's effects (uniform, images); nothing when they are all off.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_layout: &wgpu::BindGroupLayout,
        target: &RenderTarget,
        frame: &Frame,
        prev_view_proj: Mat4,
        continuous: bool,
    ) {
        self.renders += 1;
        self.frame = None;
        let renders = self.renders;
        self.targets.retain(|_, t| renders - t.used < STALE_AFTER);
        let post = &frame.post;
        let dof = (frame.shaded && post.depth_of_field.active).then_some(&post.depth_of_field);
        let motion_blur = (frame.shaded
            && post.motion_blur.active
            && post.motion_blur.intensity > 0.0
            && post.motion_blur.maximum_velocity > 0.0)
            .then_some(&post.motion_blur);
        let exposure = (frame.shaded && post.auto_exposure.active).then_some(&post.auto_exposure);
        // Auto Exposure stopped: back to the fixed exposure.
        if exposure.is_none()
            && let Some(t) = self.targets.get_mut(&target.id)
            && t.exposure_on
        {
            queue.write_buffer(&target.exposure, 0, bytemuck::bytes_of(&EXPOSURE_OFF));
            t.exposure_on = false;
        }
        if dof.is_none() && motion_blur.is_none() && exposure.is_none() {
            return;
        }
        if self.pipes.is_none() {
            self.pipes = Some(Pipes::new(device, queue, view_layout));
        }
        let out = target.output_size();
        let state = self
            .targets
            .entry(target.id)
            .or_insert_with(|| TargetFx::new(device, out));
        if state.out != out {
            *state = TargetFx::new(device, out);
        }
        state.used = renders;
        let now = Instant::now();
        let dt = state
            .last
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.25));
        state.last = Some(now);
        let blurs = dof.is_some() as usize + motion_blur.is_some() as usize;
        for (i, slot) in state.chain.iter_mut().enumerate() {
            if i < blurs && slot.is_none() {
                *slot = Some(Image::new(device, "misty post chain", out, HDR_FORMAT));
            }
        }
        if dof.is_some() && state.dof.is_none() {
            state.dof = Some(DofImages::new(device, out));
        }
        if motion_blur.is_some() && state.mb.is_none() {
            let tiles = (
                out.0.div_ceil(params::MB_TILE),
                out.1.div_ceil(params::MB_TILE),
            );
            state.mb = Some([
                Image::new(device, "misty motion blur tiles", tiles, TILE_FORMAT),
                Image::new(device, "misty motion blur neighbours", tiles, TILE_FORMAT),
            ]);
        }
        if exposure.is_some() && state.bins.is_none() {
            state.bins = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty exposure histogram"),
                size: BINS_BYTES,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let reset = !state.exposure_on || !continuous;
        if exposure.is_some() && !state.exposure_on {
            queue.write_buffer(&target.exposure, 0, bytemuck::bytes_of(&EXPOSURE_OFF));
        }
        state.exposure_on = exposure.is_some();
        let p = params::pack(&params::ParamInput {
            camera: frame.camera,
            aspect: target.aspect(),
            prev_view_proj,
            out_size: out,
            internal_size: target.size(),
            dof,
            motion_blur,
            exposure,
            dt,
            reset,
        });
        queue.write_buffer(&state.params, 0, bytemuck::bytes_of(&p));
        self.frame = Some(FxFrame {
            target: target.id,
            dof: dof.map(|d| d.mode),
            motion_blur: motion_blur.is_some(),
            exposure: exposure.is_some(),
        });
    }

    /// This render's depth of field or motion blur reads the main pass's depth (it must be
    /// stored).
    pub(super) fn keeps_depth(&self) -> bool {
        self.frame.is_some_and(|f| f.dof.is_some() || f.motion_blur)
    }

    /// GPU memory of the views' images and buffers (Profiler Memory module).
    pub(super) fn bytes(&self) -> u64 {
        self.targets
            .values()
            .map(|t| {
                let chain: u64 = t.chain.iter().flatten().map(Image::bytes).sum();
                let dof = t.dof.as_ref().map_or(0, DofImages::bytes);
                let mb: u64 = t.mb.iter().flatten().map(Image::bytes).sum();
                chain + dof + mb + t.bins.as_ref().map_or(0, |_| BINS_BYTES)
            })
            .sum()
    }
}

impl TargetFx {
    fn new(device: &wgpu::Device, out: (u32, u32)) -> Self {
        Self {
            out,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty post effects"),
                size: std::mem::size_of::<params::FxParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            chain: [None, None],
            dof: None,
            mb: None,
            bins: None,
            last: None,
            used: 0,
            exposure_on: false,
        }
    }
}

impl Pipes {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        use wgpu::ShaderStages as S;
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let float = |multisampled: bool| wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float {
                filterable: !multisampled,
            },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty post effects"),
            entries: &[
                entry(0, S::FRAGMENT, float(false)),
                entry(
                    1,
                    S::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    2,
                    S::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                ),
                entry(3, S::FRAGMENT, float(true)),
                entry(4, S::FRAGMENT, uniform),
                entry(5, S::FRAGMENT, float(false)),
                entry(6, S::FRAGMENT, float(false)),
            ],
        });
        let layouts = [view_layout, &layout];
        let pass = |name, source, fs, format, second: bool| {
            pipelines::build(
                device,
                Spec {
                    fs: Some(fs),
                    depth: None,
                    samples: 1,
                    color: Some(format),
                    // The second HDR target (the near field of the Bokeh gather).
                    glow: second.then_some(true),
                    ..Spec::new(name, source, &layouts)
                },
            )
        };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let ae_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty auto exposure"),
            entries: &[
                entry(0, S::COMPUTE, float(false)),
                entry(1, S::COMPUTE, uniform),
                entry(2, S::COMPUTE, storage(false)),
                entry(3, S::COMPUTE, storage(false)),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("auto exposure"),
            source: wgpu::ShaderSource::Wgsl(format!("{PARAMS_SRC}\n{AE_SRC}").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty auto exposure"),
            bind_group_layouts: &[Some(&ae_layout)],
            immediate_size: 0,
        });
        let compute = |entry| {
            pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: super::pipeline_cache(device).as_ref(),
            })
        };
        let black = device
            .create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("misty post black"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[0, 0, 0, 0],
            )
            .create_view(&Default::default());
        Self {
            dof_prefilter: pass(
                "dof prefilter",
                DOF_SRC,
                "fs_dof_prefilter",
                HDR_FORMAT,
                false,
            ),
            dof_tiles: pass("dof tiles", DOF_SRC, "fs_dof_tiles", TILE_FORMAT, false),
            dof_dilate: pass("dof dilate", DOF_SRC, "fs_dof_dilate", TILE_FORMAT, false),
            dof_gather: pass("dof gather", DOF_SRC, "fs_dof_gather", HDR_FORMAT, true),
            dof_blur_h: pass("dof blur", DOF_SRC, "fs_dof_blur_h", HDR_FORMAT, false),
            dof_blur_v: pass("dof blur", DOF_SRC, "fs_dof_blur_v", HDR_FORMAT, false),
            dof_composite: pass(
                "dof composite",
                DOF_SRC,
                "fs_dof_composite",
                HDR_FORMAT,
                false,
            ),
            mb_tiles: pass(
                "motion blur tiles",
                MB_SRC,
                "fs_mb_tiles",
                TILE_FORMAT,
                false,
            ),
            mb_neighbours: pass(
                "motion blur neighbours",
                MB_SRC,
                "fs_mb_neighbours",
                TILE_FORMAT,
                false,
            ),
            mb_gather: pass("motion blur", MB_SRC, "fs_mb_gather", HDR_FORMAT, false),
            ae_histogram: compute("cs_histogram"),
            ae_average: compute("cs_average"),
            ae_layout,
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty post effects"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            black,
        }
    }

    /// Group 1 of a pass: `src` and the aux images (black when not given).
    fn group(
        &self,
        device: &wgpu::Device,
        target: &RenderTarget,
        params: &wgpu::Buffer,
        src: &wgpu::TextureView,
        aux: [Option<&wgpu::TextureView>; 2],
    ) -> wgpu::BindGroup {
        let tex = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty post effects"),
            layout: &self.layout,
            entries: &[
                tex(0, src),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                tex(2, target.depth_view()),
                tex(3, target.velocity_view()),
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: params.as_entire_binding(),
                },
                tex(5, aux[0].unwrap_or(&self.black)),
                tex(6, aux[1].unwrap_or(&self.black)),
            ],
        })
    }
}

/// One fullscreen pass into `outputs` (cleared; the first is the colour target).
fn draw(
    encoder: &mut wgpu::CommandEncoder,
    outputs: &[&wgpu::TextureView],
    pipeline: &wgpu::RenderPipeline,
    view_group: &wgpu::BindGroup,
    group: &wgpu::BindGroup,
) {
    let attachments: Vec<_> = outputs
        .iter()
        .map(|view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })
        })
        .collect();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("misty post effects"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, view_group, &[]);
    pass.set_bind_group(1, group, &[]);
    pass.draw(0..3, 0..1);
}

impl Renderer {
    /// This render's Auto Exposure, Depth of Field and Motion Blur (after the temporal
    /// resolve). Returns the composite group reading the last blur's image, or `None` when
    /// the composite reads the resolved image as usual.
    pub(super) fn postfx_passes(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
    ) -> Option<wgpu::BindGroup> {
        let fx = &self.postfx;
        let f = fx.frame.filter(|f| f.target == target.id)?;
        let (pipes, state) = (fx.pipes.as_ref()?, fx.targets.get(&target.id)?);
        let view_group = &self.view_bind_group;
        let post = |name: &str, shader: &'static str| {
            let name = name.to_owned();
            move || frame_debug::events::post_event(name, shader)
        };
        let (w, h) = target.output_size();
        let hdr = |name: &str| frame_debug::target(name, (w, h), "Rgba16Float");
        if f.exposure
            && let Some(bins) = &state.bins
            && frame_debug::whole(
                &self.capture,
                &["Post", "Auto Exposure"],
                frame_debug::TargetInfo::default,
                EventTarget::None,
                post("Auto Exposure (histogram, adaptation)", "exposure.wgsl"),
            )
        {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty auto exposure"),
                layout: &pipes.ae_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(target.post_hdr_view()),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: state.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: bins.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: target.exposure.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("misty auto exposure"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &group, &[]);
            pass.set_pipeline(&pipes.ae_histogram);
            pass.dispatch_workgroups(w.div_ceil(32), h.div_ceil(32), 1);
            pass.set_pipeline(&pipes.ae_average);
            pass.dispatch_workgroups(1, 1, 1);
        }
        self.mark(encoder, "Auto exposure");
        let mut src = target.post_hdr_view();
        let mut written = None;
        if let (Some(mode), Some(images), Some(out)) = (f.dof, &state.dof, &state.chain[0])
            && frame_debug::whole(
                &self.capture,
                &["Post", "Depth of Field"],
                || hdr("Depth of Field"),
                EventTarget::None,
                post("Depth of Field", "dof.wgsl"),
            )
        {
            let group = |src, aux| pipes.group(device, target, &state.params, src, aux);
            let i = images;
            draw(
                encoder,
                &[&i.prefilter.view],
                &pipes.dof_prefilter,
                view_group,
                &group(src, [None, None]),
            );
            let near = match mode {
                DofMode::Bokeh => {
                    let tiles = group(&i.prefilter.view, [None, None]);
                    draw(
                        encoder,
                        &[&i.tiles.view],
                        &pipes.dof_tiles,
                        view_group,
                        &tiles,
                    );
                    let dilate = group(&i.tiles.view, [None, None]);
                    draw(
                        encoder,
                        &[&i.dilated.view],
                        &pipes.dof_dilate,
                        view_group,
                        &dilate,
                    );
                    let gather = group(&i.prefilter.view, [Some(&i.dilated.view), None]);
                    let fields = [&i.far.view, &i.near.view];
                    draw(encoder, &fields, &pipes.dof_gather, view_group, &gather);
                    Some(&i.near.view)
                }
                DofMode::Gaussian => {
                    let h = group(&i.prefilter.view, [None, None]);
                    draw(encoder, &[&i.temp.view], &pipes.dof_blur_h, view_group, &h);
                    let v = group(&i.temp.view, [None, None]);
                    draw(encoder, &[&i.far.view], &pipes.dof_blur_v, view_group, &v);
                    None
                }
            };
            let composite = group(src, [Some(&i.far.view), near]);
            draw(
                encoder,
                &[&out.view],
                &pipes.dof_composite,
                view_group,
                &composite,
            );
            src = &out.view;
            written = Some(0);
        }
        self.mark(encoder, "Depth of field");
        let next = written.map_or(0, |i| i + 1);
        if f.motion_blur
            && let (Some([tiles, neighbours]), Some(out)) = (&state.mb, &state.chain[next])
            && frame_debug::whole(
                &self.capture,
                &["Post", "Motion Blur"],
                || hdr("Motion Blur"),
                EventTarget::None,
                post("Motion Blur", "motion_blur.wgsl"),
            )
        {
            let group = |src, aux| pipes.group(device, target, &state.params, src, aux);
            draw(
                encoder,
                &[&tiles.view],
                &pipes.mb_tiles,
                view_group,
                &group(src, [None, None]),
            );
            let around = group(&tiles.view, [None, None]);
            draw(
                encoder,
                &[&neighbours.view],
                &pipes.mb_neighbours,
                view_group,
                &around,
            );
            let gather = group(src, [Some(&neighbours.view), None]);
            draw(encoder, &[&out.view], &pipes.mb_gather, view_group, &gather);
            written = Some(next);
        }
        self.mark(encoder, "Motion blur");
        let out = state.chain[written?].as_ref()?;
        Some(target.composite_group_for(device, &self.pipelines.composite_layout, &out.view))
    }
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use wgpu::naga;

    #[test]
    fn shaders_validate() {
        let common = super::super::pipelines::COMMON;
        for (name, src) in [
            ("dof", format!("{common}\n{}", super::DOF_SRC)),
            ("motion blur", format!("{common}\n{}", super::MB_SRC)),
            (
                "auto exposure",
                format!("{}\n{}", super::PARAMS_SRC, super::AE_SRC),
            ),
        ] {
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}
