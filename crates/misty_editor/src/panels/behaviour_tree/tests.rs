//! Behaviour Tree window: the optional package (install / remove and what it shows), Create ▸
//! Behaviour Tree and double-click, editing with undo / redo, copy / paste, groups and comments,
//! linking by dragging, palette drops, Arrange, save / load, the agent Properties, live debugging
//! in Play mode, and translations of the window's text.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::behaviour_tree::asset::NodeKind;
use misty_engine::behaviour_tree::blackboard::{KeyDef, KeyType};
use misty_engine::behaviour_tree::{BehaviourTree, BehaviourTreeAgent, Status};

use super::*;
use crate::actions::{Action, AssetKind};
use crate::packages::optional;
use crate::project::ProjectBrowser;

const PATH: &str = "Assets/AI/Test.btree";

fn project(tag: &str, install: bool) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_bt_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/AI")).unwrap();
    BehaviourTree::new_default().save(&root.join(PATH)).unwrap();
    if install {
        optional::install(&root, optional::BEHAVIOUR_TREES).unwrap();
    }
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.behaviour_tree.open(&root.join(PATH), PATH);
    s.behaviour_tree.canvas.frame_all = false;
    s.behaviour_tree.zoom = 1.0;
    s.behaviour_tree.pan = egui::Vec2::ZERO;
    s.behaviour_tree.show_blackboard = false;
    s.behaviour_tree.show_inspector = false;
    s.behaviour_tree.show_palette = false;
    note_package(&s);
    (s, root)
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1400.0, 900.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
    h.run();
    h
}

