//! Bind group layouts, shader modules and render pipelines.

use super::mesh::Vertex;
use super::{
    COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, LineVertex, MASK_FORMAT, SAMPLE_COUNT, SHADOW_FORMAT,
};

const COMMON: &str = include_str!("shaders/common.wgsl");

pub struct Pipelines {
    pub view_layout: wgpu::BindGroupLayout,
    /// View uniform with a dynamic offset (one slot per shadow map).
    pub shadow_view_layout: wgpu::BindGroupLayout,
    /// Lights + draws + shadow map (lit mesh pass).
    pub mesh_layout: wgpu::BindGroupLayout,
    /// Draws only (shadow and mask passes).
    pub draws_layout: wgpu::BindGroupLayout,
    /// Selection mask texture (outline pass).
    pub mask_layout: wgpu::BindGroupLayout,
    /// One sampled texture + sampler (bloom passes).
    pub post_layout: wgpu::BindGroupLayout,
    /// HDR image + sampler + bloom (final composite).
    pub composite_layout: wgpu::BindGroupLayout,
    pub sky: wgpu::RenderPipeline,
    pub grid: wgpu::RenderPipeline,
    pub lines: wgpu::RenderPipeline,
    pub mesh: wgpu::RenderPipeline,
    pub shadow: wgpu::RenderPipeline,
    pub mask: wgpu::RenderPipeline,
    pub outline: wgpu::RenderPipeline,
    pub bloom_prefilter: wgpu::RenderPipeline,
    pub bloom_down: wgpu::RenderPipeline,
    pub bloom_up: wgpu::RenderPipeline,
    pub composite: wgpu::RenderPipeline,
}

/// Depth state; `None` in [`Spec::depth`] means the pass has no depth attachment.
struct Depth {
    format: wgpu::TextureFormat,
    write: bool,
    compare: wgpu::CompareFunction,
    bias: wgpu::DepthBiasState,
}

impl Depth {
    fn main(write: bool, compare: wgpu::CompareFunction) -> Option<Self> {
        Some(Self {
            format: DEPTH_FORMAT,
            write,
            compare,
            bias: Default::default(),
        })
    }
}

/// Everything that differs between pipelines.
struct Spec<'a> {
    name: &'a str,
    source: &'a str,
    layouts: &'a [&'a wgpu::BindGroupLayout],
    vs: &'a str,
    fs: Option<&'a str>,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
    topology: wgpu::PrimitiveTopology,
    cull: Option<wgpu::Face>,
    depth: Option<Depth>,
    color: Option<wgpu::TextureFormat>,
    blend: Option<wgpu::BlendState>,
    samples: u32,
    /// Main-pass glow target: None = not in the main pass, Some(true) = writes emission.
    glow: Option<bool>,
}

impl<'a> Spec<'a> {
    /// Defaults for a fullscreen/main-pass pipeline.
    fn new(name: &'a str, source: &'a str, layouts: &'a [&'a wgpu::BindGroupLayout]) -> Self {
        Self {
            name,
            source,
            layouts,
            vs: "vs_fullscreen",
            fs: Some("fs_main"),
            buffers: &[],
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull: None,
            depth: Depth::main(false, wgpu::CompareFunction::Always),
            color: Some(HDR_FORMAT),
            blend: None,
            samples: SAMPLE_COUNT,
            glow: Some(false),
        }
    }
}

fn entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    }
}

