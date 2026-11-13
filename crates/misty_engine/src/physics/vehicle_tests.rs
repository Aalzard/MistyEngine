//! Wheel Collider behaviour: suspension at rest, driving, steering, braking, saving.

use super::*;
use crate::scene::{BoxCollider, Transform, WheelCollider};

const WHEELS: [(&str, f32, f32); 4] = [
    ("Wheel FL", -0.8, 1.3),
    ("Wheel FR", 0.8, 1.3),
    ("Wheel RL", -0.8, -1.3),
    ("Wheel RR", 0.8, -1.3),
];

/// A 1200 kg car on flat ground: box body above four wheels (radius 0.35, 0.3 m travel,
/// resting at half travel), so the car's origin rests 0.5 m above the ground.
fn car_scene(y: f32) -> (Scene, Entity, Vec<Entity>) {
    let mut scene = Scene::new("t");
    let ground = scene.spawn("Ground", None);
    let mut t = Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
    t.scale = Vec3::new(400.0, 1.0, 400.0);
    scene.set_transform(ground, t);
    scene.add(ground, BoxCollider::default());

    let car = scene.spawn("Car", None);
    scene.set_transform(car, Transform::from_position(Vec3::new(0.0, y, 0.0)));
    scene.add(
        car,
        BoxCollider {
            center: Vec3::new(0.0, 0.5, 0.0),
            size: Vec3::new(1.8, 0.6, 4.0),
            ..Default::default()
        },
    );
    scene.add(
        car,
        Rigidbody {
            mass: 1200.0,
            ..Default::default()
        },
    );
    let wheels = WHEELS
        .iter()
        .map(|(name, x, z)| {
            let w = scene.spawn(name, Some(car));
            scene.set_transform(w, Transform::from_position(Vec3::new(*x, 0.0, *z)));
            scene.add(
                w,
                WheelCollider {
                    radius: 0.35,
                    ..Default::default()
                },
            );
            w
        })
        .collect();
    (scene, car, wheels)
}

fn run(scene: &mut Scene, physics: &mut Physics, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        physics.update(scene, 1.0 / 60.0);
    }
}

fn set(scene: &mut Scene, wheels: &[Entity], f: impl Fn(usize, &mut WheelCollider)) {
    for (i, w) in wheels.iter().enumerate() {
        f(i, &mut scene.world.get::<&mut WheelCollider>(*w).unwrap());
    }
}

fn build(scene: &Scene) -> Physics {
    let physics = Physics::build(scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    physics
}

/// Heading of the car around +Y in degrees (0 = +Z, positive towards +X).
fn yaw(scene: &Scene, car: Entity) -> f32 {
    let f = scene.world_rotation(car) * Vec3::Z;
    f.x.atan2(f.z).to_degrees()
}

#[test]
fn wheels_rest_at_the_spring_target() {
    let (mut scene, car, wheels) = car_scene(0.6);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 4.0);
    for w in &wheels {
        let s = scene.world.get::<&WheelCollider>(*w).unwrap().state;
        assert!(s.is_grounded && s.simulated, "{s:?}");
        assert!((s.extension - 0.15).abs() < 0.02, "half travel: {s:?}");
        let hit = s.hit.unwrap();
        assert!(hit.point.y.abs() < 0.01, "touches the ground: {hit:?}");
        // Each wheel holds a quarter of the weight.
        assert!((hit.force - 1200.0 * 9.81 / 4.0).abs() < 300.0, "{hit:?}");
        assert!(s.rpm.abs() < 1.0, "{s:?}");
    }
    let p = scene.world_position(car);
    assert!((p.y - 0.5).abs() < 0.03, "rests on its wheels: {p}");
    // Not creeping (rapier's velocity after a step keeps some of the step's gravity).
    let v = physics.velocity(car).unwrap();
    assert!(Vec3::new(v.x, 0.0, v.z).length() < 0.02, "{v}");
    run(&mut scene, &mut physics, 2.0);
    assert!((scene.world_position(car) - p).length() < 0.02, "still");
}

