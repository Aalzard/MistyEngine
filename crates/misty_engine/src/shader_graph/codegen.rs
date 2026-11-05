//! Graph → `.shader` text: a header declaring the Blackboard properties and render state, the
//! node helpers, an optional `fn vertex` (Position Offset) and the `fn surface` (Lit) or
//! `fn fragment` (Unlit) that the custom-shader wrapper turns into a pipeline.
//!
//! Each node becomes `let n<id>_<output>` lines in dependency order. Every generated line remembers
//! the node it came from, so a WGSL error is reported on the offending node.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::nodes::{NodeKind, Port, PortDefault, Space, ValueType, master};
use super::{MASTER, Node, PropertyType, ShaderGraph, Surface, TargetMaterial};

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

/// Generated shader text with the node behind each line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    pub source: String,
    /// Node of each line (index = line - 1).
    lines: Vec<Option<u32>>,
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

/// Functions the node code calls (always emitted; naga ignores the unused ones).
const HELPERS: &str = r#"fn sg_hash_u32(x: u32) -> u32 {
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
fn sg_vertex_world_pos(v: VertexData) -> vec3<f32> {
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

struct Gen<'a> {
    g: &'a ShaderGraph,
    types: super::Types,
    stage: Stage,
    /// Output variables generated in the current function: (node, output) → (expr, type).
    vars: HashMap<(u32, String), (String, ValueType)>,
    out: Out,
}

/// Nodes feeding the given ports of the master stack.
fn ancestors(g: &ShaderGraph, ports: &[&str]) -> HashSet<u32> {
    let mut stack: Vec<u32> = g
        .connections
        .iter()
        .filter(|c| c.to == MASTER && ports.contains(&c.input.as_str()))
        .map(|c| c.from)
        .collect();
    let mut seen = HashSet::new();
    while let Some(n) = stack.pop() {
        if seen.insert(n) {
            stack.extend(g.connections.iter().filter(|c| c.to == n).map(|c| c.from));
        }
    }
    seen
}

