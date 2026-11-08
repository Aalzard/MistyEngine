//! VfxDemo2 sample in `SampleProject`: round 20's effects at night over a floor.
//!
//! GPU Effect Graphs: **Fireworks** (rockets with Trail outputs bursting through a GPU Event
//! system on death into trailing sparks), **Rain Splash** (rain lines colliding with the depth
//! buffer, splashes spawned by an On Collide GPU Event system, alpha-sorted), **Debris** (lit
//! mesh cubes tumbling and bouncing), **Ribbon** (one ribbon through the particles of a path
//! made by the `Orbit` sub-graph) and **Smoke** (alpha-sorted flipbook smoke).
//!
//! Particle Systems: **Embers** (Noise, Trails, a Death sub emitter popping into sparks),
//! **Sparks Shower** (World collision on the floor and a crate, a Collision sub emitter,
//! stretched billboards, limited velocity) and **Flipbook Smoke** (Texture Sheet Animation,
//! curves and random-between-two-curves over lifetime).
//!
//! The smoke sheet is generated here (our own procedural texture, CC0). Regenerate with
//! `cargo test -p misty_editor -- --ignored write_vfx_demo2`; the other tests check the
//! committed files still match, compile, play and (on a GPU) render.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::particles::{
    Burst, CollisionType, Curve, MinMax, MinMaxColor, ParticleBlend, ParticleSystem, RenderMode,
    ShapeKind, SimulationSpace, SubEmitter, SubEmitterTrigger,
};
use misty_engine::scene::{
    BoxCollider, ClearFlags, Light, MeshCollider, MeshFilter, MeshRenderer, PrimitiveMesh, Scene,
    Transform,
};
use misty_engine::vfx::graph::{
    BlendMode, ContextKind, EffectGraph, EventSource, OutputKind, OutputSettings,
    SimulationSpace as FxSpace,
};
use misty_engine::vfx::types::{Gradient, Value};
use misty_engine::vfx::{
    Attribute, BlockKind, Composition, ContextType, OperatorKind, PropertyValue, ShapeMode,
    VisualEffect,
};

pub const SCENE: &str = "Assets/Scenes/VfxDemo2.scene";
pub const FIREWORKS: &str = "Assets/Effects/Fireworks.effect";
pub const RAIN: &str = "Assets/Effects/Rain Splash.effect";
pub const DEBRIS: &str = "Assets/Effects/Debris.effect";
pub const RIBBON: &str = "Assets/Effects/Ribbon.effect";
pub const SMOKE: &str = "Assets/Effects/Smoke.effect";
pub const ORBIT: &str = "Assets/Effects/Orbit.vfxoperator";
pub const SMOKE_SHEET: &str = "Assets/Textures/VfxDemo2 Smoke Sheet.png";
const GROUND_MAT: &str = "Assets/Materials/Vfx Demo2 Ground.mat";
const CRATE_MAT: &str = "Assets/Materials/Vfx Demo2 Crate.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn gradient(colors: &[(f32, [f32; 3])], alphas: &[(f32, f32)]) -> Gradient {
    Gradient::from_keys(colors, alphas)
}

/// Small graph builder.
struct B {
    g: EffectGraph,
}

impl B {
    fn new() -> Self {
        Self {
            g: EffectGraph::default(),
        }
    }

    fn context(&mut self, t: ContextType, x: f32, y: f32) -> u32 {
        self.g.add_context(t, [x, y])
    }

    fn init_settings(&mut self, c: u32, capacity: u32, center: [f32; 3], size: [f32; 3]) {
        if let Some(ctx) = self.g.context_mut(c)
            && let ContextKind::Initialize(s) = &mut ctx.kind
        {
            s.capacity = capacity;
            s.bounds_center = center;
            s.bounds_size = size;
        }
    }

    fn init(&mut self, capacity: u32, center: [f32; 3], size: [f32; 3]) -> u32 {
        let c = self.context(ContextType::Initialize, 0.0, 150.0);
        self.init_settings(c, capacity, center, size);
        c
    }

    fn world_space(&mut self, c: u32) {
        if let Some(ctx) = self.g.context_mut(c)
            && let ContextKind::Initialize(s) = &mut ctx.kind
        {
            s.space = FxSpace::World;
        }
    }

    fn output(&mut self, system: u32, settings: OutputSettings) -> u32 {
        let c = self.context(ContextType::Output, 0.0, 700.0);
        if let Some(ctx) = self.g.context_mut(c) {
            ctx.kind = ContextKind::Output(settings);
            ctx.system = system;
        }
        c
    }

    fn block(&mut self, ctx: u32, kind: BlockKind, values: &[(&str, Value)]) -> u32 {
        let b = self.g.add_block(ctx, kind).expect("block fits its context");
        for (n, v) in values {
            self.g
                .block_mut(b)
                .unwrap()
                .values
                .insert((*n).into(), v.clone());
        }
        b
    }

    fn set(&mut self, ctx: u32, a: Attribute, random: bool, values: &[(&str, Value)]) -> u32 {
        self.block(
            ctx,
            BlockKind::SetAttribute {
                attribute: a,
                random,
                composition: Composition::Overwrite,
            },
            values,
        )
    }

