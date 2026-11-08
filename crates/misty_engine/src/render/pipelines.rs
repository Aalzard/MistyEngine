//! Bind group layouts, shader modules and render pipelines.

use super::mesh::Vertex;
use super::particles::ParticleInstance;
use super::{COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, LineVertex, MASK_FORMAT};

/// Shared view uniform + helpers, then the clouds and fog every forward shader applies.
pub(super) const COMMON: &str = concat!(
    include_str!("shaders/common.wgsl"),
    "\n",
    include_str!("clouds/shaders/cloud_shadow_lookup.wgsl"),
    "\n",
    include_str!("clouds/shaders/clouds_apply.wgsl"),
    "\n",
    include_str!("volumetrics/shaders/fog_apply.wgsl")
);
/// Lit/unlit meshes (all material variants), after the shared per-draw struct, with the
/// soft-shadow, reflection probe and screen-space (AO, contact shadows) includes. Custom
/// `.shader` assets and the water are appended to it, so they get the same `shade`.
pub(super) const MESH_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/mesh.wgsl"),
    "\n",
    include_str!("shaders/shadows.wgsl"),
    "\n",
    include_str!("vsm/shaders/vsm_common.wgsl"),
    "\n",
    include_str!("shadow_denoise/shaders/shadow_lookup.wgsl"),
    "\n",
    include_str!("area/area_light.wgsl"),
    "\n",
    include_str!("probes/probes.wgsl"),
    "\n",
    include_str!("shaders/screen_space.wgsl"),
    "\n",
    include_str!("screen_trace/shaders/screen_trace.wgsl")
);
/// Shadow-map depth and the selection mask.
pub(super) const DEPTH_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/depth.wgsl")
);

/// Mesh vertex attributes: position, normal, uv, tangent, lightmap uv.
const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x2,
    3 => Float32x4,
    4 => Float32x2
];

