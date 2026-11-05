//! VfxDemo sample in `SampleProject`: four Effect Graphs built in code and a night scene
//! playing them. **Fireflies** (box spawn, curl-noise drift, a flicker wired from Time, Sine
//! and a per-particle random), **Sparks Fountain** (constant rate plus periodic bursts, gravity,
//! bouncing on the ground plane, stretched along velocity), **Magic Portal** (a ring spawned on
//! a circle, vortex + turbulence, additive, exposed Color / Rate / Fade) and **Million
//! Particles** (a million turbulent points conformed to a sphere). A script pulses the portal
//! through the VisualEffect Lua API. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_vfx_demo`; the other tests check the committed
//! files still match, compile, and (on a GPU) that the demo renders.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::{
    ClearFlags, Light, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, Scripts,
    Transform,
};
use misty_engine::vfx::graph::{
    BlendMode, ContextKind, EffectGraph, OutputKind, OutputSettings, SimulationSpace,
};
use misty_engine::vfx::types::{AlphaKey, ColorKey, Curve, Gradient, Value};
use misty_engine::vfx::{
    Attribute, AttributeRead, BlockKind, Composition, ContextType, OperatorKind, OrientMode,
    PropertyValue, RandomMode, ShapeMode, VisualEffect,
};

pub const SCENE: &str = "Assets/Scenes/VfxDemo.scene";
pub const FIREFLIES: &str = "Assets/Effects/Fireflies.effect";
pub const SPARKS: &str = "Assets/Effects/Sparks Fountain.effect";
pub const PORTAL: &str = "Assets/Effects/Magic Portal.effect";
pub const MILLION: &str = "Assets/Effects/Million Particles.effect";
const GROUND_MAT: &str = "Assets/Materials/Vfx Demo Ground.mat";
const SCRIPT: &str = "Assets/Scripts/PortalPulse.lua";

/// Pulses the portal's spawn rate and colour, and toggles it with Space.
const PORTAL_PULSE: &str = r#"-- Pulses the Magic Portal through the VisualEffect API; Space stops / restarts it.
local PortalPulse = { speed = 1.5 }

function PortalPulse:Start()
    self.fx = self.gameObject:GetComponent("VisualEffect")
    self.t = 0
end

function PortalPulse:Update()
    self.t = self.t + Time.deltaTime * self.speed
    local k = 0.5 + 0.5 * math.sin(self.t)
    self.fx:SetFloat("Rate", 400 + 600 * k)
    self.fx:SetColor("Color", Color.new(1.2 + k, 0.4, 3.5 - k, 1))
    if Input.GetKeyDown("space") then
        if self.fx.isPlaying then
            self.fx:Stop()
        else
            self.fx:Play()
        end
        Debug.Log("Portal particles alive:", self.fx.aliveCount)
    end
end

