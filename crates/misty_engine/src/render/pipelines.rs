//! Bind group layouts, shader modules and render pipelines.

use super::mesh::Vertex;
use super::particles::ParticleInstance;
use super::{COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, LineVertex, MASK_FORMAT, SAMPLE_COUNT};

const COMMON: &str = include_str!("shaders/common.wgsl");
/// Lit/unlit meshes (all material variants), with the soft-shadow and reflection probe includes.
pub(super) const MESH_SRC: &str = concat!(
    include_str!("shaders/mesh.wgsl"),
    "
",
    include_str!("shaders/shadows.wgsl"),
    "
",
    include_str!("probes/probes.wgsl")
);
/// Shadow-map depth and the selection mask.
pub(super) const DEPTH_SRC: &str = include_str!("shaders/depth.wgsl");

/// Mesh vertex attributes: position, normal, uv, tangent.
const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x2,
    3 => Float32x4
];

/// Vertex buffer layout of [`Vertex`] meshes.
pub(super) fn mesh_buffers() -> [wgpu::VertexBufferLayout<'static>; 1] {
    [wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &MESH_ATTRIBUTES,
    }]
}
/// The terrain fragment stage reuses the mesh shader's vertex stage and lighting.
const TERRAIN_SRC: &str = concat!(
    include_str!("shaders/mesh.wgsl"),
    "\n",
    include_str!("shaders/shadows.wgsl"),
    "
",
    include_str!("probes/probes.wgsl"),
    "\n",
    include_str!("shaders/terrain.wgsl")
);

pub struct Pipelines {
    pub view_layout: wgpu::BindGroupLayout,
    /// View uniform with a dynamic offset (one slot per shadow map).
    pub shadow_view_layout: wgpu::BindGroupLayout,
    /// Lights + draws + shadow map (lit mesh pass).
    pub mesh_layout: wgpu::BindGroupLayout,
    /// Draws only (shadow and mask passes).
    pub draws_layout: wgpu::BindGroupLayout,
    /// Per-material albedo, normal, sampler, metallic, occlusion, emission (mesh pass, group 2).
    pub material_layout: wgpu::BindGroupLayout,
    /// Selection mask texture (outline pass).
    pub mask_layout: wgpu::BindGroupLayout,
    /// One sampled texture + sampler (bloom passes).
    pub post_layout: wgpu::BindGroupLayout,
    /// HDR image + sampler + bloom (final composite).
    pub composite_layout: wgpu::BindGroupLayout,
    pub sky: wgpu::RenderPipeline,
    pub grid: wgpu::RenderPipeline,
    pub lines: wgpu::RenderPipeline,
    /// Material variants (rendering mode, unlit, culling), built on first use.
    pub variants: super::variants::Variants,
    /// Translucent triangles with the line shader (no depth writes).
    pub overlay: wgpu::RenderPipeline,
    pub mask: wgpu::RenderPipeline,
    pub outline: wgpu::RenderPipeline,
    /// Billboards: alpha-blended and additive.
    pub particles_alpha: wgpu::RenderPipeline,
    pub particles_additive: wgpu::RenderPipeline,
    /// Splat map + layer textures + params (terrain pass, group 3).
    pub terrain_layout: wgpu::BindGroupLayout,
    /// No bindings: the terrain's unused group 2 (keeps it under 16 textures per stage).
    pub empty_layout: wgpu::BindGroupLayout,
    pub terrain: wgpu::RenderPipeline,
    pub bloom_prefilter: wgpu::RenderPipeline,
    pub bloom_down: wgpu::RenderPipeline,
    pub bloom_up: wgpu::RenderPipeline,
    pub composite: wgpu::RenderPipeline,
}

/// Depth state; `None` in [`Spec::depth`] means the pass has no depth attachment.
pub(super) struct Depth {
    pub format: wgpu::TextureFormat,
    pub write: bool,
    pub compare: wgpu::CompareFunction,
    pub bias: wgpu::DepthBiasState,
}

impl Depth {
    pub fn main(write: bool, compare: wgpu::CompareFunction) -> Option<Self> {
        Some(Self {
            format: DEPTH_FORMAT,
            write,
            compare,
            bias: Default::default(),
        })
    }
}

/// Everything that differs between pipelines.
pub(super) struct Spec<'a> {
    pub name: &'a str,
    pub source: &'a str,
    pub layouts: &'a [&'a wgpu::BindGroupLayout],
    pub vs: &'a str,
    pub fs: Option<&'a str>,
    pub buffers: &'a [wgpu::VertexBufferLayout<'a>],
    pub topology: wgpu::PrimitiveTopology,
    pub cull: Option<wgpu::Face>,
    pub depth: Option<Depth>,
    pub color: Option<wgpu::TextureFormat>,
    pub blend: Option<wgpu::BlendState>,
    pub samples: u32,
    /// Main-pass glow target: None = not in the main pass, Some(true) = writes emission
    /// (blended like the colour target).
    pub glow: Option<bool>,
}

