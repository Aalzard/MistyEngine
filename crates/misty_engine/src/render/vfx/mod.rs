//! Visual Effects on the GPU. Every system of an instance (the main one and its GPU Event
//! systems) has a particle buffer, a dead-index list, two alive-index lists (swapped every
//! step), atomic counters, an indirect-arguments buffer and, with Trail outputs, a ring of
//! past positions; the instance shares its properties, ramps and event queues. A step runs, per
//! system in order (parents first): `cs_reset` (after Reinit), `cs_event_args` + `cs_emit`
//! (GPU Event systems, indirect) or `cs_emit` (the main system), `cs_args` (writes the indirect
//! dispatch, clears the event queues it fills), `cs_update` (indirect; ages, moves, reaps,
//! sends events) and `cs_finish` (writes the indirect draw of every output); see
//! [`crate::vfx::codegen`] for the kernels. Drawing reads the survivors' alive list (or a sorted
//! copy) in the vertex shader with `draw_indirect`, so the CPU never touches particles.
//!
//! Sorting (`sort.wgsl`) is a bitonic sort of keys and particle indices: back to front for
//! alpha-blended outputs (every view, since it follows the camera) and by age for ribbons
//! (once per step). Depth buffer collision reads the last opaque scene depth a view copied and
//! that view's camera.
//!
//! Instances simulate once per new step counter (the editor draws two views a frame) and only
//! while their bounds are in a view, like the reference engine's default culling. Counters are
//! copied to a mappable buffer and read back asynchronously for `aliveCount` and the Inspector.

#[cfg(test)]
mod gpu_tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::culling::Frustum;
use super::pipelines::{self, Depth, Spec};
use super::stats::RenderStats;
use super::texture::TextureCache;
use crate::vfx::codegen::{self, CompiledEffect, CompiledSystem, SortKey, WORKGROUP};
use crate::vfx::{BlendMode, EffectDraw, EffectStats, OutputKind, SimulationSpace};

/// Renders an unused instance's buffers are kept.
const KEEP_RENDERS: u64 = 600;
/// Bytes of one particle (`VfxParticle`).
pub const PARTICLE_BYTES: u64 = 64;
const COUNTER_BYTES: u64 = 32;
/// Bytes of the indirect buffer before the draw arguments (update + event emit dispatches).
const DISPATCH_BYTES: u64 = 32;
/// Stride of the sort pass uniforms (the dynamic offset alignment).
const PASS_STRIDE: u64 = 256;

const SORT_SHADER: &str = include_str!("sort.wgsl");
const SORT_ARGS_SHADER: &str = include_str!("sort_args.wgsl");

/// Validate a generated output shader with the renderer's prelude (view, fog).
pub(crate) fn validate_output(src: &str) -> Result<(), String> {
    use wgpu::naga;
    let full = format!("{}\n{src}", pipelines::COMMON);
    let module = naga::front::wgsl::parse_str(&full).map_err(|e| e.emit_to_string(&full))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(&full))?;
    Ok(())
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SimParams {
    world: [[f32; 4]; 4],
    inv_world: [[f32; 4]; 4],
    dt: f32,
    time: f32,
    spawn_count: u32,
    capacity: u32,
    seed: u32,
    serial: u32,
    parity: u32,
    world_space: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RenderParams {
    world: [[f32; 4]; 4],
    time: f32,
    dt: f32,
    world_space: u32,
    textured: u32,
    flipbook: [f32; 4],
    misc: [f32; 4],
    trail: [f32; 4],
}

/// The depth camera of depth buffer collision (`VfxDepthCam`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DepthCam {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    size: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SortParams {
    world: [[f32; 4]; 4],
    camera: [f32; 4],
    mode: u32,
    world_space: u32,
    size: u32,
    stages: u32,
}

/// The counters struct of the simulation shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct Counters {
    pub dead: i32,
    pub alive: [u32; 2],
    pub spawned: u32,
    pub reaped: u32,
    pub last_alive: u32,
    pub pad: [u32; 2],
}

struct SystemPipelines {
    reset: wgpu::ComputePipeline,
    emit: wgpu::ComputePipeline,
    event_args: Option<wgpu::ComputePipeline>,
    args: wgpu::ComputePipeline,
    update: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
    outputs: Vec<wgpu::RenderPipeline>,
}

struct EffectPipelines {
    systems: Vec<SystemPipelines>,
    used: u64,
}

/// The sort kernels (one shader for every effect).
struct SortPipelines {
    layout: wgpu::BindGroupLayout,
    pass_layout: wgpu::BindGroupLayout,
    args_layout: wgpu::BindGroupLayout,
    args: wgpu::ComputePipeline,
    init: wgpu::ComputePipeline,
    local: wgpu::ComputePipeline,
    step: wgpu::ComputePipeline,
    merge: wgpu::ComputePipeline,
}

/// Readback states.
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const MAPPED: u8 = 3;

/// A sorted copy of a system's alive list.
struct SortGpu {
    key: SortKey,
    /// Length of the key / value buffers (power of two, ≥ 512).
    size: u32,
    /// Stages above the 512-element blocks (k = 1024 … size).
    stages: u32,
    /// (k, j) of every dynamic-offset pass entry, in dispatch order: `None` = merge of stage k.
    passes: Vec<(u32, Option<u32>)>,
    vals: wgpu::Buffer,
    params: wgpu::Buffer,
    args: wgpu::Buffer,
    group: [wgpu::BindGroup; 2],
    pass_group: wgpu::BindGroup,
    args_group: wgpu::BindGroup,
}

struct SystemGpu {
    capacity: u32,
    particles: wgpu::Buffer,
    counters: wgpu::Buffer,
    indirect: wgpu::Buffer,
    /// The dispatch arguments, copied out of `indirect` (a buffer can't be bound as storage
    /// and read as indirect arguments in one dispatch).
    dispatch: wgpu::Buffer,
    sim_params: wgpu::Buffer,
    dead: wgpu::Buffer,
    alive: [wgpu::Buffer; 2],
    trail: wgpu::Buffer,
    trail_points: u32,
    render_params: Vec<wgpu::Buffer>,
    /// Per parity (the alive list read).
    sim_groups: Vec<wgpu::BindGroup>,
    /// Per output, per parity (the alive list drawn, or the sorted list).
    render_groups: Vec<[wgpu::BindGroup; 2]>,
    sorts: Vec<SortGpu>,
    readback: wgpu::Buffer,
    readback_state: Arc<AtomicU8>,
    last: Counters,
}

