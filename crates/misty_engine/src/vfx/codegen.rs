//! Effect Graph → WGSL. One simulation shader per graph holds every compute kernel
//! (`cs_reset`, `cs_emit`, `cs_args`, `cs_update`, `cs_finish`); each Output context gets a render
//! shader (`vs_main` + `fs_main`). Block inputs become expressions of the operators wired into
//! them, emitted inside the block's own scope. The Spawn context compiles to CPU expressions
//! (see [`super::spawn`]).
//!
//! GPU data flow per step (see `render/vfx`): emit pops indices from the dead list and appends
//! new particles to the current alive list; `cs_args` writes the indirect dispatch for the
//! update; update ages, moves and reaps the particles of that list, appending survivors to the
//! other alive list (reaped ones go back on the dead list); `cs_finish` writes the indirect draw
//! arguments. The lists swap every step.

use std::collections::HashMap;
use std::hash::{Hash, Hasher as _};

use glam::Vec3;

use super::blocks::{Attribute, BlockKind, Composition, ContextType, OrientMode, ShapeMode};
use super::graph::{
    BlendMode, Block, ContextKind, EffectGraph, EffectProperty, Item, MAX_CAPACITY, OutputKind,
    OutputSettings, PropertyValue, SimulationSpace,
};
use super::operators::{OperatorKind, RandomMode};
use super::types::{Curve, Gradient, Value, ValueType, color_to_linear, lit};

/// Shared structs and helpers (after the bindings of each shader).
pub const COMMON: &str = include_str!("shaders/common.wgsl");
/// Numeric property slots (vec4 each) in the properties uniform.
pub const MAX_PROPERTIES: usize = 64;
/// Gradient / curve rows of one effect.
pub const MAX_RAMPS: usize = 64;
/// Samples per baked ramp row (matches `VFX_RAMP_SAMPLES`).
pub const RAMP_SAMPLES: usize = 64;
pub const WORKGROUP: u32 = 64;

/// Bindings of the simulation shader (group 0).
pub const SIM_BINDINGS: &str = r#"
struct VfxSimParams {
    world: mat4x4<f32>,
    dt: f32,
    time: f32,
    spawn_count: u32,
    capacity: u32,
    seed: u32,
    serial: u32,
    parity: u32,
    world_space: u32,
};

struct VfxCounters {
    dead: atomic<i32>,
    alive: array<atomic<u32>, 2>,
    spawned: atomic<u32>,
    reaped: atomic<u32>,
    last_alive: u32,
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var<uniform> vfx_params: VfxSimParams;
@group(0) @binding(1) var<storage, read_write> vfx_particles: array<VfxParticle>;
@group(0) @binding(2) var<storage, read_write> vfx_dead: array<u32>;
@group(0) @binding(3) var<storage, read_write> vfx_alive_in: array<u32>;
@group(0) @binding(4) var<storage, read_write> vfx_alive_out: array<u32>;
@group(0) @binding(5) var<storage, read_write> vfx_counters: VfxCounters;
@group(0) @binding(6) var<storage, read_write> vfx_indirect: array<u32>;
@group(0) @binding(7) var<uniform> vfx_props: VfxProps;
@group(0) @binding(8) var<storage, read> vfx_ramps: array<vec4<f32>>;
"#;

/// Bindings of an output shader (group 0 is the renderer's view group).
pub const RENDER_BINDINGS: &str = r#"
struct VfxRenderParams {
    world: mat4x4<f32>,
    time: f32,
    dt: f32,
    world_space: u32,
    textured: u32,
    // x = columns, y = rows, z = 1 for frames per second (else over life), w = fps.
    flipbook: vec4<f32>,
    // x = line length per unit of speed, y = soft particle distance (0 = off).
    misc: vec4<f32>,
};

@group(1) @binding(0) var vfx_tex: texture_2d<f32>;
@group(1) @binding(2) var vfx_sampler: sampler;
@group(2) @binding(0) var<storage, read> vfx_particles: array<VfxParticle>;
@group(2) @binding(1) var<storage, read> vfx_alive: array<u32>;
@group(2) @binding(2) var<uniform> vfx_props: VfxProps;
@group(2) @binding(3) var<storage, read> vfx_ramps: array<vec4<f32>>;
@group(2) @binding(4) var<uniform> vfx_params: VfxRenderParams;
// The opaque scene's depth (soft particles; a far 1×1 texture when there is no copy).
@group(3) @binding(0) var vfx_scene_depth: texture_2d<f32>;
"#;

/// A problem in the graph, on a context, block or operator when it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphError {
    pub message: String,
    pub node: Option<u32>,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn err(node: Option<u32>, message: impl Into<String>) -> GraphError {
    GraphError {
        message: message.into(),
        node,
    }
}

/// Where a baked ramp row comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum RampSource {
    Gradient(Gradient),
    Curve(Curve),
    /// A Gradient property (instances may override it).
    Property(String),
}

/// A spawn input evaluated on the CPU.
#[derive(Clone, Debug, PartialEq)]
pub enum CpuExpr {
    Const(f32),
    Property(String),
    Binary(char, Box<CpuExpr>, Box<CpuExpr>),
}

/// One enabled Spawn block.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnBlock {
    pub kind: BlockKind,
    pub inputs: Vec<(&'static str, CpuExpr)>,
}

impl SpawnBlock {
    pub fn input(&self, name: &str) -> Option<&CpuExpr> {
        self.inputs.iter().find(|(n, _)| *n == name).map(|(_, e)| e)
    }
}

/// One Output context, ready for the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledOutput {
    pub source: String,
    pub settings: OutputSettings,
    /// Vertices per particle (6 quad, 1 point, 2 line).
    pub vertex_count: u32,
}

/// A graph turned into shaders and the data instances need.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEffect {
    /// Content hash (pipelines and GPU state are keyed by it).
    pub hash: u64,
    pub sim_source: String,
    pub outputs: Vec<CompiledOutput>,
    pub capacity: u32,
    pub bounds_center: Vec3,
    pub bounds_size: Vec3,
    pub space: SimulationSpace,
    pub spawn: Vec<SpawnBlock>,
    /// Blackboard properties with their defaults.
    pub properties: Vec<EffectProperty>,
    /// Numeric property → uniform slot.
    pub slots: Vec<(String, usize)>,
    pub ramps: Vec<RampSource>,
}

impl CompiledEffect {
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.slots.iter().find(|(n, _)| n == name).map(|(_, s)| *s)
    }
}

/// Which kernel is being generated (decides what operators can read).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Init,
    Update,
    Output,
}

