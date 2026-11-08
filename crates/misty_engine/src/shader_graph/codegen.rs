//! Graph → `.shader` text: a header declaring the Blackboard properties, keywords and render
//! state, the node helpers, Custom Function code, an optional `fn vertex` (Position Offset) and
//! the `fn surface` (Lit) or `fn fragment` (Unlit) that the custom-shader wrapper turns into a
//! pipeline. A Decal graph becomes a function the decal pass calls instead ([`generate_decal`]).
//!
//! Each node becomes `let n<id>_<output>` lines in dependency order. Sub Graph nodes are inlined
//! (their nodes prefixed with the Sub Graph node's id). Keyword nodes pass on the input of the
//! variant being generated, and only the nodes that variant reads are generated. Every generated
//! line remembers the node it came from (a Sub Graph's lines: the Sub Graph node), so a WGSL error
//! is reported on the offending node.
//!
//! **Custom Function contract.** Inline: the body is placed in
//! `fn sg_fn_<name>_<hash>(<inputs>) -> <struct of outputs>` with each output a `var` of its
//! type, so it reads inputs and assigns outputs by name (`Out = In * 2.0;`). File: the `.wgsl`
//! file is included once and `<name>(<inputs in order>)` is called; it returns the output's type
//! (one output) or a struct whose fields are named after the outputs.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::nodes::{FunctionSource, NodeKind, Port, PortDefault, Slot, Space, ValueType, master};
use super::{
    Keyword, KeywordKind, MASTER, MAX_VARIANTS, Node, NormalSpace, PropertyType, ShaderGraph,
    Surface, TargetMaterial, Types, Variant,
};

/// A problem with a graph, on a node when it can be pinned to one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphError {
    pub node: Option<u32>,
    pub message: String,
}

impl GraphError {
    fn on(node: u32, message: impl Into<String>) -> Self {
        Self {
            node: Some(node),
            message: message.into(),
        }
    }

    fn graph(message: impl Into<String>) -> Self {
        Self {
            node: None,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.node {
            Some(n) => write!(f, "node {n}: {}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// Reads the assets a graph refers to: Sub Graphs and Custom Function files.
pub trait Resolver {
    fn graph(&self, path: &str) -> Result<ShaderGraph, String>;
    fn text(&self, path: &str) -> Result<String, String>;
}

/// Reads project files (`crate::assets::absolute`).
pub struct FileResolver;

impl Resolver for FileResolver {
    fn graph(&self, path: &str) -> Result<ShaderGraph, String> {
        ShaderGraph::load(&crate::assets::absolute(path)).map_err(|e| format!("{path}: {e}"))
    }

    fn text(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(crate::assets::absolute(path)).map_err(|e| format!("{path}: {e}"))
    }
}

/// Generated shader text with the node behind each line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    pub source: String,
    /// Node of each line (index = line - 1).
    lines: Vec<Option<u32>>,
    /// Sub Graphs and Custom Function files read (hot reload follows them).
    pub deps: Vec<String>,
    /// Keywords of the shader: the graph's, then those only its Sub Graphs declare.
    pub keywords: Vec<Keyword>,
}

impl Generated {
    /// The node a 1-based line of `source` was generated for.
    pub fn node_at_line(&self, line: usize) -> Option<u32> {
        line.checked_sub(1)
            .and_then(|i| self.lines.get(i))
            .copied()
            .flatten()
    }
}

/// WGSL keywords and names of the prelude a property must not shadow.
const RESERVED: [&str; 34] = [
    "alias",
    "break",
    "case",
    "const",
    "continue",
    "default",
    "discard",
    "else",
    "enable",
    "false",
    "fn",
    "for",
    "if",
    "let",
    "loop",
    "override",
    "return",
    "struct",
    "switch",
    "true",
    "var",
    "while",
    "material",
    "globals",
    "view",
    "draws",
    "input",
    "tex_sampler",
    "f32",
    "i32",
    "u32",
    "bool",
    "scene_color",
    "scene_depth",
];

/// Is `name` usable as a WGSL identifier (and a property name)?
pub fn valid_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name != "_"
        && !name.starts_with("__")
        && !name.starts_with("sg_")
        && !RESERVED.contains(&name)
}

/// Which shader function a node is generated into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
}

/// A WGSL float literal.
pub fn lit(v: f32) -> String {
    if !v.is_finite() {
        return "0.0".into();
    }
    // The `f` suffix makes it a concrete f32 (naga only folds abstract floats in constants).
    let s = format!("{v:?}");
    if s.contains(['.', 'e', 'E']) {
        format!("{s}f")
    } else {
        format!("{s}.0f")
    }
}

/// A constant of type `ty` from the first components of `v`.
pub fn literal(v: [f32; 4], ty: ValueType) -> String {
    match ty.dims() {
        0 | 1 => lit(v[0]),
        n => {
            let parts: Vec<String> = v[..n].iter().map(|&c| lit(c)).collect();
            format!("{}({})", ty.wgsl(), parts.join(", "))
        }
    }
}

/// An expression of type `from` as type `to`: floats splat, wider vectors truncate, narrower ones
/// pad with zeros.
pub fn convert(expr: &str, from: ValueType, to: ValueType) -> String {
    if from == to || from == ValueType::Texture || to == ValueType::Texture {
        return expr.to_owned();
    }
    let (m, n) = (from.dims(), to.dims());
    if m == 1 {
        format!("{}({expr})", to.wgsl())
    } else if n == 1 {
        format!("({expr}).x")
    } else if m > n {
        format!("({expr}).{}", &"xyzw"[..n])
    } else {
        let zeros = vec!["0.0"; n - m].join(", ");
        format!("{}({expr}, {zeros})", to.wgsl())
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear RGB (alpha kept) of an sRGB colour.
pub fn linear_color(c: [f32; 4]) -> [f32; 4] {
    [
        srgb_to_linear(c[0]),
        srgb_to_linear(c[1]),
        srgb_to_linear(c[2]),
        c[3],
    ]
}

/// Functions the node code calls (always emitted; naga ignores the unused ones). They only use
/// WGSL builtins, so the decal pass includes them too.
pub const HELPERS: &str = r#"fn sg_hash_u32(x: u32) -> u32 {
    var h = x * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}
fn sg_hash22(p: vec2<f32>) -> vec2<f32> {
    let i = bitcast<vec2<u32>>(vec2<i32>(floor(p)));
    let a = sg_hash_u32(i.x ^ sg_hash_u32(i.y));
    let b = sg_hash_u32(a ^ 2654435769u);
    return vec2<f32>(f32(a & 65535u), f32(b & 65535u)) / 65535.0;
}
fn sg_gradient_dir(p: vec2<f32>) -> vec2<f32> {
    let a = sg_hash22(p).x * 6.2831853;
    return vec2<f32>(cos(a), sin(a));
}
fn sg_gradient_noise(uv: vec2<f32>, scale: f32) -> f32 {
    let p = uv * scale;
    let ip = floor(p);
    let fp = p - ip;
    let d00 = dot(sg_gradient_dir(ip), fp);
    let d01 = dot(sg_gradient_dir(ip + vec2<f32>(0.0, 1.0)), fp - vec2<f32>(0.0, 1.0));
    let d10 = dot(sg_gradient_dir(ip + vec2<f32>(1.0, 0.0)), fp - vec2<f32>(1.0, 0.0));
    let d11 = dot(sg_gradient_dir(ip + vec2<f32>(1.0, 1.0)), fp - vec2<f32>(1.0, 1.0));
    let f = fp * fp * fp * (fp * (fp * 6.0 - 15.0) + 10.0);
    return mix(mix(d00, d01, f.y), mix(d10, d11, f.y), f.x) + 0.5;
}
fn sg_voronoi(uv: vec2<f32>, angle: f32, density: f32) -> vec2<f32> {
    let p = uv * density;
    let ip = floor(p);
    let fp = p - ip;
    var best = 8.0;
    var cell = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let o = vec2<f32>(f32(x), f32(y));
            let h = sg_hash22(ip + o);
            let d = length(o + 0.5 + 0.5 * sin(h * 6.2831853 + vec2<f32>(angle)) - fp);
            if (d < best) {
                best = d;
                cell = h.x;
            }
        }
    }
    return vec2<f32>(best, cell);
}
fn sg_checker(uv: vec2<f32>, a: vec3<f32>, b: vec3<f32>, freq: vec2<f32>) -> vec3<f32> {
    let c = floor(uv * freq);
    let s = c.x + c.y;
    return mix(a, b, s - 2.0 * floor(s * 0.5));
}
fn sg_triplanar_weights(n: vec3<f32>, blend: f32) -> vec3<f32> {
    let w = pow(abs(n), vec3<f32>(max(blend, 0.01)));
    return w / max(w.x + w.y + w.z, 1e-6);
}
fn sg_object_pos(m: mat4x4<f32>, world: vec3<f32>) -> vec3<f32> {
    let d = world - m[3].xyz;
    return vec3<f32>(
        dot(d, m[0].xyz) / max(dot(m[0].xyz, m[0].xyz), 1e-12),
        dot(d, m[1].xyz) / max(dot(m[1].xyz, m[1].xyz), 1e-12),
        dot(d, m[2].xyz) / max(dot(m[2].xyz, m[2].xyz), 1e-12));
}
fn sg_object_dir(m: mat4x4<f32>, dir: vec3<f32>) -> vec3<f32> {
    let r = vec3<f32>(dot(dir, m[0].xyz), dot(dir, m[1].xyz), dot(dir, m[2].xyz));
    return r * inverseSqrt(max(dot(r, r), 1e-12));
}
"#;

/// Helpers that read the mesh pipeline's draws and view (not in the decal pass).
const MESH_HELPERS: &str = r#"fn sg_vertex_world_pos(v: VertexData) -> vec3<f32> {
    return (draws[v.draw].model * vec4<f32>(v.position, 1.0)).xyz;
}
fn sg_vertex_world_normal(v: VertexData) -> vec3<f32> {
    return normalize((draws[v.draw].normal * vec4<f32>(v.normal, 0.0)).xyz);
}
fn sg_vertex_view_dir(v: VertexData) -> vec3<f32> {
    return normalize(view.camera_pos.xyz - sg_vertex_world_pos(v));
}
fn sg_vertex_screen(v: VertexData) -> vec4<f32> {
    let c = view.view_proj * vec4<f32>(sg_vertex_world_pos(v), 1.0);
    let n = c.xyz / max(c.w, 1e-6);
    return vec4<f32>(n.x * 0.5 + 0.5, 0.5 - n.y * 0.5, n.z, 1.0);
}
fn sg_object_normal_to_world(input: SurfaceInput, n: vec3<f32>) -> vec3<f32> {
    return normalize((draws[input.draw].normal * vec4<f32>(n, 0.0)).xyz);
}
"#;

