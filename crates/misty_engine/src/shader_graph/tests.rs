use super::codegen::{Stage, convert, literal};
use super::eval::{self, Context, Fallback};
use super::*;

/// A graph with one property of each type.
fn with_properties() -> ShaderGraph {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    for (label, ty) in PropertyType::MENU {
        g.add_property(label, ty);
    }
    g
}

fn texture_node(g: &mut ShaderGraph) -> u32 {
    g.add_node(
        NodeKind::Property {
            name: "_Texture2D".into(),
        },
        [-400.0, 0.0],
    )
}

/// `kind` wired into the master stack (texture inputs fed by the Texture2D property, the first
/// output into `port`, converted as needed).
fn wired(kind: NodeKind, material: TargetMaterial, port: &str) -> ShaderGraph {
    let mut g = with_properties();
    g.target.material = material;
    let tex = texture_node(&mut g);
    let id = g.add_node(kind.clone(), [-200.0, 0.0]);
    for p in kind.inputs() {
        if p.ty == PortType::TEXTURE {
            g.connect(tex, "Out", id, p.name).unwrap();
        }
    }
    let first = g.outputs(g.node(id).unwrap())[0].name;
    g.connect(id, first, MASTER, port)
        .unwrap_or_else(|e| panic!("{}: {e}", kind.title()));
    g
}

#[test]
fn every_node_compiles_in_both_stages_and_targets() {
    let mut kinds = NodeKind::catalog();
    for p in PropertyType::MENU {
        let name = with_properties()
            .properties
            .iter()
            .find(|q| q.ty == p.1)
            .unwrap()
            .name
            .clone();
        if !matches!(p.1, PropertyType::Texture2D { .. }) {
            kinds.push(NodeKind::Property { name });
        }
    }
    for kind in kinds {
        for (material, port) in [
            (TargetMaterial::Lit, master::BASE_COLOR),
            (TargetMaterial::Unlit, master::ALPHA),
            (TargetMaterial::Lit, master::POSITION),
            (TargetMaterial::Lit, master::NORMAL),
        ] {
            let g = wired(kind.clone(), material, port);
            let cg = compile(&g).unwrap_or_else(|e| {
                let src = generate(&g).map(|g| g.source).unwrap_or_default();
                panic!("{} into {port}: {e}\n{src}", kind.title())
            });
            assert_eq!(
                cg.source.contains("fn vertex("),
                port == master::POSITION,
                "{}",
                kind.title()
            );
        }
    }
}

#[test]
fn target_settings_reach_the_header() {
    let mut g = with_properties();
    g.target = Target {
        material: TargetMaterial::Unlit,
        surface: Surface::Transparent,
        alpha_clip: true,
        two_sided: true,
        ..Default::default()
    };
    let cg = compile(&g).unwrap();
    let asset = crate::shader_asset::parse(&cg.source).unwrap();
    use crate::shader_asset::{Blend, Cull, PropertyKind, ShaderModel};
    assert_eq!(asset.settings.model, ShaderModel::Unlit);
    assert_eq!(asset.settings.blend, Blend::Alpha);
    assert_eq!(asset.settings.cull, Cull::Off);
    assert!(cg.source.contains("discard;"));
    let kinds: Vec<PropertyKind> = asset.properties.iter().map(|p| p.kind).collect();
    assert_eq!(kinds.len(), 6);
    assert!(kinds.contains(&PropertyKind::Toggle));
    assert!(kinds.contains(&PropertyKind::Range(0.0, 1.0)));
    let range = asset.property("_Range").unwrap();
    assert_eq!(range.display, "Range");
    assert_eq!(
        range.default,
        crate::shader_asset::PropertyValue::Float(0.5)
    );
    // Lit + Transparent keeps reflections (premultiplied).
    g.target.material = TargetMaterial::Lit;
    let asset = crate::shader_asset::parse(&compile(&g).unwrap().source).unwrap();
    assert_eq!(asset.settings.blend, Blend::Premultiply);
    assert_eq!(asset.settings.model, ShaderModel::Surface);
}

