//! The Animator IK pass per layer: `OnAnimatorIK(i)` once per IK Pass layer, bottom to top,
//! each solving its goals on the pose blended up to that layer, by that layer's weight and
//! Avatar Mask; the layers above blend over the IK'd pose.

use glam::Vec3;
use hecs::Entity;

use super::animator::AnimatorPlayer;
use super::clip::{AnimationClip, Property};
use super::controller::*;
use super::mask::{AvatarMask, MaskEntry};
use crate::rigging::animator_ik::{self as ik, IkGoal, IkGoals};
use crate::scene::{Animator, Scene, Transform};

const CTRL: &str = "Assets/IK.controller";
const ARM: &str = "Hips/Spine/Chest/LeftArm";
/// The left hand as animated with no clips (arm along -x).
const ANIMATED: Vec3 = Vec3::new(-0.8, 1.55, 0.0);
const TARGET: Vec3 = Vec3::new(-0.4, 1.2, 0.3);
const OTHER: Vec3 = Vec3::new(-0.5, 1.8, -0.3);

fn add(scene: &mut Scene, parent: Entity, name: &str, pos: Vec3) -> Entity {
    let e = scene.spawn(name, Some(parent));
    scene.set_transform(e, Transform::from_position(pos));
    e
}

/// "Hero" over a small humanoid rig found by bone names (the same as the solver tests).
fn rig(scene: &mut Scene) -> Entity {
    let root = scene.spawn("Hero", None);
    let hips = add(scene, root, "Hips", Vec3::new(0.0, 1.0, 0.0));
    let spine = add(scene, hips, "Spine", Vec3::new(0.0, 0.2, 0.0));
    let chest = add(scene, spine, "Chest", Vec3::new(0.0, 0.2, 0.0));
    let neck = add(scene, chest, "Neck", Vec3::new(0.0, 0.2, 0.0));
    add(scene, neck, "Head", Vec3::new(0.0, 0.1, 0.0));
    for (side, x) in [("Left", -1.0f32), ("Right", 1.0)] {
        let arm = add(
            scene,
            chest,
            &format!("{side}Arm"),
            Vec3::new(0.2 * x, 0.15, 0.0),
        );
        let fore = add(
            scene,
            arm,
            &format!("{side}ForeArm"),
            Vec3::new(0.3 * x, 0.0, -0.02),
        );
        add(
            scene,
            fore,
            &format!("{side}Hand"),
            Vec3::new(0.3 * x, 0.0, 0.02),
        );
        let up = add(
            scene,
            hips,
            &format!("{side}UpLeg"),
            Vec3::new(0.1 * x, 0.0, 0.0),
        );
        let leg = add(
            scene,
            up,
            &format!("{side}Leg"),
            Vec3::new(0.0, -0.45, 0.02),
        );
        add(
            scene,
            leg,
            &format!("{side}Foot"),
            Vec3::new(0.0, -0.45, -0.02),
        );
    }
    root
}

fn machine(motion: Option<&str>) -> StateMachine {
    StateMachine {
        states: vec![State {
            name: "S".into(),
            motion: motion.map(Into::into),
            ..Default::default()
        }],
        default_state: Some("S".into()),
        ..Default::default()
    }
}

fn layer(name: &str, motion: Option<&str>, ik_pass: bool) -> Layer {
    Layer {
        name: name.into(),
        ik_pass,
        machine: machine(motion),
        ..Default::default()
    }
}

/// A clip holding the left upper arm at `z` degrees about z (90 = hanging down), the forearm
/// and hand straight (the whole arm is animated, so it replaces IK from the layers below).
fn arm_clip(z: f32) -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        ..Default::default()
    };
    let fore = format!("{ARM}/LeftForeArm");
    let hand = format!("{fore}/LeftHand");
    for (path, z) in [(ARM.to_string(), z), (fore, 0.0), (hand, 0.0)] {
        for (i, v) in [(0, 0.0), (1, 0.0), (2, z)] {
            c.set_key(&path, Property::Rotation(i), 0.0, v);
            c.set_key(&path, Property::Rotation(i), 1.0, v);
        }
    }
    c
}

/// A mask that lets the right arm through but not the left one.
fn right_arm_mask() -> AvatarMask {
    let chest = "Hips/Spine/Chest";
    let mut transforms = Vec::new();
    for side in ["Left", "Right"] {
        let arm = format!("{chest}/{side}Arm");
        for path in [
            arm.clone(),
            format!("{arm}/{side}ForeArm"),
            format!("{arm}/{side}ForeArm/{side}Hand"),
        ] {
            transforms.push(MaskEntry {
                path,
                active: side == "Right",
            });
        }
    }
    AvatarMask { transforms }
}