    fn op(&mut self, kind: OperatorKind, values: &[(&str, Value)]) -> u32 {
        let o = self.g.add_operator(kind, [-300.0, 0.0]);
        for (n, v) in values {
            self.g
                .operator_mut(o)
                .unwrap()
                .values
                .insert((*n).into(), v.clone());
        }
        o
    }

    fn link(&mut self, from: u32, out: &str, to: u32, input: &str) {
        self.g
            .connect(from, out, to, input)
            .unwrap_or_else(|e| panic!("{out} → {input}: {e}"));
    }

    /// A GPU Event system's contexts.
    fn event_system(&mut self, source: EventSource, count: u32) -> (u32, u32, u32) {
        let sys = self.g.add_event_system(0, source, [600.0, 0.0]);
        let ev = self
            .g
            .system_context(sys, ContextType::GpuEvent)
            .unwrap()
            .id;
        if let Some(c) = self.g.context_mut(ev)
            && let ContextKind::GpuEvent(s) = &mut c.kind
        {
            s.count = count;
        }
        // add_event_system adds a quad output; outputs are set by the caller.
        let out = self.g.system_context(sys, ContextType::Output).unwrap().id;
        self.g.remove(out);
        let init = self
            .g
            .system_context(sys, ContextType::Initialize)
            .unwrap()
            .id;
        let update = self.g.system_context(sys, ContextType::Update).unwrap().id;
        (sys, init, update)
    }

    fn finish(mut self) -> EffectGraph {
        crate::panels::effect_graph::layout::arrange(&mut self.g);
        self.g
    }
}

fn f(v: f32) -> Value {
    Value::Float(v)
}

