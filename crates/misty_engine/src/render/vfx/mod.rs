//! Visual Effects on the GPU: per instance a particle buffer, a dead-index list, two alive-index
//! lists (swapped every step), atomic counters and an indirect-arguments buffer. A step runs
//! `cs_reset` (after Reinit), `cs_emit` (pops dead indices), `cs_args` (writes the indirect
//! dispatch), `cs_update` (indirect; ages, moves, reaps) and `cs_finish` (writes the indirect draw
//! of every output); see [`crate::vfx::codegen`] for the kernels. Drawing reads the survivors'
//! alive list in the vertex shader with `draw_indirect`, so the CPU never touches particles.
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
use glam::Mat4;

use super::culling::Frustum;
use super::pipelines::{self, Depth, Spec};
use super::stats::RenderStats;
use super::texture::TextureCache;
use crate::vfx::codegen::{self, CompiledEffect, WORKGROUP};
use crate::vfx::{BlendMode, EffectDraw, EffectStats, OutputKind, SimulationSpace};

/// Renders an unused instance's buffers are kept.
const KEEP_RENDERS: u64 = 600;
/// Bytes of one particle (`VfxParticle`).
pub const PARTICLE_BYTES: u64 = 64;
const COUNTER_BYTES: u64 = 32;

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

struct EffectPipelines {
    reset: wgpu::ComputePipeline,
    emit: wgpu::ComputePipeline,
    args: wgpu::ComputePipeline,
    update: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
    outputs: Vec<wgpu::RenderPipeline>,
    used: u64,
}

/// Readback states.
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const MAPPED: u8 = 3;

struct Instance {
    hash: u64,
    capacity: u32,
    outputs: usize,
    particles: wgpu::Buffer,
    counters: wgpu::Buffer,
    indirect: wgpu::Buffer,
    /// The update's dispatch arguments, copied out of `indirect` (a buffer can't be bound as
    /// storage and read as indirect arguments in one dispatch).
    dispatch: wgpu::Buffer,
    sim_params: wgpu::Buffer,
    props: wgpu::Buffer,
    ramps: wgpu::Buffer,
    ramp_len: usize,
    dead: wgpu::Buffer,
    alive: [wgpu::Buffer; 2],
    render_params: Vec<wgpu::Buffer>,
    /// Per parity (the alive list read).
    sim_groups: Vec<wgpu::BindGroup>,
    /// Per output, per parity (the alive list drawn).
    render_groups: Vec<[wgpu::BindGroup; 2]>,
    /// Alive list read by the next step (and drawn now).
    parity: u32,
    serial: u64,
    reset_gen: u64,
    data_key: u64,
    used: u64,
    readback: wgpu::Buffer,
    readback_state: Arc<AtomicU8>,
    last_stats: EffectStats,
}

/// One output of one instance drawn this render.
struct FrameDraw {
    id: u64,
    hash: u64,
    output: usize,
    texture: Option<String>,
    /// Soft particles (reads the scene depth copy).
    soft: bool,
}

/// Scene depth for soft particles: a bind group per scene copy, and a far fallback.
struct SoftDepth {
    fallback: wgpu::BindGroup,
    groups: HashMap<u64, wgpu::BindGroup>,
}

#[derive(Default)]
pub(super) struct Effects {
    /// Simulation, output and scene-depth bind group layouts.
    layouts: Option<(
        wgpu::BindGroupLayout,
        wgpu::BindGroupLayout,
        wgpu::BindGroupLayout,
    )>,
    soft: Option<SoftDepth>,
    pipelines: HashMap<u64, EffectPipelines>,
    instances: HashMap<u64, Instance>,
    renders: u64,
    frame: Vec<FrameDraw>,
    /// Instances stepped by the last [`Effects::prepare`] (tests, stats).
    pub stepped: usize,
    /// Instances drawn (inside a view) by the last prepare.
    pub visible: usize,
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

fn layouts(
    device: &wgpu::Device,
) -> (
    wgpu::BindGroupLayout,
    wgpu::BindGroupLayout,
    wgpu::BindGroupLayout,
) {
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
        ],
    });
    let depth = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty vfx scene depth"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: S::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    (sim, render, depth)
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