#[derive(Default)]
struct Out {
    lines: Vec<(String, Option<u32>)>,
}

impl Out {
    fn push(&mut self, line: impl Into<String>, node: Option<u32>) {
        let line = line.into();
        for l in line.lines() {
            self.lines.push((l.to_owned(), node));
        }
    }
}

/// What every level of a generation shares.
struct Ctx<'a> {
    resolver: &'a dyn Resolver,
    variant: &'a Variant,
    /// Generating a decal function (properties come from `input.params`).
    decal: bool,
    /// Decal: the parameter slot of each top-level property.
    decal_slots: HashMap<String, usize>,
}

struct Gen<'a> {
    g: &'a ShaderGraph,
    types: Types,
    stage: Stage,
    /// Output variables generated in the current function: (node, output) → (expr, type).
    vars: HashMap<(u32, String), (String, ValueType)>,
    out: Out,
    /// Prefix of variable names (a Sub Graph's nodes: `n<sub node id>_`).
    prefix: String,
    /// Inside a Sub Graph: the top-level node every line belongs to.
    blame: Option<u32>,
    /// Inside a Sub Graph: its properties' values (expressions of the parent's inputs).
    bindings: Option<HashMap<String, (String, ValueType)>>,
    /// Sub Graph paths being expanded (a graph inside itself is an error).
    stack: Vec<String>,
    ctx: &'a Ctx<'a>,
}

/// Nodes feeding the given ports of the master stack in a variant (a Keyword node only follows
/// the input the variant selects).
fn needed(g: &ShaderGraph, ports: &[&str], variant: &Variant) -> HashSet<u32> {
    let mut stack: Vec<u32> = g
        .connections
        .iter()
        .filter(|c| c.to == MASTER && ports.contains(&c.input.as_str()))
        .map(|c| c.from)
        .collect();
    let mut seen = HashSet::new();
    while let Some(n) = stack.pop() {
        if !seen.insert(n) {
            continue;
        }
        let selected = match g.node(n).map(|x| &x.kind) {
            Some(NodeKind::Keyword { name }) => g
                .keyword(name)
                .and_then(|k| k.port_for(keyword_value(k, variant))),
            _ => None,
        };
        let is_keyword = matches!(g.node(n).map(|x| &x.kind), Some(NodeKind::Keyword { .. }));
        stack.extend(
            g.connections
                .iter()
                .filter(|c| c.to == n && (!is_keyword || Some(c.input.as_str()) == selected))
                .map(|c| c.from),
        );
    }
    seen
}

/// A keyword's value in a variant (its default when the variant leaves it out).
fn keyword_value(k: &Keyword, variant: &Variant) -> u32 {
    variant
        .get(&k.name)
        .copied()
        .unwrap_or(k.default)
        .min(k.count() as u32 - 1)
}

/// Name of an inline Custom Function's generated function (identical code shares one).
fn inline_fn_name(function: &str, body: &str, inputs: &[Slot], outputs: &[Slot]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (function, body).hash(&mut h);
    for s in inputs.iter().chain(outputs) {
        (s.name.as_str(), s.ty).hash(&mut h);
    }
    let safe: String = function
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    format!("sg_fn_{safe}_{:08x}", h.finish() as u32)
}

impl<'a> Gen<'a> {
    fn new(g: &'a ShaderGraph, stage: Stage, ctx: &'a Ctx<'a>) -> Self {
        Self {
            g,
            types: g.types(),
            stage,
            vars: HashMap::new(),
            out: Out::default(),
            prefix: String::new(),
            blame: None,
            bindings: None,
            stack: Vec::new(),
            ctx,
        }
    }

    /// The node a line is reported on.
    fn who(&self, node: u32) -> u32 {
        self.blame.unwrap_or(node)
    }

    fn err(&self, node: u32, message: impl Into<String>) -> GraphError {
        GraphError::on(self.who(node), message)
    }

    fn emit(&mut self, line: impl Into<String>, node: Option<u32>) {
        let node = node.map(|n| self.who(n)).or(self.blame);
        self.out.push(line, node);
    }