fn v3(x: f32, y: f32, z: f32) -> Value {
    Value::Vec3([x, y, z])
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Value {
    Value::Color([r, g, b, a])
}

fn fade(colors: &[(f32, [f32; 3])]) -> Value {
    Value::Gradient(gradient(colors, &[(0.0, 1.0), (0.7, 0.9), (1.0, 0.0)]))
}

/// Rockets with trails, bursting into trailing sparks where they die.
pub fn fireworks() -> EffectGraph {
    let mut b = B::new();
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    b.block(
        spawn,
        BlockKind::PeriodicBurst,
        &[("Count", f(3.0)), ("Period", f(1.4)), ("Delay", f(0.2))],
    );
    let init = b.init(64, [0.0, 7.0, 0.0], [16.0, 16.0, 16.0]);
    b.world_space(init);
    b.block(
        init,
        BlockKind::PositionBox {
            mode: ShapeMode::Volume,
        },
        &[("Size", v3(3.0, 0.0, 1.0))],
    );
    b.set(
        init,
        Attribute::Velocity,
        true,
        &[("A", v3(-1.2, 9.0, -0.8)), ("B", v3(1.2, 11.0, 0.8))],
    );
    b.set(
        init,
        Attribute::Lifetime,
        true,
        &[("A", f(1.0)), ("B", f(1.25))],
    );
    b.set(init, Attribute::Size, false, &[("Size", f(0.12))]);
    b.set(
        init,
        Attribute::Color,
        false,
        &[("Color", rgba(4.0, 2.6, 1.2, 1.0))],
    );
    let update = b.context(ContextType::Update, 0.0, 450.0);
    b.block(update, BlockKind::Gravity, &[]);
    b.output(
        0,
        OutputSettings {
            kind: OutputKind::Trail,
            blend: BlendMode::Additive,
            trail_points: 16,
            ..Default::default()
        },
    );
    // Sparks: 90 per rocket, spraying out and falling with trails.
    let (sys, cinit, cupdate) = b.event_system(EventSource::OnDie, 90);
    b.init_settings(cinit, 8192, [0.0, 7.0, 0.0], [16.0, 16.0, 16.0]);
    b.block(
        cinit,
        BlockKind::PositionSphere {
            mode: ShapeMode::Surface,
        },
        &[("Radius", f(0.05))],
    );
    b.block(
        cinit,
        BlockKind::VelocityFromDirection,
        &[("Min Speed", f(3.5)), ("Max Speed", f(6.0))],
    );
    b.block(
        cinit,
        BlockKind::InheritSourceVelocity,
        &[("Multiplier", f(0.3))],
    );
    b.set(
        cinit,
        Attribute::Lifetime,
        true,
        &[("A", f(1.2)), ("B", f(2.0))],
    );
    b.set(
        cinit,
        Attribute::Size,
        true,
        &[("A", f(0.04)), ("B", f(0.08))],
    );
    b.set(
        cinit,
        Attribute::Color,
        true,
        &[
            ("A", rgba(5.0, 1.2, 2.5, 1.0)),
            ("B", rgba(1.2, 2.5, 6.0, 1.0)),
        ],
    );
    b.block(
        cupdate,
        BlockKind::Gravity,
        &[("Force", v3(0.0, -3.0, 0.0))],
    );
    b.block(cupdate, BlockKind::LinearDrag, &[("Drag", f(1.2))]);
    let out = b.output(
        sys,
        OutputSettings {
            kind: OutputKind::Trail,
            blend: BlendMode::Additive,
            trail_points: 10,
            ..Default::default()
        },
    );
    b.block(
        out,
        BlockKind::ColorOverLife,
        &[("Gradient", fade(&[(0.0, [1.0; 3]), (1.0, [1.0, 0.6, 0.4])]))],
    );
    b.finish()
}

/// Rain lines hitting what the camera sees, splashing through an On Collide event system.
pub fn rain_splash() -> EffectGraph {
    let mut b = B::new();
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    b.block(spawn, BlockKind::ConstantRate, &[("Rate", f(500.0))]);
    let init = b.init(4096, [0.0, 3.0, 0.0], [8.0, 8.0, 8.0]);
    b.world_space(init);
    b.block(
        init,
        BlockKind::PositionBox {
            mode: ShapeMode::Volume,
        },
        &[("Center", v3(0.0, 6.0, 0.0)), ("Size", v3(5.0, 0.5, 5.0))],
    );
    b.set(
        init,
        Attribute::Velocity,
        true,
        &[("A", v3(-0.2, -10.0, -0.2)), ("B", v3(0.2, -8.0, 0.2))],
    );
    b.set(init, Attribute::Lifetime, false, &[("Lifetime", f(2.0))]);
    b.set(init, Attribute::Size, false, &[("Size", f(0.02))]);
    b.set(
        init,
        Attribute::Color,
        false,
        &[("Color", rgba(0.7, 0.8, 1.6, 0.7))],
    );
    let update = b.context(ContextType::Update, 0.0, 450.0);
    b.block(
        update,
        BlockKind::CollideDepthBuffer,
        &[("Bounce", f(0.0)), ("Lifetime Loss", f(1.0))],
    );
    b.output(
        0,
        OutputSettings {
            kind: OutputKind::Line,
            blend: BlendMode::Additive,
            line_length: 0.03,
            ..Default::default()
        },
    );
    let (sys, cinit, cupdate) = b.event_system(EventSource::OnCollide, 4);
    b.init_settings(cinit, 8192, [0.0, 3.0, 0.0], [8.0, 8.0, 8.0]);
    b.block(
        cinit,
        BlockKind::PositionSphere {
            mode: ShapeMode::Surface,
        },
        &[("Radius", f(0.03))],
    );
    b.block(
        cinit,
        BlockKind::VelocityFromDirection,
        &[("Min Speed", f(0.6)), ("Max Speed", f(1.6))],
    );
    b.set(
        cinit,
        Attribute::Lifetime,
        true,
        &[("A", f(0.25)), ("B", f(0.45))],
    );
    b.set(
        cinit,
        Attribute::Size,
        true,
        &[("A", f(0.03)), ("B", f(0.06))],
    );
    b.set(
        cinit,
        Attribute::Color,
        false,
        &[("Color", rgba(0.8, 0.9, 1.0, 0.8))],
    );
    b.block(cupdate, BlockKind::Gravity, &[]);
    let out = b.output(
        sys,
        OutputSettings {
            blend: BlendMode::Alpha,
            ..Default::default()
        },
    );
    b.block(
        out,
        BlockKind::ColorOverLife,
        &[("Gradient", fade(&[(0.0, [1.0; 3]), (1.0, [1.0; 3])]))],
    );
    b.finish()
}

/// Lit cubes thrown up, tumbling and bouncing on the floor.
pub fn debris() -> EffectGraph {
    let mut b = B::new();
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    b.block(
        spawn,
        BlockKind::PeriodicBurst,
        &[("Count", f(40.0)), ("Period", f(2.5)), ("Delay", f(0.0))],
    );
    let init = b.init(256, [0.0, 2.0, 0.0], [10.0, 6.0, 10.0]);
    b.block(
        init,
        BlockKind::PositionCone {
            mode: ShapeMode::Volume,
        },
        &[("Radius", f(0.2)), ("Angle", f(30.0)), ("Height", f(0.1))],
    );
    b.block(
        init,
        BlockKind::VelocityFromDirection,
        &[("Min Speed", f(3.0)), ("Max Speed", f(6.0))],
    );
    b.set(init, Attribute::Lifetime, false, &[("Lifetime", f(4.5))]);
    b.set(
        init,
        Attribute::Size,
        true,
        &[("A", f(0.1)), ("B", f(0.22))],
    );
    b.set(
        init,
        Attribute::Angle,
        true,
        &[("A", f(0.0)), ("B", f(360.0))],
    );
    b.set(
        init,
        Attribute::Color,
        true,
        &[
            ("A", rgba(0.9, 0.55, 0.3, 1.0)),
            ("B", rgba(0.5, 0.6, 0.7, 1.0)),
        ],
    );
    let update = b.context(ContextType::Update, 0.0, 450.0);
    b.block(update, BlockKind::Gravity, &[]);
    b.block(
        update,
        BlockKind::CollidePlane,
        &[("Bounce", f(0.45)), ("Friction", f(0.35))],
    );
    // Spin: Angle += 240° × dt.
    let spin = b.block(
        update,
        BlockKind::SetAttribute {
            attribute: Attribute::Angle,
            random: false,
            composition: Composition::Add,
        },
        &[],
    );
    let time = b.op(OperatorKind::Time, &[]);
    let rate = b.op(OperatorKind::Multiply, &[("B", f(240.0))]);
    b.link(time, "Delta", rate, "A");
    b.link(rate, "Out", spin, "Angle");
    b.output(
        0,
        OutputSettings {
            kind: OutputKind::Mesh,
            blend: BlendMode::Opaque,
            ..Default::default()
        },
    );
    b.finish()
}

/// The Orbit sub-graph: a point circling at `Radius` with `Speed`, bobbing up and down.
pub fn orbit() -> EffectGraph {
    let mut b = B::new();
    b.g.add_property("Radius", PropertyValue::Float(1.6));
    b.g.add_property("Speed", PropertyValue::Float(1.8));
    let radius = b.op(
        OperatorKind::Property {
            name: "Radius".into(),
        },
        &[],
    );
    let speed = b.op(
        OperatorKind::Property {
            name: "Speed".into(),
        },
        &[],
    );
    let time = b.op(OperatorKind::Time, &[]);
    let angle = b.op(OperatorKind::Multiply, &[]);
    b.link(time, "Total", angle, "A");
    b.link(speed, "Out", angle, "B");
    let cos = b.op(OperatorKind::Cosine, &[]);
    let sin = b.op(OperatorKind::Sine, &[]);
    b.link(angle, "Out", cos, "Input");
    b.link(angle, "Out", sin, "Input");
    let x = b.op(OperatorKind::Multiply, &[]);
    let z = b.op(OperatorKind::Multiply, &[]);
    b.link(cos, "Out", x, "A");
    b.link(radius, "Out", x, "B");
    b.link(sin, "Out", z, "A");
    b.link(radius, "Out", z, "B");
    let bob_t = b.op(OperatorKind::Multiply, &[("B", f(2.3))]);
    b.link(time, "Total", bob_t, "A");
    let bob = b.op(OperatorKind::Sine, &[]);
    b.link(bob_t, "Out", bob, "Input");
    let y = b.op(
        OperatorKind::Remap,
        &[
            ("In Min", f(-1.0)),
            ("In Max", f(1.0)),
            ("Out Min", f(1.2)),
            ("Out Max", f(2.0)),
        ],
    );
    b.link(bob, "Out", y, "Input");
    let combine = b.op(OperatorKind::Combine, &[]);
    b.link(x, "Out", combine, "X");
    b.link(y, "Out", combine, "Y");
    b.link(z, "Out", combine, "Z");
    let out = b.op(
        OperatorKind::SubgraphOutput {
            name: "Position".into(),
        },
        &[],
    );
    b.link(combine, "Out", out, "Value");
    b.g
}

/// One ribbon through particles born along the Orbit sub-graph's path.
pub fn ribbon() -> EffectGraph {
    let mut b = B::new();
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    b.block(spawn, BlockKind::ConstantRate, &[("Rate", f(70.0))]);
    let init = b.init(512, [0.0, 1.6, 0.0], [5.0, 4.0, 5.0]);
    b.world_space(init);
    let pos = b.set(init, Attribute::Position, false, &[]);
    // The node's ports come from the sub-graph (registered: no project root needed).
    misty_engine::vfx::subgraph::register(ORBIT, orbit());
    let orbit = b.op(
        OperatorKind::Subgraph { path: ORBIT.into() },
        &[("Speed", f(2.2))],
    );
    b.link(orbit, "Position", pos, "Position");
    b.set(init, Attribute::Lifetime, false, &[("Lifetime", f(1.6))]);
    b.set(init, Attribute::Size, false, &[("Size", f(0.18))]);
    b.set(
        init,
        Attribute::Color,
        false,
        &[("Color", rgba(0.6, 2.2, 3.0, 1.0))],
    );
    b.context(ContextType::Update, 0.0, 450.0);
    let out = b.output(
        0,
        OutputSettings {
            kind: OutputKind::Ribbon,
            blend: BlendMode::Additive,
            ..Default::default()
        },
    );
    b.block(
        out,
        BlockKind::ColorOverLife,
        &[(
            "Gradient",
            Value::Gradient(gradient(
                &[(0.0, [1.0; 3]), (1.0, [1.0, 0.3, 1.0])],
                &[(0.0, 1.0), (1.0, 0.0)],
            )),
        )],
    );
    b.block(
        out,
        BlockKind::SizeOverLife,
        &[("Curve", Value::Curve(Curve::linear(1.0, 0.1)))],
    );
    b.finish()
}

/// Soft smoke drawn back to front from a generated flipbook.
pub fn smoke() -> EffectGraph {
    let mut b = B::new();
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    b.block(spawn, BlockKind::ConstantRate, &[("Rate", f(14.0))]);
    let init = b.init(256, [0.0, 2.5, 0.0], [6.0, 6.0, 6.0]);
    b.block(
        init,
        BlockKind::PositionSphere {
            mode: ShapeMode::Volume,
        },
        &[("Radius", f(0.3))],
    );
    b.set(
        init,
        Attribute::Velocity,
        true,
        &[("A", v3(-0.15, 0.5, -0.15)), ("B", v3(0.15, 0.9, 0.15))],
    );
    b.set(
        init,
        Attribute::Lifetime,
        true,
        &[("A", f(4.0)), ("B", f(5.5))],
    );
    b.set(init, Attribute::Size, true, &[("A", f(0.7)), ("B", f(1.0))]);
    b.set(
        init,
        Attribute::Angle,
        true,
        &[("A", f(0.0)), ("B", f(360.0))],
    );
    b.set(
        init,
        Attribute::Color,
        false,
        &[("Color", rgba(0.55, 0.55, 0.6, 0.7))],
    );
    let update = b.context(ContextType::Update, 0.0, 450.0);
    b.block(
        update,
        BlockKind::Turbulence,
        &[("Intensity", f(0.3)), ("Frequency", f(0.5))],
    );
    b.block(update, BlockKind::LinearDrag, &[("Drag", f(0.3))]);
    let out = b.output(
        0,
        OutputSettings {
            blend: BlendMode::Alpha,
            texture: Some(SMOKE_SHEET.into()),
            flipbook: [4, 4],
            soft_particles: true,
            ..Default::default()
        },
    );
    b.block(
        out,
        BlockKind::ColorOverLife,
        &[(
            "Gradient",
            Value::Gradient(gradient(
                &[(0.0, [1.0; 3]), (1.0, [0.8; 3])],
                &[(0.0, 0.0), (0.15, 1.0), (1.0, 0.0)],
            )),
        )],
    );
    b.block(
        out,
        BlockKind::SizeOverLife,
        &[("Curve", Value::Curve(Curve::linear(0.6, 2.2)))],
    );
    b.finish()
}

pub fn effects() -> [(&'static str, EffectGraph); 5] {
    [
        (FIREWORKS, fireworks()),
        (RAIN, rain_splash()),
        (DEBRIS, debris()),
        (RIBBON, ribbon()),
        (SMOKE, smoke()),
    ]
}

/// Deterministic value noise (0..1) for the smoke sheet.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed;
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

fn value_noise(u: f32, v: f32, cells: f32, seed: u32) -> f32 {
    let (x, y) = (u * cells, v * cells);
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    lerp(
        lerp(hash(ix, iy, seed), hash(ix + 1, iy, seed), sx),
        lerp(hash(ix, iy + 1, seed), hash(ix + 1, iy + 1, seed), sx),
        sy,
    )
}

/// A 4×4 flipbook of a smoke puff swelling and thinning out (512×512, white with alpha).
pub fn smoke_sheet() -> RgbaImage {
    const CELL: u32 = 128;
    RgbaImage::from_fn(CELL * 4, CELL * 4, |x, y| {
        let frame = (y / CELL) * 4 + x / CELL;
        let t = frame as f32 / 15.0;
        let u = (x % CELL) as f32 / CELL as f32;
        let v = (y % CELL) as f32 / CELL as f32;
        let (dx, dy) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
        let r = (dx * dx + dy * dy).sqrt();
        let radius = 0.55 + 0.35 * t;
        let mut n = 0.0;
        let mut amp = 0.5;
        for (k, cells) in [4.0f32, 8.0, 16.0].iter().enumerate() {
            n += value_noise(u + t * 0.3, v - t * 0.2, *cells, 7 + k as u32) * amp;
            amp *= 0.5;
        }
        let edge = ((radius - r) / radius).clamp(0.0, 1.0);
        let density = (edge * edge * (0.5 + n)).clamp(0.0, 1.0) * (1.0 - 0.6 * t);
        let shade = (200.0 + 55.0 * n) as u8;
        Rgba([shade, shade, shade, (density * 255.0) as u8])
    })
}

fn embers() -> ParticleSystem {
    let mut p = ParticleSystem::default();
    p.main.start_lifetime = MinMax::between(1.8, 2.6);
    p.main.start_speed = MinMax::between(0.6, 1.4);
    p.main.start_size = MinMax::between(0.06, 0.12);
    p.main.start_color = MinMaxColor::between([1.0, 0.55, 0.15, 1.0], [1.0, 0.8, 0.3, 1.0]);
    p.main.simulation_space = SimulationSpace::World;
    p.main.max_particles = 400;
    p.shape.shape = ShapeKind::Cone;
    p.shape.angle = 15.0;
    p.shape.radius = 0.4;
    p.emission.rate_over_time = MinMax::constant(30.0);
    p.noise.enabled = true;
    p.noise.strength = MinMax::curve(1.2, Curve::linear(0.3, 1.0));
    p.noise.frequency = 0.9;
    p.noise.scroll_speed = 0.6;
    p.noise.octaves = 2;
    p.color_over_lifetime.enabled = true;
    p.color_over_lifetime.gradient = gradient(
        &[(0.0, [1.0; 3]), (1.0, [1.0, 0.3, 0.1])],
        &[(0.0, 1.0), (0.8, 1.0), (1.0, 0.0)],
    );
    p.size_over_lifetime.enabled = true;
    p.size_over_lifetime.size = Some(MinMax::curve(
        1.0,
        Curve::from_keys(&[[0.0, 0.4], [0.2, 1.0], [1.0, 0.5]]),
    ));
    p.trails.enabled = true;
    p.trails.lifetime = MinMax::constant(0.25);
    p.trails.min_vertex_distance = 0.05;
    p.sub_emitters.enabled = true;
    p.sub_emitters.list = vec![SubEmitter {
        trigger: SubEmitterTrigger::Death,
        emitter: "Ember Pops".into(),
        count: 6,
        probability: 0.5,
        inherit_color: true,
        ..Default::default()
    }];
    p.renderer.blend = ParticleBlend::Additive;
    p.renderer.intensity = 3.0;
    p.seed = 21;
    p
}

fn ember_pops() -> ParticleSystem {
    let mut p = ParticleSystem::default();
    p.main.play_on_awake = false;
    p.main.start_lifetime = MinMax::between(0.3, 0.6);
    p.main.start_speed = MinMax::between(0.8, 1.8);
    p.main.start_size = MinMax::constant(0.03);
    p.main.gravity_modifier = 0.5;
    p.main.simulation_space = SimulationSpace::World;
    p.shape.shape = ShapeKind::Sphere;
    p.shape.radius = 0.02;
    p.emission.rate_over_time = MinMax::constant(0.0);
    p.renderer.blend = ParticleBlend::Additive;
    p.renderer.intensity = 4.0;
    p.seed = 22;
    p
}

fn sparks_shower() -> ParticleSystem {
    let mut p = ParticleSystem::default();
    p.main.start_lifetime = MinMax::constant(3.0);
    p.main.start_speed = MinMax::between(2.0, 4.0);
    p.main.start_size = MinMax::constant(0.05);
    p.main.start_color = MinMaxColor::constant([1.0, 0.75, 0.35, 1.0]);
    p.main.gravity_modifier = 1.0;
    p.main.simulation_space = SimulationSpace::World;
    p.main.max_particles = 600;
    p.shape.shape = ShapeKind::Cone;
    p.shape.angle = 35.0;
    p.shape.radius = 0.05;
    p.emission.rate_over_time = MinMax::constant(0.0);
    p.emission.bursts = vec![Burst {
        time: 0.0,
        count: 40,
        cycles: 0,
        interval: 0.4,
        probability: 1.0,
    }];
    p.limit_velocity_over_lifetime.enabled = true;
    p.limit_velocity_over_lifetime.limit = MinMax::constant(7.0);
    p.limit_velocity_over_lifetime.dampen = 0.3;
    p.collision.enabled = true;
    p.collision.kind = CollisionType::World;
    p.collision.bounce = 0.4;
    p.collision.dampen = 0.2;
    p.collision.lifetime_loss = 0.2;
    p.sub_emitters.enabled = true;
    p.sub_emitters.list = vec![SubEmitter {
        trigger: SubEmitterTrigger::Collision,
        emitter: "Spark Splash".into(),
        count: 3,
        inherit_velocity: 0.2,
        ..Default::default()
    }];
    p.color_over_lifetime.enabled = true;
    p.color_over_lifetime.gradient = gradient(
        &[
            (0.0, [1.0; 3]),
            (0.5, [1.0, 0.6, 0.2]),
            (1.0, [0.8, 0.2, 0.1]),
        ],
        &[(0.0, 1.0), (1.0, 0.0)],
    );
    p.renderer.render_mode = RenderMode::Stretched;
    p.renderer.speed_scale = 0.06;
    p.renderer.blend = ParticleBlend::Additive;
    p.renderer.intensity = 4.0;
    p.seed = 23;
    p
}

fn spark_splash() -> ParticleSystem {
    let mut p = ember_pops();
    p.main.start_color = MinMaxColor::constant([1.0, 0.85, 0.5, 1.0]);
    p.main.start_lifetime = MinMax::between(0.15, 0.3);
    p.main.max_particles = 800;
    p.seed = 24;
    p
}

fn flipbook_smoke() -> ParticleSystem {
    let mut p = ParticleSystem::default();
    p.main.start_lifetime = MinMax::between(3.0, 4.0);
    p.main.start_speed = MinMax::between(0.2, 0.5);
    p.main.start_size = MinMax::between(0.8, 1.2);
    p.main.start_rotation = MinMax::between(0.0, 360.0);
    p.main.start_color = MinMaxColor::constant([0.6, 0.62, 0.68, 0.8]);
    p.main.max_particles = 120;
    p.shape.shape = ShapeKind::Cone;
    p.shape.angle = 10.0;
    p.shape.radius = 0.3;
    p.emission.rate_over_time = MinMax::curve(
        12.0,
        Curve::from_keys(&[[0.0, 0.5], [0.5, 1.0], [1.0, 0.5]]),
    );
    p.velocity_over_lifetime.enabled = true;
    p.velocity_over_lifetime.y = MinMax::curve(0.6, Curve::linear(1.0, 0.2));
    p.velocity_over_lifetime.space = SimulationSpace::World;
    p.rotation_over_lifetime.enabled = true;
    p.rotation_over_lifetime.angular_velocity = MinMax::between(-30.0, 30.0);
    p.size_over_lifetime.enabled = true;
    p.size_over_lifetime.size = Some(MinMax::between_curves(
        1.0,
        Curve::linear(0.5, 1.6),
        Curve::linear(0.7, 2.4),
    ));
    p.color_over_lifetime.enabled = true;
    p.color_over_lifetime.gradient = gradient(
        &[(0.0, [1.0; 3]), (1.0, [0.7; 3])],
        &[(0.0, 0.0), (0.2, 1.0), (1.0, 0.0)],
    );
    p.texture_sheet_animation.enabled = true;
    p.texture_sheet_animation.tiles_x = 4;
    p.texture_sheet_animation.tiles_y = 4;
    p.renderer.texture = Some(SMOKE_SHEET.into());
    p.seed = 25;
    p
}

fn ground() -> Material {
    Material {
        color: [0.09, 0.1, 0.12, 1.0],
        smoothness: 0.4,
        ..Default::default()
    }
}

fn crate_material() -> Material {
    Material {
        color: [0.35, 0.28, 0.2, 1.0],
        smoothness: 0.2,
        ..Default::default()
    }
}

fn effect(s: &mut Scene, name: &str, path: &str, at: Vec3, seed: u32) {
    let e = s.spawn(name, None);
    s.set_transform(e, Transform::from_position(at));
    s.add(
        e,
        VisualEffect {
            effect: Some(path.into()),
            play_on_awake: true,
            seed,
            reseed_on_play: false,
            overrides: Vec::new(),
        },
    );
}

fn system(
    s: &mut Scene,
    name: &str,
    p: ParticleSystem,
    t: Transform,
    parent: Option<misty_engine::hecs::Entity>,
) -> misty_engine::hecs::Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, p);
    e
}

