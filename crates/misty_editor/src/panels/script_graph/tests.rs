//! Script Graph window: Create ▸ Visual Script Graph and double-click, the node search (by
//! category, filtered from a dragged wire), type-checked wiring, error badges, copy / paste and
//! macros, the Lua API as nodes, Inspector fields, and breakpoints pausing Play.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::visual_script::{Literal, NoFiles, PinType, ScriptGraph, Variable};

use super::*;
use crate::actions::{Action, AssetKind};
use crate::project::ProjectBrowser;

const PATH: &str = "Assets/Logic/Test.vgraph";

fn project(tag: &str, g: &ScriptGraph) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_vsg_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Logic")).unwrap();
    g.save(&root.join(PATH)).unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.script_graph.open(&root.join(PATH), PATH);
    s.script_graph.canvas.frame_all = false;
    s.script_graph.zoom = 1.0;
    s.script_graph.pan = egui::Vec2::ZERO;
    s.script_graph.show_blackboard = false;
    s.script_graph.show_inspector = false;
    (s, root)
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1400.0, 900.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
    h.run();
    h
}

/// Screen position of a pin.
fn pin(h: &Harness<'_, EditorState>, node: u32, name: &str, output: bool) -> Pos2 {
    let w = &h.state().script_graph;
    let view = w.canvas.view.expect("drawn");
    let g = w.view();
    let n = g.node(node).unwrap();
    let l = canvas::layout(g, n, &NoFiles);
    let (pins, at) = if output {
        (&l.ports.outputs, &l.outputs)
    } else {
        (&l.ports.inputs, &l.inputs)
    };
    let i = pins.iter().position(|p| p.name == name).unwrap();
    view.screen(egui::pos2(n.position[0] + at[i].x, n.position[1] + at[i].y))
}