struct Instance {
    hash: u64,
    systems: Vec<SystemGpu>,
    props: wgpu::Buffer,
    ramps: wgpu::Buffer,
    ramp_len: usize,
    events: wgpu::Buffer,
    /// Alive list read by the next step (and drawn now); all systems flip together.
    parity: u32,
    serial: u64,
    reset_gen: u64,
    data_key: u64,
    used: u64,
    last_stats: EffectStats,
}

/// One output of one instance drawn this render.
struct FrameDraw {
    id: u64,
    hash: u64,
    system: usize,
    output: usize,
    /// Index of the output in the effect's flattened output list (textures).
    texture: Option<String>,
    /// Soft particles (reads the scene depth copy).
    soft: bool,
}

/// Scene depth for soft particles: a bind group per scene copy, and a far fallback.
struct SoftDepth {
    fallback: wgpu::BindGroup,
    groups: HashMap<u64, wgpu::BindGroup>,
    /// Depth buffer collision: the same textures with the depth camera uniform.
    sim_fallback: wgpu::BindGroup,
    sim_groups: HashMap<u64, wgpu::BindGroup>,
    camera: wgpu::Buffer,
}

/// The last scene depth copy a view made, and that view's camera.
#[derive(Clone, Copy)]
struct DepthSource {
    id: u64,
    view_proj: Mat4,
    camera: Vec3,
    size: (u32, u32),
}

/// GPU time of the simulation and sorting of one prepare (tests and cost reports).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct StepTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    period: f32,
}

#[derive(Default)]
struct Layouts {
    sim: Option<wgpu::BindGroupLayout>,
    sim_depth: Option<wgpu::BindGroupLayout>,
    render: Option<wgpu::BindGroupLayout>,
    depth: Option<wgpu::BindGroupLayout>,
}

#[derive(Default)]
pub(super) struct Effects {
    layouts: Layouts,
    soft: Option<SoftDepth>,
    sort: Option<SortPipelines>,
    pipelines: HashMap<u64, EffectPipelines>,
    instances: HashMap<u64, Instance>,
    /// Mesh output vertices per primitive, and an empty buffer for the other outputs.
    meshes: HashMap<crate::scene::PrimitiveMesh, wgpu::Buffer>,
    dummy: Option<wgpu::Buffer>,
    renders: u64,
    frame: Vec<FrameDraw>,
    /// A drawn effect collides with the depth buffer (the scene copy is needed).
    wants_depth: bool,
    depth_source: Option<DepthSource>,
    /// Instances stepped by the last [`Effects::prepare`] (tests, stats).
    pub stepped: usize,
    /// Instances drawn (inside a view) by the last prepare.
    pub visible: usize,
    /// Sort passes recorded by the last prepare (tests).
    pub sorted: usize,
    pub(crate) timer: Option<StepTimer>,
}

fn entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BufferBindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn depth_texture_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn make_layouts(device: &wgpu::Device) -> Layouts {
    use wgpu::BufferBindingType as B;
    use wgpu::ShaderStages as S;
    let rw = B::Storage { read_only: false };
    let ro = B::Storage { read_only: true };
    let sim = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx sim"),
        entries: &[
            entry(0, S::COMPUTE, B::Uniform),
            entry(1, S::COMPUTE, rw),
            entry(2, S::COMPUTE, rw),
            entry(3, S::COMPUTE, rw),
            entry(4, S::COMPUTE, rw),
            entry(5, S::COMPUTE, rw),
            entry(6, S::COMPUTE, rw),
            entry(7, S::COMPUTE, B::Uniform),
            entry(8, S::COMPUTE, ro),
            entry(9, S::COMPUTE, rw),
            entry(10, S::COMPUTE, rw),
        ],
    });
    let sim_depth = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx sim depth"),
        entries: &[
            depth_texture_entry(0, S::COMPUTE),
            entry(1, S::COMPUTE, B::Uniform),
        ],
    });
    let render = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx render"),
        entries: &[
            entry(0, S::VERTEX, ro),
            entry(1, S::VERTEX, ro),
            entry(2, S::VERTEX_FRAGMENT, B::Uniform),
            entry(3, S::VERTEX_FRAGMENT, ro),
            entry(4, S::VERTEX_FRAGMENT, B::Uniform),
            entry(5, S::VERTEX, ro),
            entry(6, S::VERTEX, ro),
        ],
    });
    let depth = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx scene depth"),
        entries: &[depth_texture_entry(0, S::FRAGMENT)],
    });
    Layouts {
        sim: Some(sim),
        sim_depth: Some(sim_depth),
        render: Some(render),
        depth: Some(depth),
    }
}

fn depth_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty vfx scene depth"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
    })
}

fn sim_depth_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    camera: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty vfx sim depth"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: camera.as_entire_binding(),
            },
        ],
    })
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage,
        mapped_at_creation: false,
    })
}

fn additive() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

