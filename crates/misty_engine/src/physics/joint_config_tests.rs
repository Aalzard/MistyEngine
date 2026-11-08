//! Configurable and Character Joints (limits, soft limits, drives, projection), break forces
//! and Physics Materials.

use super::*;
use crate::scene::{
    BoxCollider, CharacterJoint, ConfigurableJoint, FixedJoint, JointDrive, JointMotion,
    JointProjectionMode, SoftJointLimit, SoftJointLimitSpring, SpringJoint, Transform,
};

const G: Vec3 = Vec3::new(0.0, -9.81, 0.0);

fn body(scene: &mut Scene, name: &str, pos: Vec3, scale: Vec3) -> Entity {
    let e = scene.spawn(name, None);
    let mut t = Transform::from_position(pos);
    t.scale = scale;
    scene.set_transform(e, t);
    scene.add(e, BoxCollider::default());
    scene.add(e, Rigidbody::default());
    e
}

fn run(scene: &mut Scene, physics: &mut Physics, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        physics.update(scene, 1.0 / 60.0);
    }
}

/// Signed rotation of `e` about +Z, in degrees.
fn angle_z(scene: &Scene, e: Entity) -> f32 {
    let v = scene.world_rotation(e) * Vec3::X;
    v.y.atan2(v.x).to_degrees()
}

fn locked() -> ConfigurableJoint {
    ConfigurableJoint {
        x_motion: JointMotion::Locked,
        y_motion: JointMotion::Locked,
        z_motion: JointMotion::Locked,
        angular_x_motion: JointMotion::Locked,
        angular_y_motion: JointMotion::Locked,
        angular_z_motion: JointMotion::Locked,
        ..Default::default()
    }
}

/// A 2 m bar fixed to the world at its left end, turning about Z (the joint's primary axis).
fn bar(joint: ConfigurableJoint) -> (Scene, Entity) {
    let mut scene = Scene::new("t");
    let bar = body(
        &mut scene,
        "Bar",
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(2.0, 0.2, 0.2),
    );
    scene.world.get::<&mut Rigidbody>(bar).unwrap().angular_drag = 2.0;
    scene.add(
        bar,
        ConfigurableJoint {
            anchor: Vec3::new(-0.5, 0.0, 0.0),
            axis: Vec3::Z,
            secondary_axis: Vec3::Y,
            ..joint
        },
    );
    (scene, bar)
}

#[test]
fn configurable_joint_limits_its_angular_axis() {
    let (mut scene, bar) = bar(ConfigurableJoint {
        angular_x_motion: JointMotion::Limited,
        low_angular_x_limit: SoftJointLimit::new(-30.0),
        high_angular_x_limit: SoftJointLimit::new(30.0),
        ..locked()
    });
    let mut physics = Physics::build(&scene, G);
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    run(&mut scene, &mut physics, 3.0);
    let a = angle_z(&scene, bar);
    assert!(
        (a.abs() - 30.0).abs() < 3.0,
        "stopped at the 30° limit: {a}"
    );
    let anchor = scene
        .world_matrix(bar)
        .transform_point3(Vec3::new(-0.5, 0.0, 0.0));
    assert!(anchor.length() < 0.05, "linear axes locked: {anchor}");
}

#[test]
fn configurable_joint_free_axis_swings_and_locked_one_holds() {
    let (mut scene, bar) = bar(ConfigurableJoint {
        angular_x_motion: JointMotion::Free,
        ..locked()
    });
    let mut physics = Physics::build(&scene, G);
    run(&mut scene, &mut physics, 2.0);
    assert!(angle_z(&scene, bar).abs() > 45.0, "free about the axis");
    let p = scene.world_position(bar);
    assert!(p.z.abs() < 0.02, "the other rotations are locked: {p}");
}

#[test]
fn soft_angular_limit_lets_it_past_then_springs_back() {
    let (mut scene, bar) = bar(ConfigurableJoint {
        angular_x_motion: JointMotion::Limited,
        low_angular_x_limit: SoftJointLimit::new(-10.0),
        high_angular_x_limit: SoftJointLimit::new(10.0),
        angular_x_limit_spring: SoftJointLimitSpring {
            spring: 200.0,
            damper: 20.0,
        },
        ..locked()
    });
    let mut physics = Physics::build(&scene, G);
    run(&mut scene, &mut physics, 4.0);
    let a = angle_z(&scene, bar).abs();
    // Torque of the 1 kg bar's weight 1 m out: 9.81 N·m → 9.81 / 200 rad ≈ 2.8° past 10°.
    assert!(
        a > 10.5 && a < 20.0,
        "settled a little past the soft limit: {a}"
    );
}

