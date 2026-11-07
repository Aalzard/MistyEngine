//! Grass culling on the GPU (shaders/grass_cull.wgsl). Every chunk a grass pass draws is a
//! "job": a compute pass tests its blades once (outside the view, or a flat fan the pass
//! skips) and writes the ones that can show, in draw order, into a compact index list with
//! the count in the job's indirect draw arguments. The passes then draw each chunk with
//! `draw_indirect`, the vertex stage fetching its blade through the list (grass_blade.wgsl
//! `grass_fetch`). Before, every blade of a chunk's prefix went through the ten vertices of
//! its card in every pass, only to be thrown away there: near the camera most of them (the
//! chunk around the camera is half behind it, and most of its blades are the flat fans the
//! depth, prepass and shadow passes skip).
//!
//! The tests read a compact copy of each blade (root, height, key: 16 bytes) from one arena
//! buffer per terrain, so one dispatch covers every chunk.

use std::collections::{BTreeMap, HashMap};

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::terrain::details::Blade;

const SRC: &str = include_str!("shaders/grass_cull.wgsl");
/// Blades per workgroup (grass_cull.wgsl `GROUP`).
const GROUP: u32 = 256;
/// Arena allocations are rounded up to this many records.
const GRAIN: u32 = 64;

/// The passes that draw grass: the lit grass, the grass depth pass, the AO prepass, up to
/// four sun cascades and the two slices of the near grass shadow map.
pub(super) const KINDS: usize = 9;
pub(super) const LIT: usize = 0;
pub(super) const DEPTH: usize = 1;
pub(super) const PREPASS: usize = 2;
pub(super) const CASCADE: usize = 3;
pub(super) const NEAR: usize = 7;

/// One pass's view of the grass (grass_cull.wgsl `Kind`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(super) struct Kind {
    pub view_proj: [[f32; 4]; 4],
    pub origin: [f32; 4],
    pub camera: [f32; 4],
    pub lod: [f32; 4],
    pub shadow: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Job {
    base: u32,
    count: u32,
    first_wg: u32,
    first_lane: u32,
    lanes: u32,
    pad: [u32; 3],
}

/// A pass drawing a job's chunk (grass_cull.wgsl `Lane`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Lane {
    kind: u32,
    count: u32,
    args: u32,
    pad: u32,
    card: [u32; 2],
    pad1: [u32; 2],
}

/// The cull record of a blade (grass_cull.wgsl `records`).
pub(super) fn record(b: &Blade) -> [u32; 4] {
    let key = if b.extra[0] >= 2.0 {
        b.extra[0] - 2.0
    } else {
        b.extra[0]
    };
    let height = (b.position[3].max(0.0) * 1000.0).ceil().min(65535.0) as u32;
    let key = (key.clamp(0.0, 1.0) * 65535.0).floor() as u32;
    [
        b.position[0].to_bits(),
        b.position[1].to_bits(),
        b.position[2].to_bits(),
        (height << 16) | key.min(65535),
    ]
}

/// The three compute pipelines (shared by every terrain).
pub(super) struct CullPipelines {
    count: wgpu::ComputePipeline,
    scan: wgpu::ComputePipeline,
    write: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

impl CullPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty grass cull"),
            source: wgpu::ShaderSource::Wgsl(SRC.into()),
        });
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty grass cull"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(2, true),
                storage(3, true),
                storage(4, false),
                storage(5, false),
                storage(6, false),
                storage(7, true),
                storage(8, true),
                storage(9, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty grass cull"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            super::pipelines::note_pipeline();
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
            count: pipeline("cs_count"),
            scan: pipeline("cs_scan"),
            write: pipeline("cs_write"),
            layout,
        }
    }
}