    fn binding(&self, d: PortDefault) -> (String, ValueType) {
        let frag = self.stage == Stage::Fragment;
        let (e, ty) = match d {
            PortDefault::Uv => (if frag { "input.uv" } else { "v.uv" }, ValueType::Vec2),
            PortDefault::WorldNormal => (
                if frag {
                    "input.world_normal"
                } else {
                    "sg_vertex_world_normal(v)"
                },
                ValueType::Vec3,
            ),
            PortDefault::ViewDir => (
                if frag {
                    "input.view_dir"
                } else {
                    "sg_vertex_view_dir(v)"
                },
                ValueType::Vec3,
            ),
            PortDefault::WorldPos => (
                if frag {
                    "input.world_pos"
                } else {
                    "sg_vertex_world_pos(v)"
                },
                ValueType::Vec3,
            ),
            PortDefault::Value(_) | PortDefault::None => ("0.0", ValueType::Float),
        };
        (e.to_owned(), ty)
    }

    /// Expression of an input as type `want`.
    fn input(&self, node: &Node, port: &Port, want: ValueType) -> Result<String, GraphError> {
        if let Some(c) = self.g.input_connection(node.id, port.name)
            && let Some((expr, ty)) = self.vars.get(&(c.from, c.output.clone()))
        {
            return Ok(convert(expr, *ty, want));
        }
        match port.default {
            PortDefault::Value(_) => Ok(literal(node.value(port), want)),
            PortDefault::None => Err(self.err(
                node.id,
                format!("connect a Texture2D property to {}", port.name),
            )),
            d => {
                let (e, ty) = self.binding(d);
                Ok(convert(&e, ty, want))
            }
        }
    }

