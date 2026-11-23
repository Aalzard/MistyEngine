//! The Animator IK pass solver: hands and feet reach their goals (with weights, hints and
//! rotations), the head looks at a point, and last frame's IK is undone before the next.

use glam::{Quat, Vec3};
use hecs::Entity;

use super::*;
use crate::animation::humanoid::Avatar;
use crate::scene::{Scene, Transform};

fn add(scene: &mut Scene, parent: Entity, name: &str, pos: Vec3) -> Entity {
    let e = scene.spawn(name, Some(parent));
    scene.set_transform(e, Transform::from_position(pos));
    e
}

/// A small humanoid: hips 1 m up, arms along ±x with a slight bend back at the elbow, legs
/// down with a slight bend forward at the knee.
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

struct Setup {
    scene: Scene,
    root: Entity,
    avatar: Avatar,
    ik: AnimatorIkState,
}

fn setup() -> Setup {
    let mut scene = Scene::new("t");
    let root = rig(&mut scene);
    let avatar = Avatar::auto_map(&scene, root);
    Setup {
        scene,
        root,
        avatar,
        ik: AnimatorIkState::new(),
    }
}

fn find(s: &Scene, name: &str) -> Entity {
    s.find_by_name(name).unwrap_or_else(|| panic!("{name}"))
}

impl Setup {
    fn wp(&self, name: &str) -> Vec3 {
        self.scene.world_position(find(&self.scene, name))
    }

    fn frame(&mut self, f: impl FnOnce(&mut IkGoals)) {
        self.ik.begin(&mut self.scene, self.root);
        f(self.ik.goals_mut(self.root));
        self.ik.solve(&mut self.scene, self.root, &self.avatar);
    }
}

#[test]
fn hand_bone_reaches_its_goal() {
    let mut s = setup();
    let target = Vec3::new(-0.4, 1.2, 0.3);
    s.frame(|g| {
        let goal = g.goal_mut(IkGoal::LeftHand);
        goal.position = target;
        goal.position_weight = 1.0;
    });
    let hand = s.wp("LeftHand");
    assert!(
        hand.distance(target) < 1e-3,
        "hand at {hand}, target {target}"
    );
    // The other limbs keep their pose.
    assert!(s.wp("RightHand").distance(Vec3::new(0.8, 1.55, 0.0)) < 1e-4);
    // Bone lengths are kept.
    let (arm, fore) = (s.wp("LeftArm"), s.wp("LeftForeArm"));
    assert!((arm.distance(fore) - Vec3::new(0.3, 0.0, 0.02).length()).abs() < 1e-4);
}

#[test]
fn foot_goal_and_half_weight() {
    let mut s = setup();
    let start = s.wp("RightFoot");
    let target = start + Vec3::new(0.0, 0.3, 0.2);
    s.frame(|g| {
        let goal = g.goal_mut(IkGoal::RightFoot);
        goal.position = target;
        goal.position_weight = 1.0;
    });
    assert!(s.wp("RightFoot").distance(target) < 1e-3);
    // Next frame at half weight: halfway from the animated pose (restored), not from last
    // frame's result.
    s.frame(|g| {
        let goal = g.goal_mut(IkGoal::RightFoot);
        goal.position = target;
        goal.position_weight = 0.5;
    });
    let half = start.lerp(target, 0.5);
    assert!(
        s.wp("RightFoot").distance(half) < 1e-3,
        "{}",
        s.wp("RightFoot")
    );
}

#[test]
fn goals_reset_every_frame_and_the_pose_comes_back() {
    let mut s = setup();
    let start = s.wp("LeftHand");
    s.frame(|g| {
        let goal = g.goal_mut(IkGoal::LeftHand);
        goal.position = Vec3::new(-0.3, 1.2, 0.3);
        goal.position_weight = 1.0;
    });
    assert!(s.wp("LeftHand").distance(start) > 0.1);
    // A frame where the script sets nothing: weights are back to 0, the arm to its pose.
    s.frame(|_| {});
    assert_eq!(
        s.ik.goals(s.root).goals[IkGoal::LeftHand as usize].position_weight,
        0.0
    );
    assert!(s.wp("LeftHand").distance(start) < 1e-5);
}