return PortalPulse
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn gradient(colors: &[(f32, [f32; 3])], alphas: &[(f32, f32)]) -> Gradient {
    Gradient {
        color_keys: colors
            .iter()
            .map(|&(time, color)| ColorKey { time, color })
            .collect(),
        alpha_keys: alphas
            .iter()
            .map(|&(time, alpha)| AlphaKey { time, alpha })
            .collect(),
    }
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

    fn init(&mut self, x: f32, y: f32, capacity: u32, center: [f32; 3], size: [f32; 3]) -> u32 {
        let c = self.context(ContextType::Initialize, x, y);
        if let Some(ctx) = self.g.context_mut(c)
            && let ContextKind::Initialize(s) = &mut ctx.kind
        {
            s.capacity = capacity;
            s.bounds_center = center;
            s.bounds_size = size;
            s.space = SimulationSpace::Local;
        }
        c
    }

    fn output(&mut self, x: f32, y: f32, settings: OutputSettings) -> u32 {
        let c = self.context(ContextType::Output, x, y);
        if let Some(ctx) = self.g.context_mut(c) {
            ctx.kind = ContextKind::Output(settings);
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

    fn op(&mut self, kind: OperatorKind, x: f32, y: f32, values: &[(&str, Value)]) -> u32 {
        let o = self.g.add_operator(kind, [x, y]);
        for (n, v) in values {
            self.g
                .operator_mut(o)
                .unwrap()
                .values
                .insert((*n).into(), v.clone());
        }
        o
    }

    fn prop(&mut self, name: &str, x: f32, y: f32) -> u32 {
        self.op(OperatorKind::Property { name: name.into() }, x, y, &[])
    }

    /// The graph, laid out tidily.
    fn finish(mut self) -> EffectGraph {
        crate::panels::effect_graph::layout::arrange(&mut self.g);
        self.g
    }

    fn link(&mut self, from: u32, out: &str, to: u32, input: &str) {
        self.g
            .connect(from, out, to, input)
            .unwrap_or_else(|e| panic!("{out} → {input}: {e}"));
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

/// Glowing motes drifting on curl noise, each flickering at its own phase.
pub fn fireflies() -> EffectGraph {
    let mut b = B::new();
    b.g.add_property("Rate", PropertyValue::Float(40.0));
    b.g.add_property("Color", PropertyValue::Color([3.0, 2.6, 0.7, 1.0]));
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    let rate = b.block(spawn, BlockKind::ConstantRate, &[]);
    let p = b.prop("Rate", -260.0, 40.0);
    b.link(p, "Out", rate, "Rate");
    let init = b.init(0.0, 140.0, 2048, [0.0, 1.4, 0.0], [9.0, 4.0, 9.0]);
    b.block(
        init,
        BlockKind::PositionBox {
            mode: ShapeMode::Volume,
        },
        &[("Center", v3(0.0, 1.3, 0.0)), ("Size", v3(7.0, 2.2, 7.0))],
    );
    b.set(
        init,
        Attribute::Velocity,
        true,
        &[("A", v3(-0.2, -0.1, -0.2)), ("B", v3(0.2, 0.15, 0.2))],
    );
    b.set(
        init,
        Attribute::Lifetime,
        true,
        &[("A", f(4.0)), ("B", f(8.0))],
    );
    b.set(
        init,
        Attribute::Size,
        true,
        &[("A", f(0.05)), ("B", f(0.11))],
    );
    let color = b.set(init, Attribute::Color, false, &[]);
    let pc = b.prop("Color", -260.0, 420.0);
    b.link(pc, "Out", color, "Color");
    let update = b.context(ContextType::Update, 0.0, 640.0);
    b.block(
        update,
        BlockKind::Turbulence,
        &[
            ("Intensity", f(1.4)),
            ("Frequency", f(0.6)),
            ("Scroll", f(0.15)),
        ],
    );
    b.block(update, BlockKind::LinearDrag, &[("Drag", f(0.8))]);
    let out = b.output(
        0.0,
        860.0,
        OutputSettings {
            kind: OutputKind::Quad,
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
                &[(0.0, [1.0, 1.0, 1.0]), (1.0, [1.0, 1.0, 1.0])],
                &[(0.0, 0.0), (0.15, 1.0), (0.8, 1.0), (1.0, 0.0)],
            )),
        )],
    );
    // Flicker: size × (0.6 + 0.4 sin(time × 7 + random phase)).
    let flicker = b.block(
        out,
        BlockKind::SetAttribute {
            attribute: Attribute::Size,
            random: false,
            composition: Composition::Multiply,
        },
        &[],
    );
    let time = b.op(OperatorKind::Time, -760.0, 900.0, &[]);
    let speed = b.op(OperatorKind::Multiply, -560.0, 900.0, &[("B", f(7.0))]);
    b.link(time, "Total", speed, "A");
    let phase = b.op(
        OperatorKind::GetAttribute {
            attribute: AttributeRead::ParticleRandom,
        },
        -760.0,
        1020.0,
        &[],
    );
    let spread = b.op(
        OperatorKind::Multiply,
        -560.0,
        1020.0,
        &[("B", f(std::f32::consts::TAU))],
    );
    b.link(phase, "Out", spread, "A");
    let sum = b.op(OperatorKind::Add, -380.0, 960.0, &[]);
    b.link(speed, "Out", sum, "A");
    b.link(spread, "Out", sum, "B");
    let sine = b.op(OperatorKind::Sine, -220.0, 960.0, &[]);
    b.link(sum, "Out", sine, "Input");
    let remap = b.op(
        OperatorKind::Remap,
        -220.0,
        1040.0,
        &[
            ("In Min", f(-1.0)),
            ("In Max", f(1.0)),
            ("Out Min", f(0.35)),
            ("Out Max", f(1.0)),
        ],
    );
    b.link(sine, "Out", remap, "Input");
    b.link(remap, "Out", flicker, "Size");
    b.finish()
}

/// Sparks shot up a cone, falling under gravity and bouncing on the ground, with bursts.
pub fn sparks_fountain() -> EffectGraph {
    let mut b = B::new();
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    b.block(spawn, BlockKind::ConstantRate, &[("Rate", f(140.0))]);
    b.block(
        spawn,
        BlockKind::PeriodicBurst,
        &[("Count", f(160.0)), ("Period", f(1.5)), ("Delay", f(0.0))],
    );
    let init = b.init(0.0, 200.0, 8192, [0.0, 2.0, 0.0], [9.0, 6.0, 9.0]);
    b.block(
        init,
        BlockKind::PositionCone {
            mode: ShapeMode::Volume,
        },
        &[("Radius", f(0.08)), ("Angle", f(10.0)), ("Height", f(0.05))],
    );
    b.block(
        init,
        BlockKind::VelocityFromDirection,
        &[("Min Speed", f(6.0)), ("Max Speed", f(8.5))],
    );
    b.set(
        init,
        Attribute::Lifetime,
        true,
        &[("A", f(1.6)), ("B", f(2.6))],
    );
    b.set(
        init,
        Attribute::Size,
        true,
        &[("A", f(0.025)), ("B", f(0.05))],
    );
    b.set(
        init,
        Attribute::Color,
        true,
        &[
            ("A", rgba(5.0, 2.6, 0.8, 1.0)),
            ("B", rgba(4.0, 1.2, 0.25, 1.0)),
        ],
    );
    let update = b.context(ContextType::Update, 0.0, 620.0);
    b.block(update, BlockKind::Gravity, &[]);
    b.block(
        update,
        BlockKind::CollidePlane,
        &[
            ("Bounce", f(0.45)),
            ("Friction", f(0.15)),
            ("Lifetime Loss", f(0.1)),
        ],
    );
    b.block(update, BlockKind::LinearDrag, &[("Drag", f(0.15))]);
    let out = b.output(
        0.0,
        940.0,
        OutputSettings {
            kind: OutputKind::Quad,
            blend: BlendMode::Additive,
            ..Default::default()
        },
    );
    b.block(
        out,
        BlockKind::Orient {
            mode: OrientMode::AlongVelocity,
        },
        &[("Stretch", f(0.045))],
    );
    b.block(
        out,
        BlockKind::ColorOverLife,
        &[(
            "Gradient",
            Value::Gradient(gradient(
                &[
                    (0.0, [1.0, 1.0, 0.9]),
                    (0.4, [1.0, 0.75, 0.35]),
                    (1.0, [0.9, 0.25, 0.1]),
                ],
                &[(0.0, 1.0), (0.7, 0.9), (1.0, 0.0)],
            )),
        )],
    );
    b.finish()
}

/// A swirling ring of light: circle spawn, vortex + turbulence, additive glow.
pub fn magic_portal() -> EffectGraph {
    let mut b = B::new();
    b.g.add_property("Rate", PropertyValue::Float(700.0));
    b.g.add_property("Color", PropertyValue::Color([1.6, 0.45, 3.6, 1.0]));
    b.g.add_property(
        "Fade",
        PropertyValue::Gradient(gradient(
            &[
                (0.0, [1.0, 1.0, 1.0]),
                (0.3, [0.8, 0.6, 1.0]),
                (1.0, [0.3, 0.5, 1.0]),
            ],
            &[(0.0, 0.0), (0.1, 1.0), (0.7, 0.8), (1.0, 0.0)],
        )),
    );
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    let rate = b.block(spawn, BlockKind::ConstantRate, &[]);
    let p = b.prop("Rate", -260.0, 40.0);
    b.link(p, "Out", rate, "Rate");
    let init = b.init(0.0, 140.0, 16_384, [0.0, 0.0, 0.0], [6.0, 6.0, 3.0]);
    b.block(
        init,
        BlockKind::PositionCircle {
            mode: ShapeMode::Surface,
        },
        &[("Radius", f(1.5))],
    );
    b.set(
        init,
        Attribute::Velocity,
        true,
        &[("A", v3(-0.1, -0.1, -0.3)), ("B", v3(0.1, 0.1, 0.3))],
    );
    b.set(
        init,
        Attribute::Lifetime,
        true,
        &[("A", f(1.4)), ("B", f(2.8))],
    );
    b.set(
        init,
        Attribute::Size,
        true,
        &[("A", f(0.03)), ("B", f(0.08))],
    );
    let color = b.set(init, Attribute::Color, false, &[]);
    let pc = b.prop("Color", -260.0, 400.0);
    b.link(pc, "Out", color, "Color");
    let update = b.context(ContextType::Update, 0.0, 600.0);
    b.block(
        update,
        BlockKind::Vortex,
        &[
            ("Axis", v3(0.0, 0.0, 1.0)),
            ("Speed", f(3.5)),
            ("Pull", f(0.5)),
        ],
    );
    b.block(
        update,
        BlockKind::Turbulence,
        &[
            ("Intensity", f(0.9)),
            ("Frequency", f(1.1)),
            ("Scroll", f(0.4)),
        ],
    );
    b.block(update, BlockKind::LinearDrag, &[("Drag", f(1.2))]);
    let out = b.output(
        0.0,
        900.0,
        OutputSettings {
            kind: OutputKind::Quad,
            blend: BlendMode::Additive,
            ..Default::default()
        },
    );
    let fade = b.block(out, BlockKind::ColorOverLife, &[]);
    let pf = b.prop("Fade", -260.0, 940.0);
    b.link(pf, "Out", fade, "Gradient");
    b.block(
        out,
        BlockKind::SizeOverLife,
        &[("Curve", Value::Curve(Curve::linear(1.0, 0.3)))],
    );
    // A second output: thin streaks along the swirl.
    let lines = b.output(
        360.0,
        900.0,
        OutputSettings {
            kind: OutputKind::Line,
            blend: BlendMode::Additive,
            line_length: 0.06,
            ..Default::default()
        },
    );
    let streak = b.block(
        lines,
        BlockKind::SetAttribute {
            attribute: Attribute::Color,
            random: false,
            composition: Composition::Multiply,
        },
        &[],
    );
    let dim = b.op(
        OperatorKind::Random {
            mode: RandomMode::PerParticle,
        },
        100.0,
        1100.0,
        &[("Min", f(0.1)), ("Max", f(0.6))],
    );
    b.link(dim, "Out", streak, "Color");
    b.finish()
}

/// One million points drifting on curl noise, held to a sphere.
pub fn million_particles() -> EffectGraph {
    let mut b = B::new();
    let spawn = b.context(ContextType::Spawn, 0.0, 0.0);
    b.block(spawn, BlockKind::ConstantRate, &[("Rate", f(260_000.0))]);
    let init = b.init(0.0, 140.0, 1 << 20, [0.0, 0.0, 0.0], [8.0, 8.0, 8.0]);
    b.block(
        init,
        BlockKind::PositionSphere {
            mode: ShapeMode::Surface,
        },
        &[("Radius", f(2.5))],
    );
    b.block(
        init,
        BlockKind::VelocityFromDirection,
        &[("Min Speed", f(0.0)), ("Max Speed", f(0.3))],
    );
    b.set(
        init,
        Attribute::Lifetime,
        true,
        &[("A", f(3.5)), ("B", f(4.5))],
    );
    b.set(init, Attribute::Size, false, &[("Size", f(0.04))]);
    let update = b.context(ContextType::Update, 0.0, 520.0);
    b.block(
        update,
        BlockKind::Turbulence,
        &[
            ("Intensity", f(1.5)),
            ("Frequency", f(0.55)),
            ("Scroll", f(0.2)),
        ],
    );
    b.block(
        update,
        BlockKind::ConformToSphere,
        &[
            ("Radius", f(2.5)),
            ("Attraction", f(8.0)),
            ("Stick", f(3.0)),
        ],
    );
    let out = b.output(
        0.0,
        820.0,
        OutputSettings {
            kind: OutputKind::Quad,
            blend: BlendMode::Additive,
            ..Default::default()
        },
    );
    // Colour from the position: a gradient sampled by height.
    let color = b.block(
        out,
        BlockKind::SetAttribute {
            attribute: Attribute::Color,
            random: false,
            composition: Composition::Overwrite,
        },
        &[],
    );
    let pos = b.op(
        OperatorKind::GetAttribute {
            attribute: AttributeRead::Position,
        },
        -700.0,
        860.0,
        &[],
    );
    let split = b.op(OperatorKind::Split, -520.0, 860.0, &[]);
    b.link(pos, "Out", split, "Input");
    let remap = b.op(
        OperatorKind::Remap,
        -360.0,
        860.0,
        &[
            ("In Min", f(-2.8)),
            ("In Max", f(2.8)),
            ("Out Min", f(0.0)),
            ("Out Max", f(1.0)),
        ],
    );
    b.link(split, "Y", remap, "Input");
    let sample = b.op(
        OperatorKind::SampleGradient,
        -200.0,
        860.0,
        &[(
            "Gradient",
            Value::Gradient(gradient(
                &[
                    (0.0, [0.15, 0.55, 1.0]),
                    (0.5, [0.75, 0.3, 1.0]),
                    (1.0, [1.0, 0.45, 0.6]),
                ],
                &[(0.0, 0.9), (1.0, 0.9)],
            )),
        )],
    );
    b.link(remap, "Out", sample, "Time");
    b.link(sample, "Out", color, "Color");
    b.block(
        out,
        BlockKind::ColorOverLife,
        &[(
            "Gradient",
            Value::Gradient(gradient(
                &[(0.0, [0.6, 0.6, 0.6]), (1.0, [0.6, 0.6, 0.6])],
                &[(0.0, 0.0), (0.1, 1.0), (0.85, 1.0), (1.0, 0.0)],
            )),
        )],
    );
    b.finish()
}

pub fn effects() -> [(&'static str, EffectGraph); 4] {
    [
        (FIREFLIES, fireflies()),
        (SPARKS, sparks_fountain()),
        (PORTAL, magic_portal()),
        (MILLION, million_particles()),
    ]
}

fn ground() -> Material {
    Material {
        color: [0.08, 0.09, 0.11, 1.0],
        smoothness: 0.35,
        ..Default::default()
    }
}

fn effect(
    s: &mut Scene,
    name: &str,
    path: &str,
    t: Transform,
    seed: u32,
) -> misty_engine::hecs::Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
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
    e
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("VfxDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 3.2, -10.5))
                .with_euler(Vec3::new(10.0, 0.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.clear_flags = ClearFlags::SolidColor;
        c.background = [0.02, 0.025, 0.05, 0.0];
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 0.2;
        l.color = [0.5, 0.6, 1.0];
    }
    let floor = s.spawn("Ground", None);
    let t = Transform {
        scale: Vec3::new(3.0, 1.0, 2.0),
        ..Default::default()
    };
    s.set_transform(floor, t);
    s.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(
        floor,
        MeshRenderer {
            material: Some(GROUND_MAT.into()),
            ..Default::default()
        },
    );
    effect(
        &mut s,
        "Fireflies",
        FIREFLIES,
        Transform::from_position(Vec3::new(-6.0, 0.0, 2.0)),
        11,
    );
    effect(
        &mut s,
        "Sparks Fountain",
        SPARKS,
        Transform::from_position(Vec3::new(-2.4, 0.0, 0.0)),
        12,
    );
    let portal = effect(
        &mut s,
        "Magic Portal",
        PORTAL,
        Transform::from_position(Vec3::new(1.8, 2.0, 0.5)),
        13,
    );
    s.add(
        portal,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    effect(
        &mut s,
        "Million Particles",
        MILLION,
        Transform::from_position(Vec3::new(7.0, 3.0, 7.0)),
        14,
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
fn write_vfx_demo() {
    let root = project();
    for (path, g) in effects() {
        g.save(&root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    ensure_meta(&root.join("Assets/Effects"));
    ground().save(&root.join(GROUND_MAT)).unwrap();
    ensure_meta(&root.join(GROUND_MAT));
    std::fs::write(root.join(SCRIPT), PORTAL_PULSE).unwrap();
    ensure_meta(&root.join(SCRIPT));
    demo_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn vfx_demo_matches_its_generator() {
    let root = project();
    for (path, g) in effects() {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        assert_eq!(EffectGraph::from_json(&text).unwrap(), g, "{path}");
        assert!(
            root.join(format!("{path}.meta")).exists(),
            "{path} has a GUID"
        );
    }
    assert_eq!(Material::load(&root.join(GROUND_MAT)).unwrap(), ground());
    let script = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(script.replace("\r\n", "\n"), PORTAL_PULSE);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == demo_scene().to_json(),
        "regenerate with write_vfx_demo"
    );
    let (_, report) = Scene::from_json("VfxDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn demo_effects_compile_with_their_features() {
    for (path, g) in effects() {
        let c = misty_engine::vfx::compile(&g).unwrap_or_else(|e| panic!("{path}: {e:?}"));
        match path {
            FIREFLIES => {
                assert!(c.sim_source.contains("vfx_curl"), "turbulence");
                assert!(c.outputs[0].source.contains("sin("), "flicker");
            }
            SPARKS => {
                assert_eq!(c.spawn.len(), 2, "rate + periodic burst");
                assert!(c.sim_source.contains("next < 0.0f"), "plane collision");
            }
            PORTAL => {
                assert_eq!(c.outputs.len(), 2, "quads and lines");
                assert!(c.slot("Rate").is_some() && c.slot("Color").is_some());
                assert!(c.sim_source.contains("tangent"), "vortex");
            }
            _ => assert_eq!(c.capacity, 1 << 20),
        }
    }
}

/// The portal script drives the effect through the Lua API while playing.
#[test]
fn portal_script_sets_properties() {
    let root = project();
    misty_engine::assets::set_project_root(&root);
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let settings = misty_engine::settings::ProjectSettings::default();
    let (audio, _log) = misty_engine::audio::Audio::null(root.clone());
    let mut rt = misty_engine::runtime::Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &settings,
        audio,
    );
    let mut time = misty_engine::runtime::Time::default();
    for _ in 0..5 {
        time.advance(1.0 / 30.0);
        rt.update(&mut scene, &time, Default::default());
    }
    let portal = scene.find_by_name("Magic Portal").unwrap();
    let v = scene.world.get::<&VisualEffect>(portal).unwrap();
    assert!(
        matches!(v.override_value("Rate"), Some(PropertyValue::Float(r)) if *r > 400.0),
        "{:?}",
        v.overrides
    );
    assert!(matches!(
        v.override_value("Color"),
        Some(PropertyValue::Color(_))
    ));
    let inst = scene
        .world
        .get::<&misty_engine::vfx::EffectInstance>(portal)
        .unwrap();
    assert!(inst.playing && inst.serial == 5, "stepped every frame");
}

/// On a GPU: the demo renders bright particles over the dark ground.
#[test]
fn vfx_demo_renders() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    misty_engine::assets::set_project_root(&root);
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    // Absolute effect paths: independent of the process-wide project root other tests change.
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
    for _ in 0..45 {
        misty_engine::vfx::update(&mut scene, 1.0 / 30.0);
        let v = misty_engine::render_scene::GameView::new(&scene, &assets).unwrap();
        renderer.render_with_effects(device, queue, &target, &v.frame(), &v.effects);
        view = Some(v);
    }
    let view = view.unwrap();
    assert_eq!(view.effects.len(), 4, "every effect compiled");
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
    let img = crate::sample_render::Image { pixels };
    let lum: Vec<f32> = img
        .pixels
        .chunks(4)
        .map(|p| 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32)
        .collect();
    let mean = lum.iter().sum::<f32>() / lum.len() as f32;
    let var = lum.iter().map(|l| (l - mean) * (l - mean)).sum::<f32>() / lum.len() as f32;
    eprintln!("mean {mean}, std {}", var.sqrt());
    assert!(var.sqrt() > 8.0, "flat image: std {}", var.sqrt());
    // The portal ring glows against the night.
    let ring = img.brightness(&view.camera, Vec3::new(1.8 + 1.5, 2.0, 0.5));
    let sky = img.brightness(&view.camera, Vec3::new(0.0, 6.0, 8.0));
    assert!(ring > sky + 20.0, "portal {ring} vs sky {sky}");
}
