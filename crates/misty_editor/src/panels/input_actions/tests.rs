//! Input Actions window: the three columns, adding maps / actions / bindings, Listen, the
//! control scheme filter, saving, Assets ▸ Create ▸ Input Actions and the Player Input
//! Inspector.

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::input::actions::{InputActionAsset, PlayerInput, PlayerInputBehavior};

use super::*;
use crate::actions::AssetKind;
use crate::commands;
use crate::project::ProjectBrowser;

const PATH: &str = "Assets/Input/Controls.inputactions";

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_ia_win_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Input")).unwrap();
    InputActionAsset::starter("Player")
        .save(&root.join(PATH))
        .unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    open(&mut s, PATH);
    (s, root)
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1100.0, 1000.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
    h.run();
    h
}

fn saved(root: &std::path::Path) -> InputActionAsset {
    InputActionAsset::load(&root.join(PATH)).unwrap()
}

fn key(h: &mut Harness<'_, EditorState>, key: Key) {
    for pressed in [true, false] {
        h.event(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

#[test]
fn three_columns_show_maps_actions_and_binding_properties() {
    let (s, root) = project("columns");
    let mut h = window(s);
    for label in [
        "Action Maps",
        "Actions",
        "Player",
        "UI",
        "▾ Move",
        "WASD",
        "Up: W [Keyboard]",
        "Left Stick [Gamepad]",
        "▾ Jump",
        "Space [Keyboard]",
        "Auto-Save",
    ] {
        h.get_by_label(label);
    }
    // Selecting an action shows its properties.
    h.get_by_label("▾ Jump").click();
    h.run();
    assert_eq!(h.state().input_actions.selection, Selection::Action(3));
    h.get_by_label("Action Type");
    h.get_by_label("No interactions have been added.");
    // A binding shows its path, Listen and the schemes.
    h.get_by_label("Space [Keyboard]").click();
    h.run();
    assert_eq!(
        h.state().input_actions.selection,
        Selection::Binding(3, 0, None)
    );
    h.get_by_label("Listen");
    h.get_by_label("Use in Control Scheme");
    h.get_by_label("Keyboard&Mouse");
    // The UI map lists its own actions.
    h.get_by_label("UI").click();
    h.run();
    h.get_by_label("▾ Submit");
    assert!(h.query_by_label("▾ Jump").is_none());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn listen_binds_the_next_key_and_save_writes_the_asset() {
    let (mut s, root) = project("listen");
    select(&mut s, "Player/Jump/0");
    let mut h = window(s);
    h.get_by_label("Listen").click();
    // Listening keeps repainting.
    h.step();
    h.step();
    assert!(h.state().input_actions.listening);
    h.get_by_label("Listening… (Esc cancels)");
    key(&mut h, Key::J);
    assert!(!h.state().input_actions.listening);
    let a = h.state().input_actions.asset.clone().unwrap();
    assert_eq!(a.maps[0].actions[3].bindings[0].path, "<Keyboard>/j");
    // The row and the path's display name under the field.
    assert_eq!(h.get_all_by_label("J [Keyboard]").count(), 2);
    assert!(h.state().input_actions.dirty());
    assert_eq!(
        saved(&root).maps[0].actions[3].bindings[0].path,
        "<Keyboard>/space"
    );
    h.get_by_label("Save Asset").click();
    h.run();
    assert!(!h.state().input_actions.dirty());
    assert_eq!(
        saved(&root).maps[0].actions[3].bindings[0].path,
        "<Keyboard>/j"
    );
    // Escape cancels Listen and keeps the path.
    h.get_by_label("Listen").click();
    h.step();
    h.step();
    key(&mut h, Key::Escape);
    assert!(!h.state().input_actions.listening);
    let a = h.state().input_actions.asset.clone().unwrap();
    assert_eq!(a.maps[0].actions[3].bindings[0].path, "<Keyboard>/j");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn adding_maps_actions_composites_and_processors_with_auto_save() {
    let (mut s, root) = project("add");
    s.input_actions.auto_save = true;
    let mut h = window(s);
    // Action Maps ▸ + adds a map with one action and renames it.
    h.get_all_by_label("+").next().unwrap().click();
    h.run();
    let a = saved(&root);
    assert_eq!(a.maps.len(), 3);
    assert_eq!(a.maps[2].name, "New action map");
    assert_eq!(a.maps[2].actions[0].name, "New action");
    assert!(h.state().input_actions.renaming.is_some());
    key(&mut h, Key::Enter);
    // Back to Player: Jump ▸ + ▸ Add Binding With One Modifier.
    h.get_by_label("Player").click();
    h.run();
    // The + on Jump's row.
    let row = h.get_by_label("▾ Jump").rect();
    let plus = h
        .get_all_by_label("+")
        .find(|n| (n.rect().center().y - row.center().y).abs() < 6.0)
        .expect("a + on the row");
    plus.click();
    h.run();
    h.get_by_label("Add Binding With One Modifier").click();
    h.run();
    let jump = &saved(&root).maps[0].actions[3];
    let b = jump.bindings.last().unwrap();
    assert_eq!(b.composite, "ButtonWithOneModifier");
    assert_eq!(
        b.parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["modifier", "button"]
    );
    assert_eq!(
        h.state().input_actions.selection,
        Selection::Binding(3, 2, Some(0))
    );
    // The modifier part: pick Shift from the path picker (typed here) and add a processor.
    {
        let st = h.state_mut();
        let a = st.input_actions.asset.as_mut().unwrap();
        a.maps[0].actions[3].bindings[2].parts[0].path = "<Keyboard>/shift".into();
    }
    h.run();
    h.get_by_label("Modifier: Shift [Keyboard]");
    h.get_all_by_label("+").last().unwrap().click();
    h.run();
    h.get_by_label("Invert").click();
    h.run();
    let part = &saved(&root).maps[0].actions[3].bindings[2].parts[0];
    assert_eq!(part.processors, "invert");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn control_scheme_filter_hides_other_bindings() {
    let (mut s, root) = project("filter");
    s.input_actions.scheme_filter = Some(1); // Gamepad
    let h = window(s);
    h.get_by_label("Button South [Gamepad]");
    assert!(h.query_by_label("Space [Keyboard]").is_none());
    assert!(h.query_by_label("WASD").is_none());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn create_menu_makes_an_input_actions_asset() {
    let (mut s, root) = project("create");
    s.project.current = root.join("Assets/Input");
    commands::apply(
        &mut s,
        crate::actions::Action::CreateAsset(AssetKind::InputActions),
    );
    let rel = "Assets/Input/New Input Actions.inputactions";
    assert!(root.join(rel).is_file());
    assert!(s.assets.input_actions.contains(rel));
    let a = InputActionAsset::load(&root.join(rel)).unwrap();
    assert_eq!(a.name, "New Input Actions");
    assert!(a.find_action("Player/Move").is_some());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn player_input_inspector_edits_the_component() {
    let (mut s, root) = project("inspector");
    s.refresh_assets();
    let e = s.scene.spawn("Hero", None);
    s.scene.add(
        e,
        PlayerInput {
            actions: Some(PATH.into()),
            ..Default::default()
        },
    );
    s.select(e);
    let mut h = Harness::builder().with_size([420.0, 900.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::inspector_show(ui, s),
        s,
    );
    h.run();
    for label in [
        "Player Input",
        "Actions",
        "Default Map",
        "Behavior",
        "Auto-Switch",
    ] {
        h.get_by_label(label);
    }
    // Send Messages lists the methods scripts get.
    h.get_by_label_contains("OnMove(value)");
    h.get_by_label_contains("OnJump(value)");
    // Invoke Events: per-action call lists; + adds a call to this object's On<Action>.
    h.state_mut()
        .scene
        .world
        .get::<&mut PlayerInput>(e)
        .unwrap()
        .behavior = PlayerInputBehavior::InvokeEvents;
    h.run();
    h.get_by_label("Events");
    h.get_by_label("Player").click();
    h.run();
    h.get_by_label("Jump (CallbackContext)");
    let plus = h.get_all_by_label("+").count();
    assert!(plus >= 7, "one + per Player action: {plus}");
    // The last seven are the Player map's actions; Jump is the fourth.
    h.get_all_by_label("+").nth(plus - 7 + 3).unwrap().click();
    h.run();
    let c = h
        .state()
        .scene
        .world
        .get::<&PlayerInput>(e)
        .unwrap()
        .clone();
    assert_eq!(c.events.len(), 1);
    assert_eq!(c.events[0].action, "Player/Jump");
    assert_eq!(c.events[0].method, "OnJump");
    assert_eq!(c.events[0].target, Some(e));
    std::fs::remove_dir_all(root).ok();
}
