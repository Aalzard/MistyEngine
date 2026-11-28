//! Inspector edits while playing: Rigidbody and collider values reach the live world.

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

/// Steps with their contact events.
fn steps(physics: &mut Physics, scene: &mut Scene, n: u32) -> Vec<Contact> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(physics.step(scene));
        physics.sync_out(scene);
    }
    all
}

#[test]
fn collider_edits_while_playing_change_the_live_collider() {
    let mut scene = Scene::new("t");
    let f = floor(&mut scene);
    let a = cube(&mut scene, "A", Vec3::new(0.0, 2.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    steps(&mut physics, &mut scene, 120);
    assert!((scene.world_position(a).y - 1.0).abs() < 0.05);

    // A bigger box: it rests higher.
    scene.world.get::<&mut BoxCollider>(a).unwrap().size = Vec3::splat(2.0);
    physics.refresh_object(&scene, a);
    steps(&mut physics, &mut scene, 120);
    assert!(
        (scene.world_position(a).y - 1.5).abs() < 0.05,
        "{}",
        scene.world_position(a)
    );

    // The floor becomes a trigger: the box falls through (no rebuild, so no exit-enter churn
    // on the box's own contacts while a slider is dragged).
    scene.world.get::<&mut BoxCollider>(f).unwrap().is_trigger = true;
    physics.refresh_object(&scene, f);
    steps(&mut physics, &mut scene, 60);
    assert!(
        scene.world_position(a).y < 0.0,
        "fell through the trigger floor"
    );
}

#[test]
fn switching_a_collider_off_while_playing_ends_its_contacts() {
    let mut scene = Scene::new("t");
    let f = floor(&mut scene);
    let a = cube(&mut scene, "A", Vec3::new(0.0, 1.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let began = steps(&mut physics, &mut scene, 30);
    assert!(began.iter().any(|c| c.started && !c.trigger));
    scene.world.get::<&mut BoxCollider>(f).unwrap().enabled = false;
    physics.refresh_object(&scene, f);
    let ended = steps(&mut physics, &mut scene, 30);
    assert!(
        ended.iter().any(|c| !c.started && (c.a == f || c.b == f)),
        "{ended:?}"
    );
    assert!(scene.world_position(a).y < 0.5);
    // Back on: it is a collider again.
    scene.world.get::<&mut BoxCollider>(f).unwrap().enabled = true;
    physics.refresh_object(&scene, f);
    assert_eq!(physics.colliders_of.get(&f).map(Vec::len), Some(1));
}

#[test]
fn rigidbody_edits_while_playing_reach_the_body() {
    let mut scene = Scene::new("t");
    let a = cube(&mut scene, "A", Vec3::new(0.0, 5.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    {
        let mut rb = scene.world.get::<&mut Rigidbody>(a).unwrap();
        rb.use_gravity = false;
        rb.mass = 4.0;
        rb.drag = 3.0;
    }
    physics.refresh_object(&scene, a);
    physics.set_velocity(a, Vec3::X);
    steps(&mut physics, &mut scene, 30);
    assert!(
        (scene.world_position(a).y - 5.0).abs() < 1e-3,
        "gravity off"
    );
    assert!(physics.velocity(a).unwrap().x < 0.5, "drag slows it");
    assert!((physics.mass(a).unwrap() - 4.0).abs() < 1e-3);
    scene.world.get::<&mut Rigidbody>(a).unwrap().is_kinematic = true;
    physics.refresh_object(&scene, a);
    assert!(physics.body(a).unwrap().is_kinematic());
}