struct Gen<'a> {
    g: &'a EffectGraph,
    types: super::graph::Types,
    ramps: Vec<RampSource>,
    slots: Vec<(String, usize)>,
    errors: Vec<GraphError>,
    /// Lines of the current kernel body and the node each came from.
    lines: Vec<(String, Option<u32>)>,
    memo: HashMap<(u32, String), (String, ValueType)>,
    counter: usize,
    stage: Stage,
    /// Block or operator whose code is being emitted.
    owner: Option<u32>,
}

/// Convert an expression between value types (floats splat, vectors truncate, alpha pads 1).
fn convert(e: &str, from: ValueType, to: ValueType) -> String {
    use ValueType as T;
    match (from, to) {
        (a, b) if a == b => e.to_owned(),
        (T::Float, T::Vec3) => format!("vec3<f32>({e})"),
        (T::Float, T::Color) => format!("vec4<f32>(vec3<f32>({e}), 1.0f)"),
        (T::Vec3, T::Float) | (T::Color, T::Float) => format!("({e}).x"),
        (T::Vec3, T::Color) => format!("vec4<f32>({e}, 1.0f)"),
        (T::Color, T::Vec3) => format!("({e}).xyz"),
        _ => e.to_owned(),
    }
}

fn splat(v: f32, ty: ValueType) -> String {
    convert(&lit(v), ValueType::Float, ty)
}

impl<'a> Gen<'a> {
    fn new(g: &'a EffectGraph) -> Self {
        let mut slots = Vec::new();
        for p in &g.properties {
            if matches!(
                p.value,
                PropertyValue::Float(_) | PropertyValue::Vector3(_) | PropertyValue::Color(_)
            ) {
                slots.push((p.name.clone(), slots.len()));
            }
        }
        Self {
            g,
            types: g.types(),
            ramps: Vec::new(),
            slots,
            errors: Vec::new(),
            lines: Vec::new(),
            memo: HashMap::new(),
            counter: 0,
            stage: Stage::Init,
            owner: None,
        }
    }

    fn line(&mut self, s: impl Into<String>) {
        self.lines.push((s.into(), self.owner));
    }

    fn var(&mut self, ty: ValueType, expr: String) -> String {
        self.counter += 1;
        let name = format!("v{}", self.counter);
        self.line(format!("    let {name}: {} = {expr};", ty.wgsl()));
        name
    }

    fn ramp(&mut self, src: RampSource, node: Option<u32>) -> String {
        let i = match self.ramps.iter().position(|r| *r == src) {
            Some(i) => i,
            None => {
                if self.ramps.len() >= MAX_RAMPS {
                    self.errors.push(err(
                        node,
                        format!("more than {MAX_RAMPS} gradients and curves"),
                    ));
                    return "0u".into();
                }
                self.ramps.push(src);
                self.ramps.len() - 1
            }
        };
        format!("{i}u")
    }

    /// Literal of an inline value as `ty`.
    fn literal(&mut self, v: &Value, ty: ValueType, node: Option<u32>) -> String {
        match v {
            Value::Gradient(g) => self.ramp(RampSource::Gradient(g.clone()), node),
            Value::Curve(c) => self.ramp(RampSource::Curve(c.clone()), node),
            Value::Float(f) => convert(&lit(*f), ValueType::Float, ty),
            Value::Vec3(a) => convert(
                &format!("vec3<f32>({}, {}, {})", lit(a[0]), lit(a[1]), lit(a[2])),
                ValueType::Vec3,
                ty,
            ),
            Value::Color(c) => {
                let l = color_to_linear(*c);
                convert(
                    &format!(
                        "vec4<f32>({}, {}, {}, {})",
                        lit(l[0]),
                        lit(l[1]),
                        lit(l[2]),
                        lit(l[3])
                    ),
                    ValueType::Color,
                    ty,
                )
            }
        }
    }

    /// Expression for input `name` of block/operator `id`, converted to `want`.
    fn input(&mut self, id: u32, name: &str, want: ValueType) -> String {
        if let Some(l) = self.g.input_link(id, name).cloned() {
            match self.op_output(l.from, &l.output) {
                Some((e, ty)) => return convert(&e, ty, want),
                None => return splat(0.0, want),
            }
        }
        let port = self.g.inputs(id).into_iter().find(|p| p.name == name);
        let value = match self.g.find(id) {
            Some(Item::Block(ci, bi)) => {
                let b = &self.g.contexts[ci].blocks[bi];
                port.map(|p| b.value(&p))
            }
            Some(Item::Operator(oi)) => {
                let o = &self.g.operators[oi];
                port.map(|p| o.value(&p))
            }
            _ => None,
        };
        match value {
            Some(v) => self.literal(&v, want, Some(id)),
            None => splat(0.0, want),
        }
    }

    /// The type an unconnected or connected input of an operator has after resolution.
    fn input_type(&self, id: u32, name: &str) -> ValueType {
        let Some(op) = self.g.operator(id) else {
            return ValueType::Float;
        };
        match op.kind.input(name) {
            Some(p) => p.ty.resolve(self.types.dynamic(id)),
            None => ValueType::Float,
        }
    }

