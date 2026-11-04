//! GPU side of the FFT ocean: the initial spectrum (from `water::waves`), the compute passes
//! and the displacement / slope textures (with mips) the water shader samples.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::water::waves::{FFT_SIZE, Spectrum, SpectrumParams};

const N: u32 = FFT_SIZE as u32;
pub const MIPS: u32 = 8;
pub const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FftUniform {
    a: [f32; 4],
    b: [f32; 4],
}

/// One simulation step's inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FftStep {
    pub time: f32,
    pub choppiness: f32,
    pub delta: f32,
    /// 0..1: how readily crests foam.
    pub crest_foam: f32,
}

pub struct Fft {
    params: wgpu::Buffer,
    h0: wgpu::Buffer,
    spectrum: Option<Arc<Spectrum>>,
    group: wgpu::BindGroup,
    spectrum_pipeline: wgpu::ComputePipeline,
    rows: wgpu::ComputePipeline,
    columns: wgpu::ComputePipeline,
    finalize: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    mip_groups: Vec<wgpu::BindGroup>,
    pub displacement: wgpu::TextureView,
    pub slope: wgpu::TextureView,
    /// Time of the last simulated step (several views a frame share one).
    last: Option<FftStep>,
    pending: bool,
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn image_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: TEXTURE_FORMAT,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

fn sampled_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn compute(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    entry: &str,
) -> wgpu::ComputePipeline {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&pl),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

impl Fft {
    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let cells = (N * N) as u64;
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let params = buffer(
            "misty fft params",
            std::mem::size_of::<FftUniform>() as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let h0 = buffer("misty fft h0", cells * 16, storage);
        let work = buffer("misty fft work", cells * 32, storage);
        let foam = buffer("misty fft foam", cells * 4, storage);
        let texture = |label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: N,
                    height: N,
                    depth_or_array_layers: 1,
                },
                mip_level_count: MIPS,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TEXTURE_FORMAT,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let displacement = texture("misty ocean displacement");
        let slope = texture("misty ocean slope");
        let mip = |t: &wgpu::Texture, level: u32| {
            t.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty fft"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_entry(1, true),
                storage_entry(2, false),
                storage_entry(3, false),
                image_entry(4),
                image_entry(5),
            ],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty fft"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: h0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: work.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: foam.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&mip(&displacement, 0)),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&mip(&slope, 0)),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty water fft"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/water_fft.wgsl").into()),
        });
        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty fft mips"),
            entries: &[
                sampled_entry(0),
                sampled_entry(1),
                image_entry(2),
                image_entry(3),
            ],
        });
        let mip_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty water mips"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/water_mips.wgsl").into()),
        });
        let mip_groups = (1..MIPS)
            .map(|level| {
                let views = [
                    mip(&displacement, level - 1),
                    mip(&slope, level - 1),
                    mip(&displacement, level),
                    mip(&slope, level),
                ];
                let entries: Vec<wgpu::BindGroupEntry> = views
                    .iter()
                    .enumerate()
                    .map(|(i, v)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: wgpu::BindingResource::TextureView(v),
                    })
                    .collect();
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("misty fft mip"),
                    layout: &mip_layout,
                    entries: &entries,
                })
            })
            .collect();
        Self {
            spectrum_pipeline: compute(device, &module, &layout, "spectrum"),
            rows: compute(device, &module, &layout, "fft_rows"),
            columns: compute(device, &module, &layout, "fft_columns"),
            finalize: compute(device, &module, &layout, "finalize"),
            downsample: compute(device, &mip_module, &mip_layout, "downsample"),
            mip_groups,
            params,
            h0,
            spectrum: None,
            group,
            displacement: displacement.create_view(&Default::default()),
            slope: slope.create_view(&Default::default()),
            last: None,
            pending: false,
        }
    }

    /// Tile size of the current spectrum (m).
    pub fn tile(&self) -> f32 {
        self.spectrum.as_ref().map_or(100.0, |s| s.tile)
    }

    /// Upload a new spectrum when the wind changed and queue a step for `step.time`.
    pub fn prepare(&mut self, queue: &wgpu::Queue, params: SpectrumParams, step: FftStep) {
        if self.spectrum.as_ref().is_none_or(|s| s.params != params) {
            let s = crate::water::waves::spectrum(params);
            queue.write_buffer(&self.h0, 0, bytemuck::cast_slice(&s.h0));
            self.spectrum = Some(s);
            self.last = None;
        }
        if self.last.is_some_and(|l| l.time == step.time && l == step) {
            return;
        }
        let delta = self
            .last
            .map_or(0.0, |l| (step.time - l.time).clamp(0.0, 0.5));
        let u = FftUniform {
            a: [step.time, self.tile(), step.choppiness, delta],
            // Folding starts below a Jacobian of 1; more crest foam raises the threshold.
            b: [0.35 + 0.55 * step.crest_foam, 2.5, 2.5, 0.0],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&u));
        self.last = Some(step);
        self.pending = true;
    }

    /// Record the compute passes of the queued step (once per step).
    pub fn record(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !std::mem::take(&mut self.pending) {
            return;
        }
        crate::profile_scope!("Water.FFT");
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty ocean fft"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.group, &[]);
        let tiles = N.div_ceil(8);
        pass.set_pipeline(&self.spectrum_pipeline);
        pass.dispatch_workgroups(tiles, tiles, 1);
        pass.set_pipeline(&self.rows);
        pass.dispatch_workgroups(N, 1, 1);
        pass.set_pipeline(&self.columns);
        pass.dispatch_workgroups(N, 1, 1);
        pass.set_pipeline(&self.finalize);
        pass.dispatch_workgroups(tiles, tiles, 1);
        pass.set_pipeline(&self.downsample);
        for (i, group) in self.mip_groups.iter().enumerate() {
            let size = (N >> (i + 1)).max(1);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(size.div_ceil(8), size.div_ceil(8), 1);
        }
    }
}
