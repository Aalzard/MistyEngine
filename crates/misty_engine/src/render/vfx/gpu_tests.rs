//! Headless GPU checks of Visual Effects (skipped without an adapter): the dead list and alive
//! lists stay consistent (spawned, reaped, alive ≤ capacity), spawn rates match the CPU spawn
//! system, a seed gives the same particles, a million particles simulate, and an additive
//! effect lights up the image.

use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Environment, Frame, PostSettings, Renderer};
use crate::vfx::graph::{ContextKind, EffectGraph};
use crate::vfx::{
    Attribute, BlockKind, CompiledEffect, Composition, ContextType, EffectInstance, Value,
    VisualEffect, compile,
};

const W: u32 = 256;
const H: u32 = 160;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 1.0, -8.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 100.0,
    }
}

struct Setup {
    camera: Camera,
    environment: Environment,
}

impl Setup {
    fn new() -> Self {
        let mut environment = Environment::default();
        environment.settings.environment_lighting.intensity = 0.0;
        Self {
            camera: camera(),
            environment,
        }
    }

    fn frame(&self) -> Frame<'_> {
        Frame {
            camera: &self.camera,
            grid: None,
            skybox: false,
            background: [0.0; 3],
            environment: &self.environment,
            post: PostSettings {
                bloom_intensity: 0.0,
                ..Default::default()
            },
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &[],
            water: &[],
            shaded: true,
            lights: &[],
        }
    }
}

fn set(g: &mut EffectGraph, block: u32, name: &str, v: Value) {
    g.block_mut(block).unwrap().values.insert(name.into(), v);
}

fn attribute(g: &mut EffectGraph, ctx: u32, a: Attribute, v: Value) {
    let b = g
        .add_block(
            ctx,
            BlockKind::SetAttribute {
                attribute: a,
                random: false,
                composition: Composition::Overwrite,
            },
        )
        .unwrap();
    set(g, b, a.label(), v);
}

/// An effect: one spawn block, sphere positions, a lifetime, capacity.
fn effect(
    capacity: u32,
    spawn: BlockKind,
    inputs: &[(&str, f32)],
    lifetime: f32,
) -> Arc<CompiledEffect> {
    let mut g = EffectGraph::default();
    let s = g.add_context(ContextType::Spawn, [0.0, 0.0]);
    let b = g.add_block(s, spawn).unwrap();
    for (n, v) in inputs {
        set(&mut g, b, n, Value::Float(*v));
    }
    let init = g.add_context(ContextType::Initialize, [0.0, 100.0]);
    if let Some(c) = g.context_mut(init)
        && let ContextKind::Initialize(st) = &mut c.kind
    {
        st.capacity = capacity;
        st.bounds_size = [100.0; 3];
    }
    g.add_block(
        init,
        BlockKind::PositionSphere {
            mode: crate::vfx::ShapeMode::Volume,
        },
    );
    g.add_block(init, BlockKind::VelocityFromDirection);
    attribute(&mut g, init, Attribute::Lifetime, Value::Float(lifetime));
    attribute(&mut g, init, Attribute::Size, Value::Float(0.3));
    attribute(
        &mut g,
        init,
        Attribute::Color,
        Value::Color([3.0, 1.5, 0.5, 1.0]),
    );
    let upd = g.add_context(ContextType::Update, [0.0, 300.0]);
    g.add_block(upd, BlockKind::LinearDrag);
    g.add_context(ContextType::Output, [0.0, 400.0]);
    Arc::new(compile(&g).unwrap_or_else(|e| panic!("{e:?}")))
}

struct Run {
    renderer: Renderer,
    target: crate::render::RenderTarget,
    setup: Setup,
    inst: EffectInstance,
    comp: VisualEffect,
}

impl Run {
    fn new(device: &wgpu::Device, effect: Arc<CompiledEffect>, seed: u32) -> Self {
        let renderer = Renderer::new(device);
        let target = renderer.create_target(device, W, H);
        let comp = VisualEffect {
            seed,
            reseed_on_play: false,
            ..Default::default()
        };
        let mut inst = EffectInstance::with_effect(effect, &comp);
        inst.play(&comp);
        Self {
            renderer,
            target,
            setup: Setup::new(),
            inst,
            comp,
        }
    }

