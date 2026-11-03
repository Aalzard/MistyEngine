//! Animator: conditions, triggers, exit time, cross-fades, blend trees, write defaults, files.

use hecs::Entity;

use super::animator::{self, AnimatorPlayer, exit_time_reached};
use super::clip::{AnimationClip, Property};
use super::controller::*;
use crate::scene::{Animator, Scene};

const CTRL: &str = "Assets/Test.controller";
const Y: Property = Property::Position(1);

/// A one-second looping clip holding position.y at `y` (or ramping from `y` to `y2`).
fn clip(y: f32, y2: f32) -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        ..Default::default()
    };
    c.set_key("", Y, 0.0, y);
    c.set_key("", Y, 1.0, y2);
    for i in 0..2 {
        c.curves[0].keys.set_tangent(i, super::TangentMode::Linear);
    }
    c
}

fn state(name: &str, motion: &str) -> State {
    State {
        name: name.into(),
        motion: Some(motion.into()),
        ..Default::default()
    }
}

fn param(name: &str, kind: ParamKind) -> Parameter {
    Parameter {
        name: name.into(),
        kind,
        default: 0.0,
    }
}

fn cond(p: &str, mode: ConditionMode, threshold: f32) -> Condition {
    Condition {
        parameter: p.into(),
        mode,
        threshold,
    }
}

/// Idle (y = 0) and Up (y = 2); `transitions` link them.
fn controller(transitions: Vec<Transition>) -> AnimatorController {
    AnimatorController {
        parameters: vec![
            param("Jump", ParamKind::Trigger),
            param("Up", ParamKind::Bool),
            param("Speed", ParamKind::Float),
            param("Mode", ParamKind::Int),
        ],
        states: vec![state("Idle", "idle.anim"), state("Up", "up.anim")],
        default_state: Some("Idle".into()),
        transitions,
        ..Default::default()
    }
}

fn instant(from: Node, to: Node, conditions: Vec<Condition>) -> Transition {
    Transition {
        has_exit_time: false,
        duration: 0.0,
        conditions,
        ..Transition::new(from, to)
    }
}

fn setup(c: AnimatorController) -> (Scene, Entity, AnimatorPlayer) {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    scene.add(
        e,
        Animator {
            controller: Some(CTRL.into()),
            ..Default::default()
        },
    );
    let mut p = AnimatorPlayer::new(std::env::temp_dir());
    p.insert_controller(CTRL, c);
    p.insert_clip("idle.anim", clip(0.0, 0.0));
    p.insert_clip("up.anim", clip(2.0, 2.0));
    p.insert_clip("ramp.anim", clip(0.0, 10.0));
    (scene, e, p)
}

fn current(p: &mut AnimatorPlayer, s: &Scene, e: Entity) -> String {
    p.status(s, e).unwrap().current.unwrap().name
}

fn y(s: &Scene, e: Entity) -> f32 {
    s.transform(e).position.y
}

#[test]
fn condition_modes_compare_values() {
    use ConditionMode::*;
    assert!(If.test(1.0, 0.0) && !If.test(0.0, 0.0));
    assert!(IfNot.test(0.0, 0.0) && !IfNot.test(1.0, 0.0));
    assert!(Greater.test(0.6, 0.5) && !Greater.test(0.5, 0.5));
    assert!(Less.test(0.4, 0.5) && !Less.test(0.5, 0.5));
    assert!(Equals.test(3.0, 3.0) && !Equals.test(2.0, 3.0));
    assert!(NotEqual.test(2.0, 3.0) && !NotEqual.test(3.0, 3.0));
    assert_eq!(ParamKind::Trigger.modes(), &[If]);
}

#[test]
fn starts_in_the_default_state_and_poses_at_once() {
    let (mut s, e, mut p) = setup(controller(vec![]));
    s.set_transform(
        e,
        crate::scene::Transform::from_position(glam::Vec3::new(0.0, 5.0, 0.0)),
    );
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Idle");
    assert_eq!(y(&s, e), 0.0);
}

#[test]
fn trigger_fires_once_and_is_consumed() {
    let t = vec![
        instant(
            Node::state("Idle"),
            Node::state("Up"),
            vec![cond("Jump", ConditionMode::If, 0.0)],
        ),
        instant(
            Node::state("Up"),
            Node::state("Idle"),
            vec![cond("Jump", ConditionMode::If, 0.0)],
        ),
    ];
    let (mut s, e, mut p) = setup(controller(t));
    p.update(&mut s, 0.016);
    p.set_param(&s, e, "Jump", animator::TRIGGER, 1.0);
    p.update(&mut s, 0.016);
    assert_eq!(current(&mut p, &s, e), "Up");
    assert_eq!(y(&s, e), 2.0);
    assert_eq!(p.param(&s, e, "Jump", animator::TRIGGER), 0.0, "consumed");
    // Without a new trigger it stays in Up.
    p.update(&mut s, 0.016);
    assert_eq!(current(&mut p, &s, e), "Up");
}

