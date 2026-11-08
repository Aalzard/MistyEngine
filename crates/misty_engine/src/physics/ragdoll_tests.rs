//! Ragdoll Wizard output (bones, colliders, joints, masses) and switching ragdolls at runtime.

use super::*;
use crate::scene::{BoxCollider, Transform};

/// A standing humanoid skeleton (bones only) named like most rigs, facing +Z, 1.75 m tall.
pub(crate) fn skeleton(scene: &mut Scene) -> Entity {
    let root = scene.spawn("Character", None);
    let bone = |s: &mut Scene, name: &str, parent: Entity, local: Vec3| {
        let e = s.spawn(name, Some(parent));
        s.set_transform(e, Transform::from_position(local));
        e
    };
    let hips = bone(scene, "Hips", root, Vec3::new(0.0, 1.0, 0.0));
    let spine = bone(scene, "Spine", hips, Vec3::new(0.0, 0.2, 0.0));
    let chest = bone(scene, "Chest", spine, Vec3::new(0.0, 0.2, 0.0));
    let neck = bone(scene, "Neck", chest, Vec3::new(0.0, 0.2, 0.0));
    bone(scene, "Head", neck, Vec3::new(0.0, 0.1, 0.0));
    for (side, x) in [("Left", -1.0), ("Right", 1.0)] {
        let upper = bone(
            scene,
            &format!("{side}UpperArm"),
            chest,
            Vec3::new(0.2 * x, 0.15, 0.0),
        );
        let lower = bone(
            scene,
            &format!("{side}LowerArm"),
            upper,
            Vec3::new(0.0, -0.28, 0.0),
        );
        bone(
            scene,
            &format!("{side}Hand"),
            lower,
            Vec3::new(0.0, -0.26, 0.0),
        );
        let thigh = bone(
            scene,
            &format!("{side}UpperLeg"),
            hips,
            Vec3::new(0.1 * x, -0.05, 0.0),
        );
        let calf = bone(
            scene,
            &format!("{side}LowerLeg"),
            thigh,
            Vec3::new(0.0, -0.45, 0.0),
        );
        bone(
            scene,
            &format!("{side}Foot"),
            calf,
            Vec3::new(0.0, -0.42, 0.0),
        );
    }
    root
}

fn floor(scene: &mut Scene) {
    let f = scene.spawn("Floor", None);
    let mut t = Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
    t.scale = Vec3::new(20.0, 1.0, 20.0);
    scene.set_transform(f, t);
    scene.add(f, BoxCollider::default());
}

fn named(scene: &Scene, name: &str) -> Entity {
    scene.find_by_name(name).unwrap()
}

#[test]
fn auto_fill_finds_the_humanoid_bones_by_name() {
    let mut scene = Scene::new("t");
    let root = skeleton(&mut scene);
    let bones = auto_fill(&scene, root);
    assert!(
        bones.missing(&scene).is_empty(),
        "{:?}",
        bones.missing(&scene)
    );
    let expect = [
        "Hips",
        "LeftUpperLeg",
        "LeftLowerLeg",
        "LeftFoot",
        "RightUpperLeg",
        "RightLowerLeg",
        "RightFoot",
        "LeftUpperArm",
        "LeftLowerArm",
        "RightUpperArm",
        "RightLowerArm",
        "Chest",
        "Head",
        "LeftHand",
        "RightHand",
    ];
    for (slot, name) in expect.iter().enumerate() {
        assert_eq!(
            bones.get(slot).map(|e| scene.name(e)),
            Some((*name).to_owned()),
            "{}",
            LABELS[slot]
        );
    }
}

#[test]
fn the_wizard_needs_every_required_bone() {
    let mut scene = Scene::new("t");
    let root = skeleton(&mut scene);
    let mut bones = auto_fill(&scene, root);
    bones.set(12, None);
    let err = create(&mut scene, root, &bones, &RagdollOptions::default()).unwrap_err();
    assert!(err.contains("Head"), "{err}");
    assert!(
        scene
            .world
            .get::<&Rigidbody>(named(&scene, "Hips"))
            .is_err()
    );
}

