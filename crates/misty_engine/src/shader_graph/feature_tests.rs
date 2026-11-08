//! Keywords and variants, Custom Functions, Sub Graphs, groups and copy / paste, the master
//! stack per target, vertex colours and Decal graphs.

use super::codegen::{self, compile_variant, generate_variant};
use super::eval::{self, Context};
use super::*;

/// Sub Graphs and function files kept in memory.
#[derive(Default)]
pub(super) struct Mem {
    pub graphs: std::collections::HashMap<String, ShaderGraph>,
    pub files: std::collections::HashMap<String, String>,
}

impl Resolver for Mem {
    fn graph(&self, path: &str) -> Result<ShaderGraph, String> {
        self.graphs
            .get(path)
            .cloned()
            .ok_or_else(|| format!("{path}: missing"))
    }

    fn text(&self, path: &str) -> Result<String, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("{path}: missing"))
    }
}

fn with_properties() -> ShaderGraph {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    for (label, ty) in PropertyType::MENU {
        g.add_property(label, ty);
    }
    g
}

/// Lit graph whose Base Color comes from a Keyword node: noise when On, a colour when Off.
fn keyword_graph() -> (ShaderGraph, u32, u32) {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let kw = g.add_keyword("Noise", KeywordKind::Boolean);
    assert_eq!(kw, "_NOISE_ON");
    let k = g.add_node(NodeKind::Keyword { name: kw }, [0.0; 2]);
    let noise = g.add_node(NodeKind::GradientNoise, [0.0; 2]);
    let color = g.add_node(
        NodeKind::Color {
            value: [1.0, 0.0, 0.0, 1.0],
        },
        [0.0; 2],
    );
    g.connect(noise, "Out", k, "On").unwrap();
    g.connect(color, "Out", k, "Off").unwrap();
    g.connect(k, "Out", MASTER, master::BASE_COLOR).unwrap();
    (g, k, noise)
}

