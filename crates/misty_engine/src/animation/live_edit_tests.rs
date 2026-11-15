//! Animator Controllers edited during Play: running Animators move onto the new version.

use hecs::Entity;

use super::animator::{self, AnimatorPlayer};
use super::clip::{AnimationClip, Property};
use super::controller::*;
use crate::scene::{Animator, Scene};

const CTRL: &str = "Assets/Live.controller";
const Y: Property = Property::Position(1);

/// A one-second looping clip holding position.y at `y`.
fn clip(y: f32) -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        ..Default::default()
    };
    c.set_key("", Y, 0.0, y);
    c.set_key("", Y, 1.0, y);
    c
}

fn state(name: &str, motion: &str) -> State {
    State {
        name: name.into(),
        motion: Some(motion.into()),
        ..Default::default()
    }
}

fn param(name: &str, kind: ParamKind, default: f32) -> Parameter {
    Parameter {
        name: name.into(),
        kind,
        default,
    }
}

/// Idle (y = 0), Up (y = 2) and Side (y = 5); Idle fades to Up over a second while `Up` is on.
fn controller() -> AnimatorController {
    AnimatorController {
        parameters: vec![
            param("Jump", ParamKind::Trigger, 0.0),
            param("Up", ParamKind::Bool, 0.0),
            param("Speed", ParamKind::Float, 0.0),
            param("Mode", ParamKind::Int, 0.0),
        ],
        base: StateMachine {
            states: vec![
                state("Idle", "idle.anim"),
                state("Up", "up.anim"),
                state("Side", "side.anim"),
            ],
            default_state: Some("Idle".into()),
            transitions: vec![Transition {
                has_exit_time: false,
                duration: 1.0,
                conditions: vec![Condition {
                    parameter: "Up".into(),
                    mode: ConditionMode::If,
                    threshold: 0.0,
                }],
                ..Transition::new(Node::state("Idle"), Node::state("Up"))
            }],
            ..Default::default()
        },
        ..Default::default()
    }
}

fn animated(scene: &mut Scene, name: &str) -> Entity {
    let e = scene.spawn(name, None);
    scene.add(
        e,
        Animator {
            controller: Some(CTRL.into()),
            ..Default::default()
        },
    );
    e
}

fn setup(c: AnimatorController) -> (Scene, Entity, AnimatorPlayer) {
    let mut scene = Scene::new("t");
    let e = animated(&mut scene, "Thing");
    let mut p = AnimatorPlayer::new(std::env::temp_dir());
    p.insert_controller(CTRL, c);
    p.insert_clip("idle.anim", clip(0.0));
    p.insert_clip("up.anim", clip(2.0));
    p.insert_clip("side.anim", clip(5.0));
    (scene, e, p)
}

fn current(p: &mut AnimatorPlayer, s: &Scene, e: Entity) -> (String, f32) {
    let c = p.status(s, e).unwrap().current.unwrap();
    (c.name, c.normalized_time)
}

fn y(s: &Scene, e: Entity) -> f32 {
    s.transform(e).position.y
}

#[test]
fn current_state_and_time_survive_an_edit() {
    let (mut s, e, mut p) = setup(controller());
    p.update(&mut s, 0.0);
    assert!(p.play(&s, e, "Side", -1, Some(0.0)));
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.3);
    // Remove the state before it (indices shift), add one and change a transition.
    let mut c = controller();
    c.base.remove_state("Up");
    c.base.add_state("Wave", [0.0, 0.0]);
    c.base.transitions.push(Transition::new(
        Node::state("Side"),
        Node::state("Idle"),
    ));
    p.reload_controller(CTRL, c);
    let (name, norm) = current(&mut p, &s, e);
    assert_eq!(name, "Side");
    assert!((norm - 0.3).abs() < 1e-5, "{norm}");
    p.update(&mut s, 0.1);
    let (name, norm) = current(&mut p, &s, e);
    assert_eq!(name, "Side");
    assert!((norm - 0.4).abs() < 1e-5, "{norm}");
    assert_eq!(y(&s, e), 5.0);
}

#[test]
fn a_renamed_current_state_keeps_playing() {
    let (mut s, e, mut p) = setup(controller());
    p.update(&mut s, 0.0);
    assert!(p.play(&s, e, "Side", -1, Some(0.25)));
    p.update(&mut s, 0.0);
    let mut c = controller();
    c.base.rename_state("Side", "Strafe");
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.1);
    let (name, norm) = current(&mut p, &s, e);
    assert_eq!(name, "Strafe");
    assert!((norm - 0.35).abs() < 1e-5, "{norm}");
}

