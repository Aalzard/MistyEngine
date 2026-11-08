//! Humanoid avatars: mapping two differently named and posed rigs, converting a clip to a
//! humanoid clip and retargeting it onto the other rig.

use glam::{Quat, Vec3};
use hecs::Entity;

use super::animator::AnimatorPlayer;
use super::binding;
use super::clip::{AnimationClip, Property};
use super::controller::{AnimatorController, State, StateMachine};
use super::humanoid::*;
use crate::scene::{Animator, Scene, Transform};

fn add(scene: &mut Scene, parent: Entity, name: &str, pos: Vec3, rot_z: f32) -> Entity {
    let e = scene.spawn(name, Some(parent));
    scene.set_transform(
        e,
        Transform::from_position(pos).with_euler(Vec3::new(0.0, 0.0, rot_z)),
    );
    e
}

/// Rig A: "Hips/Spine/Head", "LeftArm/LeftForeArm/LeftHand"…, arms along ±x, hips 1 m up.
fn rig_a(scene: &mut Scene) -> Entity {
    let root = scene.spawn("Hero", None);
    let hips = add(scene, root, "Hips", Vec3::new(0.0, 1.0, 0.0), 0.0);
    let spine = add(scene, hips, "Spine", Vec3::new(0.0, 0.2, 0.0), 0.0);
    add(scene, spine, "Head", Vec3::new(0.0, 0.4, 0.0), 0.0);
    for (side, x) in [("Left", -1.0f32), ("Right", 1.0)] {
        let arm = add(
            scene,
            spine,
            &format!("{side}Arm"),
            Vec3::new(0.2 * x, 0.3, 0.0),
            0.0,
        );
        let fore = add(
            scene,
            arm,
            &format!("{side}ForeArm"),
            Vec3::new(0.3 * x, 0.0, 0.0),
            0.0,
        );
        add(
            scene,
            fore,
            &format!("{side}Hand"),
            Vec3::new(0.25 * x, 0.0, 0.0),
            0.0,
        );
        let up = add(
            scene,
            hips,
            &format!("{side}UpLeg"),
            Vec3::new(0.1 * x, 0.0, 0.0),
            0.0,
        );
        let leg = add(
            scene,
            up,
            &format!("{side}Leg"),
            Vec3::new(0.0, -0.45, 0.0),
            0.0,
        );
        add(
            scene,
            leg,
            &format!("{side}Foot"),
            Vec3::new(0.0, -0.45, 0.0),
            0.0,
        );
    }
    root
}

/// Rig B: other names ("pelvis", "upperarm_l"…), half the size, and arm bones whose local axes
/// are turned 90° (their children sit along local +y).
fn rig_b(scene: &mut Scene) -> Entity {
    let root = scene.spawn("Robot", None);
    let pelvis = add(scene, root, "pelvis", Vec3::new(3.0, 0.5, 0.0), 0.0);
    let spine = add(scene, pelvis, "spine_01", Vec3::new(0.0, 0.1, 0.0), 0.0);
    let neck = add(scene, spine, "neck_01", Vec3::new(0.0, 0.2, 0.0), 0.0);
    add(scene, neck, "head", Vec3::new(0.0, 0.05, 0.0), 0.0);
    for (side, x) in [("l", -1.0f32), ("r", 1.0)] {
        let arm = add(
            scene,
            spine,
            &format!("upperarm_{side}"),
            Vec3::new(0.1 * x, 0.15, 0.0),
            -90.0 * x,
        );
        let fore = add(
            scene,
            arm,
            &format!("lowerarm_{side}"),
            Vec3::new(0.0, 0.15, 0.0),
            0.0,
        );
        add(
            scene,
            fore,
            &format!("hand_{side}"),
            Vec3::new(0.0, 0.12, 0.0),
            0.0,
        );
        let up = add(
            scene,
            pelvis,
            &format!("thigh_{side}"),
            Vec3::new(0.05 * x, 0.0, 0.0),
            0.0,
        );
        let leg = add(
            scene,
            up,
            &format!("calf_{side}"),
            Vec3::new(0.0, -0.22, 0.0),
            0.0,
        );
        add(
            scene,
            leg,
            &format!("foot_{side}"),
            Vec3::new(0.0, -0.22, 0.0),
            0.0,
        );
    }
    root
}

