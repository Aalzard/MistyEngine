//! GPU objects of the screen-space trace: compute pipelines and layouts, placeholders, and each
//! view's images, pyramids and buffers.

use super::*;

impl Passes {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        use wgpu::BindingType as B;
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let texture = |filterable| B::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let depth = B::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let storage = |format| B::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        };
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let main = layout(
            "misty screen trace",
            &[
                entry(
                    0,
                    B::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, depth),
                entry(2, texture(false)),
                entry(3, texture(true)),
                entry(4, B::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(5, texture(false)),
                entry(6, texture(false)),
                entry(7, texture(false)),
                entry(8, texture(false)),
                entry(9, storage(IMAGE_FORMAT)),
                entry(10, storage(IMAGE_FORMAT)),
                entry(
                    11,
                    B::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        );
        let hiz_copy_layout = layout(
            "misty hiz copy",
            &[entry(0, depth), entry(2, storage(HIZ_FORMAT))],
        );
        let hiz_down_layout = layout(
            "misty hiz down",
            &[entry(1, texture(false)), entry(2, storage(HIZ_FORMAT))],
        );
        let color_down_layout = layout(
            "misty colour history mips",
            &[
                entry(1, texture(false)),
                entry(3, storage(IMAGE_FORMAT)),
                entry(4, texture(false)),
            ],
        );

        let module = |label, src: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            })
        };
        let ssgi = module("misty ssgi", format!("{COMMON_SRC}\n{SSGI_SRC}"));
        let ssr = module("misty ssr", format!("{COMMON_SRC}\n{SSR_SRC}"));
        let compose = module(
            "misty trace compose",
            format!("{COMMON_SRC}\n{COMPOSE_SRC}"),
        );
        let mips = module("misty trace mips", MIPS_SRC.to_owned());
        let pipeline_layout = |l: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("misty screen trace"),
                bind_group_layouts: &[Some(l)],
                immediate_size: 0,
            })
        };
        let main_layout = pipeline_layout(&main);
        let compute = |layout: &wgpu::PipelineLayout,
                       module: &wgpu::ShaderModule,
                       entry: &str,
                       constants: &[(&str, f64)]| {
            crate::render::pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                module,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants,
                    ..Default::default()
                },
                cache: None,
            })
        };
        let step = |s: f64| compute(&main_layout, &ssgi, "cs_gi_denoise", &[("STEP", s)]);
        let pipelines = vec![
            (
                Kernel::GiTrace,
                compute(&main_layout, &ssgi, "cs_gi_trace", &[]),
            ),
            (
                Kernel::GiTemporal,
                compute(&main_layout, &ssgi, "cs_gi_temporal", &[]),
            ),
            (Kernel::GiDenoise1, step(1.0)),
            (Kernel::GiDenoise2, step(2.0)),
            (Kernel::GiDenoise4, step(4.0)),
            (
                Kernel::SsrTrace,
                compute(&main_layout, &ssr, "cs_ssr_trace", &[]),
            ),
            (
                Kernel::SsrTemporal,
                compute(&main_layout, &ssr, "cs_ssr_temporal", &[]),
            ),
            (
                Kernel::SsrBlur,
                compute(&main_layout, &ssr, "cs_ssr_blur", &[]),
            ),
            (
                Kernel::Compose,
                compute(&main_layout, &compose, "cs_compose", &[]),
            ),
            (
                Kernel::HizCopy,
                compute(
                    &pipeline_layout(&hiz_copy_layout),
                    &mips,
                    "cs_hiz_copy",
                    &[],
                ),
            ),
            (
                Kernel::HizDown,
                compute(
                    &pipeline_layout(&hiz_down_layout),
                    &mips,
                    "cs_hiz_down",
                    &[],
                ),
            ),
            (
                Kernel::ColorDown,
                compute(
                    &pipeline_layout(&color_down_layout),
                    &mips,
                    "cs_color_down",
                    &[],
                ),
            ),
        ];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty screen trace"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let image = |label, usage| image(device, label, (1, 1), IMAGE_FORMAT, 1, usage);
        let sampled = wgpu::TextureUsages::TEXTURE_BINDING;
        let store = wgpu::TextureUsages::STORAGE_BINDING;
        Self {
            layout: main,
            hiz_copy_layout,
            hiz_down_layout,
            color_down_layout,
            pipelines,
            sampler,
            dummy_in: view(&image("misty trace placeholder", sampled)),
            dummy_out: [
                view(&image("misty trace placeholder out", store)),
                view(&image("misty trace placeholder out", store)),
            ],
            dummy_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty trace placeholder"),
                size: buffer_bytes(1),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }

    pub(super) fn dispatch(&self, pass: &mut wgpu::ComputePass, step: &Step) {
        let Some((_, pipeline)) = self.pipelines.iter().find(|(k, _)| *k == step.kernel) else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &step.group, &[]);
        pass.dispatch_workgroups(step.size.0.div_ceil(8), step.size.1.div_ceil(8), 1);
    }
}

fn image(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
    mips: u32,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn view(t: &wgpu::Texture) -> wgpu::TextureView {
    t.create_view(&Default::default())
}

/// Single-mip views of every level.
fn mip_views(t: &wgpu::Texture) -> Vec<wgpu::TextureView> {
    (0..t.mip_level_count())
        .map(|level| {
            t.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        })
        .collect()
}

impl TargetState {
    /// Resources of a view for `key`; `bytes` = texel buffer size.
    pub(super) fn new(device: &wgpu::Device, key: Key, bytes: u64) -> Self {
        let full = key.full;
        let rw = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING;
        let scaled = |s: u32| (full.0.div_ceil(s), full.1.div_ceil(s));
        let levels = |(w, h): (u32, u32)| 32 - w.max(h).leading_zeros();
        let make = |label, size| view(&image(device, label, size, IMAGE_FORMAT, 1, rw));
        let color = image(
            device,
            "misty trace colour history",
            full,
            IMAGE_FORMAT,
            levels(full).min(MAX_COLOR_MIPS),
            rw | wgpu::TextureUsages::COPY_DST,
        );
        let gi_size = key.gi.map_or((1, 1), scaled);
        let gi = key.gi.map(|_| GiImages {
            trace: make("misty ssgi trace", gi_size),
            accum: [
                make("misty ssgi accumulation", gi_size),
                make("misty ssgi accumulation", gi_size),
            ],
            info: [
                make("misty ssgi history info", gi_size),
                make("misty ssgi history info", gi_size),
            ],
            blur: [
                make("misty ssgi denoise", gi_size),
                make("misty ssgi denoise", gi_size),
            ],
        });
        let ssr_size = key.ssr.map_or((1, 1), scaled);
        let ssr = key.ssr.map(|_| {
            let hiz = image(device, "misty hiz", full, HIZ_FORMAT, levels(full), rw);
            SsrImages {
                trace: make("misty ssr trace", ssr_size),
                history: [
                    make("misty ssr history", ssr_size),
                    make("misty ssr history", ssr_size),
                ],
                out: make("misty ssr", ssr_size),
                hiz_all: view(&hiz),
                hiz_mips: mip_views(&hiz),
            }
        });
        Self {
            key,
            gi_size,
            ssr_size,
            color_all: view(&color),
            color_mips: mip_views(&color),
            color,
            gi,
            ssr,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty screen trace params"),
                size: std::mem::size_of::<TraceParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            texels: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty screen trace texels"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            frame: 0,
            history: None,
            color_valid: false,
        }
    }
}

pub(super) fn tex(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

pub(super) fn buf(binding: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: b.as_entire_binding(),
    }
}