/// Vertex buffer layout of [`Vertex`] meshes.
/// Grass blades: one instance per [`crate::terrain::details::Blade`].
pub(super) fn mesh_buffers() -> [wgpu::VertexBufferLayout<'static>; 1] {
    [wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &MESH_ATTRIBUTES,
    }]
}
/// The terrain shader reuses the mesh shader's draws and lighting.
const TERRAIN_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/mesh.wgsl"),
    "\n",
    include_str!("shaders/shadows.wgsl"),
    "\n",
    include_str!("vsm/shaders/vsm_common.wgsl"),
    "\n",
    include_str!("shadow_denoise/shaders/shadow_lookup.wgsl"),
    "\n",
    include_str!("area/area_light.wgsl"),
    "\n",
    include_str!("probes/probes.wgsl"),
    "\n",
    include_str!("shaders/screen_space.wgsl"),
    "\n",
    include_str!("screen_trace/shaders/screen_trace.wgsl"),
    "\n",
    include_str!("shaders/terrain_vertex.wgsl"),
    "\n",
    include_str!("shaders/grass_field.wgsl"),
    "\n",
    include_str!("shaders/terrain_layers.wgsl"),
    "\n",
    include_str!("shaders/terrain.wgsl")
);
/// Terrain shadow casters: the depth shader plus terrain vertex pulling (after draw.wgsl).
const TERRAIN_SHADOW_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/depth.wgsl"),
    "\n",
    include_str!("shaders/terrain_vertex.wgsl")
);
/// Grass blades, lit like meshes (same includes as `MESH_SRC`).
pub(super) const GRASS_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/mesh.wgsl"),
    "\n",
    include_str!("shaders/shadows.wgsl"),
    "\n",
    include_str!("vsm/shaders/vsm_common.wgsl"),
    "\n",
    include_str!("shadow_denoise/shaders/shadow_lookup.wgsl"),
    "\n",
    include_str!("area/area_light.wgsl"),
    "\n",
    include_str!("probes/probes.wgsl"),
    "\n",
    include_str!("shaders/screen_space.wgsl"),
    "\n",
    include_str!("screen_trace/shaders/screen_trace.wgsl"),
    "\n",
    include_str!("shaders/grass_field.wgsl"),
    "\n",
    include_str!("shaders/grass_blade.wgsl"),
    "\n",
    include_str!("shaders/grass.wgsl")
);
/// Grass shadow casters: the blades' geometry, alpha tested.
const GRASS_SHADOW_SRC: &str = concat!(
    include_str!("shaders/grass_field.wgsl"),
    "\n",
    include_str!("shaders/grass_blade.wgsl"),
    "\n",
    include_str!("shaders/grass_shadow.wgsl")
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
    /// The sky drawn after the opaque scene: on the far plane, depth tested (`main_pass`).
    pub sky_late: wgpu::RenderPipeline,
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
    /// The terrain's depth alone (main pass targets untouched), and its shading tested
    /// against that depth: the grass visibility buffer's standing blades go between them
    /// (`Renderer::main_pass`).
    pub terrain_depth: wgpu::RenderPipeline,
    pub terrain_shade: wgpu::RenderPipeline,
    /// Terrain shadow casters (vertex pulling from the heightmap texture).
    pub terrain_shadow: wgpu::RenderPipeline,
    /// Grass parameters (group 2 of the grass pipeline).
    pub grass_layout: wgpu::BindGroupLayout,
    /// A chunk's blades and the list of the ones a draw shows (group 3 of the grass
    /// pipelines).
    pub grass_blades_layout: wgpu::BindGroupLayout,
    pub grass: wgpu::RenderPipeline,
    /// Grass shadow casters (groups: shadow view, draws, grass).
    pub grass_shadow: wgpu::RenderPipeline,
    /// Grass depth before the lit grass (same pass): alpha-tested depth writes only, so each
    /// covered sample is shaded once however many blades overlap it.
    pub grass_depth: wgpu::RenderPipeline,
    pub bloom_prefilter: wgpu::RenderPipeline,
    pub bloom_down: wgpu::RenderPipeline,
    pub bloom_up: wgpu::RenderPipeline,
    pub composite: wgpu::RenderPipeline,
    /// FXAA / sharpening after the composite.
    pub finish: super::finish::FinishPipelines,
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
    /// MSAA coverage from the fragment's alpha (grass cards).
    pub alpha_to_coverage: bool,
    /// The fragment stage writes the motion-vector target (`velocity` at the location after
    /// the colour targets, see common.wgsl). Main-pass pipelines (glow + depth) always have
    /// the target, masked off when they don't write it; a prepass pipeline gets it only when
    /// this is set.
    pub velocity: bool,
    /// Channels written to the colour target (a depth-only pass with alpha to coverage keeps
    /// just the alpha).
    pub color_writes: wgpu::ColorWrites,
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
            samples: super::sample_count(),
            glow: Some(false),
            alpha_to_coverage: false,
            velocity: false,
            color_writes: wgpu::ColorWrites::ALL,
        }
    }
}