#[test]
fn a_removed_current_state_falls_back_to_the_default() {
    let (mut s, e, mut p) = setup(controller());
    p.update(&mut s, 0.0);
    assert!(p.play(&s, e, "Side", -1, Some(0.5)));
    p.update(&mut s, 0.1);
    assert_eq!(y(&s, e), 5.0);
    let mut c = controller();
    c.base.remove_state("Side");
    c.base.default_state = Some("Up".into());
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.1);
    let (name, norm) = current(&mut p, &s, e);
    assert_eq!(name, "Up");
    assert_eq!(norm, 0.0, "starts the default state from its beginning");
    assert_eq!(y(&s, e), 2.0);
}

#[test]
fn a_parameter_type_change_resets_only_that_parameter() {
    let (mut s, e, mut p) = setup(controller());
    p.update(&mut s, 0.0);
    p.set_param(&s, e, "Speed", animator::FLOAT, 0.7);
    p.set_param(&s, e, "Mode", animator::INT, 2.0);
    let mut c = controller();
    c.parameters[3] = param("Mode", ParamKind::Float, 0.5);
    c.parameters.remove(0); // Jump
    c.parameters.push(param("Fresh", ParamKind::Int, 3.0));
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.016);
    let params = p.status(&s, e).unwrap().params;
    let get = |n: &str| params.iter().find(|q| q.0 == n).map(|q| (q.1, q.2));
    assert_eq!(get("Speed"), Some((ParamKind::Float, 0.7)), "kept");
    assert_eq!(get("Up"), Some((ParamKind::Bool, 0.0)), "kept");
    assert_eq!(get("Mode"), Some((ParamKind::Float, 0.5)), "new type: new default");
    assert_eq!(get("Fresh"), Some((ParamKind::Int, 3.0)), "added: its default");
    assert_eq!(get("Jump"), None, "removed");
    assert_eq!(params.len(), 4);
}

#[test]
fn a_changed_default_keeps_the_running_value() {
    // A new default does not overwrite the running value; parameters that start fresh use it.
    let (mut s, e, mut p) = setup(controller());
    p.update(&mut s, 0.0);
    p.set_param(&s, e, "Speed", animator::FLOAT, 0.7);
    let mut c = controller();
    c.parameters[2].default = 9.0;
    p.reload_controller(CTRL, c);
    assert_eq!(p.param(&s, e, "Speed", animator::FLOAT), 0.7);
}

/// Start the Idle → Up cross-fade and run it halfway.
fn halfway(p: &mut AnimatorPlayer, s: &mut Scene, e: Entity) {
    p.update(s, 0.0);
    p.set_param(s, e, "Up", animator::BOOL, 1.0);
    p.update(s, 0.0);
    p.update(s, 0.5);
    let st = p.status(s, e).unwrap();
    assert_eq!(st.next.as_ref().unwrap().0.name, "Up", "fading");
    assert!((y(s, e) - 1.0).abs() < 1e-4, "{}", y(s, e));
}

#[test]
fn a_removed_transition_mid_blend_snaps_to_its_destination() {
    let (mut s, e, mut p) = setup(controller());
    halfway(&mut p, &mut s, e);
    let mut c = controller();
    c.base.transitions.clear();
    p.reload_controller(CTRL, c);
    let st = p.status(&s, e).unwrap();
    assert!(st.next.is_none(), "no blend left");
    assert_eq!(st.current.unwrap().name, "Up");
    p.update(&mut s, 0.1);
    assert_eq!(y(&s, e), 2.0);
    assert_eq!(current(&mut p, &s, e).0, "Up");
}

#[test]
fn a_removed_destination_mid_blend_stays_on_the_source() {
    let (mut s, e, mut p) = setup(controller());
    halfway(&mut p, &mut s, e);
    let mut c = controller();
    c.base.remove_state("Up");
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.1);
    let st = p.status(&s, e).unwrap();
    assert!(st.next.is_none());
    assert_eq!(st.current.unwrap().name, "Idle");
    assert_eq!(y(&s, e), 0.0);
}