#[test]
fn bool_float_and_int_conditions_gate_transitions() {
    let t = vec![instant(
        Node::state("Idle"),
        Node::state("Up"),
        vec![
            cond("Up", ConditionMode::If, 0.0),
            cond("Speed", ConditionMode::Greater, 0.5),
            cond("Mode", ConditionMode::Equals, 2.0),
        ],
    )];
    let (mut s, e, mut p) = setup(controller(t));
    p.set_param(&s, e, "Up", animator::BOOL, 1.0);
    p.set_param(&s, e, "Speed", animator::FLOAT, 0.7);
    p.update(&mut s, 0.016);
    assert_eq!(current(&mut p, &s, e), "Idle", "Mode is still 0");
    p.set_param(&s, e, "Mode", animator::INT, 2.0);
    p.update(&mut s, 0.016);
    assert_eq!(current(&mut p, &s, e), "Up");
    // Wrong type or missing name: a warning, nothing changes.
    p.take_logs();
    p.set_param(&s, e, "Speed", animator::BOOL, 1.0);
    p.set_param(&s, e, "Nope", animator::FLOAT, 1.0);
    let logs = p.take_logs();
    assert_eq!(logs.len(), 2, "{logs:?}");
    assert!(logs[1].contains("does not exist"));
}

#[test]
fn exit_time_waits_for_the_clip() {
    let back = Transition {
        has_exit_time: true,
        exit_time: 1.0,
        duration: 0.0,
        ..Transition::new(Node::state("Up"), Node::state("Idle"))
    };
    let mut c = controller(vec![back]);
    c.default_state = Some("Up".into());
    let (mut s, e, mut p) = setup(c);
    p.update(&mut s, 0.0);
    for _ in 0..9 {
        p.update(&mut s, 0.1);
    }
    assert_eq!(current(&mut p, &s, e), "Up", "0.9 of the clip");
    p.update(&mut s, 0.15);
    assert_eq!(current(&mut p, &s, e), "Idle");
    // Exit times below 1 repeat every loop; above 1 fire once.
    assert!(exit_time_reached(1.7, 1.8, 0.75, true));
    assert!(!exit_time_reached(1.2, 1.3, 0.75, true));
    assert!(exit_time_reached(1.7, 2.1, 0.75, true), "wrapped past it");
    assert!(exit_time_reached(2.5, 2.6, 2.0, true));
    assert!(!exit_time_reached(0.5, 0.6, 0.75, true));
}

#[test]
fn cross_fade_blends_sampled_values() {
    let t = vec![Transition {
        has_exit_time: false,
        duration: 1.0,
        conditions: vec![cond("Up", ConditionMode::If, 0.0)],
        ..Transition::new(Node::state("Idle"), Node::state("Up"))
    }];
    let (mut s, e, mut p) = setup(controller(t));
    p.update(&mut s, 0.0);
    p.set_param(&s, e, "Up", animator::BOOL, 1.0);
    p.update(&mut s, 0.0);
    let st = p.status(&s, e).unwrap();
    assert_eq!(st.next.as_ref().unwrap().0.name, "Up");
    p.update(&mut s, 0.5);
    assert!((y(&s, e) - 1.0).abs() < 1e-4, "halfway: {}", y(&s, e));
    assert_eq!(current(&mut p, &s, e), "Idle", "still fading");
    p.update(&mut s, 0.6);
    assert_eq!(current(&mut p, &s, e), "Up");
    assert!(p.status(&s, e).unwrap().next.is_none());
    assert_eq!(y(&s, e), 2.0);
}

#[test]
fn any_state_and_exit_transitions() {
    let t = vec![
        instant(
            Node::AnyState,
            Node::state("Up"),
            vec![cond("Up", ConditionMode::If, 0.0)],
        ),
        instant(
            Node::state("Up"),
            Node::Exit,
            vec![cond("Up", ConditionMode::IfNot, 0.0)],
        ),
    ];
    let (mut s, e, mut p) = setup(controller(t));
    p.set_param(&s, e, "Up", animator::BOOL, 1.0);
    p.update(&mut s, 0.016);
    assert_eq!(current(&mut p, &s, e), "Up");
    // Any State does not restart the state it already plays.
    p.update(&mut s, 0.3);
    let norm = p.status(&s, e).unwrap().current.unwrap().normalized_time;
    assert!(norm > 0.25, "{norm}");
    // Exit goes back to the default state.
    p.set_param(&s, e, "Up", animator::BOOL, 0.0);
    p.update(&mut s, 0.016);
    assert_eq!(current(&mut p, &s, e), "Idle");
}

#[test]
fn play_and_cross_fade_from_scripts() {
    let (mut s, e, mut p) = setup(controller(vec![]));
    p.update(&mut s, 0.0);
    assert!(p.play(&s, e, "Up", None));
    p.update(&mut s, 0.0);
    assert_eq!(current(&mut p, &s, e), "Up");
    assert!(!p.play(&s, e, "Missing", None));
    assert!(
        p.take_logs()
            .iter()
            .any(|l| l.contains("could not be found"))
    );
    assert!(p.cross_fade(&s, e, "Base Layer.Idle", 0.5, false, 0.0));
    p.update(&mut s, 0.25);
    assert!((y(&s, e) - 1.0).abs() < 1e-4, "{}", y(&s, e));
}

