//! Terrain trees culled on the GPU (shaders/trees_cull.wgsl), like the grass
//! (`grass_cull.rs`): every placed tree of a terrain lives in a persistent storage buffer,
//! uploaded once when the terrain's trees change. Each frame one compute dispatch tests every
//! tree against the camera and the sun's cascades, picks its level of detail and appends it to
//! the instance list of its (view, prototype, level); the lists' counts are the instance counts
//! of their indirect draw arguments. The passes draw each list with one indirect draw, so the
//! CPU cost no longer grows with the number of trees (before: a per-tree loop with hash map
//! pushes, and every visible tree's instance uploaded, each frame).
//!
//! Used with cascaded shadows; the virtual shadow map's page views and near layer keep the CPU
//! lists (trees.rs).
//!
//! Occlusion: the camera's lists also skip trees hidden behind last frame's depth (a Hi-Z
//! pyramid of the target's depth after the main pass, `TreeHiz`), while the camera has
//! hardly moved since: inside a forest most trees past the first rows are behind trunks and
//! crowns. The cascades' lists are never occlusion culled.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::culling::Frustum;

const SRC: &str = include_str!("shaders/trees_cull.wgsl");
/// Trees per workgroup (trees_cull.wgsl `cull`).
const GROUP: u32 = 256;
/// The camera and up to four cascades.
pub(super) const MAX_VIEWS: usize = 1 + super::cascades::MAX_CASCADES;
/// u32 per list in the arguments buffer: two indexed draws (bark, leaves) of five.
const ARGS_PER_LIST: u32 = 10;
/// Most trees of one prototype in the camera's full-mesh list and in a cascade's lists
/// (more are dropped: far beyond what the distances let through).
const NEAR_CAP: u32 = 8192;
const SHADOW_CAP: u32 = 16384;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    camera: [f32; 4],
    lod: [f32; 4],
    counts: [u32; 4],
    /// x = Hi-Z mip levels (0 = no occlusion test), yz = the target's size (pixels).
    occlusion: [f32; 4],
    /// The view-projection the Hi-Z pyramid's depth was drawn with.
    hiz_view_proj: [[f32; 4]; 4],
}

/// Last frame's depth as a Hi-Z pyramid (per target), for the camera's occlusion test.
pub(in crate::render) struct TreeHiz {
    pub state: super::gpu_culling::TargetState,
    pub view_proj: Mat4,
    pub camera: Vec3,
    pub forward: Vec3,
}

impl TreeHiz {
    /// Is the pyramid usable for a frame seen from `camera` looking along `forward` at a
    /// target of `size`? (Only while the view has hardly changed: trees it hid may show now.)
    pub fn usable(&self, size: (u32, u32), camera: Vec3, forward: Vec3) -> bool {
        self.state.fits(size) && self.camera.distance(camera) < 1.0 && self.forward.dot(forward) > 0.998
    }
}

/// The occlusion test's inputs for a frame.
pub(in crate::render) struct TreeOcclusion<'a> {
    pub pyramid: &'a wgpu::TextureView,
    pub view_proj: Mat4,
    pub mips: u32,
    pub size: (u32, u32),
}