#[test]
fn wheels_in_the_air_hang_at_full_extension() {
    let (mut scene, _, wheels) = car_scene(5.0);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 0.1);
    let s = physics.wheel_state(wheels[0]).unwrap();
    assert!(!s.is_grounded && (s.extension - 0.3).abs() < 1e-5, "{s:?}");
    // The wheel pose hangs below the wheel object.
    let (p, _) = wheel_world_pose(&scene, wheels[0]).unwrap();
    let top = scene.world_position(wheels[0]);
    assert!((top.y - p.y - 0.3).abs() < 0.01, "{p} {top}");
}

#[test]
fn motor_torque_drives_the_car_forward() {
    let (mut scene, car, wheels) = car_scene(0.5);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 1.0);
    // Rear-wheel drive.
    set(&mut scene, &wheels, |i, w| {
        if i >= 2 {
            w.motor_torque = 600.0;
        }
    });
    run(&mut scene, &mut physics, 3.0);
    let p = scene.world_position(car);
    let v = physics.velocity(car).unwrap();
    assert!(p.z > 3.0 && v.z > 2.0, "drove forward: {p} {v}");
    assert!(
        p.x.abs() < 0.3 && yaw(&scene, car).abs() < 3.0,
        "straight: {p}"
    );
    let s = scene.world.get::<&WheelCollider>(wheels[0]).unwrap().state;
    // Front wheels roll with the car: rpm matches the ground speed.
    let rolling = s.rpm / 60.0 * std::f32::consts::TAU * 0.35;
    assert!(
        (rolling - v.z).abs() < 0.5,
        "rolls without slipping: {s:?} {v}"
    );
    // Reverse.
    set(&mut scene, &wheels, |i, w| {
        if i >= 2 {
            w.motor_torque = -600.0;
        }
    });
    run(&mut scene, &mut physics, 7.0);
    assert!(physics.velocity(car).unwrap().z < -0.5, "reverses");
}

#[test]
fn steering_turns_the_car() {
    let (mut scene, car, wheels) = car_scene(0.5);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 1.0);
    set(&mut scene, &wheels, |i, w| {
        if i < 2 {
            w.steer_angle = 20.0;
        } else {
            w.motor_torque = 400.0;
        }
    });
    run(&mut scene, &mut physics, 4.0);
    let p = scene.world_position(car);
    let heading = yaw(&scene, car);
    assert!(
        heading > 20.0,
        "turned right (towards +X): {heading} at {p}"
    );
    assert!(p.x > 1.0, "{p}");
    let up = scene.world_rotation(car) * Vec3::Y;
    assert!(up.y > 0.95, "stays upright: {up}");
}

#[test]
fn brakes_stop_the_car() {
    let (mut scene, car, wheels) = car_scene(0.5);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 1.0);
    set(&mut scene, &wheels, |_, w| w.motor_torque = 500.0);
    run(&mut scene, &mut physics, 3.0);
    let fast = physics.velocity(car).unwrap().z;
    assert!(fast > 4.0, "{fast}");
    set(&mut scene, &wheels, |_, w| {
        w.motor_torque = 0.0;
        w.brake_torque = 3000.0;
    });
    run(&mut scene, &mut physics, 4.0);
    let v = physics.velocity(car).unwrap();
    assert!(Vec3::new(v.x, 0.0, v.z).length() < 0.05, "stopped: {v}");
    let s = physics.wheel_state(wheels[0]).unwrap();
    assert!(s.rpm.abs() < 1.0, "{s:?}");
}

#[test]
fn coasting_without_brakes_keeps_rolling() {
    let (mut scene, car, wheels) = car_scene(0.5);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 1.0);
    set(&mut scene, &wheels, |_, w| w.motor_torque = 500.0);
    run(&mut scene, &mut physics, 2.0);
    set(&mut scene, &wheels, |_, w| w.motor_torque = 0.0);
    let before = physics.velocity(car).unwrap().z;
    run(&mut scene, &mut physics, 1.0);
    let after = physics.velocity(car).unwrap().z;
    assert!(
        after > before * 0.8 && after < before + 0.01,
        "{before} → {after}"
    );
}

