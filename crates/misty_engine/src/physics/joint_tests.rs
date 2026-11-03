//! Fixed / Hinge / Spring Joint behaviour and joint serialisation.

use super::*;
use crate::scene::{
    BoxCollider, FixedJoint, HingeJoint, JointLimits, JointMotor, SpringJoint, Transform,
};

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

/// A 2 m bar hinged to the world at its left end (axis Z), swinging down under gravity.
fn bar(limits: Option<f32>) -> (Scene, Entity) {
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
        HingeJoint {
            anchor: Vec3::new(-0.5, 0.0, 0.0),
            axis: Vec3::Z,
            use_limits: limits.is_some(),
            limits: JointLimits {
                min: -limits.unwrap_or(0.0),
                max: limits.unwrap_or(0.0),
            },
            ..Default::default()
        },
    );
    (scene, bar)
}

#[test]
fn hinge_swings_around_its_anchor() {
    let (mut scene, bar) = bar(None);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    run(&mut scene, &mut physics, 2.0);
    // Swings below the anchor at the origin, 1 m from it, in the XY plane.
    let p = scene.world_position(bar);
    assert!(
        (p.length() - 1.0).abs() < 0.05 && p.y < -0.5 && p.z.abs() < 0.01,
        "{p}"
    );
    let anchor = scene
        .world_matrix(bar)
        .transform_point3(Vec3::new(-0.5, 0.0, 0.0));
    assert!(anchor.length() < 0.05, "{anchor}");
    assert!(
        angle_z(&scene, bar).abs() > 45.0,
        "swung past the 30° a limit would allow"
    );
}

#[test]
fn hinge_limits_stop_the_swing() {
    let (mut scene, bar) = bar(Some(30.0));
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    run(&mut scene, &mut physics, 3.0);
    let a = angle_z(&scene, bar);
    assert!(
        (a.abs() - 30.0).abs() < 3.0,
        "stopped at the 30° limit, got {a}"
    );
    // Still attached at the anchor.
    let anchor = scene
        .world_matrix(bar)
        .transform_point3(Vec3::new(-0.5, 0.0, 0.0));
    assert!(anchor.length() < 0.05, "{anchor}");
}

#[test]
fn hinge_motor_spins_at_its_target_velocity() {
    let mut scene = Scene::new("t");
    let wheel = body(&mut scene, "Wheel", Vec3::ZERO, Vec3::ONE);
    scene.add(
        wheel,
        HingeJoint {
            axis: Vec3::Y,
            use_motor: true,
            motor: JointMotor {
                target_velocity: 180.0,
                force: 100.0,
            },
            ..Default::default()
        },
    );
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    run(&mut scene, &mut physics, 1.0);
    let w = physics.angular_velocity(wheel).unwrap();
    assert!((w.y.abs() - std::f32::consts::PI).abs() < 0.2, "{w}");
    assert!(
        w.x.abs() < 0.05 && w.z.abs() < 0.05,
        "only around the axis: {w}"
    );
}

#[test]
fn fixed_joint_keeps_bodies_together() {
    let mut scene = Scene::new("t");
    let a = body(&mut scene, "A", Vec3::ZERO, Vec3::ONE);
    let b = body(&mut scene, "B", Vec3::new(2.0, 0.0, 0.0), Vec3::ONE);
    scene.add(
        b,
        FixedJoint {
            connected_body: Some(a),
            ..Default::default()
        },
    );
    // A third body glued to the world doesn't fall.
    let pinned = body(&mut scene, "Pinned", Vec3::new(0.0, 5.0, 0.0), Vec3::ONE);
    scene.add(pinned, FixedJoint::default());
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    run(&mut scene, &mut physics, 1.0);
    let (pa, pb) = (scene.world_position(a), scene.world_position(b));
    assert!(pa.y < -3.0, "A falls freely: {pa}");
    assert!(
        (pb - pa - Vec3::new(2.0, 0.0, 0.0)).length() < 0.05,
        "{pa} {pb}"
    );
    assert!((scene.world_position(pinned).y - 5.0).abs() < 0.05);
}

