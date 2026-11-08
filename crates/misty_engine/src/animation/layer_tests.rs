//! Layers (override, additive, synced) with Avatar Masks, transition interruption, sub-state
//! machine traversal and root motion.

use glam::Vec3;
use hecs::Entity;

use super::animator::{self, AnimatorPlayer};
use super::clip::{AnimationClip, Property};
use super::controller::*;
use super::mask::{AvatarMask, MaskEntry};
use super::root_motion::{Delta, clip_delta, is_root_key};
use crate::scene::{Animator, Scene, Transform};

const CTRL: &str = "Assets/T.controller";
const Y: Property = Property::Position(1);

/// A one-second looping clip: `path`'s y ramps linearly from `a` to `b`.
fn ramp(path: &str, a: f32, b: f32) -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        ..Default::default()
    };
    c.set_key(path, Y, 0.0, a);
    c.set_key(path, Y, 1.0, b);
    for i in 0..2 {
        c.curves[0].keys.set_tangent(i, super::TangentMode::Linear);
    }
    c
}

/// Body and Arm held at `y`.
fn pose(y: f32) -> AnimationClip {
    let mut c = ramp("Body", y, y);
    c.set_key("Arm", Y, 0.0, y);
    c.set_key("Arm", Y, 1.0, y);
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

fn cond(p: &str, mode: ConditionMode) -> Condition {
    Condition {
        parameter: p.into(),
        mode,
        threshold: 0.0,
    }
}

fn machine(states: Vec<State>, transitions: Vec<Transition>) -> StateMachine {
    StateMachine {
        default_state: states.first().map(|s| s.name.clone()),
        states,
        transitions,
        ..Default::default()
    }
}

/// "Hero" with children "Body" and "Arm" (rest y = 0), playing `c`.
fn setup(c: AnimatorController) -> (Scene, Entity, AnimatorPlayer) {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Hero", None);
    scene.spawn("Body", Some(e));
    scene.spawn("Arm", Some(e));
    scene.add(
        e,
        Animator {
            controller: Some(CTRL.into()),
            ..Default::default()
        },
    );
    let mut p = AnimatorPlayer::new(std::env::temp_dir());
    p.insert_controller(CTRL, c);
    for (path, y) in [("p1.anim", 1.0), ("p5.anim", 5.0), ("p7.anim", 7.0)] {
        p.insert_clip(path, pose(y));
    }
    p.insert_clip("a0.anim", ramp("", 0.0, 0.0));
    p.insert_clip("a2.anim", ramp("", 2.0, 2.0));
    p.insert_clip("a4.anim", ramp("", 4.0, 4.0));
    p.insert_mask(
        "arm.mask",
        AvatarMask {
            transforms: ["", "Body", "Arm"]
                .iter()
                .map(|p| MaskEntry {
                    path: (*p).into(),
                    active: *p == "Arm",
                })
                .collect(),
        },
    );
    (scene, e, p)
}

fn y_of(s: &Scene, name: &str) -> f32 {
    s.transform(s.find_by_name(name).unwrap()).position.y
}

fn current(p: &mut AnimatorPlayer, s: &Scene, e: Entity) -> String {
    p.status(s, e).unwrap().current.unwrap().name
}

fn layer(name: &str, m: StateMachine) -> Layer {
    Layer {
        name: name.into(),
        machine: m,
        ..Default::default()
    }
}

// ---- layers and masks ------------------------------------------------------------------------

#[test]
fn override_layer_with_a_mask_replaces_only_masked_bones() {
    let c = AnimatorController {
        base: machine(vec![state("Idle", "p1.anim")], vec![]),
        layers: vec![Layer {
            mask: Some("arm.mask".into()),
            ..layer("Arms", machine(vec![state("Wave", "p5.anim")], vec![]))
        }],
        ..Default::default()
    };
    let (mut s, e, mut p) = setup(c);
    p.update(&mut s, 0.0);
    assert_eq!(y_of(&s, "Body"), 1.0, "masked out: base layer");
    assert_eq!(y_of(&s, "Arm"), 5.0, "masked in: override");
    p.set_layer_weight(&s, e, 1, 0.5);
    p.update(&mut s, 0.1);
    assert_eq!(y_of(&s, "Body"), 1.0);
    assert!((y_of(&s, "Arm") - 3.0).abs() < 1e-5, "half weight");
    p.set_layer_weight(&s, e, 1, 0.0);
    p.update(&mut s, 0.1);
    assert_eq!(y_of(&s, "Arm"), 1.0, "weight 0: the layer is off");
    // Masks only filter Transform curves, and an empty mask lets everything through.
    let m = AvatarMask::default();
    assert!(m.allows("Anything", Y));
    let arm = AvatarMask {
        transforms: vec![MaskEntry {
            path: "Arm".into(),
            active: false,
        }],
    };
    assert!(!arm.allows("Arm", Y) && !arm.allows("Leg", Y));
    assert!(arm.allows("Arm", Property::LightIntensity));
}

#[test]
fn additive_layer_adds_motion_relative_to_the_first_frame() {
    let c = AnimatorController {
        base: machine(vec![state("Idle", "p1.anim")], vec![]),
        layers: vec![Layer {
            blending: LayerBlending::Additive,
            ..layer("Breath", machine(vec![state("Bob", "bob.anim")], vec![]))
        }],
        ..Default::default()
    };
    let (mut s, e, mut p) = setup(c);
    // Arm y: 10 at the first frame, 14 at the end: +2 halfway.
    p.insert_clip("bob.anim", ramp("Arm", 10.0, 14.0));
    p.update(&mut s, 0.0);
    assert_eq!(y_of(&s, "Arm"), 1.0, "first frame adds nothing");
    p.update(&mut s, 0.5);
    assert!((y_of(&s, "Arm") - 3.0).abs() < 1e-4, "{}", y_of(&s, "Arm"));
    p.set_layer_weight(&s, e, 1, 0.5);
    p.update(&mut s, 0.0);
    assert!((y_of(&s, "Arm") - 2.0).abs() < 1e-4, "{}", y_of(&s, "Arm"));
    assert_eq!(y_of(&s, "Body"), 1.0, "Body is not in the additive clip");
}

#[test]
fn synced_layer_follows_the_source_states_with_its_own_motions() {
    let go = instant(Node::state("Idle"), Node::state("Run"), "Go");
    let c = AnimatorController {
        parameters: vec![param("Go", ParamKind::Bool)],
        base: machine(
            vec![state("Idle", "p1.anim"), state("Run", "p5.anim")],
            vec![go],
        ),
        layers: vec![Layer {
            sync_source: Some(0),
            sync_motions: [("Idle".to_owned(), "p7.anim".to_owned())].into(),
            ..layer("Hurt", StateMachine::default())
        }],
    };
    let (mut s, e, mut p) = setup(c);
    p.update(&mut s, 0.0);
    assert_eq!(y_of(&s, "Body"), 7.0, "synced Idle motion wins");
    let st = p.status(&s, e).unwrap();
    assert_eq!(st.layers[1].current.as_ref().unwrap().name, "Idle");
    assert_eq!(
        st.layers[1].current.as_ref().unwrap().full_path,
        "Hurt.Idle"
    );
    p.set_param(&s, e, "Go", animator::BOOL, 1.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Run");
    assert_eq!(y_of(&s, "Body"), 5.0, "no synced motion for Run");
}

#[test]
fn controller_sanitizes_layers_and_round_trips() {
    let mut c = AnimatorController {
        base: machine(vec![state("Idle", "p1.anim")], vec![]),
        layers: vec![
            layer("A", machine(vec![state("X", "p5.anim")], vec![])),
            Layer {
                sync_source: Some(2),
                ..layer("B", StateMachine::default())
            },
        ],
        ..Default::default()
    };
    let text = c.to_json();
    let back = AnimatorController::from_json(&text).unwrap();
    assert_eq!(
        back.layers[1].sync_source, None,
        "a layer can't sync to itself"
    );
    assert_eq!(back.layers[0], c.layers[0]);
    assert_eq!(c.add_layer("A"), 3);
    assert_eq!(c.layer_name(3), "A 1");
    c.layers[1].sync_source = Some(1);
    c.remove_layer(1);
    assert_eq!(c.layers[0].sync_source, None, "its source went away");
    assert_eq!(c.layer_index("A 1"), Some(2));
    assert_eq!(c.layer_name(0), BASE_LAYER);
    assert_eq!(c.motions(), ["p1.anim"], "layer A (with p5) was removed");
}

// ---- interruption ----------------------------------------------------------------------------

fn instant(from: Node, to: Node, on: &str) -> Transition {
    Transition {
        has_exit_time: false,
        duration: 0.0,
        conditions: vec![cond(on, ConditionMode::If)],
        ..Transition::new(from, to)
    }
}

fn fade(from: &str, to: &str, on: &str, seconds: f32) -> Transition {
    Transition {
        duration: seconds,
        ..instant(Node::state(from), Node::state(to), on)
    }
}

/// A (y 0), B (y 2), C (y 4) on the object itself; `transitions` link them.
fn abc(transitions: Vec<Transition>) -> AnimatorController {
    AnimatorController {
        parameters: vec![
            param("GoB", ParamKind::Bool),
            param("GoC", ParamKind::Bool),
            param("Any", ParamKind::Trigger),
        ],
        base: machine(
            vec![
                state("A", "a0.anim"),
                state("B", "a2.anim"),
                state("C", "a4.anim"),
            ],
            transitions,
        ),
        ..Default::default()
    }
}

/// Start A → B, half-way set GoC; returns the next state name after 0.1 s more.
fn interrupt_with(transitions: Vec<Transition>) -> (Option<String>, Scene, Entity, AnimatorPlayer) {
    let (mut s, e, mut p) = setup(abc(transitions));
    p.update(&mut s, 0.0);
    p.set_param(&s, e, "GoB", animator::BOOL, 1.0);
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.5);
    p.set_param(&s, e, "GoC", animator::BOOL, 1.0);
    p.update(&mut s, 0.0);
    let next = p.status(&s, e).unwrap().next.map(|n| n.0.name);
    (next, s, e, p)
}

fn with_source(mut t: Transition, src: InterruptionSource, ordered: bool) -> Transition {
    t.interruption_source = src;
    t.ordered_interruption = ordered;
    t
}

#[test]
fn interruption_source_none_lets_the_blend_finish() {
    let ab = fade("A", "B", "GoB", 1.0);
    let (next, mut s, e, mut p) = interrupt_with(vec![
        ab,
        fade("A", "C", "GoC", 0.5),
        fade("B", "C", "GoC", 0.5),
    ]);
    assert_eq!(next.as_deref(), Some("B"), "not interrupted");
    p.update(&mut s, 0.6);
    assert_eq!(
        p.status(&s, e).unwrap().next.unwrap().0.name,
        "C",
        "then B → C"
    );
}

#[test]
fn current_state_interruption_respects_ordering() {
    let ab = || fade("A", "B", "GoB", 1.0);
    let ac = || fade("A", "C", "GoC", 1.0);
    let cur = InterruptionSource::CurrentState;
    // A → C listed after A → B: ordered interruption forbids it, unordered allows it.
    let (next, ..) = interrupt_with(vec![with_source(ab(), cur, true), ac()]);
    assert_eq!(next.as_deref(), Some("B"));
    let (next, ..) = interrupt_with(vec![with_source(ab(), cur, false), ac()]);
    assert_eq!(next.as_deref(), Some("C"));
    // Listed before it (higher priority), ordered interruption allows it.
    let (next, ..) = interrupt_with(vec![ac(), with_source(ab(), cur, true)]);
    assert_eq!(next.as_deref(), Some("C"));
    // Next State transitions don't count for Current State.
    let (next, ..) = interrupt_with(vec![
        with_source(ab(), cur, true),
        fade("B", "C", "GoC", 1.0),
    ]);
    assert_eq!(next.as_deref(), Some("B"));
}

#[test]
fn next_state_interruption_and_the_frozen_pose() {
    let ab = with_source(
        fade("A", "B", "GoB", 1.0),
        InterruptionSource::NextState,
        true,
    );
    let (next, mut s, e, mut p) = interrupt_with(vec![
        ab,
        fade("A", "C", "GoC", 1.0),
        fade("B", "C", "GoC", 1.0),
    ]);
    assert_eq!(next.as_deref(), Some("C"), "B → C interrupts");
    assert_eq!(
        current(&mut p, &s, e),
        "B",
        "interrupted from the next state"
    );
    // Frozen at y = 1 (half A, half B), blended to C (4) over 1 s: 2.5 halfway.
    p.update(&mut s, 0.5);
    let y = s.transform(e).position.y;
    assert!((y - 2.5).abs() < 1e-3, "{y}");
    p.update(&mut s, 0.6);
    assert_eq!(current(&mut p, &s, e), "C");
    assert_eq!(s.transform(e).position.y, 4.0);
}

#[test]
fn current_then_next_checks_both_groups_in_order() {
    let both = InterruptionSource::NextStateThenCurrentState;
    let ab = with_source(fade("A", "B", "GoB", 1.0), both, false);
    let mut ba = fade("B", "A", "GoC", 1.0);
    ba.conditions = vec![cond("GoC", ConditionMode::If)];
    // Next state's B → A comes before current state's A → C.
    let (next, ..) = interrupt_with(vec![ab, fade("A", "C", "GoC", 1.0), ba]);
    assert_eq!(next.as_deref(), Some("A"));
    assert_eq!(
        InterruptionSource::CurrentStateThenNextState.order(),
        &[false, true]
    );
}

#[test]
fn any_state_interrupts_any_blend_but_not_its_own_destination() {
    let mut any = instant(Node::AnyState, Node::state("C"), "Any");
    any.duration = 0.2;
    let (mut s, e, mut p2) = setup(abc(vec![fade("A", "B", "GoB", 1.0), any]));
    p2.update(&mut s, 0.0);
    p2.set_param(&s, e, "GoB", animator::BOOL, 1.0);
    p2.update(&mut s, 0.0);
    p2.update(&mut s, 0.3);
    p2.set_param(&s, e, "Any", animator::TRIGGER, 1.0);
    p2.update(&mut s, 0.0);
    assert_eq!(p2.status(&s, e).unwrap().next.unwrap().0.name, "C");
    assert_eq!(p2.param(&s, e, "Any", animator::TRIGGER), 0.0, "consumed");
    p2.update(&mut s, 0.3);
    assert_eq!(current(&mut p2, &s, e), "C");
    // Firing again while in C: Any State doesn't restart it (Can Transition To Self is off).
    p2.set_param(&s, e, "Any", animator::TRIGGER, 1.0);
    p2.update(&mut s, 0.1);
    assert!(p2.status(&s, e).unwrap().next.is_none());
}

#[test]
fn muted_and_solo_transitions() {
    let mut m = abc(vec![
        instant(Node::state("A"), Node::state("B"), "GoB"),
        instant(Node::state("A"), Node::state("C"), "GoB"),
    ]);
    m.base.transitions[0].mute = true;
    let (mut s, e, mut p) = setup(m.clone());
    p.set_param(&s, e, "GoB", animator::BOOL, 1.0);
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "C", "muted A → B is skipped");
    m.base.transitions[0].mute = false;
    m.base.transitions[1].solo = true;
    let (mut s, e, mut p) = setup(m);
    p.set_param(&s, e, "GoB", animator::BOOL, 1.0);
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "C", "only solo transitions count");
}