#[test]
fn wheel_models_follow_the_wheels() {
    let (mut scene, _, wheels) = car_scene(0.5);
    let model = scene.spawn("Tyre", None);
    scene
        .world
        .get::<&mut WheelCollider>(wheels[0])
        .unwrap()
        .wheel_model = Some(model);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 1.0);
    let (p, r) = wheel_world_pose(&scene, wheels[0]).unwrap();
    assert!((scene.world_position(model) - p).length() < 1e-4);
    assert!(scene.world_rotation(model).dot(r).abs() > 0.9999);
    assert!((p.y - 0.35).abs() < 0.03, "wheel centre one radius up: {p}");
}

#[test]
fn a_wheel_without_a_rigidbody_warns() {
    let mut scene = Scene::new("t");
    let w = scene.spawn("Loose Wheel", None);
    scene.add(w, WheelCollider::default());
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings[0].contains("Wheel Collider needs a Rigidbody"));
}

#[test]
fn wheel_colliders_save_and_load() {
    let (mut scene, car, wheels) = car_scene(0.5);
    let model = scene.spawn("Tyre", Some(car));
    {
        let mut w = scene.world.get::<&mut WheelCollider>(wheels[1]).unwrap();
        w.wheel_model = Some(model);
        w.steer_angle = 12.5;
        w.forward_friction.stiffness = 1.5;
        w.suspension_spring.target_position = 0.3;
    }
    let (loaded, report) = Scene::from_json("t", &scene.to_json()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let w = loaded.find_by_name("Wheel FR").unwrap();
    let c = loaded.world.get::<&WheelCollider>(w).unwrap();
    assert_eq!(c.wheel_model, loaded.find_by_name("Tyre"));
    assert_eq!(c.steer_angle, 12.5);
    assert_eq!(c.forward_friction.stiffness, 1.5);
    assert_eq!(c.suspension_spring.target_position, 0.3);
    assert_eq!(c.radius, 0.35);
    assert_eq!(loaded.to_json(), scene.to_json());
}

/// Gravity and mass changed while playing (the Racing template's settings): the wheels carry
/// the new weight, and the car still rests at the spring target.
#[test]
fn gravity_and_mass_change_while_playing() {
    let (mut scene, car, wheels) = car_scene(0.6);
    let mut physics = build(&scene);
    run(&mut scene, &mut physics, 3.0);
    let load = |scene: &Scene| {
        wheels
            .iter()
            .map(|w| {
                let s = scene.world.get::<&WheelCollider>(*w).unwrap().state;
                s.hit.map_or(0.0, |h| h.force)
            })
            .sum::<f32>()
    };
    assert!((load(&scene) - 1200.0 * 9.81).abs() < 600.0);
    physics.set_gravity(Vec3::new(0.0, -3.0, 0.0));
    assert_eq!(physics.gravity(), Vec3::new(0.0, -3.0, 0.0));
    run(&mut scene, &mut physics, 3.0);
    assert!(
        (load(&scene) - 1200.0 * 3.0).abs() < 300.0,
        "{}",
        load(&scene)
    );
    physics.set_mass(car, 2400.0);
    run(&mut scene, &mut physics, 3.0);
    assert!(
        (load(&scene) - 2400.0 * 3.0).abs() < 400.0,
        "{}",
        load(&scene)
    );
    let p = scene.world_position(car);
    assert!((p.y - 0.5).abs() < 0.05, "rests on its wheels: {p}");
}

/// A NaN Motor Torque or Steer Angle (set from a script) made the tyre solver's
/// `clamp(-NaN, NaN)` panic on the next physics step.
#[test]
fn nan_torque_or_steering_does_not_panic() {
    for (torque, steer) in [(f32::NAN, 0.0), (0.0, f32::NAN), (1e38, 0.0)] {
        let (mut scene, car, wheels) = car_scene(0.5);
        let mut physics = build(&scene);
        run(&mut scene, &mut physics, 0.5);
        set(&mut scene, &wheels, |_, w| {
            w.motor_torque = torque;
            w.steer_angle = steer;
        });
        run(&mut scene, &mut physics, 0.2);
        let _ = scene.world_position(car);
    }
}
