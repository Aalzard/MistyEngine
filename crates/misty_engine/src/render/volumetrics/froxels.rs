//! Froxel volume compute passes: light (density + lighting + temporal) into a per-view history
//! texture, then front-to-back integration into the shared volume the forward shaders sample.

const COMMON: &str = include_str!("shaders/froxel_common.wgsl");
pub(super) const LIGHT_SRC: &str = include_str!("shaders/froxel_light.wgsl");
pub(super) const INTEGRATE_SRC: &str = include_str!("shaders/froxel_integrate.wgsl");
/// rgb = scattering / in-scatter, a = extinction / transmittance.
const VOLUME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Full source of a froxel compute shader.
pub(super) fn source(body: &str) -> String {
    format!("{COMMON}\n{body}")
}

pub(super) struct FroxelPipelines {
    light_layout: wgpu::BindGroupLayout,
    integrate_layout: wgpu::BindGroupLayout,
    light: wgpu::ComputePipeline,
    integrate: wgpu::ComputePipeline,
    shadow_sampler: wgpu::Sampler,
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

fn texture(
    sample_type: wgpu::TextureSampleType,
    dim: wgpu::TextureViewDimension,
) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type,
        view_dimension: dim,
        multisampled: false,
    }
}

fn storage_3d() -> wgpu::BindingType {
    wgpu::BindingType::StorageTexture {
        access: wgpu::StorageTextureAccess::WriteOnly,
        format: VOLUME_FORMAT,
        view_dimension: wgpu::TextureViewDimension::D3,
    }
}

fn compute(
    device: &wgpu::Device,
    name: &str,
    body: &str,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(source(body).into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(name),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(name),
        layout: Some(&layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    })
}

/// Resources of one froxel dispatch.
pub(super) struct Dispatch<'a> {
    pub uniform: &'a wgpu::Buffer,
    pub sampler: &'a wgpu::Sampler,
    pub history: &'a wgpu::TextureView,
    pub lit: &'a wgpu::TextureView,
    pub integrated: &'a wgpu::TextureView,
    pub shadows: (&'a wgpu::TextureView, &'a wgpu::TextureView),
    pub size: [u32; 3],
}

impl FroxelPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::TextureSampleType as T;
        use wgpu::TextureViewDimension as D;
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let float = T::Float { filterable: true };
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty froxel light"),
            entries: &[
                entry(0, uniform),
                entry(1, texture(float, D::D3)),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(3, storage_3d()),
                entry(4, texture(T::Depth, D::D2)),
                entry(5, texture(T::Depth, D::D2Array)),
                entry(
                    6,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
            ],
        });
        let integrate_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty froxel integrate"),
            entries: &[
                entry(0, uniform),
                entry(1, texture(float, D::D3)),
                entry(2, storage_3d()),
            ],
        });
        Self {
            light: compute(device, "misty froxel light", LIGHT_SRC, &light_layout),
            integrate: compute(
                device,
                "misty froxel integrate",
                INTEGRATE_SRC,
                &integrate_layout,
            ),
            light_layout,
            integrate_layout,
            shadow_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty froxel shadow sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                compare: Some(wgpu::CompareFunction::LessEqual),
                ..Default::default()
            }),
        }
    }

    /// Light the froxels (reading last frame's `history`) into `lit`, then integrate `lit` into
    /// `integrated`.
    pub fn encode(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, d: &Dispatch) {
        let tex = wgpu::BindingResource::TextureView;
        let light_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty froxel light group"),
            layout: &self.light_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: d.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: tex(d.history),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(d.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: tex(d.lit),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: tex(d.shadows.0),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: tex(d.shadows.1),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
            ],
        });
        let integrate_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty froxel integrate group"),
            layout: &self.integrate_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: d.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: tex(d.lit),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: tex(d.integrated),
                },
            ],
        });
        let [w, h, n] = d.size;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty volumetric fog"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.light);
        pass.set_bind_group(0, &light_group, &[]);
        pass.dispatch_workgroups(w.div_ceil(4), h.div_ceil(4), n.div_ceil(4));
        pass.set_pipeline(&self.integrate);
        pass.set_bind_group(0, &integrate_group, &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
    }
}

/// A 3D froxel texture (sampled + written by compute).
pub(super) fn volume_texture(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 3],
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: size[2],
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: VOLUME_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Bytes of one volume of `size`.
pub(super) fn volume_bytes(size: [u32; 3]) -> u64 {
    8 * size.iter().map(|v| *v as u64).product::<u64>()
}

/// Per-view lit froxels: written this frame, read back as history next frame.
pub(super) struct History {
    pub size: [u32; 3],
    views: [wgpu::TextureView; 2],
    /// Which of `views` is written this frame.
    parity: usize,
    pub frame: u32,
    pub prev: Option<super::uniforms::PrevView>,
}

impl History {
    pub fn new(device: &wgpu::Device, size: [u32; 3]) -> Self {
        Self {
            size,
            views: [
                volume_texture(device, "misty froxel history a", size),
                volume_texture(device, "misty froxel history b", size),
            ],
            parity: 0,
            frame: 0,
            prev: None,
        }
    }

    /// (read = last frame, write = this frame).
    pub fn pair(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        (&self.views[1 - self.parity], &self.views[self.parity])
    }

    pub fn advance(&mut self) {
        self.parity = 1 - self.parity;
        self.frame = self.frame.wrapping_add(1);
    }
}
