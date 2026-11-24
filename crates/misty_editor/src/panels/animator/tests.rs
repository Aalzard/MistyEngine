//! Animator window: graph menus (Create State, Make Transition, Set Default, Delete), dragging
//! nodes, parameters, the Inspector for a selected transition, undo, and the live view.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::animation::AnimatorController;
use misty_engine::animation::controller::{Node, ParamKind, State, StateMachine};

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
        base: StateMachine {
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
        },
        ..Default::default()
    };
    s.anim_clips.controllers.create(PATH, c).unwrap();
    s.animator.open(Some(PATH.into()));
    (s, root)
}

/// The Animator window on the left, the Inspector on the right.
fn window(s: EditorState) -> Harness<'static, EditorState> {
    window_sized(s, 600.0)
}

/// [`window`] with a taller Inspector (long blend tree editors).
fn window_sized(s: EditorState, height: f32) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1200.0, height])
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
        saved(&root).base.state("New State").is_some(),
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
    assert_eq!(c.base.transitions.len(), 1);
    assert_eq!(c.base.transitions[0].label(), "Idle -> Walk");
    assert!(c.base.transitions[0].has_exit_time);
    // The Inspector shows the selected transition.
    h.get_by_label("Idle -> Walk  (Transition)");
    h.get_by_label("Has Exit Time");

    // Right-click Walk ▸ Set as Layer Default State.
    click(&mut h, walk, PointerButton::Secondary);
    h.get_by_label("Set as Layer Default State").click();
    h.run();
    assert_eq!(saved(&root).base.default_state.as_deref(), Some("Walk"));

    // Undo twice: the default goes back to Idle, then the transition disappears.
    for _ in 0..2 {
        h.state_mut().push(Action::Undo);
        apply_actions(h.state_mut());
    }
    h.run();
    let c = saved(&root);
    assert_eq!(c.base.default_state.as_deref(), Some("Idle"));
    assert!(c.base.transitions.is_empty());

    // Right-click New State ▸ Delete.
    let new_state = h.get_by_label("New State").rect().center();
    click(&mut h, new_state, PointerButton::Secondary);
    h.get_by_label("Delete").click();
    h.run();
    assert!(saved(&root).base.state("New State").is_none());
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
    let x = saved(&root).base.state("Walk").unwrap().position[0];
    assert!(x > 60.0, "moved right: {x}");
    h.state_mut().push(Action::Undo);
    apply_actions(h.state_mut());
    assert_eq!(saved(&root).base.state("Walk").unwrap().position[0], 0.0);
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