    fn model(&self) -> Result<&'static str, String> {
        if self.ctx.decal {
            return Err("object space is not available in a Decal graph".into());
        }
        Ok(match self.stage {
            Stage::Fragment => "draws[input.draw].model",
            Stage::Vertex => "draws[v.draw].model",
        })
    }

    fn sample(&self, tex: &str, uv: &str) -> String {
        if self.ctx.decal {
            return format!("sg_decal_texture({tex}, {uv}, input.lod)");
        }
        match self.stage {
            Stage::Fragment => format!("textureSample({tex}, tex_sampler, {uv})"),
            Stage::Vertex => format!("textureSampleLevel({tex}, tex_sampler, {uv}, 0.0)"),
        }
    }

    /// Generate one node's lines and register its outputs.
    fn node(&mut self, node: &Node) -> Result<(), GraphError> {
        use NodeKind as K;
        let id = node.id;
        let dynamic = self.types.dynamic(id);
        match &node.kind {
            K::Master => return Ok(()),
            K::Keyword { name } => return self.keyword_node(node, name),
            K::SubGraph { path, .. } => return self.sub_graph_node(node, path),
            _ => {}
        }
        let mut ins: HashMap<&str, String> = HashMap::new();
        for p in self.g.inputs(node) {
            let want = p.ty.resolve(dynamic);
            ins.insert(p.name, self.input(node, &p, want)?);
        }
        let i = |name: &str| ins.get(name).cloned().unwrap_or_else(|| "0.0".into());
        let frag = self.stage == Stage::Fragment;
        let decal = self.ctx.decal;
        let mut pre: Vec<String> = Vec::new();
        let prefix = self.prefix.clone();
        let v = |n: &str| format!("{prefix}n{id}_{n}");
        let model = || self.model().map_err(|e| self.err(id, e));
        let outs: Vec<String> = match &node.kind {
            K::Master | K::Keyword { .. } | K::SubGraph { .. } => unreachable!(),
            K::Property { name } => {
                if let Some(b) = self.bindings.as_ref().and_then(|b| b.get(name)) {
                    vec![b.0.clone()]
                } else {
                    let Some(p) = self.g.property(name) else {
                        return Err(self.err(id, format!("no property named {name}")));
                    };
                    if decal {
                        let slot = self.ctx.decal_slots.get(name).copied().unwrap_or(0);
                        match p.ty {
                            PropertyType::Texture2D { .. } | PropertyType::Color => {
                                vec![format!("input.params[{slot}]")]
                            }
                            PropertyType::Vector => vec![format!("input.params[{slot}]")],
                            _ => vec![format!("input.params[{slot}].x")],
                        }
                    } else {
                        match p.ty {
                            PropertyType::Texture2D { .. } => vec![name.clone()],
                            _ => vec![format!("material.{name}")],
                        }
                    }
                }
            }
            K::Color { value } => vec![literal(linear_color(*value), ValueType::Vec4)],
            K::Float => vec![i("X")],
            K::Vector2 => vec![format!("vec2<f32>({}, {})", i("X"), i("Y"))],
            K::Vector3 => vec![format!("vec3<f32>({}, {}, {})", i("X"), i("Y"), i("Z"))],
            K::Vector4 => vec![format!(
                "vec4<f32>({}, {}, {}, {})",
                i("X"),
                i("Y"),
                i("Z"),
                i("W")
            )],
            K::Time => {
                if decal {
                    vec![
                        "input.time".into(),
                        "sin(input.time)".into(),
                        "cos(input.time)".into(),
                        "0.0166f".into(),
                    ]
                } else {
                    ["x", "y", "z", "w"]
                        .iter()
                        .map(|c| format!("globals.time.{c}"))
                        .collect()
                }
            }
            K::Uv => vec![self.binding(PortDefault::Uv).0],
            K::Position { space } => {
                let world = self.binding(PortDefault::WorldPos).0;
                vec![match (space, frag) {
                    (Space::World, _) => world,
                    (Space::Object, true) => format!("sg_object_pos({}, {world})", model()?),
                    (Space::Object, false) => "v.position".into(),
                }]
            }
            K::Normal { space } => {
                let world = self.binding(PortDefault::WorldNormal).0;
                vec![match (space, frag) {
                    (Space::World, _) => world,
                    (Space::Object, true) => format!("sg_object_dir({}, {world})", model()?),
                    (Space::Object, false) => "v.normal".into(),
                }]
            }
            K::ViewDirection { space } => {
                let world = self.binding(PortDefault::ViewDir).0;
                vec![match space {
                    Space::World => world,
                    Space::Object => format!("sg_object_dir({}, {world})", model()?),
                }]
            }
            K::VertexColor => vec![if decal {
                "vec4<f32>(1.0)".into()
            } else if frag {
                "input.vertex_color".into()
            } else {
                "v.color".into()
            }],
            K::ScreenPosition => vec![if decal {
                "vec4<f32>(input.screen_uv, 0.0, 1.0)".into()
            } else if frag {
                "vec4<f32>(input.screen_uv, input.depth, 1.0)".into()
            } else {
                "sg_vertex_screen(v)".into()
            }],
            K::Add => vec![format!("({} + {})", i("A"), i("B"))],
            K::Subtract => vec![format!("({} - {})", i("A"), i("B"))],
            K::Multiply => vec![format!("({} * {})", i("A"), i("B"))],
            K::Divide => vec![format!("({} / {})", i("A"), i("B"))],
            K::Power => vec![format!("pow({}, {})", i("A"), i("B"))],
            K::Minimum => vec![format!("min({}, {})", i("A"), i("B"))],
            K::Maximum => vec![format!("max({}, {})", i("A"), i("B"))],
            K::Lerp => vec![format!("mix({}, {}, {})", i("A"), i("B"), i("T"))],
            K::Clamp => vec![format!("clamp({}, {}, {})", i("In"), i("Min"), i("Max"))],
            K::Saturate => vec![format!("saturate({})", i("In"))],
            K::OneMinus => vec![format!("(1.0 - {})", i("In"))],
            K::Negate => vec![format!("(-{})", i("In"))],
            K::Remap => {
                pre.push(format!("let {} = {};", v("im"), i("In Min Max")));
                pre.push(format!("let {} = {};", v("om"), i("Out Min Max")));
                let (im, om) = (v("im"), v("om"));
                vec![format!(
                    "({om}.x + ({} - {im}.x) * ({om}.y - {om}.x) / ({im}.y - {im}.x))",
                    i("In")
                )]
            }
            K::Step => vec![format!("step({}, {})", i("Edge"), i("In"))],
            K::Smoothstep => vec![format!(
                "smoothstep({}, {}, {})",
                i("Edge1"),
                i("Edge2"),
                i("In")
            )],
            K::Sine => vec![format!("sin({})", i("In"))],
            K::Cosine => vec![format!("cos({})", i("In"))],
            K::Absolute => vec![format!("abs({})", i("In"))],
            K::Floor => vec![format!("floor({})", i("In"))],
            K::Fraction => vec![format!("fract({})", i("In"))],
            K::Dot => vec![if dynamic == ValueType::Float {
                format!("({} * {})", i("A"), i("B"))
            } else {
                format!("dot({}, {})", i("A"), i("B"))
            }],
            K::Cross => vec![format!("cross({}, {})", i("A"), i("B"))],
            K::Normalize => vec![if dynamic == ValueType::Float {
                format!("sign({})", i("In"))
            } else {
                format!("normalize({})", i("In"))
            }],
            K::Length => vec![if dynamic == ValueType::Float {
                format!("abs({})", i("In"))
            } else {
                format!("length({})", i("In"))
            }],
            K::Split => {
                let whole = convert(&i("In"), dynamic, ValueType::Vec4);
                pre.push(format!("let {}: vec4<f32> = {whole};", v("in")));
                ["x", "y", "z", "w"]
                    .iter()
                    .map(|c| format!("{}.{c}", v("in")))
                    .collect()
            }
            K::Combine => {
                let (r, g, b, a) = (i("R"), i("G"), i("B"), i("A"));
                vec![
                    format!("vec4<f32>({r}, {g}, {b}, {a})"),
                    format!("vec3<f32>({r}, {g}, {b})"),
                    format!("vec2<f32>({r}, {g})"),
                ]
            }
            K::SampleTexture2D { normal_map } => {
                let s = self.sample(&i("Texture"), &i("UV"));
                let s = if *normal_map {
                    format!("vec4<f32>(unpack_normal({s}, 1.0), 1.0)")
                } else {
                    s
                };
                pre.push(format!("let {}: vec4<f32> = {s};", v("s")));
                let s = v("s");
                vec![
                    s.clone(),
                    format!("{s}.x"),
                    format!("{s}.y"),
                    format!("{s}.z"),
                    format!("{s}.w"),
                ]
            }
            K::TilingAndOffset => {
                vec![format!("({} * {} + {})", i("UV"), i("Tiling"), i("Offset"))]
            }
            K::NormalFromTexture => {
                let (t, uv) = (i("Texture"), i("UV"));
                pre.push(format!("let {} = {uv};", v("uv")));
                pre.push(format!("let {} = pow({}, 3.0) * 0.1;", v("o"), i("Offset")));
                let (uvn, o) = (v("uv"), v("o"));
                pre.push(format!("let {} = {}.x;", v("h0"), self.sample(&t, &uvn)));
                pre.push(format!(
                    "let {} = {}.x;",
                    v("h1"),
                    self.sample(&t, &format!("{uvn} + vec2<f32>({o}, 0.0)"))
                ));
                pre.push(format!(
                    "let {} = {}.x;",
                    v("h2"),
                    self.sample(&t, &format!("{uvn} + vec2<f32>(0.0, {o})"))
                ));
                let s = i("Strength");
                vec![format!(
                    "normalize(vec3<f32>(({h0} - {h1}) * {s}, ({h0} - {h2}) * {s}, 1.0))",
                    h0 = v("h0"),
                    h1 = v("h1"),
                    h2 = v("h2"),
                )]
            }
            K::NormalStrength => {
                pre.push(format!("let {} = {};", v("n"), i("In")));
                pre.push(format!("let {} = {};", v("k"), i("Strength")));
                let (n, k) = (v("n"), v("k"));
                vec![format!(
                    "normalize(vec3<f32>({n}.xy * {k}, mix(1.0, {n}.z, saturate({k}))))"
                )]
            }
            K::Triplanar { space } => {
                let t = i("Texture");
                let mut pos = i("Position");
                let mut nrm = i("Normal");
                if *space == Space::Object {
                    let m = model()?;
                    if self.g.input_connection(id, "Position").is_none() {
                        pos = format!("sg_object_pos({m}, {pos})");
                    }
                    if self.g.input_connection(id, "Normal").is_none() {
                        nrm = format!("sg_object_dir({m}, {nrm})");
                    }
                }
                pre.push(format!("let {} = {pos} * {};", v("p"), i("Tile")));
                pre.push(format!(
                    "let {} = sg_triplanar_weights({nrm}, {});",
                    v("w"),
                    i("Blend")
                ));
                let p = v("p");
                for (axis, uv) in [("x", "zy"), ("y", "xz"), ("z", "xy")] {
                    pre.push(format!(
                        "let {}: vec4<f32> = {};",
                        v(&format!("t{axis}")),
                        self.sample(&t, &format!("{p}.{uv}"))
                    ));
                }
                vec![format!(
                    "({tx} * {w}.x + {ty} * {w}.y + {tz} * {w}.z)",
                    tx = v("tx"),
                    ty = v("ty"),
                    tz = v("tz"),
                    w = v("w"),
                )]
            }
            K::GradientNoise => vec![format!("sg_gradient_noise({}, {})", i("UV"), i("Scale"))],
            K::Voronoi => {
                pre.push(format!(
                    "let {} = sg_voronoi({}, {}, {});",
                    v("v"),
                    i("UV"),
                    i("Angle Offset"),
                    i("Cell Density")
                ));
                vec![format!("{}.x", v("v")), format!("{}.y", v("v"))]
            }
            K::Checkerboard => vec![format!(
                "sg_checker({}, {}, {}, {})",
                i("UV"),
                i("Color A"),
                i("Color B"),
                i("Frequency")
            )],
            K::Fresnel => vec![format!(
                "pow(1.0 - saturate(dot(normalize({}), normalize({}))), {})",
                i("Normal"),
                i("View Dir"),
                i("Power")
            )],
            K::Preview => vec![i("In")],
            K::CustomFunction {
                function,
                source,
                body,
                inputs,
                outputs,
                ..
            } => {
                if decal && inputs.iter().any(|s| s.ty == ValueType::Texture) {
                    return Err(self.err(
                        id,
                        "texture inputs of a Custom Function are not available in a Decal graph",
                    ));
                }
                let args: Vec<String> = inputs.iter().map(|s| i(&s.name)).collect();
                let name = match source {
                    FunctionSource::Inline => inline_fn_name(function, body, inputs, outputs),
                    FunctionSource::File => function.clone(),
                };
                let call = format!("{name}({})", args.join(", "));
                if *source == FunctionSource::File && outputs.len() == 1 {
                    vec![call]
                } else {
                    pre.push(format!("let {} = {call};", v("r")));
                    outputs
                        .iter()
                        .map(|s| format!("{}.{}", v("r"), s.name))
                        .collect()
                }
            }
        };
        for line in pre {
            self.emit(format!("    {line}"), Some(id));
        }
        self.register(node, outs)
    }

    /// Bind a node's output expressions to `let` variables.
    fn register(&mut self, node: &Node, outs: Vec<String>) -> Result<(), GraphError> {
        let id = node.id;
        let dynamic = self.types.dynamic(id);
        for (k, (port, expr)) in self.g.outputs(node).iter().zip(outs).enumerate() {
            let ty = self
                .types
                .output(id, port.name)
                .unwrap_or(port.ty.resolve(dynamic));
            if ty == ValueType::Texture {
                self.vars.insert((id, port.name.to_owned()), (expr, ty));
                continue;
            }
            let name = format!("{}n{id}_{k}", self.prefix);
            self.emit(format!("    let {name}: {} = {expr};", ty.wgsl()), Some(id));
            self.vars.insert((id, port.name.to_owned()), (name, ty));
        }
        Ok(())
    }

    /// A Keyword node: the input the variant selects.
    fn keyword_node(&mut self, node: &Node, name: &str) -> Result<(), GraphError> {
        let Some(k) = self.g.keyword(name) else {
            return Err(self.err(node.id, format!("no keyword named {name}")));
        };
        let value = keyword_value(k, self.ctx.variant);
        let Some(port) = k.port_for(value).and_then(|p| self.g.input_port(node, p)) else {
            return Err(self.err(node.id, format!("{name} has no entries")));
        };
        let want = port.ty.resolve(self.types.dynamic(node.id));
        let expr = self.input(node, &port, want)?;
        self.register(node, vec![expr])
    }

    /// A Sub Graph node: its graph inlined with its properties bound to this node's inputs.
    fn sub_graph_node(&mut self, node: &Node, path: &str) -> Result<(), GraphError> {
        let id = node.id;
        if self.stack.iter().any(|p| p == path) || self.stack.len() > 8 {
            return Err(self.err(id, format!("{path} uses itself")));
        }
        let sub = self
            .ctx
            .resolver
            .graph(path)
            .map_err(|e| self.err(id, format!("cannot read the Sub Graph: {e}")))?;
        if !sub.is_sub_graph() {
            return Err(self.err(id, format!("{path} is not a Sub Graph")));
        }
        let name = node.kind.title();
        let (order, types) =
            check(&sub).map_err(|e| self.err(id, format!("in {name}: {}", e.message)))?;
        // The parent's inputs by the Sub Graph's property display names.
        let mut bindings = HashMap::new();
        let (slots, _) = sub.sub_graph_interface();
        for (p, slot) in sub.properties.iter().zip(&slots) {
            let port = slot.input_port();
            let expr = self.input(node, &port, slot.ty)?;
            bindings.insert(p.name.clone(), (expr, slot.ty));
        }
        let mut inner = Gen {
            g: &sub,
            types,
            stage: self.stage,
            vars: HashMap::new(),
            out: std::mem::take(&mut self.out),
            prefix: format!("{}n{id}_", self.prefix),
            blame: Some(self.who(id)),
            bindings: Some(bindings),
            stack: self
                .stack
                .iter()
                .cloned()
                .chain([path.to_owned()])
                .collect(),
            ctx: self.ctx,
        };
        let ports: Vec<&str> = sub.outputs.iter().map(|s| s.name.as_str()).collect();
        let result = (|| {
            let want = needed(&sub, &ports, self.ctx.variant);
            for nid in &order {
                if want.contains(nid)
                    && let Some(n) = sub.node(*nid)
                {
                    inner.node(n).map_err(|e| GraphError {
                        node: e.node,
                        message: if e.message.starts_with("in ") {
                            e.message
                        } else {
                            format!("in {name}: {}", e.message)
                        },
                    })?;
                }
            }
            sub.outputs
                .iter()
                .map(|s| inner.master_input(&s.name, s.ty))
                .collect::<Result<Vec<String>, GraphError>>()
        })();
        self.out = std::mem::take(&mut inner.out);
        let outs = result?;
        self.register(node, outs)
    }

    /// Generate the nodes feeding `ports` of the master stack, in order.
    fn nodes_for(&mut self, order: &[u32], ports: &[&str]) -> Result<(), GraphError> {
        let want = needed(self.g, ports, self.ctx.variant);
        let g = self.g;
        for id in order {
            if want.contains(id)
                && let Some(n) = g.node(*id)
            {
                self.node(n)?;
            }
        }
        Ok(())
    }

    /// A master-stack input as type `ty`.
    fn master_input(&self, name: &str, ty: ValueType) -> Result<String, GraphError> {
        let m = self
            .g
            .node(MASTER)
            .ok_or_else(|| GraphError::graph("the graph has no master stack"))?;
        let port = self
            .g
            .input_port(m, name)
            .ok_or_else(|| GraphError::graph(format!("no master port {name}")))?;
        self.input(m, &port, ty)
    }
}