fn setup(c: AnimatorController) -> (Scene, Entity, AnimatorPlayer) {
    let mut scene = Scene::new("t");
    let e = rig(&mut scene);
    scene.add(
        e,
        Animator {
            controller: Some(CTRL.into()),
            ..Default::default()
        },
    );
    let mut p = AnimatorPlayer::new(std::env::temp_dir());
    p.insert_controller(CTRL, c);
    p.insert_clip("down.anim", arm_clip(90.0));
    p.insert_clip("half.anim", arm_clip(40.0));
    p.insert_mask("right.mask", right_arm_mask());
    (scene, e, p)
}

/// One frame of the Animator and its IK pass, as the scripting host runs it: `f(layer, goals,
/// left hand position at the call)` plays `OnAnimatorIK(layer)`. Returns the layers called.
fn frame(
    p: &mut AnimatorPlayer,
    scene: &mut Scene,
    mut f: impl FnMut(usize, &mut IkGoals, Vec3),
) -> Vec<usize> {
    p.update(scene, 0.1);
    let mut called = Vec::new();
    for (e, layers) in p.ik_passes(scene) {
        p.ik_begin(scene, e);
        for l in layers {
            p.ik_layer_begin(scene, e, l);
            let avatar = p.ik_avatar(scene, e);
            let hand = ik::effector_position(scene, e, &avatar, IkGoal::LeftHand);
            f(l, p.ik.goals_mut(e), hand);
            p.ik_layer_solve(scene, e, l);
            called.push(l);
        }
        p.ik_end(scene, e);
    }
    called
}

fn reach(g: &mut IkGoals, goal: IkGoal, at: Vec3) {
    let goal = g.goal_mut(goal);
    goal.position = at;
    goal.position_weight = 1.0;
}

fn wp(s: &Scene, name: &str) -> Vec3 {
    s.world_position(s.find_by_name(name).unwrap())
}

fn two_ik_layers(arms: Layer) -> AnimatorController {
    AnimatorController {
        base: machine(None),
        base_ik_pass: true,
        layers: vec![arms],
        ..Default::default()
    }
}

#[test]
fn each_ik_layer_is_called_once_in_order_with_goals_cleared_between() {
    let (mut s, _, mut p) = setup(two_ik_layers(layer("Arms", None, true)));
    let mut seen = Vec::new();
    let called = frame(&mut p, &mut s, |l, g, _| {
        seen.push((l, g.goals[IkGoal::LeftHand as usize].position_weight));
        reach(g, IkGoal::LeftHand, TARGET);
    });
    assert_eq!(called, [0, 1]);
    // Layer 1 starts with no goals although layer 0 set one.
    assert_eq!(seen, [(0, 0.0), (1, 0.0)]);
    assert!(wp(&s, "LeftHand").distance(TARGET) < 1e-3);
}

#[test]
fn a_higher_ik_layer_overrides_a_lower_one_on_the_same_goal() {
    let (mut s, _, mut p) = setup(two_ik_layers(layer("Arms", None, true)));
    frame(&mut p, &mut s, |l, g, _| {
        reach(g, IkGoal::LeftHand, if l == 0 { OTHER } else { TARGET });
    });
    let hand = wp(&s, "LeftHand");
    assert!(hand.distance(TARGET) < 1e-3, "hand at {hand}");
    // A goal only the lower layer sets stays solved (the upper one animates nothing).
    frame(&mut p, &mut s, |l, g, _| {
        if l == 0 {
            reach(g, IkGoal::LeftHand, OTHER);
        }
    });
    let hand = wp(&s, "LeftHand");
    assert!(hand.distance(OTHER) < 1e-3, "hand at {hand}");
}

#[test]
fn an_override_layer_above_replaces_the_ik_of_the_layers_below() {
    // Only the base layer has IK Pass; "Arms" (weight 1) animates the left arm down.
    let (mut s, _, mut p) = setup(two_ik_layers(layer("Arms", Some("down.anim"), false)));
    let mut at_call = Vec::new();
    let called = frame(&mut p, &mut s, |_, g, hand| {
        at_call.push(hand);
        reach(g, IkGoal::LeftHand, TARGET);
    });
    assert_eq!(called, [0]);
    // The base layer's call sees the base pose, not the Arms layer's.
    assert!(at_call[0].distance(ANIMATED) < 1e-4, "{at_call:?}");
    let down = Vec3::new(-0.2, 0.95, 0.0);
    let hand = wp(&s, "LeftHand");
    assert!(hand.distance(down) < 1e-3, "hand at {hand}");
}