#[test]
fn dynamic_ports_promote_to_the_widest_input() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let color = g.add_node(
        NodeKind::Color {
            value: [1.0, 0.0, 0.0, 1.0],
        },
        [0.0; 2],
    );
    let uv = g.add_node(NodeKind::Uv, [0.0; 2]);
    let add = g.add_node(NodeKind::Add, [0.0; 2]);
    g.connect(uv, "Out", add, "A").unwrap();
    assert_eq!(g.types().output(add, "Out"), Some(ValueType::Vec2));
    g.connect(color, "Out", add, "B").unwrap();
    assert_eq!(g.types().output(add, "Out"), Some(ValueType::Vec4));
    // Vec4 into a Vec3 master port truncates; float splats; vec2 pads.
    g.connect(add, "Out", MASTER, master::BASE_COLOR).unwrap();
    let src = compile(&g).unwrap().source;
    assert!(src.contains(".xyz"), "{src}");
    assert_eq!(
        convert("x", ValueType::Float, ValueType::Vec3),
        "vec3<f32>(x)"
    );
    assert_eq!(
        convert("v", ValueType::Vec2, ValueType::Vec4),
        "vec4<f32>(v, 0.0, 0.0)"
    );
    assert_eq!(convert("v", ValueType::Vec3, ValueType::Float), "(v).x");
    assert_eq!(
        literal([1.0, 2.0, 3.0, 4.0], ValueType::Vec2),
        "vec2<f32>(1.0f, 2.0f)"
    );
    // The CPU evaluator follows the same rules.
    assert_eq!(
        eval::convert([2.0, 0.0, 0.0, 0.0], ValueType::Float, ValueType::Vec3),
        [2.0; 4]
    );
    assert_eq!(
        eval::convert([1.0, 2.0, 3.0, 4.0], ValueType::Vec4, ValueType::Vec2),
        [1.0, 2.0, 0.0, 0.0]
    );
}

#[test]
fn textures_only_connect_to_textures() {
    let mut g = with_properties();
    let tex = texture_node(&mut g);
    let sample = g.add_node(NodeKind::SampleTexture2D { normal_map: false }, [0.0; 2]);
    let float = g.add_node(NodeKind::Float, [0.0; 2]);
    assert!(g.connect(float, "Out", sample, "Texture").is_err());
    assert!(g.connect(tex, "Out", sample, "UV").is_err());
    assert!(g.connect(tex, "Out", MASTER, master::ALPHA).is_err());
    g.connect(tex, "Out", sample, "Texture").unwrap();
    // An unconnected texture input is an error on that node.
    let lone = g.add_node(NodeKind::SampleTexture2D { normal_map: false }, [0.0; 2]);
    g.connect(lone, "R", MASTER, master::ALPHA).unwrap();
    let e = generate(&g).unwrap_err();
    assert_eq!(e.node, Some(lone));
    assert!(e.message.contains("Texture2D"), "{e}");
}

#[test]
fn loops_are_rejected() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let a = g.add_node(NodeKind::Add, [0.0; 2]);
    let b = g.add_node(NodeKind::Multiply, [0.0; 2]);
    g.connect(a, "Out", b, "A").unwrap();
    let e = g.connect(b, "Out", a, "A").unwrap_err();
    assert!(e.contains("loop"), "{e}");
    assert!(g.connect(a, "Out", a, "B").is_err(), "no self loops");
    // A loop written into the file is reported on a node of the loop.
    g.connections.push(Connection {
        from: b,
        output: "Out".into(),
        to: a,
        input: "B".into(),
    });
    g.connect(b, "Out", MASTER, master::ALPHA).ok();
    let e = generate(&g).unwrap_err();
    assert!(e.node == Some(a) || e.node == Some(b), "{e:?}");
    assert!(g.topo_order().is_err());
}

#[test]
fn connecting_replaces_the_old_input_and_removal_cleans_up() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let a = g.add_node(NodeKind::Float, [0.0; 2]);
    let b = g.add_node(NodeKind::Float, [0.0; 2]);
    g.connect(a, "Out", MASTER, master::ALPHA).unwrap();
    g.connect(b, "Out", MASTER, master::ALPHA).unwrap();
    assert_eq!(g.connections.len(), 1);
    assert_eq!(g.input_connection(MASTER, master::ALPHA).unwrap().from, b);
    assert!(g.remove_node(b));
    assert!(g.connections.is_empty());
    assert!(!g.remove_node(MASTER), "the master stack stays");
}

#[test]
fn graph_files_round_trip() {
    let mut g = with_properties();
    g.target.surface = Surface::Transparent;
    let noise = g.add_node(NodeKind::GradientNoise, [-300.0, 40.0]);
    let pos = g.add_node(
        NodeKind::Position {
            space: Space::Object,
        },
        [-500.0, 0.0],
    );
    g.node_mut(noise)
        .unwrap()
        .values
        .insert("Scale".into(), [25.0; 4]);
    g.connect(noise, "Out", MASTER, master::ALPHA).unwrap();
    g.connect(pos, "Out", MASTER, master::POSITION).unwrap();
    g.notes.push(StickyNote {
        position: [0.0, 200.0],
        size: [200.0, 100.0],
        text: "hello".into(),
    });
    let text = g.to_json();
    assert!(text.contains("\"type\": \"GradientNoise\""), "{text}");
    let back = ShaderGraph::from_json(&text).unwrap();
    assert_eq!(back, g);
    let dir = std::env::temp_dir().join(format!("misty_sg_{}", std::process::id()));
    let path = dir.join("A.shadergraph");
    g.save(&path).unwrap();
    assert_eq!(ShaderGraph::load(&path).unwrap(), g);
    std::fs::remove_dir_all(dir).ok();
    assert!(ShaderGraph::from_json("{}").is_err());
}