fn sort_pipelines(device: &wgpu::Device) -> SortPipelines {
    use wgpu::BufferBindingType as B;
    use wgpu::ShaderStages as S;
    let rw = B::Storage { read_only: false };
    let ro = B::Storage { read_only: true };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx sort"),
        entries: &[
            entry(0, S::COMPUTE, B::Uniform),
            entry(1, S::COMPUTE, ro),
            entry(2, S::COMPUTE, ro),
            entry(3, S::COMPUTE, ro),
            entry(4, S::COMPUTE, rw),
            entry(5, S::COMPUTE, rw),
        ],
    });
    let pass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx sort pass"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: S::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: B::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(16),
            },
            count: None,
        }],
    });
    let args_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx sort args"),
        entries: &[
            entry(0, S::COMPUTE, ro),
            entry(1, S::COMPUTE, rw),
            entry(2, S::COMPUTE, B::Uniform),
        ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty vfx sort"),
        source: wgpu::ShaderSource::Wgsl(SORT_SHADER.into()),
    });
    let args_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty vfx sort args"),
        source: wgpu::ShaderSource::Wgsl(SORT_ARGS_SHADER.into()),
    });
    let sort_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty vfx sort"),
        bind_group_layouts: &[Some(&layout), Some(&pass_layout)],
        immediate_size: 0,
    });
    let args_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty vfx sort args"),
        bind_group_layouts: &[Some(&args_layout)],
        immediate_size: 0,
    });
    let compute = |m: &wgpu::ShaderModule, pl: &wgpu::PipelineLayout, entry: &str| {
        crate::render::pipelines::note_pipeline();
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(pl),
            module: m,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: crate::render::pipeline_cache(device).as_ref(),
        })
    };
    SortPipelines {
        args: compute(&args_module, &args_pl, "cs_sort_args"),
        init: compute(&module, &sort_layout, "cs_sort_init"),
        local: compute(&module, &sort_layout, "cs_sort_local"),
        step: compute(&module, &sort_layout, "cs_sort_step"),
        merge: compute(&module, &sort_layout, "cs_sort_merge"),
        layout,
        pass_layout,
        args_layout,
    }
}

fn create_pipelines(
    device: &wgpu::Device,
    shared: &pipelines::Pipelines,
    layouts: &Layouts,
    effect: &CompiledEffect,
) -> EffectPipelines {
    let (Some(sim_layout), Some(sim_depth), Some(render_layout), Some(depth_layout)) = (
        &layouts.sim,
        &layouts.sim_depth,
        &layouts.render,
        &layouts.depth,
    ) else {
        unreachable!("layouts are made before pipelines")
    };
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty vfx sim"),
        bind_group_layouts: &[Some(sim_layout), Some(sim_depth)],
        immediate_size: 0,
    });
    let systems = effect
        .systems
        .iter()
        .map(|sys| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("misty vfx sim"),
                source: wgpu::ShaderSource::Wgsl(sys.sim_source.as_str().into()),
            });
            let compute = |entry: &str| {
                crate::render::pipelines::note_pipeline();
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: crate::render::pipeline_cache(device).as_ref(),
                })
            };
            let outputs = sys
                .outputs
                .iter()
                .map(|o| {
                    let layouts = [
                        &shared.view_layout,
                        &shared.material_layout,
                        render_layout,
                        depth_layout,
                    ];
                    let s = &o.settings;
                    let topology = match s.kind {
                        OutputKind::Point => wgpu::PrimitiveTopology::PointList,
                        OutputKind::Line => wgpu::PrimitiveTopology::LineList,
                        _ => wgpu::PrimitiveTopology::TriangleList,
                    };
                    let opaque = s.blend == BlendMode::Opaque;
                    pipelines::build(
                        device,
                        Spec {
                            vs: "vs_main",
                            fs: Some("fs_main"),
                            topology,
                            depth: Depth::main(opaque, wgpu::CompareFunction::LessEqual),
                            blend: match s.blend {
                                BlendMode::Additive => Some(additive()),
                                BlendMode::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
                                BlendMode::Opaque => None,
                            },
                            glow: Some(true),
                            ..Spec::new("misty vfx output", &o.source, &layouts)
                        },
                    )
                })
                .collect();
            SystemPipelines {
                reset: compute("cs_reset"),
                emit: compute("cs_emit"),
                event_args: sys.event.as_ref().map(|_| compute("cs_event_args")),
                args: compute("cs_args"),
                update: compute("cs_update"),
                finish: compute("cs_finish"),
                outputs,
            }
        })
        .collect();
    EffectPipelines { systems, used: 0 }
}

fn bind(i: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding: i,
        resource: b.as_entire_binding(),
    }
}

impl SortGpu {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sp: &SortPipelines,
        sys: &SystemGpu,
        key: SortKey,
    ) -> Self {
        use wgpu::BufferUsages as U;
        let size = sys.capacity.next_power_of_two().max(512);
        let stages = (size / 512).trailing_zeros();
        let mut passes = Vec::new();
        for s in 0..stages {
            let k = 1024u32 << s;
            let mut j = k / 2;
            while j >= 512 {
                passes.push((k, Some(j)));
                j /= 2;
            }
            passes.push((k, None));
        }
        let keys = buffer(device, "misty vfx sort keys", size as u64 * 4, U::STORAGE);
        let vals = buffer(
            device,
            "misty vfx sort vals",
            size as u64 * 4,
            U::STORAGE | U::COPY_SRC,
        );
        let params = buffer(
            device,
            "misty vfx sort params",
            std::mem::size_of::<SortParams>() as u64,
            U::UNIFORM | U::COPY_DST,
        );
        let args = buffer(
            device,
            "misty vfx sort args",
            (8 + 8 * stages.max(1) as u64) * 4,
            U::STORAGE | U::INDIRECT,
        );
        let pass_buf = buffer(
            device,
            "misty vfx sort passes",
            PASS_STRIDE * passes.len().max(1) as u64,
            U::UNIFORM | U::COPY_DST,
        );
        let mut data = vec![0u8; (PASS_STRIDE as usize) * passes.len().max(1)];
        for (i, (k, j)) in passes.iter().enumerate() {
            let at = i * PASS_STRIDE as usize;
            data[at..at + 4].copy_from_slice(&k.to_le_bytes());
            data[at + 4..at + 8].copy_from_slice(&j.unwrap_or(0).to_le_bytes());
        }
        queue.write_buffer(&pass_buf, 0, &data);
        let group = |p: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty vfx sort"),
                layout: &sp.layout,
                entries: &[
                    bind(0, &params),
                    bind(1, &sys.particles),
                    bind(2, &sys.alive[p]),
                    bind(3, &sys.counters),
                    bind(4, &keys),
                    bind(5, &vals),
                ],
            })
        };
        let pass_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vfx sort pass"),
            layout: &sp.pass_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &pass_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(16),
                }),
            }],
        });
        let args_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty vfx sort args"),
            layout: &sp.args_layout,
            entries: &[bind(0, &sys.counters), bind(1, &args), bind(2, &params)],
        });
        let group = [group(0), group(1)];
        Self {
            key,
            size,
            stages,
            passes,
            vals,
            params,
            args,
            group,
            pass_group,
            args_group,
        }
    }

    /// Record the sort of the list drawn at `parity`.
    fn record(&self, encoder: &mut wgpu::CommandEncoder, sp: &SortPipelines, parity: u32) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("misty vfx sort args"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&sp.args);
            pass.set_bind_group(0, &self.args_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty vfx sort"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.group[parity as usize], &[]);
        pass.set_bind_group(1, &self.pass_group, &[0]);
        pass.set_pipeline(&sp.init);
        pass.dispatch_workgroups_indirect(&self.args, 0);
        pass.set_pipeline(&sp.local);
        pass.dispatch_workgroups_indirect(&self.args, 16);
        let mut stage = 0u64;
        let mut last_k = 0;
        for (i, (k, j)) in self.passes.iter().enumerate() {
            if *k != last_k {
                if last_k != 0 {
                    stage += 1;
                }
                last_k = *k;
            }
            pass.set_bind_group(1, &self.pass_group, &[(i as u64 * PASS_STRIDE) as u32]);
            match j {
                Some(_) => {
                    pass.set_pipeline(&sp.step);
                    pass.dispatch_workgroups_indirect(&self.args, 32 + stage * 32);
                }
                None => {
                    pass.set_pipeline(&sp.merge);
                    pass.dispatch_workgroups_indirect(&self.args, 32 + stage * 32 + 16);
                }
            }
        }
    }
}