#[test]
fn edits_during_play_reach_the_running_animator_and_survive_stop() {
    let (mut s, root) = project("live_edit");
    let hero = s.scene.spawn("Hero", None);
    s.scene.add(
        hero,
        misty_engine::scene::Animator {
            controller: Some(PATH.into()),
            ..Default::default()
        },
    );
    crate::play::start(&mut s);
    crate::play::tick(&mut s, 1.0 / 60.0);
    crate::play::tick(&mut s, 0.25);
    let status = |s: &EditorState| {
        s.runtime
            .as_ref()
            .unwrap()
            .animator_status(&s.scene, hero)
            .unwrap()
    };
    let before = status(&s).current.unwrap();
    // Rename the playing state, add a parameter and a state (as the window does: undo + save).
    assert!(crate::anim_controllers::edit(
        &mut s,
        PATH,
        "Rename State",
        false,
        |c| {
            c.base.rename_state("Idle", "Rest");
            c.add_parameter(ParamKind::Float);
            c.base.add_state("Wave", [200.0, 0.0]);
        }
    ));
    // Paused: the edit still reaches the game (and the live view) without a step.
    s.play = crate::state::PlayState::Paused;
    crate::play::tick(&mut s, 1.0 / 60.0);
    let st = status(&s);
    let cur = st.current.unwrap();
    assert_eq!(cur.name, "Rest");
    assert_eq!(cur.normalized_time, before.normalized_time, "time kept");
    // The saved file and the running copy agree.
    let disk = saved(&root);
    let names: Vec<&str> = disk.base.states.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Rest", "Walk", "Wave"]);
    let disk_params: Vec<(String, ParamKind, f32)> = disk
        .parameters
        .iter()
        .map(|p| (p.name.clone(), p.kind, p.default))
        .collect();
    assert_eq!(st.params, disk_params);
    // The live view highlights the renamed state.
    let mut h = window(s);
    h.get_by_label("● Live");
    let s = h.state_mut();
    // Undo in Play mode reaches the game too.
    commands::apply(s, Action::Undo);
    s.play = crate::state::PlayState::Playing;
    crate::play::tick(s, 1.0 / 60.0);
    assert_eq!(status(s).current.unwrap().name, "Idle");
    assert!(crate::anim_controllers::edit(
        s,
        PATH,
        "Rename State",
        false,
        |c| {
            c.base.rename_state("Idle", "Rest");
        }
    ));
    crate::play::tick(s, 1.0 / 60.0);
    assert_eq!(status(s).current.unwrap().name, "Rest");
    // Stop keeps the edit, in the file and in the editor.
    crate::play::stop(s);
    assert!(saved(&root).base.state("Rest").is_some());
    let held = s.anim_clips.controllers.get(PATH).unwrap();
    assert!(held.base.state("Rest").is_some());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn nothing_is_handed_to_the_game_while_no_controller_changes() {
    let (mut s, root) = project("live_idle");
    let hero = s.scene.spawn("Hero", None);
    s.scene.add(
        hero,
        misty_engine::scene::Animator {
            controller: Some(PATH.into()),
            ..Default::default()
        },
    );
    crate::play::start(&mut s);
    let generation = s.anim_clips.controllers.generation();
    for _ in 0..10 {
        crate::play::tick(&mut s, 1.0 / 60.0);
    }
    assert_eq!(s.anim_clips.controllers.generation(), generation);
    assert!(
        s.anim_clips
            .controllers
            .changed_since(generation)
            .is_empty()
    );
    let rebinds = |s: &EditorState| s.runtime.as_ref().unwrap().animator_rebinds();
    assert_eq!(rebinds(&s), 0);
    crate::play::stop(&mut s);
    std::fs::remove_dir_all(root).ok();
}

// ---- layers, sub-state machines, blend trees, masks ------------------------------------------

#[test]
fn layers_tab_adds_a_layer_and_edits_its_settings() {
    let (mut s, root) = project("layers");
    let mask = "Assets/Upper.mask";
    misty_engine::animation::AvatarMask::default()
        .save(&root.join(mask))
        .unwrap();
    s.assets.masks.insert(mask.into());
    let mut h = window(s);
    h.get_by_label("Layers").click();
    h.run();
    assert_eq!(
        h.query_all_by_label("Base Layer").count(),
        2,
        "row and breadcrumb"
    );
    h.get_by_label("+").click();
    h.run();
    let c = saved(&root);
    assert_eq!(c.layers.len(), 1);
    assert_eq!(c.layers[0].name, "New Layer");
    assert_eq!(h.state().animator.layer, 1, "the new layer is shown");
    // The graph now shows the new (empty) layer's machine.
    assert!(h.query_by_label("Idle").is_none());
    // The selected layer's settings: Blending Additive, Mask Upper.
    h.get_by_value("Override").click();
    h.run();
    h.get_by_label("Additive").click();
    h.run();
    assert_eq!(
        saved(&root).layers[0].blending,
        misty_engine::animation::controller::LayerBlending::Additive
    );
    h.get_by_value("None (Avatar Mask)").click();
    h.run();
    h.get_by_label("Upper").click();
    h.run();
    assert_eq!(saved(&root).layers[0].mask.as_deref(), Some(mask));
    // Back to the base layer: its states again.
    h.state_mut().animator.show_layer(0);
    h.run();
    h.get_by_label("Idle");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn sub_state_machines_are_created_entered_and_left() {
    let (s, root) = project("submachine");
    let mut h = window(s);
    let empty = h.get_by_label("Exit").rect().center() + egui::vec2(-60.0, 160.0);
    click(&mut h, empty, PointerButton::Secondary);
    h.get_by_label("Create Sub-State Machine").click();
    h.run();
    let c = saved(&root);
    assert_eq!(c.base.sub_machines.len(), 1);
    let path = c.base.sub_machines[0].path.clone();
    assert_eq!(path, "New StateMachine");

    // Idle: Make Transition onto the sub-machine node.
    let idle = h.get_by_label("Idle").rect().center();
    click(&mut h, idle, PointerButton::Secondary);
    h.get_by_label("Make Transition").click();
    h.run();
    let node = h.get_by_label("New StateMachine").rect().center();
    click(&mut h, node, PointerButton::Primary);
    assert_eq!(
        saved(&root).base.transitions[0].to,
        Node::Machine(path.clone())
    );

    // Right-click it, Open: its graph has the up node and the breadcrumb shows it.
    click(&mut h, node, PointerButton::Secondary);
    h.get_by_label("Open").click();
    h.run();
    assert_eq!(h.state().animator.machine, path);
    h.get_by_label("(Up) Base Layer");
    assert!(
        h.query_by_label("Walk").is_none(),
        "Walk lives in the parent"
    );
    // A state made here belongs to the sub-machine and becomes its default.
    let empty = h.get_by_label("Exit").rect().center() + egui::vec2(-60.0, 160.0);
    click(&mut h, empty, PointerButton::Secondary);
    h.get_by_label("Create State ⏵").hover();
    h.run();
    h.get_by_label("Empty").click();
    h.run();
    let c = saved(&root);
    let st = c.base.state("New State").unwrap();
    assert_eq!(st.machine, path);
    assert_eq!(
        c.base.sub(&path).unwrap().default_state.as_deref(),
        Some("New State")
    );
    // The Inspector shows the sub-machine when it is selected from the parent.
    h.get_by_label("Base Layer").click();
    h.run();
    assert_eq!(h.state().animator.machine, "");
    let node = h.get_by_label("New StateMachine").rect().center();
    click(&mut h, node, PointerButton::Primary);
    h.get_by_label("New StateMachine  (Sub-State Machine)");
    h.get_by_label("Default State");
    std::fs::remove_dir_all(root).ok();
}

/// A controller with floats X and Y and a "Move" blend tree state of `blend_type`.
fn blend_project(
    tag: &str,
    blend_type: misty_engine::animation::controller::BlendType,
) -> (EditorState, std::path::PathBuf) {
    use misty_engine::animation::controller::{BlendChild, BlendTree, Parameter};
    let (mut s, root) = project(tag);
    let child = |name: &str, x: f32, y: f32| BlendChild {
        position: [x, y],
        threshold: x,
        ..BlendChild::clip(&format!("Assets/{name}.anim"))
    };
    crate::anim_controllers::edit(&mut s, PATH, "setup", false, |c| {
        for n in ["X", "Y"] {
            c.parameters.push(Parameter {
                name: n.into(),
                kind: ParamKind::Float,
                default: 0.0,
            });
        }
        let name = c.base.add_state("Move", [200.0, 0.0]);
        c.base.state_mut(&name).unwrap().blend_tree = Some(BlendTree {
            blend_type,
            parameter: "X".into(),
            parameter_y: "Y".into(),
            children: vec![
                child("Idle", 0.0, 0.0),
                child("Walk", 0.0, 1.0),
                child("Right", 1.0, 0.0),
            ],
            ..Default::default()
        });
    });
    s.animator.selected = Some(GraphItem::Node(Node::state("Move")));
    s.inspector_asset = Some(PATH.into());
    (s, root)
}

#[test]
fn blend_tree_editor_changes_type_motions_and_the_preview() {
    use misty_engine::animation::controller::BlendType;
    let (s, root) = blend_project("blendtree", BlendType::SimpleDirectional2D);
    let mut h = window_sized(s, 1000.0);
    h.get_by_label("Parameter Y");
    // Idle sits at the preview point (0, 0): it has all the weight.
    h.get_by_label("100%");
    // Blend Type: 2D Freeform Cartesian.
    h.get_by_value("2D Simple Directional").click();
    h.run();
    h.get_by_label("2D Freeform Cartesian").click();
    h.run();
    let tree = |root: &std::path::Path| {
        saved(root)
            .base
            .state("Move")
            .unwrap()
            .blend_tree
            .clone()
            .unwrap()
    };
    assert_eq!(tree(&root).blend_type, BlendType::FreeformCartesian2D);
    // Add Motion Field: a fourth child to the right of the last one.
    h.get_by_label("+ Add Motion Field").click();
    h.run();
    let t = tree(&root);
    assert_eq!(t.children.len(), 4);
    assert_eq!(t.children[3].position, [2.0, 0.0]);
    // Clicking the diagram moves the preview: the parameter defaults change.
    let plot = h.get_by_label(super::blend_view::DIAGRAM_LABEL).rect();
    let at = plot.right_top() + egui::vec2(-4.0, 4.0);
    click(&mut h, at, PointerButton::Primary);
    let c = saved(&root);
    let x = c.parameter("X").unwrap().default;
    let y = c.parameter("Y").unwrap().default;
    assert!(x > 1.0 && y > 0.5, "top-right corner: ({x}, {y})");
    // 1D trees show Threshold fields instead of positions.
    h.get_by_value("2D Freeform Cartesian").click();
    h.run();
    h.get_by_label("1D").click();
    h.run();
    assert!(h.query_all_by_label("Threshold").count() > 0);
    assert!(h.query_by_label("Parameter Y").is_none());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn blend_tree_view_opens_in_the_window() {
    use misty_engine::animation::controller::BlendType;
    let (s, root) = blend_project("blendview", BlendType::FreeformDirectional2D);
    let mut h = window_sized(s, 1000.0);
    h.get_by_label("Open in Animator Window").click();
    h.run();
    assert_eq!(h.state().animator.blend_view.as_deref(), Some("Move"));
    h.get_by_label("Move (Blend Tree)");
    h.get_by_label("Motions");
    h.get_by_label("2D Freeform Directional Blend Tree");
    // The layer in the breadcrumb goes back to the graph.
    h.get_by_label("Base Layer").click();
    h.run();
    assert_eq!(h.state().animator.blend_view, None);
    h.get_by_label("Walk");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn mask_editor_imports_a_skeleton_and_toggles_bones() {
    use misty_engine::animation::AvatarMask;
    let (mut s, root) = project("mask");
    let mask = "Assets/Upper.mask";
    AvatarMask::default().save(&root.join(mask)).unwrap();
    let hero = s.scene.spawn("Hero", None);
    s.scene.add(hero, misty_engine::scene::Animator::default());
    let spine = s.scene.spawn("Spine", Some(hero));
    s.scene.spawn("Arm", Some(spine));
    s.scene.spawn("Leg", Some(hero));
    s.selection.clear();
    s.inspector_asset = Some(mask.into());
    let mut h = Harness::builder().with_size([500.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::inspector::show(ui, s),
        s,
    );
    h.run();
    h.get_by_label("Upper (Avatar Mask)");
    h.get_by_value("Hero");
    h.get_by_label(crate::panels::inspector::MASK_IMPORT_LABEL)
        .click();
    h.run();
    let load = || AvatarMask::load(&root.join(mask)).unwrap();
    let paths: Vec<String> = load().transforms.iter().map(|t| t.path.clone()).collect();
    assert_eq!(paths, ["", "Spine", "Spine/Arm", "Leg"]);
    assert!(load().transforms.iter().all(|t| t.active));
    h.get_by_label("4 of 4 active");
    // Unticking Spine unticks everything below it.
    h.get_by_label("Spine").click();
    h.run();
    let m = load();
    assert!(!m.is_active("Spine") && !m.is_active("Spine/Arm"));
    assert!(m.is_active("Leg") && m.is_active(""));
    h.get_by_label("2 of 4 active");
    // Undo puts it back.
    h.state_mut().push(Action::Undo);
    apply_actions(h.state_mut());
    h.run();
    assert!(load().is_active("Spine/Arm"));
    // None, then importing again keeps the flags.
    h.get_by_label("None").click();
    h.run();
    h.get_by_label(crate::panels::inspector::MASK_IMPORT_LABEL)
        .click();
    h.run();
    assert!(load().transforms.iter().all(|t| !t.active));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn transition_inspector_edits_interruption() {
    use misty_engine::animation::controller::{InterruptionSource, Transition};
    let (mut s, root) = project("interrupt");
    crate::anim_controllers::edit(&mut s, PATH, "setup", false, |c| {
        c.base
            .transitions
            .push(Transition::new(Node::state("Idle"), Node::state("Walk")));
    });
    s.animator.selected = Some(GraphItem::Transition(0));
    s.inspector_asset = Some(PATH.into());
    let mut h = window(s);
    h.get_by_label("Interruption Source");
    h.get_by_value("None").click();
    h.run();
    h.get_by_label("Next State Then Current State").click();
    h.run();
    let t = saved(&root).base.transitions[0].clone();
    assert_eq!(
        t.interruption_source,
        InterruptionSource::NextStateThenCurrentState
    );
    h.get_by_label("Ordered Interruption");
    std::fs::remove_dir_all(root).ok();
}

/// Renaming the controller asset during Play rewrote the playing scene's Animator to the new
/// path, and the running Animator took that for a different controller: it restarted in its
/// default state at time 0 (parameters reset) instead of carrying on.
#[test]
fn renaming_the_controller_during_play_keeps_the_running_animator() {
    let (mut s, root) = project("live_rename");
    let hero = s.scene.spawn("Hero", None);
    s.scene.add(
        hero,
        misty_engine::scene::Animator {
            controller: Some(PATH.into()),
            ..Default::default()
        },
    );
    crate::play::start(&mut s);
    crate::play::tick(&mut s, 1.0 / 60.0);
    crate::play::tick(&mut s, 0.25);
    let status = |s: &EditorState| {
        s.runtime
            .as_ref()
            .unwrap()
            .animator_status(&s.scene, hero)
            .unwrap()
    };
    let before = status(&s).current.unwrap().normalized_time;
    assert!(before > 0.2, "{before}");
    crate::commands::apply(
        &mut s,
        Action::RenameAsset {
            path: root.join(PATH),
            name: "Champion".into(),
        },
    );
    let new_path = "Assets/Champion.controller";
    assert!(root.join(new_path).exists());
    crate::play::tick(&mut s, 0.1);
    let st = status(&s);
    assert_eq!(st.controller, new_path);
    let cur = st.current.unwrap();
    assert_eq!(cur.name, "Idle");
    assert!(
        (cur.normalized_time - (before + 0.1)).abs() < 1e-4,
        "restarted: {} (was {before})",
        cur.normalized_time
    );
    // Edits to the renamed asset still reach it.
    assert!(crate::anim_controllers::edit(
        &mut s,
        new_path,
        "Rename State",
        false,
        |c| {
            c.base.rename_state("Idle", "Rest");
        }
    ));
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert_eq!(status(&s).current.unwrap().name, "Rest");
    crate::play::stop(&mut s);
    std::fs::remove_dir_all(root).ok();
}

/// Undoing an Animator Controller edit during Play also put back the scene copy taken when the
/// edit was made: everything the game had done since (objects moved, spawned or destroyed)
/// was rewound, though the step only changed the controller asset.
#[test]
fn undoing_a_controller_edit_during_play_leaves_the_running_scene_alone() {
    let (mut s, root) = project("live_undo_scene");
    let hero = s.scene.spawn("Hero", None);
    s.scene.add(
        hero,
        misty_engine::scene::Animator {
            controller: Some(PATH.into()),
            ..Default::default()
        },
    );
    crate::play::start(&mut s);
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert!(crate::anim_controllers::edit(
        &mut s,
        PATH,
        "Rename State",
        false,
        |c| {
            c.base.rename_state("Idle", "Rest");
        }
    ));
    crate::play::tick(&mut s, 1.0 / 60.0);
    // The game moves the hero and spawns a bullet after the edit.
    let mut t = s.scene.transform(hero);
    t.position.x = 7.0;
    s.scene.set_transform(hero, t);
    let bullet = s.scene.spawn("Bullet", None);
    for _ in 0..3 {
        commands::apply(&mut s, Action::Undo);
        commands::apply(&mut s, Action::Redo);
    }
    commands::apply(&mut s, Action::Undo);
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert!(s.scene.contains(bullet), "spawned object kept");
    assert_eq!(s.scene.transform(hero).position.x, 7.0, "game state kept");
    let st = s
        .runtime
        .as_ref()
        .unwrap()
        .animator_status(&s.scene, hero)
        .unwrap();
    assert_eq!(
        st.current.unwrap().name,
        "Idle",
        "the controller edit is undone"
    );
    commands::apply(&mut s, Action::Redo);
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert!(s.scene.contains(bullet));
    let st = s
        .runtime
        .as_ref()
        .unwrap()
        .animator_status(&s.scene, hero)
        .unwrap();
    assert_eq!(st.current.unwrap().name, "Rest");
    crate::play::stop(&mut s);
    std::fs::remove_dir_all(root).ok();
}

/// An Avatar Mask edited in the Inspector during Play was saved, but running Animators kept
/// filtering their layers with the copy they read when Play started (only controllers were
/// handed to the game).
#[test]
fn mask_edits_during_play_reach_running_animator_layers() {
    use misty_engine::animation::clip::Property;
    use misty_engine::animation::controller::{Layer, StateMachine};
    use misty_engine::animation::mask::MaskEntry;
    use misty_engine::animation::{AnimationClip, AvatarMask};
    let (mut s, root) = project("live_mask");
    // Body and Arm held at y = `y` (one second, looping).
    let pose = |y: f32| {
        let mut c = AnimationClip {
            length: 1.0,
            ..Default::default()
        };
        for path in ["Body", "Arm"] {
            c.set_key(path, Property::Position(1), 0.0, y);
            c.set_key(path, Property::Position(1), 1.0, y);
        }
        c
    };
    pose(1.0).save(&root.join("Assets/P1.anim")).unwrap();
    pose(5.0).save(&root.join("Assets/P5.anim")).unwrap();
    let mask = "Assets/Arm.mask";
    let entries = |body: bool| AvatarMask {
        transforms: [("", false), ("Body", body), ("Arm", true)]
            .iter()
            .map(|(p, on)| MaskEntry {
                path: (*p).into(),
                active: *on,
            })
            .collect(),
    };
    entries(false).save(&root.join(mask)).unwrap();
    let ctrl = "Assets/Masked.controller";
    let one = |name: &str, motion: &str| StateMachine {
        states: vec![State {
            name: name.into(),
            motion: Some(motion.into()),
            ..Default::default()
        }],
        default_state: Some(name.into()),
        ..Default::default()
    };
    let c = AnimatorController {
        base: one("Idle", "Assets/P1.anim"),
        layers: vec![Layer {
            name: "Arms".into(),
            weight: 1.0,
            mask: Some(mask.into()),
            machine: one("Wave", "Assets/P5.anim"),
            ..Default::default()
        }],
        ..Default::default()
    };
    s.anim_clips.controllers.create(ctrl, c).unwrap();
    let hero = s.scene.spawn("Hero", None);
    let body = s.scene.spawn("Body", Some(hero));
    let arm = s.scene.spawn("Arm", Some(hero));
    s.scene.add(
        hero,
        misty_engine::scene::Animator {
            controller: Some(ctrl.into()),
            ..Default::default()
        },
    );
    crate::play::start(&mut s);
    crate::play::tick(&mut s, 1.0 / 60.0);
    let y = |s: &EditorState, e| s.scene.transform(e).position.y;
    assert_eq!(y(&s, body), 1.0, "masked out");
    assert_eq!(y(&s, arm), 5.0, "masked in");
    // Switch Body on in the mask (as the Inspector's mask editor does).
    assert!(crate::avatar_masks::edit(
        &mut s,
        mask,
        "Toggle Body",
        false,
        |m| *m = entries(true)
    ));
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert_eq!(y(&s, body), 5.0, "the edited mask lets Body through");
    // Undo reaches the game too.
    commands::apply(&mut s, Action::Undo);
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert_eq!(y(&s, body), 1.0);
    crate::play::stop(&mut s);
    std::fs::remove_dir_all(root).ok();
}

// ---- Round 83: clip settings during Play -----------------------------------------------------

/// Loop Time changed in the Inspector during Play reached nothing: Animations kept the clip
/// they read at Play start until Play was entered again. Clip changes (Loop Time, keys) now
/// reach running Animation components and Animators; undoing Loop Time in Play leaves the
/// running scene alone (the step holds only the clip).
#[test]
fn clip_changes_during_play_reach_running_animations_and_animators() {
    use misty_engine::animation::AnimationClip;
    use misty_engine::animation::clip::Property;
    let (mut s, root) = project("live_clip");
    // Panel.y goes 0 -> 1 over one second (Loop Time on); Body.y held at `y`.
    let ramp = {
        let mut c = AnimationClip {
            length: 1.0,
            looping: true,
            ..Default::default()
        };
        c.set_key("Panel", Property::Position(1), 0.0, 0.0);
        c.set_key("Panel", Property::Position(1), 1.0, 1.0);
        c
    };
    ramp.save(&root.join("Assets/Ramp.anim")).unwrap();
    let hold = |c: &mut AnimationClip, y: f32| {
        c.curves.clear();
        c.set_key("Body", Property::Position(1), 0.0, y);
        c.set_key("Body", Property::Position(1), 1.0, y);
    };
    let mut held = AnimationClip {
        length: 1.0,
        ..Default::default()
    };
    hold(&mut held, 1.0);
    held.save(&root.join("Assets/Hold.anim")).unwrap();
    let ctrl = "Assets/Hold.controller";
    let c = AnimatorController {
        base: StateMachine {
            states: vec![State {
                name: "Hold".into(),
                motion: Some("Assets/Hold.anim".into()),
                ..Default::default()
            }],
            default_state: Some("Hold".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    s.anim_clips.controllers.create(ctrl, c).unwrap();
    let door = s.scene.spawn("Door", None);
    let panel = s.scene.spawn("Panel", Some(door));
    s.scene.add(
        door,
        misty_engine::scene::Animation {
            clip: Some("Assets/Ramp.anim".into()),
            ..Default::default()
        },
    );
    let hero = s.scene.spawn("Hero", None);
    let body = s.scene.spawn("Body", Some(hero));
    s.scene.add(
        hero,
        misty_engine::scene::Animator {
            controller: Some(ctrl.into()),
            ..Default::default()
        },
    );
    let mover = s.scene.spawn("Mover", None);
    crate::play::start(&mut s);
    // Play steps at most 1/3 s a frame: walk in 0.05 s steps.
    let run = |s: &mut EditorState, seconds: f32| {
        for _ in 0..(seconds / 0.05).round() as usize {
            crate::play::tick(s, 0.05);
        }
    };
    crate::play::tick(&mut s, 1.0 / 60.0);
    run(&mut s, 0.5);
    let y = |s: &EditorState, e| s.scene.transform(e).position.y;
    assert!((y(&s, panel) - 0.5).abs() < 0.05, "{}", y(&s, panel));
    assert_eq!(y(&s, body), 1.0);
    // Loop Time off in the Inspector (saved at the end of the frame).
    crate::panels::inspector::anim::set_loop_time(&mut s, "Assets/Ramp.anim", false);
    assert!(s.anim_clips.save_dirty().is_empty());
    // The game moves something meanwhile.
    s.scene.set_transform(
        mover,
        misty_engine::scene::Transform::from_position(glam::Vec3::X * 7.0),
    );
    run(&mut s, 0.75);
    assert_eq!(y(&s, panel), 1.0, "holds the end instead of wrapping");
    // The Animation window's kind of edit: keys of the clip an Animator plays.
    hold(s.anim_clips.get_mut("Assets/Hold.anim").unwrap(), 3.0);
    assert!(s.anim_clips.save_dirty().is_empty());
    crate::play::tick(&mut s, 1.0 / 60.0);
    assert_eq!(
        y(&s, body),
        3.0,
        "the running Animator samples the new keys"
    );
    // Undo Loop Time: the clip goes back, the running scene stays.
    commands::apply(&mut s, Action::Undo);
    assert!(s.anim_clips.save_dirty().is_empty());
    assert!(s.anim_clips.get("Assets/Ramp.anim").unwrap().looping);
    assert_eq!(
        s.scene.transform(mover).position.x,
        7.0,
        "scene not rewound"
    );
    crate::play::stop(&mut s);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn layers_tab_switches_ik_pass_on_the_base_and_other_layers() {
    let (s, root) = project("ikpass");
    let mut h = window(s);
    h.get_by_label("Layers").click();
    h.run();
    // The base layer (selected) has only IK Pass.
    h.get_by_label(super::layers::IK_PASS_LABEL).click();
    h.run();
    assert!(saved(&root).base_ik_pass);
    assert!(saved(&root).ik_pass(0));
    h.get_by_label("IK");
    h.get_by_label("+").click();
    h.run();
    h.get_by_label(super::layers::IK_PASS_LABEL).click();
    h.run();
    let c = saved(&root);
    assert!(c.layers[0].ik_pass && c.ik_pass(1));
    assert_eq!(h.query_all_by_label("IK").count(), 2, "badge on both rows");
    // Undo turns the new layer's IK Pass off again.
    commands::apply(h.state_mut(), Action::Undo);
    h.run();
    assert!(!saved(&root).layers[0].ik_pass);
    assert!(saved(&root).base_ik_pass);
    std::fs::remove_dir_all(root).ok();
}
