//! Cloud compute passes: the shadow map, the reduced-resolution trace and the temporal
//! resolve, plus the per-view images they write.

const COMMON: &str = include_str!("shaders/cloud_common.wgsl");
const TRACE_SRC: &str = include_str!("shaders/cloud_trace.wgsl");
const TEMPORAL_SRC: &str = include_str!("shaders/cloud_temporal.wgsl");
const SHADOW_SRC: &str = include_str!("shaders/cloud_shadow.wgsl");
/// Layer 0 = rgb light + a transmittance, layer 1 = r distance (km).
pub(super) const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(super) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Full source of a cloud compute shader.
pub(super) fn source(body: &str) -> String {
    format!("{COMMON}\n{body}")
}

/// Every compute shader, for validation tests.
#[cfg(test)]
pub(super) fn sources() -> [(&'static str, String); 3] {
    [
        ("cloud trace", source(TRACE_SRC)),
        ("cloud temporal", source(TEMPORAL_SRC)),
        ("cloud shadow", source(SHADOW_SRC)),
    ]
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

fn texture(dim: wgpu::TextureViewDimension) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: dim,
        multisampled: false,
    }
}

fn storage(format: wgpu::TextureFormat, dim: wgpu::TextureViewDimension) -> wgpu::BindingType {
    wgpu::BindingType::StorageTexture {
        access: wgpu::StorageTextureAccess::WriteOnly,
        format,
        view_dimension: dim,
    }
}

fn uniform() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn sampler() -> wgpu::BindingType {
    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
}

/// Uniform, the noise textures and their sampler (bindings 0–5), then `output` at 6.
fn noise_layout(
    device: &wgpu::Device,
    label: &str,
    output: wgpu::BindingType,
) -> wgpu::BindGroupLayout {
    use wgpu::TextureViewDimension as D;
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            entry(0, uniform()),
            entry(1, texture(D::D3)),
            entry(2, texture(D::D3)),
            entry(3, texture(D::D2)),
            entry(4, texture(D::D2)),
            entry(5, sampler()),
            entry(6, output),
        ],
    })
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
    crate::render::pipelines::note_pipeline();
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(name),
        layout: Some(&layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: crate::render::pipeline_cache(device).as_ref(),
    })
}

pub(super) struct CloudPipelines {
    trace_layout: wgpu::BindGroupLayout,
    shadow_layout: wgpu::BindGroupLayout,
    temporal_layout: wgpu::BindGroupLayout,
    trace: wgpu::ComputePipeline,
    shadow: wgpu::ComputePipeline,
    temporal: wgpu::ComputePipeline,
}

/// Everything one render's cloud passes read and write.
pub(super) struct Dispatch<'a> {
    pub uniform: &'a wgpu::Buffer,
    pub noise: &'a super::noise::NoiseTextures,
    pub repeat: &'a wgpu::Sampler,
    pub clamp: &'a wgpu::Sampler,
    pub raw: &'a wgpu::TextureView,
    pub history: &'a wgpu::TextureView,
    pub resolved: &'a wgpu::TextureView,
    pub trace: (u32, u32),
    /// The shadow map and its side, when clouds cast shadows.
    pub shadow: Option<(&'a wgpu::TextureView, u32)>,
}

impl CloudPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::TextureViewDimension as D;
        let trace_layout = noise_layout(
            device,
            "misty cloud trace",
            storage(IMAGE_FORMAT, D::D2Array),
        );
        let shadow_layout =
            noise_layout(device, "misty cloud shadow", storage(SHADOW_FORMAT, D::D2));
        let temporal_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty cloud temporal"),
            entries: &[
                entry(0, uniform()),
                entry(6, texture(D::D2Array)),
                entry(7, texture(D::D2Array)),
                entry(8, sampler()),
                entry(9, storage(IMAGE_FORMAT, D::D2Array)),
            ],
        });
        Self {
            trace: compute(device, "misty cloud trace", TRACE_SRC, &trace_layout),
            shadow: compute(device, "misty cloud shadow", SHADOW_SRC, &shadow_layout),
            temporal: compute(
                device,
                "misty cloud temporal",
                TEMPORAL_SRC,
                &temporal_layout,
            ),
            trace_layout,
            shadow_layout,
            temporal_layout,
        }
    }

    fn noise_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        d: &Dispatch,
        output: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let tex = wgpu::BindingResource::TextureView;
        let n = d.noise;
        let resources = [
            d.uniform.as_entire_binding(),
            tex(&n.base),
            tex(&n.detail),
            tex(&n.weather),
            tex(&n.blue),
            wgpu::BindingResource::Sampler(d.repeat),
            tex(output),
        ];
        let entries: Vec<_> = resources
            .into_iter()
            .enumerate()
            .map(|(i, resource)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource,
            })
            .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty cloud group"),
            layout,
            entries: &entries,
        })
    }

    /// Shadow map (when on), trace into `raw`, then resolve `raw` + `history` into `resolved`.
    pub fn encode(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, d: &Dispatch) {
        let tex = wgpu::BindingResource::TextureView;
        let trace_group = self.noise_group(device, &self.trace_layout, d, d.raw);
        let shadow_group = d
            .shadow
            .map(|(view, size)| (self.noise_group(device, &self.shadow_layout, d, view), size));
        let temporal_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty cloud temporal group"),
            layout: &self.temporal_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: d.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: tex(d.raw),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: tex(d.history),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::Sampler(d.clamp),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: tex(d.resolved),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty volumetric clouds"),
            timestamp_writes: None,
        });
        if let Some((group, size)) = &shadow_group {
            pass.set_pipeline(&self.shadow);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(size.div_ceil(8), size.div_ceil(8), 1);
        }
        let (w, h) = d.trace;
        pass.set_pipeline(&self.trace);
        pass.set_bind_group(0, &trace_group, &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        pass.set_pipeline(&self.temporal);
        pass.set_bind_group(0, &temporal_group, &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
    }
}

/// A two-layer cloud image (sampled + written by compute).
pub(super) fn image(device: &wgpu::Device, label: &str, size: (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
}

/// The cloud shadow map.
pub(super) fn shadow_map(device: &wgpu::Device, size: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("misty cloud shadow map"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Per-view images: this frame's trace and the two resolved images (ping-pong history).
pub(super) struct CloudTarget {
    pub size: (u32, u32),
    raw: wgpu::TextureView,
    resolved: [wgpu::TextureView; 2],
    /// Which of `resolved` this render writes (and the forward shaders read).
    parity: usize,
    pub frame: u32,
    pub prev_view_proj: Option<glam::Mat4>,
    pub last_used: u64,
}

impl CloudTarget {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        Self {
            size,
            raw: image(device, "misty cloud trace", size),
            resolved: [
                image(device, "misty clouds a", size),
                image(device, "misty clouds b", size),
            ],
            parity: 0,
            frame: 0,
            prev_view_proj: None,
            last_used: 0,
        }
    }

    pub fn raw(&self) -> &wgpu::TextureView {
        &self.raw
    }

    /// (history = last render, resolved = this render).
    pub fn pair(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        (&self.resolved[1 - self.parity], &self.resolved[self.parity])
    }

    pub fn parity(&self) -> usize {
        self.parity
    }

    pub fn advance(&mut self) {
        self.parity = 1 - self.parity;
        self.frame = self.frame.wrapping_add(1);
    }

    pub fn bytes(&self) -> u64 {
        3 * 2 * 8 * self.size.0 as u64 * self.size.1 as u64
    }
}
