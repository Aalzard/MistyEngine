//! ShaderGraphDemo sample in `SampleProject`: three Shader Graph assets built in code and the
//! scene showing them. **Dissolve** (gradient noise against an animated threshold with alpha
//! clipping, and a glowing edge band), **Hologram** (unlit, transparent: fresnel rim plus scrolling
//! scan lines) and **Water Ripple** (vertex offset: concentric sine waves on a tiled pool, coloured
//! by wave height). Regenerate with
//! `cargo test -p misty_editor -- --ignored write_shader_graph_demo`; the other tests check the
//! committed files still match, that the graphs compile, and (on a GPU) that the demo renders.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Transform};
use misty_engine::shader_asset::PropertyValue;
use misty_engine::shader_graph::{
    MASTER, NodeKind, PropertyType, ShaderGraph, Space, Surface, Target, TargetMaterial, master,
};

const SCENE: &str = "Assets/Scenes/ShaderGraphDemo.scene";
pub const DISSOLVE: &str = "Assets/Shaders/Dissolve.shadergraph";
pub const HOLOGRAM: &str = "Assets/Shaders/Hologram.shadergraph";
pub const WATER: &str = "Assets/Shaders/Water Ripple.shadergraph";
const DISSOLVE_MAT: &str = "Assets/Materials/Dissolve.mat";
const HOLOGRAM_MAT: &str = "Assets/Materials/Hologram Graph.mat";
const WATER_MAT: &str = "Assets/Materials/Water Ripple.mat";
const FLOOR_MAT: &str = "Assets/Materials/Graph Demo Floor.mat";
const PLINTH_MAT: &str = "Assets/Materials/Graph Demo Plinth.mat";
/// Centre of the ripple pool (the Water Ripple material's `_Center`).
const POOL: Vec3 = Vec3::new(3.4, 0.05, 0.4);

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
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

    fn node(&mut self, kind: NodeKind, x: f32, y: f32) -> u32 {
        self.g.add_node(kind, [x, y])
    }

    fn read(&mut self, name: &str, x: f32, y: f32) -> u32 {
        self.node(NodeKind::Property { name: name.into() }, x, y)
    }

    fn set(&mut self, id: u32, port: &str, v: f32) {
        if let Some(n) = self.g.node_mut(id) {
            n.values.insert(port.into(), [v; 4]);
        }
    }

    fn set_vec(&mut self, id: u32, port: &str, v: [f32; 4]) {
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
}