    /// Emit an operator output (memoised per block); None after reporting an error.
    fn op_output(&mut self, id: u32, port: &str) -> Option<(String, ValueType)> {
        if let Some(v) = self.memo.get(&(id, port.to_owned())) {
            return Some(v.clone());
        }
        let op = self.g.operator(id)?.clone();
        let d = self.types.dynamic(id);
        let out_ty = self.types.output(id, port).unwrap_or(d);
        let saved = self.owner;
        self.owner = Some(id);
        let i = |s: &mut Self, n: &str| {
            let t = s.input_type(id, n);
            s.input(id, n, t)
        };
        let expr: Option<String> = match &op.kind {
            OperatorKind::Property { name } => match self.g.property(name).map(|p| &p.value) {
                None => {
                    self.errors
                        .push(err(Some(id), format!("no property called {name}")));
                    None
                }
                Some(PropertyValue::Texture(_)) => {
                    self.errors.push(err(
                        Some(id),
                        "Texture properties are used by an Output context's texture",
                    ));
                    None
                }
                Some(PropertyValue::Gradient(_)) => {
                    Some(self.ramp(RampSource::Property(name.clone()), Some(id)))
                }
                Some(v) => {
                    let slot = self
                        .slots
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, s)| *s)
                        .unwrap_or(0);
                    if slot >= MAX_PROPERTIES {
                        self.errors.push(err(
                            Some(id),
                            format!("more than {MAX_PROPERTIES} numeric properties"),
                        ));
                    }
                    Some(match v {
                        PropertyValue::Float(_) => format!("vfx_props.v[{slot}].x"),
                        PropertyValue::Vector3(_) => format!("vfx_props.v[{slot}].xyz"),
                        _ => format!("vfx_props.v[{slot}]"),
                    })
                }
            },
            OperatorKind::Float | OperatorKind::Vector3 | OperatorKind::Color => {
                Some(i(self, "Value"))
            }
            OperatorKind::Time => Some(if port == "Delta" {
                "vfx_params.dt".into()
            } else {
                "vfx_params.time".into()
            }),
            OperatorKind::GetAttribute { attribute } => Some(attribute.expr().to_owned()),
            OperatorKind::Add
            | OperatorKind::Subtract
            | OperatorKind::Multiply
            | OperatorKind::Divide => {
                let (a, b) = (self.input(id, "A", d), self.input(id, "B", d));
                let o = match op.kind {
                    OperatorKind::Add => "+",
                    OperatorKind::Subtract => "-",
                    OperatorKind::Multiply => "*",
                    _ => "/",
                };
                Some(format!("({a} {o} {b})"))
            }
            OperatorKind::Minimum | OperatorKind::Maximum | OperatorKind::Power => {
                let (a, b) = (self.input(id, "A", d), self.input(id, "B", d));
                let f = match op.kind {
                    OperatorKind::Minimum => "min",
                    OperatorKind::Maximum => "max",
                    _ => "pow",
                };
                Some(format!("{f}({a}, {b})"))
            }
            OperatorKind::Lerp => {
                let (a, b) = (self.input(id, "A", d), self.input(id, "B", d));
                let t = self.input(id, "T", d);
                Some(format!("mix({a}, {b}, {t})"))
            }
            OperatorKind::Clamp => {
                let x = self.input(id, "Input", d);
                let (lo, hi) = (self.input(id, "Min", d), self.input(id, "Max", d));
                Some(format!("clamp({x}, {lo}, {hi})"))
            }
            OperatorKind::Remap => {
                let x = self.input(id, "Input", d);
                let im = self.input(id, "In Min", d);
                let ix = self.input(id, "In Max", d);
                let om = self.input(id, "Out Min", d);
                let ox = self.input(id, "Out Max", d);
                Some(format!(
                    "mix({om}, {ox}, ({x} - {im}) / max({ix} - {im}, {}))",
                    splat(1e-6, d)
                ))
            }
            OperatorKind::Absolute => Some(format!("abs({})", self.input(id, "Input", d))),
            OperatorKind::Negate => Some(format!("(-{})", self.input(id, "Input", d))),
            OperatorKind::OneMinus => Some(format!(
                "({} - {})",
                splat(1.0, d),
                self.input(id, "Input", d)
            )),
            OperatorKind::Fraction => Some(format!("fract({})", self.input(id, "Input", d))),
            OperatorKind::Sine => Some(format!("sin({})", self.input(id, "Input", d))),
            OperatorKind::Cosine => Some(format!("cos({})", self.input(id, "Input", d))),
            OperatorKind::Length => Some(format!("length({})", i(self, "Input"))),
            OperatorKind::Normalize => Some(format!("vfx_safe_normalize({})", i(self, "Input"))),
            OperatorKind::Distance | OperatorKind::Dot | OperatorKind::Cross => {
                let (a, b) = (i(self, "A"), i(self, "B"));
                let f = match op.kind {
                    OperatorKind::Distance => "distance",
                    OperatorKind::Dot => "dot",
                    _ => "cross",
                };
                Some(format!("{f}({a}, {b})"))
            }
            OperatorKind::Combine => {
                let (x, y, z) = (i(self, "X"), i(self, "Y"), i(self, "Z"));
                Some(format!("vec3<f32>({x}, {y}, {z})"))
            }
            OperatorKind::Split => {
                let v = i(self, "Input");
                let c = match port {
                    "Y" => "y",
                    "Z" => "z",
                    _ => "x",
                };
                Some(format!("({v}).{c}"))
            }
            OperatorKind::Random { mode } => {
                let (lo, hi) = (self.input(id, "Min", d), self.input(id, "Max", d));
                let salt = id.wrapping_mul(2_654_435_761);
                let r = match (mode, d) {
                    (RandomMode::PerEvaluation, ValueType::Vec3) => "vfx_rand3()".to_owned(),
                    (RandomMode::PerEvaluation, ValueType::Color) => "vfx_rand4()".to_owned(),
                    (RandomMode::PerEvaluation, _) => "vfx_rand()".to_owned(),
                    (RandomMode::PerParticle, ValueType::Vec3) => {
                        format!("vfx_prand3(p.seed, {salt}u)")
                    }
                    (RandomMode::PerParticle, ValueType::Color) => {
                        format!("vfx_prand4(p.seed, {salt}u)")
                    }
                    (RandomMode::PerParticle, _) => format!("vfx_prand(p.seed, {salt}u)"),
                };
                Some(format!("mix({lo}, {hi}, {r})"))
            }
            OperatorKind::Noise | OperatorKind::CurlNoise => {
                let c = i(self, "Coordinate");
                let f = i(self, "Frequency");
                let func = if op.kind == OperatorKind::Noise {
                    "vfx_noise"
                } else {
                    "vfx_curl"
                };
                Some(format!("{func}({c} * {f})"))
            }
            OperatorKind::SampleGradient => {
                let g = self.input(id, "Gradient", ValueType::Gradient);
                let t = i(self, "Time");
                Some(format!("vfx_ramp({g}, {t})"))
            }
            OperatorKind::SampleCurve => {
                let c = self.input(id, "Curve", ValueType::Curve);
                let t = i(self, "Time");
                Some(format!("vfx_ramp({c}, {t}).x"))
            }
        };
        let result = expr.map(|e| {
            let name = self.var(out_ty, e);
            (name, out_ty)
        });
        self.owner = saved;
        if let Some(r) = &result {
            self.memo.insert((id, port.to_owned()), r.clone());
        }
        result
    }

    /// Statements of one block.
    fn block(&mut self, b: &Block) {
        if !b.enabled {
            return;
        }
        self.memo.clear();
        self.owner = Some(b.id);
        self.line("    {");
        let id = b.id;
        let f = |s: &mut Self, n: &str| s.input(id, n, ValueType::Float);
        let v3 = |s: &mut Self, n: &str| s.input(id, n, ValueType::Vec3);
        match &b.kind {
            BlockKind::ConstantRate | BlockKind::SingleBurst | BlockKind::PeriodicBurst => {}
            BlockKind::SetAttribute {
                attribute,
                random,
                composition,
            } => {
                let ty = attribute.ty();
                let value = if *random {
                    let a = self.input(id, "A", ty);
                    let bb = self.input(id, "B", ty);
                    let r = match ty {
                        ValueType::Vec3 => "vfx_rand3()",
                        _ => "vfx_rand()",
                    };
                    format!("mix({a}, {bb}, {r})")
                } else {
                    self.input(id, attribute.label(), ty)
                };
                let field = attribute.field();
                let rhs = match composition {
                    Composition::Overwrite => value,
                    Composition::Add => format!("{field} + {value}"),
                    Composition::Multiply => format!("{field} * {value}"),
                };
                self.line(format!("    {field} = {rhs};"));
            }
            BlockKind::PositionSphere { mode } => {
                let c = v3(self, "Center");
                let r = f(self, "Radius");
                let k = match mode {
                    ShapeMode::Volume => format!("{r} * pow(vfx_rand(), 0.33333334f)"),
                    ShapeMode::Surface => r,
                };
                self.line("    let d = vfx_rand_dir();");
                self.line(format!("    p.position = {c} + d * ({k});"));
                self.line("    direction = d;");
            }
            BlockKind::PositionBox { mode } => {
                let c = v3(self, "Center");
                let s = v3(self, "Size");
                self.line("    var q = vfx_rand3() - vec3<f32>(0.5f);");
                if *mode == ShapeMode::Surface {
                    self.line("    let axis = min(u32(vfx_rand() * 3.0f), 2u);");
                    self.line("    q[axis] = select(-0.5f, 0.5f, vfx_rand() < 0.5f);");
                }
                self.line(format!("    p.position = {c} + q * {s};"));
                self.line("    direction = vfx_safe_normalize(q);");
            }
            BlockKind::PositionCone { mode } => {
                let r = f(self, "Radius");
                let a = f(self, "Angle");
                let h = f(self, "Height");
                let (rr, hh) = match mode {
                    ShapeMode::Volume => ("sqrt(vfx_rand())", "vfx_rand()"),
                    ShapeMode::Surface => ("1.0f", "0.0f"),
                };
                self.line(format!("    let ang = radians({a});"));
                self.line("    let turn = vfx_rand() * 6.2831853f;");
                self.line(format!("    let rr = {rr};"));
                self.line(format!("    let hf = {hh};"));
                self.line(format!("    let rad = {r} + tan(ang) * {h} * hf;"));
                self.line("    let disc = vec2<f32>(cos(turn), sin(turn)) * rr;");
                self.line(format!(
                    "    p.position = vec3<f32>(disc.x * rad, hf * {h}, disc.y * rad);"
                ));
                self.line(
                    "    direction = vfx_safe_normalize(vec3<f32>(disc.x * sin(ang), cos(ang), disc.y * sin(ang)));",
                );
            }
            BlockKind::PositionCircle { mode } => {
                let c = v3(self, "Center");
                let r = f(self, "Radius");
                let rr = match mode {
                    ShapeMode::Volume => "sqrt(vfx_rand())",
                    ShapeMode::Surface => "1.0f",
                };
                self.line("    let turn = vfx_rand() * 6.2831853f;");
                self.line("    let dir2 = vec3<f32>(cos(turn), sin(turn), 0.0f);");
                self.line(format!("    p.position = {c} + dir2 * ({r} * {rr});"));
                self.line("    direction = dir2;");
            }
            BlockKind::VelocityFromDirection => {
                let lo = f(self, "Min Speed");
                let hi = f(self, "Max Speed");
                self.line(format!(
                    "    p.velocity = direction * mix({lo}, {hi}, vfx_rand());"
                ));
            }
            BlockKind::Gravity => {
                let g = v3(self, "Force");
                self.line(format!("    p.velocity = p.velocity + {g} * dt;"));
            }
            BlockKind::LinearDrag => {
                let d = f(self, "Drag");
                self.line(format!(
                    "    p.velocity = p.velocity / (1.0f + max({d}, 0.0f) * dt);"
                ));
            }
            BlockKind::Turbulence => {
                let k = f(self, "Intensity");
                let q = f(self, "Frequency");
                let s = f(self, "Scroll");
                self.line(format!(
                    "    let field = vfx_curl(p.position * {q} + vec3<f32>(vfx_params.time * {s}));"
                ));
                self.line(format!("    p.velocity = p.velocity + field * {k} * dt;"));
            }
            BlockKind::ConformToSphere => {
                let c = v3(self, "Center");
                let r = f(self, "Radius");
                let a = f(self, "Attraction");
                let s = f(self, "Stick");
                self.line(format!("    let to_c = {c} - p.position;"));
                self.line("    let dist = length(to_c);");
                self.line("    let dir = to_c / max(dist, 0.0001f);");
                self.line(format!("    let off = dist - {r};"));
                self.line(format!(
                    "    p.velocity = p.velocity + dir * off * {a} * dt;"
                ));
                self.line(format!(
                    "    p.velocity = p.velocity / (1.0f + {s} * dt * clamp(1.0f - abs(off), 0.0f, 1.0f));"
                ));
            }
            BlockKind::Vortex => {
                let c = v3(self, "Center");
                let ax = v3(self, "Axis");
                let sp = f(self, "Speed");
                let pull = f(self, "Pull");
                self.line(format!("    let ax = vfx_safe_normalize({ax});"));
                self.line(format!("    let rel = p.position - {c};"));
                self.line("    let radial = rel - ax * dot(rel, ax);");
                self.line("    let rl = max(length(radial), 0.0001f);");
                self.line("    let tangent = cross(ax, radial / rl);");
                self.line(format!(
                    "    p.velocity = p.velocity + (tangent * {sp} - radial / rl * {pull}) * dt;"
                ));
            }
            BlockKind::CollidePlane => {
                let pt = v3(self, "Point");
                let n = v3(self, "Normal");
                let bounce = f(self, "Bounce");
                let fr = f(self, "Friction");
                let loss = f(self, "Lifetime Loss");
                self.line(format!("    let n = vfx_safe_normalize({n});"));
                self.line("    let radius = p.size * 0.5f;");
                self.line(format!(
                    "    let next = dot(p.position + p.velocity * dt - {pt}, n) - radius;"
                ));
                self.line("    if (next < 0.0f) {");
                self.line(format!(
                    "        let now = dot(p.position - {pt}, n) - radius;"
                ));
                self.line("        p.position = p.position - n * min(now, 0.0f);");
                self.line("        let vn = dot(p.velocity, n);");
                self.line("        if (vn < 0.0f) {");
                self.line("            let vt = p.velocity - n * vn;");
                self.line(format!(
                    "            p.velocity = vt * (1.0f - clamp({fr}, 0.0f, 1.0f)) - n * vn * {bounce};"
                ));
                self.line(format!("            p.age = p.age + {loss} * p.lifetime;"));
                self.line("        }");
                self.line("    }");
            }
            BlockKind::CollideSphere { inverted } => {
                let c = v3(self, "Center");
                let r = f(self, "Radius");
                let bounce = f(self, "Bounce");
                let fr = f(self, "Friction");
                let sign = if *inverted { "-1.0f" } else { "1.0f" };
                self.line(format!("    let rel = p.position + p.velocity * dt - {c};"));
                self.line("    let dist = length(rel);");
                self.line("    let n0 = rel / max(dist, 0.0001f);");
                self.line(format!("    let n = n0 * {sign};"));
                self.line("    let radius = p.size * 0.5f;");
                self.line(format!(
                    "    let pen = ({r} + radius * {sign} - dist) * {sign};"
                ));
                self.line("    if (pen > 0.0f) {");
                self.line("        p.position = p.position + n * pen;");
                self.line("        let vn = dot(p.velocity, n);");
                self.line("        if (vn < 0.0f) {");
                self.line("            let vt = p.velocity - n * vn;");
                self.line(format!(
                    "            p.velocity = vt * (1.0f - clamp({fr}, 0.0f, 1.0f)) - n * vn * {bounce};"
                ));
                self.line("        }");
                self.line("    }");
            }
            BlockKind::KillSphere => {
                let c = v3(self, "Center");
                let r = f(self, "Radius");
                self.line(format!(
                    "    if (distance(p.position, {c}) < {r}) {{ kill = true; }}"
                ));
            }
            BlockKind::ColorOverLife => {
                let g = self.input(id, "Gradient", ValueType::Gradient);
                if self.stage == Stage::Output {
                    self.line(format!(
                        "    p.color = p.color * vfx_ramp({g}, vfx_life(p));"
                    ));
                } else {
                    self.line(format!("    p.color = vfx_ramp({g}, vfx_life(p));"));
                }
            }
            BlockKind::SizeOverLife => {
                let c = self.input(id, "Curve", ValueType::Curve);
                if self.stage == Stage::Output {
                    self.line(format!(
                        "    p.size = p.size * vfx_ramp({c}, vfx_life(p)).x;"
                    ));
                } else {
                    self.line(format!("    p.size = vfx_ramp({c}, vfx_life(p)).x;"));
                }
            }
            BlockKind::Orient { mode } => {
                let m = match mode {
                    OrientMode::FaceCamera => 0,
                    OrientMode::AlongVelocity => 1,
                    OrientMode::FaceUp => 2,
                };
                self.line(format!("    orient = {m}u;"));
                if *mode == OrientMode::AlongVelocity {
                    let s = f(self, "Stretch");
                    self.line(format!("    stretch = {s};"));
                }
            }
        }
        self.line("    }");
        self.owner = None;
    }

    fn take_body(&mut self) -> Vec<(String, Option<u32>)> {
        std::mem::take(&mut self.lines)
    }
}