fn press(h: &mut Harness<'_, EditorState>, pos: Pos2, button: PointerButton, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

fn drag(h: &mut Harness<'_, EditorState>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    press(h, from, PointerButton::Primary, true);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        h.event(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    press(h, to, PointerButton::Primary, false);
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

fn type_text(h: &mut Harness<'_, EditorState>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run();
}

/// OnStart at the origin.
fn start_graph() -> ScriptGraph {
    let mut g = ScriptGraph::default();
    g.add_node("OnStart", [100.0, 100.0]);
    g
}

#[test]
fn create_menu_makes_a_graph_that_double_click_opens() {
    let (mut s, root) = project("create", &start_graph());
    s.script_graph = ScriptGraphWindow::default();
    s.project.open_folder(root.join("Assets/Logic"));
    crate::commands::apply(&mut s, Action::CreateAsset(AssetKind::ScriptGraph));
    let file = root.join("Assets/Logic/New Script Graph.vgraph");
    assert!(file.exists(), "created");
    crate::panels::project::open_file(&mut s, Some(file));
    let w = &s.script_graph;
    assert_eq!(
        w.path.as_deref(),
        Some("Assets/Logic/New Script Graph.vgraph")
    );
    let kinds: Vec<&str> = w.graph.nodes.iter().map(|n| n.kind.as_str()).collect();
    assert_eq!(kinds, ["OnStart", "OnUpdate"]);
    assert_eq!(AssetKind::ScriptGraph.label(), "Visual Script Graph");
    assert!(AssetKind::ALL.contains(&AssetKind::ScriptGraph));
}

#[test]
fn space_opens_the_search_and_enter_creates_the_node() {
    let (s, _root) = project("search", &start_graph());
    let mut h = window(s);
    let empty = egui::pos2(900.0, 500.0);
    h.event(Event::PointerMoved(empty));
    h.step();
    h.state_mut().script_graph.focused = true;
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(h.state().script_graph.search.is_some(), "search opened");
    // With no text the nodes are listed by category.
    assert!(h.query_by_label("Events").is_some());
    assert!(h.query_by_label("Engine").is_some());
    type_text(&mut h, "branch");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let w = &h.state().script_graph;
    assert!(w.search.is_none());
    assert!(w.graph.nodes.iter().any(|n| n.kind == "Branch"));
    assert!(w.dirty());
    // Undo takes it away again.
    h.state_mut().script_graph.focused = true;
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert!(
        !h.state()
            .script_graph
            .graph
            .nodes
            .iter()
            .any(|n| n.kind == "Branch")
    );
}

#[test]
fn a_wire_dropped_on_empty_canvas_opens_a_filtered_search_and_wires_the_new_node() {
    let (s, _root) = project("filtered", &start_graph());
    let mut h = window(s);
    let out = pin(&h, 1, "Out", true);
    drag(&mut h, out, out + egui::vec2(260.0, 40.0));
    let search = h
        .state()
        .script_graph
        .search
        .as_ref()
        .expect("search opened");
    assert_eq!(search.from.as_ref().map(|f| f.ty), Some(PinType::Exec));
    let list = search::filtered(h.state_mut());
    assert!(list.iter().any(|(e, _)| e.kind == "Branch"));
    assert!(
        !list.iter().any(|(e, _)| e.kind == "AddFloat"),
        "pure nodes have no exec input"
    );
    type_text(&mut h, "debug log");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let g = &h.state().script_graph.graph;
    let log = g
        .nodes
        .iter()
        .find(|n| n.kind == "Api" && n.param("member") == "Log")
        .expect("Debug.Log created");
    assert!(
        g.connections
            .iter()
            .any(|c| c.from == 1 && c.output == "Out" && c.to == log.id && c.input == "In"),
        "{:?}",
        g.connections
    );
}

#[test]
fn wires_are_type_checked_and_problems_show_on_nodes() {
    let mut g = start_graph();
    let v = g.add_node("MakeVector3", [100.0, 300.0]);
    let br = g.add_node("Branch", [450.0, 100.0]);
    let (s, _root) = project("types", &g);
    let mut h = window(s);
    // A Vector3 into a Bool input is refused.
    let from = pin(&h, v, "Vector", true);
    let to = pin(&h, br, "Condition", false);
    drag(&mut h, from, to);
    let w = &h.state().script_graph;
    assert!(w.graph.connections.is_empty(), "{:?}", w.graph.connections);
    assert!(
        w.canvas
            .flash
            .as_ref()
            .is_some_and(|(t, _)| t.contains("Vector3 cannot connect to Bool")),
        "{:?}",
        w.canvas.flash
    );
    // Exec into the Branch is fine.
    let a = pin(&h, 1, "Out", true);
    let b = pin(&h, br, "In", false);
    drag(&mut h, a, b);
    assert_eq!(h.state().script_graph.graph.connections.len(), 1);
    // A node with a problem gets a badge and the toolbar says so.
    let missing = h.state_mut().script_graph.add_node(
        "GetVariable",
        &[("name".into(), "Nope".into())],
        [450.0, 300.0],
    );
    h.run();
    let w = &h.state().script_graph;
    assert!(
        w.error_of(missing)
            .is_some_and(|e| e.contains("no variable named 'Nope'")),
        "{:?}",
        w.errors
    );
    assert!(h.query_by_label_contains("⚠").is_some());
}

#[test]
fn copy_paste_and_collapse_to_macro() {
    let mut g = start_graph();
    g.variables
        .push(Variable::new("Speed", PinType::Float, Literal::Float(2.0)));
    let get = g.add_node_with("GetVariable", &[("name", "Speed")], [100.0, 300.0]);
    let mul = g.add_node("MultiplyFloat", [300.0, 300.0]);
    let log = g.add_node_with(
        "Api",
        &[("owner", "Debug"), ("member", "Log"), ("mode", "call")],
        [500.0, 100.0],
    );
    g.connect(get, "Value", mul, "A", &NoFiles).unwrap();
    g.connect(mul, "Result", log, "Message", &NoFiles).unwrap();
    g.connect(1, "Out", log, "In", &NoFiles).unwrap();
    let (mut s, root) = project("macro", &g);
    let w = &mut s.script_graph;
    w.selected = [get, mul].into();
    w.copy_selected().unwrap();
    w.paste(None);
    assert_eq!(w.graph.nodes.len(), 6);
    assert_eq!(w.selected.len(), 2);
    assert_eq!(
        w.graph.connections.len(),
        4,
        "the wire between them is copied too"
    );
    // The pasted pair goes; the original pair becomes a macro.
    w.delete_selected();
    w.selected = [get, mul].into();
    let name = w.collapse_to_macro(&root).expect("collapsed");
    assert_eq!(name, "Macro");
    let m = w.graph.macro_graph("Macro").unwrap();
    assert!(m.nodes.iter().any(|n| n.kind == "GraphOutput"));
    let sub = w
        .graph
        .nodes
        .iter()
        .find(|n| n.kind == "Subgraph")
        .unwrap()
        .id;
    assert!(
        w.graph
            .connections
            .iter()
            .any(|c| c.from == sub && c.to == log && c.input == "Message")
    );
    let errors = compile::check(&w.graph, &NoFiles);
    assert!(errors.is_empty(), "{errors:?}");
    // Open the macro and come back.
    w.enter_macro("Macro");
    assert!(!w.at_root());
    assert!(w.view().nodes.iter().any(|n| n.kind == "MultiplyFloat"));
    w.leave_macro(0);
    assert!(w.at_root());
}

#[test]
fn most_of_the_lua_api_is_available_as_nodes() {
    let entries = search::api_entries();
    let has = |owner: &str, member: &str, mode: &str| {
        entries.iter().any(|e| {
            let p = |k: &str| {
                e.params
                    .iter()
                    .find(|(x, _)| x == k)
                    .map(|(_, v)| v.as_str())
            };
            p("owner") == Some(owner) && p("member") == Some(member) && p("mode") == Some(mode)
        })
    };
    for (owner, member, mode) in [
        ("#Transform", "position", "get"),
        ("#Transform", "position", "set"),
        ("#Transform", "Rotate", "call"),
        ("#GameObject", "SetActive", "call"),
        ("#Rigidbody", "AddForce", "call"),
        ("#AudioSource", "Play", "call"),
        ("Physics", "Raycast", "call"),
        ("Input", "GetKey", "call"),
        ("Time", "deltaTime", "get"),
        ("Debug", "Log", "call"),
        ("SceneManager", "LoadScene", "call"),
        ("", "Instantiate", "call"),
        ("#Text", "text", "set"),
    ] {
        assert!(has(owner, member, mode), "{owner}.{member} ({mode})");
    }
    // Every documented member (functions and properties) has a node.
    let documented: usize = crate::api_docs::sections()
        .iter()
        .flat_map(|s| {
            s.entries
                .iter()
                .map(move |e| (s.owner.clone(), e.name.clone()))
        })
        .filter(|(_, n)| !n.starts_with('_'))
        .count();
    let distinct: std::collections::BTreeSet<(String, String)> = entries
        .iter()
        .map(|e| {
            let p = |k: &str| {
                e.params
                    .iter()
                    .find(|(x, _)| x == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            (p("owner"), p("member"))
        })
        .collect();
    assert!(
        distinct.len() * 10 >= documented * 9,
        "{} of {documented} members",
        distinct.len()
    );
    assert!(entries.iter().any(|e| e.category == "Engine/Transform"));
}

#[test]
fn graph_object_variables_are_inspector_fields() {
    let mut g = start_graph();
    g.variables
        .push(Variable::new("Points", PinType::Int, Literal::Int(5)).object());
    g.variables
        .push(Variable::new("Hidden", PinType::Int, Literal::Int(1)));
    let (mut s, _root) = project("fields", &g);
    s.refresh_assets();
    let meta = s
        .assets
        .scripts
        .get(PATH)
        .expect("graph listed with scripts");
    let fields = meta.fields.as_ref().unwrap();
    assert_eq!(
        fields,
        &vec![(
            "Points".to_owned(),
            misty_engine::scene::ScriptValue::Number(5.0)
        )]
    );
}

#[test]
fn a_breakpoint_pauses_play_and_step_and_continue_drive_the_flow() {
    let mut g = ScriptGraph::default();
    let update = g.add_node("OnUpdate", [0.0, 0.0]);
    let a = g.add_node_with(
        "Api",
        &[("owner", "Debug"), ("member", "Log"), ("mode", "call")],
        [250.0, 0.0],
    );
    g.set_value(a, "Message", Literal::String("first".into()));
    let b = g.add_node_with(
        "Api",
        &[("owner", "Debug"), ("member", "Log"), ("mode", "call")],
        [500.0, 0.0],
    );
    g.set_value(b, "Message", Literal::String("second".into()));
    g.connect(update, "Out", a, "In", &NoFiles).unwrap();
    g.connect(a, "Out", b, "In", &NoFiles).unwrap();
    let (mut s, _root) = project("debug", &g);
    let e = s.scene.spawn("Runner", None);
    s.scene.add(
        e,
        misty_engine::scene::Scripts {
            list: vec![misty_engine::scene::ScriptRef {
                path: PATH.into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
    s.script_graph.toggle_breakpoint(b);
    crate::play::start(&mut s);
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert_eq!(
        s.play,
        crate::state::PlayState::Paused,
        "breakpoint paused Play"
    );
    let live = s.script_graph.live.clone().unwrap();
    assert_eq!(live.paused, Some(b));
    assert_eq!(live.watched.as_deref(), Some("Runner"));
    assert!(
        live.hits.contains_key(&(a, "In".to_owned())),
        "{:?}",
        live.hits
    );
    let logged = |s: &EditorState, text: &str| {
        s.console
            .entries
            .iter()
            .filter(|l| l.message == text)
            .count()
    };
    assert_eq!(logged(&s, "first"), 1);
    assert_eq!(logged(&s, "second"), 0);
    continue_flow(&mut s, true);
    assert_eq!(logged(&s, "second"), 1, "step ran the node");
    assert_eq!(s.play, crate::state::PlayState::Paused);
    continue_flow(&mut s, false);
    assert_eq!(s.play, crate::state::PlayState::Playing);
    crate::play::stop(&mut s);
}
