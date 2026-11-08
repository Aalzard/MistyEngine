//! Shader Graph window, round 20: copy / paste between graphs, groups, keywords on the
//! Blackboard, Node Settings of a Custom Function, the unsaved-changes prompt, Sub Graph assets
//! and keyword fields in the material Inspector.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::shader_graph::{
    KeywordKind, MASTER, NodeKind, ShaderGraph, TargetMaterial, master,
};

use super::*;
use crate::project::ProjectBrowser;

const A: &str = "Assets/Shaders/A.shadergraph";
const B: &str = "Assets/Shaders/B.shadergraph";

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_sg20_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Shaders")).unwrap();
    new_graph(TargetMaterial::Lit).save(&root.join(A)).unwrap();
    ShaderGraph::new(TargetMaterial::Unlit)
        .save(&root.join(B))
        .unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.shader_graph.open(&root.join(A), A);
    s.shader_graph.zoom = 0.8;
    s.shader_graph.canvas.frame_all = false;
    (s, root)
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1300.0, 760.0])
        .build_ui_state(
            |ui, s: &mut EditorState| {
                show(ui, s);
                show_windows(ui.ctx(), s);
            },
            s,
        );
    h.run();
    h
}

fn click(h: &mut Harness<'_, EditorState>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
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

fn count(g: &ShaderGraph, f: impl Fn(&NodeKind) -> bool) -> usize {
    g.nodes.iter().filter(|n| f(&n.kind)).count()
}

#[test]
fn copy_in_one_graph_paste_in_another() {
    let (mut s, root) = project("paste");
    // A's starter graph: a Base Color property node. Select it.
    let prop = s
        .shader_graph
        .graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Property { .. }))
        .unwrap()
        .id;
    s.shader_graph.selected = [prop].into();
    s.shader_graph.focused = true;
    let mut h = window(s);
    h.event(Event::Copy);
    h.run();
    assert!(h.state().shader_graph.clipboard.is_some(), "copied");
    // Open B (A is unchanged, so no prompt) and paste: the node and its property arrive.
    let file = root.join(B);
    h.state_mut().shader_graph.open(&file, B);
    h.state_mut().shader_graph.focused = true;
    h.run();
    assert!(
        h.state().shader_graph.clipboard.is_some(),
        "kept across graphs"
    );
    // The system clipboard's text pastes too (another editor window, say).
    let text = h.state().shader_graph.clipboard.clone().unwrap().to_text();
    h.event(Event::Paste(text));
    h.run();
    let g = &h.state().shader_graph.graph;
    assert_eq!(count(g, |k| matches!(k, NodeKind::Property { .. })), 1);
    assert!(g.property("_Base_Color").is_some() || !g.properties.is_empty());
    assert!(h.state().shader_graph.dirty());
    // Paste again from the window's own clipboard: a second copy, further along.
    h.state_mut().shader_graph.paste(None, None);
    let g = &h.state().shader_graph.graph;
    assert_eq!(count(g, |k| matches!(k, NodeKind::Property { .. })), 2);
    assert_eq!(g.properties.len(), 1, "the property is reused");
    // Cut takes the selection away (and Undo brings it back).
    h.event(Event::Cut);
    h.run();
    assert_eq!(
        count(&h.state().shader_graph.graph, |k| matches!(
            k,
            NodeKind::Property { .. }
        )),
        1
    );
    h.state_mut().shader_graph.undo();
    assert_eq!(
        count(&h.state().shader_graph.graph, |k| matches!(
            k,
            NodeKind::Property { .. }
        )),
        2
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn groups_frame_their_nodes_and_move_them() {
    let (mut s, root) = project("groups");
    let (mut a, mut b) = (0, 0);
    s.shader_graph.edit("Setup", false, |g| {
        a = g.add_node(NodeKind::Float, [-600.0, 200.0]);
        b = g.add_node(NodeKind::OneMinus, [-380.0, 200.0]);
        g.connect(a, "Out", b, "In").unwrap();
    });
    s.shader_graph.selected = [a, b].into();
    s.shader_graph.focused = true;
    let mut h = window(s);
    key(&mut h, Key::G, Modifiers::COMMAND);
    let g = &h.state().shader_graph.graph;
    assert_eq!(g.groups.len(), 1);
    assert_eq!(g.groups[0].nodes, [a, b]);
    h.run();
    // Drag the group by its title bar: both nodes follow.
    let bar = h.get_by_label("Group New Group").rect();
    let before = h.state().shader_graph.graph.node(a).unwrap().position;
    drag(&mut h, bar.center(), bar.center() + egui::vec2(60.0, 40.0));
    let g = &h.state().shader_graph.graph;
    let after = g.node(a).unwrap().position;
    assert!(
        after[0] > before[0] + 30.0 && after[1] > before[1] + 20.0,
        "{before:?} → {after:?}"
    );
    let db = g.node(b).unwrap().position;
    assert!((db[0] - after[0] - 220.0).abs() < 1.0, "moved together");
    // Deleting a member keeps the group for the other.
    h.state_mut().shader_graph.selected = [a].into();
    h.state_mut().shader_graph.delete_selected();
    assert_eq!(h.state().shader_graph.graph.groups[0].nodes, [b]);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn keywords_from_the_blackboard_become_keyword_nodes() {
    let (s, root) = project("keywords");
    let mut h = window(s);
    h.get_by_label("+").click();
    h.run();
    h.get_by_label("Boolean Keyword").click();
    h.run();
    let w = &h.state().shader_graph;
    let name = w.blackboard.selected.clone().expect("selected");
    assert_eq!(w.graph.keyword(&name).unwrap().kind, KeywordKind::Boolean);
    // Drag its row onto the canvas: a Keyword node with On / Off inputs (settings closed, so
    // only the row carries the name).
    h.state_mut().shader_graph.blackboard.selected = None;
    h.run();
    let row = h.get_by_label("Boolean").rect().center();
    let m = h.get_by_label("Master Stack").rect();
    drag(&mut h, row, egui::pos2(m.left() - 160.0, m.bottom() + 80.0));
    let g = &h.state().shader_graph.graph;
    let k = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Keyword { .. }))
        .unwrap_or_else(|| {
            panic!(
                "dropped as a node: row {row:?}, master {m:?}, nodes {:?}",
                g.nodes.iter().map(|n| n.kind.title()).collect::<Vec<_>>()
            )
        });
    let ports: Vec<&str> = g.inputs(k).iter().map(|p| p.name).collect();
    assert_eq!(ports, ["On", "Off"]);
    let k = k.id;
    h.state_mut().shader_graph.edit("Use", false, |g| {
        g.connect(k, "Out", MASTER, master::ALPHA).unwrap();
    });
    h.run();
    assert!(
        h.state().shader_graph.error.is_none(),
        "{:?}",
        h.state().shader_graph.error
    );
    h.get_by_label("2 variants (limit 128)");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn custom_function_node_settings_edit_its_ports() {
    let (mut s, root) = project("customfn");
    let mut id = 0;
    s.shader_graph.edit("Setup", false, |g| {
        id = g.add_node(
            NodeKind::catalog()
                .into_iter()
                .find(|k| matches!(k, NodeKind::CustomFunction { .. }))
                .unwrap(),
            [-600.0, 200.0],
        );
        g.connect(id, "Out", MASTER, master::BASE_COLOR).unwrap();
    });
    s.shader_graph.selected = [id].into();
    let mut h = window(s);
    assert!(
        h.state().shader_graph.error.is_none(),
        "{:?}",
        h.state().shader_graph.error
    );
    h.get_by_label("+ Input").click();
    h.run();
    let g = &h.state().shader_graph.graph;
    let NodeKind::CustomFunction { inputs, .. } = &g.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(inputs.len(), 2);
    // A broken body puts the error on the node.
    h.state_mut().shader_graph.edit("Break", false, |g| {
        if let Some(NodeKind::CustomFunction { body, .. }) = g.node_mut(id).map(|n| &mut n.kind) {
            *body = "Out = missing_name;".into();
        }
    });
    h.run();
    assert_eq!(
        h.state().shader_graph.error.as_ref().and_then(|e| e.node),
        Some(id)
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn unsaved_changes_ask_before_closing_or_switching() {
    let (mut s, root) = project("prompt");
    s.shader_graph.edit("Add", false, |g| {
        g.add_node(NodeKind::Fresnel, [-500.0, 100.0]);
    });
    assert!(!may_close_tab(&mut s), "dirty: the prompt comes up");
    assert_eq!(s.shader_graph.prompt, Some(AfterPrompt::CloseTab));
    let mut h = window(s);
    h.get_by_label("Save changes?");
    h.get_by_label("Don't Save").click();
    h.run();
    let st = h.state_mut();
    assert!(st.shader_graph.prompt.is_none() && !st.shader_graph.dirty());
    let actions = std::mem::take(&mut st.actions);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, crate::actions::Action::CloseTab(Tab::ShaderGraph))),
        "{actions:?}"
    );
    // Opening another graph with unsaved changes: Save writes them, then B opens.
    st.shader_graph.edit("Add", false, |g| {
        g.add_node(NodeKind::Fresnel, [-500.0, 100.0]);
    });
    open(st, B);
    assert_eq!(st.shader_graph.prompt, Some(AfterPrompt::Open(B.into())));
    assert_eq!(st.shader_graph.path.as_deref(), Some(A), "not yet");
    h.run();
    h.get_by_label("Save").click();
    h.run();
    assert_eq!(h.state().shader_graph.path.as_deref(), Some(B));
    let a = ShaderGraph::load(&root.join(A)).unwrap();
    assert_eq!(count(&a, |k| *k == NodeKind::Fresnel), 1, "saved");
    // Quitting with unsaved changes asks too; the answer lets the window close.
    h.state_mut().shader_graph.edit("Add", false, |g| {
        g.add_node(NodeKind::Fresnel, [0.0, 0.0]);
    });
    assert!(h.state_mut().shader_graph.cancel_close());
    assert_eq!(h.state().shader_graph.prompt, Some(AfterPrompt::Quit));
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().shader_graph.prompt.is_none() && h.state().shader_graph.dirty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn sub_graph_assets_open_and_appear_in_the_search() {
    let (mut s, root) = project("subgraph");
    let sub = "Assets/Shaders/Tint.shadersubgraph";
    new_sub_graph().save(&root.join(sub)).unwrap();
    crate::panels::project::open_file(&mut s, Some(root.join(sub)));
    assert_eq!(s.shader_graph.path.as_deref(), Some(sub));
    assert!(s.shader_graph.graph.is_sub_graph());
    assert!(s.shader_graph.error.is_none());
    // Back in A, the search lists it; creating it reads its interface.
    s.shader_graph.open(&root.join(A), A);
    let found = search::project_sub_graphs(&root, Some(A));
    assert_eq!(found.len(), 1);
    let NodeKind::SubGraph {
        inputs, outputs, ..
    } = &found[0]
    else {
        panic!()
    };
    assert_eq!((inputs.len(), outputs.len()), (1, 1));
    // Its own window does not offer it (no Sub Graph inside itself).
    assert!(search::project_sub_graphs(&root, Some(sub)).is_empty());
    let mut h = window(s);
    h.state_mut().shader_graph.focused = true;
    let m = h.get_by_label("Master Stack").rect();
    let spot = egui::pos2(m.left() - 160.0, m.bottom() + 80.0);
    click(&mut h, spot);
    key(&mut h, Key::Space, Modifiers::NONE);
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("tint");
    h.run();
    h.get_by_label("  Tint").click();
    h.run();
    let g = &h.state().shader_graph.graph;
    let n = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::SubGraph { .. }))
        .expect("created");
    assert_eq!(g.outputs(n).len(), 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn materials_show_a_graphs_keywords() {
    let root = std::env::temp_dir().join(format!("misty_sg20_mat_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let file = root.join("Kw.shadergraph");
    let path = file.to_string_lossy().replace('\\', "/");
    let mut g = ShaderGraph::new(TargetMaterial::Lit);
    g.add_keyword("Detail", KeywordKind::Boolean);
    g.add_keyword(
        "Quality",
        KeywordKind::Enum {
            entries: vec!["Low".into(), "High".into()],
        },
    );
    g.save(&file).unwrap();
    misty_engine::shader_asset::reload(&path);
    let m = misty_engine::assets::Material {
        shader: path,
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
    h.get_by_label("Detail");
    h.get_by_label("Quality");
    // The dropdown stores the entry index like a property value.
    h.state_mut().properties.insert(
        "_QUALITY".into(),
        misty_engine::shader_asset::PropertyValue::Float(1.0),
    );
    h.run();
    assert_eq!(
        h.state().properties.get("_QUALITY"),
        Some(&misty_engine::shader_asset::PropertyValue::Float(1.0))
    );
    assert_eq!(h.state().params().custom.unwrap().variant, "_QUALITY=1");
    std::fs::remove_dir_all(root).ok();
}