#[test]
fn keywords_compile_as_variants_with_only_their_nodes() {
    let (mut g, k, noise) = keyword_graph();
    // Dynamic type: the widest input (the colour's vector 4).
    assert_eq!(g.types().output(k, "Out"), Some(ValueType::Vec4));
    let off = compile(&g).unwrap();
    assert!(
        !off.source.contains(&format!("let n{noise}_0")),
        "Off skips the noise"
    );
    assert!(
        off.source
            .contains("// @keyword _NOISE_ON \"Noise\" toggle = 0")
    );
    let on = compile_variant(
        &g,
        &Variant::from([("_NOISE_ON".into(), 1)]),
        &FileResolver,
    )
    .unwrap();
    assert!(on.source.contains(&format!("let n{noise}_0")), "{}", on.source);
    // An Enum keyword multiplies the variants; every one compiles.
    let mode = g.add_keyword(
        "Mode",
        KeywordKind::Enum {
            entries: vec!["A".into(), "B".into(), "C".into()],
        },
    );
    let e = g.add_node(NodeKind::Keyword { name: mode.clone() }, [0.0; 2]);
    assert_eq!(
        g.inputs(g.node(e).unwrap())
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    g.connect(e, "Out", MASTER, master::SMOOTHNESS).unwrap();
    g.node_mut(e).unwrap().values.insert("C".into(), [0.9; 4]);
    assert_eq!(g.variant_count(), 6);
    assert_eq!(g.variants().len(), 6);
    assert_eq!(codegen::compile_all(&g, &FileResolver).unwrap(), 6);
    let c = generate_variant(&g, &Variant::from([(mode.clone(), 2)]), &FileResolver).unwrap();
    assert!(c.source.contains("0.9f"), "{}", c.source);
    // Keys name the values off their defaults.
    let v = Variant::from([(mode.clone(), 2), ("_NOISE_ON".into(), 0)]);
    assert_eq!(variant_key(&g.keywords, &v), "_MODE=2");
    assert_eq!(parse_variant_key("_MODE=2"), Variant::from([(mode, 2)]));
    // Past the cap: an error on a Keyword node.
    for i in 0..6 {
        g.add_keyword(&format!("K{i}"), KeywordKind::Boolean);
    }
    assert!(g.variant_count() > MAX_VARIANTS);
    let err = generate(&g).unwrap_err();
    assert!(err.message.contains("limit"), "{err}");
    assert!(matches!(
        g.node(err.node.unwrap()).unwrap().kind,
        NodeKind::Keyword { .. }
    ));
    // Removing a keyword removes its nodes.
    g.remove_keyword("_NOISE_ON");
    assert!(g.node(k).is_none());
}

#[test]
fn keyword_variants_load_through_the_shader_library() {
    let dir = std::env::temp_dir().join(format!("misty_sg_kw_{}", std::process::id()));
    let (g, _, _) = keyword_graph();
    let file = dir.join("Kw.shadergraph");
    g.save(&file).unwrap();
    let rel = file.to_string_lossy().replace('\\', "/");
    crate::shader_asset::reload(&rel);
    let base = crate::shader_asset::load(&rel).unwrap();
    assert_eq!(base.keywords.len(), 1);
    assert_eq!(base.keywords[0].options, ["Off", "On"]);
    let values = std::collections::BTreeMap::from([(
        "_NOISE_ON".to_owned(),
        crate::shader_asset::PropertyValue::Float(1.0),
    )]);
    let key = base.variant_key(&values);
    assert_eq!(key, "_NOISE_ON=1");
    let on = crate::shader_asset::load_variant(&rel, &key).unwrap();
    assert!(on.source != base.source);
    // A material asking for it gets that variant.
    let m = crate::assets::Material {
        shader: rel.clone(),
        properties: values,
        ..Default::default()
    };
    assert_eq!(m.params().custom.unwrap().variant, "_NOISE_ON=1");
    std::fs::remove_dir_all(dir).ok();
}

fn custom_function(body: &str) -> NodeKind {
    NodeKind::CustomFunction {
        function: "Tint".into(),
        source: FunctionSource::Inline,
        body: body.into(),
        file: String::new(),
        inputs: vec![
            Slot::new("Color", ValueType::Vec3),
            Slot::new("Amount", ValueType::Float).with_value([0.5; 4]),
        ],
        outputs: vec![
            Slot::new("Out", ValueType::Vec3),
            Slot::new("Luma", ValueType::Float),
        ],
    }
}

#[test]
fn custom_functions_compile_inline_and_from_files() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let f = g.add_node(
        custom_function("Out = Color * Amount;\nLuma = dot(Out, vec3<f32>(0.3, 0.59, 0.11));"),
        [0.0; 2],
    );
    let uv = g.add_node(NodeKind::Uv, [0.0; 2]);
    g.connect(uv, "Out", f, "Color").unwrap();
    g.connect(f, "Out", MASTER, master::BASE_COLOR).unwrap();
    g.connect(f, "Luma", MASTER, master::SMOOTHNESS).unwrap();
    let cg = compile(&g).unwrap_or_else(|e| panic!("{e}"));
    assert!(cg.source.contains("struct sg_fn_Tint_"), "{}", cg.source);
    assert!(cg.source.contains(".Luma"));
    // A WGSL mistake in the body is reported on the node.
    g.node_mut(f).unwrap().kind = custom_function("Out = Color * Amout;\nLuma = 1.0;");
    let e = compile(&g).unwrap_err();
    assert_eq!(e.node, Some(f), "{e}");
    // Bad port names are refused before WGSL.
    let mut bad = custom_function("Out = Color;");
    if let NodeKind::CustomFunction { inputs, .. } = &mut bad {
        inputs[0].name = "my color".into();
    }
    g.node_mut(f).unwrap().kind = bad;
    assert_eq!(generate(&g).unwrap_err().node, Some(f));
    // From a file: the function returns a struct named after the outputs.
    let mut mem = Mem::default();
    mem.files.insert(
        "Assets/Shaders/Tint.wgsl".into(),
        "struct TintOut { Out: vec3<f32>, Luma: f32, };\nfn Tint(c: vec3<f32>, a: f32) -> TintOut {\n    return TintOut(c * a, a);\n}\n".into(),
    );
    let mut file = custom_function("");
    if let NodeKind::CustomFunction {
        source, file: path, ..
    } = &mut file
    {
        *source = FunctionSource::File;
        *path = "Assets/Shaders/Tint.wgsl".into();
    }
    g.node_mut(f).unwrap().kind = file;
    let cg = compile_variant(&g, &Variant::new(), &mem).unwrap_or_else(|e| panic!("{e}"));
    assert!(cg.source.contains("fn Tint(c: vec3<f32>"));
    assert_eq!(cg.deps, ["Assets/Shaders/Tint.wgsl"]);
    // A missing file is an error on the node.
    let e = compile_variant(&g, &Variant::new(), &Mem::default()).unwrap_err();
    assert_eq!(e.node, Some(f));
}