#[test]
fn speed_and_normalized_time() {
    let mut c = controller(vec![]);
    c.states[0].motion = Some("ramp.anim".into());
    c.states[0].speed = 2.0;
    let (mut s, e, mut p) = setup(c);
    s.world.get::<&mut Animator>(e).unwrap().speed = 0.5;
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.3);
    let info = p.status(&s, e).unwrap().current.unwrap();
    assert!((info.normalized_time - 0.3).abs() < 1e-5);
    assert!((y(&s, e) - 3.0).abs() < 1e-3, "{}", y(&s, e));
}

#[test]
fn blend_tree_mixes_children_by_parameter() {
    let tree = BlendTree {
        parameter: "Speed".into(),
        children: vec![
            BlendChild {
                motion: Some("up.anim".into()),
                threshold: 1.0,
            },
            BlendChild {
                motion: Some("idle.anim".into()),
                threshold: 0.0,
            },
        ],
    };
    assert_eq!(tree.weights(-1.0), vec![(1, 1.0)]);
    assert_eq!(tree.weights(5.0), vec![(0, 1.0)]);
    let mut c = controller(vec![]);
    c.states[0].blend_tree = Some(tree);
    let (mut s, e, mut p) = setup(c);
    p.set_param(&s, e, "Speed", animator::FLOAT, 0.25);
    p.update(&mut s, 0.0);
    assert!((y(&s, e) - 0.5).abs() < 1e-4, "{}", y(&s, e));
}

#[test]
fn unanimated_properties_return_to_their_rest_value() {
    let mut c = controller(vec![instant(
        Node::state("Up"),
        Node::state("Idle"),
        vec![cond("Up", ConditionMode::IfNot, 0.0)],
    )]);
    c.default_state = Some("Up".into());
    c.states[0].motion = None; // Idle animates nothing.
    let (mut s, e, mut p) = setup(c);
    s.set_transform(
        e,
        crate::scene::Transform::from_position(glam::Vec3::new(0.0, 7.0, 0.0)),
    );
    p.set_param(&s, e, "Up", animator::BOOL, 1.0);
    p.update(&mut s, 0.0);
    assert_eq!(y(&s, e), 2.0);
    p.set_param(&s, e, "Up", animator::BOOL, 0.0);
    p.update(&mut s, 0.1);
    p.update(&mut s, 0.1);
    assert_eq!(y(&s, e), 7.0, "write defaults");
}

#[test]
fn disabling_restarts_unless_state_is_kept() {
    let (mut s, e, mut p) = setup(controller(vec![]));
    p.update(&mut s, 0.0);
    p.play(&s, e, "Up", None);
    p.update(&mut s, 0.1);
    s.world.get::<&mut Animator>(e).unwrap().enabled = false;
    p.update(&mut s, 0.1);
    s.world.get::<&mut Animator>(e).unwrap().enabled = true;
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Idle");
    s.world
        .get::<&mut Animator>(e)
        .unwrap()
        .keep_state_on_disable = true;
    p.play(&s, e, "Up", None);
    s.world.get::<&mut Animator>(e).unwrap().enabled = false;
    p.update(&mut s, 0.1);
    s.world.get::<&mut Animator>(e).unwrap().enabled = true;
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Up");
}

#[test]
fn controller_round_trips_and_follows_renames() {
    let t = vec![instant(
        Node::state("Idle"),
        Node::state("Up"),
        vec![cond("Jump", ConditionMode::If, 0.0)],
    )];
    let mut c = controller(t);
    let dir = std::env::temp_dir().join(format!("misty_ctrl_{}", std::process::id()));
    let path = dir.join("A.controller");
    c.save(&path).unwrap();
    assert_eq!(AnimatorController::load(&path).unwrap(), c);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(c.rename_state("Up", "Idle").as_deref(), Some("Idle 1"));
    assert_eq!(c.transitions[0].to, Node::state("Idle 1"));
    c.rename_parameter("Jump", "Hop");
    assert_eq!(c.transitions[0].conditions[0].parameter, "Hop");
    c.remove_state("Idle");
    assert!(c.transitions.is_empty());
    assert_eq!(c.default_state.as_deref(), Some("Idle 1"));
    assert_eq!(c.add_parameter(ParamKind::Bool), "New Bool");
    assert_eq!(c.add_parameter(ParamKind::Bool), "New Bool 1");

    // A broken default state and dangling transitions are repaired on load.
    let text = r#"{"states":[{"name":"A"}],"default_state":"Gone",
        "transitions":[{"from":{"State":"A"},"to":{"State":"Gone"}}]}"#;
    let c = AnimatorController::from_json(text).unwrap();
    assert_eq!(c.default_state.as_deref(), Some("A"));
    assert!(c.transitions.is_empty());
    assert!(is_controller("Assets/X.controller"));
}