/// Header line for one property.
fn property_line(p: &super::GraphProperty) -> String {
    let display = p.display.replace('"', "'").replace("//", "/");
    let v = p.value;
    let ty = match p.ty {
        PropertyType::Float => format!("float = {}", v[0]),
        PropertyType::Range { min, max } => format!("range({min}, {max}) = {}", v[0]),
        PropertyType::Color => format!("color = ({}, {}, {}, {})", v[0], v[1], v[2], v[3]),
        PropertyType::Vector => format!("vector = ({}, {}, {}, {})", v[0], v[1], v[2], v[3]),
        PropertyType::Texture2D { fallback } => {
            format!("texture2d = {}", fallback.texture_default().name())
        }
        PropertyType::Boolean => format!("toggle = {}", if v[0] > 0.5 { 1 } else { 0 }),
    };
    // The type goes after the quoted label: `NAME "Label" TYPE = DEFAULT`.
    let (kind, default) = ty.split_once(" = ").unwrap_or((&ty, ""));
    format!("// @property {} \"{display}\" {kind} = {default}", p.name)
}

/// Header line for one keyword.
fn keyword_line(k: &Keyword) -> String {
    let display = k.display.replace('"', "'").replace("//", "/");
    let kind = match &k.kind {
        KeywordKind::Boolean => "toggle".to_owned(),
        KeywordKind::Enum { entries } => format!(
            "enum({})",
            entries
                .iter()
                .map(|e| e.replace([',', '(', ')'], " "))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    format!(
        "// @keyword {} \"{display}\" {kind} = {}",
        k.name, k.default
    )
}

/// Structural checks of one graph; returns the node order and resolved types.
pub fn check(g: &ShaderGraph) -> Result<(Vec<u32>, Types), GraphError> {
    let mut seen = HashSet::new();
    for p in &g.properties {
        if !valid_ident(&p.name) {
            return Err(GraphError::graph(format!(
                "property reference '{}' is not a valid shader name",
                p.name
            )));
        }
        if !seen.insert(p.name.as_str()) {
            return Err(GraphError::graph(format!(
                "two properties are named {}",
                p.name
            )));
        }
    }
    for k in &g.keywords {
        if !valid_ident(&k.name) {
            return Err(GraphError::graph(format!(
                "keyword reference '{}' is not a valid shader name",
                k.name
            )));
        }
        if !seen.insert(k.name.as_str()) {
            return Err(GraphError::graph(format!(
                "two properties or keywords are named {}",
                k.name
            )));
        }
        if let KeywordKind::Enum { entries } = &k.kind {
            let mut names = HashSet::new();
            if entries.is_empty()
                || !entries
                    .iter()
                    .all(|e| !e.trim().is_empty() && names.insert(e))
            {
                return Err(GraphError::graph(format!(
                    "keyword {} needs distinct, named entries",
                    k.name
                )));
            }
        }
    }
    if g.variant_count() > MAX_VARIANTS {
        let first = g
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Keyword { .. }))
            .map(|n| n.id);
        return Err(GraphError {
            node: first,
            message: format!(
                "the keywords make {} variants; the limit is {MAX_VARIANTS}",
                g.variant_count()
            ),
        });
    }
    if g.is_sub_graph() {
        let mut names = HashSet::new();
        for s in &g.outputs {
            if s.name.trim().is_empty() || !names.insert(s.name.as_str()) {
                return Err(GraphError::graph("Sub Graph outputs need distinct names"));
            }
        }
    }
    for n in &g.nodes {
        match &n.kind {
            NodeKind::Property { name } if g.property(name).is_none() => {
                return Err(GraphError::on(n.id, format!("no property named {name}")));
            }
            NodeKind::Keyword { name } if g.keyword(name).is_none() => {
                return Err(GraphError::on(n.id, format!("no keyword named {name}")));
            }
            NodeKind::CustomFunction {
                function,
                source,
                inputs,
                outputs,
                file,
                ..
            } => {
                let all_valid = std::iter::once(function)
                    .chain(inputs.iter().map(|s| &s.name))
                    .chain(outputs.iter().map(|s| &s.name))
                    .all(|s| valid_ident(s));
                if !all_valid {
                    return Err(GraphError::on(
                        n.id,
                        "function, input and output names must be valid WGSL names",
                    ));
                }
                let mut names = HashSet::new();
                if !inputs.iter().chain(outputs).all(|s| names.insert(&s.name)) {
                    return Err(GraphError::on(n.id, "two ports share a name"));
                }
                if outputs.is_empty() {
                    return Err(GraphError::on(n.id, "a Custom Function needs an output"));
                }
                if outputs.iter().any(|s| s.ty == ValueType::Texture) {
                    return Err(GraphError::on(n.id, "outputs cannot be textures"));
                }
                if *source == FunctionSource::File && file.trim().is_empty() {
                    return Err(GraphError::on(n.id, "choose the .wgsl file"));
                }
            }
            _ => {}
        }
    }
    let order = g
        .topo_order()
        .map_err(|id| GraphError::on(id, "this node is part of a loop"))?;
    let types = g.types();
    for c in &g.connections {
        let (Some(_), Some(to)) = (g.node(c.from), g.node(c.to)) else {
            continue;
        };
        let Some(from_ty) = types.output(c.from, &c.output) else {
            return Err(GraphError::on(
                c.from,
                format!("no output named {}", c.output),
            ));
        };
        let Some(port) = g.input_port(to, &c.input) else {
            return Err(GraphError::on(c.to, format!("no input named {}", c.input)));
        };
        let to_ty = port.ty.resolve(types.dynamic(c.to));
        if !from_ty.converts_to(to_ty) {
            return Err(GraphError::on(
                c.to,
                format!(
                    "{} needs a {}, not a {}",
                    c.input,
                    to_ty.label(),
                    from_ty.label()
                ),
            ));
        }
    }
    Ok((order, types))
}