/// A Sub Graph: Scale (float) into UV noise as "Out" (float), and a constant "Tint" (vector 3).
pub(super) fn noise_sub_graph() -> ShaderGraph {
    let mut s = ShaderGraph::new_sub_graph();
    s.outputs = vec![
        Slot::new("Out", ValueType::Float),
        Slot::new("Tint", ValueType::Vec3),
    ];
    let scale = s.add_property("Scale", PropertyType::Float);
    s.properties[0].value = [7.0; 4];
    let p = s.add_node(NodeKind::Property { name: scale }, [0.0; 2]);
    let noise = s.add_node(NodeKind::GradientNoise, [0.0; 2]);
    s.connect(p, "Out", noise, "Scale").unwrap();
    s.connect(noise, "Out", MASTER, "Out").unwrap();
    s.node_mut(MASTER)
        .unwrap()
        .values
        .insert("Tint".into(), [0.2, 0.4, 0.6, 0.0]);
    s
}

#[test]
fn sub_graphs_inline_with_their_inputs_and_outputs() {
    let sub = noise_sub_graph();
    assert!(compile(&sub).is_ok(), "a Sub Graph previews on its own");
    let (ins, outs) = sub.sub_graph_interface();
    assert_eq!(ins[0].name, "Scale");
    assert_eq!(ins[0].value, [7.0; 4]);
    assert_eq!(outs.len(), 2);
    let mut mem = Mem::default();
    mem.graphs
        .insert("Assets/Noise.shadersubgraph".into(), sub.clone());
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let n = g.add_node(
        NodeKind::SubGraph {
            path: "Assets/Noise.shadersubgraph".into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        },
        [0.0; 2],
    );
    assert!(
        g.refresh_sub_graphs(&mem),
        "the interface comes from the asset"
    );
    assert_eq!(g.outputs(g.node(n).unwrap()).len(), 2);
    let f = g.add_node(NodeKind::Float, [0.0; 2]);
    g.node_mut(f).unwrap().values.insert("X".into(), [3.0; 4]);
    g.connect(f, "Out", n, "Scale").unwrap();
    g.connect(n, "Out", MASTER, master::SMOOTHNESS).unwrap();
    g.connect(n, "Tint", MASTER, master::BASE_COLOR).unwrap();
    let cg = compile_variant(&g, &Variant::new(), &mem).unwrap_or_else(|e| panic!("{e}"));
    // The Sub Graph's nodes are inlined under the node's prefix, reading the parent's input.
    assert!(cg.source.contains(&format!("n{n}_n")), "{}", cg.source);
    assert!(
        cg.source
            .contains(&format!("sg_gradient_noise(input.uv, n{n}_n")),
        "{}",
        cg.source
    );
    assert!(cg.source.contains("0.2f, 0.4f, 0.6f"));
    assert!(cg.source.contains(&format!(": f32 = n{f}_0;")), "the input feeds the property");
    assert_eq!(cg.deps, ["Assets/Noise.shadersubgraph"]);
    // The CPU preview evaluates it too.
    struct Graphs(std::sync::Arc<ShaderGraph>);
    impl eval::Textures for Graphs {
        fn sample(&self, p: &GraphProperty, _: [f32; 2]) -> [f32; 4] {
            eval::fallback_color(p)
        }
        fn graph(&self, _: &str) -> Option<std::sync::Arc<ShaderGraph>> {
            Some(self.0.clone())
        }
    }
    let e = eval::evaluate(
        &g,
        &Context {
            size: 8,
            time: 0.0,
            sphere: false,
        },
        &Graphs(std::sync::Arc::new(sub.clone())),
    );
    let tint = e.values(n, "Tint").unwrap();
    assert!((tint[0][1] - 0.4).abs() < 1e-6);
    let noise = e.values(n, "Out").unwrap();
    assert!(noise.iter().any(|v| (v[0] - noise[0][0]).abs() > 1e-3));
    // A WGSL error inside the Sub Graph lands on the Sub Graph node.
    let mut broken = sub.clone();
    let cf = broken.add_node(custom_function("Out = nope;\nLuma = 0.0;"), [0.0; 2]);
    broken.connect(cf, "Luma", MASTER, "Out").unwrap();
    mem.graphs
        .insert("Assets/Noise.shadersubgraph".into(), broken);
    let err = compile_variant(&g, &Variant::new(), &mem).unwrap_err();
    assert_eq!(err.node, Some(n), "{err}");
    // A Sub Graph using itself is refused.
    let mut selfish = noise_sub_graph();
    let inner = selfish.add_node(
        NodeKind::SubGraph {
            path: "Assets/Noise.shadersubgraph".into(),
            inputs: Vec::new(),
            outputs: vec![Slot::new("Out", ValueType::Float)],
        },
        [0.0; 2],
    );
    selfish.connect(inner, "Out", MASTER, "Out").unwrap();
    mem.graphs
        .insert("Assets/Noise.shadersubgraph".into(), selfish);
    let err = compile_variant(&g, &Variant::new(), &mem).unwrap_err();
    assert!(err.message.contains("uses itself"), "{err}");
    // Connections to ports that went away are dropped on refresh.
    let mut fewer = noise_sub_graph();
    fewer.outputs.truncate(1);
    mem.graphs
        .insert("Assets/Noise.shadersubgraph".into(), fewer);
    assert!(g.refresh_sub_graphs(&mem));
    assert!(g.input_connection(MASTER, master::BASE_COLOR).is_none());
    assert!(g.input_connection(MASTER, master::SMOOTHNESS).is_some());
}