impl<'a> Spec<'a> {
    /// Defaults for a fullscreen/main-pass pipeline.
    pub fn new(name: &'a str, source: &'a str, layouts: &'a [&'a wgpu::BindGroupLayout]) -> Self {
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
        let probe = super::probes::gpu::layout_entries();
        let mesh_layout = layout(
            "misty mesh",
            &[
                entry(0, S::FRAGMENT, uniform()),
                entry(1, S::VERTEX_FRAGMENT, storage()),
                // Sun cascades (one layer each).
                entry(
                    2,
                    S::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
                // Instance → draw slot (instanced batches).
                entry(5, S::VERTEX, storage()),
                // Soft-shadow parameters (shadows.wgsl).
                entry(6, S::FRAGMENT, uniform()),
                // Reflection probes (bindings 20–24, see probes/gpu.rs).
                probe[0],
                probe[1],
                probe[2],
                probe[3],
                probe[4],
            ],
        );
        let draws_layout = layout(
            "misty draws",
            &[
                entry(1, S::VERTEX, storage()),
                entry(2, S::VERTEX, storage()),
            ],
        );
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
        let material_layout = layout(
            "misty material",
            &[
                entry(0, S::FRAGMENT, filtered),
                entry(1, S::FRAGMENT, filtered),
                entry(2, S::FRAGMENT, filtering),
                entry(3, S::FRAGMENT, filtered),
                entry(4, S::FRAGMENT, filtered),
                entry(5, S::FRAGMENT, filtered),
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

        // The sky writes the sun disc to the glow target.
        let sky = build(
            device,
            Spec {
                glow: Some(true),
                ..Spec::new("sky", include_str!("shaders/sky.wgsl"), &[&view_layout])
            },
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
        let overlay = build(
            device,
            Spec {
                vs: "vs_main",
                buffers: &line_buffers,
                depth: Depth::main(false, wgpu::CompareFunction::LessEqual),
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                ..Spec::new(
                    "overlay",
                    include_str!("shaders/lines.wgsl"),
                    &[&view_layout],
                )
            },
        );
        let mesh_buffers = mesh_buffers();
        let depth_src = DEPTH_SRC;
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

        // Particles: tested against opaque depth but don't write it; glow gets the bright part.
        let particle_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x4,
                1 => Float32x4,
                2 => Float32x4
            ],
        }];
        let particle_src = include_str!("shaders/particles.wgsl");
        let particle_layouts = [&view_layout, &material_layout];
        let particles = |name, fs, blend| Spec {
            vs: "vs_main",
            fs: Some(fs),
            buffers: &particle_buffers,
            depth: Depth::main(false, wgpu::CompareFunction::LessEqual),
            blend: Some(blend),
            glow: Some(true),
            ..Spec::new(name, particle_src, &particle_layouts)
        };
        let add_light = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let particles_alpha = build(
            device,
            particles(
                "particles alpha",
                "fs_main",
                wgpu::BlendState::ALPHA_BLENDING,
            ),
        );
        let particles_additive = build(
            device,
            particles("particles additive", "fs_additive", add_light),
        );

        // Terrain: mesh layouts plus its own textures at group 3.
        let mut terrain_entries = vec![
            entry(0, S::FRAGMENT, uniform()),
            entry(1, S::FRAGMENT, filtered),
            entry(2, S::FRAGMENT, filtering),
            entry(11, S::FRAGMENT, filtering),
        ];
        terrain_entries.extend((3..11).map(|b| entry(b, S::FRAGMENT, filtered)));
        let terrain_layout = layout("misty terrain", &terrain_entries);
        let empty_layout = layout("misty empty", &[]);
        let terrain = build(
            device,
            Spec {
                vs: "vs_main",
                fs: Some("fs_terrain"),
                buffers: &mesh_buffers,
                cull: Some(wgpu::Face::Back),
                depth: Depth::main(true, wgpu::CompareFunction::Less),
                glow: Some(true),
                ..Spec::new(
                    "terrain",
                    TERRAIN_SRC,
                    &[&view_layout, &mesh_layout, &empty_layout, &terrain_layout],
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
            material_layout,
            mask_layout,
            sky,
            grid,
            lines,
            variants: Default::default(),
            overlay,
            mask,
            outline,
            particles_alpha,
            particles_additive,
            terrain_layout,
            empty_layout,
            terrain,
        }
    }
}

pub(super) fn build(device: &wgpu::Device, spec: Spec) -> wgpu::RenderPipeline {
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
            blend: if writes { spec.blend } else { None },
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
    use super::{COMMON, DEPTH_SRC, MESH_SRC};
    use wgpu::naga;

    /// Every shader must parse and validate (catches WGSL errors without a GPU).
    #[test]
    fn shaders_validate() {
        for (name, src) in [
            ("sky", include_str!("shaders/sky.wgsl")),
            ("grid", include_str!("shaders/grid.wgsl")),
            ("lines", include_str!("shaders/lines.wgsl")),
            ("mesh", MESH_SRC),
            ("depth", DEPTH_SRC),
            ("outline", include_str!("shaders/outline.wgsl")),
            ("bloom", include_str!("shaders/bloom.wgsl")),
            ("composite", include_str!("shaders/composite.wgsl")),
            ("particles", include_str!("shaders/particles.wgsl")),
            ("terrain", super::TERRAIN_SRC),
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