#[test]
fn hint_turns_the_elbow_towards_it() {
    let mut s = setup();
    let target = Vec3::new(-0.45, 1.3, 0.0);
    for z in [-1.0f32, 1.0] {
        s.frame(|g| {
            let goal = g.goal_mut(IkGoal::LeftHand);
            goal.position = target;
            goal.position_weight = 1.0;
            let hint = g.hint_mut(IkHint::LeftElbow);
            hint.position = Vec3::new(-0.4, 1.3, z);
            hint.weight = 1.0;
        });
        assert!(s.wp("LeftHand").distance(target) < 1e-3);
        let elbow = s.wp("LeftForeArm");
        assert!(elbow.z * z > 0.05, "elbow {elbow} should lean to z {z}");
    }
}

#[test]
fn rotation_goal_is_an_effector_rotation() {
    let mut s = setup();
    let hand = find(&s.scene, "RightHand");
    // Identity: the rest rotation (here the identity too); a turn: turn · rest.
    let turn = Quat::from_rotation_y(0.7);
    s.frame(|g| {
        let goal = g.goal_mut(IkGoal::RightHand);
        goal.rotation = turn;
        goal.rotation_weight = 1.0;
    });
    let r = s.scene.world_rotation(hand);
    assert!(r.angle_between(turn) < 1e-3);
    assert!(
        effector_rotation(&s.scene, s.root, &s.avatar, IkGoal::RightHand).angle_between(turn)
            < 1e-3
    );
    // Position untouched by a rotation-only goal.
    assert!(s.wp("RightHand").distance(Vec3::new(0.8, 1.55, 0.0)) < 1e-4);
}

#[test]
fn head_looks_at_the_point() {
    let mut s = setup();
    let head = find(&s.scene, "Head");
    let p = s.scene.world_position(head) + Vec3::new(1.0, 0.3, 1.0);
    s.frame(|g| {
        g.look_at.position = p;
        g.look_at.weight = 1.0;
        g.look_at.clamp_weight = 0.0;
    });
    let fwd = s.scene.world_rotation(head) * Vec3::Z;
    let want = (p - s.scene.world_position(head)).normalize();
    assert!(fwd.angle_between(want) < 1e-3, "{fwd} vs {want}");
    // Clamped at 0.5: at most 90° even for a point behind.
    let behind = s.scene.world_position(head) + Vec3::new(0.0, 0.0, -1.0);
    s.frame(|g| {
        g.look_at.position = behind;
        g.look_at.weight = 1.0;
    });
    let fwd = s.scene.world_rotation(head) * Vec3::Z;
    assert!(fwd.angle_between(Vec3::Z) <= std::f32::consts::FRAC_PI_2 + 1e-3);
}

#[test]
fn body_weight_turns_the_spine_too() {
    let mut s = setup();
    let p = s.wp("Head") + Vec3::new(1.0, 0.0, 0.2);
    s.frame(|g| {
        g.look_at.position = p;
        g.look_at.weight = 1.0;
        g.look_at.body_weight = 1.0;
        g.look_at.head_weight = 0.0;
        g.look_at.clamp_weight = 0.0;
    });
    let chest = s.scene.world_rotation(find(&s.scene, "Chest"));
    assert!(chest.angle_between(Quat::IDENTITY) > 0.3);
}

#[test]
fn bones_and_numbers() {
    let s = setup();
    assert_eq!(human_body_bone(17), Some(HumanBone::LeftHand));
    assert_eq!(human_body_bone(54), None);
    assert_eq!(IkGoal::from_name("RightFoot"), Some(IkGoal::RightFoot));
    assert_eq!(IkHint::from_index(3), Some(IkHint::RightElbow));
    assert_eq!(
        bone_entity(&s.scene, s.root, &s.avatar, HumanBone::Head),
        Some(find(&s.scene, "Head"))
    );
}

#[test]
fn out_of_reach_goal_straightens_the_arm_towards_it() {
    let mut s = setup();
    let shoulder = s.wp("LeftArm");
    let target = Vec3::new(-0.4, 0.2, 1.5);
    s.frame(|g| {
        let goal = g.goal_mut(IkGoal::LeftHand);
        goal.position = target;
        goal.position_weight = 1.0;
    });
    let hand = s.wp("LeftHand");
    let dir = (target - shoulder).normalize();
    assert!((hand - shoulder).normalize().angle_between(dir) < 1e-2);
    assert!(hand.is_finite());
}
