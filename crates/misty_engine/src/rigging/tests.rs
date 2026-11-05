use glam::{Quat, Vec3};
use hecs::Entity;

use super::*;
use crate::scene::{Scene, Transform};

fn at(s: &mut Scene, name: &str, parent: Option<Entity>, p: Vec3) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, Transform::from_position(p));
    e
}

/// Rig Builder ▸ Rig ▸ constraint object, returning the constraint object.
fn rig(s: &mut Scene) -> Entity {
    let builder = at(s, "Character", None, Vec3::ZERO);
    s.add(builder, RigBuilder::default());
    let r = at(s, "Rig", Some(builder), Vec3::ZERO);
    s.add(r, Rig::default());
    at(s, "Constraint", Some(r), Vec3::ZERO)
}

struct Arm {
    scene: Scene,
    bones: [Entity; 3],
    target: Entity,
    hint: Entity,
    constraint: Entity,
}

/// An arm along +X (two 1 m bones) with a Two Bone IK constraint.
fn arm(target: Vec3, hint: Vec3) -> Arm {
    let mut s = Scene::new("Test");
    let c = rig(&mut s);
    let root = at(&mut s, "Shoulder", None, Vec3::ZERO);
    let mid = at(&mut s, "Elbow", Some(root), Vec3::X);
    let tip = at(&mut s, "Hand", Some(mid), Vec3::X);
    let t = at(&mut s, "Target", None, target);
    let h = at(&mut s, "Hint", None, hint);
    s.add(
        c,
        TwoBoneIk {
            root: Some(root),
            mid: Some(mid),
            tip: Some(tip),
            target: Some(t),
            hint: Some(h),
            target_rotation_weight: 0.0,
            ..Default::default()
        },
    );
    Arm {
        scene: s,
        bones: [root, mid, tip],
        target: t,
        hint: h,
        constraint: c,
    }
}

fn eval(s: &mut Scene, st: &mut RigState) {
    evaluate(s, st, 1.0 / 60.0, None);
}

#[test]
fn two_bone_ik_reaches_and_straightens() {
    let goal = Vec3::new(1.0, 1.2, 0.3);
    let mut a = arm(goal, Vec3::new(0.5, 0.0, 2.0));
    let mut st = RigState::new();
    eval(&mut a.scene, &mut st);
    let tip = a.scene.world_position(a.bones[2]);
    assert!((tip - goal).length() < 1e-3, "tip {tip} goal {goal}");
    // Bone lengths are kept.
    let [r, m, t] = a.bones.map(|b| a.scene.world_position(b));
    assert!(((m - r).length() - 1.0).abs() < 1e-4);
    assert!(((t - m).length() - 1.0).abs() < 1e-4);

    // Out of reach: the arm points straight at the target.
    let far = Vec3::new(4.0, 2.0, -1.0);
    a.scene
        .set_transform(a.target, Transform::from_position(far));
    eval(&mut a.scene, &mut st);
    let [r, m, t] = a.bones.map(|b| a.scene.world_position(b));
    let dir = far.normalize();
    assert!((t - r).normalize().dot(dir) > 0.9999, "{t}");
    assert!((m - r).normalize().dot(dir) > 0.999, "elbow straight {m}");
    assert!(((t - r).length() - 2.0).abs() < 1e-3);
}

#[test]
fn hint_picks_the_elbow_side() {
    let goal = Vec3::new(1.2, 0.0, 0.0);
    for side in [1.0, -1.0] {
        let mut a = arm(goal, Vec3::new(0.6, 0.0, 3.0 * side));
        eval(&mut a.scene, &mut RigState::new());
        let elbow = a.scene.world_position(a.bones[1]);
        assert!(elbow.z * side > 0.5, "side {side}: elbow {elbow}");
        assert!(
            elbow.y.abs() < 1e-3,
            "elbow stays in the hint plane: {elbow}"
        );
        let tip = a.scene.world_position(a.bones[2]);
        assert!((tip - goal).length() < 1e-3);
    }
    let _ = arm(goal, Vec3::ZERO).hint;
}