#[test]
fn linear_drive_holds_the_body_at_its_target_position() {
    let mut scene = Scene::new("t");
    let b = body(&mut scene, "Lift", Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
    scene.add(
        b,
        ConfigurableJoint {
            y_motion: JointMotion::Free,
            target_position: Vec3::new(0.0, 0.5, 0.0),
            y_drive: JointDrive {
                position_spring: 5000.0,
                position_damper: 150.0,
                ..Default::default()
            },
            ..locked()
        },
    );
    let mut physics = Physics::build(&scene, G);
    run(&mut scene, &mut physics, 3.0);
    let y = scene.world_position(b).y;
    // Spring sag: m g / k = 9.81 / 5000 ≈ 2 mm.
    assert!((y - 1.5).abs() < 0.03, "lifted to its target: {y}");
    let x = scene.world_position(b).x;
    assert!(x.abs() < 0.01, "X stays locked: {x}");
}

#[test]
fn drive_maximum_force_caps_the_lift() {
    let mut scene = Scene::new("t");
    let b = body(&mut scene, "Weak", Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
    scene.add(
        b,
        ConfigurableJoint {
            y_motion: JointMotion::Free,
            target_position: Vec3::new(0.0, 0.5, 0.0),
            y_drive: JointDrive {
                position_spring: 5000.0,
                position_damper: 150.0,
                maximum_force: 2.0,
            },
            ..locked()
        },
    );
    let mut physics = Physics::build(&scene, G);
    run(&mut scene, &mut physics, 1.0);
    assert!(
        scene.world_position(b).y < 1.0,
        "2 N can't hold up a 1 kg body"
    );
}

#[test]
fn angular_velocity_drive_spins_the_body() {
    let mut scene = Scene::new("t");
    let b = body(&mut scene, "Spinner", Vec3::ZERO, Vec3::ONE);
    scene.add(
        b,
        ConfigurableJoint {
            axis: Vec3::Y,
            secondary_axis: Vec3::X,
            angular_x_motion: JointMotion::Free,
            target_angular_velocity: Vec3::new(90.0, 0.0, 0.0),
            angular_x_drive: JointDrive {
                position_damper: 1000.0,
                ..Default::default()
            },
            ..locked()
        },
    );
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    run(&mut scene, &mut physics, 1.0);
    let w = physics.angular_velocity(b).unwrap();
    assert!(
        (w.y.abs() - 90f32.to_radians()).abs() < 0.1,
        "90°/s about the joint axis (world Y): {w}"
    );
}

#[test]
fn character_joint_twist_and_swing_limits_hold() {
    let mut scene = Scene::new("t");
    let bar = body(
        &mut scene,
        "Limb",
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(2.0, 0.2, 0.2),
    );
    scene.world.get::<&mut Rigidbody>(bar).unwrap().angular_drag = 2.0;
    scene.add(
        bar,
        CharacterJoint {
            anchor: Vec3::new(-0.5, 0.0, 0.0),
            // Twist about Z (the swing under gravity), swing axis Y.
            axis: Vec3::Z,
            swing_axis: Vec3::Y,
            low_twist_limit: SoftJointLimit::new(-25.0),
            high_twist_limit: SoftJointLimit::new(25.0),
            swing1_limit: SoftJointLimit::new(10.0),
            swing2_limit: SoftJointLimit::new(10.0),
            ..Default::default()
        },
    );
    let mut physics = Physics::build(&scene, G);
    run(&mut scene, &mut physics, 3.0);
    let a = angle_z(&scene, bar).abs();
    assert!((a - 25.0).abs() < 3.0, "held at the 25° twist limit: {a}");
    let anchor = scene
        .world_matrix(bar)
        .transform_point3(Vec3::new(-0.5, 0.0, 0.0));
    assert!(
        anchor.length() < 0.05,
        "ball joint stays on its anchor: {anchor}"
    );
}

/// A 2 kg block hanging under a fixed joint with this break force.
fn hanging(break_force: f32) -> (Scene, Entity) {
    let mut scene = Scene::new("t");
    let b = body(&mut scene, "Weight", Vec3::new(0.0, -1.0, 0.0), Vec3::ONE);
    scene.world.get::<&mut Rigidbody>(b).unwrap().mass = 2.0;
    scene.add(
        b,
        FixedJoint {
            break_force,
            ..Default::default()
        },
    );
    (scene, b)
}

#[test]
fn joint_force_reads_the_weight_it_holds() {
    let (mut scene, b) = hanging(f32::INFINITY);
    let mut physics = Physics::build(&scene, G);
    run(&mut scene, &mut physics, 1.0);
    let (force, _) = physics.joint_force(b, "Fixed Joint").unwrap();
    assert!(
        (force - 2.0 * 9.81).abs() < 2.0,
        "a 2 kg weight pulls with about 19.6 N: {force}"
    );
}

#[test]
fn joints_break_over_their_break_force_only() {
    let (mut strong_scene, strong) = hanging(40.0);
    let mut physics = Physics::build(&strong_scene, G);
    run(&mut strong_scene, &mut physics, 1.0);
    assert!(physics.has_joint(strong, "Fixed Joint"));
    assert!(physics.take_joint_breaks().is_empty());
    assert!(strong_scene.world_position(strong).y > -1.05, "still held");

    let (mut weak_scene, weak) = hanging(10.0);
    let mut physics = Physics::build(&weak_scene, G);
    run(&mut weak_scene, &mut physics, 1.0);
    assert!(!physics.has_joint(weak, "Fixed Joint"));
    let breaks = physics.take_joint_breaks();
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].entity, weak);
    assert_eq!(breaks[0].joint, "Fixed Joint");
    assert!(breaks[0].force > 10.0, "{:?}", breaks[0]);
    assert!(weak_scene.world_position(weak).y < -2.0, "fell once free");
}