/// Function definitions of the Custom Function nodes `ids` of `g` reaches (Sub Graphs
/// included), deduplicated by name: (name, text, node blamed).
fn collect_functions(
    g: &ShaderGraph,
    ids: &HashSet<u32>,
    ctx: &Ctx<'_>,
    blame: Option<u32>,
    stack: &mut Vec<String>,
    out: &mut Vec<(String, String, u32)>,
    deps: &mut Vec<String>,
) -> Result<(), GraphError> {
    let mut sorted: Vec<&u32> = ids.iter().collect();
    sorted.sort();
    for &id in sorted {
        let Some(n) = g.node(id) else { continue };
        let who = blame.unwrap_or(id);
        match &n.kind {
            NodeKind::CustomFunction {
                function,
                source: FunctionSource::Inline,
                body,
                inputs,
                outputs,
                ..
            } => {
                let name = inline_fn_name(function, body, inputs, outputs);
                if out.iter().any(|(n, _, _)| *n == name) {
                    continue;
                }
                let mut text = format!("struct {name}_out {{\n");
                for s in outputs {
                    let _ = writeln!(text, "    {}: {},", s.name, s.ty.wgsl());
                }
                text.push_str("};\n");
                let params: Vec<String> = inputs
                    .iter()
                    .map(|s| format!("{}: {}", s.name, s.ty.wgsl()))
                    .collect();
                let _ = writeln!(text, "fn {name}({}) -> {name}_out {{", params.join(", "));
                for s in outputs {
                    let _ = writeln!(text, "    var {}: {};", s.name, s.ty.wgsl());
                }
                for l in body.lines() {
                    let _ = writeln!(text, "    {l}");
                }
                let fields: Vec<&str> = outputs.iter().map(|s| s.name.as_str()).collect();
                let _ = writeln!(text, "    return {name}_out({});\n}}", fields.join(", "));
                out.push((name, text, who));
            }
            NodeKind::CustomFunction {
                source: FunctionSource::File,
                file,
                ..
            } => {
                let key = format!("file:{file}");
                if out.iter().any(|(n, _, _)| *n == key) {
                    continue;
                }
                let text = ctx.resolver.text(file).map_err(|e| {
                    GraphError::on(who, format!("cannot read the function file: {e}"))
                })?;
                if !deps.contains(file) {
                    deps.push(file.clone());
                }
                out.push((key, text, who));
            }
            NodeKind::SubGraph { path, .. } => {
                if stack.iter().any(|p| p == path) || stack.len() > 8 {
                    return Err(GraphError::on(who, format!("{path} uses itself")));
                }
                if !deps.contains(path) {
                    deps.push(path.clone());
                }
                let Ok(sub) = ctx.resolver.graph(path) else {
                    // Reported when the node is generated.
                    continue;
                };
                let ports: Vec<&str> = sub.outputs.iter().map(|s| s.name.as_str()).collect();
                let inner = needed(&sub, &ports, ctx.variant);
                stack.push(path.clone());
                collect_functions(&sub, &inner, ctx, Some(who), stack, out, deps)?;
                stack.pop();
            }
            _ => {}
        }
    }
    Ok(())
}

/// The graph's keywords, then those declared only by the Sub Graphs it uses (recursively).
pub fn effective_keywords(g: &ShaderGraph, resolver: &dyn Resolver) -> Vec<Keyword> {
    fn walk(
        g: &ShaderGraph,
        resolver: &dyn Resolver,
        out: &mut Vec<Keyword>,
        stack: &mut Vec<String>,
    ) {
        for k in &g.keywords {
            if !out.iter().any(|o| o.name == k.name) {
                out.push(k.clone());
            }
        }
        for n in &g.nodes {
            if let NodeKind::SubGraph { path, .. } = &n.kind
                && !stack.contains(path)
                && stack.len() < 8
                && let Ok(sub) = resolver.graph(path)
            {
                stack.push(path.clone());
                walk(&sub, resolver, out, stack);
                stack.pop();
            }
        }
    }
    let mut out = Vec::new();
    walk(g, resolver, &mut out, &mut Vec::new());
    out
}

/// First line of a Decal graph's shader text (only its header: see [`generate_variant`]).
pub const DECAL_HEADER: &str =
    "// Decal Shader Graph: assign a material using it to a Decal Projector.";