#[test]
fn groups_and_copy_paste_across_graphs() {
    let mut a = with_properties();
    let p = a.add_node(
        NodeKind::Property {
            name: "_Float".into(),
        },
        [0.0, 0.0],
    );
    let one = a.add_node(NodeKind::OneMinus, [200.0, 0.0]);
    a.connect(p, "Out", one, "In").unwrap();
    a.connect(one, "Out", MASTER, master::ALPHA).unwrap();
    a.group("Alpha", &[p, one, MASTER]).unwrap();
    assert_eq!(a.groups[0].nodes, [p, one], "never the master stack");
    // Copy both (the master stack stays behind) into a graph without the property.
    let clip = a.copy(&[p, one, MASTER]);
    assert_eq!(clip.nodes.len(), 2);
    assert_eq!(clip.connections.len(), 1, "only the connection between them");
    assert_eq!(clip.properties.len(), 1);
    let text = clip.to_text();
    let back = Clipboard::from_text(&text).unwrap();
    assert_eq!(back, clip);
    assert!(Clipboard::from_text("{\"nodes\": []}").is_none());
    let mut b = ShaderGraph::new(TargetMaterial::Unlit);
    let ids = b.paste(&back, [50.0, 10.0]);
    assert_eq!(ids.len(), 2);
    assert!(b.property("_Float").is_some(), "the property came along");
    assert_eq!(b.connections.len(), 1);
    assert_eq!(b.node(ids[1]).unwrap().position, [250.0, 10.0]);
    assert_eq!(b.groups.len(), 1);
    // Pasting again reuses the property; a same-named property of another type is kept and
    // the copy is renamed.
    b.paste(&back, [0.0; 2]);
    assert_eq!(b.properties.len(), 1);
    let mut c = ShaderGraph::new(TargetMaterial::Lit);
    c.properties
        .push(GraphProperty::new("_Float", "Float", PropertyType::Color));
    let ids = c.paste(&back, [0.0; 2]);
    assert_eq!(c.properties.len(), 2);
    let NodeKind::Property { name } = &c.node(ids[0]).unwrap().kind else {
        panic!()
    };
    assert_ne!(name, "_Float");
    assert!(compile(&c).is_ok());
    // Removing a node takes it out of its group; an empty group goes.
    a.remove_node(p);
    assert_eq!(a.groups[0].nodes, [one]);
    a.ungroup(&[one]);
    assert!(a.groups.is_empty());
}

#[test]
fn master_stack_follows_the_target() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let names = |g: &ShaderGraph| -> Vec<&str> {
        g.master_ports().iter().map(|(_, p)| p.name).collect()
    };
    for p in [
        master::OCCLUSION,
        master::TRANSMISSION,
        master::NORMAL,
        master::POSITION,
    ] {
        assert!(names(&g).contains(&p), "{p}");
    }
    let ao = g.add_node(NodeKind::Float, [0.0; 2]);
    g.connect(ao, "Out", MASTER, master::OCCLUSION).unwrap();
    let c = g.add_node(
        NodeKind::Color {
            value: [0.5, 1.0, 0.2, 1.0],
        },
        [0.0; 2],
    );
    g.connect(c, "Out", MASTER, master::TRANSMISSION).unwrap();
    let n = g.add_node(NodeKind::Normal { space: Space::World }, [0.0; 2]);
    g.connect(n, "Out", MASTER, master::NORMAL).unwrap();
    let src = compile(&g).unwrap().source;
    assert!(src.contains("o.occlusion =") && src.contains("o.transmission ="));
    assert!(src.contains("tangent_to_world(input,"));
    g.target.normal_space = NormalSpace::World;
    assert!(compile(&g).unwrap().source.contains("o.normal = normalize("));
    g.target.normal_space = NormalSpace::Object;
    assert!(
        compile(&g)
            .unwrap()
            .source
            .contains("sg_object_normal_to_world(")
    );
    g.target.material = TargetMaterial::Unlit;
    assert!(!names(&g).contains(&master::OCCLUSION));
    g.target.material = TargetMaterial::Decal;
    let decal = names(&g);
    assert!(!decal.contains(&master::POSITION) && !decal.contains(&master::TRANSMISSION));
    assert!(decal.contains(&master::OCCLUSION) && decal.contains(&master::NORMAL));
    // Files without the new fields stay as they were.
    let json = ShaderGraph::new(TargetMaterial::Lit).to_json();
    assert!(!json.contains("normal_space") && !json.contains("keywords"));
}

