//! GPU-driven culling of the opaque main-pass draws, in two phases so nothing pops:
//!
//! 1. Draws that were visible last frame (and are inside the frustum) are drawn first.
//! 2. Their depth becomes a Hi-Z pyramid; every candidate is then tested against the frustum
//!    and the pyramid with this frame's camera. Visible ones not drawn yet are drawn now, and
//!    the result is remembered for the next frame.
//!
//! The CPU only groups candidates into batches (like instancing does); a compute pass picks
//! each batch's visible instances and writes them, in order, with their count into indirect
//! draw arguments. Batches sharing shader, textures and vertex buffers are issued with one
//! `multi_draw_indexed_indirect` (wgpu loops where the API has no native multi-draw).
//! Shadow maps and transparent draws keep the CPU path.

mod frame;
mod hiz;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

pub(super) use hiz::TargetState;

use super::batch::Batch;
use super::culling::Aabb;

const CULL_SRC: &str = include_str!("cull.wgsl");
const WORKGROUP: u32 = 64;

/// Device features GPU culling needs (indirect draws that start at an instance offset).
pub fn supported(device: &wgpu::Device) -> bool {
    device
        .features()
        .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CullUniform {
    view_proj: [[f32; 4]; 4],
    planes: [[f32; 4]; 6],
    info: [u32; 4],
    size: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BatchInfo {
    first: u32,
    count: u32,
    out_a: u32,
    out_b: u32,
}

/// One frame's GPU-culled opaque batches.
pub(super) struct CulledBatches {
    /// Batch layout (`first`/`count` index the candidates; `draw` is a member's index).
    pub batches: Vec<Batch>,
    /// Candidate draw indices, batch after batch.
    pub candidates: Vec<u32>,
    /// Where in the slots buffer the two phases write (each `candidates.len()` long).
    pub out_a: u32,
    pub out_b: u32,
}

impl CulledBatches {
    /// Slots-buffer entries the phases need after `base`.
    pub fn slots_needed(&self) -> usize {
        2 * self.candidates.len()
    }

    /// Byte offset of batch `i`'s indirect arguments in `phase` (0 or 1).
    pub fn args_offset(&self, phase: usize, i: usize) -> u64 {
        ((phase * self.batches.len() + i)
            * std::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>()) as u64
    }
}

/// Compute pipelines and the per-frame buffers (grown as needed).
pub(super) struct GpuCuller {
    cull: wgpu::ComputePipeline,
    compact: wgpu::ComputePipeline,
    hiz: hiz::HizPipelines,
    uniforms: [wgpu::Buffer; 2],
    bounds: Buffer,
    candidates: Buffer,
    flags: Buffer,
    batches: Buffer,
    pub args: Buffer,
}

/// A storage buffer that grows to the next power of two.
pub(super) struct Buffer {
    pub buffer: wgpu::Buffer,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl Buffer {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        let usage = usage | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: 256,
            usage,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            label,
            usage,
        }
    }

    /// Write `data`, growing first when it doesn't fit.
    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) {
        let size = (data.len() as u64).max(16);
        if size > self.buffer.size() {
            self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: size.next_power_of_two(),
                usage: self.usage,
                mapped_at_creation: false,
            });
        }
        if !data.is_empty() {
            queue.write_buffer(&self.buffer, 0, data);
        }
    }
}