    fn step(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, dt: f32) {
        self.inst.step(&self.comp, dt);
        let d = self.inst.draw(Mat4::IDENTITY, &self.comp).unwrap();
        let frame = self.setup.frame();
        self.renderer
            .render_with_effects(device, queue, &self.target, &frame, &[d]);
    }

    fn counters(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> super::Counters {
        self.renderer
            .effects
            .read_counters(device, queue, self.inst.id)
            .expect("instance on the GPU")
    }

    fn particles(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 16]> {
        self.renderer
            .effects
            .read_particles(device, queue, self.inst.id)
    }
}

#[test]
fn free_list_and_alive_lists_stay_consistent() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // Bursts of 600 every 0.1 s into 1000 slots, living 0.35 s.
    let fx = effect(
        1000,
        BlockKind::PeriodicBurst,
        &[("Count", 600.0), ("Period", 0.1), ("Delay", 0.0)],
        0.35,
    );
    let mut run = Run::new(&device, fx, 1);
    run.step(&device, &queue, 0.05);
    let c = run.counters(&device, &queue);
    assert_eq!(c.spawned, 600);
    assert_eq!(c.last_alive, 600);
    assert_eq!(c.dead, 400);
    run.step(&device, &queue, 0.1);
    let c = run.counters(&device, &queue);
    assert_eq!(
        c.spawned, 1000,
        "only 400 slots were free for the second burst"
    );
    assert_eq!(c.last_alive, 1000);
    assert_eq!(c.dead, 0);
    let (dead, alive) = run
        .renderer
        .effects
        .read_lists(&device, &queue, run.inst.id)
        .unwrap();
    assert!(dead.is_empty());
    let mut sorted = alive.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 1000, "every slot listed once");
    assert!(sorted.iter().all(|i| *i < 1000));
    // Old particles die, later bursts refill; alive never exceeds capacity.
    for _ in 0..12 {
        run.step(&device, &queue, 0.1);
        let c = run.counters(&device, &queue);
        assert!(c.last_alive <= 1000);
        assert_eq!(
            c.dead as u32 + c.last_alive,
            1000,
            "every slot dead or alive"
        );
        assert_eq!(c.spawned - c.reaped, c.last_alive);
    }
    // Stop: everything dies and goes back on the dead list.
    run.inst.stop();
    for _ in 0..6 {
        run.step(&device, &queue, 0.1);
    }
    let c = run.counters(&device, &queue);
    assert_eq!(c.last_alive, 0);
    assert_eq!(c.reaped, c.spawned);
    let (dead, alive) = run
        .renderer
        .effects
        .read_lists(&device, &queue, run.inst.id)
        .unwrap();
    assert!(alive.is_empty());
    let mut d = dead.clone();
    d.sort_unstable();
    assert_eq!(
        d,
        (0..1000).collect::<Vec<u32>>(),
        "the dead list is a permutation"
    );
    assert!(run.particles(&device, &queue).is_empty());
}

#[test]
fn spawn_rate_matches_the_cpu_and_reinit_clears() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let fx = effect(10_000, BlockKind::ConstantRate, &[("Rate", 1000.0)], 100.0);
    let mut run = Run::new(&device, fx, 3);
    for _ in 0..60 {
        run.step(&device, &queue, 1.0 / 60.0);
    }
    let c = run.counters(&device, &queue);
    assert_eq!(c.spawned as u64, run.inst.requested);
    assert!((999..=1000).contains(&c.last_alive), "{}", c.last_alive);
    let particles = run.particles(&device, &queue);
    assert_eq!(particles.len() as u32, c.last_alive);
    // Particles age: one second old at most.
    let oldest = particles.iter().map(|p| p[3]).fold(0.0, f32::max);
    assert!(oldest <= 1.01 && oldest > 0.9, "oldest {oldest}");
    run.inst.reinit(&run.comp);
    run.step(&device, &queue, 0.0);
    let c = run.counters(&device, &queue);
    assert_eq!(c.last_alive, 0, "Reinit clears the particles");
    assert_eq!(c.dead, 10_000);
}