#[test]
fn the_wizard_makes_colliders_bodies_and_joints() {
    let mut scene = Scene::new("t");
    let root = skeleton(&mut scene);
    let bones = auto_fill(&scene, root);
    let options = RagdollOptions {
        total_mass: 70.0,
        ..Default::default()
    };
    let report = create(&mut scene, root, &bones, &options).unwrap();
    assert_eq!(report.bodies.len(), 11);
    let w = &scene.world;
    // Masses add up to the total, distributed like a body.
    let mass = |n: &str| w.get::<&Rigidbody>(named(&scene, n)).unwrap().mass;
    let total: f32 = report
        .bodies
        .iter()
        .map(|e| w.get::<&Rigidbody>(*e).unwrap().mass)
        .sum();
    assert!((total - 70.0).abs() < 1e-3, "{total}");
    assert!(mass("Chest") > mass("Head") && mass("Hips") > mass("LeftUpperLeg"));
    assert!(mass("LeftUpperLeg") > mass("LeftLowerLeg"));
    assert!(mass("LeftUpperArm") > mass("LeftLowerArm"));
    assert_eq!(mass("LeftUpperLeg"), mass("RightUpperLeg"));
    // Feet and hands, the neck and the spine below the chest get nothing.
    for n in ["LeftFoot", "RightHand", "Neck", "Spine"] {
        assert!(w.get::<&Rigidbody>(named(&scene, n)).is_err(), "{n}");
    }
    // Capsules along the limbs, as long as the bone.
    let thigh = w
        .get::<&CapsuleCollider>(named(&scene, "LeftUpperLeg"))
        .unwrap();
    assert_eq!(thigh.direction, 1, "along the leg (local Y)");
    assert!((thigh.height - 0.45).abs() < 1e-3, "{}", thigh.height);
    assert!(
        (thigh.center.y + 0.225).abs() < 1e-3,
        "halfway down: {}",
        thigh.center
    );
    assert!(thigh.radius > 0.05 && thigh.radius < 0.15);
    let forearm = w
        .get::<&CapsuleCollider>(named(&scene, "LeftLowerArm"))
        .unwrap();
    assert!(
        (forearm.height - 0.26).abs() < 1e-3,
        "to the hand: {}",
        forearm.height
    );
    // Boxes for the pelvis and chest, a sphere for the head above its bone.
    let pelvis = w.get::<&BoxCollider>(named(&scene, "Hips")).unwrap();
    assert!(
        pelvis.size.x > 0.2 && pelvis.size.y > 0.1,
        "{}",
        pelvis.size
    );
    let chest = w.get::<&BoxCollider>(named(&scene, "Chest")).unwrap();
    assert!(chest.size.x > 0.3, "shoulder wide: {}", chest.size);
    let head = w.get::<&SphereCollider>(named(&scene, "Head")).unwrap();
    assert!(head.center.y > 0.0 && head.radius > 0.04, "{head:?}");
    // Character Joints to the body above.
    let parent = |n: &str| {
        w.get::<&CharacterJoint>(named(&scene, n))
            .unwrap()
            .connected_body
            .map(|e| scene.name(e))
    };
    for (child, up) in [
        ("LeftUpperLeg", "Hips"),
        ("RightLowerLeg", "RightUpperLeg"),
        ("LeftUpperArm", "Chest"),
        ("RightLowerArm", "RightUpperArm"),
        ("Chest", "Hips"),
        ("Head", "Chest"),
    ] {
        assert_eq!(parent(child), Some(up.to_owned()), "{child}");
    }
    assert!(w.get::<&CharacterJoint>(named(&scene, "Hips")).is_err());
    // The knee bends backwards: its twist range is mostly on the side that turns the shin back.
    let knee = w
        .get::<&CharacterJoint>(named(&scene, "LeftLowerLeg"))
        .unwrap();
    let axis = knee.axis;
    let (lo, hi) = (knee.low_twist_limit.limit, knee.high_twist_limit.limit);
    let bend = if hi.abs() > lo.abs() { hi } else { lo };
    let shin = Quat::from_axis_angle(axis, bend.to_radians()) * Vec3::NEG_Y;
    assert!(shin.z < -0.5, "the shin swings back (-Z): {shin}");
    let elbow = w
        .get::<&CharacterJoint>(named(&scene, "LeftLowerArm"))
        .unwrap();
    let (lo, hi) = (elbow.low_twist_limit.limit, elbow.high_twist_limit.limit);
    let bend = if hi.abs() > lo.abs() { hi } else { lo };
    let forearm = Quat::from_axis_angle(elbow.axis, bend.to_radians()) * Vec3::NEG_Y;
    assert!(
        forearm.z > 0.5,
        "the forearm swings forward (+Z): {forearm}"
    );
    // The root lists the bones.
    let r = w.get::<&Ragdoll>(root).unwrap();
    assert_eq!(r.bones.len(), 11);
    assert!(!r.enabled_on_start);
}