/// Assemble shader text from parts, remembering which node each line came from.
struct Source {
    text: String,
    owners: Vec<Option<u32>>,
}

impl Source {
    fn new() -> Self {
        Self {
            text: String::new(),
            owners: Vec::new(),
        }
    }

    fn push(&mut self, s: &str) {
        for l in s.lines() {
            self.text.push_str(l);
            self.text.push('\n');
            self.owners.push(None);
        }
    }

    fn body(&mut self, lines: &[(String, Option<u32>)]) {
        for (l, o) in lines {
            self.text.push_str(l);
            self.text.push('\n');
            self.owners.push(*o);
        }
    }
}

fn sim_shader(
    init: &[(String, Option<u32>)],
    update: &[(String, Option<u32>)],
    update_settings: &super::graph::UpdateSettings,
    outputs: &[(u32, u32)],
) -> Source {
    let mut s = Source::new();
    s.push(SIM_BINDINGS);
    s.push(COMMON);
    s.push(&format!(
        r#"
@compute @workgroup_size({WORKGROUP})
fn cs_reset(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let i = gid.x;
    if (i >= vfx_params.capacity) {{
        return;
    }}
    vfx_dead[i] = vfx_params.capacity - 1u - i;
    vfx_particles[i].alive = 0u;
    if (i == 0u) {{
        atomicStore(&vfx_counters.dead, i32(vfx_params.capacity));
        atomicStore(&vfx_counters.alive[0], 0u);
        atomicStore(&vfx_counters.alive[1], 0u);
        atomicStore(&vfx_counters.spawned, 0u);
        atomicStore(&vfx_counters.reaped, 0u);
        vfx_counters.last_alive = 0u;
    }}
}}

@compute @workgroup_size({WORKGROUP})
fn cs_emit(@builtin(global_invocation_id) gid: vec3<u32>) {{
    if (gid.x >= vfx_params.spawn_count) {{
        return;
    }}
    let old = atomicSub(&vfx_counters.dead, 1);
    if (old <= 0) {{
        atomicAdd(&vfx_counters.dead, 1);
        return;
    }}
    let index = vfx_dead[u32(old - 1)];
    vfx_rng = vfx_hash(vfx_params.seed ^ vfx_hash(vfx_params.serial * 2654435769u + gid.x));
    var p: VfxParticle;
    p.position = vec3<f32>(0.0);
    p.velocity = vec3<f32>(0.0);
    p.age = 0.0;
    p.lifetime = 1.0;
    p.color = vec4<f32>(1.0);
    p.size = 0.1;
    p.angle = 0.0;
    p.seed = vfx_hash(vfx_rng ^ 0x68bc21ebu);
    p.alive = 1u;
    var direction = vec3<f32>(0.0, 1.0, 0.0);
    let dt = vfx_params.dt;
"#
    ));
    s.body(init);
    s.push(
        r#"
    if (vfx_params.world_space == 1u) {
        p.position = (vfx_params.world * vec4<f32>(p.position, 1.0)).xyz;
        p.velocity = (vfx_params.world * vec4<f32>(p.velocity, 0.0)).xyz;
    }
    p.alive = 1u;
    vfx_particles[index] = p;
    let slot = atomicAdd(&vfx_counters.alive[vfx_params.parity], 1u);
    vfx_alive_in[slot] = index;
    atomicAdd(&vfx_counters.spawned, 1u);
}

@compute @workgroup_size(1)
fn cs_args() {
    let n = atomicLoad(&vfx_counters.alive[vfx_params.parity]);
    vfx_indirect[0] = (n + 63u) / 64u;
    vfx_indirect[1] = 1u;
    vfx_indirect[2] = 1u;
    atomicStore(&vfx_counters.alive[1u - vfx_params.parity], 0u);
}

@compute @workgroup_size(64)
fn cs_update(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = atomicLoad(&vfx_counters.alive[vfx_params.parity]);
    if (gid.x >= n) {
        return;
    }
    let index = vfx_alive_in[gid.x];
    var p = vfx_particles[index];
    vfx_rng = vfx_hash(p.seed ^ vfx_hash(vfx_params.serial));
    let dt = vfx_params.dt;
    var kill = false;
"#,
    );
    s.body(update);
    let mut tail = String::new();
    if update_settings.integrate {
        tail.push_str("    p.position = p.position + p.velocity * dt;\n");
    }
    if update_settings.age {
        tail.push_str("    p.age = p.age + dt;\n");
    }
    let reap = if update_settings.reap {
        "kill || p.age >= p.lifetime"
    } else {
        "kill"
    };
    tail.push_str(&format!(
        r#"    if ({reap}) {{
        p.alive = 0u;
        vfx_particles[index] = p;
        let d = atomicAdd(&vfx_counters.dead, 1);
        vfx_dead[u32(d)] = index;
        atomicAdd(&vfx_counters.reaped, 1u);
        return;
    }}
    vfx_particles[index] = p;
    let slot = atomicAdd(&vfx_counters.alive[1u - vfx_params.parity], 1u);
    vfx_alive_out[slot] = index;
}}