/// Check the graph and generate the default variant's shader text (not yet validated by naga;
/// see [`compile`]).
pub fn generate(g: &ShaderGraph) -> Result<Generated, GraphError> {
    generate_variant(g, &Variant::new(), &FileResolver)
}

/// Generate one variant (keyword values; missing ones take their defaults).
pub fn generate_variant(
    g: &ShaderGraph,
    variant: &Variant,
    resolver: &dyn Resolver,
) -> Result<Generated, GraphError> {
    let (order, _) = check(g)?;
    let keywords = effective_keywords(g, resolver);
    if g.target.material == TargetMaterial::Decal && !g.is_sub_graph() {
        // Materials read the header (properties, keywords); the decal pass compiles the graph
        // itself (`generate_decal`). On a mesh it has no shader function: the error shader.
        let mut out = Out::default();
        out.push(DECAL_HEADER, None);
        for p in &g.properties {
            out.push(property_line(p), None);
        }
        for k in &keywords {
            out.push(keyword_line(k), None);
        }
        return Ok(finish(out, Vec::new(), keywords));
    }
    if super::variant_count(&keywords) > MAX_VARIANTS {
        return Err(GraphError::graph(format!(
            "the keywords (with the Sub Graphs') make {} variants; the limit is {MAX_VARIANTS}",
            super::variant_count(&keywords)
        )));
    }
    let ctx = Ctx {
        resolver,
        variant,
        decal: false,
        decal_slots: HashMap::new(),
    };
    let t = g.target;
    let sub = g.is_sub_graph();
    let lit = t.material == TargetMaterial::Lit && !sub;
    let mut cg = Gen::new(g, Stage::Vertex, &ctx);
    cg.out.push(
        "// Generated from a Shader Graph: edit the .shadergraph in the Shader Graph window.",
        None,
    );
    cg.out.push(
        format!("// @shader {}", if lit { "surface" } else { "unlit" }),
        None,
    );
    if sub {
        // A Sub Graph's own preview: its first output, unlit.
    } else if t.surface == Surface::Transparent {
        cg.out.push(
            format!("// @blend {}", if lit { "premultiply" } else { "alpha" }),
            None,
        );
    } else if t.alpha_clip {
        cg.out.push("// @queue alphatest", None);
    }
    if t.two_sided && !sub {
        cg.out.push("// @cull off", None);
    }
    for p in &g.properties {
        cg.out.push(property_line(p), None);
    }
    for k in &keywords {
        cg.out.push(keyword_line(k), None);
    }
    cg.out.push("", None);
    cg.out.push(HELPERS, None);
    cg.out.push(MESH_HELPERS, None);

    let vertex_ports = [master::POSITION];
    let fragment_names: Vec<&'static str> = g
        .master_ports()
        .iter()
        .filter(|(b, _)| *b != super::Block::Vertex)
        .map(|(_, p)| p.name)
        .collect();
    let fragment_ports: Vec<&str> = fragment_names.to_vec();
    let has_vertex = !sub && g.input_connection(MASTER, master::POSITION).is_some();

    // Custom Function code (and files) of the nodes this variant uses.
    let mut used = needed(g, &fragment_ports, variant);
    if has_vertex {
        used.extend(needed(g, &vertex_ports, variant));
    }
    let mut functions = Vec::new();
    let mut deps = Vec::new();
    collect_functions(
        g,
        &used,
        &ctx,
        None,
        &mut Vec::new(),
        &mut functions,
        &mut deps,
    )?;
    for (_, text, who) in &functions {
        cg.out.push(text.as_str(), Some(*who));
    }

    // Vertex stage: only when Position Offset is connected.
    if has_vertex {
        cg.out
            .push("fn vertex(v: VertexData) -> VertexData {", None);
        cg.out.push("    var o = v;", None);
        cg.nodes_for(&order, &vertex_ports)?;
        let offset = cg.master_input(master::POSITION, ValueType::Vec3)?;
        cg.out.push(
            format!("    o.position = v.position + {offset};"),
            Some(MASTER),
        );
        cg.out.push("    return o;\n}", None);
    }

    // Fragment stage.
    cg.stage = Stage::Fragment;
    cg.vars.clear();
    let m = Some(MASTER);
    if sub {
        cg.out
            .push("fn fragment(input: SurfaceInput) -> vec4<f32> {", None);
        cg.nodes_for(&order, &fragment_ports)?;
        let color = match g.outputs.first() {
            Some(s) => cg.master_input(&s.name, ValueType::Vec3)?,
            None => "vec3<f32>(0.0)".into(),
        };
        cg.out
            .push(format!("    return vec4<f32>({color}, 1.0);\n}}"), m);
    } else if lit {
        let base = |cg: &Gen| cg.master_input(master::BASE_COLOR, ValueType::Vec3);
        cg.out
            .push("fn surface(input: SurfaceInput) -> SurfaceOutput {", None);
        cg.out.push("    var o = default_output(input);", None);
        cg.nodes_for(&order, &fragment_ports)?;
        cg.out.push(format!("    o.albedo = {};", base(&cg)?), m);
        if g.input_connection(MASTER, master::NORMAL).is_some() {
            let n = cg.master_input(master::NORMAL, ValueType::Vec3)?;
            let world = match t.normal_space {
                NormalSpace::Tangent => format!("tangent_to_world(input, {n})"),
                NormalSpace::Object => format!("sg_object_normal_to_world(input, {n})"),
                NormalSpace::World => format!("normalize({n})"),
            };
            cg.out.push(format!("    o.normal = {world};"), m);
        }
        for (field, port) in [
            ("metallic", master::METALLIC),
            ("smoothness", master::SMOOTHNESS),
            ("alpha", master::ALPHA),
        ] {
            let e = cg.master_input(port, ValueType::Float)?;
            cg.out.push(format!("    o.{field} = {e};"), m);
        }
        let e = cg.master_input(master::EMISSION, ValueType::Vec3)?;
        cg.out.push(format!("    o.emission = {e};"), m);
        if g.input_connection(MASTER, master::OCCLUSION).is_some() {
            let e = cg.master_input(master::OCCLUSION, ValueType::Float)?;
            cg.out.push(format!("    o.occlusion = {e};"), m);
        }
        if g.input_connection(MASTER, master::TRANSMISSION).is_some() {
            let e = cg.master_input(master::TRANSMISSION, ValueType::Vec3)?;
            cg.out.push(format!("    o.transmission = {e};"), m);
        }
        // Standard Lit v2 blocks: written (and their lighting compiled in) when the Graph
        // Settings turn them on (`o.coat`, `o.anisotropy`, `o.subsurface`, see
        // render/material_v2.rs).
        if t.subsurface {
            let e = cg.master_input(master::SUBSURFACE, ValueType::Vec3)?;
            cg.out.push(format!("    o.subsurface = {e};"), m);
            if g.input_connection(MASTER, master::THICKNESS).is_some() {
                let e = cg.master_input(master::THICKNESS, ValueType::Float)?;
                cg.out.push(
                    format!("    o.transmission = o.transmission * (1.0 - saturate({e}));"),
                    m,
                );
            }
        }
        if t.clear_coat {
            let e = cg.master_input(master::COAT_MASK, ValueType::Float)?;
            cg.out.push(format!("    o.coat = {e};"), m);
            let e = cg.master_input(master::COAT_SMOOTHNESS, ValueType::Float)?;
            cg.out.push(format!("    o.coat_smoothness = {e};"), m);
        }
        if t.anisotropy {
            let e = cg.master_input(master::ANISOTROPY, ValueType::Float)?;
            cg.out.push(format!("    o.anisotropy = {e};"), m);
            let e = cg.master_input(master::ANISOTROPY_DIRECTION, ValueType::Vec3)?;
            cg.out.push(
                format!("    o.anisotropy_direction = tangent_to_world(input, normalize({e}));"),
                m,
            );
        }
        if t.alpha_clip {
            let c = cg.master_input(master::ALPHA_CLIP, ValueType::Float)?;
            cg.out.push(
                format!("    if (o.alpha < {c}) {{\n        discard;\n    }}"),
                m,
            );
        }
        cg.out.push("    return o;\n}", None);
    } else {
        cg.out
            .push("fn fragment(input: SurfaceInput) -> vec4<f32> {", None);
        cg.nodes_for(&order, &fragment_ports)?;
        let color = format!(
            "{} + {}",
            cg.master_input(master::BASE_COLOR, ValueType::Vec3)?,
            cg.master_input(master::EMISSION, ValueType::Vec3)?
        );
        cg.out.push(format!("    let sg_color = {color};"), m);
        let a = cg.master_input(master::ALPHA, ValueType::Float)?;
        cg.out.push(format!("    let sg_alpha = {a};"), m);
        if t.alpha_clip {
            let c = cg.master_input(master::ALPHA_CLIP, ValueType::Float)?;
            cg.out.push(
                format!("    if (sg_alpha < {c}) {{\n        discard;\n    }}"),
                m,
            );
        }
        cg.out.push(
            "    return vec4<f32>(apply_fog(sg_color, input.world_pos), sg_alpha);\n}",
            m,
        );
    }
    Ok(finish(cg.out, deps, keywords))
}

