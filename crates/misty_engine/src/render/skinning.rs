//! GPU skinning: one compute dispatch per changed [`crate::skinning::SkinPose`] writes the posed
//! vertices into a buffer that is also a vertex buffer, so every pass (shadows, depth prepass,
//! main, outlines) draws the deformed mesh through the ordinary [`GpuMesh`] path. Static poses
//! are not re-skinned; meshes that aren't skinned never reach this module.

use std::collections::HashMap;
use std::sync::Arc;

use wgpu::util::DeviceExt;

use super::{GpuMesh, culling, mesh};
use crate::models;
use crate::skinning::{self, SkinPose};

const SRC: &str = include_str!("shaders/skin.wgsl");
const WORKGROUP: u32 = 64;
/// Frames (renders) an unused pose's buffers are kept.
const KEEP_RENDERS: u64 = 600;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    vertex_count: u32,
    joint_count: u32,
    shape_count: u32,
    _pad: u32,
}

/// Bind-pose data of one source mesh, shared by every renderer using it.
struct Base {
    src: wgpu::Buffer,
    influences: wgpu::Buffer,
    deltas: wgpu::Buffer,
    indices: wgpu::Buffer,
    vertex_count: u32,
    index_count: u32,
    shape_count: u32,
}

/// One posed renderer: its output vertices and the inputs of its dispatch.
struct Instance {
    mesh: GpuMesh,
    base: Arc<Base>,
    source: String,
    params: wgpu::Buffer,
    matrices: wgpu::Buffer,
    joint_capacity: usize,
    weights: wgpu::Buffer,
    group: wgpu::BindGroup,
    version: Option<u64>,
    used: u64,
}

#[derive(Default)]
pub(super) struct Skins {
    pipeline: Option<wgpu::ComputePipeline>,
    bases: HashMap<String, Option<Arc<Base>>>,
    instances: HashMap<String, Instance>,
    generation: u64,
    renders: u64,
    /// Dispatches recorded by the last [`Skins::prepare`] (tests, stats).
    pub dispatched: usize,
}

fn storage(
    device: &wgpu::Device,
    label: &str,
    bytes: &[u8],
    extra: wgpu::BufferUsages,
) -> wgpu::Buffer {
    // Bindings can't be empty.
    let mut data = bytes.to_vec();
    if data.len() < 16 {
        data.resize(16, 0);
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &data,
        usage: wgpu::BufferUsages::STORAGE | extra,
    })
}