@compute @workgroup_size(1)
fn cs_finish() {{
    let n = atomicLoad(&vfx_counters.alive[1u - vfx_params.parity]);
    vfx_counters.last_alive = n;
"#
    ));
    // Indirect draw arguments per output: [vertex_count, instance_count, first, first_instance]
    // at 16 + 32 × k bytes.
    for (k, verts) in outputs {
        let base = 4 + 8 * k;
        tail.push_str(&format!(
            "    vfx_indirect[{base}u] = {verts}u;\n    vfx_indirect[{}u] = n;\n    vfx_indirect[{}u] = 0u;\n    vfx_indirect[{}u] = 0u;\n",
            base + 1,
            base + 2,
            base + 3
        ));
    }
    tail.push_str("}\n");
    s.push(&tail);
    s
}

fn render_shader(body: &[(String, Option<u32>)], settings: &OutputSettings) -> Source {
    let mut s = Source::new();
    s.push(RENDER_BINDINGS);
    s.push(COMMON);
    s.push(
        r#"
struct VfxOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
};

struct VfxTargets {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VfxOut {
    var p = vfx_particles[vfx_alive[ii]];
    vfx_rng = vfx_hash(p.seed ^ 0x2545f491u);
    let dt = vfx_params.dt;
    var orient = 0u;
    var stretch = 0.0;
"#,
    );
    s.body(body);
    let shape = match settings.kind {
        OutputKind::Quad => {
            r#"
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi % 6u];
    var ax = normalize((view.inv_view_proj * vec4<f32>(1.0, 0.0, 0.0, 0.0)).xyz);
    var ay = normalize((view.inv_view_proj * vec4<f32>(0.0, 1.0, 0.0, 0.0)).xyz);
    var half_y = p.size * 0.5;
    let speed = length(vel);
    if (orient == 1u && speed > 0.0001) {
        ay = vel / speed;
        ax = vfx_safe_normalize(cross(ay, view.camera_pos.xyz - center));
        half_y = half_y + speed * stretch * 0.5;
    } else if (orient == 2u) {
        ax = vec3<f32>(1.0, 0.0, 0.0);
        ay = vec3<f32>(0.0, 0.0, -1.0);
    }
    let a = radians(p.angle);
    let rx = ax * cos(a) + ay * sin(a);
    let ry = ay * cos(a) - ax * sin(a);
    let world = center + rx * (c.x * p.size * 0.5) + ry * (c.y * half_y);
    var uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
"#
        }
        OutputKind::Point => {
            r#"
    let world = center;
    var uv = vec2<f32>(0.5, 0.5);
"#
        }
        OutputKind::Line => {
            r#"
    let world = center - vel * vfx_params.misc.x * f32(vi % 2u);
    var uv = vec2<f32>(0.5, f32(vi % 2u));
"#
        }
    };
    s.push(
        r#"
    var center = p.position;
    var vel = p.velocity;
    if (vfx_params.world_space == 0u) {
        center = (vfx_params.world * vec4<f32>(center, 1.0)).xyz;
        vel = (vfx_params.world * vec4<f32>(vel, 0.0)).xyz;
    }
"#,
    );
    s.push(shape);
    s.push(
        r#"
    // Flipbook: pick the frame and squeeze the UVs into its cell.
    let cols = max(vfx_params.flipbook.x, 1.0);
    let rows = max(vfx_params.flipbook.y, 1.0);
    let frames = cols * rows;
    if (frames > 1.0) {
        var frame = floor(vfx_life(p) * frames);
        if (vfx_params.flipbook.z > 0.5) {
            frame = floor(p.age * vfx_params.flipbook.w) % frames;
        }
        frame = min(frame, frames - 1.0);
        let cell = vec2<f32>(frame % cols, floor(frame / cols));
        uv = (cell + uv) / vec2<f32>(cols, rows);
    }
    var out: VfxOut;
    out.pos = view.view_proj * vec4<f32>(world, 1.0);
    out.uv = uv;
    out.color = p.color;
    out.world = world;
    if (p.size <= 0.0 && vfx_params.misc.z < 0.5) {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    return out;
}

// Fade near opaque geometry: 0 where the particle meets it, 1 a soft distance in front.
fn vfx_soft(in: VfxOut) -> f32 {
    if (vfx_params.misc.y <= 0.0) {
        return 1.0;
    }
    let size = vec2<f32>(textureDimensions(vfx_scene_depth));
    let px = vec2<i32>(clamp(in.pos.xy, vec2<f32>(0.0), size - 1.0));
    let d = textureLoad(vfx_scene_depth, px, 0).r;
    if (d >= 1.0) {
        return 1.0;
    }
    let ndc = vec2<f32>(in.pos.x / size.x * 2.0 - 1.0, 1.0 - in.pos.y / size.y * 2.0);
    let cam = view.camera_pos.xyz;
    let gap = distance(unproject(ndc, d), cam) - distance(in.world, cam);
    return clamp(gap / vfx_params.misc.y, 0.0, 1.0);
}

fn vfx_shade(in: VfxOut) -> VfxTargets {
    var shape = vec4<f32>(1.0);
    if (vfx_params.textured == 1u) {
        shape = textureSample(vfx_tex, vfx_sampler, in.uv);
    } else if (vfx_params.misc.z < 0.5) {
        let r = length(in.uv * 2.0 - 1.0);
        let f = clamp(1.0 - r, 0.0, 1.0);
        shape = vec4<f32>(1.0, 1.0, 1.0, f * f * (3.0 - 2.0 * f));
    }
    let c = vec4<f32>(in.color.rgb * shape.rgb, in.color.a * shape.a * vfx_soft(in));
    let bright = max(max(c.r, c.g), c.b);
    var out: VfxTargets;
    out.color = c;
    out.glow = vec4<f32>(c.rgb * clamp(bright - 1.0, 0.0, 1.0), c.a);
    return out;
}
"#,
    );
    let frag = match settings.blend {
        BlendMode::Additive => {
            r#"
@fragment
fn fs_main(in: VfxOut) -> VfxTargets {
    var out = vfx_shade(in);
    let fog = fog_visibility(fog_distance(in.world)) * vfog_glow(vfog_eval(in.world));
    out.color = vec4<f32>(out.color.rgb * fog, out.color.a);
    out.glow = vec4<f32>(out.glow.rgb * fog, out.glow.a);
    return out;
}
"#
        }
        BlendMode::Alpha => {
            r#"
@fragment
fn fs_main(in: VfxOut) -> VfxTargets {
    var out = vfx_shade(in);
    let fog = fog_visibility(fog_distance(in.world));
    let vf = vfog_eval(in.world);
    out.color = vec4<f32>(vfog_blend(mix(view.fog_color.rgb, out.color.rgb, fog), vf, 1.0), out.color.a);
    out.glow = vec4<f32>(out.glow.rgb * fog * vfog_glow(vf), out.glow.a);
    return out;
}
"#
        }
        BlendMode::Opaque => {
            r#"
@fragment
fn fs_main(in: VfxOut) -> VfxTargets {
    var out = vfx_shade(in);
    if (out.color.a < 0.5) {
        discard;
    }
    let fog = fog_visibility(fog_distance(in.world));
    let vf = vfog_eval(in.world);
    out.color = vec4<f32>(vfog_blend(mix(view.fog_color.rgb, out.color.rgb, fog), vf, 1.0), 1.0);
    out.glow = vec4<f32>(out.glow.rgb * fog * vfog_glow(vf), 1.0);
    return out;
}
"#
        }
    };
    s.push(frag);
    s
}