impl SystemGpu {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &Layouts,
        sort: &SortPipelines,
        meshes: &HashMap<crate::scene::PrimitiveMesh, wgpu::Buffer>,
        dummy: &wgpu::Buffer,
        sys: &CompiledSystem,
        shared: (&wgpu::Buffer, &wgpu::Buffer, &wgpu::Buffer),
    ) -> Self {
        use wgpu::BufferUsages as U;
        let cap = sys.capacity.max(1) as u64;
        let storage = U::STORAGE | U::COPY_DST | U::COPY_SRC;
        let particles = buffer(device, "misty vfx particles", cap * PARTICLE_BYTES, storage);
        let dead = buffer(device, "misty vfx dead", cap * 4, storage);
        let alive = [
            buffer(device, "misty vfx alive", cap * 4, storage),
            buffer(device, "misty vfx alive", cap * 4, storage),
        ];
        let counters = buffer(device, "misty vfx counters", COUNTER_BYTES, storage);
        let outputs = sys.outputs.len();
        let indirect = buffer(
            device,
            "misty vfx indirect",
            DISPATCH_BYTES + 16 * outputs.max(1) as u64,
            storage | U::INDIRECT,
        );
        let dispatch = buffer(
            device,
            "misty vfx dispatch",
            DISPATCH_BYTES,
            U::INDIRECT | U::COPY_DST,
        );
        let sim_params = buffer(
            device,
            "misty vfx params",
            std::mem::size_of::<SimParams>() as u64,
            U::UNIFORM | U::COPY_DST,
        );
        let trail = buffer(
            device,
            "misty vfx trail",
            cap * sys.trail_points as u64 * 16,
            storage,
        );
        let render_params: Vec<wgpu::Buffer> = (0..outputs)
            .map(|_| {
                buffer(
                    device,
                    "misty vfx render params",
                    std::mem::size_of::<RenderParams>() as u64,
                    U::UNIFORM | U::COPY_DST,
                )
            })
            .collect();
        let readback = buffer(
            device,
            "misty vfx readback",
            COUNTER_BYTES,
            U::MAP_READ | U::COPY_DST,
        );
        let mut s = Self {
            capacity: sys.capacity.max(1),
            sim_groups: Vec::new(),
            render_groups: Vec::new(),
            sorts: Vec::new(),
            particles,
            counters,
            indirect,
            dispatch,
            sim_params,
            dead,
            alive,
            trail,
            trail_points: sys.trail_points,
            render_params,
            readback,
            readback_state: Arc::new(AtomicU8::new(IDLE)),
            last: Counters::default(),
        };
        let sorts: Vec<SortGpu> = sys
            .sort_keys()
            .into_iter()
            .map(|k| SortGpu::new(device, queue, sort, &s, k))
            .collect();
        s.sorts = sorts;
        let (props, ramps, events) = shared;
        let sim_layout = layouts.sim.as_ref().expect("layouts");
        let render_layout = layouts.render.as_ref().expect("layouts");
        let sim = |p: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty vfx sim"),
                layout: sim_layout,
                entries: &[
                    bind(0, &s.sim_params),
                    bind(1, &s.particles),
                    bind(2, &s.dead),
                    bind(3, &s.alive[p]),
                    bind(4, &s.alive[1 - p]),
                    bind(5, &s.counters),
                    bind(6, &s.indirect),
                    bind(7, props),
                    bind(8, ramps),
                    bind(9, &s.trail),
                    bind(10, events),
                ],
            })
        };
        s.sim_groups = vec![sim(0), sim(1)];
        s.render_groups = sys
            .outputs
            .iter()
            .enumerate()
            .map(|(k, o)| {
                let sorted = o.sort.and_then(|key| s.sorts.iter().find(|x| x.key == key));
                let mesh = (o.settings.kind == OutputKind::Mesh)
                    .then(|| meshes.get(&o.settings.mesh))
                    .flatten()
                    .unwrap_or(dummy);
                let group = |p: usize| {
                    let list = sorted.map_or(&s.alive[p], |x| &x.vals);
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("misty vfx render"),
                        layout: render_layout,
                        entries: &[
                            bind(0, &s.particles),
                            bind(1, list),
                            bind(2, props),
                            bind(3, ramps),
                            bind(4, &s.render_params[k]),
                            bind(5, &s.trail),
                            bind(6, mesh),
                        ],
                    })
                };
                [group(0), group(1)]
            })
            .collect();
        s
    }
}