/// Noise against an (optionally animated) threshold: pixels below it are clipped, the band just
/// above it glows.
pub fn dissolve() -> ShaderGraph {
    let mut b = B::new(Target {
        material: TargetMaterial::Lit,
        surface: Surface::Opaque,
        alpha_clip: true,
        two_sided: true,
    });
    b.prop(
        "_BaseColor",
        "Base Color",
        PropertyType::Color,
        [0.78, 0.8, 0.85, 1.0],
    );
    b.prop(
        "_EdgeColor",
        "Edge Color",
        PropertyType::Color,
        [1.0, 0.45, 0.1, 1.0],
    );
    b.prop(
        "_EdgeIntensity",
        "Edge Intensity",
        PropertyType::Float,
        [6.0; 4],
    );
    b.prop(
        "_EdgeWidth",
        "Edge Width",
        PropertyType::Range { min: 0.0, max: 0.2 },
        [0.06; 4],
    );
    b.prop(
        "_Dissolve",
        "Dissolve",
        PropertyType::Range { min: 0.0, max: 1.0 },
        [0.45; 4],
    );
    b.prop("_Animate", "Animate", PropertyType::Boolean, [1.0; 4]);
    b.prop("_NoiseScale", "Noise Scale", PropertyType::Float, [8.0; 4]);

    let base = b.read("_BaseColor", 50.0, -200.0);
    let noise = b.node(NodeKind::GradientNoise, -790.0, -120.0);
    let scale = b.read("_NoiseScale", -1000.0, -60.0);
    let time = b.node(NodeKind::Time, -1000.0, 220.0);
    let remap = b.node(NodeKind::Remap, -790.0, 120.0);
    b.set_vec(remap, "Out Min Max", [0.15, 0.7, 0.0, 0.0]);
    let amount = b.read("_Dissolve", -790.0, 400.0);
    let animate = b.read("_Animate", -790.0, 480.0);
    let lerp = b.node(NodeKind::Lerp, -580.0, 180.0);
    let width = b.read("_EdgeWidth", -580.0, 460.0);
    let add = b.node(NodeKind::Add, -370.0, 260.0);
    let step = b.node(NodeKind::Step, -160.0, 60.0);
    let edge = b.read("_EdgeColor", -160.0, -60.0);
    let tint = b.node(NodeKind::Multiply, 50.0, 0.0);
    let intensity = b.read("_EdgeIntensity", 50.0, 260.0);
    let glow = b.node(NodeKind::Multiply, 260.0, 60.0);

    b.link(base, "Out", MASTER, master::BASE_COLOR);
    b.link(scale, "Out", noise, "Scale");
    b.link(noise, "Out", MASTER, master::ALPHA);
    b.link(time, "Sine Time", remap, "In");
    b.link(amount, "Out", lerp, "A");
    b.link(remap, "Out", lerp, "B");
    b.link(animate, "Out", lerp, "T");
    b.link(lerp, "Out", MASTER, master::ALPHA_CLIP);
    b.link(lerp, "Out", add, "A");
    b.link(width, "Out", add, "B");
    b.link(noise, "Out", step, "Edge");
    b.link(add, "Out", step, "In");
    b.link(step, "Out", tint, "A");
    b.link(edge, "Out", tint, "B");
    b.link(tint, "Out", glow, "A");
    b.link(intensity, "Out", glow, "B");
    b.link(glow, "Out", MASTER, master::EMISSION);
    b.set(MASTER, master::METALLIC, 0.1);
    b.set(MASTER, master::SMOOTHNESS, 0.55);
    b.master_at(480.0, -200.0);
    b.g.notes.push(misty_engine::shader_graph::StickyNote {
        position: [-1000.0, -300.0],
        size: [300.0, 120.0],
        text: "Pixels whose noise is below the threshold are clipped; the band just above it \
               glows. Turn off Animate to drive Dissolve from a script or the Inspector."
            .into(),
    });
    b.g
}

/// Unlit and transparent: a fresnel rim plus scan lines scrolling up in world space.
pub fn hologram() -> ShaderGraph {
    let mut b = B::new(Target {
        material: TargetMaterial::Unlit,
        surface: Surface::Transparent,
        alpha_clip: false,
        two_sided: false,
    });
    b.prop(
        "_Color",
        "Color",
        PropertyType::Color,
        [0.25, 0.85, 1.0, 1.0],
    );
    b.prop(
        "_FresnelPower",
        "Fresnel Power",
        PropertyType::Float,
        [2.0; 4],
    );
    b.prop(
        "_LineDensity",
        "Line Density",
        PropertyType::Float,
        [24.0; 4],
    );
    b.prop(
        "_ScrollSpeed",
        "Scroll Speed",
        PropertyType::Float,
        [0.5; 4],
    );
    b.prop("_Intensity", "Intensity", PropertyType::Float, [2.5; 4]);

    let pos = b.node(
        NodeKind::Position {
            space: Space::World,
        },
        -1120.0,
        120.0,
    );
    let split = b.node(NodeKind::Split, -880.0, 120.0);
    let density = b.read("_LineDensity", -880.0, 360.0);
    let lines = b.node(NodeKind::Multiply, -640.0, 160.0);
    let time = b.node(NodeKind::Time, -1120.0, 440.0);
    let speed = b.read("_ScrollSpeed", -1120.0, 620.0);
    let scroll = b.node(NodeKind::Multiply, -880.0, 480.0);
    let sub = b.node(NodeKind::Subtract, -400.0, 260.0);
    let frac = b.node(NodeKind::Fraction, -160.0, 260.0);
    let ramp = b.node(NodeKind::Smoothstep, 80.0, 260.0);
    b.set(ramp, "Edge1", 0.5);
    let dim = b.node(NodeKind::Multiply, 320.0, 260.0);
    b.set(dim, "B", 0.6);
    let fresnel = b.node(NodeKind::Fresnel, -160.0, -120.0);
    let power = b.read("_FresnelPower", -400.0, -60.0);
    let sum = b.node(NodeKind::Add, 540.0, 40.0);
    let glow = b.node(NodeKind::Add, 760.0, 40.0);
    b.set(glow, "B", 0.12);
    let alpha = b.node(NodeKind::Saturate, 980.0, 140.0);
    let color = b.read("_Color", 760.0, -200.0);
    let tint = b.node(NodeKind::Multiply, 980.0, -160.0);
    let intensity = b.read("_Intensity", 980.0, -320.0);
    let bright = b.node(NodeKind::Multiply, 1200.0, -200.0);

    b.link(pos, "Out", split, "In");
    b.link(split, "G", lines, "A");
    b.link(density, "Out", lines, "B");
    b.link(time, "Time", scroll, "A");
    b.link(speed, "Out", scroll, "B");
    b.link(lines, "Out", sub, "A");
    b.link(scroll, "Out", sub, "B");
    b.link(sub, "Out", frac, "In");
    b.link(frac, "Out", ramp, "In");
    b.link(ramp, "Out", dim, "A");
    b.link(power, "Out", fresnel, "Power");
    b.link(fresnel, "Out", sum, "A");
    b.link(dim, "Out", sum, "B");
    b.link(sum, "Out", glow, "A");
    b.link(glow, "Out", alpha, "In");
    b.link(alpha, "Out", MASTER, master::ALPHA);
    b.link(color, "Out", tint, "A");
    b.link(glow, "Out", tint, "B");
    b.link(tint, "Out", bright, "A");
    b.link(intensity, "Out", bright, "B");
    b.link(bright, "Out", MASTER, master::BASE_COLOR);
    b.master_at(1440.0, -120.0);
    b.g
}