fn create_pipelines(
    device: &wgpu::Device,
    shared: &pipelines::Pipelines,
    sim_layout: &wgpu::BindGroupLayout,
    render_layout: &wgpu::BindGroupLayout,
    depth_layout: &wgpu::BindGroupLayout,
    effect: &CompiledEffect,
) -> EffectPipelines {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty vfx sim"),
        source: wgpu::ShaderSource::Wgsl(effect.sim_source.as_str().into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty vfx sim"),
        bind_group_layouts: &[Some(sim_layout)],
        immediate_size: 0,
    });
    let compute = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let outputs = effect
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
                OutputKind::Quad => wgpu::PrimitiveTopology::TriangleList,
                OutputKind::Point => wgpu::PrimitiveTopology::PointList,
                OutputKind::Line => wgpu::PrimitiveTopology::LineList,
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
    EffectPipelines {
        reset: compute("cs_reset"),
        emit: compute("cs_emit"),
        args: compute("cs_args"),
        update: compute("cs_update"),
        finish: compute("cs_finish"),
        outputs,
        used: 0,
    }
}

impl Instance {
    fn new(
        device: &wgpu::Device,
        sim_layout: &wgpu::BindGroupLayout,
        render_layout: &wgpu::BindGroupLayout,
        effect: &CompiledEffect,
        ramp_len: usize,
    ) -> Self {
        use wgpu::BufferUsages as U;
        let cap = effect.capacity.max(1) as u64;
        let storage = U::STORAGE | U::COPY_DST | U::COPY_SRC;
        let particles = buffer(device, "misty vfx particles", cap * PARTICLE_BYTES, storage);
        let dead = buffer(device, "misty vfx dead", cap * 4, storage);
        let alive = [
            buffer(device, "misty vfx alive", cap * 4, storage),
            buffer(device, "misty vfx alive", cap * 4, storage),
        ];
        let counters = buffer(device, "misty vfx counters", COUNTER_BYTES, storage);
        let outputs = effect.outputs.len();
        let indirect = buffer(
            device,
            "misty vfx indirect",
            16 + 32 * outputs.max(1) as u64,
            storage | U::INDIRECT,
        );
        let dispatch = buffer(device, "misty vfx dispatch", 16, U::INDIRECT | U::COPY_DST);
        let sim_params = buffer(
            device,
            "misty vfx params",
            std::mem::size_of::<SimParams>() as u64,
            U::UNIFORM | U::COPY_DST,
        );
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
        let mut i = Self {
            hash: effect.hash,
            capacity: effect.capacity.max(1),
            outputs,
            sim_groups: Vec::new(),
            render_groups: Vec::new(),
            particles,
            counters,
            indirect,
            dispatch,
            sim_params,
            props,
            ramps,
            ramp_len: ramp_len.max(codegen::RAMP_SAMPLES),
            dead,
            alive,
            render_params,
            parity: 0,
            serial: 0,
            reset_gen: 0,
            data_key: 0,
            used: 0,
            readback,
            readback_state: Arc::new(AtomicU8::new(IDLE)),
            last_stats: EffectStats::default(),
        };
        i.rebuild_groups(device, sim_layout, render_layout);
        i
    }