/// The cull records of every chunk of a terrain, in one storage buffer (first-fit free list;
/// grows by copying into a buffer twice as big).
pub(super) struct Arena {
    pub buffer: wgpu::Buffer,
    /// In records.
    capacity: u32,
    /// Free runs: start → length (records).
    free: BTreeMap<u32, u32>,
    /// Bumped when the buffer is replaced (bind groups that use it are rebuilt).
    pub generation: u64,
}

const RECORD: u64 = 16;

fn arena_buffer(device: &wgpu::Device, records: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty grass cull records"),
        size: records as u64 * RECORD,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

impl Arena {
    pub fn new(device: &wgpu::Device) -> Self {
        let capacity = 1 << 18;
        Self {
            buffer: arena_buffer(device, capacity),
            capacity,
            free: BTreeMap::from([(0, capacity)]),
            generation: 0,
        }
    }

    pub fn bytes(&self) -> u64 {
        self.capacity as u64 * RECORD
    }

    /// Room for `n` records (rounded up to the grain); grows the buffer when nothing fits.
    /// None when the device can't hold a buffer that big.
    fn alloc(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, n: u32) -> Option<u32> {
        let n = n.max(1).next_multiple_of(GRAIN);
        if let Some(at) = take_run(&mut self.free, n) {
            return Some(at);
        }
        // Grow: at least twice as big, copying what is there.
        let limit = device
            .limits()
            .max_storage_buffer_binding_size
            .min(device.limits().max_buffer_size)
            / RECORD;
        let want = (self.capacity as u64 * 2).max(self.capacity as u64 + n as u64);
        if self.capacity as u64 + n as u64 > limit {
            return None;
        }
        let capacity = (want.min(limit) as u32) / GRAIN * GRAIN;
        let buffer = arena_buffer(device, capacity);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty grass cull records grow"),
        });
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &buffer, 0, self.buffer.size());
        queue.submit([encoder.finish()]);
        let old = self.capacity;
        self.buffer = buffer;
        self.capacity = capacity;
        self.generation += 1;
        self.release(old, capacity - old);
        self.alloc(device, queue, n)
    }

    /// Give back `n` records at `at` (as allocated).
    fn release(&mut self, at: u32, n: u32) {
        release_run(&mut self.free, at, n.max(1).next_multiple_of(GRAIN));
    }

    /// Store the records of `blades`; their offset (None: the arena can't grow).
    pub fn store(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        blades: &[Blade],
    ) -> Option<u32> {
        let at = self.alloc(device, queue, blades.len() as u32)?;
        let records: Vec<[u32; 4]> = blades.iter().map(record).collect();
        queue.write_buffer(
            &self.buffer,
            at as u64 * RECORD,
            bytemuck::cast_slice(&records),
        );
        Some(at)
    }

    pub fn free(&mut self, at: u32, n: u32) {
        self.release(at, n);
    }
}

/// First fit: the start of a free run of `n` records, taken out of `free`.
fn take_run(free: &mut BTreeMap<u32, u32>, n: u32) -> Option<u32> {
    let (at, len) = free
        .iter()
        .find(|(_, len)| **len >= n)
        .map(|(a, l)| (*a, *l))?;
    free.remove(&at);
    if len > n {
        free.insert(at + n, len - n);
    }
    Some(at)
}

/// Put `n` records at `at` back into `free`, merged with the runs either side.
fn release_run(free: &mut BTreeMap<u32, u32>, at: u32, n: u32) {
    let (mut start, mut len) = (at, n);
    if let Some((&p, &pl)) = free.range(..at).next_back()
        && p + pl == at
    {
        free.remove(&p);
        start = p;
        len += pl;
    }
    if let Some(nl) = free.remove(&(at + n)) {
        len += nl;
    }
    free.insert(start, len);
}