#[test]
fn weight_zero_changes_nothing_and_weights_do_not_accumulate() {
    let mut a = arm(Vec3::new(0.5, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let before: Vec<Transform> = a.bones.iter().map(|b| a.scene.transform(*b)).collect();
    a.scene
        .world
        .get::<&mut TwoBoneIk>(a.constraint)
        .unwrap()
        .weight = 0.0;
    let mut st = RigState::new();
    eval(&mut a.scene, &mut st);
    for (b, t) in a.bones.iter().zip(&before) {
        assert_eq!(a.scene.transform(*b), *t);
    }
    // Rig weight 0 also disables it.
    a.scene
        .world
        .get::<&mut TwoBoneIk>(a.constraint)
        .unwrap()
        .weight = 1.0;
    let r = a.scene.find_by_name("Rig").unwrap();
    a.scene.world.get::<&mut Rig>(r).unwrap().weight = 0.0;
    eval(&mut a.scene, &mut st);
    for (b, t) in a.bones.iter().zip(&before) {
        assert_eq!(a.scene.transform(*b), *t);
    }

    // Half weight: the same pose every frame (blended against the input, not the last result).
    a.scene.world.get::<&mut Rig>(r).unwrap().weight = 0.5;
    eval(&mut a.scene, &mut st);
    let once = a.scene.world_position(a.bones[2]);
    for _ in 0..5 {
        eval(&mut a.scene, &mut st);
    }
    let later = a.scene.world_position(a.bones[2]);
    assert!((once - later).length() < 1e-5, "{once} vs {later}");
    let full = Vec3::new(0.5, 1.0, 0.0);
    assert!(
        (once - full).length() > 0.1,
        "half weight is not the full solve"
    );

    // Ending the run puts the authored pose back.
    st.restore(&mut a.scene);
    for (b, t) in a.bones.iter().zip(&before) {
        assert_eq!(a.scene.transform(*b).position, t.position);
        assert!(a.scene.transform(*b).rotation.dot(t.rotation).abs() > 0.99999);
    }
}

#[test]
fn bones_moved_by_animation_are_the_new_input() {
    let mut a = arm(Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let mut st = RigState::new();
    eval(&mut a.scene, &mut st);
    // "Animation" writes a new root position: the rig solves from there.
    let mut t = a.scene.transform(a.bones[0]);
    t.position = Vec3::new(0.0, 0.5, 0.0);
    a.scene.set_transform(a.bones[0], t);
    eval(&mut a.scene, &mut st);
    assert!((a.scene.transform(a.bones[0]).position.y - 0.5).abs() < 1e-6);
    let tip = a.scene.world_position(a.bones[2]);
    assert!((tip - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-3);
}

#[test]
fn multi_aim_points_the_axis_at_the_sources() {
    let mut s = Scene::new("Test");
    let c = rig(&mut s);
    let head = at(&mut s, "Head", None, Vec3::new(0.0, 1.0, 0.0));
    let a = at(&mut s, "A", None, Vec3::new(3.0, 2.0, 2.0));
    let b = at(&mut s, "B", None, Vec3::new(-1.0, 2.0, 2.0));
    s.add(
        c,
        MultiAim {
            constrained_object: Some(head),
            sources: vec![
                WeightedSource {
                    object: Some(a),
                    weight: 1.0,
                },
                WeightedSource {
                    object: Some(b),
                    weight: 1.0,
                },
            ],
            ..Default::default()
        },
    );
    eval(&mut s, &mut RigState::new());
    let r = s.world_rotation(head);
    let want = (Vec3::new(1.0, 2.0, 2.0) - Vec3::Y).normalize();
    assert!((r * Vec3::Z).dot(want) > 0.9999, "{}", r * Vec3::Z);
    // Scene Up keeps the local up axis in the vertical plane through the aim.
    let side = r * Vec3::X;
    assert!(side.y.abs() < 1e-4, "no roll: {side}");

    // -X aim with the limits: Y turn clamped to 30°.
    s.set_transform(head, Transform::from_position(Vec3::new(0.0, 1.0, 0.0)));
    {
        let mut m = s.world.get::<&mut MultiAim>(c).unwrap();
        m.sources.truncate(1);
        m.aim_axis = Axis::NegX;
        m.constrained_axes = [false, true, false];
        m.min_limit = -30.0;
        m.max_limit = 30.0;
    }
    eval(&mut s, &mut RigState::new());
    let r = s.world_rotation(head);
    let (y, x, z) = r.to_euler(glam::EulerRot::YXZ);
    assert!(
        (y.abs().to_degrees() - 30.0).abs() < 0.01,
        "{}",
        y.to_degrees()
    );
    assert!(x.abs() < 1e-4 && z.abs() < 1e-4);
}

fn chain_scene(solver: ChainSolver) -> (Scene, Vec<Entity>, Vec3) {
    let mut s = Scene::new("Test");
    let c = rig(&mut s);
    let mut bones = vec![at(&mut s, "Tail 0", None, Vec3::ZERO)];
    for i in 1..6 {
        let parent = bones[i - 1];
        bones.push(at(
            &mut s,
            &format!("Tail {i}"),
            Some(parent),
            Vec3::Y * 0.5,
        ));
    }
    let goal = Vec3::new(1.2, 1.1, 0.6);
    let t = at(&mut s, "Target", None, goal);
    s.add(
        c,
        ChainIk {
            root: Some(bones[0]),
            tip: Some(bones[5]),
            target: Some(t),
            solver,
            max_iterations: 60,
            tolerance: 1e-4,
            tip_rotation_weight: 0.0,
            ..Default::default()
        },
    );
    (s, bones, goal)
}

#[test]
fn chain_ik_converges_with_both_solvers() {
    for solver in ChainSolver::ALL {
        let (mut s, bones, goal) = chain_scene(solver);
        eval(&mut s, &mut RigState::new());
        let tip = s.world_position(bones[5]);
        assert!((tip - goal).length() < 5e-3, "{solver:?}: {tip}");
        for w in bones.windows(2) {
            let l = (s.world_position(w[1]) - s.world_position(w[0])).length();
            assert!((l - 0.5).abs() < 1e-3, "{solver:?} keeps bone lengths");
        }
        assert!(s.world_position(bones[0]).length() < 1e-6, "root stays put");
    }
}

#[test]
fn damped_transform_lags_then_settles() {
    let mut s = Scene::new("Test");
    let c = rig(&mut s);
    let src = at(&mut s, "Source", None, Vec3::ZERO);
    let follower = at(&mut s, "Follower", None, Vec3::new(0.0, 1.0, 0.0));
    s.add(
        c,
        DampedTransform {
            constrained_object: Some(follower),
            source: Some(src),
            damp_position: 0.8,
            damp_rotation: 0.8,
            maintain_aim: false,
            ..Default::default()
        },
    );
    let mut st = RigState::new();
    eval(&mut s, &mut st);
    assert!((s.world_position(follower) - Vec3::Y).length() < 1e-5);
    // The source jumps: the follower moves part of the way, then catches up.
    s.set_transform(src, Transform::from_position(Vec3::new(2.0, 0.0, 0.0)));
    eval(&mut s, &mut st);
    let p = s.world_position(follower);
    assert!(p.x > 0.05 && p.x < 1.0, "lags: {p}");
    for _ in 0..240 {
        eval(&mut s, &mut st);
    }
    let p = s.world_position(follower);
    assert!(
        (p - Vec3::new(2.0, 1.0, 0.0)).length() < 1e-3,
        "settles: {p}"
    );
}

#[test]
fn override_transform_replaces_the_pose() {
    let mut s = Scene::new("Test");
    let c = rig(&mut s);
    let parent = at(&mut s, "Parent", None, Vec3::new(1.0, 0.0, 0.0));
    let obj = at(&mut s, "Obj", Some(parent), Vec3::new(0.0, 1.0, 0.0));
    s.add(
        c,
        OverrideTransform {
            constrained_object: Some(obj),
            space: OverrideSpace::World,
            position: Vec3::new(5.0, 5.0, 5.0),
            rotation: Vec3::new(0.0, 90.0, 0.0),
            ..Default::default()
        },
    );
    eval(&mut s, &mut RigState::new());
    assert!((s.world_position(obj) - Vec3::splat(5.0)).length() < 1e-4);
    let fwd = s.world_rotation(obj) * Vec3::Z;
    assert!(fwd.dot(Vec3::X) > 0.9999, "{fwd}");
    let _ = Quat::IDENTITY;
}

#[test]
fn rig_components_round_trip_through_scene_files() {
    let mut a = arm(Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let builder = a.scene.find_by_name("Character").unwrap();
    let r = a.scene.find_by_name("Rig").unwrap();
    a.scene
        .world
        .get::<&mut RigBuilder>(builder)
        .unwrap()
        .layers = vec![RigLayer {
        rig: Some(r),
        active: true,
    }];
    a.scene.add(
        a.hint,
        MultiAim {
            constrained_object: Some(a.bones[0]),
            sources: vec![WeightedSource {
                object: Some(a.target),
                weight: 0.25,
            }],
            ..Default::default()
        },
    );
    let text = a.scene.to_json();
    let (back, report) = Scene::from_json("Test", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let name_of = |s: &Scene, e: Option<Entity>| e.map(|e| s.name(e));
    let ik = back
        .world
        .get::<&TwoBoneIk>(back.find_by_name("Constraint").unwrap())
        .unwrap()
        .clone();
    assert_eq!(name_of(&back, ik.root).as_deref(), Some("Shoulder"));
    assert_eq!(name_of(&back, ik.mid).as_deref(), Some("Elbow"));
    assert_eq!(name_of(&back, ik.tip).as_deref(), Some("Hand"));
    assert_eq!(name_of(&back, ik.target).as_deref(), Some("Target"));
    assert_eq!(name_of(&back, ik.hint).as_deref(), Some("Hint"));
    let b = back
        .world
        .get::<&RigBuilder>(back.find_by_name("Character").unwrap())
        .unwrap()
        .clone();
    assert_eq!(name_of(&back, b.layers[0].rig).as_deref(), Some("Rig"));
    let aim = back
        .world
        .get::<&MultiAim>(back.find_by_name("Hint").unwrap())
        .unwrap()
        .clone();
    assert_eq!(
        name_of(&back, aim.sources[0].object).as_deref(),
        Some("Target")
    );
    assert_eq!(aim.sources[0].weight, 0.25);
    assert_eq!(back.to_json(), text);
}

#[test]
fn lua_reads_and_writes_weights_and_targets() {
    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{ScriptRef, Scripts};
    use crate::scripting::{Input, ScriptHost};

    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_rig_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    let src = r#"
local T = {}
function T:Start()
    local ik = GameObject.Find("Constraint"):GetComponent("TwoBoneIKConstraint")
    print(ik.weight, ik.target.name, ik.hint.name, tostring(ik.maintainTargetPositionOffset))
    ik.weight = 0.25
    ik.hintWeight = 0.5
    ik.target = GameObject.Find("Hint")
    print(ik.weight, ik.hintWeight, ik.target.name)
    local rig = GameObject.Find("Rig"):GetComponent("Rig")
    rig.weight = 0.75
    local aim = GameObject.Find("Hint"):GetComponent("Multi-Aim Constraint")
    aim:AddSource(GameObject.Find("Target"), 0.5)
    aim:SetSourceWeight(1, 0.8)
    print(aim.sourceCount, aim:GetSource(1).name, aim:GetSourceWeight(1), aim.aimAxis)
    aim.aimAxis = "-X"
    aim.offset = Vector3.new(0, 10, 0)
    print(aim.aimAxis, aim.offset.y)
end
return T
"#;
    std::fs::write(root.join("Assets/Scripts/T.lua"), src).unwrap();
    let mut a = arm(Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    a.scene.add(a.hint, MultiAim::default());
    a.scene.add(
        a.constraint,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/T.lua".into(),
                ..Default::default()
            }],
        },
    );
    let mut scene = a.scene;
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    h.update(&mut scene, &time, Input::default());
    let logs: Vec<String> = h.take_logs().into_iter().map(|(_, t)| t).collect();
    assert_eq!(
        logs,
        [
            "1 Target Hint false",
            "0.25 0.5 Hint",
            "1 Target 0.8 Z",
            "-X 10"
        ]
    );
    let ik = scene.world.get::<&TwoBoneIk>(a.constraint).unwrap().clone();
    assert_eq!(ik.target, Some(a.hint));
    let r = scene.find_by_name("Rig").unwrap();
    assert_eq!(scene.world.get::<&Rig>(r).unwrap().weight, 0.75);
}