// ---- sub-state machines -----------------------------------------------------------------------

/// Idle and Rest at the top; Combat holds Attack (default) and Block.
fn combat() -> AnimatorController {
    let mut base = machine(
        vec![
            state("Idle", "a0.anim"),
            state("Rest", "a0.anim"),
            State {
                machine: "Combat".into(),
                ..state("Attack", "a2.anim")
            },
            State {
                machine: "Combat".into(),
                ..state("Block", "a4.anim")
            },
        ],
        vec![
            instant(Node::state("Idle"), Node::Machine("Combat".into()), "Fight"),
            instant(Node::state("Attack"), Node::state("Block"), "Guard"),
            Transition {
                conditions: vec![cond("Guard", ConditionMode::IfNot)],
                ..instant(Node::state("Block"), Node::Exit, "Guard")
            },
            // Leaving Combat: rest when tired, otherwise the top Entry (Idle).
            Transition {
                conditions: vec![cond("Tired", ConditionMode::If)],
                ..Transition::new(Node::Machine("Combat".into()), Node::state("Rest"))
            },
            // Entering Combat while guarding goes straight to Block.
            Transition {
                conditions: vec![cond("Guard", ConditionMode::If)],
                machine: "Combat".into(),
                ..Transition::new(Node::Entry, Node::state("Block"))
            },
        ],
    );
    base.sub_machines.push(SubMachine {
        path: "Combat".into(),
        default_state: Some("Attack".into()),
        ..Default::default()
    });
    AnimatorController {
        parameters: vec![
            param("Fight", ParamKind::Trigger),
            param("Guard", ParamKind::Bool),
            param("Tired", ParamKind::Bool),
        ],
        base,
        ..Default::default()
    }
}