#[test]
fn same_seed_same_particles() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let fx = effect(
        4096,
        BlockKind::SingleBurst,
        &[("Count", 500.0), ("Delay", 0.0)],
        5.0,
    );
    let positions = |seed: u32| {
        let mut run = Run::new(&device, fx.clone(), seed);
        for _ in 0..10 {
            run.step(&device, &queue, 1.0 / 30.0);
        }
        let mut p: Vec<[u32; 3]> = run
            .particles(&device, &queue)
            .iter()
            .map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()])
            .collect();
        p.sort_unstable();
        p
    };
    let a = positions(42);
    let b = positions(42);
    let c = positions(43);
    assert_eq!(a.len(), 500);
    assert_eq!(a, b, "same seed, same particles (whatever slots they got)");
    assert_ne!(a, c, "another seed moves them");
}

#[test]
fn a_million_particles_simulate() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let n = 1u32 << 20;
    let fx = effect(
        n,
        BlockKind::SingleBurst,
        &[("Count", n as f32), ("Delay", 0.0)],
        10.0,
    );
    let mut run = Run::new(&device, fx, 5);
    let start = std::time::Instant::now();
    for _ in 0..10 {
        run.step(&device, &queue, 1.0 / 60.0);
    }
    let c = run.counters(&device, &queue);
    eprintln!("10 steps of 1M particles: {:?}", start.elapsed());
    assert_eq!(c.last_alive, n);
    assert_eq!(c.dead, 0);
}

#[test]
fn additive_particles_light_the_image_and_culled_ones_wait() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let fx = effect(2000, BlockKind::ConstantRate, &[("Rate", 2000.0)], 3.0);
    let mut run = Run::new(&device, fx.clone(), 9);
    for _ in 0..20 {
        run.step(&device, &queue, 1.0 / 30.0);
    }
    let img = read_back(&device, &queue, &run.target);
    let lum: Vec<f32> = img
        .chunks(4)
        .map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0)
        .collect();
    let bright = lum.iter().filter(|l| **l > 60.0).count();
    let mean = lum.iter().sum::<f32>() / lum.len() as f32;
    let var = lum.iter().map(|l| (l - mean) * (l - mean)).sum::<f32>() / lum.len() as f32;
    eprintln!("bright {bright}, mean {mean}, std {}", var.sqrt());
    assert!(bright > 200, "{bright} bright pixels");
    assert!(var.sqrt() > 5.0, "not flat");
    // Bounds far behind the camera: the effect is culled and doesn't simulate.
    let mut culled = Run::new(&device, effect(64, BlockKind::ConstantRate, &[], 1.0), 9);
    culled.inst.step(&culled.comp, 0.1);
    let mut d = culled.inst.draw(Mat4::IDENTITY, &culled.comp).unwrap();
    d.world = Mat4::from_translation(Vec3::new(0.0, 0.0, -500.0));
    d.bounds = crate::vfx::world_bounds(&d.effect, &d.world);
    let frame = culled.setup.frame();
    culled
        .renderer
        .render_with_effects(&device, &queue, &culled.target, &frame, &[d]);
    assert_eq!(culled.renderer.effects.stepped, 0);
    assert_eq!(culled.renderer.effects.instance_count(), 0);
}

// ---- Round 20: GPU sort, ribbons, trails, mesh output, GPU events, depth collision.

use crate::vfx::codegen::SortKey;
use crate::vfx::graph::{BlendMode, EventSource, OutputKind, OutputSettings};

struct NoMaterials;
impl crate::render_scene::MaterialSource for NoMaterials {
    fn params(&self, _: Option<&str>) -> crate::render::MaterialParams {
        crate::render::MaterialParams::default()
    }
}