/// Concentric sine waves moving the vertices up and down (world space, so a pool of tiles
/// ripples as one surface), the colour following the wave height.
pub fn water_ripple() -> ShaderGraph {
    let mut b = B::new(Target::default());
    b.prop(
        "_DeepColor",
        "Deep Color",
        PropertyType::Color,
        [0.03, 0.2, 0.33, 1.0],
    );
    b.prop(
        "_CrestColor",
        "Crest Color",
        PropertyType::Color,
        [0.45, 0.82, 0.9, 1.0],
    );
    b.prop("_Amplitude", "Amplitude", PropertyType::Float, [0.06; 4]);
    b.prop("_Frequency", "Frequency", PropertyType::Float, [9.0; 4]);
    b.prop("_Speed", "Speed", PropertyType::Float, [3.0; 4]);
    b.prop("_Center", "Center", PropertyType::Vector, [0.0; 4]);

    let pos = b.node(
        NodeKind::Position {
            space: Space::World,
        },
        -1360.0,
        0.0,
    );
    let center = b.read("_Center", -1360.0, 140.0);
    let rel = b.node(NodeKind::Subtract, -1120.0, 40.0);
    let split = b.node(NodeKind::Split, -880.0, 40.0);
    let flat = b.node(NodeKind::Combine, -640.0, 40.0);
    let dist = b.node(NodeKind::Length, -400.0, 40.0);
    let freq = b.read("_Frequency", -400.0, 220.0);
    let phase = b.node(NodeKind::Multiply, -160.0, 60.0);
    let time = b.node(NodeKind::Time, -400.0, 320.0);
    let speed = b.read("_Speed", -400.0, 500.0);
    let motion = b.node(NodeKind::Multiply, -160.0, 380.0);
    let sub = b.node(NodeKind::Subtract, 80.0, 160.0);
    let wave = b.node(NodeKind::Sine, 320.0, 160.0);
    let amp = b.read("_Amplitude", 320.0, 340.0);
    let height = b.node(NodeKind::Multiply, 560.0, 220.0);
    let offset = b.node(NodeKind::Vector3, 800.0, 220.0);
    let remap = b.node(NodeKind::Remap, 560.0, -160.0);
    let deep = b.read("_DeepColor", 560.0, -360.0);
    let crest = b.read("_CrestColor", 560.0, -280.0);
    let color = b.node(NodeKind::Lerp, 800.0, -260.0);

    b.link(pos, "Out", rel, "A");
    b.link(center, "Out", rel, "B");
    b.link(rel, "Out", split, "In");
    b.link(split, "R", flat, "R");
    b.link(split, "B", flat, "G");
    b.link(flat, "RG", dist, "In");
    b.link(dist, "Out", phase, "A");
    b.link(freq, "Out", phase, "B");
    b.link(time, "Time", motion, "A");
    b.link(speed, "Out", motion, "B");
    b.link(phase, "Out", sub, "A");
    b.link(motion, "Out", sub, "B");
    b.link(sub, "Out", wave, "In");
    b.link(wave, "Out", height, "A");
    b.link(amp, "Out", height, "B");
    b.link(height, "Out", offset, "Y");
    b.link(offset, "Out", MASTER, master::POSITION);
    b.link(wave, "Out", remap, "In");
    b.link(deep, "Out", color, "A");
    b.link(crest, "Out", color, "B");
    b.link(remap, "Out", color, "T");
    b.link(color, "Out", MASTER, master::BASE_COLOR);
    b.set(MASTER, master::SMOOTHNESS, 0.9);
    b.master_at(1060.0, -120.0);
    b.g
}