#[test]
fn break_torque_snaps_a_bent_joint() {
    let mut scene = Scene::new("t");
    // A 2 m bar fixed at its end: its weight makes a torque of about 9.81 N·m.
    let bar = body(
        &mut scene,
        "Beam",
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(2.0, 0.2, 0.2),
    );
    scene.add(
        bar,
        ConfigurableJoint {
            anchor: Vec3::new(-0.5, 0.0, 0.0),
            break_torque: 4.0,
            ..locked()
        },
    );
    let mut physics = Physics::build(&scene, G);
    for _ in 0..3 {
        physics.step(&scene);
    }
    let held = physics.joint_force(bar, "Configurable Joint");
    run(&mut scene, &mut physics, 0.5);
    let breaks = physics.take_joint_breaks();
    assert_eq!(breaks.len(), 1, "{breaks:?} (force, torque: {held:?})");
    assert_eq!(breaks[0].joint, "Configurable Joint");
}

#[test]
fn spring_joints_break_when_pulled_too_hard() {
    let mut scene = Scene::new("t");
    let b = body(&mut scene, "Bob", Vec3::new(0.0, -1.0, 0.0), Vec3::ONE);
    scene.add(
        b,
        SpringJoint {
            spring: 100.0,
            damper: 1.0,
            break_force: 5.0,
            ..Default::default()
        },
    );
    let mut physics = Physics::build(&scene, G);
    run(&mut scene, &mut physics, 1.0);
    let breaks = physics.take_joint_breaks();
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].joint, "Spring Joint");
    assert!(!physics.has_joint(b, "Spring Joint"));
}

