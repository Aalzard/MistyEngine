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
    assert!(
        y < 0.0,
        "the cube falls through the switched-off floor, y = {y}"
    );
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
    assert_eq!(
        scene.world_position(c).y,
        4.0,
        "inactive objects don't move"
    );
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
            .filter(|k| {
                k.trigger && k.started && ([k.a, k.b] == [zone, body] || [k.b, k.a] == [zone, body])
            })
            .count()
    };
    assert_eq!(enters(&first), 1);

    scene.game_object_mut(parent).active = false;
    run(&mut physics, &mut scene, 5);
    assert!(!physics.has_body(parent));
    assert_eq!(
        physics.collider_count(),
        colliders - 1,
        "the child's collider left too"
    );

    scene.game_object_mut(parent).active = true;
    let again = run(&mut physics, &mut scene, 30);
    assert!(physics.has_body(parent));
    assert_eq!(
        physics.collider_count(),
        colliders,
        "no duplicate colliders"
    );
    assert_eq!(enters(&again), 1, "the trigger fires again: {again:?}");
    let y = scene.world_position(parent).y;
    assert!(
        (y - 1.0).abs() < 0.05,
        "the child collider rests on the floor, y = {y}"
    );

    // Switching an active object on again changes nothing.
    scene.game_object_mut(parent).active = true;
    run(&mut physics, &mut scene, 2);
    assert_eq!(physics.collider_count(), colliders);
}

/// Contacts between `x` and `y` (either order) that are triggers or not, starting or ending.
fn count(cs: &[Contact], x: Entity, y: Entity, trigger: bool, started: bool) -> usize {
    cs.iter()
        .filter(|k| {
            k.trigger == trigger
                && k.started == started
                && ((k.a == x && k.b == y) || (k.a == y && k.b == x))
        })
        .count()
}

/// An object switched off inside a trigger (or the trigger itself switched off) left without
/// an exit: the zone's script saw Enter, then Enter again when the object came back, and a
/// script counting what is inside never went back to zero. Collisions lost their exit too.
#[test]
fn switching_off_sends_the_exit_for_its_contacts() {
    let mut scene = Scene::new("t");
    let floor = cube(&mut scene, "Floor", Vec3::ZERO, false);
    let zone = cube(&mut scene, "Zone", Vec3::new(5.0, 1.0, 0.0), false);
    scene
        .world
        .get::<&mut BoxCollider>(zone)
        .unwrap()
        .is_trigger = true;
    let guest = cube(&mut scene, "Guest", Vec3::new(5.0, 1.0, 0.0), true);
    scene
        .world
        .get::<&mut Rigidbody>(guest)
        .unwrap()
        .is_kinematic = true;
    let crate_ = cube(&mut scene, "Crate", Vec3::new(0.0, 1.0, 0.0), true);
    let mut physics = Physics::build(&scene, G);
    let first = run(&mut physics, &mut scene, 30);
    assert_eq!(count(&first, zone, guest, true, true), 1);
    assert_eq!(count(&first, floor, crate_, false, true), 1);

    // The object inside switched off: the zone hears it leave.
    scene.game_object_mut(guest).active = false;
    let off = run(&mut physics, &mut scene, 5);
    assert_eq!(count(&off, zone, guest, true, false), 1, "{off:?}");
    scene.game_object_mut(guest).active = true;
    let on = run(&mut physics, &mut scene, 5);
    assert_eq!(count(&on, zone, guest, true, true), 1, "{on:?}");

    // The trigger switched off: the object inside hears the exit too.
    scene.game_object_mut(zone).active = false;
    let off = run(&mut physics, &mut scene, 5);
    assert_eq!(count(&off, zone, guest, true, false), 1, "{off:?}");

    // A floor switched off under a crate: OnCollisionExit.
    scene.game_object_mut(floor).active = false;
    let off = run(&mut physics, &mut scene, 2);
    assert_eq!(count(&off, floor, crate_, false, false), 1, "{off:?}");
}

