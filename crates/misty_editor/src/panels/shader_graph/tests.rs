//! Shader Graph window: Create Node search, connecting ports by dragging (type-checked), deleting,
//! undo/redo, Save Asset, the Blackboard, opening from the Project window and the material
//! Inspector showing a graph's properties.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::shader_graph::{MASTER, NodeKind, ShaderGraph, TargetMaterial, master};

use super::*;
use crate::project::ProjectBrowser;

const PATH: &str = "Assets/Shaders/Test.shadergraph";

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_sg_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Shaders")).unwrap();
    new_graph(TargetMaterial::Lit)
        .save(&root.join(PATH))
        .unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    let file = root.join(PATH);
    s.shader_graph.open(&file, PATH);
    s.shader_graph.zoom = 0.8;
    s.shader_graph.canvas.frame_all = false;
    (s, root)
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1300.0, 760.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
    h.run();
    h
}

fn saved(root: &std::path::Path) -> ShaderGraph {
    ShaderGraph::load(&root.join(PATH)).unwrap()
}

fn click(h: &mut Harness<'_, EditorState>, pos: Pos2, button: PointerButton) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

fn drag(h: &mut Harness<'_, EditorState>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        h.event(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.run();
}

fn key(h: &mut Harness<'_, EditorState>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.event(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
        h.step();
    }
    h.run();
}

fn nodes_of(g: &ShaderGraph, f: impl Fn(&NodeKind) -> bool) -> Vec<u32> {
    g.nodes
        .iter()
        .filter(|n| f(&n.kind))
        .map(|n| n.id)
        .collect()
}

/// An empty spot of the canvas (left of and below the master stack, clear of the panels).
fn empty_spot(h: &Harness<'_, EditorState>) -> Pos2 {
    let m = h.get_by_label("Master Stack").rect();
    egui::pos2(m.left() - 140.0, m.bottom() + 90.0)
}