/// Motion vectors: last frame's uv minus this frame's, per pixel (common.wgsl `velocity_of`).
pub const VELOCITY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

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
        // View uniform + the fog volume, sampler and uniform (render/volumetrics).
        let mut view_entries = vec![entry(0, S::VERTEX_FRAGMENT, uniform())];
        view_entries.extend(super::volumetrics::view_layout_entries());
        // HDRI sky: uniform, background and specular cubes, sampler (render/hdri_sky).
        view_entries.extend(super::hdri_sky::layout_entries());
        view_entries.extend(super::clouds::view_layout_entries());
        // (The grass visibility resolve lights blades in a compute pass, grass_vis.rs.)
        for e in &mut view_entries {
            e.visibility |= S::COMPUTE;
        }
        let view_layout = layout("misty view", &view_entries);
        let probe = super::probes::gpu::layout_entries();
        let lightmap = super::lightmap_gpu::layout_entries();
        // What `shade_grass` reads is visible to compute too (the grass visibility resolve,
        // grass_vis.rs; only that: compute stages have the same storage buffer limit).
        let lit = S::FRAGMENT | S::COMPUTE;
        let mesh_layout = layout(
            "misty mesh",
            &[
                entry(0, lit, uniform()),
                entry(1, S::VERTEX_FRAGMENT, storage()),
                // Sun cascades (one layer each).
                entry(
                    2,
                    lit,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    3,
                    lit,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
                entry(
                    4,
                    lit,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                // Instance → draw slot (instanced batches).
                entry(5, S::VERTEX, storage()),
                // Soft-shadow parameters (shadows.wgsl).
                entry(6, lit, uniform()),
                // Rectangle lights' LTC table (area/area_light.wgsl).
                entry(7, S::FRAGMENT, storage()),
                // Near-camera grass shadow map (one layer per slice) and its uniform
                // (grass_near_shadow.rs).
                entry(
                    8,
                    lit,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(9, lit, uniform()),
                // The sun's virtual shadow map: page table and physical pool (render/vsm).
                entry(
                    10,
                    lit,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    11,
                    lit,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                // Its near layer (casters that sway, redrawn every frame).
                entry(
                    12,
                    lit,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                // Reflection probes (bindings 20–24, see probes/gpu.rs).
                probe[0],
                probe[1],
                probe[2],
                probe[3],
                probe[4],
                // Baked lightmaps (bindings 25–26, see lightmap_gpu.rs).
                lightmap[0],
                lightmap[1],
                // Screen-space AO and contact shadow texels (screen_space.rs).
                entry(16, lit, storage()),
                // Screen-space GI and reflection texels (screen_trace).
                entry(17, lit, storage()),
                // Temporal soft-shadow mask (shadow_denoise).
                entry(super::shadow_denoise::BINDING, S::FRAGMENT, storage()),
                // 2D lights and shadow maps (light2d.rs).
                entry(super::light2d::BINDING, S::FRAGMENT, storage()),
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
                // Auto Exposure's multiplier (render/postfx).
                entry(3, S::FRAGMENT, storage()),
            ],
        );

        // The sky writes the sun disc to the glow target, and the camera's rotation to the
        // motion vectors.
        let sky = build(
            device,
            Spec {
                glow: Some(true),
                velocity: true,
                ..Spec::new("sky", include_str!("shaders/sky.wgsl"), &[&view_layout])
            },
        );
        let sky_late = build(
            device,
            Spec {
                vs: "vs_sky_far",
                depth: Depth::main(false, wgpu::CompareFunction::LessEqual),
                glow: Some(true),
                velocity: true,
                ..Spec::new(
                    "sky late",
                    include_str!("shaders/sky.wgsl"),
                    &[&view_layout],
                )
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
                2 => Float32x4,
                3 => Float32x4,
                4 => Float32x4
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

        // Terrain: mesh layouts plus its own textures at group 3 (see terrain_vertex.wgsl).
        let array = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        };
        let unfiltered = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let terrain_entries = [
            entry(0, S::VERTEX_FRAGMENT, uniform()),
            entry(1, S::FRAGMENT, filtered),
            entry(2, S::FRAGMENT, filtering),
            entry(3, S::FRAGMENT, array),
            entry(4, S::FRAGMENT, array),
            entry(6, S::FRAGMENT, filtering),
            entry(12, S::VERTEX, unfiltered),
            entry(13, S::VERTEX_FRAGMENT, filtered),
        ];
        let terrain_layout = layout("misty terrain", &terrain_entries);
        let empty_layout = layout("misty empty", &[]);
        let terrain = build(
            device,
            Spec {
                vs: "vs_terrain",
                fs: Some("fs_terrain"),
                cull: Some(wgpu::Face::Back),
                depth: Depth::main(true, wgpu::CompareFunction::Less),
                glow: Some(true),
                velocity: true,
                ..Spec::new(
                    "terrain",
                    TERRAIN_SRC,
                    &[&view_layout, &mesh_layout, &empty_layout, &terrain_layout],
                )
            },
        );
        let terrain_depth = build(
            device,
            Spec {
                vs: "vs_terrain",
                fs: Some("fs_terrain_depth"),
                cull: Some(wgpu::Face::Back),
                depth: Depth::main(true, wgpu::CompareFunction::Less),
                glow: Some(false),
                color_writes: wgpu::ColorWrites::empty(),
                ..Spec::new(
                    "terrain depth",
                    TERRAIN_SRC,
                    &[&view_layout, &mesh_layout, &empty_layout, &terrain_layout],
                )
            },
        );
        let terrain_shade = build(
            device,
            Spec {
                vs: "vs_terrain",
                fs: Some("fs_terrain"),
                cull: Some(wgpu::Face::Back),
                depth: Depth::main(false, wgpu::CompareFunction::LessEqual),
                glow: Some(true),
                velocity: true,
                ..Spec::new(
                    "terrain shade",
                    TERRAIN_SRC,
                    &[&view_layout, &mesh_layout, &empty_layout, &terrain_layout],
                )
            },
        );
        let terrain_shadow = build(
            device,
            Spec {
                vs: "vs_terrain_depth",
                fs: None,
                cull: Some(wgpu::Face::Back),
                depth: Some(Depth {
                    format: super::SHADOW_FORMAT,
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
                ..Spec::new(
                    "terrain shadow",
                    TERRAIN_SHADOW_SRC,
                    &[
                        &shadow_view_layout,
                        &draws_layout,
                        &material_layout,
                        &terrain_layout,
                    ],
                )
            },
        );
        // Grass parameters, the grass atlas (grass and flower layers) and its sampler.
        let grass_layout = layout(
            "misty grass",
            &[
                entry(0, S::VERTEX_FRAGMENT | S::COMPUTE, uniform()),
                entry(
                    1,
                    lit,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    lit,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        );
        // The drawn chunk's blades and the list of the ones a draw shows (grass_cull.rs).
        let grass_blades_layout = layout(
            "misty grass blades",
            &[
                entry(0, S::VERTEX, storage()),
                entry(1, S::VERTEX, storage()),
            ],
        );
        let grass = build(
            device,
            Spec {
                vs: "vs_grass",
                fs: Some("fs_grass"),
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                // Tests against the depth the grass depth pipeline wrote just before.
                depth: Depth::main(false, wgpu::CompareFunction::LessEqual),
                glow: Some(true),
                alpha_to_coverage: true,
                velocity: true,
                ..Spec::new(
                    "grass",
                    GRASS_SRC,
                    &[
                        &view_layout,
                        &mesh_layout,
                        &grass_layout,
                        &grass_blades_layout,
                    ],
                )
            },
        );
        let grass_depth = build(
            device,
            Spec {
                vs: "vs_grass_depth",
                fs: Some("fs_grass_depth"),
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                depth: Depth::main(true, wgpu::CompareFunction::Less),
                glow: Some(false),
                alpha_to_coverage: true,
                color_writes: wgpu::ColorWrites::ALPHA,
                ..Spec::new(
                    "grass depth",
                    GRASS_SRC,
                    &[
                        &view_layout,
                        &mesh_layout,
                        &grass_layout,
                        &grass_blades_layout,
                    ],
                )
            },
        );
        let grass_shadow = build(
            device,
            Spec {
                vs: "vs_grass_shadow",
                fs: Some("fs_grass_shadow"),
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                depth: Some(Depth {
                    format: super::SHADOW_FORMAT,
                    write: true,
                    compare: wgpu::CompareFunction::LessEqual,
                    // Blades shade themselves: a larger slope bias keeps a card's own shadow
                    // off it.
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.5,
                        clamp: 0.0,
                    },
                }),
                color: None,
                samples: 1,
                glow: None,
                ..Spec::new(
                    "grass shadow",
                    GRASS_SHADOW_SRC,
                    &[
                        &shadow_view_layout,
                        &draws_layout,
                        &grass_layout,
                        &grass_blades_layout,
                    ],
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

        let finish = super::finish::FinishPipelines::new(device, &post_layouts);
        Self {
            finish,
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
            sky_late,
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
            terrain_depth,
            terrain_shade,
            terrain_shadow,
            grass_layout,
            grass_blades_layout,
            grass,
            grass_shadow,
            grass_depth,
        }
    }
}

/// Pipelines built since the program started (render and compute, every module): the
/// Stats overlay and the warm-up tests see where a shader compiled in the middle of a run.
static BUILT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Count a pipeline build (called next to every `create_*_pipeline`).
pub(crate) fn note_pipeline() {
    BUILT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// How many pipelines were built so far (see [`note_pipeline`]).
pub fn pipelines_built() -> u64 {
    BUILT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The device whose shader modules are kept, with its modules by source.
type Modules = (
    wgpu::Device,
    std::collections::HashMap<String, wgpu::ShaderModule>,
);
static MODULES: std::sync::Mutex<Option<Modules>> = std::sync::Mutex::new(None);

fn modules() -> std::sync::MutexGuard<'static, Option<Modules>> {
    MODULES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keep the shader modules made on `device` (the editor's and the player's one device), so
/// each source is parsed once (see [`shader_module`]). Only one device: wgpu compares devices
/// by id, and devices of different instances (tests make many) can share one.
pub fn cache_shader_modules(device: &wgpu::Device) {
    *modules() = Some((device.clone(), Default::default()));
}

/// The shader module of `source` (after the common prelude) on `device`: made once on the
/// device of [`cache_shader_modules`], every time elsewhere.
///
/// Most pipelines share their source with others (the variants of a mesh shader; the main,
/// shadow and prepass pipelines of the terrain, grass and trees), and parsing and validating
/// the prelude plus the source was two thirds of what building the pipelines of a scene's
/// first frame cost (148 of 218 ms for MeadowDemo's 51).
fn shader_module(device: &wgpu::Device, name: &str, source: &str) -> wgpu::ShaderModule {
    let cached = modules()
        .as_ref()
        .filter(|(d, _)| d == device)
        .and_then(|(_, m)| m.get(source).cloned());
    if let Some(m) = cached {
        return m;
    }
    crate::profile_scope!("Pipeline.Module");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(format!("{COMMON}\n{source}").into()),
    });
    if let Some((d, m)) = modules().as_mut()
        && d == device
    {
        m.insert(source.to_owned(), module.clone());
    }
    module
}

pub(super) fn build(device: &wgpu::Device, spec: Spec) -> wgpu::RenderPipeline {
    crate::profile_scope!(format!("Pipeline: {}", spec.name));
    let module = shader_module(device, spec.name, spec.source);
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
                write_mask: spec.color_writes,
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
    // The motion-vector target: every main pass pipeline has it (the pass binds it), the
    // prepass ones that write it too. Never blended: a transparent surface leaves the
    // motion of what is behind it.
    let main_pass = spec.glow.is_some() && spec.depth.is_some();
    if main_pass || spec.velocity {
        targets.push(Some(wgpu::ColorTargetState {
            format: VELOCITY_FORMAT,
            blend: None,
            write_mask: if spec.velocity {
                wgpu::ColorWrites::ALL
            } else {
                wgpu::ColorWrites::empty()
            },
        }));
    }
    crate::render::pipelines::note_pipeline();
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
            alpha_to_coverage_enabled: spec.alpha_to_coverage && spec.samples > 1,
            ..Default::default()
        },
        fragment: spec.fs.map(|fs| wgpu::FragmentState {
            module: &module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: crate::render::pipeline_cache(device).as_ref(),
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
            ("finish", super::super::finish::SRC),
            ("particles", include_str!("shaders/particles.wgsl")),
            ("terrain", super::TERRAIN_SRC),
            ("ao prepass", super::super::screen_space::PREPASS_SRC),
            (
                "ao terrain prepass",
                super::super::screen_space::TERRAIN_PREPASS_SRC,
            ),
            ("terrain shadow", super::TERRAIN_SHADOW_SRC),
            ("grass", super::GRASS_SRC),
            ("grass shadow", super::GRASS_SHADOW_SRC),
            (
                "grass prepass",
                super::super::screen_space::GRASS_PREPASS_SRC,
            ),
            (
                "grass visibility",
                format!(
                    "{}
{}",
                    super::GRASS_SRC,
                    include_str!("shaders/grass_vis.wgsl")
                )
                .leak(),
            ),
            (
                "grass composite",
                include_str!("shaders/grass_composite.wgsl"),
            ),
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
