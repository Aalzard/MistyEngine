//! GPU objects of the screen-space effects: prepass and compute pipelines, and each view's
//! textures, buffers and bind groups.

use super::*;

impl Passes {
    pub(super) fn new(device: &wgpu::Device, p: &pipelines::Pipelines) -> Self {
        let buffers = pipelines::mesh_buffers();
        let mut prepass = Vec::new();
        for cutout in [false, true] {
            for double_sided in [false, true] {
                let all = [&p.view_layout, &p.draws_layout, &p.material_layout];
                // Only alpha-tested variants sample the albedo (group 2).
                let layouts = if cutout { &all[..] } else { &all[..2] };
                let pipeline = pipelines::build(
                    device,
                    Spec {
                        vs: "vs_main",
                        fs: Some(if cutout {
                            "fs_normal_cutout"
                        } else {
                            "fs_normal"
                        }),
                        buffers: &buffers,
                        cull: (!double_sided).then_some(wgpu::Face::Back),
                        depth: Some(Depth {
                            format: DEPTH_FORMAT,
                            write: true,
                            compare: wgpu::CompareFunction::Less,
                            bias: Default::default(),
                        }),
                        color: Some(NORMAL_FORMAT),
                        samples: 1,
                        glow: None,
                        velocity: true,
                        ..Spec::new("ao prepass", PREPASS_SRC, layouts)
                    },
                );
                let key = ShadowKey {
                    cutout,
                    double_sided,
                };
                prepass.push((key, pipeline));
            }
        }

        let terrain = pipelines::build(
            device,
            Spec {
                vs: "vs_terrain_prepass",
                fs: Some("fs_normal"),
                cull: Some(wgpu::Face::Back),
                depth: Some(Depth {
                    format: DEPTH_FORMAT,
                    write: true,
                    compare: wgpu::CompareFunction::Less,
                    bias: Default::default(),
                }),
                color: Some(NORMAL_FORMAT),
                samples: 1,
                glow: None,
                velocity: true,
                ..Spec::new(
                    "ao terrain prepass",
                    TERRAIN_PREPASS_SRC,
                    &[
                        &p.view_layout,
                        &p.draws_layout,
                        &p.material_layout,
                        &p.terrain_layout,
                    ],
                )
            },
        );

        let blade_buffers = pipelines::blade_buffers();
        let grass = pipelines::build(
            device,
            Spec {
                vs: "vs_grass_prepass",
                fs: Some("fs_grass_prepass"),
                buffers: &blade_buffers,
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                depth: Some(Depth {
                    format: DEPTH_FORMAT,
                    write: true,
                    compare: wgpu::CompareFunction::Less,
                    bias: Default::default(),
                }),
                color: Some(NORMAL_FORMAT),
                samples: 1,
                glow: None,
                velocity: true,
                ..Spec::new(
                    "ao grass prepass",
                    GRASS_PREPASS_SRC,
                    &[&p.view_layout, &p.draws_layout, &p.grass_layout],
                )
            },
        );

        use wgpu::BindingType as B;
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let texture = |sample_type| B::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let storage = |read_only| B::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty ao"),
            entries: &[
                entry(
                    0,
                    B::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, texture(wgpu::TextureSampleType::Depth)),
                entry(2, texture(unfiltered)),
                entry(3, texture(unfiltered)),
                entry(
                    4,
                    B::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: AO_FORMAT,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                ),
                entry(5, storage(true)),
                entry(6, storage(false)),
                // Motion vectors of the prepass.
                entry(7, texture(unfiltered)),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty ao"),
            source: wgpu::ShaderSource::Wgsl(AO_SRC.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty ao"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let compute = |entry| {
            crate::render::pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            prepass,
            terrain,
            grass,
            gtao: compute("cs_gtao"),
            denoise: compute("cs_denoise"),
            resolve: compute("cs_resolve"),
            layout,
        }
    }

    pub(super) fn prepass_variant(&self, key: ShadowKey) -> Option<&wgpu::RenderPipeline> {
        self.prepass.iter().find(|(k, _)| *k == key).map(|(_, p)| p)
    }
}

impl TargetState {
    /// Textures for a `full`-sized view with AO texels of `scale` pixels; `bytes` = texel buffer size.
    pub(super) fn new(
        device: &wgpu::Device,
        passes: &Passes,
        full: (u32, u32),
        scale: u32,
        bytes: u64,
    ) -> Self {
        let ao_size = (full.0.div_ceil(scale), full.1.div_ceil(scale));
        let texture = |label, (width, height): (u32, u32), format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let store = wgpu::TextureUsages::STORAGE_BINDING;
        let normal_view = texture("misty prepass normals", full, NORMAL_FORMAT, attach);
        let depth_view = texture("misty prepass depth", full, DEPTH_FORMAT, attach);
        let velocity_view = texture(
            "misty prepass velocity",
            full,
            pipelines::VELOCITY_FORMAT,
            attach,
        );
        let ao = [
            texture("misty ao a", ao_size, AO_FORMAT, store),
            texture("misty ao b", ao_size, AO_FORMAT, store),
        ];
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty ao params"),
            size: std::mem::size_of::<AoParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty screen-space texels"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let buffers = [buffer(), buffer()];
        // (input AO, output AO, history, current)
        let group = |input: usize, output: usize, history: usize, current: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty ao group"),
                layout: &passes.layout,
                entries: &[
                    buf(0, &params),
                    tex(1, &depth_view),
                    tex(2, &normal_view),
                    tex(3, &ao[input]),
                    tex(4, &ao[output]),
                    buf(5, &buffers[history]),
                    buf(6, &buffers[current]),
                    tex(7, &velocity_view),
                ],
            })
        };
        // GTAO → a, denoise a → b, resolve b → buffer 0 or 1 (the other is history).
        let groups = [
            group(1, 0, 1, 0),
            group(0, 1, 1, 0),
            group(1, 0, 1, 0),
            group(1, 0, 0, 1),
        ];
        Self {
            scale,
            full,
            ao_size,
            normal_view,
            depth_view,
            velocity_view,
            params,
            buffers,
            groups,
            frame: 0,
            history: None,
        }
    }
}

fn tex(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn buf(binding: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: b.as_entire_binding(),
    }
}