#[test]
fn auto_map_finds_human_bones_by_name() {
    let mut s = Scene::new("t");
    let ra = rig_a(&mut s);
    let rb = rig_b(&mut s);
    let a = Avatar::auto_map(&s, ra);
    let b = Avatar::auto_map(&s, rb);
    assert!(a.missing().is_empty(), "{:?}", a.missing());
    assert!(b.missing().is_empty(), "{:?}", b.missing());
    assert_eq!(
        a.bone(HumanBone::LeftUpperArm).unwrap().path,
        "Hips/Spine/LeftArm"
    );
    assert_eq!(
        a.bone(HumanBone::RightLowerLeg).unwrap().path,
        "Hips/RightUpLeg/RightLeg"
    );
    assert_eq!(b.bone(HumanBone::Hips).unwrap().path, "pelvis");
    assert_eq!(
        b.bone(HumanBone::Neck).unwrap().path,
        "pelvis/spine_01/neck_01"
    );
    assert_eq!(
        b.bone(HumanBone::LeftHand).unwrap().path,
        "pelvis/spine_01/upperarm_l/lowerarm_l/hand_l"
    );
    assert!(
        a.bone(HumanBone::Neck).is_none(),
        "optional bones may be missing"
    );
    assert!((a.hips_height - 1.0).abs() < 1e-5 && (b.hips_height - 0.5).abs() < 1e-5);
    let back = Avatar::from_json(&b.to_json()).unwrap();
    assert_eq!(back, b);
    assert!(is_avatar("Assets/Robot.avatar"));
}

/// Rig A's clip: the left upper arm swings down 90° (about z) and the hips rise 0.2 m.
fn swing() -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        looping: false,
        ..Default::default()
    };
    c.set_key("Hips/Spine/LeftArm", Property::Rotation(2), 0.0, 0.0);
    c.set_key("Hips/Spine/LeftArm", Property::Rotation(2), 1.0, 90.0);
    c.set_key("Hips", Property::Position(1), 0.0, 1.0);
    c.set_key("Hips", Property::Position(1), 1.0, 1.2);
    c.set_key("", Property::Position(2), 0.0, 0.0);
    c.set_key("", Property::Position(2), 1.0, 2.0);
    for curve in &mut c.curves {
        for i in 0..2 {
            curve.keys.set_tangent(i, super::TangentMode::Linear);
        }
    }
    c
}

/// World direction from an upper arm to its lower arm.
fn arm_dir(s: &Scene, upper: &str, lower: &str) -> Vec3 {
    let u = s.world_position(s.find_by_name(upper).unwrap());
    let l = s.world_position(s.find_by_name(lower).unwrap());
    (l - u).normalize()
}

#[test]
fn humanoid_clip_retargets_onto_another_rig() {
    let mut s = Scene::new("t");
    let ra = rig_a(&mut s);
    let rb = rig_b(&mut s);
    let (a, b) = (Avatar::auto_map(&s, ra), Avatar::auto_map(&s, rb));
    // At rest both left arms point along -x.
    let rest_a = arm_dir(&s, "LeftArm", "LeftForeArm");
    let rest_b = arm_dir(&s, "upperarm_l", "lowerarm_l");
    assert!((rest_a - rest_b).length() < 1e-4, "{rest_a} {rest_b}");

    let human = humanoid_clip(&swing(), &a, 30.0);
    assert!(is_humanoid(&human) && !is_humanoid(&swing()));
    assert!(
        human.curve("", Property::Position(2)).is_some(),
        "root motion kept"
    );
    assert!(
        human
            .curve("Hips/Spine/LeftArm", Property::Rotation(2))
            .is_none()
    );
    assert!(
        human
            .curve("@LeftUpperArm", Property::HumanRotation(3))
            .is_some()
    );

    // Rig A plays its own clip half way.
    binding::sample(&mut s, ra, &swing(), 0.5);
    let want = arm_dir(&s, "LeftArm", "LeftForeArm");
    // Rig B plays the humanoid clip through its avatar.
    s.add(
        rb,
        Animator {
            controller: Some("h.controller".into()),
            avatar: Some("b.avatar".into()),
            ..Default::default()
        },
    );
    let mut p = AnimatorPlayer::new(std::env::temp_dir());
    p.insert_avatar("b.avatar", b);
    p.insert_clip("swing.anim", human);
    p.insert_controller(
        "h.controller",
        AnimatorController {
            base: StateMachine {
                states: vec![State {
                    name: "Swing".into(),
                    motion: Some("swing.anim".into()),
                    looping: false,
                    ..Default::default()
                }],
                default_state: Some("Swing".into()),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    p.update(&mut s, 0.0);
    p.update(&mut s, 0.5);
    let got = arm_dir(&s, "upperarm_l", "lowerarm_l");
    assert!((got - want).length() < 0.02, "{got} vs {want}");
    assert!((want - rest_a).length() > 0.5, "the arm did move: {want}");
    // Hips: +0.1 of A's 1 m height, so +0.05 on B's 0.5 m hips.
    let pelvis = s.transform(s.find_by_name("pelvis").unwrap()).position;
    assert!(
        (pelvis - Vec3::new(3.0, 0.55, 0.0)).length() < 1e-3,
        "{pelvis}"
    );
    // The right arm (not in the clip) keeps its rest rotation.
    let r = s.transform(s.find_by_name("upperarm_r").unwrap()).rotation;
    assert!(r.angle_between(Quat::from_rotation_z((-90f32).to_radians())) < 1e-3);
}