/// Points +Z (a system's emission axis) straight up.
fn upward(at: Vec3) -> Transform {
    Transform::from_position(at).with_euler(Vec3::new(-90.0, 0.0, 0.0))
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("VfxDemo2");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 4.5, -15.0))
                .with_euler(Vec3::new(12.0, 0.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.clear_flags = ClearFlags::SolidColor;
        c.background = [0.015, 0.02, 0.045, 0.0];
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 0.35;
        l.color = [0.6, 0.7, 1.0];
    }
    let floor = s.spawn("Ground", None);
    s.set_transform(
        floor,
        Transform {
            scale: Vec3::new(3.0, 1.0, 2.0),
            ..Default::default()
        },
    );
    s.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(
        floor,
        MeshRenderer {
            material: Some(GROUND_MAT.into()),
            ..Default::default()
        },
    );
    s.add(floor, MeshCollider::default());
    // A crate under the rain (depth collision) and the sparks (world collision).
    for (name, at) in [
        ("Rain Crate", Vec3::new(-3.0, 0.6, 2.0)),
        ("Spark Crate", Vec3::new(0.6, 0.6, -3.0)),
    ] {
        let c = s.spawn(name, None);
        s.set_transform(
            c,
            Transform {
                position: at,
                scale: Vec3::splat(1.2),
                ..Default::default()
            },
        );
        s.add(c, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(
            c,
            MeshRenderer {
                material: Some(CRATE_MAT.into()),
                ..Default::default()
            },
        );
        s.add(c, BoxCollider::default());
    }
    effect(
        &mut s,
        "Fireworks",
        FIREWORKS,
        Vec3::new(-8.0, 0.0, 6.0),
        31,
    );
    effect(&mut s, "Rain Splash", RAIN, Vec3::new(-3.0, 0.0, 2.0), 32);
    effect(&mut s, "Debris", DEBRIS, Vec3::new(1.5, 0.0, 1.0), 33);
    effect(&mut s, "Ribbon", RIBBON, Vec3::new(4.5, 0.0, 2.5), 34);
    effect(&mut s, "Smoke", SMOKE, Vec3::new(8.0, 0.0, 4.0), 35);
    let embers = system(
        &mut s,
        "Embers",
        embers(),
        upward(Vec3::new(-6.0, 0.1, -2.0)),
        None,
    );
    system(
        &mut s,
        "Ember Pops",
        ember_pops(),
        Transform::default(),
        Some(embers),
    );
    let shower = system(
        &mut s,
        "Sparks Shower",
        sparks_shower(),
        Transform::from_position(Vec3::new(0.0, 3.5, -3.0)).with_euler(Vec3::new(90.0, 0.0, 0.0)),
        None,
    );
    system(
        &mut s,
        "Spark Splash",
        spark_splash(),
        Transform::default(),
        Some(shower),
    );
    system(
        &mut s,
        "Flipbook Smoke",
        flipbook_smoke(),
        upward(Vec3::new(5.0, 0.2, -2.5)),
        None,
    );
    s
}

/// A `.meta` with a fresh GUID unless the asset (or folder) has one.
fn ensure_meta(path: &std::path::Path) {
    if misty_engine::asset_db::meta::read_guid(path).is_none() {
        misty_engine::asset_db::meta::write_guid(path, misty_engine::asset_db::Guid::new())
            .unwrap();
    }
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_vfx_demo2() {
    let root = project();
    orbit().save(&root.join(ORBIT)).unwrap();
    ensure_meta(&root.join(ORBIT));
    for (path, g) in effects() {
        g.save(&root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    smoke_sheet().save(root.join(SMOKE_SHEET)).unwrap();
    ensure_meta(&root.join(SMOKE_SHEET));
    for (path, m) in [(GROUND_MAT, ground()), (CRATE_MAT, crate_material())] {
        m.save(&root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn vfx_demo2_matches_its_generator() {
    let root = project();
    let text = std::fs::read_to_string(root.join(ORBIT)).unwrap();
    crate::test_util::assert_close(&EffectGraph::from_json(&text).unwrap(), &orbit(), ORBIT);
    for (path, g) in effects() {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        crate::test_util::assert_close(&EffectGraph::from_json(&text).unwrap(), &g, path);
        assert!(
            root.join(format!("{path}.meta")).exists(),
            "{path} has a GUID"
        );
    }
    let sheet = misty_engine::image::open(root.join(SMOKE_SHEET))
        .unwrap()
        .to_rgba8();
    assert!(sheet == smoke_sheet(), "regenerate with write_vfx_demo2");
    for (path, m) in [(GROUND_MAT, ground()), (CRATE_MAT, crate_material())] {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_vfx_demo2",
    );
    let (_, report) = Scene::from_json("VfxDemo2", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn demo2_effects_compile_with_their_features() {
    use misty_engine::vfx::codegen::SortKey;
    misty_engine::vfx::subgraph::register(ORBIT, orbit());
    for (path, g) in effects() {
        let c = misty_engine::vfx::compile(&g).unwrap_or_else(|e| panic!("{path}: {e:?}"));
        match path {
            FIREWORKS => {
                assert_eq!(c.systems.len(), 2, "rockets and sparks");
                assert_eq!(
                    c.systems[1].event.as_ref().unwrap().source,
                    EventSource::OnDie
                );
                assert_eq!(c.root().trail_points, 16);
                assert_eq!(c.systems[1].trail_points, 10);
            }
            RAIN => {
                assert!(c.depth_collision);
                assert_eq!(
                    c.systems[1].event.as_ref().unwrap().source,
                    EventSource::OnCollide
                );
                assert_eq!(c.systems[1].outputs[0].sort, Some(SortKey::Distance));
            }
            DEBRIS => assert_eq!(c.root().outputs[0].settings.kind, OutputKind::Mesh),
            RIBBON => {
                assert_eq!(c.root().outputs[0].sort, Some(SortKey::Age));
                assert!(c.root().sim_source.contains("cos("), "the Orbit sub-graph");
            }
            _ => assert_eq!(c.root().outputs[0].sort, Some(SortKey::Distance)),
        }
    }
}

/// Playing the scene: sub emitters fire, sparks collide with the floor and crate.
#[test]
fn demo2_particle_systems_play() {
    let (mut scene, _) = Scene::from_json("VfxDemo2", &demo_scene().to_json()).unwrap();
    for _ in 0..240 {
        misty_engine::particles::update(&mut scene, 1.0 / 60.0, Vec3::new(0.0, -9.81, 0.0));
    }
    let count = |name: &str| {
        let e = scene.find_by_name(name).unwrap();
        scene
            .world
            .get::<&misty_engine::particles::ParticleEmitter>(e)
            .map_or(0, |em| em.count())
    };
    assert!(count("Embers") > 20);
    assert!(count("Ember Pops") > 0, "death sub emitter");
    assert!(count("Spark Splash") > 0, "collision sub emitter");
    assert!(count("Flipbook Smoke") > 10);
    let shower = scene.find_by_name("Sparks Shower").unwrap();
    let em = scene
        .world
        .get::<&misty_engine::particles::ParticleEmitter>(shower)
        .unwrap();
    assert!(
        em.particles.iter().all(|p| p.position.y > -0.05),
        "sparks stay above the floor"
    );
}

/// On a GPU: the demo renders bright effects over the dark floor.
#[test]
fn vfx_demo2_renders() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    misty_engine::assets::set_project_root(&root);
    misty_engine::vfx::subgraph::register(ORBIT, orbit());
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let list: Vec<_> = scene
        .world
        .query::<(misty_engine::hecs::Entity, &VisualEffect)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in list {
        let mut v = scene.world.get::<&mut VisualEffect>(e).unwrap();
        let rel = v.effect.clone().unwrap();
        v.effect = Some(root.join(rel).to_string_lossy().replace('\\', "/"));
    }
    let assets = misty_engine::assets::AssetCache::open(&root);
    let (device, queue) = &gpu;
    let mut renderer = misty_engine::render::Renderer::new(device);
    let target = renderer.create_target(
        device,
        crate::sample_render::WIDTH,
        crate::sample_render::HEIGHT,
    );
    let mut view = None;
    for _ in 0..90 {
        misty_engine::vfx::update(&mut scene, 1.0 / 30.0);
        misty_engine::particles::update(&mut scene, 1.0 / 30.0, Vec3::new(0.0, -9.81, 0.0));
        let v = misty_engine::render_scene::GameView::new(&scene, &assets).unwrap();
        renderer.render_with_effects(device, queue, &target, &v.frame(), &v.effects);
        view = Some(v);
    }
    let view = view.unwrap();
    assert_eq!(view.effects.len(), 5, "every effect compiled");
    let pixels = crate::project_thumbnail::read_pixels(device, queue, &target);
    if let Some(out) = std::env::var_os("MISTY_VFX_DUMP") {
        misty_engine::image::save_buffer(
            out,
            &pixels,
            crate::sample_render::WIDTH,
            crate::sample_render::HEIGHT,
            misty_engine::image::ColorType::Rgba8,
        )
        .unwrap();
    }
    let lum: Vec<f32> = pixels
        .chunks(4)
        .map(|p| 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32)
        .collect();
    let bright = lum.iter().filter(|l| **l > 120.0).count();
    eprintln!("{bright} bright pixels");
    assert!(bright > 300, "effects light the night: {bright}");
}
