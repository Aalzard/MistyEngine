//! Screen-space sun shafts: half-resolution prefilter of the bright sky near the sun, three
//! radial blurs, then an additive composite onto the HDR image (before bloom).

use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4Swizzles};

use crate::render::pipelines::{self, Spec};
use crate::render::{Frame, HDR_FORMAT, srgb_to_linear};

pub(super) const SRC: &str = include_str!("shaders/sun_shafts.wgsl");

/// `Shafts` in sun_shafts.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct ShaftUniform {
    sun: [f32; 4],
    color: [f32; 4],
    shape: [f32; 4],
}

/// The shafts uniform for this view, or `None` when nothing would show (off, no sun, sun
/// behind the camera or far off screen).
pub(super) fn uniform(frame: &Frame, aspect: f32) -> Option<ShaftUniform> {
    let s = &frame.post.sun_shafts;
    let sun = frame.environment.sun?;
    if !s.active || s.intensity <= 0.0 || sun.intensity <= 0.0 {
        return None;
    }
    let cam = frame.camera;
    let dir = sun.direction.normalize_or(Vec3::Y);
    let clip = cam.view_projection(aspect) * (cam.position + dir * cam.far * 0.5).extend(1.0);
    if clip.w <= 0.0 || cam.forward().dot(dir) <= 0.0 {
        return None;
    }
    let ndc = clip.xy() / clip.w;
    let fade = 1.0 - smoothstep(1.0, 1.8, ndc.x.abs().max(ndc.y.abs()));
    if fade <= 0.0 {
        return None;
    }
    let tint = Vec3::from(srgb_to_linear(s.tint));
    let color = tint * Vec3::from(srgb_to_linear(sun.color)) * sun.intensity * s.intensity;
    Some(ShaftUniform {
        sun: [
            ndc.x * 0.5 + 0.5,
            0.5 - ndc.y * 0.5,
            fade,
            s.threshold.max(0.0),
        ],
        color: color.extend(s.length.clamp(0.0, 1.0)).to_array(),
        shape: [s.radius.max(0.01), aspect, 0.0, 0.0],
    })
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) struct ShaftPipelines {
    layout: wgpu::BindGroupLayout,
    prefilter: wgpu::RenderPipeline,
    blurs: [wgpu::RenderPipeline; 3],
    composite: wgpu::RenderPipeline,
    pub uniform: wgpu::Buffer,
}

/// Half-resolution ping-pong images of one view.
pub(super) struct ShaftTextures {
    pub size: (u32, u32),
    views: [wgpu::TextureView; 2],
}

impl ShaftTextures {
    pub fn new(device: &wgpu::Device, full: (u32, u32)) -> Self {
        let size = ((full.0 / 2).max(1), (full.1 / 2).max(1));
        let make = || {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("misty sun shafts"),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: HDR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self {
            size,
            views: [make(), make()],
        }
    }

    pub fn bytes(&self) -> u64 {
        2 * 8 * self.size.0 as u64 * self.size.1 as u64
    }
}

/// One fullscreen pass of the chain: `pipeline` reads `group` into `output`.
pub(super) struct Step<'a> {
    pub pipeline: &'a wgpu::RenderPipeline,
    pub group: wgpu::BindGroup,
    pub output: &'a wgpu::TextureView,
    pub clear: bool,
}

impl ShaftPipelines {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> Self {
        use wgpu::ShaderStages as S;
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: S::FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty sun shafts"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    2,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                ),
                entry(
                    3,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let layouts = [view_layout, &layout];
        let pass = |name, fs, blend| {
            pipelines::build(
                device,
                Spec {
                    fs: Some(fs),
                    depth: None,
                    samples: 1,
                    blend,
                    color: Some(HDR_FORMAT),
                    glow: None,
                    ..Spec::new(name, SRC, &layouts)
                },
            )
        };
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
        Self {
            prefilter: pass("sun shafts prefilter", "fs_prefilter", None),
            blurs: [
                pass("sun shafts blur", "fs_blur_long", None),
                pass("sun shafts blur", "fs_blur_mid", None),
                pass("sun shafts blur", "fs_blur_short", None),
            ],
            composite: pass("sun shafts composite", "fs_composite", Some(add)),
            layout,
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty sun shafts"),
                size: std::mem::size_of::<ShaftUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    fn group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        depth: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty sun shafts group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        })
    }

    /// The passes in order: prefilter hdr → a, blurs a → b → a → b, composite b → hdr.
    pub fn steps<'a>(
        &'a self,
        device: &wgpu::Device,
        tex: &'a ShaftTextures,
        hdr: &'a wgpu::TextureView,
        depth: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> Vec<Step<'a>> {
        let [a, b] = &tex.views;
        let step = |pipeline, source, output, clear| Step {
            pipeline,
            group: self.group(device, source, sampler, depth),
            output,
            clear,
        };
        vec![
            step(&self.prefilter, hdr, a, true),
            step(&self.blurs[0], a, b, true),
            step(&self.blurs[1], b, a, true),
            step(&self.blurs[2], a, b, true),
            step(&self.composite, b, hdr, false),
        ]
    }
}