/// The graph of [`effect`] (to change before compiling).
fn base_graph(
    capacity: u32,
    spawn: BlockKind,
    inputs: &[(&str, f32)],
    lifetime: f32,
) -> EffectGraph {
    let mut g = EffectGraph::default();
    let s = g.add_context(ContextType::Spawn, [0.0, 0.0]);
    let b = g.add_block(s, spawn).unwrap();
    for (n, v) in inputs {
        set(&mut g, b, n, Value::Float(*v));
    }
    let init = g.add_context(ContextType::Initialize, [0.0, 100.0]);
    if let Some(c) = g.context_mut(init)
        && let ContextKind::Initialize(st) = &mut c.kind
    {
        st.capacity = capacity;
        st.bounds_size = [100.0; 3];
    }
    g.add_context(ContextType::Update, [0.0, 300.0]);
    attribute(&mut g, init, Attribute::Lifetime, Value::Float(lifetime));
    g
}

fn init_of(g: &EffectGraph, system: u32) -> u32 {
    g.system_context(system, ContextType::Initialize)
        .unwrap()
        .id
}

fn update_of(g: &EffectGraph, system: u32) -> u32 {
    g.system_context(system, ContextType::Update).unwrap().id
}

fn sphere(g: &mut EffectGraph, ctx: u32, center: [f32; 3], radius: f32) {
    let b = g
        .add_block(
            ctx,
            BlockKind::PositionSphere {
                mode: crate::vfx::ShapeMode::Volume,
            },
        )
        .unwrap();
    set(g, b, "Center", Value::Vec3(center));
    set(g, b, "Radius", Value::Float(radius));
}

fn output(g: &mut EffectGraph, system: u32, s: OutputSettings) {
    let out = g.add_context(ContextType::Output, [0.0, 500.0]);
    let c = g.context_mut(out).unwrap();
    c.system = system;
    c.kind = ContextKind::Output(s);
}

fn compiled(g: &EffectGraph) -> Arc<CompiledEffect> {
    Arc::new(compile(g).unwrap_or_else(|e| panic!("{e:?}")))
}

/// Every particle slot of a system (index = slot), alive or not.
fn all_particles(
    run: &Run,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    system: usize,
) -> Vec<[f32; 16]> {
    let sys = run
        .renderer
        .effects
        .system(run.inst.id, system)
        .expect("system on the GPU");
    let bytes = super::read_buffer(device, queue, &sys.particles, sys.capacity as u64 * 64);
    bytemuck::cast_slice(&bytes).to_vec()
}

#[test]
fn alpha_particles_sort_back_to_front_on_the_gpu() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // 3000 particles: a 4096-key sort (global steps and workgroup merges).
    let mut g = base_graph(4000, BlockKind::SingleBurst, &[("Count", 3000.0)], 10.0);
    let init = init_of(&g, 0);
    sphere(&mut g, init, [0.0, 1.0, 0.0], 3.0);
    output(
        &mut g,
        0,
        OutputSettings {
            blend: BlendMode::Alpha,
            ..Default::default()
        },
    );
    let mut run = Run::new(&device, compiled(&g), 4);
    run.step(&device, &queue, 1.0 / 30.0);
    run.step(&device, &queue, 1.0 / 30.0);
    assert!(run.renderer.effects.sorted >= 1, "sorted this render");
    let sorted = run
        .renderer
        .effects
        .read_sorted(&device, &queue, run.inst.id, 0, SortKey::Distance)
        .unwrap();
    assert_eq!(sorted.len(), 3000);
    let particles = all_particles(&run, &device, &queue, 0);
    let cam = run.setup.camera.position;
    let dist: Vec<f32> = sorted
        .iter()
        .map(|i| {
            let p = particles[*i as usize];
            assert_eq!(p[15].to_bits(), 1, "only live particles are listed");
            Vec3::new(p[0], p[1], p[2]).distance(cam)
        })
        .collect();
    for w in dist.windows(2) {
        assert!(w[0] >= w[1], "back to front: {} then {}", w[0], w[1]);
    }
    let mut unique = sorted.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 3000, "each particle once");
    // Another camera, another order (sorted every view).
    run.setup.camera.position = Vec3::new(0.0, 1.0, 8.0);
    run.setup.camera.rotation = Quat::from_rotation_y(std::f32::consts::PI);
    let d = run.inst.draw(Mat4::IDENTITY, &run.comp).unwrap();
    let frame = run.setup.frame();
    run.renderer
        .render_with_effects(&device, &queue, &run.target, &frame, &[d]);
    let again = run
        .renderer
        .effects
        .read_sorted(&device, &queue, run.inst.id, 0, SortKey::Distance)
        .unwrap();
    let cam = run.setup.camera.position;
    let first = particles[again[0] as usize];
    let last = particles[again[2999] as usize];
    assert!(
        Vec3::new(first[0], first[1], first[2]).distance(cam)
            >= Vec3::new(last[0], last[1], last[2]).distance(cam)
    );
    assert_ne!(
        again[0], sorted[0],
        "the farthest particle changed with the camera"
    );
}