    fn rebuild_groups(
        &mut self,
        device: &wgpu::Device,
        sim_layout: &wgpu::BindGroupLayout,
        render_layout: &wgpu::BindGroupLayout,
    ) {
        fn bind(i: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding: i,
                resource: b.as_entire_binding(),
            }
        }
        let sim = |p: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty vfx sim"),
                layout: sim_layout,
                entries: &[
                    bind(0, &self.sim_params),
                    bind(1, &self.particles),
                    bind(2, &self.dead),
                    bind(3, &self.alive[p]),
                    bind(4, &self.alive[1 - p]),
                    bind(5, &self.counters),
                    bind(6, &self.indirect),
                    bind(7, &self.props),
                    bind(8, &self.ramps),
                ],
            })
        };
        self.sim_groups = vec![sim(0), sim(1)];
        self.render_groups = (0..self.outputs)
            .map(|k| {
                let group = |p: usize| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("misty vfx render"),
                        layout: render_layout,
                        entries: &[
                            bind(0, &self.particles),
                            bind(1, &self.alive[p]),
                            bind(2, &self.props),
                            bind(3, &self.ramps),
                            bind(4, &self.render_params[k]),
                        ],
                    })
                };
                [group(0), group(1)]
            })
            .collect();
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
            .map(|i| i.capacity as u64 * (PARTICLE_BYTES + 12) + (i.ramp_len * 16) as u64)
            .sum()
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
        cull: bool,
    ) {
        self.frame.clear();
        self.stepped = 0;
        self.visible = 0;
        self.renders += 1;
        let renders = self.renders;
        let _ = device.poll(wgpu::PollType::Poll);
        self.collect_readbacks();
        if draws.is_empty() {
            self.collect_garbage();
            return;
        }
        let (sim_layout, render_layout, depth_layout) =
            self.layouts.get_or_insert_with(|| layouts(device));
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
            self.soft = Some(SoftDepth {
                fallback: depth_group(device, depth_layout, &view),
                groups: HashMap::new(),
            });
        }
        let frustum = Frustum::from_matrix(view_proj);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty vfx simulate"),
        });
        let mut stepped = Vec::new();
        for d in draws {
            if cull && !frustum.intersects(&d.bounds) {
                continue;
            }
            self.visible += 1;
            let effect = &d.effect;
            let pipes = self.pipelines.entry(effect.hash).or_insert_with(|| {
                create_pipelines(
                    device,
                    shared,
                    sim_layout,
                    render_layout,
                    depth_layout,
                    effect,
                )
            });
            pipes.used = renders;
            let rebuild = self
                .instances
                .get(&d.id)
                .is_none_or(|i| i.hash != effect.hash || i.ramp_len < d.ramps.len());
            if rebuild {
                let inst = Instance::new(device, sim_layout, render_layout, effect, d.ramps.len());
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
            if reset || step {
                if reset {
                    inst.reset_gen = d.reset_gen;
                    inst.parity = 0;
                }
                let world_space = effect.space == SimulationSpace::World;
                let params = SimParams {
                    world: d.world.to_cols_array_2d(),
                    dt: if step { d.dt } else { 0.0 },
                    time: d.time,
                    spawn_count: if step { d.spawn_count } else { 0 },
                    capacity: inst.capacity,
                    seed: d.seed,
                    serial: d.serial as u32,
                    parity: inst.parity,
                    world_space: world_space as u32,
                };
                queue.write_buffer(&inst.sim_params, 0, bytemuck::bytes_of(&params));
                let group = &inst.sim_groups[inst.parity as usize];
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("misty vfx emit"),
                        timestamp_writes: None,
                    });
                    pass.set_bind_group(0, group, &[]);
                    if reset {
                        pass.set_pipeline(&pipes.reset);
                        pass.dispatch_workgroups(inst.capacity.div_ceil(WORKGROUP), 1, 1);
                    }
                    if step {
                        if params.spawn_count > 0 {
                            pass.set_pipeline(&pipes.emit);
                            pass.dispatch_workgroups(params.spawn_count.div_ceil(WORKGROUP), 1, 1);
                        }
                        pass.set_pipeline(&pipes.args);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                }
                if step {
                    encoder.copy_buffer_to_buffer(&inst.indirect, 0, &inst.dispatch, 0, 12);
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("misty vfx update"),
                        timestamp_writes: None,
                    });
                    pass.set_bind_group(0, group, &[]);
                    pass.set_pipeline(&pipes.update);
                    pass.dispatch_workgroups_indirect(&inst.dispatch, 0);
                    pass.set_pipeline(&pipes.finish);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                if step {
                    inst.serial = d.serial;
                    inst.parity = 1 - inst.parity;
                    self.stepped += 1;
                }
                if inst.readback_state.load(Ordering::Acquire) == IDLE {
                    encoder.copy_buffer_to_buffer(
                        &inst.counters,
                        0,
                        &inst.readback,
                        0,
                        COUNTER_BYTES,
                    );
                    inst.readback_state.store(COPIED, Ordering::Release);
                    stepped.push(d.id);
                }
            }
            // Render parameters follow the object every render.
            for (k, o) in effect.outputs.iter().enumerate() {
                let s = &o.settings;
                let texture = d.textures.get(k).cloned().flatten();
                let params = RenderParams {
                    world: d.world.to_cols_array_2d(),
                    time: d.time,
                    dt: d.dt,
                    world_space: (effect.space == SimulationSpace::World) as u32,
                    textured: texture.is_some() as u32,
                    flipbook: [
                        s.flipbook[0].max(1) as f32,
                        s.flipbook[1].max(1) as f32,
                        (s.flipbook_mode == crate::vfx::graph::FlipbookMode::Fps) as u32 as f32,
                        s.flipbook_fps,
                    ],
                    misc: [
                        s.line_length,
                        if s.soft_particles {
                            s.soft_distance
                        } else {
                            0.0
                        },
                        (s.kind != OutputKind::Quad) as u32 as f32,
                        0.0,
                    ],
                };
                queue.write_buffer(&inst.render_params[k], 0, bytemuck::bytes_of(&params));
                self.frame.push(FrameDraw {
                    id: d.id,
                    hash: effect.hash,
                    output: k,
                    texture,
                    soft: s.soft_particles && s.kind == OutputKind::Quad,
                });
            }
        }
        queue.submit([encoder.finish()]);
        for id in stepped {
            if let Some(inst) = self.instances.get(&id) {
                let state = inst.readback_state.clone();
                state.store(MAPPING, Ordering::Release);
                inst.readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |r| {
                        state.store(if r.is_ok() { MAPPED } else { IDLE }, Ordering::Release);
                    });
            }
        }
        self.collect_garbage();
    }

    /// Does an output drawn this render fade against the scene depth (needs the scene copy)?
    /// Any effect output to draw this render (Frame Debugger).
    pub fn has_draws(&self) -> bool {
        !self.frame.is_empty()
    }

    pub fn needs_scene(&self) -> bool {
        self.frame.iter().any(|f| f.soft)
    }

    /// Bind the scene depth copy `id` for soft particles this render.
    pub fn scene_depth(&mut self, device: &wgpu::Device, id: u64, view: &wgpu::TextureView) {
        let (Some((_, _, layout)), Some(soft)) = (&self.layouts, &mut self.soft) else {
            return;
        };
        if soft.groups.len() > 8 && !soft.groups.contains_key(&id) {
            soft.groups.clear();
        }
        soft.groups
            .entry(id)
            .or_insert_with(|| depth_group(device, layout, view));
    }

    /// Read finished counter copies and publish them.
    fn collect_readbacks(&mut self) {
        for (id, inst) in &mut self.instances {
            if inst.readback_state.load(Ordering::Acquire) != MAPPED {
                continue;
            }
            let c: Counters = {
                let view = inst.readback.slice(..).get_mapped_range().expect("mapped");
                bytemuck::pod_read_unaligned(&view[..COUNTER_BYTES as usize])
            };
            inst.readback.unmap();
            inst.readback_state.store(IDLE, Ordering::Release);
            inst.last_stats = EffectStats {
                alive: c.last_alive,
                spawned: c.spawned,
                reaped: c.reaped,
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
            if let Some(i) = self.instances.remove(&id)
                && i.readback_state.load(Ordering::Acquire) == MAPPED
            {
                i.readback.unmap();
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
            let (Some(pipeline), Some(groups)) = (
                pipes.outputs.get(f.output),
                inst.render_groups.get(f.output),
            ) else {
                continue;
            };
            let Some(material) = textures.get(f.texture.as_deref(), None) else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(1, material, &[]);
            pass.set_bind_group(2, &groups[inst.parity as usize], &[]);
            pass.set_bind_group(3, depth, &[]);
            pass.draw_indirect(&inst.indirect, 16 + 32 * f.output as u64);
            let n = inst.last_stats.alive as u64;
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

    /// Blocking read of an instance's counters (tests).
    #[cfg(test)]
    pub(crate) fn read_counters(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
    ) -> Option<Counters> {
        let inst = self.instances.get(&id)?;
        let bytes = read_buffer(device, queue, &inst.counters, COUNTER_BYTES);
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
        let Some(inst) = self.instances.get(&id) else {
            return Vec::new();
        };
        let bytes = read_buffer(
            device,
            queue,
            &inst.particles,
            inst.capacity as u64 * PARTICLE_BYTES,
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
        let c = self.read_counters(device, queue, id)?;
        let size = inst.capacity as u64 * 4;
        let dead: Vec<u32> =
            bytemuck::cast_slice(&read_buffer(device, queue, &inst.dead, size)).to_vec();
        let alive: Vec<u32> = bytemuck::cast_slice(&read_buffer(
            device,
            queue,
            &inst.alive[inst.parity as usize],
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
