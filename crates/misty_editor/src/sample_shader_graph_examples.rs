//! Shader Graph examples in `SampleProject/Assets/Examples/Shader Graph`, built in code:
//! - **Dissolve Glow**: noise (Gradient or Cells, an Enum keyword) against a threshold (fixed or
//!   animated, a Boolean keyword) through the **Dissolve Edge** Sub Graph: alpha clipping and a
//!   glowing edge band.
//! - **Triplanar Rock**: rock and moss textures projected along the world axes (no UVs), moss on
//!   the faces looking up through a Custom Function.
//! - **Wind Sway**: grass blades bending in the wind by their vertex colour (red = how far a
//!   vertex sways), coloured from root to tip, light through the blades (Transmission). The
//!   blades are a small glTF with vertex colours.
//! - **Puddle Decal**: a Decal graph for a Decal Projector: an irregular wet patch, ripples on a
//!   keyword (a Custom Function normal).
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_shader_graph_examples`; the
//! other tests check the files still match, that every variant of every graph compiles and (on a
//! GPU) that the scene renders.

use std::path::{Path, PathBuf};

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    DecalProjector, MeshFilter, MeshRef, MeshRenderer, PrimitiveMesh, Scene, Transform,
};
use misty_engine::shader_asset::PropertyValue;
use misty_engine::shader_graph::{
    FunctionSource, KeywordKind, MASTER, NodeKind, PropertyType, Resolver, ShaderGraph, Slot,
    Space, Surface, Target, TargetMaterial, TextureFallback, ValueType, master,
};

const DIR: &str = "Assets/Examples/Shader Graph";
pub const EDGE: &str = "Assets/Examples/Shader Graph/Dissolve Edge.shadersubgraph";
pub const DISSOLVE: &str = "Assets/Examples/Shader Graph/Dissolve Glow.shadergraph";
pub const ROCK: &str = "Assets/Examples/Shader Graph/Triplanar Rock.shadergraph";
pub const WIND: &str = "Assets/Examples/Shader Graph/Wind Sway.shadergraph";
pub const PUDDLE: &str = "Assets/Examples/Shader Graph/Puddle Decal.shadergraph";
const PLANT: &str = "Assets/Examples/Shader Graph/Wind Plant.gltf";
const SCENE: &str = "Assets/Examples/Shader Graph/ShaderGraphExamples.scene";
const DISSOLVE_MAT: &str = "Assets/Examples/Shader Graph/Materials/Dissolve Glow.mat";
const DISSOLVE_CELLS_MAT: &str = "Assets/Examples/Shader Graph/Materials/Dissolve Cells.mat";
const ROCK_MAT: &str = "Assets/Examples/Shader Graph/Materials/Triplanar Rock.mat";
const WIND_MAT: &str = "Assets/Examples/Shader Graph/Materials/Wind Sway.mat";
const PUDDLE_MAT: &str = "Assets/Examples/Shader Graph/Materials/Puddle Decal.mat";
const FLOOR_MAT: &str = "Assets/Examples/Shader Graph/Materials/Examples Floor.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Reads Sub Graphs and files from a project folder (independent of the process-wide root).
pub struct RootResolver(pub PathBuf);

impl Resolver for RootResolver {
    fn graph(&self, path: &str) -> Result<ShaderGraph, String> {
        ShaderGraph::load(&self.0.join(path)).map_err(|e| format!("{path}: {e}"))
    }

    fn text(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(self.0.join(path)).map_err(|e| format!("{path}: {e}"))
    }
}

/// Small graph builder: nodes at canvas positions, connections that must succeed.
struct B {
    g: ShaderGraph,
}

impl B {
    fn new(target: Target) -> Self {
        let mut g = ShaderGraph::new(target.material);
        g.target = target;
        Self { g }
    }

    fn prop(&mut self, name: &str, display: &str, ty: PropertyType, value: [f32; 4]) {
        let mut p = misty_engine::shader_graph::GraphProperty::new(name, display, ty);
        p.value = value;
        self.g.properties.push(p);
    }

    fn texture(&mut self, name: &str, display: &str, fallback: TextureFallback, path: &str) {
        let mut p = misty_engine::shader_graph::GraphProperty::new(
            name,
            display,
            PropertyType::Texture2D { fallback },
        );
        p.texture = Some(path.into());
        self.g.properties.push(p);
    }

    fn keyword(&mut self, name: &str, display: &str, kind: KeywordKind, default: u32) {
        self.g.keywords.push(misty_engine::shader_graph::Keyword {
            name: name.into(),
            display: display.into(),
            kind,
            default,
        });
    }

    fn node(&mut self, kind: NodeKind, x: f32, y: f32) -> u32 {
        self.g.add_node(kind, [x, y])
    }

    fn read(&mut self, name: &str, x: f32, y: f32) -> u32 {
        self.node(NodeKind::Property { name: name.into() }, x, y)
    }

    fn set(&mut self, id: u32, port: &str, v: [f32; 4]) {
        if let Some(n) = self.g.node_mut(id) {
            n.values.insert(port.into(), v);
        }
    }

    fn link(&mut self, a: u32, out: &str, b: u32, input: &str) {
        self.g
            .connect(a, out, b, input)
            .unwrap_or_else(|e| panic!("{a}.{out} -> {b}.{input}: {e}"));
    }

    fn master_at(&mut self, x: f32, y: f32) {
        if let Some(m) = self.g.node_mut(MASTER) {
            m.position = [x, y];
        }
    }

    fn note(&mut self, x: f32, y: f32, w: f32, h: f32, text: &str) {
        self.g.notes.push(misty_engine::shader_graph::StickyNote {
            position: [x, y],
            size: [w, h],
            text: text.into(),
        });
    }
}

/// Sub Graph: Noise, Threshold and Width in; Alpha (clip at 0.5) and Edge (the glowing band) out.
pub fn dissolve_edge() -> ShaderGraph {
    let mut b = B {
        g: ShaderGraph::new_sub_graph(),
    };
    b.g.outputs = vec![
        Slot::new("Alpha", ValueType::Float),
        Slot::new("Edge", ValueType::Float),
    ];
    b.prop("_Noise", "Noise", PropertyType::Float, [0.5; 4]);
    b.prop("_Threshold", "Threshold", PropertyType::Float, [0.5; 4]);
    b.prop("_Width", "Width", PropertyType::Float, [0.06; 4]);
    let noise = b.read("_Noise", -560.0, -60.0);
    let threshold = b.read("_Threshold", -560.0, 20.0);
    let width = b.read("_Width", -560.0, 140.0);
    let d = b.node(NodeKind::Subtract, -360.0, -40.0);
    b.link(noise, "Out", d, "A");
    b.link(threshold, "Out", d, "B");
    let alpha = b.node(NodeKind::Add, -140.0, -120.0);
    b.link(d, "Out", alpha, "A");
    b.set(alpha, "B", [0.5; 4]);
    b.link(alpha, "Out", MASTER, "Alpha");
    let band = b.node(NodeKind::Smoothstep, -140.0, 80.0);
    b.set(band, "Edge1", [0.0; 4]);
    b.link(width, "Out", band, "Edge2");
    b.link(d, "Out", band, "In");
    let edge = b.node(NodeKind::OneMinus, 80.0, 80.0);
    b.link(band, "Out", edge, "In");
    b.link(edge, "Out", MASTER, "Edge");
    b.master_at(300.0, -40.0);
    b.g.group("Edge band", &[band, edge]);
    b.g
}

pub fn dissolve() -> ShaderGraph {
    let mut b = B::new(Target {
        material: TargetMaterial::Lit,
        surface: Surface::Opaque,
        alpha_clip: true,
        two_sided: true,
        ..Default::default()
    });
    b.prop(
        "_BaseColor",
        "Base Color",
        PropertyType::Color,
        [0.72, 0.76, 0.82, 1.0],
    );
    b.prop(
        "_EdgeColor",
        "Edge Color",
        PropertyType::Color,
        [1.0, 0.42, 0.08, 1.0],
    );
    b.prop(
        "_Threshold",
        "Threshold",
        PropertyType::Range { min: 0.0, max: 1.0 },
        [0.45; 4],
    );
    b.prop("_NoiseScale", "Noise Scale", PropertyType::Float, [12.0; 4]);
    b.keyword("_ANIMATE_ON", "Animate", KeywordKind::Boolean, 0);
    b.keyword(
        "_PATTERN",
        "Pattern",
        KeywordKind::Enum {
            entries: vec!["Gradient".into(), "Cells".into()],
        },
        0,
    );
    // Noise: Gradient or Cells.
    let scale = b.read("_NoiseScale", -1180.0, -40.0);
    let gradient = b.node(NodeKind::GradientNoise, -960.0, -160.0);
    b.link(scale, "Out", gradient, "Scale");
    let half = b.node(NodeKind::Multiply, -960.0, 120.0);
    b.link(scale, "Out", half, "A");
    b.set(half, "B", [0.5; 4]);
    let cells = b.node(NodeKind::Voronoi, -740.0, 120.0);
    b.link(half, "Out", cells, "Cell Density");
    let pattern = b.node(
        NodeKind::Keyword {
            name: "_PATTERN".into(),
        },
        -500.0,
        -60.0,
    );
    b.link(gradient, "Out", pattern, "Gradient");
    b.link(cells, "Out", pattern, "Cells");
    // Threshold: the property, or swinging with time.
    let threshold = b.read("_Threshold", -960.0, 360.0);
    let time = b.node(NodeKind::Time, -960.0, 480.0);
    let swing = b.node(NodeKind::Remap, -740.0, 480.0);
    b.link(time, "Sine Time", swing, "In");
    b.set(swing, "Out Min Max", [0.15, 0.8, 0.0, 0.0]);
    let animate = b.node(
        NodeKind::Keyword {
            name: "_ANIMATE_ON".into(),
        },
        -500.0,
        380.0,
    );
    b.link(swing, "Out", animate, "On");
    b.link(threshold, "Out", animate, "Off");
    // The Sub Graph: alpha and edge.
    let edge = b.node(
        NodeKind::SubGraph {
            path: EDGE.into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        },
        -260.0,
        100.0,
    );
    let (inputs, outputs) = dissolve_edge().sub_graph_interface();
    if let Some(NodeKind::SubGraph {
        inputs: i,
        outputs: o,
        ..
    }) = b.g.node_mut(edge).map(|n| &mut n.kind)
    {
        *i = inputs;
        *o = outputs;
    }
    b.link(pattern, "Out", edge, "Noise");
    b.link(animate, "Out", edge, "Threshold");
    b.link(edge, "Alpha", MASTER, master::ALPHA);
    let color = b.read("_BaseColor", -260.0, -260.0);
    b.link(color, "Out", MASTER, master::BASE_COLOR);
    let edge_color = b.read("_EdgeColor", -260.0, 380.0);
    let glow = b.node(NodeKind::Multiply, -20.0, 300.0);
    b.link(edge, "Edge", glow, "A");
    b.link(edge_color, "Out", glow, "B");
    let bright = b.node(NodeKind::Multiply, 180.0, 300.0);
    b.link(glow, "Out", bright, "A");
    b.set(bright, "B", [4.0; 4]);
    b.link(bright, "Out", MASTER, master::EMISSION);
    b.master_at(420.0, -120.0);
    b.g.group("Noise", &[scale, gradient, half, cells, pattern]);
    b.g.group("Threshold", &[threshold, time, swing, animate]);
    b.note(
        -1180.0,
        -330.0,
        300.0,
        110.0,
        "Keywords: Pattern picks the noise, Animate swings the threshold. Each combination is \
         its own shader variant (4 here).",
    );
    b.g
}

pub fn triplanar_rock() -> ShaderGraph {
    let mut b = B::new(Target {
        material: TargetMaterial::Lit,
        ..Default::default()
    });
    b.texture(
        "_RockTex",
        "Rock",
        TextureFallback::Grey,
        "Assets/Textures/TerrainRock.png",
    );
    b.texture(
        "_MossTex",
        "Moss",
        TextureFallback::White,
        "Assets/Decals/Moss.png",
    );
    b.prop("_Tile", "Tiling", PropertyType::Float, [0.6; 4]);
    b.prop(
        "_MossAmount",
        "Moss Amount",
        PropertyType::Range { min: 0.0, max: 1.0 },
        [0.55; 4],
    );
    let rock = b.read("_RockTex", -980.0, -220.0);
    let moss = b.read("_MossTex", -980.0, 60.0);
    let tile = b.read("_Tile", -980.0, -60.0);
    let rock_tp = b.node(
        NodeKind::Triplanar {
            space: Space::World,
        },
        -720.0,
        -240.0,
    );
    b.link(rock, "Out", rock_tp, "Texture");
    b.link(tile, "Out", rock_tp, "Tile");
    let moss_tp = b.node(
        NodeKind::Triplanar {
            space: Space::World,
        },
        -720.0,
        60.0,
    );
    b.link(moss, "Out", moss_tp, "Texture");
    b.link(tile, "Out", moss_tp, "Tile");
    let normal = b.node(
        NodeKind::Normal {
            space: Space::World,
        },
        -720.0,
        340.0,
    );
    let amount = b.read("_MossAmount", -720.0, 460.0);
    let mask = b.node(
        NodeKind::CustomFunction {
            function: "MossMask".into(),
            source: FunctionSource::Inline,
            body: "// Moss on the faces looking up, more with Amount.\n\
                   let start = 1.0 - Amount;\n\
                   Mask = smoothstep(start, start + 0.3, N.y);"
                .into(),
            file: String::new(),
            inputs: vec![
                Slot::new("N", ValueType::Vec3),
                Slot::new("Amount", ValueType::Float),
            ],
            outputs: vec![Slot::new("Mask", ValueType::Float)],
        },
        -460.0,
        360.0,
    );
    b.link(normal, "Out", mask, "N");
    b.link(amount, "Out", mask, "Amount");
    let mix = b.node(NodeKind::Lerp, -200.0, -40.0);
    b.link(rock_tp, "Out", mix, "A");
    b.link(moss_tp, "Out", mix, "B");
    b.link(mask, "Mask", mix, "T");
    b.link(mix, "Out", MASTER, master::BASE_COLOR);
    let smooth = b.node(NodeKind::Lerp, -200.0, 260.0);
    b.set(smooth, "A", [0.32; 4]);
    b.set(smooth, "B", [0.12; 4]);
    b.link(mask, "Mask", smooth, "T");
    b.link(smooth, "Out", MASTER, master::SMOOTHNESS);
    b.master_at(120.0, -120.0);
    b.g.group("Moss on top", &[normal, amount, mask]);
    b.note(
        -980.0,
        -420.0,
        280.0,
        90.0,
        "Triplanar: the textures are projected along X, Y and Z and blended by the normal, so \
         the rock needs no UVs.",
    );
    b.g
}

pub fn wind_sway() -> ShaderGraph {
    let mut b = B::new(Target {
        material: TargetMaterial::Lit,
        two_sided: true,
        ..Default::default()
    });
    b.prop(
        "_RootColor",
        "Root Color",
        PropertyType::Color,
        [0.10, 0.28, 0.06, 1.0],
    );
    b.prop(
        "_TipColor",
        "Tip Color",
        PropertyType::Color,
        [0.62, 0.78, 0.22, 1.0],
    );
    b.prop("_Strength", "Strength", PropertyType::Float, [0.22; 4]);
    b.prop("_Speed", "Speed", PropertyType::Float, [1.8; 4]);
    b.prop(
        "_Wind",
        "Wind Direction",
        PropertyType::Vector,
        [1.0, 0.0, 0.35, 0.0],
    );
    // How far a vertex sways: its colour's red (0 at the root, 1 at the tip).
    let vc = b.node(NodeKind::VertexColor, -1100.0, 0.0);
    let split = b.node(NodeKind::Split, -880.0, 0.0);
    b.link(vc, "Out", split, "In");
    // Phase: time × speed, offset across the world so the blades do not move as one.
    let time = b.node(NodeKind::Time, -1100.0, -300.0);
    let speed = b.read("_Speed", -1100.0, -160.0);
    let t = b.node(NodeKind::Multiply, -880.0, -260.0);
    b.link(time, "Time", t, "A");
    b.link(speed, "Out", t, "B");
    let pos = b.node(
        NodeKind::Position {
            space: Space::World,
        },
        -1100.0,
        -460.0,
    );
    let p = b.node(NodeKind::Split, -880.0, -460.0);
    b.link(pos, "Out", p, "In");
    let across = b.node(NodeKind::Add, -660.0, -440.0);
    b.link(p, "R", across, "A");
    b.link(p, "B", across, "B");
    let phase = b.node(NodeKind::Add, -440.0, -320.0);
    b.link(t, "Out", phase, "A");
    b.link(across, "Out", phase, "B");
    let wave = b.node(NodeKind::Sine, -240.0, -320.0);
    b.link(phase, "Out", wave, "In");
    // Lean into the wind a little, plus the wave.
    let lean = b.node(NodeKind::Add, -40.0, -320.0);
    b.link(wave, "Out", lean, "A");
    b.set(lean, "B", [0.6; 4]);
    let strength = b.read("_Strength", -40.0, -160.0);
    let amount = b.node(NodeKind::Multiply, 160.0, -280.0);
    b.link(lean, "Out", amount, "A");
    b.link(strength, "Out", amount, "B");
    let weighted = b.node(NodeKind::Multiply, 360.0, -240.0);
    b.link(amount, "Out", weighted, "A");
    b.link(split, "R", weighted, "B");
    let wind = b.read("_Wind", 360.0, -420.0);
    let offset = b.node(NodeKind::Multiply, 560.0, -320.0);
    b.link(wind, "Out", offset, "A");
    b.link(weighted, "Out", offset, "B");
    b.link(offset, "Out", MASTER, master::POSITION);
    // Colour root to tip; light comes through the blades.
    let root = b.read("_RootColor", -660.0, 120.0);
    let tip = b.read("_TipColor", -660.0, 220.0);
    let color = b.node(NodeKind::Lerp, -440.0, 140.0);
    b.link(root, "Out", color, "A");
    b.link(tip, "Out", color, "B");
    b.link(split, "R", color, "T");
    b.link(color, "Out", MASTER, master::BASE_COLOR);
    let through = b.node(NodeKind::Multiply, -200.0, 260.0);
    b.link(tip, "Out", through, "A");
    b.set(through, "B", [0.35; 4]);
    b.link(through, "Out", MASTER, master::TRANSMISSION);
    b.set(MASTER, master::SMOOTHNESS, [0.3; 4]);
    b.master_at(800.0, -200.0);
    b.g.group(
        "Sway",
        &[
            time, speed, t, pos, p, across, phase, wave, lean, strength, amount,
        ],
    );
    b.note(
        -1100.0,
        180.0,
        300.0,
        100.0,
        "Vertex colour red says how far each vertex sways: 0 at the root, 1 at the tip (painted \
         in the model).",
    );
    b.g
}

pub fn puddle_decal() -> ShaderGraph {
    let mut b = B::new(Target {
        material: TargetMaterial::Decal,
        ..Default::default()
    });
    b.prop(
        "_WaterColor",
        "Water Color",
        PropertyType::Color,
        [0.16, 0.18, 0.2, 1.0],
    );
    b.prop(
        "_Edge",
        "Edge Softness",
        PropertyType::Range {
            min: 0.01,
            max: 0.4,
        },
        [0.12; 4],
    );
    b.keyword("_RIPPLES_ON", "Ripples", KeywordKind::Boolean, 1);
    // An irregular round patch: distance from the centre pushed around by noise.
    let uv = b.node(NodeKind::Uv, -1000.0, -40.0);
    let centred = b.node(NodeKind::Subtract, -800.0, -40.0);
    b.link(uv, "Out", centred, "A");
    b.set(centred, "B", [0.5; 4]);
    let dist = b.node(NodeKind::Length, -600.0, -40.0);
    b.link(centred, "Out", dist, "In");
    let noise = b.node(NodeKind::GradientNoise, -800.0, 140.0);
    b.set(noise, "Scale", [5.0; 4]);
    let wobble = b.node(NodeKind::Remap, -600.0, 140.0);
    b.link(noise, "Out", wobble, "In");
    b.set(wobble, "In Min Max", [0.0, 1.0, 0.0, 0.0]);
    b.set(wobble, "Out Min Max", [-0.1, 0.1, 0.0, 0.0]);
    let shaped = b.node(NodeKind::Add, -400.0, 40.0);
    b.link(dist, "Out", shaped, "A");
    b.link(wobble, "Out", shaped, "B");
    let edge = b.read("_Edge", -600.0, 300.0);
    let inner = b.node(NodeKind::Subtract, -400.0, 260.0);
    b.set(inner, "A", [0.36; 4]);
    b.link(edge, "Out", inner, "B");
    let fade = b.node(NodeKind::Smoothstep, -200.0, 120.0);
    b.link(inner, "Out", fade, "Edge1");
    b.set(fade, "Edge2", [0.36; 4]);
    b.link(shaped, "Out", fade, "In");
    let alpha = b.node(NodeKind::OneMinus, 0.0, 120.0);
    b.link(fade, "Out", alpha, "In");
    b.link(alpha, "Out", MASTER, master::ALPHA);
    let water = b.read("_WaterColor", 0.0, -160.0);
    b.link(water, "Out", MASTER, master::BASE_COLOR);
    b.set(MASTER, master::SMOOTHNESS, [0.95; 4]);
    // Ripples (a keyword): rings moving out, as a tangent-space normal.
    let time = b.node(NodeKind::Time, -800.0, 440.0);
    let ripple = b.node(
        NodeKind::CustomFunction {
            function: "Ripple".into(),
            source: FunctionSource::Inline,
            body: "let p = UV - vec2<f32>(0.5);\n\
                   let d = max(length(p), 1e-3);\n\
                   let w = sin(d * 70.0 - T * 5.0) * 0.18 * saturate(1.0 - d * 2.4);\n\
                   N = normalize(vec3<f32>(p / d * w, 1.0));"
                .into(),
            file: String::new(),
            inputs: vec![
                Slot::new("UV", ValueType::Vec2),
                Slot::new("T", ValueType::Float),
            ],
            outputs: vec![Slot::new("N", ValueType::Vec3)],
        },
        -560.0,
        440.0,
    );
    b.link(uv, "Out", ripple, "UV");
    b.link(time, "Time", ripple, "T");
    let ripples = b.node(
        NodeKind::Keyword {
            name: "_RIPPLES_ON".into(),
        },
        -260.0,
        440.0,
    );
    b.link(ripple, "N", ripples, "On");
    b.set(ripples, "Off", [0.0, 0.0, 1.0, 0.0]);
    b.link(ripples, "Out", MASTER, master::NORMAL);
    b.master_at(260.0, -60.0);
    b.g.group("Ripples", &[time, ripple, ripples]);
    b.note(
        -1000.0,
        -260.0,
        300.0,
        100.0,
        "Decal target: assign a material using this graph to a Decal Projector. It paints the \
         surfaces in the projector's box.",
    );
    b.g
}

/// Every example graph (the Sub Graph first).
pub fn graphs() -> Vec<(&'static str, ShaderGraph)> {
    vec![
        (EDGE, dissolve_edge()),
        (DISSOLVE, dissolve()),
        (ROCK, triplanar_rock()),
        (WIND, wind_sway()),
        (PUDDLE, puddle_decal()),
    ]
}

fn graph_material(graph: &ShaderGraph, shader: &str) -> Material {
    Material {
        shader: shader.into(),
        properties: graph.default_values(),
        ..Default::default()
    }
}

pub fn materials() -> Vec<(&'static str, Material)> {
    let mut cells = graph_material(&dissolve(), DISSOLVE);
    cells
        .properties
        .insert("_PATTERN".into(), PropertyValue::Float(1.0));
    cells
        .properties
        .insert("_ANIMATE_ON".into(), PropertyValue::Float(1.0));
    cells.properties.insert(
        "_EdgeColor".into(),
        PropertyValue::Vector([0.2, 0.7, 1.0, 1.0]),
    );
    vec![
        (DISSOLVE_MAT, graph_material(&dissolve(), DISSOLVE)),
        (DISSOLVE_CELLS_MAT, cells),
        (ROCK_MAT, graph_material(&triplanar_rock(), ROCK)),
        (WIND_MAT, graph_material(&wind_sway(), WIND)),
        (PUDDLE_MAT, graph_material(&puddle_decal(), PUDDLE)),
        (
            FLOOR_MAT,
            Material {
                color: [0.42, 0.4, 0.37, 1.0],
                smoothness: 0.2,
                ..Default::default()
            },
        ),
    ]
}

/// Standard base64 (for the glTF's embedded buffer).
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A clump of grass blades as a glTF: each blade a tapering strip of four segments, its vertex
/// colour's red 0 at the root and 1 at the tip (how far the Wind Sway graph bends it).
pub fn wind_plant_gltf() -> String {
    let (mut pos, mut nrm, mut col, mut idx): (Vec<f32>, Vec<f32>, Vec<f32>, Vec<u32>) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let blades = 9;
    let segments = 4;
    for k in 0..blades {
        let a = k as f32 * 2.399_963; // golden angle
        let r = 0.05 + 0.03 * (k % 3) as f32;
        let base = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
        let facing = a + 1.2;
        let side = Vec3::new(facing.cos(), 0.0, facing.sin());
        let normal = Vec3::new(-facing.sin(), 0.0, facing.cos());
        let height = 0.9 + 0.12 * (k % 4) as f32;
        let lean = Vec3::new(a.cos(), 0.0, a.sin()) * 0.18;
        let first = (pos.len() / 3) as u32;
        for s in 0..=segments {
            let f = s as f32 / segments as f32;
            let width = 0.05 * (1.0 - f * 0.85);
            let centre = base + Vec3::Y * (height * f) + lean * (f * f);
            for sign in [-1.0f32, 1.0] {
                let p = centre + side * width * sign;
                pos.extend([p.x, p.y, p.z]);
                nrm.extend([normal.x, normal.y, normal.z]);
                col.extend([f, 0.6, 0.2, 1.0]);
            }
        }
        for s in 0..segments {
            let i = first + s * 2;
            idx.extend([i, i + 1, i + 2, i + 1, i + 3, i + 2]);
        }
    }
    let count = pos.len() / 3;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in pos.chunks(3) {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let mut bytes: Vec<u8> = Vec::new();
    for v in pos.iter().chain(&nrm).chain(&col) {
        bytes.extend(v.to_le_bytes());
    }
    for i in &idx {
        bytes.extend(i.to_le_bytes());
    }
    let (p_len, n_len, c_len) = (count * 12, count * 12, count * 16);
    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "Misty Engine sample generator" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "name": "Wind Plant" }],
        "meshes": [{
            "name": "Wind Plant",
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1, "COLOR_0": 2 },
                "indices": 3
            }]
        }],
        "buffers": [{
            "byteLength": bytes.len(),
            "uri": format!("data:application/octet-stream;base64,{}", base64(&bytes))
        }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": p_len, "target": 34962 },
            { "buffer": 0, "byteOffset": p_len, "byteLength": n_len, "target": 34962 },
            { "buffer": 0, "byteOffset": p_len + n_len, "byteLength": c_len, "target": 34962 },
            { "buffer": 0, "byteOffset": p_len + n_len + c_len, "byteLength": idx.len() * 4, "target": 34963 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": count, "type": "VEC3", "min": lo, "max": hi },
            { "bufferView": 1, "componentType": 5126, "count": count, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": count, "type": "VEC4" },
            { "bufferView": 3, "componentType": 5125, "count": idx.len(), "type": "SCALAR" }
        ]
    });
    serde_json::to_string_pretty(&json).expect("json")
}

fn object(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    mesh: MeshRef,
    t: Transform,
    material: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter { mesh: Some(mesh) });
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

fn trs(position: Vec3, euler: Vec3, scale: Vec3) -> Transform {
    let mut t = Transform::from_position(position).with_euler(euler);
    t.scale = scale;
    t
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

pub fn examples_scene() -> Scene {
    let mut s = Scene::default_3d("ShaderGraphExamples");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.6, -6.4))
                .with_euler(Vec3::new(18.0, 0.0, 0.0)),
        );
    }
    let prim = MeshRef::Primitive;
    object(
        &mut s,
        "Floor",
        None,
        prim(PrimitiveMesh::Plane),
        scaled(Vec3::ZERO, Vec3::new(1.4, 1.0, 1.0)),
        FLOOR_MAT,
    );
    object(
        &mut s,
        "Dissolve Sphere",
        None,
        prim(PrimitiveMesh::Sphere),
        scaled(Vec3::new(-2.6, 0.75, 0.6), Vec3::splat(1.5)),
        DISSOLVE_MAT,
    );
    object(
        &mut s,
        "Dissolve Cells Cube",
        None,
        prim(PrimitiveMesh::Cube),
        trs(
            Vec3::new(-1.0, 0.55, 1.6),
            Vec3::new(0.0, 30.0, 0.0),
            Vec3::splat(1.1),
        ),
        DISSOLVE_CELLS_MAT,
    );
    let rocks = s.spawn("Rocks", None);
    for (i, (p, sc, rot)) in [
        (Vec3::new(0.8, 0.45, 1.0), Vec3::new(1.5, 0.9, 1.2), 15.0),
        (Vec3::new(1.7, 0.3, 0.1), Vec3::new(0.8, 0.6, 0.9), -40.0),
    ]
    .into_iter()
    .enumerate()
    {
        object(
            &mut s,
            &format!("Rock {}", i + 1),
            Some(rocks),
            prim(PrimitiveMesh::Sphere),
            trs(p, Vec3::new(0.0, rot, 8.0), sc),
            ROCK_MAT,
        );
    }
    let plants = s.spawn("Wind Plants", None);
    for (i, p) in [
        Vec3::new(2.9, 0.0, 1.4),
        Vec3::new(3.3, 0.0, 0.6),
        Vec3::new(2.5, 0.0, 0.2),
        Vec3::new(3.6, 0.0, 1.7),
    ]
    .into_iter()
    .enumerate()
    {
        object(
            &mut s,
            &format!("Wind Plant {}", i + 1),
            Some(plants),
            MeshRef::Asset(format!("{PLANT}#0")),
            Transform::from_position(p).with_euler(Vec3::new(0.0, i as f32 * 70.0, 0.0)),
            WIND_MAT,
        );
    }
    let decal = s.spawn("Puddle Decal", None);
    s.set_transform(
        decal,
        Transform::from_position(Vec3::new(-0.2, 0.6, -0.9)).with_euler(Vec3::new(90.0, 0.0, 0.0)),
    );
    s.add(
        decal,
        DecalProjector {
            size: [2.2, 1.6, 1.4],
            material: Some(PUDDLE_MAT.into()),
            ..Default::default()
        },
    );
    s
}

/// A `.meta` with a GUID from the path (stable across regenerations) unless one exists.
fn ensure_meta(root: &Path, rel: &str) {
    use std::hash::{Hash, Hasher};
    let meta = root.join(format!("{rel}.meta"));
    if meta.exists() {
        return;
    }
    let mut a = std::collections::hash_map::DefaultHasher::new();
    ("misty-sample", rel).hash(&mut a);
    let mut b = std::collections::hash_map::DefaultHasher::new();
    (rel, "misty-sample-2").hash(&mut b);
    let guid = format!("{:016x}{:016x}", a.finish(), b.finish());
    std::fs::write(meta, format!("{{\n  \"guid\": \"{guid}\"\n}}\n")).unwrap();
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_shader_graph_examples() {
    let root = project();
    std::fs::create_dir_all(root.join(DIR).join("Materials")).unwrap();
    for (path, g) in graphs() {
        g.save(&root.join(path)).unwrap();
        ensure_meta(&root, path);
    }
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
        ensure_meta(&root, path);
    }
    std::fs::write(root.join(PLANT), wind_plant_gltf()).unwrap();
    ensure_meta(&root, PLANT);
    examples_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root, SCENE);
    ensure_meta(&root, "Assets/Examples");
    ensure_meta(&root, DIR);
    ensure_meta(&root, &format!("{DIR}/Materials"));
}

#[test]
fn shader_graph_examples_match_their_generator() {
    let root = project();
    for (path, g) in graphs() {
        let text =
            std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        crate::test_util::assert_close(&ShaderGraph::from_json(&text).unwrap(), &g, path);
    }
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    assert_eq!(
        std::fs::read_to_string(root.join(PLANT)).unwrap(),
        wind_plant_gltf(),
        "regenerate with write_shader_graph_examples"
    );
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &examples_scene().to_json(),
        "regenerate with write_shader_graph_examples",
    );
    let (_, report) = Scene::from_json("ShaderGraphExamples", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// Each example compiles in every variant, from the files on disk.
#[test]
fn every_shader_graph_example_compiles() {
    let root = project();
    let resolver = RootResolver(root.clone());
    let mut variants = Vec::new();
    for (path, _) in graphs() {
        let g = ShaderGraph::load(&root.join(path)).unwrap();
        let n = misty_engine::shader_graph::compile_all(&g, &resolver)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        variants.push((path, n));
    }
    assert_eq!(
        variants,
        [(EDGE, 1), (DISSOLVE, 4), (ROCK, 1), (WIND, 1), (PUDDLE, 2)]
    );
}

#[test]
fn dissolve_example_uses_its_sub_graph_and_keywords() {
    let root = project();
    let resolver = RootResolver(root.clone());
    let g = ShaderGraph::load(&root.join(DISSOLVE)).unwrap();
    let cells = misty_engine::shader_graph::Variant::from([
        ("_PATTERN".to_owned(), 1),
        ("_ANIMATE_ON".to_owned(), 1),
    ]);
    let src = misty_engine::shader_graph::compile_variant(&g, &cells, &resolver)
        .unwrap()
        .source;
    assert!(src.contains("sg_voronoi") && !src.contains("sg_gradient_noise(input.uv"));
    assert!(src.contains("globals.time.y"), "animated threshold");
    let plain =
        misty_engine::shader_graph::compile_variant(&g, &Default::default(), &resolver).unwrap();
    assert!(
        plain.source.contains("sg_gradient_noise(input.uv")
            && !plain.source.contains("sg_voronoi(input")
    );
    assert_eq!(plain.deps, [EDGE]);
    // The Wind Sway example offsets vertices by their colour and lets light through.
    let wind = ShaderGraph::load(&root.join(WIND)).unwrap();
    let src = misty_engine::shader_graph::compile_variant(&wind, &Default::default(), &resolver)
        .unwrap()
        .source;
    assert!(src.contains("v.color") && src.contains("o.transmission"));
}

#[test]
fn wind_plant_imports_with_vertex_colours() {
    let dir = std::env::temp_dir().join(format!("misty_plant_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Plant.gltf");
    std::fs::write(&file, wind_plant_gltf()).unwrap();
    let model = misty_engine::models::load(&file).unwrap_or_else(|e| panic!("{e}"));
    let mesh = &model.meshes[0].data;
    assert_eq!(mesh.colors.len(), mesh.vertices.len());
    let (lo, hi) = mesh
        .colors
        .iter()
        .fold((255u8, 0u8), |(lo, hi), c| (lo.min(c[0]), hi.max(c[0])));
    assert!(lo < 5 && hi > 250, "red runs root to tip: {lo}..{hi}");
    assert_eq!(base64(b"Ma"), "TWE=");
    assert_eq!(base64(b"Man"), "TWFu");
    std::fs::remove_dir_all(dir).ok();
}

/// On a GPU: the examples render without the error shader, the decal paints the floor and the
/// plants take their vertex colours.
#[test]
fn shader_graph_examples_render() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    misty_engine::assets::set_project_root(&root);
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    // The plant model by absolute path too: tests running alongside move the project root.
    let plant = root.join(PLANT).to_string_lossy().replace('\\', "/");
    for f in scene.world.query::<&mut MeshFilter>().iter() {
        if let Some(MeshRef::Asset(key)) = &mut f.mesh
            && key.starts_with(PLANT)
        {
            *key = format!("{plant}#0");
        }
    }
    misty_engine::models::model(&plant).unwrap_or_else(|e| panic!("{plant}: {e}"));
    let mut assets = misty_engine::assets::AssetCache::open(&root);
    // Absolute paths (see shader_graph_demo_renders): the graphs load now, while the project
    // root is this one.
    for m in assets.materials.values_mut() {
        if m.is_custom() {
            let abs = root.join(&m.shader).to_string_lossy().replace('\\', "/");
            misty_engine::shader_asset::reload(&abs);
            misty_engine::shader_asset::load(&abs).unwrap_or_else(|e| panic!("{abs}: {e}"));
            m.shader = abs;
        }
    }
    let mut view = misty_engine::render_scene::GameView::new(&scene, &assets).unwrap();
    for d in &mut view.environment.decals {
        if let Some(m) = &d.decal.material {
            d.decal.material = Some(root.join(m).to_string_lossy().replace('\\', "/"));
        }
    }
    let img = crate::sample_render::render(&gpu, &view.frame(), 2);
    if let Some(out) = std::env::var_os("MISTY_SG_EXAMPLES_DUMP") {
        misty_engine::image::save_buffer(
            out,
            &img.pixels,
            crate::sample_render::WIDTH,
            crate::sample_render::HEIGHT,
            misty_engine::image::ColorType::Rgba8,
        )
        .unwrap();
    }
    let px: Vec<[u8; 3]> = img.pixels.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
    let magenta = px
        .iter()
        .filter(|p| p[0] > 200 && p[1] < 60 && p[2] > 200)
        .count();
    assert!(magenta < px.len() / 500, "{magenta} magenta pixels");
    // The puddle darkens the floor where it lies, against the floor beside it.
    let puddle = img.brightness(&view.camera, Vec3::new(-0.2, 0.0, -0.9));
    let dry = img.brightness(&view.camera, Vec3::new(1.2, 0.0, -1.6));
    assert!(puddle < dry * 0.9, "puddle {puddle} vs dry floor {dry}");
    // Grass tips are lighter than their roots (vertex colour red drives the colour).
    let tip = img.brightness(&view.camera, Vec3::new(2.9, 1.0, 1.4));
    let root_px = img.brightness(&view.camera, Vec3::new(2.9, 0.05, 1.4));
    assert!(tip > root_px, "tip {tip} vs root {root_px}");
}