#[test]
fn ribbons_join_particles_youngest_first() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut g = base_graph(2048, BlockKind::ConstantRate, &[("Rate", 300.0)], 3.0);
    let init = init_of(&g, 0);
    sphere(&mut g, init, [0.0, 1.0, 0.0], 0.2);
    output(
        &mut g,
        0,
        OutputSettings {
            kind: OutputKind::Ribbon,
            ..Default::default()
        },
    );
    let mut run = Run::new(&device, compiled(&g), 5);
    for _ in 0..20 {
        run.step(&device, &queue, 1.0 / 30.0);
    }
    let c = run.counters(&device, &queue);
    let sorted = run
        .renderer
        .effects
        .read_sorted(&device, &queue, run.inst.id, 0, SortKey::Age)
        .unwrap();
    assert_eq!(sorted.len() as u32, c.last_alive);
    let particles = all_particles(&run, &device, &queue, 0);
    let ages: Vec<f32> = sorted.iter().map(|i| particles[*i as usize][3]).collect();
    for w in ages.windows(2) {
        assert!(w[0] <= w[1], "youngest first: {} then {}", w[0], w[1]);
    }
    // Births spread over the step: no two particles share an age.
    let mut bits: Vec<u32> = ages.iter().map(|a| a.to_bits()).collect();
    bits.dedup();
    assert_eq!(bits.len(), ages.len(), "distinct ages");
    let img = read_back(&device, &queue, &run.target);
    let lit = img.chunks(4).filter(|p| p[0] > 30).count();
    assert!(lit > 50, "the ribbon draws: {lit} pixels");
}

#[test]
fn trails_remember_the_last_positions() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut g = base_graph(64, BlockKind::SingleBurst, &[("Count", 10.0)], 10.0);
    let init = init_of(&g, 0);
    sphere(&mut g, init, [0.0, 1.0, 0.0], 0.1);
    g.add_block(init, BlockKind::VelocityFromDirection).unwrap();
    output(
        &mut g,
        0,
        OutputSettings {
            kind: OutputKind::Trail,
            trail_points: 6,
            ..Default::default()
        },
    );
    let fx = compiled(&g);
    assert_eq!(fx.root().trail_points, 6);
    let mut run = Run::new(&device, fx, 6);
    let dt = 1.0 / 30.0;
    for _ in 0..10 {
        run.step(&device, &queue, dt);
    }
    let ring = run
        .renderer
        .effects
        .read_trail(&device, &queue, run.inst.id, 0);
    let particles = all_particles(&run, &device, &queue, 0);
    let head = (run.inst.serial % 6) as usize;
    let mut checked = 0;
    for (i, p) in particles.iter().enumerate() {
        if p[15].to_bits() != 1 {
            continue;
        }
        let pos = Vec3::new(p[0], p[1], p[2]);
        let vel = Vec3::new(p[4], p[5], p[6]);
        let point = |k: usize| {
            let v = ring[i * 6 + (head + 6 - k) % 6];
            Vec3::new(v[0], v[1], v[2])
        };
        assert!(
            point(0).distance(pos) < 1e-4,
            "the newest point is the particle"
        );
        for k in 0..5 {
            let step = point(k).distance(point(k + 1));
            assert!(
                (step - vel.length() * dt).abs() < 1e-3,
                "one step apart: {step} vs {}",
                vel.length() * dt
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 10);
    let img = read_back(&device, &queue, &run.target);
    assert!(
        img.chunks(4).filter(|p| p[0] > 30).count() > 20,
        "trails draw"
    );
}

#[test]
fn mesh_particles_draw_opaque_cubes() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut g = base_graph(512, BlockKind::SingleBurst, &[("Count", 200.0)], 10.0);
    let init = init_of(&g, 0);
    sphere(&mut g, init, [0.0, 1.0, 0.0], 2.0);
    attribute(&mut g, init, Attribute::Size, Value::Float(0.4));
    attribute(&mut g, init, Attribute::Angle, Value::Float(30.0));
    attribute(
        &mut g,
        init,
        Attribute::Color,
        Value::Color([4.0, 3.0, 1.0, 1.0]),
    );
    output(
        &mut g,
        0,
        OutputSettings {
            kind: OutputKind::Mesh,
            blend: BlendMode::Opaque,
            ..Default::default()
        },
    );
    let mut run = Run::new(&device, compiled(&g), 7);
    for _ in 0..3 {
        run.step(&device, &queue, 1.0 / 30.0);
    }
    let img = read_back(&device, &queue, &run.target);
    let lit = img.chunks(4).filter(|p| p[0] > 40).count();
    eprintln!("{lit} lit pixels");
    assert!(lit > 800, "200 cubes cover the view: {lit}");
}