fn press(h: &mut Harness<'_, EditorState>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

fn drag(h: &mut Harness<'_, EditorState>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    press(h, from, true);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        h.event(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    press(h, to, false);
    h.run();
}

fn node_rect(h: &Harness<'_, EditorState>, id: u32) -> egui::Rect {
    let w = &h.state().behaviour_tree;
    canvas::node_rect(w.canvas.view.expect("drawn"), w.tree.node(id).unwrap())
}

/// Install puts the package in the manifest and lock (nothing copied); the window, Window ▸ AI
/// item, Create ▸ entry and Add Component entry appear; Remove takes them away again.
#[test]
fn package_install_and_remove() {
    let (mut s, root) = project("pkg", false);
    // Not installed by default.
    assert!(!optional::installed(&root, optional::BEHAVIOUR_TREES));
    assert!(!s.optional_packages.has(optional::BEHAVIOUR_TREES));
    assert!(!package_installed());
    assert!(!crate::panels::inspector::component_offered("Behaviour Tree Agent"));
    assert!(crate::panels::inspector::component_offered("Nav Mesh Agent"));
    assert!(
        optional::all()
            .iter()
            .any(|p| p.name == optional::BEHAVIOUR_TREES && p.display_name == "Behaviour Trees")
    );
    let h = window(s);
    h.get_by_label(&tr!("Install Behaviour Trees"));
    let mut s = h.into_state();
    // Install.
    s.packages.ensure_loaded(&root);
    s.packages.install_optional(&root, optional::BEHAVIOUR_TREES);
    s.optional_packages = optional::Installed::load(&root);
    note_package(&s);
    assert!(package_installed());
    assert!(crate::panels::inspector::component_offered("Behaviour Tree Agent"));
    let manifest = crate::packages::manifest::Manifest::load(&root).unwrap();
    assert_eq!(
        manifest
            .dependencies
            .get(optional::BEHAVIOUR_TREES)
            .map(String::as_str),
        Some(optional::REQUIREMENT)
    );
    let lock = crate::packages::manifest::Lock::load(&root).unwrap();
    assert_eq!(
        lock.dependencies
            .get(optional::BEHAVIOUR_TREES)
            .map(|l| l.source.as_str()),
        Some("builtin")
    );
    assert!(!root.join("Packages").join(optional::BEHAVIOUR_TREES).exists());
    // The window now shows the tree; Create ▸ Behaviour Tree works.
    let h = window(s);
    h.get_by_label(&tr!("Save Asset"));
    let mut s = h.into_state();
    s.project.open_folder(root.join("Assets/AI"));
    crate::commands::apply(&mut s, Action::CreateAsset(AssetKind::BehaviourTree));
    let made = root.join("Assets/AI/New Behaviour Tree.btree");
    assert!(made.is_file());
    crate::panels::project::open_file(&mut s, Some(made));
    assert_eq!(
        s.behaviour_tree.path.as_deref(),
        Some("Assets/AI/New Behaviour Tree.btree")
    );
    let kinds: Vec<&str> = s
        .behaviour_tree
        .tree
        .nodes
        .iter()
        .map(|n| n.kind.type_name())
        .collect();
    assert_eq!(kinds, ["Root", "Selector"]);
    // Remove.
    s.packages.remove(&root, optional::BEHAVIOUR_TREES);
    s.optional_packages = optional::Installed::load(&root);
    note_package(&s);
    assert!(!package_installed());
    assert!(!optional::installed(&root, optional::BEHAVIOUR_TREES));
    let lock = crate::packages::manifest::Lock::load(&root).unwrap();
    assert!(!lock.dependencies.contains_key(optional::BEHAVIOUR_TREES));
    // The asset stays (the user's work), the window asks to install again.
    assert!(root.join(PATH).is_file());
    let h = window(s);
    h.get_by_label(&tr!("Install Behaviour Trees"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn editing_undo_redo_copy_paste_group_comment_and_save() {
    let (mut s, root) = project("edit", true);
    let w = &mut s.behaviour_tree;
    let selector = 2;
    let wait = w.add_node(
        NodeKind::Wait {
            seconds: 1.0,
            random: 0.0,
        },
        [300.0, 80.0],
        Some(selector),
    );
    let log = w.add_node(
        NodeKind::Log {
            message: "hi".into(),
            level: misty_engine::behaviour_tree::asset::LogLevel::Info,
        },
        [300.0, -40.0],
        Some(selector),
    );
    assert_eq!(w.tree.node(selector).unwrap().children, vec![log, wait], "top first");
    assert!(w.dirty());
    // Undo / redo.
    assert_eq!(w.undo().as_deref(), Some("Create Node"));
    assert!(w.tree.node(log).is_none());
    assert_eq!(w.redo().as_deref(), Some("Create Node"));
    assert!(w.tree.node(log).is_some());
    // Copy / paste: a copy with new ids, not linked to the old parent.
    w.selected = [wait, log].into();
    let text = w.copy_selected().unwrap();
    assert!(text.contains("\"Wait\""));
    w.paste(None);
    assert_eq!(w.selected.len(), 2);
    assert!(w.selected.iter().all(|id| *id > log));
    assert_eq!(w.tree.node(selector).unwrap().children.len(), 2);
    w.paste(Some(&text));
    assert_eq!(w.tree.nodes.len(), 2 + 2 + 2 + 2);
    // Duplicate, delete (the Root stays).
    w.selected = [1, wait].into();
    w.delete_selected();
    assert!(w.tree.node(1).is_some() && w.tree.node(wait).is_none());
    // Group and comment.
    w.selected = [log].into();
    w.group_selected();
    assert_eq!(w.tree.groups.len(), 1);
    w.add_comment([0.0, -200.0]);
    assert_eq!(w.tree.comments.len(), 1);
    // Arrange lays the tree out left to right.
    w.edit("Arrange", false, |t| t.arrange());
    let root_x = w.tree.node(1).unwrap().position[0];
    assert!(w.tree.node(selector).unwrap().position[0] > root_x);
    assert!(w.tree.node(log).unwrap().position[0] > w.tree.node(selector).unwrap().position[0]);
    // Save and load back.
    w.save(&root.join(PATH)).unwrap();
    assert!(!w.dirty());
    let back = BehaviourTree::load(&root.join(PATH)).unwrap();
    assert_eq!(back, w.tree);
    // Pasting tree JSON from elsewhere brings the keys it needs.
    let mut other = BehaviourTree::default();
    other.keys.push(KeyDef::new("Target", KeyType::Object));
    other.nodes.push(misty_engine::behaviour_tree::BtNode::new(
        1,
        NodeKind::MoveTo {
            target: "Target".into(),
            acceptance: 1.0,
        },
        [0.0, 0.0],
    ));
    w.paste(Some(&other.to_json()));
    assert!(w.tree.key("Target").is_some());
    let _ = std::fs::remove_dir_all(root);
}

/// Dragging from a node's output onto another node links it; Ctrl+Z unlinks.
#[test]
fn dragging_from_an_output_links_nodes() {
    let (mut s, root) = project("link", true);
    let id = s.behaviour_tree.add_node(
        NodeKind::Wait {
            seconds: 1.0,
            random: 0.0,
        },
        [320.0, 120.0],
        None,
    );
    s.behaviour_tree.selected.clear();
    let mut h = window(s);
    let from = node_rect(&h, 2).right_center();
    let to = node_rect(&h, id).center();
    drag(&mut h, from, to);
    assert_eq!(h.state().behaviour_tree.tree.node(2).unwrap().children, vec![id]);
    // A loop is refused with a message.
    let from = node_rect(&h, 2).right_center();
    let to = node_rect(&h, 1).center();
    drag(&mut h, from, to);
    assert!(h.state().behaviour_tree.canvas.flash.is_some());
    // Moving a node by dragging it.
    let before = h.state().behaviour_tree.tree.node(id).unwrap().position;
    let c = node_rect(&h, id).center();
    drag(&mut h, c, c + egui::vec2(40.0, 30.0));
    let after = h.state().behaviour_tree.tree.node(id).unwrap().position;
    assert!((after[0] - before[0] - 40.0).abs() < 2.0, "{before:?} {after:?}");
    let _ = std::fs::remove_dir_all(root);
}

/// The palette lists every node kind and the project's custom leaves.
#[test]
fn palette_lists_nodes_and_custom_leaves() {
    let (mut s, root) = project("palette", true);
    std::fs::write(
        root.join("Assets/AI/Taunt.lua"),
        "-- @behaviour-leaf Taunt\n-- @param line Over here!\nlocal T = {}\nreturn T\n",
    )
    .unwrap();
    std::fs::write(root.join("Assets/AI/Plain.lua"), "return {}\n").unwrap();
    let items = palette::items(&mut s);
    for kind in NodeKind::palette() {
        assert!(items.iter().any(|i| i.kind.type_name() == kind.type_name()));
    }
    let taunt = items
        .iter()
        .find(|i| i.custom.as_deref() == Some("Taunt"))
        .expect("custom leaf listed");
    match &taunt.kind {
        NodeKind::CustomLua { script, params } => {
            assert_eq!(script, "Assets/AI/Taunt.lua");
            assert_eq!(params.get("line").map(String::as_str), Some("Over here!"));
        }
        k => panic!("{k:?}"),
    }
    assert_eq!(items.iter().filter(|i| i.custom.is_some()).count(), 1);
    s.behaviour_tree.show_palette = true;
    let h = window(s);
    h.get_by_label("Taunt");
    let _ = std::fs::remove_dir_all(root);
}

/// Play mode: the window follows the agent and shows running nodes and blackboard values.
#[test]
fn live_debugging_shows_node_states_and_values() {
    let (mut s, root) = project("live", true);
    let mut t = BehaviourTree::new_default();
    t.keys.push(KeyDef::new("Count", KeyType::Int));
    let wait = t.next_id();
    t.nodes.push(misty_engine::behaviour_tree::BtNode::new(
        wait,
        NodeKind::Wait {
            seconds: 10.0,
            random: 0.0,
        },
        [400.0, 0.0],
    ));
    t.link(2, wait).unwrap();
    t.save(&root.join(PATH)).unwrap();
    s.behaviour_tree.open(&root.join(PATH), PATH);
    let e = s.scene.spawn("Guard", None);
    s.scene.add(
        e,
        BehaviourTreeAgent {
            tree: Some(PATH.into()),
            ..Default::default()
        },
    );
    s.pause_on_play = false;
    crate::play::start(&mut s);
    for _ in 0..3 {
        crate::play::tick(&mut s, 1.0 / 60.0);
    }
    let live = s.behaviour_tree.live.clone().expect("live view");
    assert_eq!(s.behaviour_tree.agents.len(), 1);
    assert_eq!(
        s.behaviour_tree.live_of(wait).map(|l| l.0),
        Some(Status::Running)
    );
    assert!(live.keys.iter().any(|k| k.0 == "Count"));
    s.behaviour_tree.show_blackboard = true;
    let h = window(s);
    h.get_by_label(&tr!("Live values"));
    let mut s = h.into_state();
    crate::play::stop(&mut s);
    let _ = std::fs::remove_dir_all(root);
}

/// Every text of the window, its nodes and the package is translated into Turkish and Arabic.
#[test]
fn texts_are_translated() {
    use crate::i18n::{Language, catalog};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = [
        "panels/behaviour_tree/mod.rs",
        "panels/behaviour_tree/canvas.rs",
        "panels/behaviour_tree/inspector.rs",
        "panels/behaviour_tree/blackboard.rs",
        "panels/behaviour_tree/palette.rs",
        "panels/inspector/bt_editors.rs",
    ];
    let re = regex::Regex::new(r#"tr!\(\s*"((?:[^"\\]|\\.)*)""#).unwrap();
    let mut keys: Vec<String> = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(dir.join(f)).unwrap();
        for c in re.captures_iter(&text) {
            keys.push(c[1].replace("\\\"", "\""));
        }
    }
    for kind in NodeKind::palette().iter().chain([&NodeKind::Root]) {
        keys.push(kind.label().into());
        keys.push(kind.help().into());
        keys.push(kind.category().label().into());
    }
    for t in KeyType::ALL {
        keys.push(t.label().into());
    }
    use misty_engine::behaviour_tree::asset::{AbortMode, LogLevel, ParallelPolicy};
    use misty_engine::behaviour_tree::blackboard::CompareOp;
    keys.extend(AbortMode::ALL.iter().map(|a| a.label().to_owned()));
    keys.extend(ParallelPolicy::ALL.iter().map(|a| a.label().to_owned()));
    keys.extend(LogLevel::ALL.iter().map(|a| a.label().to_owned()));
    keys.extend(["Is Set", "Is Not Set"].map(String::from));
    keys.extend(
        [Status::Idle, Status::Running, Status::Success, Status::Failure].map(|s| s.label().to_owned()),
    );
    keys.extend(
        [
            "Behaviour Tree",
            "Behaviour Tree Agent",
            "Behaviour Trees",
            "Create Node",
            "Connect",
            "Disconnect",
            "Delete",
            "Paste",
            "Group Selection",
            "Add Comment",
            "Move",
            "Edit Node",
            "Arrange",
        ]
        .map(String::from),
    );
    let _ = CompareOp::ALL;
    keys.sort();
    keys.dedup();
    let mut missing = Vec::new();
    for lang in [Language::Turkish, Language::Arabic] {
        for k in &keys {
            if catalog::get(lang, k).is_none() {
                missing.push(format!("{}: {k:?}", lang.code()));
            }
        }
    }
    assert!(missing.is_empty(), "untranslated:\n{}", missing.join("\n"));
}

/// The `BehaviourTree` Lua table is also Script Graph nodes (get / set values, start / stop /
/// restart, finish), from the same binding table as the Lua reference.
#[test]
fn script_graphs_get_behaviour_tree_nodes() {
    let entries = crate::panels::script_graph::search::api_entries();
    for member in [
        "GetValue", "SetValue", "HasKey", "Start", "Stop", "Restart", "IsRunning", "GetStatus",
        "Finish",
    ] {
        assert!(
            entries.iter().any(|e| {
                e.params
                    .iter()
                    .any(|(k, v)| k == "owner" && v == "BehaviourTree")
                    && e.params.iter().any(|(k, v)| k == "member" && v == member)
            }),
            "no Script Graph node for BehaviourTree.{member}"
        );
    }
}