fn finish(out: Out, deps: Vec<String>, keywords: Vec<Keyword>) -> Generated {
    let mut source = String::new();
    let mut lines = Vec::with_capacity(out.lines.len());
    for (l, n) in out.lines {
        let _ = writeln!(source, "{l}");
        lines.push(n);
    }
    Generated {
        source,
        lines,
        deps,
        keywords,
    }
}

/// Generate and validate a graph's default variant as the renderer would compile it; WGSL
/// errors land on the node whose line failed.
pub fn compile(g: &ShaderGraph) -> Result<Generated, GraphError> {
    compile_variant(g, &Variant::new(), &FileResolver)
}

/// Generate and validate one variant.
pub fn compile_variant(
    g: &ShaderGraph,
    variant: &Variant,
    resolver: &dyn Resolver,
) -> Result<Generated, GraphError> {
    if g.target.material == TargetMaterial::Decal && !g.is_sub_graph() {
        return compile_decal(g, variant, resolver).map(|d| Generated {
            source: d.body,
            lines: Vec::new(),
            deps: d.deps,
            keywords: d.keywords,
        });
    }
    let cg = generate_variant(g, variant, resolver)?;
    crate::render::custom_shader::check(&cg.source).map_err(|e| GraphError {
        node: cg.node_at_line(e.line),
        message: e.message,
    })?;
    Ok(cg)
}

/// Compile every variant (examples and tests); returns how many there are.
pub fn compile_all(g: &ShaderGraph, resolver: &dyn Resolver) -> Result<usize, GraphError> {
    let keywords = effective_keywords(g, resolver);
    let variants = super::all_variants(&keywords);
    if variants.is_empty() {
        return Err(GraphError::graph(format!(
            "the keywords make more than {MAX_VARIANTS} variants"
        )));
    }
    for v in &variants {
        compile_variant(g, v, resolver).map_err(|e| GraphError {
            node: e.node,
            message: format!(
                "variant {}: {}",
                super::variant_key(&keywords, v),
                e.message
            ),
        })?;
    }
    Ok(variants.len())
}

// ---- Decal target --------------------------------------------------------------------------

/// Most Blackboard properties a Decal graph can have (one parameter slot each).
pub const DECAL_MAX_PROPERTIES: usize = 8;

/// A Decal graph turned into code for the decal pass.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecalGenerated {
    /// Function definitions (Custom Functions, files) by name, to include once per module.
    pub functions: Vec<(String, String)>,
    /// `fn <name>(input: DecalInput) -> DecalOutput { … }`.
    pub body: String,
    /// The properties in parameter-slot order.
    pub properties: Vec<super::GraphProperty>,
    pub deps: Vec<String>,
    pub keywords: Vec<Keyword>,
}

/// Generate a Decal graph's variant as `fn <fn_name>(input: DecalInput) -> DecalOutput`.
pub fn generate_decal(
    g: &ShaderGraph,
    variant: &Variant,
    resolver: &dyn Resolver,
    fn_name: &str,
) -> Result<DecalGenerated, GraphError> {
    if g.target.material != TargetMaterial::Decal {
        return Err(GraphError::graph("not a Decal graph"));
    }
    let (order, _) = check(g)?;
    if g.properties.len() > DECAL_MAX_PROPERTIES {
        return Err(GraphError::graph(format!(
            "a Decal graph has at most {DECAL_MAX_PROPERTIES} properties"
        )));
    }
    let keywords = effective_keywords(g, resolver);
    let ctx = Ctx {
        resolver,
        variant,
        decal: true,
        decal_slots: g
            .properties
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), i))
            .collect(),
    };
    let ports: Vec<&'static str> = g.master_ports().iter().map(|(_, p)| p.name).collect();
    let used = needed(g, &ports, variant);
    let mut functions = Vec::new();
    let mut deps = Vec::new();
    collect_functions(
        g,
        &used,
        &ctx,
        None,
        &mut Vec::new(),
        &mut functions,
        &mut deps,
    )?;
    let mut cg = Gen::new(g, Stage::Fragment, &ctx);
    cg.out.push(
        format!("fn {fn_name}(input: DecalInput) -> DecalOutput {{"),
        None,
    );
    cg.out.push("    var o: DecalOutput;", None);
    cg.nodes_for(&order, &ports)?;
    let m = Some(MASTER);
    for (field, port, ty) in [
        ("albedo", master::BASE_COLOR, ValueType::Vec3),
        ("alpha", master::ALPHA, ValueType::Float),
        ("normal", master::NORMAL, ValueType::Vec3),
        ("metallic", master::METALLIC, ValueType::Float),
        ("smoothness", master::SMOOTHNESS, ValueType::Float),
        ("occlusion", master::OCCLUSION, ValueType::Float),
        ("emission", master::EMISSION, ValueType::Vec3),
    ] {
        let e = cg.master_input(port, ty)?;
        cg.out.push(format!("    o.{field} = {e};"), m);
    }
    cg.out.push("    return o;\n}", None);
    let generated = finish(cg.out, Vec::new(), Vec::new());
    Ok(DecalGenerated {
        functions: functions
            .into_iter()
            .map(|(name, text, _)| (name, text))
            .collect(),
        body: generated.source,
        properties: g.properties.clone(),
        deps,
        keywords,
    })
}

/// Generate and validate a Decal graph inside a minimal decal module.
pub fn compile_decal(
    g: &ShaderGraph,
    variant: &Variant,
    resolver: &dyn Resolver,
) -> Result<DecalGenerated, GraphError> {
    let d = generate_decal(g, variant, resolver, "sg_decal_1")?;
    crate::render::decals::validate_graph_named(&d, "sg_decal_1").map_err(GraphError::graph)?;
    Ok(d)
}