#[test]
fn vertex_colour_reaches_both_stages() {
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    let vc = g.add_node(NodeKind::VertexColor, [0.0; 2]);
    let split = g.add_node(NodeKind::Split, [0.0; 2]);
    let mul = g.add_node(NodeKind::Multiply, [0.0; 2]);
    g.connect(vc, "Out", split, "In").unwrap();
    g.connect(split, "R", mul, "A").unwrap();
    g.connect(mul, "Out", MASTER, master::POSITION).unwrap();
    g.connect(vc, "Out", MASTER, master::BASE_COLOR).unwrap();
    let src = compile(&g).unwrap_or_else(|e| panic!("{e}")).source;
    assert!(
        src.contains("v.color") && src.contains("input.vertex_color"),
        "{src}"
    );
    let wrapped = crate::render::custom_shader::wrap(&crate::shader_asset::parse(&src).unwrap());
    assert!(wrapped.body.contains("@location(5) color: vec4<f32>"));
}

/// A Decal graph: a texture property tinted by a colour, a keyword picking the smoothness.
pub(super) fn decal_graph() -> ShaderGraph {
    let mut g = ShaderGraph::new(TargetMaterial::Decal);
    let tex = g.add_property(
        "Mask",
        PropertyType::Texture2D {
            fallback: TextureFallback::White,
        },
    );
    let tint = g.add_property("Tint", PropertyType::Color);
    let t = g.add_node(NodeKind::Property { name: tex }, [0.0; 2]);
    let c = g.add_node(NodeKind::Property { name: tint }, [0.0; 2]);
    let s = g.add_node(NodeKind::SampleTexture2D { normal_map: false }, [0.0; 2]);
    let m = g.add_node(NodeKind::Multiply, [0.0; 2]);
    g.connect(t, "Out", s, "Texture").unwrap();
    g.connect(s, "RGBA", m, "A").unwrap();
    g.connect(c, "Out", m, "B").unwrap();
    g.connect(m, "Out", MASTER, master::BASE_COLOR).unwrap();
    g.connect(s, "A", MASTER, master::ALPHA).unwrap();
    let kw = g.add_keyword("Wet", KeywordKind::Boolean);
    let k = g.add_node(NodeKind::Keyword { name: kw }, [0.0; 2]);
    g.node_mut(k).unwrap().values.insert("On".into(), [0.95; 4]);
    g.connect(k, "Out", MASTER, master::SMOOTHNESS).unwrap();
    let noise = g.add_node(NodeKind::GradientNoise, [0.0; 2]);
    g.connect(noise, "Out", MASTER, master::OCCLUSION).unwrap();
    g
}

#[test]
fn decal_graphs_compile_into_the_decal_pass() {
    let g = decal_graph();
    let header = generate(&g).unwrap().source;
    assert!(header.starts_with(codegen::DECAL_HEADER) && !header.contains("fn surface"));
    let asset = crate::shader_asset::parse(&header).unwrap();
    assert_eq!((asset.properties.len(), asset.keywords.len()), (2, 1));
    assert_eq!(codegen::compile_all(&g, &FileResolver).unwrap(), 2);
    let d = codegen::generate_decal(
        &g,
        &Variant::from([("_WET_ON".into(), 1)]),
        &FileResolver,
        "sg_decal_3",
    )
    .unwrap();
    assert!(
        d.body
            .starts_with("fn sg_decal_3(input: DecalInput) -> DecalOutput")
    );
    assert!(
        d.body.contains("sg_decal_texture(input.params[0]"),
        "{}",
        d.body
    );
    assert!(d.body.contains("input.params[1]") && d.body.contains("0.95f"));
    assert_eq!(d.properties.len(), 2);
    // Object-space nodes have no object in a decal.
    let mut bad = g.clone();
    let p = bad.add_node(
        NodeKind::Position {
            space: Space::Object,
        },
        [0.0; 2],
    );
    bad.connect(p, "Out", MASTER, master::EMISSION).unwrap();
    let e = codegen::compile_decal(&bad, &Variant::new(), &FileResolver).unwrap_err();
    assert_eq!(e.node, Some(p), "{e}");
}