#[test]
fn create_connect_delete_undo_redo_and_save() {
    let (s, root) = project("edit");
    let mut h = window(s);
    for label in [
        "Master Stack",
        "Blackboard",
        "Graph Settings",
        "Main Preview",
        "Save Asset",
    ] {
        h.get_by_label(label);
    }

    // Right-click the canvas ▸ Create Node, search "fresnel", pick it.
    let spot = empty_spot(&h);
    click(&mut h, spot, PointerButton::Secondary);
    h.get_by_label("Create Node").click();
    h.run();
    assert!(h.state().shader_graph.search.is_some(), "the search opens");
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("fresnel");
    h.run();
    h.get_by_label("  Fresnel Effect").click();
    h.run();
    let g = &h.state().shader_graph.graph;
    let fresnel = nodes_of(g, |k| *k == NodeKind::Fresnel);
    assert_eq!(fresnel.len(), 1, "created");
    let fresnel = fresnel[0];
    assert!(h.state().shader_graph.selected.contains(&fresnel));
    assert!(h.state().shader_graph.dirty());

    // Drag its output onto the master stack's Emission.
    let from = h.get_by_label("Fresnel Effect out Out").rect().center();
    let to = h.get_by_label("Master Stack in Emission").rect().center();
    drag(&mut h, from, to);
    let c = h
        .state()
        .shader_graph
        .graph
        .input_connection(MASTER, master::EMISSION)
        .cloned()
        .expect("connected");
    assert_eq!(c.from, fresnel);
    assert!(
        h.state().shader_graph.error.is_none(),
        "{:?}",
        h.state().shader_graph.error
    );

    // Select it and press Delete: the node and its connection go.
    let node = h.get_by_label("Fresnel Effect").rect();
    click(
        &mut h,
        node.center_top() + egui::vec2(0.0, 8.0),
        PointerButton::Primary,
    );
    assert!(h.state().shader_graph.focused);
    key(&mut h, Key::Delete, Modifiers::NONE);
    let g = &h.state().shader_graph.graph;
    assert!(g.node(fresnel).is_none(), "deleted");
    assert!(g.input_connection(MASTER, master::EMISSION).is_none());

    // Ctrl+Z brings it back (with the connection), Ctrl+Y deletes it again, Ctrl+Z restores.
    key(&mut h, Key::Z, Modifiers::COMMAND);
    let g = &h.state().shader_graph.graph;
    assert!(g.node(fresnel).is_some(), "undone");
    assert!(g.input_connection(MASTER, master::EMISSION).is_some());
    key(&mut h, Key::Y, Modifiers::COMMAND);
    assert!(
        h.state().shader_graph.graph.node(fresnel).is_none(),
        "redone"
    );
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert!(h.state().shader_graph.graph.node(fresnel).is_some());

    // Nothing reached the file until Save Asset.
    assert!(nodes_of(&saved(&root), |k| *k == NodeKind::Fresnel).is_empty());
    h.get_by_label("Save Asset").click();
    h.run();
    let file = saved(&root);
    assert_eq!(nodes_of(&file, |k| *k == NodeKind::Fresnel).len(), 1);
    assert!(file.input_connection(MASTER, master::EMISSION).is_some());
    assert!(!h.state().shader_graph.dirty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn connections_are_type_checked_and_loops_refused() {
    let (mut s, root) = project("types");
    let w = &mut s.shader_graph;
    let (mut tex, mut float, mut add) = (0, 0, 0);
    w.edit("Setup", false, |g| {
        tex = g.add_node(
            NodeKind::SampleTexture2D { normal_map: false },
            [-640.0, -260.0],
        );
        float = g.add_node(NodeKind::Float, [-640.0, 200.0]);
        add = g.add_node(NodeKind::Add, [-330.0, 200.0]);
        g.connect(float, "Out", add, "A").unwrap();
    });
    let mut h = window(s);
    // A float does not go into a texture input.
    let from = h.get_by_label("Float out Out").rect().center();
    let to = h
        .get_by_label("Sample Texture 2D in Texture")
        .rect()
        .center();
    drag(&mut h, from, to);
    assert!(
        h.state()
            .shader_graph
            .graph
            .input_connection(tex, "Texture")
            .is_none()
    );
    assert!(
        h.state().shader_graph.canvas.flash.is_some(),
        "the refusal is shown"
    );
    // Add's output back into Float's input would loop.
    let from = h.get_by_label("Add out Out").rect().center();
    let to = h.get_by_label("Float in X").rect().center();
    drag(&mut h, from, to);
    assert!(
        h.state()
            .shader_graph
            .graph
            .input_connection(float, "X")
            .is_none()
    );
    // Vectors promote: Add (a float) into Base Color (a vector 3) is fine.
    let to = h.get_by_label("Master Stack in Base Color").rect().center();
    drag(&mut h, from, to);
    assert_eq!(
        h.state()
            .shader_graph
            .graph
            .input_connection(MASTER, master::BASE_COLOR)
            .map(|c| c.from),
        Some(add)
    );
    // The unconnected texture input is reported on its node.
    h.state_mut().shader_graph.edit("Use", false, |g| {
        g.connect(tex, "R", MASTER, master::ALPHA).unwrap();
    });
    h.run();
    assert_eq!(
        h.state().shader_graph.error.as_ref().and_then(|e| e.node),
        Some(tex)
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn blackboard_adds_properties_that_drag_onto_the_canvas() {
    let (s, root) = project("blackboard");
    let mut h = window(s);
    h.get_by_label("+").click();
    h.run();
    h.get_by_label("Range").click();
    h.run();
    let w = &h.state().shader_graph;
    assert_eq!(w.blackboard.selected.as_deref(), Some("_Range"));
    assert!(w.graph.property("_Range").is_some());
    // Drag the row onto the canvas: a Property node reading it.
    let row = h.get_by_label("Range").rect().center();
    let spot = empty_spot(&h);
    drag(&mut h, row, spot);
    let reads = nodes_of(
        &h.state().shader_graph.graph,
        |k| matches!(k, NodeKind::Property { name } if name == "_Range"),
    );
    assert_eq!(reads.len(), 1, "dropped as a node");
    // Removing the property removes its nodes; undo restores both.
    h.state_mut()
        .shader_graph
        .edit("Delete Property", false, |g| g.remove_property("_Range"));
    assert!(h.state().shader_graph.graph.node(reads[0]).is_none());
    h.state_mut().shader_graph.undo();
    assert!(h.state().shader_graph.graph.node(reads[0]).is_some());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn space_opens_the_search_and_dropping_a_wire_creates_a_connected_node() {
    let (s, root) = project("space");
    let mut h = window(s);
    let spot = empty_spot(&h);
    click(&mut h, spot, PointerButton::Primary);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(h.state().shader_graph.search.is_some());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().shader_graph.search.is_none());
    // Drag from Master ▸ Alpha to empty canvas: the search offers nodes and connects the pick.
    let from = h.get_by_label("Master Stack in Alpha").rect().center();
    drag(&mut h, from, spot);
    let search = h.state().shader_graph.search.as_ref().expect("opened");
    assert!(search.from.is_some());
    let list = search::filtered(h.state());
    assert!(!list.is_empty());
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("gradient noise");
    h.run();
    h.get_by_label("  Gradient Noise").click();
    h.run();
    let g = &h.state().shader_graph.graph;
    let noise = nodes_of(g, |k| *k == NodeKind::GradientNoise)[0];
    assert_eq!(
        g.input_connection(MASTER, master::ALPHA).map(|c| c.from),
        Some(noise)
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn double_click_and_window_menu_open_the_shader_graph() {
    let (mut s, root) = project("open");
    s.shader_graph = Default::default();
    crate::panels::project::open_file(&mut s, Some(root.join(PATH)));
    assert_eq!(s.shader_graph.path.as_deref(), Some(PATH));
    let actions = std::mem::take(&mut s.actions);
    assert!(
        matches!(
            actions.as_slice(),
            [crate::actions::Action::FocusTab(Tab::ShaderGraph)]
        ),
        "{actions:?}"
    );
    let mut h = Harness::builder()
        .with_size([900.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| crate::menu_bar::show(ui, s), s);
    h.run();
    h.get_by_label("Window").click();
    h.run();
    h.get_by_label("Rendering ⏵").hover();
    h.run();
    h.get_by_label("Shader Graph").click();
    h.run();
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert!(
        matches!(
            actions.as_slice(),
            [crate::actions::Action::FocusTab(Tab::ShaderGraph)]
        ),
        "{actions:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn materials_using_a_graph_show_its_properties() {
    let root = std::env::temp_dir().join(format!("misty_sg_inspector_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // An absolute shader path: independent of the process-wide project root.
    let file = root.join("Props.shadergraph");
    let graph = file.to_string_lossy().replace('\\', "/");
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    g.add_property(
        "Edge Glow",
        misty_engine::shader_graph::PropertyType::Boolean,
    );
    g.add_property(
        "Amount",
        misty_engine::shader_graph::PropertyType::Range { min: 0.0, max: 2.0 },
    );
    g.save(&file).unwrap();
    misty_engine::shader_asset::reload(&graph);
    let m = misty_engine::assets::Material {
        shader: graph,
        ..Default::default()
    };
    let mut h = Harness::builder().with_size([500.0, 400.0]).build_ui_state(
        |ui, m: &mut misty_engine::assets::Material| {
            egui::Grid::new("g").show(ui, |ui| {
                crate::panels::inspector::shader_material::custom_fields(ui, "t", m, &[]);
            });
        },
        m,
    );
    h.run();
    h.get_by_label("Edge Glow");
    h.get_by_label("Amount");
    std::fs::remove_dir_all(root).ok();
}
