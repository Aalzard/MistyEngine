//! CPU evaluation of a graph over a small grid, for the Shader Graph window's node previews.
//! It mirrors the generated WGSL (same hash and noise functions, same conversions), on a flat
//! quad (UV across the square) or a sphere for nodes that read 3D geometry.

use std::collections::HashMap;

use super::codegen::linear_color;
use super::nodes::{NodeKind, PortDefault, Space, ValueType};
use super::{GraphProperty, PropertyType, ShaderGraph, TextureFallback, Types};

/// Where previews are evaluated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Context {
    /// Grid size in pixels (square).
    pub size: usize,
    /// Seconds.
    pub time: f32,
    /// Sphere (3D) instead of a flat quad.
    pub sphere: bool,
}

/// Texture sampling for Sample Texture 2D (by property).
pub trait Textures {
    /// Linear RGBA of `property`'s texture at `uv` (repeating).
    fn sample(&self, property: &GraphProperty, uv: [f32; 2]) -> [f32; 4];
}

/// The properties' fallback colours.
pub struct Fallback;

impl Textures for Fallback {
    fn sample(&self, property: &GraphProperty, _uv: [f32; 2]) -> [f32; 4] {
        fallback_color(property)
    }
}

/// Linear colour of a texture property's fallback.
pub fn fallback_color(property: &GraphProperty) -> [f32; 4] {
    match property.ty {
        PropertyType::Texture2D { fallback } => match fallback {
            TextureFallback::White => [1.0; 4],
            TextureFallback::Black => [0.0, 0.0, 0.0, 1.0],
            TextureFallback::Grey => [0.2159, 0.2159, 0.2159, 1.0],
            TextureFallback::Normal => [0.5, 0.5, 1.0, 1.0],
        },
        _ => [1.0; 4],
    }
}

/// One pixel's geometry.
#[derive(Clone, Copy)]
struct Geo {
    uv: [f32; 2],
    pos: [f32; 3],
    normal: [f32; 3],
    view: [f32; 3],
    screen: [f32; 2],
    covered: bool,
}

fn geometry(ctx: &Context, x: usize, y: usize) -> Geo {
    let s = ctx.size as f32;
    let (fx, fy) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
    if !ctx.sphere {
        return Geo {
            uv: [fx, 1.0 - fy],
            pos: [fx - 0.5, 0.5 - fy, 0.0],
            normal: [0.0, 0.0, 1.0],
            view: [0.0, 0.0, 1.0],
            screen: [fx, fy],
            covered: true,
        };
    }
    let (px, py) = (fx * 2.0 - 1.0, 1.0 - fy * 2.0);
    let r2 = px * px + py * py;
    let covered = r2 <= 1.0;
    let nz = (1.0 - r2.min(1.0)).sqrt();
    let n = [px, py, nz];
    Geo {
        uv: [
            px.atan2(nz) / std::f32::consts::TAU + 0.5,
            py.clamp(-1.0, 1.0).asin() / std::f32::consts::PI + 0.5,
        ],
        pos: [px * 0.5, py * 0.5, nz * 0.5],
        normal: n,
        view: [0.0, 0.0, 1.0],
        screen: [fx, fy],
        covered,
    }
}

type V = [f32; 4];

fn v3(a: [f32; 3]) -> V {
    [a[0], a[1], a[2], 0.0]
}

/// Same rules as [`super::codegen::convert`].
pub fn convert(v: V, from: ValueType, to: ValueType) -> V {
    if from == to || from == ValueType::Texture || to == ValueType::Texture {
        return v;
    }
    let (m, n) = (from.dims(), to.dims());
    if m == 1 {
        return [v[0]; 4];
    }
    let mut out = [0.0; 4];
    out[..n.min(m)].copy_from_slice(&v[..n.min(m)]);
    out
}

fn map(a: V, f: impl Fn(f32) -> f32) -> V {
    [f(a[0]), f(a[1]), f(a[2]), f(a[3])]
}

fn map2(a: V, b: V, f: impl Fn(f32, f32) -> f32) -> V {
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]), f(a[3], b[3])]
}

