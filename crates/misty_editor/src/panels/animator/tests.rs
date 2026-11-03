//! Animator window: graph menus (Create State, Make Transition, Set Default, Delete), dragging
//! nodes, parameters, the Inspector for a selected transition, undo, and the live view.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::animation::AnimatorController;
use misty_engine::animation::controller::{Node, ParamKind, State};

use super::*;
use crate::actions::Action;
use crate::commands;
use crate::project::ProjectBrowser;

const PATH: &str = "Assets/Hero.controller";

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_animator_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    let c = AnimatorController {
        states: vec![
            State {
                name: "Idle".into(),
                position: [0.0, -60.0],
                ..Default::default()
            },
            State {
                name: "Walk".into(),
                position: [0.0, 80.0],
                ..Default::default()
            },
        ],
        default_state: Some("Idle".into()),
        ..Default::default()
    };
    s.anim_clips.controllers.create(PATH, c).unwrap();
    s.animator.open(Some(PATH.into()));
    (s, root)
}

/// The Animator window on the left, the Inspector on the right.
fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1200.0, 600.0])
        .build_ui_state(
            |ui, s: &mut EditorState| {
                let (left, right) = ui.max_rect().split_left_right_at_x(860.0);
                ui.scope_builder(UiBuilder::new().max_rect(left), |ui| show(ui, s));
                ui.scope_builder(UiBuilder::new().max_rect(right), |ui| {
                    crate::panels::inspector::show(ui, s)
                });
            },
            s,
        );
    h.run();
    h
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

fn saved(root: &std::path::Path) -> AnimatorController {
    AnimatorController::load(&root.join(PATH)).unwrap()
}

fn apply_actions(s: &mut EditorState) {
    for a in std::mem::take(&mut s.actions) {
        commands::apply(s, a);
    }
}

#[test]
fn graph_menus_build_the_state_machine_and_undo() {
    let (s, root) = project("menus");
    let mut h = window(s);
    for label in [
        "Entry",
        "Any State",
        "Exit",
        "Idle",
        "Walk",
        "Base Layer",
        "Parameters",
    ] {
        h.get_by_label(label);
    }

    // Right-click empty canvas ▸ Create State ▸ Empty.
    let empty = h.get_by_label("Exit").rect().center() + egui::vec2(-60.0, 160.0);
    click(&mut h, empty, PointerButton::Secondary);
    h.get_by_label("Create State ⏵").hover();
    h.run();
    h.get_by_label("Empty").click();
    h.run();
    assert!(
        saved(&root).state("New State").is_some(),
        "written to the asset"
    );
    assert_eq!(
        h.state().animator.selected,
        Some(GraphItem::Node(Node::state("New State")))
    );

    // Right-click Idle ▸ Make Transition, then click Walk.
    let idle = h.get_by_label("Idle").rect().center();
    click(&mut h, idle, PointerButton::Secondary);
    h.get_by_label("Make Transition").click();
    h.run();
    assert_eq!(h.state().animator.making, Some(Node::state("Idle")));
    let walk = h.get_by_label("Walk").rect().center();
    click(&mut h, walk, PointerButton::Primary);
    let c = saved(&root);
    assert_eq!(c.transitions.len(), 1);
    assert_eq!(c.transitions[0].label(), "Idle -> Walk");
    assert!(c.transitions[0].has_exit_time);
    // The Inspector shows the selected transition.
    h.get_by_label("Idle -> Walk  (Transition)");
    h.get_by_label("Has Exit Time");

    // Right-click Walk ▸ Set as Layer Default State.
    click(&mut h, walk, PointerButton::Secondary);
    h.get_by_label("Set as Layer Default State").click();
    h.run();
    assert_eq!(saved(&root).default_state.as_deref(), Some("Walk"));

    // Undo twice: the default goes back to Idle, then the transition disappears.
    for _ in 0..2 {
        h.state_mut().push(Action::Undo);
        apply_actions(h.state_mut());
    }
    h.run();
    let c = saved(&root);
    assert_eq!(c.default_state.as_deref(), Some("Idle"));
    assert!(c.transitions.is_empty());

    // Right-click New State ▸ Delete.
    let new_state = h.get_by_label("New State").rect().center();
    click(&mut h, new_state, PointerButton::Secondary);
    h.get_by_label("Delete").click();
    h.run();
    assert!(saved(&root).state("New State").is_none());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn dragging_a_node_moves_it_in_one_undo_step() {
    let (s, root) = project("drag");
    let mut h = window(s);
    let from = h.get_by_label("Walk").rect().center();
    h.event(Event::PointerMoved(from));
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=5 {
        h.event(Event::PointerMoved(from + egui::vec2(20.0 * i as f32, 0.0)));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: from + egui::vec2(100.0, 0.0),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let x = saved(&root).state("Walk").unwrap().position[0];
    assert!(x > 60.0, "moved right: {x}");
    h.state_mut().push(Action::Undo);
    apply_actions(h.state_mut());
    assert_eq!(saved(&root).state("Walk").unwrap().position[0], 0.0);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn parameters_are_added_and_listed() {
    let (s, root) = project("params");
    let mut h = window(s);
    h.get_by_label("List is Empty");
    h.get_by_label("+").click();
    h.run();
    h.get_by_label("Trigger").click();
    h.run();
    let c = saved(&root);
    assert_eq!(c.parameters.len(), 1);
    assert_eq!(c.parameters[0].kind, ParamKind::Trigger);
    assert_eq!(
        h.state().animator.renaming.as_ref().map(|r| r.0.as_str()),
        Some("New Trigger"),
        "starts renaming"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn window_menu_opens_the_animator() {
    let (s, root) = project("menu");
    let mut h = Harness::builder()
        .with_size([900.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| crate::menu_bar::show(ui, s), s);
    h.run();
    h.get_by_label("Window").click();
    h.run();
    h.get_by_label("Animation ⏵").hover();
    h.run();
    h.get_by_label("Animator").click();
    h.run();
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert!(
        matches!(
            actions.as_slice(),
            [Action::FocusTab(crate::layout::Tab::Animator)]
        ),
        "{actions:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn play_mode_shows_the_live_state() {
    let (mut s, root) = project("live");
    let hero = s.scene.spawn("Hero", None);
    s.scene.add(
        hero,
        misty_engine::scene::Animator {
            controller: Some(PATH.into()),
            ..Default::default()
        },
    );
    s.selection.set(hero);
    crate::play::start(&mut s);
    crate::play::tick(&mut s, 1.0 / 60.0);
    let mut h = window(s);
    h.get_by_label("● Live");
    assert_eq!(h.state().animator.target, Some(hero));
    let st = h
        .state()
        .runtime
        .as_ref()
        .unwrap()
        .animator_status(&h.state().scene, hero)
        .unwrap();
    assert_eq!(st.current.unwrap().name, "Idle");
    crate::play::stop(h.state_mut());
    std::fs::remove_dir_all(root).ok();
}
