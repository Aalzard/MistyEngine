//! Effect Graph window: Add Block and Create Node searches, linking an operator into a block by
//! dragging, undo/redo, Save Asset, compile errors on nodes, and opening from the Project
//! window.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::vfx::{BlockKind, ContextType, EffectGraph, OperatorKind};

use super::*;
use crate::project::ProjectBrowser;

const PATH: &str = "Assets/Effects/Test.effect";

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_vfxg_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Effects")).unwrap();
    new_graph().save(&root.join(PATH)).unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    let file = root.join(PATH);
    s.effect_graph.open(&file, PATH);
    s.effect_graph.zoom = 0.75;
    s.effect_graph.pan = egui::vec2(-150.0, -420.0);
    s.effect_graph.canvas.frame_all = false;
    (s, root)
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1400.0, 900.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
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

fn type_text(h: &mut Harness<'_, EditorState>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run();
}

fn blocks_of(g: &EffectGraph, t: ContextType) -> Vec<BlockKind> {
    g.context_of(t)
        .map(|c| c.blocks.iter().map(|b| b.kind.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn add_block_from_the_footer_search_with_undo_and_redo() {
    let (s, _root) = project("block");
    let mut h = window(s);
    for label in [
        "Spawn",
        "Initialize Particle",
        "Update Particle",
        "Blackboard",
        "Save Asset",
        "Constant Spawn Rate",
        "Linear Drag",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label} is shown");
    }
    let foot = h
        .get_by_label("Add Block to Update Particle")
        .rect()
        .center();
    click(&mut h, foot);
    assert!(h.state().effect_graph.search.is_some(), "Add Block opened");
    type_text(&mut h, "gravity");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let w = &h.state().effect_graph;
    assert!(w.search.is_none());
    assert!(
        blocks_of(&w.graph, ContextType::Update).contains(&BlockKind::Gravity),
        "Gravity added to Update"
    );
    assert!(w.dirty());
    // Undo / redo with the keyboard (the window has focus after the clicks).
    h.state_mut().effect_graph.focused = true;
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert!(
        !blocks_of(&h.state().effect_graph.graph, ContextType::Update)
            .contains(&BlockKind::Gravity)
    );
    key(&mut h, Key::Y, Modifiers::COMMAND);
    assert!(
        blocks_of(&h.state().effect_graph.graph, ContextType::Update).contains(&BlockKind::Gravity)
    );
}

#[test]
fn create_context_and_operator_then_link_it_into_a_block() {
    let (s, root) = project("link");
    let mut h = window(s);
    // Space over empty canvas: Create Node; pick an Output context.
    let empty = h.get_by_label("Spawn").rect().right_top() + egui::vec2(260.0, 10.0);
    h.event(Event::PointerMoved(empty));
    h.step();
    h.state_mut().effect_graph.focused = true;
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(
        matches!(
            h.state().effect_graph.search.as_ref().map(|s| &s.mode),
            Some(search::Mode::Nodes { .. })
        ),
        "Create Node opened"
    );
    type_text(&mut h, "context output");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let outputs = h
        .state()
        .effect_graph
        .graph
        .contexts
        .iter()
        .filter(|c| c.kind.ty() == ContextType::Output)
        .count();
    assert_eq!(outputs, 2, "a second Output context");

    // A Float operator left of the Update context, linked into Linear Drag's Drag.
    let drag_row = h.get_by_label("Linear Drag in Drag").rect().center();
    let at = h
        .state()
        .effect_graph
        .canvas
        .view
        .unwrap()
        .graph(drag_row - egui::vec2(340.0, 40.0));
    let op = {
        let w = &mut h.state_mut().effect_graph;
        let mut id = 0;
        w.edit("Create Node", false, |g| {
            id = g.add_operator(OperatorKind::Float, [at.x, at.y]);
        });
        id
    };
    h.run();
    let out = h.get_by_label("Float out Out").rect().center();
    let input = h.get_by_label("Linear Drag in Drag").rect().center();
    drag(&mut h, out, input);
    let w = &h.state().effect_graph;
    let drag_block = w.graph.context_of(ContextType::Update).unwrap().blocks[0].id;
    assert!(
        w.graph
            .input_link(drag_block, "Drag")
            .is_some_and(|l| l.from == op),
        "linked: {:?}",
        w.graph.links
    );
    assert!(w.errors.is_empty(), "{:?}", w.errors);
    // Undo removes the link only.
    h.state_mut().effect_graph.undo();
    h.run();
    let w = &h.state().effect_graph;
    assert!(w.graph.input_link(drag_block, "Drag").is_none());
    assert!(w.graph.operator(op).is_some());

    // Save Asset writes the working copy.
    h.state_mut().effect_graph.redo();
    h.run();
    let save = h.get_by_label("Save Asset").rect().center();
    click(&mut h, save);
    let saved = EffectGraph::load(&root.join(PATH)).unwrap();
    assert_eq!(saved, h.state().effect_graph.graph);
    assert!(!h.state().effect_graph.dirty());
}

#[test]
fn compile_errors_show_on_their_nodes() {
    let (mut s, _root) = project("errors");
    let init = s
        .effect_graph
        .graph
        .context_of(ContextType::Initialize)
        .unwrap()
        .id;
    s.effect_graph.edit("Delete", false, |g| g.remove(init));
    let extra = {
        let mut id = 0;
        s.effect_graph.edit("Create Context", false, |g| {
            id = g.add_context(ContextType::Spawn, [400.0, 0.0]);
        });
        id
    };
    let h = window(s);
    let w = &h.state().effect_graph;
    assert!(w.errors.iter().any(|e| e.message.contains("Initialize")));
    assert!(w.error_of(extra).is_some(), "second Spawn context flagged");
    assert!(
        h.query_by_label_contains("⚠").is_some(),
        "toolbar shows the error"
    );
}

#[test]
fn double_click_in_the_project_opens_the_window() {
    let (mut s, root) = project("open");
    s.effect_graph = EffectGraphWindow::default();
    crate::panels::project::open_file(&mut s, Some(root.join(PATH)));
    assert_eq!(s.effect_graph.path.as_deref(), Some(PATH));
    assert!(
        s.effect_graph
            .graph
            .context_of(ContextType::Spawn)
            .is_some()
    );
}