/// The node a validation error's line came from.
fn owner_of(source: &Source, line: usize) -> Option<u32> {
    source.owners.get(line.saturating_sub(1)).copied().flatten()
}

/// Validate a compute shader with naga (render shaders are validated by the renderer, which
/// owns the view prelude).
fn validate_compute(src: &Source) -> Result<(), GraphError> {
    use wgpu::naga;
    let module = naga::front::wgsl::parse_str(&src.text).map_err(|e| {
        let line = e.location(&src.text).map_or(0, |l| l.line_number as usize);
        err(
            owner_of(src, line),
            format!("generated WGSL: {}", e.emit_to_string(&src.text)),
        )
    })?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| {
        let line = e
            .spans()
            .next()
            .map(|(s, _)| s.location(&src.text).line_number as usize)
            .unwrap_or(0);
        err(
            owner_of(src, line),
            format!("generated WGSL: {}", e.emit_to_string(&src.text)),
        )
    })?;
    Ok(())
}

/// Problems of the graph's structure (contexts, blocks, links), before any code.
pub fn check(g: &EffectGraph) -> Vec<GraphError> {
    let mut errors = Vec::new();
    for ty in [
        ContextType::Spawn,
        ContextType::Initialize,
        ContextType::Update,
    ] {
        let list: Vec<u32> = g
            .contexts
            .iter()
            .filter(|c| c.kind.ty() == ty)
            .map(|c| c.id)
            .collect();
        for extra in list.iter().skip(1) {
            errors.push(err(
                Some(*extra),
                format!("only one {} context per effect", ty.label()),
            ));
        }
    }
    if g.context_of(ContextType::Spawn).is_none() {
        errors.push(err(None, "add a Spawn context"));
    }
    if g.context_of(ContextType::Initialize).is_none() {
        errors.push(err(None, "add an Initialize Particle context"));
    }
    if g.context_of(ContextType::Output).is_none() {
        errors.push(err(None, "add an Output Particle context"));
    }
    for c in &g.contexts {
        let ty = c.kind.ty();
        for b in &c.blocks {
            if !b.kind.allowed_in(ty) {
                errors.push(err(
                    Some(b.id),
                    format!("{} doesn't belong in {}", b.kind.title(), ty.label()),
                ));
            }
        }
        match &c.kind {
            ContextKind::Initialize(s) if s.capacity == 0 || s.capacity > MAX_CAPACITY => {
                errors.push(err(
                    Some(c.id),
                    format!("Capacity must be between 1 and {MAX_CAPACITY}"),
                ));
            }
            ContextKind::Output(s) => {
                if let Some(name) = &s.texture_property
                    && !matches!(
                        g.property(name).map(|p| &p.value),
                        Some(PropertyValue::Texture(_))
                    )
                {
                    errors.push(err(Some(c.id), format!("{name} is not a Texture property")));
                }
            }
            _ => {}
        }
    }
    for l in &g.links {
        if g.operator(l.from).is_none() || g.find(l.to).is_none() {
            continue;
        }
        if let Some(Item::Block(ci, _)) = g.find(l.to)
            && g.contexts[ci].kind.ty() == ContextType::Spawn
            && !g.cpu_evaluable(l.from)
        {
            errors.push(err(
                Some(l.to),
                "Spawn blocks only take properties, constants and + − × ÷",
            ));
        }
    }
    errors
}