/// A storage buffer that grows to the next power of two (`generation` bumps when replaced).
struct Grow {
    buffer: wgpu::Buffer,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl Grow {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        let usage = usage | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 256,
                usage,
                mapped_at_creation: false,
            }),
            label,
            usage,
        }
    }

    /// Room for `bytes`; true when the buffer was replaced (its contents are not kept).
    fn reserve(&mut self, device: &wgpu::Device, bytes: u64) -> bool {
        if bytes <= self.buffer.size() {
            return false;
        }
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: bytes.next_power_of_two(),
            usage: self.usage,
            mapped_at_creation: false,
        });
        true
    }
}

/// A chunk drawn by a pass this frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct JobDraw {
    pub key: (u32, u32),
    /// Index of the job (its indirect arguments).
    pub job: u32,
    /// Blades of the chunk's prefix the job tests (the most it can draw).
    pub count: u32,
    pub verts: u32,
    pub first_vertex: u32,
}

/// What a planned pass draws: its kind's tests and its chunks in draw order (with each
/// chunk's arena offset).
pub(super) struct PlannedKind {
    pub kind: Kind,
    pub draws: Vec<(JobDraw, u32)>,
}

/// One terrain's culling this frame: the jobs of every pass, their buffers and bind group.
pub(super) struct CullFrame {
    frame: wgpu::Buffer,
    kinds: wgpu::Buffer,
    jobs: Grow,
    lanes: Grow,
    wg_job: Grow,
    masks: Grow,
    offsets: Grow,
    indices: Grow,
    args: Grow,
    group: Option<wgpu::BindGroup>,
    /// Bumped when the index buffer is replaced (the chunks' draw groups are rebuilt).
    pub indices_generation: u64,
    /// The chunks each pass draws, in order.
    pub per_kind: [Vec<JobDraw>; KINDS],
    workgroups: u32,
}