#[test]
fn spring_joint_settles_at_its_stretch_and_respects_max_distance() {
    let hang = |max_distance: f32| {
        let mut scene = Scene::new("t");
        let e = body(&mut scene, "Weight", Vec3::new(0.0, 5.0, 0.0), Vec3::ONE);
        scene.world.get::<&mut Rigidbody>(e).unwrap().drag = 1.0;
        scene.add(
            e,
            SpringJoint {
                spring: 50.0,
                damper: 5.0,
                max_distance,
                ..Default::default()
            },
        );
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        run(&mut scene, &mut physics, 8.0);
        scene.world_position(e).y
    };
    // Weight force m·g = 9.81 N stretches a 50 N/m spring by ~0.196 m.
    let y = hang(0.0);
    assert!((y - (5.0 - 0.196)).abs() < 0.03, "{y}");
    // No force within the first metre.
    let y = hang(1.0);
    assert!((y - (5.0 - 1.196)).abs() < 0.03, "{y}");
}

#[test]
fn spring_chain_connects_bodies() {
    let mut scene = Scene::new("t");
    let top = body(
        &mut scene,
        "Top",
        Vec3::new(0.0, 5.0, 0.0),
        Vec3::splat(0.3),
    );
    scene.add(top, FixedJoint::default());
    let link = body(
        &mut scene,
        "Link",
        Vec3::new(0.0, 4.0, 0.0),
        Vec3::splat(0.3),
    );
    scene.add(
        link,
        SpringJoint {
            connected_body: Some(top),
            spring: 100.0,
            damper: 2.0,
            min_distance: 0.0,
            max_distance: 0.0,
            ..Default::default()
        },
    );
    scene.world.get::<&mut Rigidbody>(link).unwrap().drag = 1.0;
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    run(&mut scene, &mut physics, 6.0);
    let y = scene.world_position(link).y;
    assert!((y - (4.0 - 0.098)).abs() < 0.03, "hangs below the top: {y}");
    physics.remove(top);
    run(&mut scene, &mut physics, 1.0);
    let y = scene.world_position(link).y;
    assert!(y < 1.0, "falls (with drag) once its anchor is gone: {y}");
}

#[test]
fn joints_save_and_load_with_their_connected_body() {
    let mut scene = Scene::new("t");
    let frame = body(&mut scene, "Frame", Vec3::ZERO, Vec3::ONE);
    let door = body(&mut scene, "Door", Vec3::X, Vec3::ONE);
    let hinge = HingeJoint {
        connected_body: Some(frame),
        anchor: Vec3::new(-0.5, 0.0, 0.0),
        axis: Vec3::Y,
        use_limits: true,
        limits: JointLimits {
            min: -90.0,
            max: 10.0,
        },
        ..Default::default()
    };
    scene.add(door, hinge.clone());
    scene.add(
        door,
        SpringJoint {
            connected_body: Some(frame),
            ..Default::default()
        },
    );
    scene.add(frame, FixedJoint::default());
    scene.add(frame, crate::scene::CharacterController::default());
    let (loaded, report) = Scene::from_json("t", &scene.to_json()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let (frame2, door2) = (
        loaded.find_by_name("Frame").unwrap(),
        loaded.find_by_name("Door").unwrap(),
    );
    let h = loaded.world.get::<&HingeJoint>(door2).unwrap();
    assert_eq!(h.connected_body, Some(frame2));
    assert_eq!(
        HingeJoint {
            connected_body: hinge.connected_body.map(|_| frame2),
            ..hinge
        },
        *h
    );
    let s = loaded.world.get::<&SpringJoint>(door2).unwrap();
    assert_eq!(s.connected_body, Some(frame2));
    assert_eq!(
        loaded
            .world
            .get::<&FixedJoint>(frame2)
            .unwrap()
            .connected_body,
        None
    );
    assert_eq!(
        *loaded
            .world
            .get::<&crate::scene::CharacterController>(frame2)
            .unwrap(),
        crate::scene::CharacterController::default()
    );
}

#[test]
fn adding_a_joint_adds_a_rigidbody() {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    assert!(scene.add_by_name(e, "Hinge Joint"));
    assert!(scene.world.get::<&Rigidbody>(e).is_ok());
    let names = scene.component_names(e);
    let rb = names.iter().position(|n| n == "Rigidbody").unwrap();
    let joint = names.iter().position(|n| n == "Hinge Joint").unwrap();
    assert!(rb < joint, "{names:?}");
}