#[test]
fn projection_pulls_a_drifted_body_back_at_once() {
    let make = |projection: bool| {
        let mut scene = Scene::new("t");
        let b = body(&mut scene, "Link", Vec3::new(0.0, -1.0, 0.0), Vec3::ONE);
        scene.add(
            b,
            ConfigurableJoint {
                projection_mode: if projection {
                    JointProjectionMode::PositionAndRotation
                } else {
                    JointProjectionMode::None
                },
                projection_distance: 0.1,
                projection_angle: 5.0,
                ..locked()
            },
        );
        let mut physics = Physics::build(&scene, Vec3::ZERO);
        // Knock it 1.5 m away from its joint (a stretched chain, a teleport) and project.
        physics.teleport(b, Vec3::new(1.5, -1.0, 0.0), Quat::from_rotation_z(1.0));
        physics.project_joints();
        let (p, q) = physics.body_pose(b).unwrap();
        (
            (p - Vec3::new(0.0, -1.0, 0.0)).length(),
            q.angle_between(Quat::IDENTITY),
        )
    };
    let (with, turned) = make(true);
    let (without, _) = make(false);
    assert!(
        (with - 0.1).abs() < 1e-3,
        "projected to 0.1 m from its anchor: {with}"
    );
    assert!(
        (turned - 5f32.to_radians()).abs() < 0.01,
        "the locked rotation is projected to 5° too: {turned}"
    );
    assert!(
        (without - 1.5).abs() < 1e-3,
        "no projection, no pull: {without}"
    );
    // The solver then closes the rest.
    let (mut scene, b) = {
        let mut scene = Scene::new("t");
        let b = body(&mut scene, "Link", Vec3::new(0.0, -1.0, 0.0), Vec3::ONE);
        scene.add(
            b,
            ConfigurableJoint {
                projection_mode: JointProjectionMode::PositionAndRotation,
                projection_distance: 0.1,
                ..locked()
            },
        );
        (scene, b)
    };
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    physics.teleport(b, Vec3::new(1.5, -1.0, 0.0), Quat::IDENTITY);
    run(&mut scene, &mut physics, 0.5);
    let left = (scene.world_position(b) - Vec3::new(0.0, -1.0, 0.0)).length();
    assert!(left < 0.01, "{left}");
}

#[test]
fn joint_components_round_trip_with_infinite_break_force() {
    let j = ConfigurableJoint {
        x_motion: JointMotion::Limited,
        break_force: 120.0,
        ..Default::default()
    };
    let json = serde_json::to_value(&j).unwrap();
    assert!(
        json["break_torque"].is_null(),
        "infinity is written as null"
    );
    let back: ConfigurableJoint = serde_json::from_value(json).unwrap();
    assert_eq!(back, j);
    let c: CharacterJoint = serde_json::from_str("{}").unwrap();
    assert!(c.break_force.is_infinite());
    let mut scene = Scene::new("t");
    let e = scene.spawn("Limb", None);
    assert!(scene.add_by_name(e, "Character Joint"));
    assert!(
        scene.world.get::<&Rigidbody>(e).is_ok(),
        "needs a Rigidbody"
    );
}

// ---- Physics Materials ------------------------------------------------------------------------

/// A material file in a temp folder (colliders take absolute paths too).
fn material_file(name: &str, m: &material::PhysicsMaterial) -> String {
    let dir = std::env::temp_dir().join(format!("misty_physmat_{}", std::process::id()));
    let path = dir.join(format!("{name}.physicmat"));
    m.save(&path).unwrap();
    path.to_string_lossy().into_owned()
}

/// A box on a 25° slope with a material, optionally pushed down the slope; how far it slid.
fn slide(box_mat: &str, ground_mat: &str, push: f32) -> f32 {
    let mut scene = Scene::new("t");
    let slope = scene.spawn("Slope", None);
    let tilt = Quat::from_rotation_z(25f32.to_radians());
    let mut t = Transform::from_position(Vec3::ZERO);
    t.set_rotation(tilt);
    t.scale = Vec3::new(20.0, 0.2, 4.0);
    scene.set_transform(slope, t);
    scene.add(
        slope,
        BoxCollider {
            material: Some(ground_mat.to_owned()),
            ..Default::default()
        },
    );
    let start = tilt * Vec3::new(0.0, 0.1 + 0.25, 0.0);
    let b = scene.spawn("Box", None);
    let mut t = Transform::from_position(start);
    t.set_rotation(tilt);
    t.scale = Vec3::splat(0.5);
    scene.set_transform(b, t);
    scene.add(
        b,
        BoxCollider {
            material: Some(box_mat.to_owned()),
            ..Default::default()
        },
    );
    scene.add(b, Rigidbody::default());
    let mut physics = Physics::build(&scene, G);
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    // Let it settle on the slope first.
    run(&mut scene, &mut physics, 0.3);
    let settled = scene.world_position(b);
    if push != 0.0 {
        physics.set_velocity(b, tilt * Vec3::new(-push, 0.0, 0.0));
    }
    run(&mut scene, &mut physics, 1.5);
    (scene.world_position(b) - settled).length()
}