#[test]
fn sub_state_machines_enter_through_entry_and_leave_through_exit() {
    let (mut s, e, mut p) = setup(combat());
    p.update(&mut s, 0.0);
    p.set_param(&s, e, "Fight", animator::TRIGGER, 1.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Attack", "Combat's default state");
    let info = p.status(&s, e).unwrap().current.unwrap();
    assert_eq!(info.full_path, "Base Layer.Combat.Attack");
    p.set_param(&s, e, "Guard", animator::BOOL, 1.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Block");
    // Exit with nothing from Combat holding: the top Entry → Idle.
    p.set_param(&s, e, "Guard", animator::BOOL, 0.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Idle");
    // Entering again while guarding: Combat's Entry transition picks Block.
    p.set_param(&s, e, "Guard", animator::BOOL, 1.0);
    p.set_param(&s, e, "Fight", animator::TRIGGER, 1.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Block");
    // Leaving when tired: the transition from the Combat node goes to Rest.
    p.set_param(&s, e, "Tired", animator::BOOL, 1.0);
    p.set_param(&s, e, "Guard", animator::BOOL, 0.0);
    p.update(&mut s, 0.1);
    assert_eq!(current(&mut p, &s, e), "Rest");
    // Play accepts full paths.
    assert!(p.play(&s, e, "Base Layer.Combat.Attack", -1, None));
    p.update(&mut s, 0.0);
    assert_eq!(current(&mut p, &s, e), "Attack");
}

#[test]
fn sub_state_machine_editing_keeps_references() {
    let mut c = combat();
    let m = &mut c.base;
    let inner = m.add_sub_machine("Melee", "Combat", [0.0, 0.0]);
    assert_eq!(inner, "Combat/Melee");
    let slash = m.add_state_in("Slash", &inner, [0.0, 0.0]);
    assert_eq!(
        m.sub(&inner).unwrap().default_state.as_deref(),
        Some("Slash")
    );
    assert_eq!(
        m.rename_sub_machine("Combat", "Fight").as_deref(),
        Some("Fight")
    );
    assert_eq!(m.state(&slash).unwrap().machine, "Fight/Melee");
    assert_eq!(m.state("Attack").unwrap().machine, "Fight");
    assert!(
        m.transitions
            .iter()
            .any(|t| t.to == Node::Machine("Fight".into()))
    );
    assert!(m.transitions.iter().any(|t| t.machine == "Fight"));
    assert_eq!(m.children_of("Fight").count(), 1);
    assert_eq!(machine_parent("Fight/Melee"), "Fight");
    assert!(machine_within("Fight/Melee", "Fight") && !machine_within("Fighter", "Fight"));
    m.move_state("Slash", "");
    assert_eq!(m.sub("Fight/Melee").unwrap().default_state, None);
    m.remove_sub_machine("Fight");
    assert!(m.state("Attack").is_none() && m.state("Slash").is_some());
    assert!(m.sub_machines.is_empty());
    assert!(
        m.transitions
            .iter()
            .all(|t| !matches!(t.to, Node::Machine(_)))
    );
    // A state in a machine that doesn't exist moves to the top on load.
    let text = r#"{"states":[{"name":"A","machine":"Gone"}]}"#;
    let c = AnimatorController::from_json(text).unwrap();
    assert_eq!(c.base.states[0].machine, "");
}

// ---- root motion ----------------------------------------------------------------------------

/// 2 m forwards (z) per second, linear; `turn` degrees of yaw over the clip.
fn walk(turn: f32, root: &str) -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        ..Default::default()
    };
    c.set_key(root, Property::Position(2), 0.0, 0.0);
    c.set_key(root, Property::Position(2), 1.0, 2.0);
    c.set_key(root, Property::Rotation(1), 0.0, 0.0);
    c.set_key(root, Property::Rotation(1), 1.0, turn);
    c.set_key(root, Y, 0.0, 1.0);
    c.set_key(root, Y, 0.5, 1.2);
    c.set_key(root, Y, 1.0, 1.0);
    for curve in &mut c.curves {
        for i in 0..curve.keys.len() {
            curve.keys.set_tangent(i, super::TangentMode::Linear);
        }
    }
    c
}

#[test]
fn clip_deltas_accumulate_across_loops() {
    let c = walk(0.0, "");
    let d = |a, b, looping| clip_delta(&c, "", a, b, looping);
    assert!((d(0.2, 0.7, true).position.z - 1.0).abs() < 1e-4);
    assert!(
        (d(0.9, 1.1, true).position.z - 0.4).abs() < 1e-4,
        "across the wrap"
    );
    assert!(
        (d(0.5, 2.5, true).position.z - 4.0).abs() < 1e-4,
        "two loops"
    );
    assert!(
        (d(0.9, 1.5, false).position.z - 0.2).abs() < 1e-4,
        "held at the end"
    );
    assert!(
        (d(0.6, 0.4, true).position.z + 0.4).abs() < 1e-4,
        "backwards"
    );
    // Height belongs to the object's own root motion.
    assert!((d(0.0, 0.5, true).position.y - 0.2).abs() < 1e-4);
    // Turning clips: each part is measured from where the previous one ends.
    let a = Delta {
        position: Vec3::Z,
        yaw: 90.0,
    };
    let both = a.then(Delta {
        position: Vec3::Z,
        yaw: 0.0,
    });
    assert!((both.position - Vec3::new(1.0, 0.0, 1.0)).length() < 1e-5);
    let back = a.then(a.inverse());
    assert!(back.position.length() < 1e-5 && back.yaw.abs() < 1e-5);
    let turn = clip_delta(&walk(90.0, ""), "", 0.0, 1.0, true);
    assert!((turn.yaw - 90.0).abs() < 1e-3);
    assert!(is_root_key("", "", Y) && !is_root_key("Hips", "Hips", Y));
    assert!(is_root_key("Hips", "Hips", Property::Position(0)));
    assert!(!is_root_key("Hips", "Hips/Spine", Property::Position(0)));
}

fn root_motion_setup(bone: &str) -> (Scene, Entity, AnimatorPlayer) {
    let c = AnimatorController {
        base: machine(vec![state("Walk", "walk.anim")], vec![]),
        ..Default::default()
    };
    let (mut s, e, mut p) = setup(c);
    if !bone.is_empty() {
        s.spawn(bone, Some(e));
    }
    p.insert_clip("walk.anim", walk(0.0, bone));
    {
        let mut a = s.world.get::<&mut Animator>(e).unwrap();
        a.apply_root_motion = true;
        a.root_motion_bone = bone.into();
    }
    s.set_transform(
        e,
        Transform::from_position(Vec3::new(5.0, 0.0, 0.0)).with_euler(Vec3::new(0.0, 90.0, 0.0)),
    );
    (s, e, p)
}

/// Apply the player's moves the way the scripting host does without a Character Controller.
fn apply_moves(s: &mut Scene, p: &mut AnimatorPlayer) {
    for m in p.take_moves() {
        let pos = s.world_position(m.entity);
        s.set_world_position(m.entity, pos + m.delta_position);
        let r = s.world_rotation(m.entity);
        s.set_world_rotation(m.entity, r * m.delta_rotation);
    }
}

#[test]
fn root_motion_moves_the_object_by_the_accumulated_delta() {
    let (mut s, e, mut p) = root_motion_setup("");
    p.update(&mut s, 0.0);
    apply_moves(&mut s, &mut p);
    for _ in 0..15 {
        p.update(&mut s, 0.1);
        apply_moves(&mut s, &mut p);
    }
    // 1.5 s at 2 m/s along the facing (+x), plus the height bob of the half loop; the object's
    // own curves are not written.
    let pos = s.transform(e).position;
    assert!((pos - Vec3::new(8.0, 0.2, 0.0)).length() < 1e-3, "{pos}");
    let (d, _) = p.root_delta(e);
    assert!(
        (d - Vec3::new(0.2, 0.04, 0.0)).length() < 1e-4,
        "last frame: {d}"
    );
}

#[test]
fn bone_root_motion_pins_the_bone_and_moves_the_object() {
    let (mut s, e, mut p) = root_motion_setup("Hips");
    let hips = s.find_by_name("Hips").unwrap();
    p.update(&mut s, 0.0);
    apply_moves(&mut s, &mut p);
    for _ in 0..5 {
        p.update(&mut s, 0.1);
        apply_moves(&mut s, &mut p);
    }
    let pos = s.transform(e).position;
    assert!((pos - Vec3::new(6.0, 0.0, 0.0)).length() < 1e-3, "{pos}");
    let h = s.transform(hips).position;
    assert!(h.z.abs() < 1e-5, "travel stays on the object: {h}");
    assert!((h.y - 1.2).abs() < 1e-4, "the bone keeps its bob: {h}");
}

#[test]
fn root_motion_off_lets_curves_set_the_position() {
    let (mut s, e, mut p) = root_motion_setup("");
    s.world.get::<&mut Animator>(e).unwrap().apply_root_motion = false;
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.25);
    assert!(p.take_moves().is_empty());
    let pos = s.transform(e).position;
    assert!((pos - Vec3::new(5.0, 1.1, 0.5)).length() < 1e-4, "{pos}");
}

#[test]
fn match_target_reaches_the_target_at_the_end_time() {
    let (mut s, e, mut p) = root_motion_setup("");
    s.world.get::<&mut Animator>(e).unwrap().apply_root_motion = false;
    p.insert_clip("walk.anim", ramp("Arm", 0.0, 0.0));
    p.update(&mut s, 0.0);
    let target = Vec3::new(5.0, 2.0, 3.0);
    let q = glam::Quat::IDENTITY;
    assert!(p.match_target(&s, e, target, q, Vec3::new(0.0, 1.0, 1.0), 0.0, 0.2, 0.6));
    for _ in 0..3 {
        p.update(&mut s, 0.1);
        p.apply_match_targets(&mut s);
    }
    assert!(p.is_matching_target(e));
    let mid = s.transform(e).position;
    assert!(mid.y > 0.0 && mid.y < 2.0, "{mid}");
    for _ in 0..4 {
        p.update(&mut s, 0.1);
        p.apply_match_targets(&mut s);
    }
    assert!(!p.is_matching_target(e));
    let pos = s.transform(e).position;
    assert!((pos - target).length() < 1e-3, "{pos}");
}