/// (file, graph) of the demo's graphs.
pub fn graphs() -> [(&'static str, ShaderGraph); 3] {
    [
        (DISSOLVE, dissolve()),
        (HOLOGRAM, hologram()),
        (WATER, water_ripple()),
    ]
}

fn materials() -> Vec<(&'static str, Material)> {
    let graph = |shader: &str| Material {
        shader: shader.into(),
        ..Default::default()
    };
    let mut water = graph(WATER);
    water.properties = BTreeMap::from([(
        "_Center".to_owned(),
        PropertyValue::Vector([POOL.x, 0.0, POOL.z, 0.0]),
    )]);
    vec![
        (DISSOLVE_MAT, graph(DISSOLVE)),
        (HOLOGRAM_MAT, graph(HOLOGRAM)),
        (WATER_MAT, water),
        (
            FLOOR_MAT,
            Material {
                color: [0.2, 0.21, 0.23, 1.0],
                smoothness: 0.35,
                ..Default::default()
            },
        ),
        (
            PLINTH_MAT,
            Material {
                color: [0.12, 0.12, 0.14, 1.0],
                metallic: 0.7,
                smoothness: 0.5,
                ..Default::default()
            },
        ),
    ]
}

fn shape(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    mesh: PrimitiveMesh,
    t: Transform,
    material: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("ShaderGraphDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 3.2, -7.6))
                .with_euler(Vec3::new(18.0, 0.0, 0.0)),
        );
    }
    shape(
        &mut s,
        "Floor",
        None,
        PrimitiveMesh::Plane,
        scaled(Vec3::ZERO, Vec3::new(2.0, 1.0, 2.0)),
        FLOOR_MAT,
    );
    // Dissolve: a sphere and a cube on a plinth.
    let plinth = shape(
        &mut s,
        "Dissolve Plinth",
        None,
        PrimitiveMesh::Cylinder,
        scaled(Vec3::new(-3.2, 0.1, 0.4), Vec3::new(2.2, 0.1, 2.2)),
        PLINTH_MAT,
    );
    let _ = plinth;
    shape(
        &mut s,
        "Dissolve Sphere",
        None,
        PrimitiveMesh::Sphere,
        scaled(Vec3::new(-3.6, 1.1, 0.6), Vec3::splat(1.6)),
        DISSOLVE_MAT,
    );
    shape(
        &mut s,
        "Dissolve Cube",
        None,
        PrimitiveMesh::Cube,
        Transform {
            position: Vec3::new(-2.4, 0.6, -0.3),
            scale: Vec3::splat(0.8),
            ..Default::default()
        }
        .with_euler(Vec3::new(0.0, 30.0, 0.0)),
        DISSOLVE_MAT,
    );
    // Hologram: a figure above a projector.
    shape(
        &mut s,
        "Hologram Projector",
        None,
        PrimitiveMesh::Cylinder,
        scaled(Vec3::new(0.0, 0.1, 0.6), Vec3::new(1.2, 0.1, 1.2)),
        PLINTH_MAT,
    );
    shape(
        &mut s,
        "Hologram",
        None,
        PrimitiveMesh::Capsule,
        scaled(Vec3::new(0.0, 1.35, 0.6), Vec3::new(1.0, 1.0, 1.0)),
        HOLOGRAM_MAT,
    );
    // Water Ripple: a pool of 5×5 small tiles (dense vertices for the waves).
    let pool = s.spawn("Water Ripple Pool", None);
    s.set_transform(pool, Transform::from_position(POOL));
    let tiles = 5;
    let tile = 0.6;
    for z in 0..tiles {
        for x in 0..tiles {
            let at = Vec3::new(
                (x as f32 - (tiles as f32 - 1.0) * 0.5) * tile,
                0.0,
                (z as f32 - (tiles as f32 - 1.0) * 0.5) * tile,
            );
            shape(
                &mut s,
                &format!("Tile {}", z * tiles + x + 1),
                Some(pool),
                PrimitiveMesh::Plane,
                scaled(at, Vec3::new(tile / 10.0, 1.0, tile / 10.0)),
                WATER_MAT,
            );
        }
    }
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_shader_graph_demo() {
    let root = project();
    for (path, g) in graphs() {
        g.save(&root.join(path)).unwrap();
    }
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn shader_graph_demo_matches_its_generator() {
    let root = project();
    for (path, g) in graphs() {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        crate::test_util::assert_close(&ShaderGraph::from_json(&text).unwrap(), &g, path);
    }
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_shader_graph_demo",
    );
    let (_, report) = Scene::from_json("ShaderGraphDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn demo_graphs_compile_with_their_features() {
    use misty_engine::shader_asset::{Blend, Cull, ShaderModel, parse};
    for (path, g) in graphs() {
        let cg = misty_engine::shader_graph::compile(&g).unwrap_or_else(|e| panic!("{path}: {e}"));
        let asset = parse(&cg.source).unwrap();
        match path {
            DISSOLVE => {
                assert!(cg.source.contains("discard;"));
                assert_eq!(asset.settings.cull, Cull::Off);
                assert!(cg.source.contains("sg_gradient_noise"));
            }
            HOLOGRAM => {
                assert_eq!(asset.settings.model, ShaderModel::Unlit);
                assert_eq!(asset.settings.blend, Blend::Alpha);
            }
            _ => assert!(asset.has_vertex, "{path} moves vertices"),
        }
    }
}

/// On a GPU: the demo renders without the magenta error shader and is far from flat.
#[test]
fn shader_graph_demo_renders() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    // Material values resolve against the graphs (by project path) when the view is built.
    misty_engine::assets::set_project_root(&root);
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let assets = misty_engine::assets::AssetCache::open(&root);
    let mut view = misty_engine::render_scene::GameView::new(&scene, &assets).unwrap();
    // Absolute shader paths: independent of the process-wide project root other tests change.
    for d in std::sync::Arc::make_mut(&mut view.meshes) {
        if let Some(c) = &mut d.material.custom {
            let abs = root.join(&c.shader).to_string_lossy().replace('\\', "/");
            misty_engine::shader_asset::load(&abs).unwrap_or_else(|e| panic!("{abs}: {e}"));
            c.shader = abs;
        }
    }
    let img = crate::sample_render::render(&gpu, &view.frame(), 2);
    if let Some(out) = std::env::var_os("MISTY_SG_DUMP") {
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
    assert!(
        magenta < px.len() / 500,
        "{magenta} magenta pixels: a graph failed to compile"
    );
    let lum: Vec<f32> = px
        .iter()
        .map(|p| 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32)
        .collect();
    let mean = lum.iter().sum::<f32>() / lum.len() as f32;
    let var = lum.iter().map(|l| (l - mean) * (l - mean)).sum::<f32>() / lum.len() as f32;
    eprintln!("mean {mean}, std {}", var.sqrt());
    assert!(var.sqrt() > 12.0, "flat image: std {}", var.sqrt());
    // The hologram glows cyan against the dark floor.
    let holo = img.brightness(&view.camera, Vec3::new(0.38, 1.2, 0.6));
    let floor = img.brightness(&view.camera, Vec3::new(1.6, 0.0, -2.0));
    assert!(holo > floor, "hologram {holo} vs floor {floor}");
}