impl Base {
    fn new(device: &wgpu::Device, key: &str) -> Option<Self> {
        let (model, i) = skinning::model_mesh(&crate::scene::MeshRef::Asset(key.to_owned()))?;
        let m = &model.meshes[i];
        if m.data.indices.is_empty() {
            return None;
        }
        let n = m.data.vertices.len();
        let mut influences: Vec<[u32; 4]> = Vec::with_capacity(n * 2);
        for v in 0..n {
            let (j, w) = match &m.skin {
                Some(s) => (s.joints[v], s.weights[v]),
                None => ([0; 4], [1.0, 0.0, 0.0, 0.0]),
            };
            influences.push(j.map(u32::from));
            influences.push(w.map(f32::to_bits));
        }
        let mut deltas: Vec<f32> = Vec::with_capacity(m.blend_shapes.len() * n * 6);
        for s in &m.blend_shapes {
            for v in 0..n {
                deltas.extend(s.positions.get(v).copied().unwrap_or_default());
                deltas.extend(s.normals.get(v).copied().unwrap_or_default());
            }
        }
        let copy = wgpu::BufferUsages::empty();
        Some(Self {
            src: storage(device, key, bytemuck::cast_slice(&m.data.vertices), copy),
            influences: storage(device, key, bytemuck::cast_slice(&influences), copy),
            deltas: storage(device, key, bytemuck::cast_slice(&deltas), copy),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(key),
                contents: bytemuck::cast_slice(&m.data.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            vertex_count: n as u32,
            index_count: m.data.indices.len() as u32,
            shape_count: m.blend_shapes.len() as u32,
        })
    }
}

impl Skins {
    fn pipeline(&mut self, device: &wgpu::Device) -> &wgpu::ComputePipeline {
        self.pipeline.get_or_insert_with(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("misty skinning"),
                source: wgpu::ShaderSource::Wgsl(SRC.into()),
            });
            crate::render::pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("misty skinning"),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        })
    }

    /// The posed mesh drawn for `key` (after [`Skins::prepare`] this render).
    pub(super) fn get(&self, key: &str) -> Option<&GpuMesh> {
        self.instances.get(key).map(|i| &i.mesh)
    }

    /// Bytes of the posed vertex buffers (stats).
    pub(super) fn bytes(&self) -> u64 {
        self.instances.values().map(|i| i.mesh.bytes()).sum()
    }

    /// Make the GPU copies of the poses in `keys` current and skin the ones that changed.
    pub(super) fn prepare<'k>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        keys: impl Iterator<Item = &'k str>,
    ) {
        self.dispatched = 0;
        self.renders += 1;
        let generation = models::generation();
        if generation != self.generation {
            self.generation = generation;
            self.bases.clear();
            self.instances.clear();
        }
        let mut todo: Vec<String> = Vec::new();
        for key in keys {
            if todo.iter().any(|k| k == key) {
                continue;
            }
            let Some(pose) = skinning::pose(key) else {
                continue;
            };
            if self.update(device, queue, key, &pose) {
                todo.push(key.to_owned());
            }
        }
        let renders = self.renders;
        self.instances
            .retain(|_, i| renders.saturating_sub(i.used) < KEEP_RENDERS);
        if todo.is_empty() {
            return;
        }
        self.pipeline(device);
        let pipeline = self.pipeline.as_ref().expect("created above");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty skinning"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("misty skinning"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            for key in &todo {
                let Some(i) = self.instances.get(key) else {
                    continue;
                };
                pass.set_bind_group(0, &i.group, &[]);
                pass.dispatch_workgroups(i.base.vertex_count.div_ceil(WORKGROUP), 1, 1);
                self.dispatched += 1;
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Upload `pose` for `key`; true if it needs skinning this render.
    fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &str,
        pose: &SkinPose,
    ) -> bool {
        let Some(source) = pose_source(pose) else {
            return false;
        };
        let base = match self.bases.get(&source) {
            Some(b) => b.clone(),
            None => {
                let b = Base::new(device, &source).map(Arc::new);
                self.bases.insert(source.clone(), b.clone());
                b
            }
        };
        let Some(base) = base else {
            return false;
        };
        let joints = pose.matrices.len();
        let rebuild = self.instances.get(key).is_none_or(|i| {
            i.source != source || !Arc::ptr_eq(&i.base, &base) || i.joint_capacity < joints
        });
        if rebuild {
            self.pipeline(device);
            let instance = Instance::new(
                device,
                self.pipeline.as_ref().expect("made"),
                &base,
                &source,
                joints,
            );
            self.instances.insert(key.to_owned(), instance);
        }
        let renders = self.renders;
        let i = self.instances.get_mut(key).expect("inserted above");
        i.used = renders;
        let (lo, hi) = pose.bounds;
        i.mesh.bounds = culling::Aabb::new(lo, hi);
        if i.version == Some(pose.version) {
            return false;
        }
        i.version = Some(pose.version);
        let params = Params {
            vertex_count: base.vertex_count,
            joint_count: joints as u32,
            shape_count: base.shape_count.min(pose.weights.len() as u32),
            _pad: 0,
        };
        queue.write_buffer(&i.params, 0, bytemuck::bytes_of(&params));
        if joints > 0 {
            queue.write_buffer(&i.matrices, 0, bytemuck::cast_slice(&pose.matrices));
        }
        if !pose.weights.is_empty() {
            queue.write_buffer(&i.weights, 0, bytemuck::cast_slice(&pose.weights));
        }
        true
    }
}

/// Mesh key ("path#index") of a pose's source mesh.
fn pose_source(pose: &SkinPose) -> Option<String> {
    match &pose.mesh {
        crate::scene::MeshRef::Asset(k) if !skinning::is_pose_key(k) => Some(k.clone()),
        _ => None,
    }
}

impl Instance {
    fn new(
        device: &wgpu::Device,
        pipeline: &wgpu::ComputePipeline,
        base: &Arc<Base>,
        source: &str,
        joints: usize,
    ) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
        let n = base.vertex_count as usize;
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty skinned vertices"),
            size: (n * std::mem::size_of::<mesh::Vertex>()).max(16) as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty skinning params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let joint_capacity = joints.max(1).next_power_of_two();
        let matrices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty skin matrices"),
            size: (joint_capacity * 64) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let weights = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty blend shape weights"),
            size: (base.shape_count.max(4) as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty skinning"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                entry(0, &uniform),
                entry(1, &base.src),
                entry(2, &base.influences),
                entry(3, &base.deltas),
                entry(4, &weights),
                entry(5, &matrices),
                entry(6, &output),
            ],
        });
        Self {
            mesh: GpuMesh {
                vertices: output,
                indices: base.indices.clone(),
                buffer_id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                first_index: 0,
                base_vertex: 0,
                count: base.index_count,
                vertex_count: base.vertex_count,
                bounds: culling::Aabb::new(glam::Vec3::ZERO, glam::Vec3::ZERO),
            },
            base: base.clone(),
            source: source.to_owned(),
            params: uniform,
            matrices,
            joint_capacity,
            weights,
            group,
            version: None,
            used: 0,
        }
    }
}

#[cfg(test)]
impl Skins {
    /// The posed vertices of `key` read back from the GPU.
    pub(super) fn read_vertices(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &str,
    ) -> Option<Vec<mesh::Vertex>> {
        let i = self.instances.get(key)?;
        let size = i.mesh.vertex_count as u64 * std::mem::size_of::<mesh::Vertex>() as u64;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty skin readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&i.mesh.vertices, 0, &read, 0, size);
        queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().ok()?;
        Some(bytemuck::cast_slice(&data).to_vec())
    }
}

#[cfg(test)]
#[path = "skinning_gpu_tests.rs"]
mod gpu_tests;