#[test]
fn running_the_wizard_again_replaces_its_work() {
    let mut scene = Scene::new("t");
    let root = skeleton(&mut scene);
    let bones = auto_fill(&scene, root);
    create(&mut scene, root, &bones, &RagdollOptions::default()).unwrap();
    let options = RagdollOptions {
        total_mass: 40.0,
        strength: 500.0,
        ..Default::default()
    };
    create(&mut scene, root, &bones, &options).unwrap();
    let hips = named(&scene, "Hips");
    let names = scene.component_names(hips);
    assert_eq!(
        names.iter().filter(|n| *n == "Rigidbody").count(),
        1,
        "{names:?}"
    );
    let knee = named(&scene, "LeftLowerLeg");
    assert_eq!(
        scene
            .world
            .get::<&CharacterJoint>(knee)
            .unwrap()
            .break_force,
        500.0
    );
}

/// Run the physics and the post-animation pass like the script host does.
fn play(scene: &mut Scene, physics: &mut Physics, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        physics.update(scene, 1.0 / 60.0);
        physics.after_animation(scene, 1.0 / 60.0);
    }
}

#[test]
fn a_disabled_ragdoll_stands_and_an_enabled_one_falls() {
    let mut scene = Scene::new("t");
    floor(&mut scene);
    let root = skeleton(&mut scene);
    let bones = auto_fill(&scene, root);
    create(&mut scene, root, &bones, &RagdollOptions::default()).unwrap();
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let head = named(&scene, "Head");
    play(&mut scene, &mut physics, 1.0);
    assert!(
        scene.world_position(head).y > 1.6,
        "kinematic: still standing"
    );
    assert_eq!(physics.ragdoll_enabled(root), Some(false));

    // Knock it over: physics on, a shove at the chest.
    assert!(physics.set_ragdoll(&scene, named(&scene, "LeftLowerLeg"), true, None));
    assert_eq!(physics.ragdoll_enabled(root), Some(true));
    physics.apply_impulse(named(&scene, "Chest"), Vec3::new(0.0, 0.0, 30.0));
    play(&mut scene, &mut physics, 3.0);
    let h = scene.world_position(head);
    assert!(h.y < 0.6, "fell to the floor: {h}");
    assert!(h.is_finite());
    // Joints held: each limb still starts where its parent ends.
    let knee = scene.world_position(named(&scene, "LeftLowerLeg"));
    let thigh = scene.world_matrix(named(&scene, "LeftUpperLeg"));
    let thigh_end = thigh.transform_point3(Vec3::new(0.0, -0.45, 0.0));
    assert!((knee - thigh_end).length() < 0.05, "{knee} vs {thigh_end}");
    // Nothing went through the floor.
    for b in &scene.world.get::<&Ragdoll>(root).unwrap().bones.clone() {
        let y = scene.world_position(b.unwrap()).y;
        assert!(y > -0.1, "{} at {y}", scene.name(b.unwrap()));
    }
}

#[test]
fn disabling_blends_from_the_physics_pose_back_to_the_animation() {
    let mut scene = Scene::new("t");
    floor(&mut scene);
    let root = skeleton(&mut scene);
    let bones = auto_fill(&scene, root);
    create(&mut scene, root, &bones, &RagdollOptions::default()).unwrap();
    let rest: Vec<(Entity, Transform)> = scene
        .walk()
        .into_iter()
        .map(|(e, _)| (e, scene.transform(e)))
        .collect();
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    physics.set_ragdoll(&scene, root, true, None);
    play(&mut scene, &mut physics, 2.0);
    let head = named(&scene, "Head");
    let fallen = scene.world_position(head);
    assert!(fallen.y < 1.0);
    physics.set_ragdoll(&scene, root, false, Some(0.5));
    // An "Animator" holding the rest pose: written each frame before the ragdoll pass.
    let animate = |scene: &mut Scene| {
        for (e, t) in &rest {
            scene.set_transform(*e, *t);
        }
    };
    let mut halfway = Vec3::ZERO;
    for frame in 0..40 {
        physics.update(&mut scene, 1.0 / 60.0);
        animate(&mut scene);
        physics.after_animation(&mut scene, 1.0 / 60.0);
        if frame == 14 {
            halfway = scene.world_position(head);
        }
    }
    let standing = Vec3::new(0.0, 1.7, 0.0);
    let end = scene.world_position(head);
    assert!(
        (end - standing).length() < 0.01,
        "back on the animation: {end}"
    );
    assert!(
        halfway.y > fallen.y + 0.1 && halfway.y < standing.y - 0.1,
        "blending: {} between {} and {}",
        halfway.y,
        fallen.y,
        standing.y
    );
    assert_eq!(physics.ragdoll_enabled(head), Some(false));
}

#[test]
fn objects_outside_a_ragdoll_are_refused() {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Crate", None);
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    assert!(!physics.set_ragdoll(&scene, e, true, None));
    assert_eq!(physics.ragdoll_enabled(e), None);
}