impl CullFrame {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            frame: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty grass cull frame"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            kinds: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty grass cull kinds"),
                size: (std::mem::size_of::<Kind>() * KINDS) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            jobs: Grow::new(device, "misty grass cull jobs", wgpu::BufferUsages::empty()),
            lanes: Grow::new(
                device,
                "misty grass cull lanes",
                wgpu::BufferUsages::empty(),
            ),
            wg_job: Grow::new(
                device,
                "misty grass cull groups",
                wgpu::BufferUsages::empty(),
            ),
            masks: Grow::new(
                device,
                "misty grass cull masks",
                wgpu::BufferUsages::empty(),
            ),
            offsets: Grow::new(
                device,
                "misty grass cull offsets",
                wgpu::BufferUsages::empty(),
            ),
            indices: Grow::new(
                device,
                "misty grass drawn blades",
                wgpu::BufferUsages::empty(),
            ),
            args: Grow::new(
                device,
                "misty grass draw args",
                wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC,
            ),
            group: None,
            indices_generation: 0,
            per_kind: Default::default(),
            workgroups: 0,
        }
    }

    /// This frame's passes draw through culled lists (else every blade of the prefix).
    pub fn culled(&self) -> bool {
        self.group.is_some()
    }

    pub fn indices(&self) -> &wgpu::Buffer {
        &self.indices.buffer
    }

    pub fn args(&self) -> &wgpu::Buffer {
        &self.args.buffer
    }

    pub fn bytes(&self) -> u64 {
        [
            &self.jobs,
            &self.lanes,
            &self.wg_job,
            &self.masks,
            &self.offsets,
            &self.indices,
            &self.args,
        ]
        .iter()
        .map(|b| b.buffer.size())
        .sum()
    }

    /// Lay out this frame's jobs (`planned[k]` = pass k's tests and chunks, None = not drawn)
    /// and upload them.
    pub fn plan(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &CullPipelines,
        arena: Option<&Arena>,
        planned: &[Option<PlannedKind>; KINDS],
        gpu: bool,
    ) {
        let mut kinds = [Kind::zeroed(); KINDS];
        // One job per chunk, with a lane for every pass that draws it.
        let mut by_chunk: HashMap<(u32, u32), usize> = HashMap::new();
        let mut chunk_jobs: Vec<(u32, u32, Vec<Lane>)> = Vec::new();
        let mut args = 0u32;
        let mut drawn = 0u64;
        for (k, p) in planned.iter().enumerate() {
            self.per_kind[k].clear();
            let Some(p) = p else {
                continue;
            };
            kinds[k] = p.kind;
            for (d, base) in &p.draws {
                if d.count == 0 {
                    continue;
                }
                let mut d = *d;
                d.job = args;
                let at = *by_chunk.entry(d.key).or_insert_with(|| {
                    chunk_jobs.push((*base, 0, Vec::new()));
                    chunk_jobs.len() - 1
                });
                let job = &mut chunk_jobs[at];
                job.1 = job.1.max(d.count);
                job.2.push(Lane {
                    kind: k as u32,
                    count: d.count,
                    args,
                    pad: 0,
                    card: [d.verts, d.first_vertex],
                    pad1: [0; 2],
                });
                args += 1;
                drawn += d.count as u64;
                self.per_kind[k].push(d);
            }
        }
        self.workgroups = 0;
        let Some(arena) = arena.filter(|_| gpu && !chunk_jobs.is_empty()) else {
            self.group = None;
            return;
        };
        let mut jobs: Vec<Job> = Vec::with_capacity(chunk_jobs.len());
        let mut lanes: Vec<Lane> = Vec::with_capacity(args as usize);
        let mut wg_job: Vec<u32> = Vec::new();
        let mut rows = 0;
        for (base, count, job_lanes) in chunk_jobs {
            let wgs = count.div_ceil(GROUP);
            wg_job.extend(std::iter::repeat_n(jobs.len() as u32, wgs as usize));
            jobs.push(Job {
                base,
                count,
                first_wg: wg_job.len() as u32 - wgs,
                first_lane: lanes.len() as u32,
                lanes: job_lanes.len() as u32,
                pad: [0; 3],
            });
            rows = rows.max(job_lanes.len() as u32);
            lanes.extend(job_lanes);
        }
        let wg = wg_job.len() as u32;
        self.workgroups = wg;
        self.jobs
            .reserve(device, std::mem::size_of_val(&jobs[..]) as u64);
        self.lanes
            .reserve(device, std::mem::size_of_val(&lanes[..]) as u64);
        self.wg_job.reserve(device, wg as u64 * 4);
        self.offsets
            .reserve(device, (wg as u64 * rows as u64 + 1) * 4);
        self.masks.reserve(device, wg as u64 * rows as u64 * 8 * 4);
        self.args.reserve(device, args as u64 * 16);
        if self.indices.reserve(device, drawn * 4) {
            self.indices_generation += 1;
        }
        queue.write_buffer(
            &self.frame,
            0,
            bytemuck::cast_slice(&[jobs.len() as u32, wg, rows, 0]),
        );
        queue.write_buffer(&self.kinds, 0, bytemuck::cast_slice(&kinds));
        queue.write_buffer(&self.jobs.buffer, 0, bytemuck::cast_slice(&jobs));
        queue.write_buffer(&self.lanes.buffer, 0, bytemuck::cast_slice(&lanes));
        queue.write_buffer(&self.wg_job.buffer, 0, bytemuck::cast_slice(&wg_job));
        // (Made every frame: buffers may have been replaced, the arena may have grown; it's
        // one small group.)
        fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty grass cull group"),
            layout: &pipelines.layout,
            entries: &[
                entry(0, &self.frame),
                entry(1, &self.kinds),
                entry(2, &self.jobs.buffer),
                entry(3, &arena.buffer),
                entry(4, &self.offsets.buffer),
                entry(5, &self.indices.buffer),
                entry(6, &self.args.buffer),
                entry(7, &self.lanes.buffer),
                entry(8, &self.wg_job.buffer),
                entry(9, &self.masks.buffer),
            ],
        }));
    }

    /// Blades drawn per pass after culling (blocking readback, for tests) and before.
    #[cfg(test)]
    pub fn read_counts(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> [(u64, u64); KINDS] {
        let jobs: usize = self.per_kind.iter().map(Vec::len).sum();
        let size = (jobs as u64 * 16).max(16).min(self.args.buffer.size());
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty grass args readback"),
            size,
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
        let mut out = [(0, 0); KINDS];
        for (k, draws) in self.per_kind.iter().enumerate() {
            for d in draws {
                out[k].0 += words[d.job as usize * 4 + 1] as u64;
                out[k].1 += d.count as u64;
            }
        }
        out
    }

    /// Record the culling (before the frame's first grass pass).
    pub fn encode(&self, pipelines: &CullPipelines, encoder: &mut wgpu::CommandEncoder) {
        let Some(group) = &self.group else {
            return;
        };
        if self.workgroups == 0 {
            return;
        }
        let x = self.workgroups.min(65535);
        let y = self.workgroups.div_ceil(65535);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty grass culling"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, group, &[]);
        pass.set_pipeline(&pipelines.count);
        pass.dispatch_workgroups(x, y, 1);
        pass.set_pipeline(&pipelines.scan);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_pipeline(&pipelines.write);
        pass.dispatch_workgroups(x, y, 1);
    }
}

