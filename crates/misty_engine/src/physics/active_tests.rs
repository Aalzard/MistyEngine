//! Objects switched off or on while playing (`SetActive`, Timeline activation tracks, animated
//! Active properties): their bodies and colliders leave and rejoin the physics world.

use super::*;
use crate::scene::{BoxCollider, MeshFilter, PrimitiveMesh, Transform};

const G: Vec3 = Vec3::new(0.0, -9.81, 0.0);

fn cube(scene: &mut Scene, name: &str, pos: Vec3, rigidbody: bool) -> Entity {
    let e = scene.spawn(name, None);
    scene.set_transform(e, Transform::from_position(pos));
    scene.add(
        e,
        MeshFilter {
            mesh: Some(PrimitiveMesh::Cube.into()),
        },
    );
    scene.add(e, BoxCollider::default());
    if rigidbody {
        scene.add(e, Rigidbody::default());
    }
    e
}

fn run(physics: &mut Physics, scene: &mut Scene, steps: usize) -> Vec<Contact> {
    let mut contacts = Vec::new();
    for _ in 0..steps {
        contacts.extend(physics.step(scene));
        physics.sync_out(scene);
    }
    contacts
}

/// A floor switched off while playing let nothing through: its static collider stayed in the
/// physics world, so the cube kept resting on an invisible floor.
#[test]
fn a_floor_switched_off_while_playing_lets_the_cube_fall() {
    let mut scene = Scene::new("t");
    let floor = cube(&mut scene, "Floor", Vec3::ZERO, false);
    let c = cube(&mut scene, "Cube", Vec3::new(0.0, 1.5, 0.0), true);
    let mut physics = Physics::build(&scene, G);
    run(&mut physics, &mut scene, 90);
    assert!((scene.world_position(c).y - 1.0).abs() < 0.05);
    scene.game_object_mut(floor).active = false;
    run(&mut physics, &mut scene, 60);
    let y = scene.world_position(c).y;
    assert!(y < 0.0, "the cube falls through the switched-off floor, y = {y}");
    assert!(
        physics
            .raycast(Vec3::new(-5.0, 0.0, 0.0), Vec3::X, 10.0, ALL_LAYERS)
            .is_none(),
        "raycasts miss a switched-off object"
    );
}

/// A Rigidbody that starts inactive and is switched on later got no body at all: it hung in
/// the air instead of falling.
#[test]
fn a_body_switched_on_while_playing_falls() {
    let mut scene = Scene::new("t");
    cube(&mut scene, "Floor", Vec3::ZERO, false);
    let c = cube(&mut scene, "Cube", Vec3::new(0.0, 4.0, 0.0), true);
    scene.game_object_mut(c).active = false;
    let mut physics = Physics::build(&scene, G);
    assert!(!physics.has_body(c));
    run(&mut physics, &mut scene, 30);
    assert_eq!(scene.world_position(c).y, 4.0, "inactive objects don't move");
    scene.game_object_mut(c).active = true;
    run(&mut physics, &mut scene, 120);
    assert!(physics.has_body(c));
    let y = scene.world_position(c).y;
    assert!((y - 1.0).abs() < 0.05, "the cube falls and lands, y = {y}");
}

/// Switched off and on again (a parent this time): the children's colliders come back, attached
/// to the parent's body, and a trigger fires its enter again.
#[test]
fn a_parent_switched_off_and_on_brings_its_children_back() {
    let mut scene = Scene::new("t");
    cube(&mut scene, "Floor", Vec3::ZERO, false);
    let zone = cube(&mut scene, "Zone", Vec3::new(0.0, 1.0, 0.0), false);
    scene
        .world
        .get::<&mut BoxCollider>(zone)
        .unwrap()
        .is_trigger = true;
    let parent = scene.spawn("Player", None);
    scene.set_transform(parent, Transform::from_position(Vec3::new(0.0, 1.0, 0.0)));
    scene.add(parent, Rigidbody::default());
    let body = scene.spawn("Body", Some(parent));
    scene.add(body, BoxCollider::default());
    let mut physics = Physics::build(&scene, G);
    let colliders = physics.collider_count();
    let first = run(&mut physics, &mut scene, 30);
    let enters = |cs: &[Contact]| {
        cs.iter()
            .filter(|k| k.trigger && k.started && (k.a == zone || k.b == zone))
            .count()
    };
    assert_eq!(enters(&first), 1);

    scene.game_object_mut(parent).active = false;
    run(&mut physics, &mut scene, 5);
    assert!(!physics.has_body(parent));
    assert_eq!(physics.collider_count(), colliders - 1, "the child's collider left too");

    scene.game_object_mut(parent).active = true;
    let again = run(&mut physics, &mut scene, 30);
    assert!(physics.has_body(parent));
    assert_eq!(physics.collider_count(), colliders, "no duplicate colliders");
    assert_eq!(enters(&again), 1, "the trigger fires again: {again:?}");
    let y = scene.world_position(parent).y;
    assert!((y - 1.0).abs() < 0.05, "the child collider rests on the floor, y = {y}");

    // Switching an active object on again changes nothing.
    scene.game_object_mut(parent).active = true;
    run(&mut physics, &mut scene, 2);
    assert_eq!(physics.collider_count(), colliders);
}