impl GpuCuller {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty cull"),
            source: wgpu::ShaderSource::Wgsl(CULL_SRC.into()),
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
        let uniform = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty cull uniform"),
                size: std::mem::size_of::<CullUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let none = wgpu::BufferUsages::empty();
        Self {
            cull: pipeline("cs_cull"),
            compact: pipeline("cs_compact"),
            hiz: hiz::HizPipelines::new(device),
            uniforms: [uniform(), uniform()],
            bounds: Buffer::new(device, "misty cull bounds", none),
            candidates: Buffer::new(device, "misty cull candidates", none),
            flags: Buffer::new(device, "misty cull flags", none),
            batches: Buffer::new(device, "misty cull batches", none),
            args: Buffer::new(
                device,
                "misty indirect args",
                wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC,
            ),
        }
    }

    /// Upload one frame's inputs: per-draw bounds, candidates, batch table, arguments.
    #[allow(clippy::too_many_arguments)]
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        state: &mut TargetState,
        culled: &CulledBatches,
        bounds: &[Option<Aabb>],
        ranges: &[(u32, u32, i32)],
        view_proj: &Mat4,
        size: (u32, u32),
        occlusion: bool,
    ) {
        let boxes: Vec<[f32; 8]> = bounds
            .iter()
            .map(|b| match b {
                Some(b) => [
                    b.min.x, b.min.y, b.min.z, 0.0, b.max.x, b.max.y, b.max.z, 0.0,
                ],
                None => [0.0; 8],
            })
            .collect();
        self.bounds
            .write(device, queue, bytemuck::cast_slice(&boxes));
        self.candidates
            .write(device, queue, bytemuck::cast_slice(&culled.candidates));
        let zeros = vec![0u32; culled.candidates.len()];
        self.flags
            .write(device, queue, bytemuck::cast_slice(&zeros));
        let infos: Vec<BatchInfo> = culled
            .batches
            .iter()
            .map(|b| BatchInfo {
                first: b.first,
                count: b.count,
                out_a: culled.out_a + b.first,
                out_b: culled.out_b + b.first,
            })
            .collect();
        self.batches
            .write(device, queue, bytemuck::cast_slice(&infos));
        let mut args: Vec<wgpu::util::DrawIndexedIndirectArgs> = Vec::new();
        for phase in [culled.out_a, culled.out_b] {
            for (b, (count, first_index, base_vertex)) in culled.batches.iter().zip(ranges) {
                args.push(wgpu::util::DrawIndexedIndirectArgs {
                    index_count: *count,
                    instance_count: 0,
                    first_index: *first_index,
                    base_vertex: *base_vertex,
                    first_instance: phase + b.first,
                });
            }
        }
        let bytes: Vec<u8> = args.iter().flat_map(|a| a.as_bytes().to_vec()).collect();
        self.args.write(device, queue, &bytes);
        state.ensure_draws(device, queue, bounds.len());

        let planes = super::culling::Frustum::from_matrix(view_proj).planes();
        for phase in 0..2u32 {
            let u = CullUniform {
                view_proj: view_proj.to_cols_array_2d(),
                planes: planes.map(|p| p.to_array()),
                info: [
                    culled.candidates.len() as u32,
                    phase,
                    if occlusion { state.mips() } else { 0 },
                    culled.batches.len() as u32,
                ],
                size: [size.0 as f32, size.1 as f32, 0.0, 0.0],
            };
            queue.write_buffer(&self.uniforms[phase as usize], 0, bytemuck::bytes_of(&u));
        }
    }

    /// Cull and compact one phase (0 = last frame's visible set, 1 = the rest).
    pub fn dispatch(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        state: &TargetState,
        culled: &CulledBatches,
        slots: &wgpu::Buffer,
        phase: usize,
    ) {
        let n = culled.candidates.len() as u32;
        if n == 0 {
            return;
        }
        fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        let cull_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty cull group"),
            layout: &self.cull.get_bind_group_layout(0),
            entries: &[
                entry(0, &self.uniforms[phase]),
                entry(1, &self.bounds.buffer),
                entry(2, &self.candidates.buffer),
                entry(3, state.visible_last()),
                entry(4, &self.flags.buffer),
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(state.pyramid_view()),
                },
            ],
        });
        let compact_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty compact group"),
            layout: &self.compact.get_bind_group_layout(0),
            entries: &[
                entry(0, &self.uniforms[phase]),
                entry(2, &self.candidates.buffer),
                entry(4, &self.flags.buffer),
                entry(6, &self.batches.buffer),
                entry(7, slots),
                entry(8, &self.args.buffer),
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty gpu culling"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.cull);
        pass.set_bind_group(0, &cull_group, &[]);
        pass.dispatch_workgroups(n.div_ceil(WORKGROUP), 1, 1);
        pass.set_pipeline(&self.compact);
        pass.set_bind_group(0, &compact_group, &[]);
        pass.dispatch_workgroups((culled.batches.len() as u32).div_ceil(WORKGROUP), 1, 1);
    }

    /// Build the Hi-Z pyramid from the target's depth (after the first phase).
    pub fn build_pyramid(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        state: &TargetState,
        depth: &wgpu::TextureView,
    ) {
        self.hiz.build(device, encoder, state, depth);
    }

    /// Instances drawn by each phase (blocking readback, for tests).
    #[cfg(test)]
    pub fn read_counts(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: usize,
    ) -> [u32; 2] {
        let size = (2 * batches * 20) as u64;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty args readback"),
            size: size.max(4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.args.buffer, 0, &read, 0, size);
        queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("mapped");
        let words: &[u32] = bytemuck::cast_slice(&data[..size as usize]);
        let mut out = [0; 2];
        for (i, a) in words.chunks(5).enumerate() {
            out[i / batches.max(1)] += a[1];
        }
        out
    }
}