fn map3(a: V, b: V, c: V, f: impl Fn(f32, f32, f32) -> f32) -> V {
    [
        f(a[0], b[0], c[0]),
        f(a[1], b[1], c[1]),
        f(a[2], b[2], c[2]),
        f(a[3], b[3], c[3]),
    ]
}

fn dot_n(a: V, b: V, n: usize) -> f32 {
    (0..n).map(|i| a[i] * b[i]).sum()
}

fn normalize_n(a: V, n: usize) -> V {
    let l = dot_n(a, a, n).sqrt();
    if l <= 1e-12 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for i in 0..n {
        out[i] = a[i] / l;
    }
    out
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn hash_u32(x: u32) -> u32 {
    let mut h = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    h = ((h >> ((h >> 28) + 4)) ^ h).wrapping_mul(277_803_737);
    (h >> 22) ^ h
}

/// Same as the WGSL `sg_hash22`.
pub fn hash22(p: [f32; 2]) -> [f32; 2] {
    let ix = p[0].floor() as i32 as u32;
    let iy = p[1].floor() as i32 as u32;
    let a = hash_u32(ix ^ hash_u32(iy));
    let b = hash_u32(a ^ 2_654_435_769);
    [(a & 65535) as f32 / 65535.0, (b & 65535) as f32 / 65535.0]
}

fn gradient_dir(p: [f32; 2]) -> [f32; 2] {
    let a = hash22(p)[0] * std::f32::consts::TAU;
    [a.cos(), a.sin()]
}

/// Same as the WGSL `sg_gradient_noise` (about 0..1).
pub fn gradient_noise(uv: [f32; 2], scale: f32) -> f32 {
    let p = [uv[0] * scale, uv[1] * scale];
    let ip = [p[0].floor(), p[1].floor()];
    let fp = [p[0] - ip[0], p[1] - ip[1]];
    let corner = |dx: f32, dy: f32| {
        let g = gradient_dir([ip[0] + dx, ip[1] + dy]);
        g[0] * (fp[0] - dx) + g[1] * (fp[1] - dy)
    };
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (fx, fy) = (fade(fp[0]), fade(fp[1]));
    mix(
        mix(corner(0.0, 0.0), corner(0.0, 1.0), fy),
        mix(corner(1.0, 0.0), corner(1.0, 1.0), fy),
        fx,
    ) + 0.5
}

/// Same as the WGSL `sg_voronoi`: (distance to the nearest point, its cell value).
pub fn voronoi(uv: [f32; 2], angle: f32, density: f32) -> [f32; 2] {
    let p = [uv[0] * density, uv[1] * density];
    let ip = [p[0].floor(), p[1].floor()];
    let fp = [p[0] - ip[0], p[1] - ip[1]];
    let (mut best, mut cell) = (8.0f32, 0.0);
    for y in -1..=1 {
        for x in -1..=1 {
            let o = [x as f32, y as f32];
            let h = hash22([ip[0] + o[0], ip[1] + o[1]]);
            let dx = o[0] + 0.5 + 0.5 * (h[0] * std::f32::consts::TAU + angle).sin() - fp[0];
            let dy = o[1] + 0.5 + 0.5 * (h[1] * std::f32::consts::TAU + angle).sin() - fp[1];
            let d = (dx * dx + dy * dy).sqrt();
            if d < best {
                best = d;
                cell = h[0];
            }
        }
    }
    [best, cell]
}

/// Per-pixel values of every output, plus the coverage mask.
pub struct Evaluated {
    pub size: usize,
    pub types: Types,
    values: HashMap<(u32, String), Vec<V>>,
    covered: Vec<bool>,
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

impl Evaluated {
    pub fn values(&self, node: u32, port: &str) -> Option<&[V]> {
        self.values
            .get(&(node, port.to_owned()))
            .map(|v| v.as_slice())
    }

    /// Display pixels (sRGB, row-major) of an output: floats grey, vec2 as red/green.
    pub fn image(&self, node: u32, port: &str) -> Option<Vec<[u8; 4]>> {
        let values = self.values(node, port)?;
        let ty = self.types.output(node, port)?;
        Some(
            values
                .iter()
                .zip(&self.covered)
                .map(|(v, &covered)| {
                    let rgb = match ty {
                        ValueType::Float => [v[0]; 3],
                        ValueType::Vec2 => [v[0], v[1], 0.0],
                        _ => [v[0], v[1], v[2]],
                    };
                    let a = if covered { 255 } else { 0 };
                    [to_srgb(rgb[0]), to_srgb(rgb[1]), to_srgb(rgb[2]), a]
                })
                .collect(),
        )
    }
}

/// Evaluate every node of `g` that can be (nodes on a loop or with missing inputs are skipped).
pub fn evaluate(g: &ShaderGraph, ctx: &Context, textures: &dyn Textures) -> Evaluated {
    let types = g.types();
    let n = ctx.size * ctx.size;
    let geo: Vec<Geo> = (0..n)
        .map(|i| geometry(ctx, i % ctx.size, i / ctx.size))
        .collect();
    let mut values: HashMap<(u32, String), Vec<V>> = HashMap::new();
    // Texture outputs carry the property they read.
    let mut tex: HashMap<(u32, String), GraphProperty> = HashMap::new();
    let order = g.topo_order().unwrap_or_default();
    for id in order {
        let Some(node) = g.node(id) else { continue };
        if node.kind == NodeKind::Master {
            continue;
        }
        let dynamic = types.dynamic(id);
        let inputs = node.kind.inputs();
        // Per input: per-pixel values (or a texture property).
        let mut ins: Vec<Vec<V>> = Vec::with_capacity(inputs.len());
        let mut tex_in: Option<GraphProperty> = None;
        let mut missing = false;
        for p in &inputs {
            let want = p.ty.resolve(dynamic);
            let conn = g.input_connection(id, p.name);
            if want == ValueType::Texture {
                match conn.and_then(|c| tex.get(&(c.from, c.output.clone()))) {
                    Some(prop) => tex_in = Some(prop.clone()),
                    None => missing = true,
                }
                ins.push(Vec::new());
                continue;
            }
            if let Some(c) = conn {
                match (
                    values.get(&(c.from, c.output.clone())),
                    types.output(c.from, &c.output),
                ) {
                    (Some(src), Some(ty)) => {
                        ins.push(src.iter().map(|&v| convert(v, ty, want)).collect());
                        continue;
                    }
                    _ => {
                        missing = true;
                        ins.push(Vec::new());
                        continue;
                    }
                }
            }
            let column: Vec<V> = match p.default {
                PortDefault::Value(_) => vec![convert(node.value(p), want, want); n],
                PortDefault::Uv => geo
                    .iter()
                    .map(|g| convert([g.uv[0], g.uv[1], 0.0, 0.0], ValueType::Vec2, want))
                    .collect(),
                PortDefault::WorldNormal => geo
                    .iter()
                    .map(|g| convert(v3(g.normal), ValueType::Vec3, want))
                    .collect(),
                PortDefault::ViewDir => geo
                    .iter()
                    .map(|g| convert(v3(g.view), ValueType::Vec3, want))
                    .collect(),
                PortDefault::WorldPos => geo
                    .iter()
                    .map(|g| convert(v3(g.pos), ValueType::Vec3, want))
                    .collect(),
                PortDefault::None => {
                    missing = true;
                    Vec::new()
                }
            };
            // Constant inputs keep only the type's components.
            let column = column
                .into_iter()
                .map(|v| match want.dims() {
                    1 => [v[0]; 4],
                    d => {
                        let mut o = [0.0; 4];
                        o[..d].copy_from_slice(&v[..d]);
                        o
                    }
                })
                .collect();
            ins.push(column);
        }
        let outputs = g.outputs(node);
        if let NodeKind::Property { name } = &node.kind
            && let Some(p) = g.property(name)
            && matches!(p.ty, PropertyType::Texture2D { .. })
        {
            tex.insert((id, "Out".into()), p.clone());
            continue;
        }
        if missing {
            continue;
        }
        let mut cols: Vec<Vec<V>> = vec![Vec::with_capacity(n); outputs.len()];
        for (px, gp) in geo.iter().enumerate() {
            let at = |k: usize| {
                ins.get(k)
                    .and_then(|c| c.get(px))
                    .copied()
                    .unwrap_or([0.0; 4])
            };
            let o = pixel(g, node, dynamic, &at, gp, ctx, tex_in.as_ref(), textures);
            for (k, col) in cols.iter_mut().enumerate() {
                col.push(o.get(k).copied().unwrap_or([0.0; 4]));
            }
        }
        for (p, col) in outputs.iter().zip(cols) {
            values.insert((id, p.name.to_owned()), col);
        }
    }
    Evaluated {
        size: ctx.size,
        types,
        values,
        covered: geo.iter().map(|g| g.covered).collect(),
    }
}

/// One node's outputs at one pixel; `at(k)` is input k (already converted).
#[allow(clippy::too_many_arguments)]
fn pixel(
    g: &ShaderGraph,
    node: &super::Node,
    dynamic: ValueType,
    at: &dyn Fn(usize) -> V,
    geo: &Geo,
    ctx: &Context,
    tex: Option<&GraphProperty>,
    textures: &dyn Textures,
) -> Vec<V> {
    use NodeKind as K;
    let dims = dynamic.dims();
    let t = ctx.time;
    let sample = |uv: V| -> V { tex.map_or([1.0; 4], |p| textures.sample(p, [uv[0], uv[1]])) };
    match &node.kind {
        K::Master => vec![],
        K::Property { name } => {
            let Some(p) = g.property(name) else {
                return vec![[0.0; 4]];
            };
            vec![match p.ty {
                PropertyType::Color => linear_color(p.value),
                PropertyType::Vector => p.value,
                _ => [p.value[0]; 4],
            }]
        }
        K::Color { value } => vec![linear_color(*value)],
        K::Float => vec![at(0)],
        K::Vector2 => vec![[at(0)[0], at(1)[0], 0.0, 0.0]],
        K::Vector3 => vec![[at(0)[0], at(1)[0], at(2)[0], 0.0]],
        K::Vector4 => vec![[at(0)[0], at(1)[0], at(2)[0], at(3)[0]]],
        K::Time => vec![[t; 4], [t.sin(); 4], [t.cos(); 4], [1.0 / 60.0; 4]],
        K::Uv => vec![[geo.uv[0], geo.uv[1], 0.0, 0.0]],
        K::Position { space } => vec![match space {
            Space::World => v3(geo.pos),
            Space::Object => v3(geo.pos.map(|c| c * 2.0)),
        }],
        K::Normal { .. } => vec![v3(geo.normal)],
        K::ViewDirection { .. } => vec![v3(geo.view)],
        K::VertexColor => vec![[1.0; 4]],
        K::ScreenPosition => vec![[geo.screen[0], geo.screen[1], 0.5, 1.0]],
        K::Add => vec![map2(at(0), at(1), |a, b| a + b)],
        K::Subtract => vec![map2(at(0), at(1), |a, b| a - b)],
        K::Multiply => vec![map2(at(0), at(1), |a, b| a * b)],
        K::Divide => vec![map2(at(0), at(1), |a, b| a / b)],
        K::Power => vec![map2(at(0), at(1), f32::powf)],
        K::Minimum => vec![map2(at(0), at(1), f32::min)],
        K::Maximum => vec![map2(at(0), at(1), f32::max)],
        K::Lerp => vec![map3(at(0), at(1), at(2), mix)],
        K::Clamp => vec![map3(at(0), at(1), at(2), |x, lo, hi| x.max(lo).min(hi))],
        K::Saturate => vec![map(at(0), |x| x.clamp(0.0, 1.0))],
        K::OneMinus => vec![map(at(0), |x| 1.0 - x)],
        K::Negate => vec![map(at(0), |x| -x)],
        K::Remap => {
            let (im, om) = (at(1), at(2));
            vec![map(at(0), |x| {
                om[0] + (x - im[0]) * (om[1] - om[0]) / (im[1] - im[0])
            })]
        }
        K::Step => vec![map2(at(0), at(1), |e, x| if x < e { 0.0 } else { 1.0 })],
        K::Smoothstep => vec![map3(at(0), at(1), at(2), smoothstep)],
        K::Sine => vec![map(at(0), f32::sin)],
        K::Cosine => vec![map(at(0), f32::cos)],
        K::Absolute => vec![map(at(0), f32::abs)],
        K::Floor => vec![map(at(0), f32::floor)],
        K::Fraction => vec![map(at(0), fract)],
        K::Dot => vec![[dot_n(at(0), at(1), dims); 4]],
        K::Cross => {
            let (a, b) = (at(0), at(1));
            vec![[
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
                0.0,
            ]]
        }
        K::Normalize => vec![if dims == 1 {
            [at(0)[0].signum(); 4]
        } else {
            normalize_n(at(0), dims)
        }],
        K::Length => vec![[dot_n(at(0), at(0), dims).sqrt(); 4]],
        K::Split => {
            let v = convert(at(0), dynamic, ValueType::Vec4);
            vec![[v[0]; 4], [v[1]; 4], [v[2]; 4], [v[3]; 4]]
        }
        K::Combine => {
            let (r, gg, b, a) = (at(0)[0], at(1)[0], at(2)[0], at(3)[0]);
            vec![[r, gg, b, a], [r, gg, b, 0.0], [r, gg, 0.0, 0.0]]
        }
        K::SampleTexture2D { normal_map } => {
            let mut s = sample(at(1));
            if *normal_map {
                let m = [
                    s[0] * 2.0 - 1.0,
                    s[1] * 2.0 - 1.0,
                    (s[2] * 2.0 - 1.0).max(1e-3),
                ];
                let n = normalize_n(v3(m), 3);
                s = [n[0], n[1], n[2], 1.0];
            }
            vec![s, [s[0]; 4], [s[1]; 4], [s[2]; 4], [s[3]; 4]]
        }
        K::TilingAndOffset => {
            let (uv, tl, of) = (at(0), at(1), at(2));
            vec![[uv[0] * tl[0] + of[0], uv[1] * tl[1] + of[1], 0.0, 0.0]]
        }
        K::NormalFromTexture => {
            let uv = at(1);
            let o = at(2)[0].powi(3) * 0.1;
            let s = at(3)[0];
            let h0 = sample(uv)[0];
            let h1 = sample([uv[0] + o, uv[1], 0.0, 0.0])[0];
            let h2 = sample([uv[0], uv[1] + o, 0.0, 0.0])[0];
            vec![normalize_n([(h0 - h1) * s, (h0 - h2) * s, 1.0, 0.0], 3)]
        }
        K::NormalStrength => {
            let (n, k) = (at(0), at(1)[0]);
            vec![normalize_n(
                [n[0] * k, n[1] * k, mix(1.0, n[2], k.clamp(0.0, 1.0)), 0.0],
                3,
            )]
        }
        K::GradientNoise => {
            let uv = at(0);
            vec![[gradient_noise([uv[0], uv[1]], at(1)[0]); 4]]
        }
        K::Voronoi => {
            let uv = at(0);
            let v = voronoi([uv[0], uv[1]], at(1)[0], at(2)[0]);
            vec![[v[0]; 4], [v[1]; 4]]
        }
        K::Checkerboard => {
            let (uv, a, b, f) = (at(0), at(1), at(2), at(3));
            let s = (uv[0] * f[0]).floor() + (uv[1] * f[1]).floor();
            let k = s - 2.0 * (s * 0.5).floor();
            vec![map2(a, b, |a, b| mix(a, b, k))]
        }
        K::Fresnel => {
            let (nn, vv) = (normalize_n(at(0), 3), normalize_n(at(1), 3));
            let d = dot_n(nn, vv, 3).clamp(0.0, 1.0);
            vec![[(1.0 - d).powf(at(2)[0]); 4]]
        }
        K::Preview => vec![at(0)],
    }
}

/// Does the graph read Time (its previews animate)?
pub fn uses_time(g: &ShaderGraph) -> bool {
    g.nodes.iter().any(|n| n.kind == NodeKind::Time)
}

/// Does the node (or anything feeding it) read 3D geometry? Its preview is then a sphere.
pub fn wants_sphere(g: &ShaderGraph, node: u32) -> bool {
    let mut stack = vec![node];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if g.node(id).is_some_and(|n| n.kind.is_3d_input()) {
            return true;
        }
        stack.extend(g.connections.iter().filter(|c| c.to == id).map(|c| c.from));
    }
    false
}