#[test]
fn death_events_spawn_child_particles() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut g = base_graph(256, BlockKind::SingleBurst, &[("Count", 100.0)], 0.1);
    let init = init_of(&g, 0);
    sphere(&mut g, init, [0.0, 1.0, 0.0], 0.5);
    output(&mut g, 0, OutputSettings::default());
    let child = g.add_event_system(0, EventSource::OnDie, [600.0, 0.0]);
    if let Some(c) = g.context_mut(g.system_context(child, ContextType::GpuEvent).unwrap().id)
        && let ContextKind::GpuEvent(s) = &mut c.kind
    {
        s.count = 5;
    }
    let cinit = init_of(&g, child);
    if let Some(c) = g.context_mut(cinit)
        && let ContextKind::Initialize(st) = &mut c.kind
    {
        st.capacity = 1000;
    }
    attribute(&mut g, cinit, Attribute::Lifetime, Value::Float(10.0));
    sphere(&mut g, cinit, [0.0; 3], 0.05);
    let fx = compiled(&g);
    assert_eq!(fx.systems.len(), 2);
    let mut run = Run::new(&device, fx, 8);
    for _ in 0..8 {
        run.step(&device, &queue, 1.0 / 30.0);
    }
    let parent = run.counters(&device, &queue);
    assert_eq!(parent.spawned, 100);
    assert_eq!(parent.reaped, 100, "every parent died");
    let c = run
        .renderer
        .effects
        .read_system_counters(&device, &queue, run.inst.id, 1)
        .unwrap();
    assert_eq!(c.spawned, 500, "five per death");
    assert_eq!(c.last_alive, 500);
    // Children start where their parents died (inside the parents' sphere + a step).
    let kids = run
        .renderer
        .effects
        .read_system_particles(&device, &queue, run.inst.id, 1);
    assert_eq!(kids.len(), 500);
    assert!(
        kids.iter()
            .all(|p| Vec3::new(p[0], p[1], p[2]).distance(Vec3::Y) < 0.7),
        "children at their parents"
    );
}

