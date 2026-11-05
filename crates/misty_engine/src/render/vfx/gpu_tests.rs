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