fn cpu_expr(g: &EffectGraph, id: u32, port: &str) -> CpuExpr {
    if let Some(l) = g.input_link(id, port) {
        return cpu_op(g, l.from);
    }
    let value = match g.find(id) {
        Some(Item::Block(ci, bi)) => {
            let b = &g.contexts[ci].blocks[bi];
            b.kind.input(port).map(|p| b.value(&p))
        }
        Some(Item::Operator(oi)) => {
            let o = &g.operators[oi];
            o.kind.input(port).map(|p| o.value(&p))
        }
        _ => None,
    };
    CpuExpr::Const(value.map_or(0.0, |v| v.as_float()))
}

fn cpu_op(g: &EffectGraph, op: u32) -> CpuExpr {
    let Some(o) = g.operator(op) else {
        return CpuExpr::Const(0.0);
    };
    let bin = |c: char| {
        CpuExpr::Binary(
            c,
            Box::new(cpu_expr(g, op, "A")),
            Box::new(cpu_expr(g, op, "B")),
        )
    };
    match &o.kind {
        OperatorKind::Property { name } => CpuExpr::Property(name.clone()),
        OperatorKind::Float => cpu_expr(g, op, "Value"),
        OperatorKind::Add => bin('+'),
        OperatorKind::Subtract => bin('-'),
        OperatorKind::Multiply => bin('*'),
        OperatorKind::Divide => bin('/'),
        _ => CpuExpr::Const(0.0),
    }
}