impl Instance {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &Layouts,
        sort: &SortPipelines,
        meshes: &HashMap<crate::scene::PrimitiveMesh, wgpu::Buffer>,
        dummy: &wgpu::Buffer,
        effect: &CompiledEffect,
        ramp_len: usize,
    ) -> Self {
        use wgpu::BufferUsages as U;
        let props = buffer(
            device,
            "misty vfx props",
            (codegen::MAX_PROPERTIES * 16) as u64,
            U::UNIFORM | U::COPY_DST,
        );
        let ramps = buffer(
            device,
            "misty vfx ramps",
            (ramp_len.max(codegen::RAMP_SAMPLES) * 16) as u64,
            U::STORAGE | U::COPY_DST,
        );
        let events = buffer(
            device,
            "misty vfx events",
            effect.event_words as u64 * 4,
            U::STORAGE | U::COPY_DST,
        );
        let systems = effect
            .systems
            .iter()
            .map(|s| {
                SystemGpu::new(
                    device,
                    queue,
                    layouts,
                    sort,
                    meshes,
                    dummy,
                    s,
                    (&props, &ramps, &events),
                )
            })
            .collect();
        Self {
            hash: effect.hash,
            systems,
            props,
            ramps,
            ramp_len: ramp_len.max(codegen::RAMP_SAMPLES),
            events,
            parity: 0,
            serial: 0,
            reset_gen: 0,
            data_key: 0,
            used: 0,
            last_stats: EffectStats::default(),
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl StepTimer {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        Some(Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("misty vfx timer"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty vfx timer resolve"),
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty vfx timer readback"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            period: queue.get_timestamp_period(),
        })
    }

    fn mark(&self, encoder: &mut wgpu::CommandEncoder, index: u32) {
        let _ = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty vfx timer"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &self.queries,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: None,
            }),
        });
    }

    /// Blocking read of the last prepare's GPU milliseconds.
    pub(crate) fn read_ms(&self, device: &wgpu::Device) -> f32 {
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let t: [u64; 2] = {
            let view = slice.get_mapped_range().expect("mapped");
            bytemuck::pod_read_unaligned(&view[..16])
        };
        self.readback.unmap();
        t[1].saturating_sub(t[0]) as f32 * self.period / 1.0e6
    }
}

impl Effects {
    /// Drop GPU state of every instance (tests).
    #[cfg(test)]
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// GPU bytes held by effect instances.
    pub fn bytes(&self) -> u64 {
        self.instances
            .values()
            .map(|i| {
                let systems: u64 = i
                    .systems
                    .iter()
                    .map(|s| {
                        let sorts: u64 = s.sorts.iter().map(|x| x.size as u64 * 8).sum();
                        s.capacity as u64 * (PARTICLE_BYTES + 12 + s.trail_points as u64 * 16)
                            + sorts
                    })
                    .sum();
                systems + (i.ramp_len * 16) as u64 + i.events.size()
            })
            .sum()
    }

    fn ensure_shared(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.layouts.sim.is_none() {
            self.layouts = make_layouts(device);
        }
        if self.sort.is_none() {
            self.sort = Some(sort_pipelines(device));
        }
        if self.dummy.is_none() {
            self.dummy = Some(buffer(
                device,
                "misty vfx empty",
                16,
                wgpu::BufferUsages::STORAGE,
            ));
        }
        if self.soft.is_none() {
            use wgpu::util::DeviceExt;
            let far = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("misty vfx far depth"),
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
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                bytemuck::bytes_of(&1.0f32),
            );
            let view = far.create_view(&Default::default());
            let camera = buffer(
                device,
                "misty vfx depth camera",
                std::mem::size_of::<DepthCam>() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            let layout = self.layouts.depth.as_ref().expect("layouts");
            let sim_layout = self.layouts.sim_depth.as_ref().expect("layouts");
            self.soft = Some(SoftDepth {
                fallback: depth_group(device, layout, &view),
                groups: HashMap::new(),
                sim_fallback: sim_depth_group(device, sim_layout, &view, &camera),
                sim_groups: HashMap::new(),
                camera,
            });
        }
    }