#[test]
fn ice_slides_and_rubber_grips() {
    use material::{Combine, PhysicsMaterial};
    let ice = material_file(
        "ice",
        &PhysicsMaterial {
            dynamic_friction: 0.02,
            static_friction: 0.05,
            friction_combine: Combine::Minimum,
            ..Default::default()
        },
    );
    let rubber = material_file(
        "rubber",
        &PhysicsMaterial {
            dynamic_friction: 1.0,
            static_friction: 1.2,
            friction_combine: Combine::Maximum,
            ..Default::default()
        },
    );
    let plain = material_file("plain", &PhysicsMaterial::default());
    let on_ice = slide(&ice, &plain, 0.0);
    let on_rubber = slide(&rubber, &plain, 0.0);
    assert!(
        on_ice > 2.0,
        "ice (Minimum wins over Average) slides: {on_ice}"
    );
    assert!(
        on_rubber < 0.05,
        "rubber (Maximum) holds on 25°: {on_rubber}"
    );
}

#[test]
fn static_friction_holds_until_pushed() {
    use material::PhysicsMaterial;
    // tan 25° ≈ 0.47: static 0.6 holds, dynamic 0.3 can't stop a slide.
    let grippy = material_file(
        "grippy",
        &PhysicsMaterial {
            dynamic_friction: 0.3,
            static_friction: 0.6,
            ..Default::default()
        },
    );
    let at_rest = slide(&grippy, &grippy, 0.0);
    let pushed = slide(&grippy, &grippy, 2.0);
    assert!(at_rest < 0.05, "static friction holds it: {at_rest}");
    assert!(
        pushed > 1.0,
        "once sliding, dynamic friction can't stop it: {pushed}"
    );
}

#[test]
fn bounciness_combines_with_the_floor() {
    use material::{Combine, PhysicsMaterial};
    let drop = |ball_mat: &str| {
        let mut scene = Scene::new("t");
        let floor = scene.spawn("Floor", None);
        let mut t = Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
        t.scale = Vec3::new(10.0, 1.0, 10.0);
        scene.set_transform(floor, t);
        scene.add(floor, BoxCollider::default());
        let ball = scene.spawn("Ball", None);
        scene.set_transform(ball, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
        scene.add(
            ball,
            crate::scene::SphereCollider {
                material: Some(ball_mat.to_owned()),
                ..Default::default()
            },
        );
        scene.add(ball, Rigidbody::default());
        let mut physics = Physics::build(&scene, G);
        // Fall (≈ 0.55 s), then the highest point of the first bounce.
        let mut top: f32 = 0.0;
        let mut landed = false;
        for _ in 0..120 {
            physics.update(&mut scene, 1.0 / 60.0);
            let y = scene.world_position(ball).y;
            if y < 0.6 {
                landed = true;
            }
            if landed {
                top = top.max(y);
            }
        }
        top
    };
    let bouncy = material_file(
        "bouncy",
        &PhysicsMaterial {
            bounciness: 0.9,
            bounce_combine: Combine::Maximum,
            ..Default::default()
        },
    );
    let dead = material_file("dead", &PhysicsMaterial::default());
    let high = drop(&bouncy);
    let low = drop(&dead);
    assert!(
        high > 1.2,
        "bounces back up (Maximum with the floor's 0): {high}"
    );
    assert!(low < 0.7, "no bounce: {low}");
}

#[test]
fn a_missing_material_warns_and_keeps_the_surface() {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Box", None);
    scene.add(
        e,
        BoxCollider {
            material: Some("Assets/Nope.physicmat".into()),
            ..Default::default()
        },
    );
    let physics = Physics::build(&scene, G);
    assert_eq!(physics.warnings.len(), 1, "{:?}", physics.warnings);
    assert!(physics.warnings[0].contains("Nope.physicmat"));
}