/// Compile a graph: structure checks, WGSL for the simulation and each output, the spawn
/// system. Errors carry the context, block or operator they belong to.
pub fn compile(g: &EffectGraph) -> Result<CompiledEffect, Vec<GraphError>> {
    let mut errors = check(g);
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut cg = Gen::new(g);
    let init_ctx = g.context_of(ContextType::Initialize).expect("checked");
    let ContextKind::Initialize(init) = &init_ctx.kind else {
        unreachable!("context_of returns the asked type")
    };
    cg.stage = Stage::Init;
    for b in &init_ctx.blocks {
        cg.block(b);
    }
    let init_body = cg.take_body();
    cg.stage = Stage::Update;
    let update_ctx = g.context_of(ContextType::Update);
    let update_settings = match update_ctx.map(|c| &c.kind) {
        Some(ContextKind::Update(s)) => s.clone(),
        _ => Default::default(),
    };
    if let Some(c) = update_ctx {
        for b in &c.blocks {
            cg.block(b);
        }
    }
    let update_body = cg.take_body();

    let mut outputs = Vec::new();
    let mut draws = Vec::new();
    cg.stage = Stage::Output;
    for c in g
        .contexts
        .iter()
        .filter(|c| c.kind.ty() == ContextType::Output)
    {
        let ContextKind::Output(settings) = &c.kind else {
            continue;
        };
        for b in &c.blocks {
            cg.block(b);
        }
        let body = cg.take_body();
        let vertex_count = match settings.kind {
            OutputKind::Quad => 6,
            OutputKind::Point => 1,
            OutputKind::Line => 2,
        };
        draws.push((outputs.len() as u32, vertex_count));
        let src = render_shader(&body, settings);
        outputs.push(CompiledOutput {
            source: src.text,
            settings: settings.clone(),
            vertex_count,
        });
    }
    let sim = sim_shader(&init_body, &update_body, &update_settings, &draws);
    errors.append(&mut cg.errors);
    if errors.is_empty()
        && let Err(e) = validate_compute(&sim)
    {
        errors.push(e);
    }
    if errors.is_empty() {
        for o in &outputs {
            if let Err(e) = crate::render::vfx::validate_output(&o.source) {
                errors.push(err(None, format!("output shader: {e}")));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let spawn = g
        .context_of(ContextType::Spawn)
        .map(|c| {
            c.blocks
                .iter()
                .filter(|b| b.enabled && b.kind.is_spawn())
                .map(|b| SpawnBlock {
                    kind: b.kind.clone(),
                    inputs: b
                        .kind
                        .inputs()
                        .iter()
                        .map(|p| (p.name, cpu_expr(g, b.id, p.name)))
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    let mut h = std::collections::hash_map::DefaultHasher::new();
    sim.text.hash(&mut h);
    for o in &outputs {
        o.source.hash(&mut h);
        format!("{:?}", o.settings).hash(&mut h);
    }
    init.capacity.hash(&mut h);
    Ok(CompiledEffect {
        hash: h.finish(),
        sim_source: sim.text,
        outputs,
        capacity: init.capacity,
        bounds_center: Vec3::from(init.bounds_center),
        bounds_size: Vec3::from(init.bounds_size),
        space: init.space,
        spawn,
        properties: g.properties.clone(),
        slots: cg.slots.clone(),
        ramps: cg.ramps.clone(),
    })
}

/// The properties uniform for an instance: defaults with `overrides` applied (linear colours).
pub fn property_data(
    effect: &CompiledEffect,
    overrides: &[(String, PropertyValue)],
) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0; 4]; MAX_PROPERTIES];
    for (name, slot) in &effect.slots {
        if *slot >= MAX_PROPERTIES {
            continue;
        }
        let default = effect.properties.iter().find(|p| &p.name == name);
        let value = overrides
            .iter()
            .find(|(n, v)| n == name && default.is_some_and(|d| d.value.same_type(v)))
            .map(|(_, v)| v)
            .or(default.map(|d| &d.value));
        out[*slot] = match value {
            Some(PropertyValue::Float(f)) => [*f, 0.0, 0.0, 0.0],
            Some(PropertyValue::Vector3(v)) => [v[0], v[1], v[2], 0.0],
            Some(PropertyValue::Color(c)) => color_to_linear(*c),
            _ => [0.0; 4],
        };
    }
    out
}

/// Baked ramp rows (`RAMP_SAMPLES` each) with gradient property overrides applied.
pub fn ramp_data(effect: &CompiledEffect, overrides: &[(String, PropertyValue)]) -> Vec<[f32; 4]> {
    let mut out = Vec::with_capacity(effect.ramps.len().max(1) * RAMP_SAMPLES);
    let bake_gradient = |g: &Gradient, out: &mut Vec<[f32; 4]>| {
        for i in 0..RAMP_SAMPLES {
            let t = i as f32 / (RAMP_SAMPLES - 1) as f32;
            out.push(color_to_linear(g.evaluate(t)));
        }
    };
    for r in &effect.ramps {
        match r {
            RampSource::Gradient(g) => bake_gradient(g, &mut out),
            RampSource::Curve(c) => {
                for i in 0..RAMP_SAMPLES {
                    let t = i as f32 / (RAMP_SAMPLES - 1) as f32;
                    out.push([c.evaluate(t), 0.0, 0.0, 0.0]);
                }
            }
            RampSource::Property(name) => {
                let g = overrides
                    .iter()
                    .find_map(|(n, v)| match v {
                        PropertyValue::Gradient(g) if n == name => Some(g),
                        _ => None,
                    })
                    .or_else(|| {
                        effect.properties.iter().find_map(|p| match &p.value {
                            PropertyValue::Gradient(g) if &p.name == name => Some(g),
                            _ => None,
                        })
                    })
                    .cloned()
                    .unwrap_or_default();
                bake_gradient(&g, &mut out);
            }
        }
    }
    if out.is_empty() {
        out.resize(RAMP_SAMPLES, [1.0; 4]);
    }
    out
}

/// The attribute types a Set Attribute block writes (for docs and tests).
pub fn attribute_type(a: Attribute) -> ValueType {
    a.ty()
}