/// One placed tree (trees_cull.wgsl `Tree`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(super) struct TreeGpu {
    pub foot: [f32; 4],
    pub scale: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub(super) struct ProtoGpu {
    /// x = bounding sphere centre height, y = radius (unscaled), z = 1 when it draws, w = its
    /// distance factor (trees.rs `size_factor`).
    pub bounds: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewGpu {
    planes: [[f32; 4]; 6],
    misc: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ListGpu {
    base: u32,
    capacity: u32,
    args: u32,
    mesh: u32,
}

/// Where a list lives: its first instance, room and first u32 of its draw arguments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ListInfo {
    pub base: u32,
    pub capacity: u32,
    pub args: u32,
}

impl ListInfo {
    /// Byte offset of the list's arguments (`leaves` = the second, indexed record).
    pub fn args_offset(&self, leaves: bool) -> u64 {
        (self.args + if leaves { 5 } else { 0 }) as u64 * 4
    }
}

/// Index of the list of (`view`, `proto`, `lod`).
pub(super) fn list_index(protos: usize, view: usize, proto: usize, lod: usize) -> usize {
    (view * protos + proto) * 3 + lod
}

/// The compute pipelines (one per renderer).
pub(super) struct CullPipelines {
    layout: wgpu::BindGroupLayout,
    hiz_layout: wgpu::BindGroupLayout,
    /// Bound as the pyramid when there is none (occlusion test off).
    no_hiz: wgpu::TextureView,
    cull: wgpu::ComputePipeline,
    clamp: wgpu::ComputePipeline,
}

impl CullPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty tree cull"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, storage(true)),
                entry(2, storage(true)),
                entry(3, storage(true)),
                entry(4, storage(true)),
                entry(5, storage(false)),
                entry(6, storage(false)),
            ],
        });
        let hiz_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty tree cull hiz"),
            entries: &[entry(
                0,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )],
        });
        let no_hiz = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("misty tree cull no hiz"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty tree cull"),
            source: wgpu::ShaderSource::Wgsl(SRC.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty tree cull"),
            bind_group_layouts: &[Some(&layout), Some(&hiz_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            super::pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("misty tree cull"),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: crate::render::pipeline_cache(device).as_ref(),
            })
        };
        Self {
            cull: pipeline("cull"),
            clamp: pipeline("clamp_counts"),
            layout,
            hiz_layout,
            no_hiz,
        }
    }
}

fn buffer(device: &wgpu::Device, label: &str, bytes: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.max(16),
        usage,
        mapped_at_creation: false,
    })
}

