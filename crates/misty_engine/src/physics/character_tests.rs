//! Character Controller tests: grounding, walls, steps and slopes.

use super::*;
use crate::scene::{BoxCollider, CharacterController, Transform};

const DT: f32 = 1.0 / 60.0;

fn solid(scene: &mut Scene, name: &str, pos: Vec3, size: Vec3, roll: f32) -> Entity {
    let e = scene.spawn(name, None);
    let mut t = Transform::from_position(pos).with_euler(Vec3::new(0.0, 0.0, roll));
    t.scale = size;
    scene.set_transform(e, t);
    scene.add(e, BoxCollider::default());
    e
}

/// Ground slab with its top at y = 0 and a 2 m tall controller standing on it at `x`.
fn world(x: f32) -> (Scene, Entity) {
    let mut scene = Scene::new("t");
    solid(
        &mut scene,
        "Ground",
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(40.0, 1.0, 10.0),
        0.0,
    );
    let e = scene.spawn("Player", None);
    scene.set_transform(e, Transform::from_position(Vec3::new(x, 1.3, 0.0)));
    scene.add(e, CharacterController::default());
    (scene, e)
}

fn walk(physics: &mut Physics, scene: &mut Scene, e: Entity, speed: Vec3, seconds: f32) -> bool {
    let mut grounded = false;
    for _ in 0..(seconds / DT) as usize {
        grounded = physics.simple_move(scene, e, speed, DT);
        physics.step(scene);
    }
    grounded
}

#[test]
fn simple_move_falls_onto_the_ground_and_reports_grounded() {
    let (mut scene, e) = world(0.0);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.is_character(e));
    assert!(!physics.character_grounded(e));
    assert!(walk(&mut physics, &mut scene, e, Vec3::ZERO, 1.0));
    assert!(physics.character_grounded(e));
    let y = scene.world_position(e).y;
    assert!(
        (y - 1.0).abs() < 0.12,
        "feet on the ground (skin width gap): {y}"
    );
    // Walking moves it at the requested speed.
    let x0 = scene.world_position(e).x;
    walk(&mut physics, &mut scene, e, Vec3::new(2.0, 5.0, 0.0), 1.0);
    let x1 = scene.world_position(e).x;
    assert!((x1 - x0 - 2.0).abs() < 0.1, "{x0} → {x1}");
    let v = physics.character_velocity(e);
    assert!((v.x - 2.0).abs() < 0.1 && v.y.abs() < 0.5, "{v}");
}

#[test]
fn move_stops_at_walls_and_reports_the_hit() {
    let (mut scene, e) = world(0.0);
    let wall = solid(
        &mut scene,
        "Wall",
        Vec3::new(3.0, 1.5, 0.0),
        Vec3::new(1.0, 3.0, 4.0),
        0.0,
    );
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    walk(&mut physics, &mut scene, e, Vec3::ZERO, 0.5);
    physics.take_controller_hits();
    for _ in 0..60 {
        physics.move_character(&mut scene, e, Vec3::new(0.1, 0.0, 0.0), DT);
        physics.step(&scene);
    }
    let x = scene.world_position(e).x;
    assert!(
        x < 2.0 && x > 1.8,
        "stops at the wall face (2.5 − radius): {x}"
    );
    let hits = physics.take_controller_hits();
    let hit = hits.iter().find(|h| h.other == wall).expect("hit the wall");
    assert_eq!(hit.controller, e);
    assert!(hit.normal.x < -0.9, "{:?}", hit.normal);
    assert!((hit.point.x - 2.5).abs() < 0.1, "{:?}", hit.point);
}

#[test]
fn steps_lower_than_the_step_offset_are_climbed() {
    let climb = |height: f32| {
        let (mut scene, e) = world(0.0);
        solid(
            &mut scene,
            "Step",
            Vec3::new(3.0, height / 2.0, 0.0),
            Vec3::new(2.0, height, 4.0),
            0.0,
        );
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        walk(&mut physics, &mut scene, e, Vec3::ZERO, 0.5);
        walk(&mut physics, &mut scene, e, Vec3::new(2.0, 0.0, 0.0), 2.0);
        scene.world_position(e)
    };
    let low = climb(0.2);
    assert!(
        low.x > 2.5 && (low.y - 1.2).abs() < 0.12,
        "on top of the step: {low}"
    );
    let high = climb(0.6);
    assert!(
        high.x < 2.0 && high.y < 1.15,
        "blocked by a step above 0.3: {high}"
    );
}

#[test]
fn slopes_steeper_than_the_slope_limit_are_not_climbed() {
    let climb = |degrees: f32| {
        let (mut scene, e) = world(-2.0);
        // A long ramp rising towards +X, starting near x = 0.
        let len = 12.0;
        let a = degrees.to_radians();
        let centre = Vec3::new(a.cos() * len / 2.0, a.sin() * len / 2.0 - 0.25, 0.0);
        solid(
            &mut scene,
            "Ramp",
            centre,
            Vec3::new(len, 0.5, 4.0),
            degrees,
        );
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        walk(&mut physics, &mut scene, e, Vec3::ZERO, 0.5);
        walk(&mut physics, &mut scene, e, Vec3::new(2.0, 0.0, 0.0), 3.0);
        scene.world_position(e).y
    };
    let gentle = climb(25.0);
    assert!(gentle > 2.3, "walks up a 25° ramp: {gentle}");
    let steep = climb(60.0);
    assert!(steep < 1.8, "can't walk up a 60° ramp: {steep}");
}

#[test]
fn dynamic_bodies_land_on_a_controller() {
    let (mut scene, e) = world(0.0);
    let crate_e = solid(
        &mut scene,
        "Crate",
        Vec3::new(0.0, 4.0, 0.0),
        Vec3::splat(0.5),
        0.0,
    );
    scene.add(crate_e, Rigidbody::default());
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    walk(&mut physics, &mut scene, e, Vec3::ZERO, 2.0);
    physics.sync_out(&mut scene);
    let y = scene.world_position(crate_e).y;
    assert!(y > 2.0, "rests on the controller's head: {y}");
}