impl Gen<'_> {
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
            PortDefault::None => Err(GraphError::on(
                node.id,
                format!("connect a Texture2D property to {}", port.name),
            )),
            d => {
                let (e, ty) = self.binding(d);
                Ok(convert(&e, ty, want))
            }
        }
    }

    fn model(&self) -> &'static str {
        match self.stage {
            Stage::Fragment => "draws[input.draw].model",
            Stage::Vertex => "draws[v.draw].model",
        }
    }

    fn sample(&self, tex: &str, uv: &str) -> String {
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
        let mut ins: HashMap<&str, String> = HashMap::new();
        for p in node.kind.inputs() {
            let want = p.ty.resolve(dynamic);
            ins.insert(p.name, self.input(node, &p, want)?);
        }
        let i = |name: &str| ins.get(name).cloned().unwrap_or_else(|| "0.0".into());
        let frag = self.stage == Stage::Fragment;
        let mut pre: Vec<String> = Vec::new();
        let v = |n: &str| format!("n{id}_{n}");
        let outs: Vec<String> = match &node.kind {
            K::Master => return Ok(()),
            K::Property { name } => {
                let Some(p) = self.g.property(name) else {
                    return Err(GraphError::on(id, format!("no property named {name}")));
                };
                match p.ty {
                    PropertyType::Texture2D { .. } => vec![name.clone()],
                    _ => vec![format!("material.{name}")],
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
            K::Time => ["x", "y", "z", "w"]
                .iter()
                .map(|c| format!("globals.time.{c}"))
                .collect(),
            K::Uv => vec![self.binding(PortDefault::Uv).0],
            K::Position { space } => {
                let world = self.binding(PortDefault::WorldPos).0;
                vec![match (space, frag) {
                    (Space::World, _) => world,
                    (Space::Object, true) => format!("sg_object_pos({}, {world})", self.model()),
                    (Space::Object, false) => "v.position".into(),
                }]
            }
            K::Normal { space } => {
                let world = self.binding(PortDefault::WorldNormal).0;
                vec![match (space, frag) {
                    (Space::World, _) => world,
                    (Space::Object, true) => format!("sg_object_dir({}, {world})", self.model()),
                    (Space::Object, false) => "v.normal".into(),
                }]
            }
            K::ViewDirection { space } => {
                let world = self.binding(PortDefault::ViewDir).0;
                vec![match space {
                    Space::World => world,
                    Space::Object => format!("sg_object_dir({}, {world})", self.model()),
                }]
            }
            K::VertexColor => vec!["vec4<f32>(1.0)".into()],
            K::ScreenPosition => vec![if frag {
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
        };
        for line in pre {
            self.out.push(format!("    {line}"), Some(id));
        }
        for (k, (port, expr)) in self.g.outputs(node).iter().zip(outs).enumerate() {
            let ty = self
                .types
                .output(id, port.name)
                .unwrap_or(port.ty.resolve(dynamic));
            if ty == ValueType::Texture {
                self.vars.insert((id, port.name.to_owned()), (expr, ty));
                continue;
            }
            let name = format!("n{id}_{k}");
            self.out
                .push(format!("    let {name}: {} = {expr};", ty.wgsl()), Some(id));
            self.vars.insert((id, port.name.to_owned()), (name, ty));
        }
        Ok(())
    }

    /// Generate the nodes feeding `ports` of the master stack, in order.
    fn nodes_for(&mut self, order: &[u32], ports: &[&str]) -> Result<(), GraphError> {
        let needed = ancestors(self.g, ports);
        let g = self.g;
        for id in order {
            if needed.contains(id)
                && let Some(n) = g.node(*id)
            {
                self.node(n)?;
            }
        }
        Ok(())
    }

    /// A master-stack input (None = unconnected and not needed).
    fn master_input(&self, name: &str, ty: ValueType) -> Result<String, GraphError> {
        let m = self
            .g
            .node(MASTER)
            .ok_or_else(|| GraphError::graph("the graph has no master stack"))?;
        let port = m
            .input(name)
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

/// Check the graph and generate its shader text (not yet validated by naga; see [`compile`]).
pub fn generate(g: &ShaderGraph) -> Result<Generated, GraphError> {
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
    for n in &g.nodes {
        if let NodeKind::Property { name } = &n.kind
            && g.property(name).is_none()
        {
            return Err(GraphError::on(n.id, format!("no property named {name}")));
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
        let Some(port) = to.input(&c.input) else {
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

    let t = g.target;
    let lit = t.material == TargetMaterial::Lit;
    let mut cg = Gen {
        g,
        types,
        stage: Stage::Vertex,
        vars: HashMap::new(),
        out: Out::default(),
    };
    cg.out.push(
        "// Generated from a Shader Graph: edit the .shadergraph in the Shader Graph window.",
        None,
    );
    cg.out.push(
        format!("// @shader {}", if lit { "surface" } else { "unlit" }),
        None,
    );
    if t.surface == Surface::Transparent {
        cg.out.push(
            format!("// @blend {}", if lit { "premultiply" } else { "alpha" }),
            None,
        );
    } else if t.alpha_clip {
        cg.out.push("// @queue alphatest", None);
    }
    if t.two_sided {
        cg.out.push("// @cull off", None);
    }
    for p in &g.properties {
        cg.out.push(property_line(p), None);
    }
    cg.out.push("", None);
    cg.out.push(HELPERS, None);

    // Vertex stage: only when Position Offset is connected.
    if g.input_connection(MASTER, master::POSITION).is_some() {
        cg.out
            .push("fn vertex(v: VertexData) -> VertexData {", None);
        cg.out.push("    var o = v;", None);
        cg.nodes_for(&order, &[master::POSITION])?;
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
    let ports: Vec<&str> = g
        .master_ports()
        .iter()
        .filter(|(b, _)| *b == super::Block::Fragment)
        .map(|(_, p)| p.name)
        .collect();
    let base = |cg: &Gen| cg.master_input(master::BASE_COLOR, ValueType::Vec3);
    if lit {
        cg.out
            .push("fn surface(input: SurfaceInput) -> SurfaceOutput {", None);
        cg.out.push("    var o = default_output(input);", None);
        cg.nodes_for(&order, &ports)?;
        let m = Some(MASTER);
        cg.out.push(format!("    o.albedo = {};", base(&cg)?), m);
        if g.input_connection(MASTER, master::NORMAL).is_some() {
            let n = cg.master_input(master::NORMAL, ValueType::Vec3)?;
            cg.out
                .push(format!("    o.normal = tangent_to_world(input, {n});"), m);
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
        cg.nodes_for(&order, &ports)?;
        let m = Some(MASTER);
        let color = format!(
            "{} + {}",
            base(&cg)?,
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

    let mut source = String::new();
    let mut lines = Vec::with_capacity(cg.out.lines.len());
    for (l, n) in cg.out.lines {
        let _ = writeln!(source, "{l}");
        lines.push(n);
    }
    Ok(Generated { source, lines })
}

/// Generate and validate a graph as the renderer would compile it; WGSL errors land on the node
/// whose line failed.
pub fn compile(g: &ShaderGraph) -> Result<Generated, GraphError> {
    let cg = generate(g)?;
    crate::render::custom_shader::check(&cg.source).map_err(|e| GraphError {
        node: cg.node_at_line(e.line),
        message: e.message,
    })?;
    Ok(cg)
}