#[test]
fn collision_events_fire_on_impact() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut g = base_graph(64, BlockKind::SingleBurst, &[("Count", 20.0)], 10.0);
    let init = init_of(&g, 0);
    sphere(&mut g, init, [0.0, 1.0, 0.0], 0.1);
    let upd = update_of(&g, 0);
    g.add_block(upd, BlockKind::Gravity).unwrap();
    let plane = g.add_block(upd, BlockKind::CollidePlane).unwrap();
    set(&mut g, plane, "Bounce", Value::Float(0.0));
    output(&mut g, 0, OutputSettings::default());
    let child = g.add_event_system(0, EventSource::OnCollide, [600.0, 0.0]);
    let cinit = init_of(&g, child);
    attribute(&mut g, cinit, Attribute::Lifetime, Value::Float(10.0));
    if let Some(c) = g.context_mut(g.system_context(child, ContextType::GpuEvent).unwrap().id)
        && let ContextKind::GpuEvent(s) = &mut c.kind
    {
        s.count = 2;
    }
    let mut run = Run::new(&device, compiled(&g), 9);
    // Before the fall ends: no events.
    for _ in 0..8 {
        run.step(&device, &queue, 1.0 / 30.0);
    }
    let c = run
        .renderer
        .effects
        .read_system_counters(&device, &queue, run.inst.id, 1)
        .unwrap();
    assert_eq!(c.spawned, 0, "still falling");
    for _ in 0..30 {
        run.step(&device, &queue, 1.0 / 30.0);
    }
    let c = run
        .renderer
        .effects
        .read_system_counters(&device, &queue, run.inst.id, 1)
        .unwrap();
    // One impact each (no bounce; resting contact is too slow to count as one).
    assert_eq!(c.spawned, 40, "two per impact");
    let kids = run
        .renderer
        .effects
        .read_system_particles(&device, &queue, run.inst.id, 1);
    assert!(
        kids.iter().all(|p| p[1].abs() < 0.3),
        "children on the floor"
    );
}

/// A floor plane under a camera looking at the origin, and an effect on its own object.
fn floor_scene(effect: Arc<CompiledEffect>) -> (crate::scene::Scene, hecs::Entity) {
    use crate::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Transform};
    let mut s = Scene::default_3d("DepthCollision");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.5, -6.0))
                .with_euler(Vec3::new(18.0, 0.0, 0.0)),
        );
    }
    let floor = s.spawn("Floor", None);
    s.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(floor, MeshRenderer::default());
    let e = s.spawn("Effect", None);
    let comp = VisualEffect {
        seed: 3,
        reseed_on_play: false,
        ..Default::default()
    };
    let mut inst = EffectInstance::with_effect(effect, &comp);
    inst.play(&comp);
    s.add(e, comp);
    s.world.insert_one(e, inst).unwrap();
    (s, e)
}

fn falling(depth: bool, capacity: u32, rate: f32, alpha: bool) -> Arc<CompiledEffect> {
    let mut g = base_graph(capacity, BlockKind::ConstantRate, &[("Rate", rate)], 2.0);
    let init = init_of(&g, 0);
    sphere(&mut g, init, [0.0, 2.0, 0.0], 0.6);
    attribute(&mut g, init, Attribute::Size, Value::Float(0.05));
    attribute(
        &mut g,
        init,
        Attribute::Color,
        Value::Color([2.0, 1.0, 0.4, 0.8]),
    );
    let upd = update_of(&g, 0);
    g.add_block(upd, BlockKind::Gravity).unwrap();
    if depth {
        let b = g.add_block(upd, BlockKind::CollideDepthBuffer).unwrap();
        set(&mut g, b, "Bounce", Value::Float(0.2));
    }
    output(
        &mut g,
        0,
        OutputSettings {
            blend: if alpha {
                BlendMode::Alpha
            } else {
                BlendMode::Additive
            },
            ..Default::default()
        },
    );
    compiled(&g)
}

