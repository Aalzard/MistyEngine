//! Per-target GPU culling state: the Hi-Z pyramid (R32Float, farthest depth per texel) and
//! which draws were visible last frame.

const HIZ_SRC: &str = include_str!("hiz.wgsl");
const HIZ_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

pub(in crate::render) struct HizPipelines {
    level0: wgpu::ComputePipeline,
    down: wgpu::ComputePipeline,
}

impl HizPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty hiz"),
            source: wgpu::ShaderSource::Wgsl(HIZ_SRC.into()),
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            level0: pipeline("cs_level0"),
            down: pipeline("cs_down"),
        }
    }

    /// Fill every level from the MSAA depth `depth`.
    pub fn build(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        state: &TargetState,
        depth: &wgpu::TextureView,
    ) {
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty hiz"),
            timestamp_writes: None,
        });
        for (i, level) in state.levels.iter().enumerate() {
            let (pipeline, src) = match i {
                0 => (&self.level0, texture(0, depth)),
                _ => (&self.down, texture(1, &state.levels[i - 1])),
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty hiz group"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[src, texture(2, level)],
            });
            let (w, h) = state.level_size(i);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
    }
}

pub(in crate::render) struct TargetState {
    size: (u32, u32),
    pyramid: wgpu::TextureView,
    /// One view per mip level.
    levels: Vec<wgpu::TextureView>,
    visible_last: wgpu::Buffer,
    draws: usize,
}

impl TargetState {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let base = (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1));
        let mips = 32 - base.0.max(base.1).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty hiz"),
            size: wgpu::Extent3d {
                width: base.0,
                height: base.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HIZ_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels = (0..mips)
            .map(|m| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: m,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self {
            size: base,
            pyramid: texture.create_view(&Default::default()),
            levels,
            visible_last: visible_buffer(device, 1),
            draws: 0,
        }
    }

    /// Does this state belong to a target of `size`?
    pub fn fits(&self, size: (u32, u32)) -> bool {
        self.size == (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1))
    }

    pub fn mips(&self) -> u32 {
        self.levels.len() as u32
    }

    fn level_size(&self, i: usize) -> (u32, u32) {
        let mut s = self.size;
        for _ in 0..i {
            s = (s.0.div_ceil(2).max(1), s.1.div_ceil(2).max(1));
        }
        s
    }

    pub fn pyramid_view(&self) -> &wgpu::TextureView {
        &self.pyramid
    }

    pub fn visible_last(&self) -> &wgpu::Buffer {
        &self.visible_last
    }

    /// Room for `n` draws; a new or resized list starts as "all visible".
    pub fn ensure_draws(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, n: usize) {
        if n == self.draws {
            return;
        }
        if (n * 4) as u64 > self.visible_last.size() {
            self.visible_last = visible_buffer(device, n);
        }
        queue.write_buffer(
            &self.visible_last,
            0,
            bytemuck::cast_slice(&vec![1u32; n.max(1)]),
        );
        self.draws = n;
    }
}

fn visible_buffer(device: &wgpu::Device, n: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty visible last frame"),
        size: (n.max(1) * 4).next_power_of_two() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