#[test]
fn a_kept_transition_keeps_blending_after_an_edit() {
    let (mut s, e, mut p) = setup(controller());
    halfway(&mut p, &mut s, e);
    let mut c = controller();
    // Other edits: a new state and a changed condition on the transition being taken.
    c.base.add_state("Wave", [0.0, 0.0]);
    c.base.transitions[0].conditions[0].mode = ConditionMode::IfNot;
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.25);
    let st = p.status(&s, e).unwrap();
    let (to, progress) = st.next.unwrap();
    assert_eq!(to.name, "Up");
    assert!((progress - 0.75).abs() < 1e-4, "{progress}");
    assert!((y(&s, e) - 1.5).abs() < 1e-4, "{}", y(&s, e));
}

#[test]
fn a_blend_tree_threshold_change_takes_effect() {
    let tree = |t: f32| BlendTree {
        parameter: "Speed".into(),
        children: vec![
            BlendChild::clip("idle.anim"),
            BlendChild {
                threshold: t,
                ..BlendChild::clip("up.anim")
            },
        ],
        ..Default::default()
    };
    let mut c = controller();
    c.base.states[0].blend_tree = Some(tree(1.0));
    let (mut s, e, mut p) = setup(c.clone());
    p.set_param(&s, e, "Speed", animator::FLOAT, 0.5);
    p.update(&mut s, 0.0);
    assert!((y(&s, e) - 1.0).abs() < 1e-4, "{}", y(&s, e));
    c.base.states[0].blend_tree = Some(tree(0.5));
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.016);
    assert!((y(&s, e) - 2.0).abs() < 1e-4, "{}", y(&s, e));
}

#[test]
fn a_new_motion_clip_on_the_current_state_plays() {
    let (mut s, e, mut p) = setup(controller());
    p.update(&mut s, 0.0);
    assert_eq!(y(&s, e), 0.0);
    let mut c = controller();
    c.base.states[0].motion = Some("side.anim".into());
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.016);
    assert_eq!(y(&s, e), 5.0);
}

#[test]
fn every_animator_sharing_the_controller_updates() {
    let (mut s, a, mut p) = setup(controller());
    let b = animated(&mut s, "Other");
    p.update(&mut s, 0.0);
    assert!(p.play(&s, b, "Side", -1, Some(0.2)));
    p.update(&mut s, 0.1);
    let mut c = controller();
    c.base.rename_state("Idle", "Rest");
    c.base.states[2].motion = Some("up.anim".into());
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, a).0, "Rest");
    assert_eq!(current(&mut p, &s, b).0, "Side");
    assert_eq!(y(&s, b), 2.0, "Side's new clip");
    assert_eq!(p.controller_rebinds(), 2);
}

#[test]
fn nothing_reloads_while_nothing_changes() {
    let (mut s, e, mut p) = setup(controller());
    let other = animated(&mut s, "Other");
    for _ in 0..20 {
        p.update(&mut s, 0.016);
        p.status(&s, e);
    }
    assert_eq!(p.controller_rebinds(), 0);
    p.reload_controller(CTRL, controller());
    // Another controller's version does not touch these Animators.
    p.reload_controller("Assets/Elsewhere.controller", controller());
    for _ in 0..20 {
        p.update(&mut s, 0.016);
        p.status(&s, other);
    }
    assert_eq!(p.controller_rebinds(), 2, "each Animator moved once");
}

#[test]
fn layers_follow_renames_and_removals() {
    let mut c = controller();
    let l = c.add_layer("Upper");
    c.layer_mut(l).unwrap().machine.states = vec![state("Wave", "side.anim")];
    c.layer_mut(l).unwrap().machine.default_state = Some("Wave".into());
    let (mut s, e, mut p) = setup(c.clone());
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.4);
    assert!(p.set_layer_weight(&s, e, 1, 0.5));
    c.rename_layer(1, "Arms");
    p.reload_controller(CTRL, c.clone());
    p.update(&mut s, 0.1);
    let st = p.status(&s, e).unwrap();
    assert_eq!(st.layers[1].name, "Arms");
    assert_eq!(st.layers[1].weight, 0.5, "script weight kept");
    let wave = st.layers[1].current.clone().unwrap();
    assert_eq!(wave.name, "Wave");
    assert!((wave.normalized_time - 0.5).abs() < 1e-5);
    c.remove_layer(1);
    p.reload_controller(CTRL, c);
    p.update(&mut s, 0.1);
    assert_eq!(p.status(&s, e).unwrap().layers.len(), 1);
}