#[test]
fn get_ik_position_reports_the_pose_up_to_the_layer() {
    let (mut s, _, mut p) = setup(two_ik_layers(layer("Arms", Some("down.anim"), true)));
    let mut at_call = Vec::new();
    frame(&mut p, &mut s, |l, g, hand| {
        at_call.push((l, hand));
        if l == 1 {
            reach(g, IkGoal::LeftHand, TARGET);
        }
    });
    assert!(at_call[0].1.distance(ANIMATED) < 1e-4, "{at_call:?}");
    assert!(
        at_call[1].1.distance(Vec3::new(-0.2, 0.95, 0.0)) < 1e-3,
        "{at_call:?}"
    );
    assert!(wp(&s, "LeftHand").distance(TARGET) < 1e-3);
}

#[test]
fn a_layer_mask_keeps_that_layers_goals_off_masked_bones() {
    let mut arms = layer("Arms", None, true);
    arms.mask = Some("right.mask".into());
    let (mut s, _, mut p) = setup(two_ik_layers(arms));
    let right_target = Vec3::new(0.4, 1.2, 0.3);
    frame(&mut p, &mut s, |l, g, _| {
        if l == 1 {
            reach(g, IkGoal::LeftHand, TARGET);
            reach(g, IkGoal::RightHand, right_target);
        }
    });
    let left = wp(&s, "LeftHand");
    assert!(left.distance(ANIMATED) < 1e-4, "masked arm moved to {left}");
    assert!(wp(&s, "RightHand").distance(right_target) < 1e-3);
    // The same goal from the unmasked base layer still moves the left arm.
    frame(&mut p, &mut s, |l, g, _| {
        if l == 0 {
            reach(g, IkGoal::LeftHand, TARGET);
        }
    });
    assert!(wp(&s, "LeftHand").distance(TARGET) < 1e-3);
}

#[test]
fn layer_weight_scales_that_layers_goals() {
    let mut arms = layer("Arms", None, true);
    arms.weight = 0.5;
    let (mut s, _, mut p) = setup(two_ik_layers(arms));
    frame(&mut p, &mut s, |l, g, _| {
        if l == 1 {
            reach(g, IkGoal::LeftHand, TARGET);
        }
    });
    let half = ANIMATED.lerp(TARGET, 0.5);
    let hand = wp(&s, "LeftHand");
    assert!(hand.distance(half) < 1e-3, "hand at {hand}, want {half}");
    // Weight 0: called, but no effect.
    let e = s.find_by_name("Hero").unwrap();
    p.set_layer_weight(&s, e, 1, 0.0);
    let called = frame(&mut p, &mut s, |l, g, _| {
        if l == 1 {
            reach(g, IkGoal::LeftHand, TARGET);
        }
    });
    assert_eq!(called, [0, 1]);
    assert!(wp(&s, "LeftHand").distance(ANIMATED) < 1e-4);
}

#[test]
fn no_goals_leave_the_blended_pose_as_the_animator_wrote_it() {
    // A half-weight override and an additive layer over the base: the IK pass blends them
    // again; without goals the result must match the Animator's own.
    let mut half = layer("Half", Some("half.anim"), true);
    half.weight = 0.5;
    let mut add = layer("Add", Some("down.anim"), true);
    add.blending = LayerBlending::Additive;
    add.weight = 0.25;
    let c = AnimatorController {
        base: machine(Some("down.anim")),
        base_ik_pass: true,
        layers: vec![half, add],
        ..Default::default()
    };
    let (mut s, e, mut p) = setup(c);
    let names = ["LeftArm", "LeftForeArm", "LeftHand", "RightHand", "Head"];
    for _ in 0..3 {
        p.update(&mut s, 0.1);
        let want: Vec<Vec3> = names.iter().map(|n| wp(&s, n)).collect();
        p.ik_begin(&mut s, e);
        for l in 0..3 {
            p.ik_layer_begin(&mut s, e, l);
            p.ik_layer_solve(&mut s, e, l);
        }
        p.ik_end(&mut s, e);
        for (n, w) in names.iter().zip(want) {
            assert!(wp(&s, n).distance(w) < 1e-4, "{n} at {} not {w}", wp(&s, n));
        }
    }
}

#[test]
fn controllers_without_ik_pass_run_no_ik() {
    let c = AnimatorController {
        base: machine(Some("down.anim")),
        layers: vec![layer("Arms", None, false)],
        ..Default::default()
    };
    let (mut s, _, mut p) = setup(c);
    let called = frame(&mut p, &mut s, |_, _, _| {});
    assert!(called.is_empty());
}
