//! Colliders without their own body follow their Transform (see `follow`).

use super::*;
use crate::scene::{BoxCollider, PrimitiveMesh, Transform};

fn box_at(scene: &mut Scene, name: &str, pos: Vec3, parent: Option<Entity>) -> Entity {
    let e = scene.spawn(name, parent);
    scene.set_transform(e, Transform::from_position(pos));
    scene.add(e, crate::scene::MeshFilter::primitive(PrimitiveMesh::Cube));
    scene.add(e, BoxCollider::default());
    e
}

/// `steps` fixed steps (one per update).
fn step(physics: &mut Physics, scene: &mut Scene, steps: u32) {
    for _ in 0..steps {
        let dt = physics.fixed_dt;
        physics.update(scene, dt);
    }
}

#[test]
fn a_static_collider_moved_by_a_script_moves_in_physics() {
    let mut scene = Scene::new("t");
    let wall = box_at(&mut scene, "Wall", Vec3::new(0.0, 0.0, 5.0), None);
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    step(&mut physics, &mut scene, 1);
    scene.set_world_position(wall, Vec3::new(0.0, 0.0, 8.0));
    step(&mut physics, &mut scene, 1);
    let hit = physics
        .raycast(Vec3::ZERO, Vec3::Z, 100.0, ALL_LAYERS)
        .expect("hits the wall");
    assert_eq!(hit.entity, wall);
    assert!((hit.distance - 7.5).abs() < 1e-3, "{}", hit.distance);
}

#[test]
fn a_child_of_a_moved_static_parent_follows_too() {
    let mut scene = Scene::new("t");
    let door = scene.spawn("Door", None);
    let panel = box_at(&mut scene, "Panel", Vec3::new(0.0, 0.0, 5.0), Some(door));
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    let mut t = scene.transform(door);
    t.position = Vec3::new(10.0, 0.0, 0.0);
    scene.set_transform(door, t);
    step(&mut physics, &mut scene, 1);
    assert!(
        physics
            .raycast(Vec3::ZERO, Vec3::Z, 100.0, ALL_LAYERS)
            .is_none(),
        "the old place is empty"
    );
    let hit = physics
        .raycast(Vec3::new(10.0, 0.0, 0.0), Vec3::Z, 100.0, ALL_LAYERS)
        .expect("hits the moved panel");
    assert_eq!(hit.entity, panel);
}

#[test]
fn a_box_falls_when_its_static_floor_is_moved_away() {
    let mut scene = Scene::new("t");
    let floor = box_at(&mut scene, "Floor", Vec3::ZERO, None);
    let mut t = scene.transform(floor);
    t.scale = Vec3::new(10.0, 1.0, 10.0);
    scene.set_transform(floor, t);
    let cube = box_at(&mut scene, "Cube", Vec3::new(0.0, 1.0, 0.0), None);
    scene.add(cube, Rigidbody::default());
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    // Long enough for the cube to come to rest (and fall asleep).
    step(&mut physics, &mut scene, 240);
    let rest = scene.world_position(cube).y;
    assert!((rest - 1.0).abs() < 0.05, "rests on the floor, y = {rest}");
    scene.set_world_position(floor, Vec3::new(0.0, -50.0, 0.0));
    step(&mut physics, &mut scene, 30);
    let y = scene.world_position(cube).y;
    assert!(y < rest - 1.0, "falls once the floor is gone, y = {y}");
}

#[test]
fn a_child_collider_moved_inside_its_body_moves_with_it() {
    let mut scene = Scene::new("t");
    let root = scene.spawn("Body", None);
    scene.add(
        root,
        Rigidbody {
            is_kinematic: true,
            ..Default::default()
        },
    );
    let arm = box_at(&mut scene, "Arm", Vec3::new(0.0, 0.0, 5.0), Some(root));
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    let mut t = scene.transform(arm);
    t.position = Vec3::new(0.0, 0.0, 9.0);
    scene.set_transform(arm, t);
    step(&mut physics, &mut scene, 1);
    let hit = physics
        .raycast(Vec3::ZERO, Vec3::Z, 100.0, ALL_LAYERS)
        .expect("hits the arm");
    assert_eq!(hit.entity, arm);
    assert!((hit.distance - 8.5).abs() < 1e-3, "{}", hit.distance);
}

#[test]
fn a_rescaled_static_collider_changes_size() {
    let mut scene = Scene::new("t");
    let wall = box_at(&mut scene, "Wall", Vec3::new(0.0, 0.0, 5.0), None);
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    let mut t = scene.transform(wall);
    t.scale = Vec3::splat(4.0);
    scene.set_transform(wall, t);
    step(&mut physics, &mut scene, 1);
    let hit = physics
        .raycast(Vec3::ZERO, Vec3::Z, 100.0, ALL_LAYERS)
        .expect("hits the wall");
    assert!((hit.distance - 3.0).abs() < 1e-3, "{}", hit.distance);
}