    fn mesh_buffer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        m: crate::scene::PrimitiveMesh,
    ) {
        if self.meshes.contains_key(&m) {
            return;
        }
        let data = codegen::mesh_vertices(m);
        let b = buffer(
            device,
            "misty vfx mesh",
            (data.len() * 16) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        queue.write_buffer(&b, 0, bytemuck::cast_slice(&data));
        self.meshes.insert(m, b);
    }

    /// Simulate the visible effects that have a new step and list what to draw this render.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shared: &pipelines::Pipelines,
        textures: &mut TextureCache,
        draws: &[EffectDraw],
        view_proj: &Mat4,
        camera: Vec3,
        cull: bool,
    ) {
        self.frame.clear();
        self.stepped = 0;
        self.visible = 0;
        self.sorted = 0;
        self.wants_depth = false;
        self.renders += 1;
        let renders = self.renders;
        let _ = device.poll(wgpu::PollType::Poll);
        self.collect_readbacks();
        if draws.is_empty() {
            self.collect_garbage();
            return;
        }
        self.ensure_shared(device, queue);
        for d in draws {
            for o in d.effect.all_outputs() {
                if o.settings.kind == OutputKind::Mesh {
                    self.mesh_buffer(device, queue, o.settings.mesh);
                }
            }
        }
        // The depth camera of the last scene copy (depth buffer collision).
        let soft = self.soft.as_ref().expect("made above");
        let depth_group = match self.depth_source {
            Some(src) if soft.sim_groups.contains_key(&src.id) => {
                let cam = DepthCam {
                    view_proj: src.view_proj.to_cols_array_2d(),
                    inv_view_proj: src.view_proj.inverse().to_cols_array_2d(),
                    camera: [src.camera.x, src.camera.y, src.camera.z, 1.0],
                    size: [src.size.0 as f32, src.size.1 as f32, 0.0, 0.0],
                };
                queue.write_buffer(&soft.camera, 0, bytemuck::bytes_of(&cam));
                &soft.sim_groups[&src.id]
            }
            _ => {
                queue.write_buffer(&soft.camera, 0, &[0u8; std::mem::size_of::<DepthCam>()]);
                &soft.sim_fallback
            }
        };
        let frustum = Frustum::from_matrix(view_proj);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty vfx simulate"),
        });
        if let Some(t) = &self.timer {
            t.mark(&mut encoder, 0);
        }
        let sort = self.sort.as_ref().expect("made above");
        let dummy = self.dummy.as_ref().expect("made above");
        let mut stepped = Vec::new();
        for d in draws {
            if cull && !frustum.intersects(&d.bounds) {
                continue;
            }
            self.visible += 1;
            let effect = &d.effect;
            self.wants_depth |= effect.depth_collision;
            let pipes = self
                .pipelines
                .entry(effect.hash)
                .or_insert_with(|| create_pipelines(device, shared, &self.layouts, effect));
            pipes.used = renders;
            let rebuild = self
                .instances
                .get(&d.id)
                .is_none_or(|i| i.hash != effect.hash || i.ramp_len < d.ramps.len());
            if rebuild {
                let inst = Instance::new(
                    device,
                    queue,
                    &self.layouts,
                    sort,
                    &self.meshes,
                    dummy,
                    effect,
                    d.ramps.len(),
                );
                self.instances.insert(d.id, inst);
            }
            let inst = self.instances.get_mut(&d.id).expect("inserted above");
            inst.used = renders;
            if inst.data_key != d.data_key {
                inst.data_key = d.data_key;
                queue.write_buffer(&inst.props, 0, bytemuck::cast_slice(&d.props[..]));
                if !d.ramps.is_empty() {
                    queue.write_buffer(&inst.ramps, 0, bytemuck::cast_slice(&d.ramps[..]));
                }
            }
            for t in &d.textures {
                textures.prepare(device, queue, &shared.material_layout, t.as_deref(), None);
            }
            let reset = inst.reset_gen != d.reset_gen;
            let step = d.serial != inst.serial;
            let world_space = effect.space == SimulationSpace::World;
            if reset || step {
                if reset {
                    inst.reset_gen = d.reset_gen;
                    inst.parity = 0;
                    encoder.clear_buffer(&inst.events, 0, None);
                }
                let parity = inst.parity;
                for (si, (sys, sp)) in inst.systems.iter().zip(&pipes.systems).enumerate() {
                    let csys = &effect.systems[si];
                    let spawn_count = if step && si == 0 { d.spawn_count } else { 0 };
                    let params = SimParams {
                        world: d.world.to_cols_array_2d(),
                        inv_world: d.world.inverse().to_cols_array_2d(),
                        dt: if step { d.dt } else { 0.0 },
                        time: d.time,
                        spawn_count,
                        capacity: sys.capacity,
                        seed: d.seed ^ (si as u32).wrapping_mul(0x9e37_79b9),
                        serial: d.serial as u32,
                        parity,
                        world_space: world_space as u32,
                    };
                    queue.write_buffer(&sys.sim_params, 0, bytemuck::bytes_of(&params));
                    let group = &sys.sim_groups[parity as usize];
                    if reset {
                        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("misty vfx reset"),
                            timestamp_writes: None,
                        });
                        pass.set_bind_group(0, group, &[]);
                        pass.set_bind_group(1, depth_group, &[]);
                        pass.set_pipeline(&sp.reset);
                        pass.dispatch_workgroups(sys.capacity.div_ceil(WORKGROUP), 1, 1);
                    }
                    if !step {
                        continue;
                    }
                    let event = csys.event.is_some();
                    {
                        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("misty vfx emit"),
                            timestamp_writes: None,
                        });
                        pass.set_bind_group(0, group, &[]);
                        pass.set_bind_group(1, depth_group, &[]);
                        if let Some(args) = &sp.event_args {
                            pass.set_pipeline(args);
                            pass.dispatch_workgroups(1, 1, 1);
                        } else if spawn_count > 0 {
                            pass.set_pipeline(&sp.emit);
                            pass.dispatch_workgroups(spawn_count.div_ceil(WORKGROUP), 1, 1);
                        }
                    }
                    if event {
                        encoder.copy_buffer_to_buffer(
                            &sys.indirect,
                            0,
                            &sys.dispatch,
                            0,
                            DISPATCH_BYTES,
                        );
                        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("misty vfx event emit"),
                            timestamp_writes: None,
                        });
                        pass.set_bind_group(0, group, &[]);
                        pass.set_bind_group(1, depth_group, &[]);
                        pass.set_pipeline(&sp.emit);
                        pass.dispatch_workgroups_indirect(&sys.dispatch, 16);
                    }
                    {
                        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("misty vfx args"),
                            timestamp_writes: None,
                        });
                        pass.set_bind_group(0, group, &[]);
                        pass.set_bind_group(1, depth_group, &[]);
                        pass.set_pipeline(&sp.args);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    encoder.copy_buffer_to_buffer(&sys.indirect, 0, &sys.dispatch, 0, 16);
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("misty vfx update"),
                        timestamp_writes: None,
                    });
                    pass.set_bind_group(0, group, &[]);
                    pass.set_bind_group(1, depth_group, &[]);
                    pass.set_pipeline(&sp.update);
                    pass.dispatch_workgroups_indirect(&sys.dispatch, 0);
                    pass.set_pipeline(&sp.finish);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                if step {
                    inst.serial = d.serial;
                    inst.parity = 1 - inst.parity;
                    self.stepped += 1;
                }
                let mut any = false;
                for sys in &inst.systems {
                    if sys.readback_state.load(Ordering::Acquire) == IDLE {
                        encoder.copy_buffer_to_buffer(
                            &sys.counters,
                            0,
                            &sys.readback,
                            0,
                            COUNTER_BYTES,
                        );
                        sys.readback_state.store(COPIED, Ordering::Release);
                        any = true;
                    }
                }
                if any {
                    stepped.push(d.id);
                }
            }
            // Sorted lists: by distance every view, by age once per step.
            let drawn = inst.parity;
            for sys in &inst.systems {
                for s in &sys.sorts {
                    let keep = match s.key {
                        SortKey::Distance => true,
                        SortKey::Age => reset || step,
                    };
                    if !keep {
                        continue;
                    }
                    let params = SortParams {
                        world: d.world.to_cols_array_2d(),
                        camera: [camera.x, camera.y, camera.z, 0.0],
                        mode: (s.key == SortKey::Age) as u32,
                        world_space: world_space as u32,
                        size: s.size,
                        stages: s.stages,
                    };
                    queue.write_buffer(&s.params, 0, bytemuck::bytes_of(&params));
                    s.record(&mut encoder, sort, drawn);
                    self.sorted += 1;
                }
            }
            // Render parameters follow the object every render.
            let mut flat = 0;
            for (si, csys) in effect.systems.iter().enumerate() {
                let sys = &inst.systems[si];
                for (k, o) in csys.outputs.iter().enumerate() {
                    let s = &o.settings;
                    let texture = d.textures.get(flat).cloned().flatten();
                    flat += 1;
                    let params = RenderParams {
                        world: d.world.to_cols_array_2d(),
                        time: d.time,
                        dt: d.dt,
                        world_space: world_space as u32,
                        textured: (texture.is_some() && s.kind != OutputKind::Mesh) as u32,
                        flipbook: [
                            s.flipbook[0].max(1) as f32,
                            s.flipbook[1].max(1) as f32,
                            (s.flipbook_mode == crate::vfx::graph::FlipbookMode::Fps) as u32 as f32,
                            s.flipbook_fps,
                        ],
                        misc: [
                            s.line_length,
                            if s.soft_particles && s.kind == OutputKind::Quad {
                                s.soft_distance
                            } else {
                                0.0
                            },
                            (s.kind != OutputKind::Quad) as u32 as f32,
                            0.0,
                        ],
                        trail: [
                            ((inst.serial as u32) % sys.trail_points.max(1)) as f32,
                            sys.trail_points as f32,
                            0.0,
                            0.0,
                        ],
                    };
                    queue.write_buffer(&sys.render_params[k], 0, bytemuck::bytes_of(&params));
                    self.frame.push(FrameDraw {
                        id: d.id,
                        hash: effect.hash,
                        system: si,
                        output: k,
                        texture,
                        soft: s.soft_particles && s.kind == OutputKind::Quad,
                    });
                }
            }
        }
        if let Some(t) = &self.timer {
            t.mark(&mut encoder, 1);
            encoder.resolve_query_set(&t.queries, 0..2, &t.resolve, 0);
            encoder.copy_buffer_to_buffer(&t.resolve, 0, &t.readback, 0, 16);
        }
        queue.submit([encoder.finish()]);
        for id in stepped {
            if let Some(inst) = self.instances.get(&id) {
                for sys in &inst.systems {
                    if sys.readback_state.load(Ordering::Acquire) != COPIED {
                        continue;
                    }
                    let state = sys.readback_state.clone();
                    state.store(MAPPING, Ordering::Release);
                    sys.readback
                        .slice(..)
                        .map_async(wgpu::MapMode::Read, move |r| {
                            state.store(if r.is_ok() { MAPPED } else { IDLE }, Ordering::Release);
                        });
                }
            }
        }
        self.collect_garbage();
    }

    /// Any effect output to draw this render (Frame Debugger).
    pub fn has_draws(&self) -> bool {
        !self.frame.is_empty()
    }

    /// Does an output drawn this render fade against the scene depth, or a drawn effect
    /// collide with it (both need the scene copy)?
    pub fn needs_scene(&self) -> bool {
        self.wants_depth || self.frame.iter().any(|f| f.soft)
    }

    /// Bind the scene depth copy `id` (made this render with `view_proj` from `camera`) for
    /// soft particles now and depth buffer collision in the next simulation step.
    pub fn scene_depth(
        &mut self,
        device: &wgpu::Device,
        id: u64,
        view: &wgpu::TextureView,
        view_proj: Mat4,
        camera: Vec3,
        size: (u32, u32),
    ) {
        let (Some(layout), Some(sim_layout), Some(soft)) =
            (&self.layouts.depth, &self.layouts.sim_depth, &mut self.soft)
        else {
            return;
        };
        if soft.groups.len() > 8 && !soft.groups.contains_key(&id) {
            soft.groups.clear();
            soft.sim_groups.clear();
        }
        soft.groups
            .entry(id)
            .or_insert_with(|| depth_group(device, layout, view));
        let camera_buf = &soft.camera;
        soft.sim_groups
            .entry(id)
            .or_insert_with(|| sim_depth_group(device, sim_layout, view, camera_buf));
        if self.wants_depth {
            self.depth_source = Some(DepthSource {
                id,
                view_proj,
                camera,
                size,
            });
        }
    }

    /// Read finished counter copies and publish them.
    fn collect_readbacks(&mut self) {
        for (id, inst) in &mut self.instances {
            let mut any = false;
            for sys in &mut inst.systems {
                if sys.readback_state.load(Ordering::Acquire) != MAPPED {
                    continue;
                }
                sys.last = {
                    let view = sys.readback.slice(..).get_mapped_range().expect("mapped");
                    bytemuck::pod_read_unaligned(&view[..COUNTER_BYTES as usize])
                };
                sys.readback.unmap();
                sys.readback_state.store(IDLE, Ordering::Release);
                any = true;
            }
            if !any {
                continue;
            }
            inst.last_stats = EffectStats {
                alive: inst.systems.iter().map(|s| s.last.last_alive).sum(),
                spawned: inst.systems.iter().map(|s| s.last.spawned).sum(),
                reaped: inst.systems.iter().map(|s| s.last.reaped).sum(),
            };
            crate::vfx::set_stats(*id, inst.last_stats);
        }
    }

    fn collect_garbage(&mut self) {
        let renders = self.renders;
        let gone: Vec<u64> = self
            .instances
            .iter()
            .filter(|(_, i)| i.used + KEEP_RENDERS < renders)
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            if let Some(i) = self.instances.remove(&id) {
                for s in &i.systems {
                    if s.readback_state.load(Ordering::Acquire) == MAPPED {
                        s.readback.unmap();
                    }
                }
            }
        }
        self.pipelines
            .retain(|_, p| p.used + KEEP_RENDERS >= renders);
    }

    /// The effects listed by the last prepare, in the main pass after transparent geometry.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass,
        textures: &TextureCache,
        scene: Option<u64>,
        stats: &mut RenderStats,
    ) {
        let Some(soft) = &self.soft else {
            return;
        };
        let depth = scene
            .and_then(|id| soft.groups.get(&id))
            .unwrap_or(&soft.fallback);
        for f in &self.frame {
            let (Some(inst), Some(pipes)) =
                (self.instances.get(&f.id), self.pipelines.get(&f.hash))
            else {
                continue;
            };
            let (Some(sys), Some(sp)) = (inst.systems.get(f.system), pipes.systems.get(f.system))
            else {
                continue;
            };
            let (Some(pipeline), Some(groups)) =
                (sp.outputs.get(f.output), sys.render_groups.get(f.output))
            else {
                continue;
            };
            let Some(material) = textures.get(f.texture.as_deref(), None) else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(1, material, &[]);
            pass.set_bind_group(2, &groups[inst.parity as usize], &[]);
            pass.set_bind_group(3, depth, &[]);
            pass.draw_indirect(&sys.indirect, DISPATCH_BYTES + 16 * f.output as u64);
            let n = sys.last.last_alive as u64;
            stats.batches += 1;
            stats.set_pass += 1;
            stats.triangles += 2 * n;
            stats.vertices += 4 * n;
        }
    }

    /// Nothing is drawn after the render that prepared it.
    pub fn end_frame(&mut self) {
        self.frame.clear();
    }

    #[cfg(test)]
    fn system(&self, id: u64, system: usize) -> Option<&SystemGpu> {
        self.instances.get(&id)?.systems.get(system)
    }

    /// Blocking read of an instance's (main system's) counters (tests).
    #[cfg(test)]
    pub(crate) fn read_counters(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
    ) -> Option<Counters> {
        self.read_system_counters(device, queue, id, 0)
    }

    #[cfg(test)]
    pub(crate) fn read_system_counters(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        system: usize,
    ) -> Option<Counters> {
        let sys = self.system(id, system)?;
        let bytes = read_buffer(device, queue, &sys.counters, COUNTER_BYTES);
        Some(bytemuck::pod_read_unaligned(
            &bytes[..COUNTER_BYTES as usize],
        ))
    }

    /// Blocking read of the live particles of an instance (tests): every slot with `alive`.
    #[cfg(test)]
    pub(crate) fn read_particles(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
    ) -> Vec<[f32; 16]> {
        self.read_system_particles(device, queue, id, 0)
    }

    #[cfg(test)]
    pub(crate) fn read_system_particles(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        system: usize,
    ) -> Vec<[f32; 16]> {
        let Some(sys) = self.system(id, system) else {
            return Vec::new();
        };
        let bytes = read_buffer(
            device,
            queue,
            &sys.particles,
            sys.capacity as u64 * PARTICLE_BYTES,
        );
        let all: &[[f32; 16]] = bytemuck::cast_slice(&bytes);
        all.iter()
            .filter(|p| p[15].to_bits() == 1)
            .copied()
            .collect()
    }

    /// Blocking read of the dead list and the alive list to be drawn (tests).
    #[cfg(test)]
    pub(crate) fn read_lists(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
    ) -> Option<(Vec<u32>, Vec<u32>)> {
        let inst = self.instances.get(&id)?;
        let sys = &inst.systems[0];
        let c = self.read_counters(device, queue, id)?;
        let size = sys.capacity as u64 * 4;
        let dead: Vec<u32> =
            bytemuck::cast_slice(&read_buffer(device, queue, &sys.dead, size)).to_vec();
        let alive: Vec<u32> = bytemuck::cast_slice(&read_buffer(
            device,
            queue,
            &sys.alive[inst.parity as usize],
            size,
        ))
        .to_vec();
        let n_dead = c.dead.max(0) as usize;
        let n_alive = c.alive[inst.parity as usize] as usize;
        Some((
            dead[..n_dead.min(dead.len())].to_vec(),
            alive[..n_alive.min(alive.len())].to_vec(),
        ))
    }

    /// Blocking read of a system's sorted list (`count` first entries; tests).
    #[cfg(test)]
    pub(crate) fn read_sorted(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        system: usize,
        key: SortKey,
    ) -> Option<Vec<u32>> {
        let sys = self.system(id, system)?;
        let s = sys.sorts.iter().find(|s| s.key == key)?;
        let n = sys.last.last_alive.max(
            self.read_system_counters(device, queue, id, system)?
                .last_alive,
        ) as usize;
        let all: Vec<u32> =
            bytemuck::cast_slice(&read_buffer(device, queue, &s.vals, s.size as u64 * 4)).to_vec();
        Some(all[..n.min(all.len())].to_vec())
    }

    /// Blocking read of a system's trail ring (tests).
    #[cfg(test)]
    pub(crate) fn read_trail(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        system: usize,
    ) -> Vec<[f32; 4]> {
        let Some(sys) = self.system(id, system) else {
            return Vec::new();
        };
        let size = sys.trail.size();
        bytemuck::cast_slice(&read_buffer(device, queue, &sys.trail, size)).to_vec()
    }
}

#[cfg(test)]
fn read_buffer(device: &wgpu::Device, queue: &wgpu::Queue, b: &wgpu::Buffer, size: u64) -> Vec<u8> {
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty vfx test readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(b, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let out = slice.get_mapped_range().expect("mapped").to_vec();
    staging.unmap();
    out
}