#[test]
fn properties_rename_and_remove_with_their_nodes() {
    let mut g = with_properties();
    let n = g.add_node(
        NodeKind::Property {
            name: "_Float".into(),
        },
        [0.0; 2],
    );
    g.rename_property("_Float", "_Speed").unwrap();
    assert_eq!(
        g.node(n).unwrap().kind,
        NodeKind::Property {
            name: "_Speed".into()
        }
    );
    assert!(g.rename_property("_Speed", "9x").is_err());
    assert!(g.rename_property("_Speed", "_Color").is_err());
    assert!(g.rename_property("_Speed", "loop").is_err(), "WGSL keyword");
    assert_eq!(g.unique_property_name("Color"), "_Color_1");
    g.remove_property("_Speed");
    assert!(g.node(n).is_none());
}

#[test]
fn wgsl_errors_land_on_their_node() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let f = g.add_node(NodeKind::Float, [0.0; 2]);
    let one = g.add_node(NodeKind::OneMinus, [0.0; 2]);
    g.connect(f, "Out", one, "In").unwrap();
    g.connect(one, "Out", MASTER, master::SMOOTHNESS).unwrap();
    let cg = compile(&g).unwrap();
    // Break the One Minus line by hand and check the error maps back to it.
    let line = cg
        .source
        .lines()
        .position(|l| l.contains(&format!("let n{one}_0")))
        .unwrap()
        + 1;
    assert_eq!(cg.node_at_line(line), Some(one));
    let broken: String = cg
        .source
        .lines()
        .enumerate()
        .map(|(i, l)| {
            if i + 1 == line {
                format!("{}\n", l.replace("(1.0 -", "(undefined_name -"))
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    let e = crate::render::custom_shader::check(&broken).unwrap_err();
    assert_eq!(cg.node_at_line(e.line), Some(one), "{e}");
    let _ = Stage::Fragment;
}

#[test]
fn cpu_previews_follow_the_nodes() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let uv = g.add_node(NodeKind::Uv, [0.0; 2]);
    let split = g.add_node(NodeKind::Split, [0.0; 2]);
    let step = g.add_node(NodeKind::Step, [0.0; 2]);
    let noise = g.add_node(NodeKind::GradientNoise, [0.0; 2]);
    let fresnel = g.add_node(NodeKind::Fresnel, [0.0; 2]);
    g.connect(uv, "Out", split, "In").unwrap();
    g.connect(split, "R", step, "In").unwrap();
    g.node_mut(step)
        .unwrap()
        .values
        .insert("Edge".into(), [0.5; 4]);
    let ctx = Context {
        size: 16,
        time: 0.0,
        sphere: false,
    };
    let e = eval::evaluate(&g, &ctx, &Fallback);
    let s = e.values(step, "Out").unwrap();
    assert_eq!(s[0][0], 0.0, "left half below the edge");
    assert_eq!(s[15][0], 1.0, "right half above it");
    let n = e.values(noise, "Out").unwrap();
    let (lo, hi) = n
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v[0]), b.max(v[0])));
    assert!(lo >= -0.3 && hi <= 1.3 && hi - lo > 0.2, "{lo}..{hi}");
    // Fresnel on the sphere: dark in the middle, bright at the rim.
    assert!(eval::wants_sphere(&g, fresnel));
    let sphere = eval::evaluate(
        &g,
        &Context {
            sphere: true,
            ..ctx
        },
        &Fallback,
    );
    let f = sphere.values(fresnel, "Out").unwrap();
    assert!(
        f[8 * 16 + 8][0] < 0.05 && f[8 * 16][0] > 0.3,
        "{} {}",
        f[8 * 16 + 8][0],
        f[8 * 16][0]
    );
    let img = sphere.image(fresnel, "Out").unwrap();
    assert_eq!(img[0][3], 0, "outside the sphere is transparent");
    // GPU and CPU share the noise hash.
    assert_eq!(eval::hash22([3.0, -2.0]), eval::hash22([3.5, -1.5]));
}

#[test]
fn graphs_load_through_the_shader_library() {
    let dir = std::env::temp_dir().join(format!("misty_sg_lib_{}", std::process::id()));
    let mut g = with_properties();
    let f = g.add_node(NodeKind::Fresnel, [0.0; 2]);
    g.connect(f, "Out", MASTER, master::EMISSION).unwrap();
    g.save(&dir.join("Assets/Rim.shadergraph")).unwrap();
    let rel = dir
        .join("Assets/Rim.shadergraph")
        .to_string_lossy()
        .into_owned();
    assert!(crate::shader_asset::is_shader_path(&rel));
    let asset = crate::shader_asset::load(&rel).unwrap();
    assert_eq!(asset.properties.len(), 6);
    assert!(asset.source.contains("pow(1.0 - saturate"));
    std::fs::remove_dir_all(dir).ok();
}