fn uniform() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn storage() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let view_layout = layout("misty view", &[entry(0, S::VERTEX_FRAGMENT, uniform())]);
        let mesh_layout = layout(
            "misty mesh",
            &[
                entry(0, S::FRAGMENT, uniform()),
                entry(1, S::VERTEX_FRAGMENT, storage()),
                entry(
                    2,
                    S::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    3,
                    S::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
                entry(
                    4,
                    S::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
            ],
        );
        let draws_layout = layout("misty draws", &[entry(1, S::VERTEX, storage())]);
        let shadow_view_layout = layout(
            "misty shadow view",
            &[entry(
                0,
                S::VERTEX_FRAGMENT,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
            )],
        );
        let mask_layout = layout(
            "misty mask",
            &[entry(
                0,
                S::FRAGMENT,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )],
        );

        let filtered = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let filtering = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
        let post_layout = layout(
            "misty post",
            &[
                entry(0, S::FRAGMENT, filtered),
                entry(1, S::FRAGMENT, filtering),
            ],
        );
        let composite_layout = layout(
            "misty composite",
            &[
                entry(0, S::FRAGMENT, filtered),
                entry(1, S::FRAGMENT, filtering),
                entry(2, S::FRAGMENT, filtered),
            ],
        );

        let sky = build(
            device,
            Spec::new("sky", include_str!("shaders/sky.wgsl"), &[&view_layout]),
        );
        let grid = build(
            device,
            Spec {
                depth: Depth::main(false, wgpu::CompareFunction::LessEqual),
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                ..Spec::new("grid", include_str!("shaders/grid.wgsl"), &[&view_layout])
            },
        );
        let line_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
        }];
        let lines = build(
            device,
            Spec {
                vs: "vs_main",
                buffers: &line_buffers,
                topology: wgpu::PrimitiveTopology::LineList,
                depth: Depth::main(true, wgpu::CompareFunction::LessEqual),
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                ..Spec::new("lines", include_str!("shaders/lines.wgsl"), &[&view_layout])
            },
        );
        let mesh_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
        }];
        let mesh = build(
            device,
            Spec {
                vs: "vs_main",
                buffers: &mesh_buffers,
                cull: Some(wgpu::Face::Back),
                depth: Depth::main(true, wgpu::CompareFunction::Less),
                glow: Some(true),
                ..Spec::new(
                    "mesh",
                    include_str!("shaders/mesh.wgsl"),
                    &[&view_layout, &mesh_layout],
                )
            },
        );
        let depth_src = include_str!("shaders/depth.wgsl");
        let shadow = build(
            device,
            Spec {
                vs: "vs_main",
                fs: None,
                buffers: &mesh_buffers,
                // Light-facing faces, so shadows start right at contact points (no gap).
                // Acne is handled by a small slope bias plus a texel-sized normal offset in the shader.
                cull: Some(wgpu::Face::Back),
                depth: Some(Depth {
                    format: SHADOW_FORMAT,
                    write: true,
                    compare: wgpu::CompareFunction::LessEqual,
                    bias: wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 1.0,
                        clamp: 0.0,
                    },
                }),
                color: None,
                samples: 1,
                glow: None,
                ..Spec::new("shadow", depth_src, &[&shadow_view_layout, &draws_layout])
            },
        );
        let mask = build(
            device,
            Spec {
                vs: "vs_main",
                fs: Some("fs_mask"),
                buffers: &mesh_buffers,
                depth: None,
                color: Some(MASK_FORMAT),
                samples: 1,
                glow: None,
                ..Spec::new("mask", depth_src, &[&view_layout, &draws_layout])
            },
        );
        let outline = build(
            device,
            Spec {
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                ..Spec::new(
                    "outline",
                    include_str!("shaders/outline.wgsl"),
                    &[&view_layout, &mask_layout],
                )
            },
        );

        // Post-processing: single-sample, no depth.
        let bloom_src = include_str!("shaders/bloom.wgsl");
        let post_layouts = [&view_layout, &post_layout];
        let post = |name, fs, blend, color| Spec {
            fs: Some(fs),
            depth: None,
            samples: 1,
            blend,
            color: Some(color),
            glow: None,
            ..Spec::new(name, bloom_src, &post_layouts)
        };
        let bloom_prefilter = build(
            device,
            post("bloom prefilter", "fs_prefilter", None, HDR_FORMAT),
        );
        let bloom_down = build(
            device,
            post("bloom down", "fs_downsample", None, HDR_FORMAT),
        );
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let bloom_up = build(
            device,
            post("bloom up", "fs_upsample", Some(additive), HDR_FORMAT),
        );
        let composite = build(
            device,
            Spec {
                depth: None,
                samples: 1,
                color: Some(COLOR_FORMAT),
                glow: None,
                ..Spec::new(
                    "composite",
                    include_str!("shaders/composite.wgsl"),
                    &[&view_layout, &composite_layout],
                )
            },
        );

        Self {
            view_layout,
            post_layout,
            composite_layout,
            bloom_prefilter,
            bloom_down,
            bloom_up,
            composite,
            shadow_view_layout,
            mesh_layout,
            draws_layout,
            mask_layout,
            sky,
            grid,
            lines,
            mesh,
            shadow,
            mask,
            outline,
        }
    }
}

fn build(device: &wgpu::Device, spec: Spec) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(spec.name),
        source: wgpu::ShaderSource::Wgsl(format!("{COMMON}\n{}", spec.source).into()),
    });
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> =
        spec.layouts.iter().map(|l| Some(*l)).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(spec.name),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    let mut targets: Vec<Option<wgpu::ColorTargetState>> = spec
        .color
        .map(|format| {
            vec![Some(wgpu::ColorTargetState {
                format,
                blend: spec.blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        })
        .unwrap_or_default();
    if let Some(writes) = spec.glow {
        targets.push(Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend: None,
            write_mask: if writes {
                wgpu::ColorWrites::ALL
            } else {
                wgpu::ColorWrites::empty()
            },
        }));
    }
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.name),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(spec.vs),
            compilation_options: Default::default(),
            buffers: &spec.buffers.iter().cloned().map(Some).collect::<Vec<_>>(),
        },
        primitive: wgpu::PrimitiveState {
            topology: spec.topology,
            front_face: wgpu::FrontFace::Cw,
            cull_mode: spec.cull,
            ..Default::default()
        },
        depth_stencil: spec.depth.map(|d| wgpu::DepthStencilState {
            format: d.format,
            depth_write_enabled: Some(d.write),
            depth_compare: Some(d.compare),
            stencil: Default::default(),
            bias: d.bias,
        }),
        multisample: wgpu::MultisampleState {
            count: spec.samples,
            ..Default::default()
        },
        fragment: spec.fs.map(|fs| wgpu::FragmentState {
            module: &module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::COMMON;
    use wgpu::naga;

    /// Every shader must parse and validate (catches WGSL errors without a GPU).
    #[test]
    fn shaders_validate() {
        for (name, src) in [
            ("sky", include_str!("shaders/sky.wgsl")),
            ("grid", include_str!("shaders/grid.wgsl")),
            ("lines", include_str!("shaders/lines.wgsl")),
            ("mesh", include_str!("shaders/mesh.wgsl")),
            ("depth", include_str!("shaders/depth.wgsl")),
            ("outline", include_str!("shaders/outline.wgsl")),
            ("bloom", include_str!("shaders/bloom.wgsl")),
            ("composite", include_str!("shaders/composite.wgsl")),
        ] {
            let module = naga::front::wgsl::parse_str(&format!("{COMMON}\n{src}"))
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}