/// A staging copy of the draw arguments (the lists' instance counts, read back for the
/// statistics a frame or two late, never waited for).
struct Readback {
    buffer: wgpu::Buffer,
    /// 0 = free, 1 = copied this frame, 2 = mapping, 3 = mapped.
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

/// One terrain's trees on the GPU: the placed trees, the frame's lists and their arguments.
pub(super) struct TerrainCull {
    /// The terrain data version and position the trees were uploaded for.
    pub version: u64,
    pub position: Vec3,
    count: u32,
    protos: usize,
    pub lists: Vec<ListInfo>,
    params: wgpu::Buffer,
    proto_buf: wgpu::Buffer,
    views: wgpu::Buffer,
    pub instances: wgpu::Buffer,
    pub args: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// This frame's Hi-Z pyramid (or the stand-in when the test is off).
    hiz_group: Option<wgpu::BindGroup>,
    readback: Vec<Readback>,
    /// The latest instance count of every list that has arrived (statistics).
    pub counts: Vec<u32>,
}

impl TerrainCull {
    /// Upload `trees` (prototype in `scale.w`) of a terrain with `protos` prototypes.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &CullPipelines,
        trees: &[TreeGpu],
        protos: usize,
        version: u64,
        position: Vec3,
    ) -> Self {
        use wgpu::BufferUsages as U;
        let mut per_proto = vec![0u32; protos];
        for t in trees {
            if let Some(n) = per_proto.get_mut(t.scale[3] as usize) {
                *n += 1;
            }
        }
        let mut lists = Vec::with_capacity(MAX_VIEWS * protos * 3);
        let mut base = 0u32;
        for view in 0..MAX_VIEWS {
            for &n in &per_proto {
                for lod in 0..3 {
                    let capacity = match (view, lod) {
                        (0, 0) => n.min(NEAR_CAP),
                        (0, _) => n,
                        _ => n.min(SHADOW_CAP),
                    };
                    let args = lists.len() as u32 * ARGS_PER_LIST;
                    lists.push(ListInfo {
                        base,
                        capacity,
                        args,
                    });
                    base += capacity;
                }
            }
        }
        let list_gpu: Vec<ListGpu> = lists
            .iter()
            .enumerate()
            .map(|(i, l)| ListGpu {
                base: l.base,
                capacity: l.capacity,
                args: l.args,
                mesh: u32::from(i % 3 != 2),
            })
            .collect();
        let tree_buf = buffer(
            device,
            "misty tree cull trees",
            std::mem::size_of_val(trees) as u64,
            U::STORAGE | U::COPY_DST,
        );
        queue.write_buffer(&tree_buf, 0, bytemuck::cast_slice(trees));
        let list_buf = buffer(
            device,
            "misty tree cull lists",
            std::mem::size_of_val(list_gpu.as_slice()) as u64,
            U::STORAGE | U::COPY_DST,
        );
        queue.write_buffer(&list_buf, 0, bytemuck::cast_slice(&list_gpu));
        let params = buffer(
            device,
            "misty tree cull params",
            std::mem::size_of::<Params>() as u64,
            U::UNIFORM | U::COPY_DST,
        );
        let proto_buf = buffer(
            device,
            "misty tree cull prototypes",
            (protos.max(1) * std::mem::size_of::<ProtoGpu>()) as u64,
            U::STORAGE | U::COPY_DST,
        );
        let views = buffer(
            device,
            "misty tree cull views",
            (MAX_VIEWS * std::mem::size_of::<ViewGpu>()) as u64,
            U::STORAGE | U::COPY_DST,
        );
        let instances = buffer(
            device,
            "misty tree cull instances",
            base.max(1) as u64 * std::mem::size_of::<TreeGpu>() as u64,
            U::STORAGE | U::VERTEX,
        );
        let args = buffer(
            device,
            "misty tree cull args",
            (lists.len().max(1) as u32 * ARGS_PER_LIST) as u64 * 4,
            U::STORAGE | U::INDIRECT | U::COPY_DST | U::COPY_SRC,
        );
        fn bind(binding: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: b.as_entire_binding(),
            }
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty tree cull"),
            layout: &pipelines.layout,
            entries: &[
                bind(0, &params),
                bind(1, &tree_buf),
                bind(2, &proto_buf),
                bind(3, &views),
                bind(4, &list_buf),
                bind(5, &instances),
                bind(6, &args),
            ],
        });
        Self {
            version,
            position,
            count: trees.len() as u32,
            protos,
            params,
            proto_buf,
            views,
            instances,
            args,
            group,
            hiz_group: None,
            readback: Vec::new(),
            counts: vec![0; lists.len()],
            lists,
        }
    }

    /// Copy this frame's arguments into a free staging buffer (after the cull dispatches).
    pub fn copy_counts(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        use std::sync::atomic::Ordering;
        if self.count == 0 {
            return;
        }
        let free = (self.readback.iter()).position(|r| r.state.load(Ordering::Acquire) == 0);
        let i = match free {
            Some(i) => i,
            // Three in flight at most: skip a frame rather than wait.
            None if self.readback.len() >= 3 => return,
            None => {
                self.readback.push(Readback {
                    buffer: buffer(
                        device,
                        "misty tree cull counts readback",
                        self.args.size(),
                        wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    ),
                    state: Default::default(),
                });
                self.readback.len() - 1
            }
        };
        let r = &self.readback[i];
        encoder.copy_buffer_to_buffer(&self.args, 0, &r.buffer, 0, self.args.size());
        r.state.store(1, Ordering::Release);
    }

    /// After the frame's submit: start mapping what it copied.
    pub fn counts_submitted(&self) {
        use std::sync::atomic::Ordering;
        for r in &self.readback {
            if r.state.load(Ordering::Acquire) == 1 {
                r.state.store(2, Ordering::Release);
                let state = r.state.clone();
                r.buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                    state.store(if result.is_ok() { 3 } else { 0 }, Ordering::Release);
                });
            }
        }
    }

    /// Take the newest counts that have arrived into `counts`.
    pub fn collect_counts(&mut self) {
        use std::sync::atomic::Ordering;
        for r in &self.readback {
            if r.state.load(Ordering::Acquire) != 3 {
                continue;
            }
            if let Ok(data) = r.buffer.slice(..).get_mapped_range() {
                let args: &[u32] = bytemuck::cast_slice(&data[..]);
                for (c, l) in self.counts.iter_mut().zip(&self.lists) {
                    *c = args.get(l.args as usize + 1).copied().unwrap_or(0);
                }
            }
            r.buffer.unmap();
            r.state.store(0, Ordering::Release);
        }
    }

    /// GPU bytes of the buffers (Memory module).
    pub fn bytes(&self) -> u64 {
        self.instances.size() + self.args.size() + self.count as u64 * 32
    }

    /// This frame's inputs: the camera, its view-projection, the LOD distances (full mesh end,
    /// impostor start, draw distance), the cascades (view-projection, level of detail) and
    /// their reach, the prototypes' bounds, and each list's draw arguments with zero
    /// instances (`mesh_ranges(proto, lod)` = bark and leaves index ranges of a mesh level).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &CullPipelines,
        occlusion: Option<&TreeOcclusion>,
        camera: Vec3,
        view_proj: &Mat4,
        lod: [f32; 3],
        cascades: &[(Mat4, u8, f32)],
        reach: f32,
        protos: &[ProtoGpu],
        mesh_ranges: impl Fn(usize, usize) -> [std::ops::Range<u32>; 2],
    ) {
        let views_n = (1 + cascades.len()).min(MAX_VIEWS);
        let params = Params {
            camera: camera.extend(self.count as f32).to_array(),
            lod: [lod[0], lod[1], lod[2], reach],
            counts: [views_n as u32, self.protos as u32, self.lists.len() as u32, 0],
            occlusion: match occlusion {
                Some(o) => [o.mips as f32, o.size.0 as f32, o.size.1 as f32, 0.0],
                None => [0.0; 4],
            },
            hiz_view_proj: occlusion.map_or(Mat4::IDENTITY, |o| o.view_proj).to_cols_array_2d(),
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        self.hiz_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty tree cull hiz"),
            layout: &pipelines.hiz_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    occlusion.map_or(&pipelines.no_hiz, |o| o.pyramid),
                ),
            }],
        }));
        let view = |m: &Mat4, lod: f32, texel: f32| ViewGpu {
            planes: Frustum::from_matrix(m).planes().map(|p| p.to_array()),
            misc: [lod, texel, 0.0, 0.0],
        };
        let mut views = vec![view(view_proj, -1.0, 0.0)];
        views.extend(
            cascades
                .iter()
                .take(MAX_VIEWS - 1)
                .map(|(m, l, texel)| view(m, *l as f32, *texel)),
        );
        queue.write_buffer(&self.views, 0, bytemuck::cast_slice(&views));
        let mut p = protos.to_vec();
        p.resize(self.protos.max(1), ProtoGpu::default());
        queue.write_buffer(&self.proto_buf, 0, bytemuck::cast_slice(&p));
        let mut args = vec![0u32; self.lists.len() * ARGS_PER_LIST as usize];
        for (i, l) in self.lists.iter().enumerate() {
            let (proto, lod) = ((i / 3) % self.protos.max(1), i % 3);
            let a = &mut args[l.args as usize..(l.args + ARGS_PER_LIST) as usize];
            if lod == 2 {
                // A plain draw: six vertices per impostor quad.
                a[0] = 6;
            } else {
                let [bark, leaves] = mesh_ranges(proto, lod);
                a[0] = bark.len() as u32;
                a[2] = bark.start;
                a[5] = leaves.len() as u32;
                a[7] = leaves.start;
            }
        }
        queue.write_buffer(&self.args, 0, bytemuck::cast_slice(&args));
    }

    /// The cull dispatches (before the shadow and main passes read the lists).
    pub fn encode(&self, pipelines: &CullPipelines, encoder: &mut wgpu::CommandEncoder) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty tree cull"),
            timestamp_writes: None,
        });
        let Some(hiz) = &self.hiz_group else {
            return;
        };
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_bind_group(1, hiz, &[]);
        pass.set_pipeline(&pipelines.cull);
        let groups = self.count.div_ceil(GROUP);
        let (x, y) = (groups.min(65535), groups.div_ceil(65535));
        pass.dispatch_workgroups(x, y, 1);
        pass.set_pipeline(&pipelines.clamp);
        pass.dispatch_workgroups((self.lists.len() as u32).div_ceil(64), 1, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(SRC)
            .unwrap_or_else(|e| panic!("trees_cull.wgsl: {}", e.emit_to_string(SRC)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("trees_cull.wgsl: {e:?}"));
    }

    #[test]
    fn lists_are_laid_out_per_view_prototype_and_level() {
        assert_eq!(list_index(4, 0, 0, 0), 0);
        assert_eq!(list_index(4, 0, 1, 2), 5);
        assert_eq!(list_index(4, 1, 0, 0), 12);
        let l = ListInfo {
            base: 0,
            capacity: 1,
            args: 20,
        };
        assert_eq!(l.args_offset(false), 80);
        assert_eq!(l.args_offset(true), 100);
    }
}