/// A pass's kind from its grass uniform values (the same numbers the vertex stage reads).
#[allow(clippy::too_many_arguments)]
pub(super) fn kind(
    view_proj: &Mat4,
    origin: [f32; 4],
    camera: [f32; 4],
    look_x: f32,
    draw_distance: f32,
    density: f32,
    pixel: f32,
    shadow: [f32; 4],
    near_radius: f32,
    camera_pass: bool,
    skip_flat: bool,
) -> Kind {
    Kind {
        view_proj: view_proj.to_cols_array_2d(),
        origin: [
            origin[0],
            origin[1],
            origin[2],
            if camera_pass { 1.0 } else { 0.0 },
        ],
        camera: [camera[0], camera[1], camera[2], look_x],
        lod: [
            draw_distance,
            if skip_flat { 1.0 } else { 0.0 },
            density,
            pixel,
        ],
        shadow: [shadow[0], shadow[1], shadow[2], near_radius],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(SRC)
            .unwrap_or_else(|e| panic!("grass_cull.wgsl: {}", e.emit_to_string(SRC)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("grass_cull.wgsl: {e:?}"));
    }

    #[test]
    fn records_round_conservatively() {
        let mut b = Blade::zeroed();
        b.position = [1.5, -2.0, 3.25, 0.4321];
        b.extra[0] = 2.0 + 0.7;
        let r = record(&b);
        assert_eq!(f32::from_bits(r[0]), 1.5);
        assert_eq!(f32::from_bits(r[2]), 3.25);
        // Height rounds up (a wider reach), the key down (a blade is only left out when its
        // real key is past the share for sure).
        assert!((r[3] >> 16) as f32 * 0.001 >= 0.4321);
        assert!((r[3] & 0xffff) as f32 / 65535.0 <= 0.7);
    }

    #[test]
    fn arena_reuses_and_merges_free_runs() {
        let mut free = BTreeMap::from([(0u32, 1024u32)]);
        let x = take_run(&mut free, 128).unwrap();
        let y = take_run(&mut free, 64).unwrap();
        let z = take_run(&mut free, 64).unwrap();
        assert_eq!((x, y, z), (0, 128, 192));
        assert_eq!(take_run(&mut free, 1024), None);
        release_run(&mut free, y, 64);
        // The hole is reused first.
        assert_eq!(take_run(&mut free, 64), Some(128));
        release_run(&mut free, 128, 64);
        release_run(&mut free, x, 128);
        release_run(&mut free, z, 64);
        assert_eq!(free, BTreeMap::from([(0, 1024)]));
    }
}
