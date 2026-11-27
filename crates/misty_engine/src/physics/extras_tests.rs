//! IgnoreCollision, IgnoreLayerCollision, forces at points, explosions, sleep, ClosestPoint.

use super::*;
use crate::scene::{BoxCollider, Transform};

fn cube(scene: &mut Scene, name: &str, pos: Vec3, body: bool) -> Entity {
    let e = scene.spawn(name, None);
    scene.set_transform(e, Transform::from_position(pos));
    scene.add(e, BoxCollider::default());
    if body {
        scene.add(e, Rigidbody::default());
    }
    e
}

fn floor(scene: &mut Scene) -> Entity {
    let f = cube(scene, "Floor", Vec3::ZERO, false);
    let mut t = scene.transform(f);
    t.scale = Vec3::new(20.0, 1.0, 20.0);
    scene.set_transform(f, t);
    f
}

fn steps(physics: &mut Physics, scene: &mut Scene, n: u32) {
    for _ in 0..n {
        let dt = physics.fixed_dt;
        physics.update(scene, dt);
    }
}

#[test]
fn ignored_pairs_and_layers_fall_through() {
    let mut scene = Scene::new("t");
    let f = floor(&mut scene);
    let a = cube(&mut scene, "A", Vec3::new(-2.0, 2.0, 0.0), true);
    let b = cube(&mut scene, "B", Vec3::new(2.0, 2.0, 0.0), true);
    scene.game_object_mut(b).layer = 3;
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    physics.ignore_collision(a, f, true);
    assert!(physics.collision_ignored(f, a));
    physics.ignore_layer_collision(0, 3, true);
    assert!(physics.layer_collision_ignored(3, 0));
    steps(&mut physics, &mut scene, 60);
    assert!(scene.world_position(a).y < -1.0, "A fell through the floor");
    assert!(scene.world_position(b).y < -1.0, "B (layer 3) fell through");

    // Undone: a new cube rests again.
    let mut scene = Scene::new("t");
    let f = floor(&mut scene);
    let a = cube(&mut scene, "A", Vec3::new(0.0, 2.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    physics.ignore_collision(a, f, true);
    physics.ignore_collision(a, f, false);
    steps(&mut physics, &mut scene, 120);
    assert!((scene.world_position(a).y - 1.0).abs() < 0.05);
}

#[test]
fn forces_at_points_explosions_sleep_and_closest_point() {
    let mut scene = Scene::new("t");
    let a = cube(&mut scene, "A", Vec3::new(0.0, 5.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    // Pushed sideways at its top edge: it moves and turns.
    physics.add_impulse_at_position(a, Vec3::X, Vec3::new(0.0, 5.5, 0.0));
    assert!(physics.velocity(a).unwrap().x > 0.5);
    assert!(physics.angular_velocity(a).unwrap().length() > 0.1);

    // An explosion 2 m below: pushed up, half strength at half the radius.
    let push = physics
        .explosion_push(a, 10.0, Vec3::new(0.0, 3.0, 0.0), 4.0, 0.0)
        .unwrap();
    assert!((push - Vec3::Y * 5.0).length() < 1e-3, "{push}");
    assert!(
        physics
            .explosion_push(a, 10.0, Vec3::new(0.0, -5.0, 0.0), 4.0, 0.0)
            .is_none()
    );

    physics.set_sleeping(a, true);
    assert!(physics.is_sleeping(a));
    physics.set_sleeping(a, false);
    assert!(!physics.is_sleeping(a));

    let p = physics.closest_point(a, Vec3::new(3.0, 5.0, 0.0)).unwrap();
    assert!((p - Vec3::new(0.5, 5.0, 0.0)).length() < 1e-3, "{p}");
    let inside = Vec3::new(0.1, 5.1, 0.0);
    assert_eq!(physics.closest_point(a, inside), Some(inside));
}

#[test]
fn script_changed_constraints_and_drag_reach_the_body() {
    let mut scene = Scene::new("t");
    let e = cube(&mut scene, "Box", Vec3::new(0.0, 3.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut rb = Rigidbody::default();
    rb.set_constraints(4); // FreezePositionY: gravity can't pull it down.
    rb.drag = 1.0;
    physics.apply_body_settings(e, &rb);
    for _ in 0..10 {
        physics.update(&mut scene, physics.fixed_dt);
    }
    assert!((scene.world_position(e).y - 3.0).abs() < 1e-4);
    rb.set_constraints(0);
    physics.apply_body_settings(e, &rb);
    for _ in 0..10 {
        physics.update(&mut scene, physics.fixed_dt);
    }
    assert!(scene.world_position(e).y < 2.99);
    assert_eq!(rb.constraints(), 0);
}