#[test]
fn depth_buffer_collision_keeps_particles_on_the_floor() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let lowest = |depth: bool| {
        let (mut scene, e) = floor_scene(falling(depth, 4096, 400.0, false));
        let materials = NoMaterials;
        let mut renderer = Renderer::new(&device);
        let target = renderer.create_target(&device, W, H);
        for _ in 0..50 {
            crate::vfx::update(&mut scene, 1.0 / 30.0);
            let view = crate::render_scene::GameView::new(&scene, &materials).unwrap();
            renderer.render_with_effects(&device, &queue, &target, &view.frame(), &view.effects);
        }
        let id = scene.world.get::<&EffectInstance>(e).unwrap().id;
        let particles = renderer.effects.read_particles(&device, &queue, id);
        assert!(particles.len() > 300, "{} alive", particles.len());
        // Particles that fell for a while (age over a second) settled or fell through.
        let old: Vec<f32> = particles
            .iter()
            .filter(|p| p[3] > 1.0)
            .map(|p| p[1])
            .collect();
        assert!(!old.is_empty());
        old.iter().copied().fold(f32::MAX, f32::min)
    };
    let without = lowest(false);
    let with = lowest(true);
    eprintln!("lowest after a second: {without} without, {with} with depth collision");
    assert!(
        without < -1.0,
        "they fall through the floor without the block"
    );
    assert!(with > -0.15, "they rest on the floor with it: {with}");
}

/// Cost of 100k GPU particles, alpha-sorted every frame and colliding with the depth buffer,
/// over a floor (run on purpose: `cargo test --release -p misty_engine --lib
/// vfx_cost_report -- --ignored --nocapture`).
#[test]
#[ignore = "timing report; run on purpose with the GPU lock held"]
fn vfx_cost_report() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let materials = NoMaterials;
    let (w, h) = (1920, 1080);
    let measure = |fx: Option<Arc<CompiledEffect>>, label: &str| {
        let fx_or_dummy = fx.clone().unwrap_or_else(|| falling(false, 16, 0.0, false));
        let (mut scene, e) = floor_scene(fx_or_dummy);
        if fx.is_none() {
            let _ = scene.world.despawn(e);
        }
        let mut renderer = Renderer::new(&device);
        renderer.effects.timer = super::StepTimer::new(&device, &queue);
        let target = renderer.create_target(&device, w, h);
        let (mut sim, mut frame_ms, mut n) = (0.0f32, 0.0f32, 0);
        let mut alive = 0;
        for i in 0..240 {
            crate::vfx::update(&mut scene, 1.0 / 60.0);
            let view = crate::render_scene::GameView::new(&scene, &materials).unwrap();
            renderer.render_with_effects(&device, &queue, &target, &view.frame(), &view.effects);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 180 {
                if let Some(t) = &renderer.effects.timer
                    && !view.effects.is_empty()
                {
                    sim += t.read_ms(&device);
                }
                if let Some(ms) = renderer.gpu_ms() {
                    frame_ms += ms;
                    n += 1;
                }
            }
            if i == 239
                && let Ok(inst) = scene.world.get::<&EffectInstance>(e)
            {
                alive = renderer
                    .effects
                    .read_counters(&device, &queue, inst.id)
                    .map_or(0, |c| c.last_alive);
            }
        }
        let sim = sim / 60.0;
        let frame = frame_ms / n.max(1) as f32;
        eprintln!("{label:<44} alive {alive:>6}  sim+sort {sim:6.3} ms  frame {frame:6.3} ms");
        (sim, frame)
    };
    eprintln!(
        "adapter: {:?}",
        device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
    );
    let (_, base) = measure(None, "floor only (no effect)");
    let (s1, f1) = measure(
        Some(falling(false, 100_000, 50_000.0, false)),
        "100k additive, no sort, no collision",
    );
    let (s2, f2) = measure(
        Some(falling(false, 100_000, 50_000.0, true)),
        "100k alpha (GPU sort every frame)",
    );
    let (s3, f3) = measure(
        Some(falling(true, 100_000, 50_000.0, false)),
        "100k additive + depth collision",
    );
    let (s4, f4) = measure(
        Some(falling(true, 100_000, 50_000.0, true)),
        "100k alpha + sort + depth collision",
    );
    eprintln!(
        "frame cost over the floor: additive {:.3} ms, sorted {:.3} ms, collision {:.3} ms, both {:.3} ms",
        f1 - base,
        f2 - base,
        f3 - base,
        f4 - base
    );
    eprintln!(
        "sort ≈ {:.3} ms, depth collision ≈ {:.3} ms (simulation timestamps)",
        s2 - s1,
        s3 - s1
    );
    let _ = s4;
}