/// A joint whose connected body was switched off and on again stayed broken: the body left
/// the world with the joint, and switching it on rebuilt only its own joints, not the joints
/// of other objects connected to it, so the lamp fell off its chain.
#[test]
fn a_joint_comes_back_with_its_connected_body() {
    use crate::scene::FixedJoint;
    let mut scene = Scene::new("t");
    let hook = cube(&mut scene, "Hook", Vec3::new(0.0, 5.0, 0.0), true);
    scene
        .world
        .get::<&mut Rigidbody>(hook)
        .unwrap()
        .is_kinematic = true;
    let lamp = cube(&mut scene, "Lamp", Vec3::new(0.0, 3.0, 0.0), true);
    scene.add(
        lamp,
        FixedJoint {
            connected_body: Some(hook),
            ..Default::default()
        },
    );
    let mut physics = Physics::build(&scene, G);
    run(&mut physics, &mut scene, 30);
    assert!((scene.world_position(lamp).y - 3.0).abs() < 0.05);
    assert!(physics.has_joint(lamp, "Fixed Joint"));

    scene.game_object_mut(hook).active = false;
    run(&mut physics, &mut scene, 1);
    assert!(
        !physics.has_joint(lamp, "Fixed Joint"),
        "no body to hold on to"
    );
    scene.game_object_mut(hook).active = true;
    run(&mut physics, &mut scene, 60);
    assert!(physics.has_joint(lamp, "Fixed Joint"), "the joint is back");
    let y = scene.world_position(lamp).y;
    assert!(
        (y - 3.0).abs() < 0.1,
        "the lamp hangs from the hook again, y = {y}"
    );

    // The lamp itself switched off and on while the hook is off: the joint connects to the
    // hook again when the hook comes back, not to the world.
    scene.game_object_mut(lamp).active = false;
    scene.game_object_mut(hook).active = false;
    run(&mut physics, &mut scene, 1);
    scene.game_object_mut(lamp).active = true;
    run(&mut physics, &mut scene, 1);
    scene.game_object_mut(hook).active = true;
    run(&mut physics, &mut scene, 1);
    // The hook moves away: a lamp joined to it follows, one joined to the world stays.
    let mut t = scene.transform(hook);
    t.position.x = 2.0;
    scene.set_transform(hook, t);
    run(&mut physics, &mut scene, 60);
    let x = scene.world_position(lamp).x;
    assert!(
        x > 1.5,
        "the lamp follows the hook it is joined to, x = {x}"
    );
}

/// Writing the Active flag it already has (an animated Active property or a Timeline
/// activation track holding an object on, every frame; a script calling `SetActive(true)`
/// in Update) counted as activity, so every physics step walked all its bodies looking for
/// switched objects while such a clip or timeline played.
#[test]
fn writing_the_same_active_flag_is_no_activity() {
    use crate::animation::{Property, binding};
    let mut scene = Scene::new("t");
    let e = cube(&mut scene, "Cube", Vec3::ZERO, true);
    let a = scene.activity();
    scene.set_active(e, true);
    binding::write(&mut scene, e, Property::Active, 1.0);
    assert_eq!(scene.activity(), a, "nothing switched");
    scene.set_active(e, false);
    assert_ne!(scene.activity(), a);
    assert!(!scene.game_object(e).active);
    let b = scene.activity();
    binding::write(&mut scene, e, Property::Active, 0.0);
    assert_eq!(scene.activity(), b);
    binding::write(&mut scene, e, Property::Active, 1.0);
    assert_ne!(scene.activity(), b);
    assert!(scene.game_object(e).active);
}

/// What `Scene::activity_scope` hands physics: the switched object and what is under it, or
/// everything once too many changes went by; physics follows both ways.
#[test]
fn the_activity_scope_covers_the_switched_objects() {
    let mut scene = Scene::new("t");
    cube(&mut scene, "Floor", Vec3::ZERO, false);
    let parent = scene.spawn("Parent", None);
    let child = cube(&mut scene, "Child", Vec3::new(0.0, 3.0, 0.0), true);
    scene.set_parent(child, Some(parent), None, true);
    let other = cube(&mut scene, "Other", Vec3::new(5.0, 3.0, 0.0), true);
    let seen = scene.activity();
    scene.game_object_mut(parent).active = false;
    let scope = scene.activity_scope(seen).unwrap();
    assert!(scope.contains(&parent) && scope.contains(&child));
    assert!(!scope.contains(&other), "{scope:?}");
    assert!(scene.activity_scope(seen.wrapping_sub(1_000_000)).is_none());

    let mut physics = Physics::build(&scene, G);
    run(&mut physics, &mut scene, 2);
    assert!(!physics.has_body(child) && physics.has_body(other));
    // Thousands of changes between two steps: physics looks at every object.
    for _ in 0..5000 {
        scene.game_object_mut(other).name = "Other".into();
    }
    scene.game_object_mut(parent).active = true;
    scene.game_object_mut(other).active = false;
    run(&mut physics, &mut scene, 2);
    assert!(physics.has_body(child) && !physics.has_body(other));
}
