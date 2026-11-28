//! Physics world tests: bodies, gravity, settings, scale and contacts.

use super::*;
use crate::scene::{BoxCollider, MeshCollider, MeshFilter, PrimitiveMesh, Transform};

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

#[test]
fn falling_cube_lands_on_static_ground() {
    let mut scene = Scene::new("t");
    let ground = scene.spawn("Ground", None);
    scene.add(
        ground,
        MeshFilter {
            mesh: Some(PrimitiveMesh::Plane.into()),
        },
    );
    scene.add(ground, MeshCollider::default());
    let c = cube(&mut scene, "Cube", Vec3::new(0.0, 3.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert_eq!(
        physics.body_count(),
        1,
        "only the cube has a body; the ground is a static collider"
    );
    assert_eq!(physics.collider_count(), 2);
    for _ in 0..200 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
    let y = scene.world_position(c).y;
    assert!((y - 0.5).abs() < 0.05, "cube rests on the plane, y = {y}");
    assert_eq!(
        scene.world_position(ground),
        Vec3::ZERO,
        "static colliders don't move"
    );
}

#[test]
fn gravity_off_and_frozen_axes() {
    let mut scene = Scene::new("t");
    let floating = cube(&mut scene, "Float", Vec3::new(0.0, 5.0, 0.0), true);
    scene
        .world
        .get::<&mut Rigidbody>(floating)
        .unwrap()
        .use_gravity = false;
    let frozen = cube(&mut scene, "Frozen", Vec3::new(3.0, 5.0, 0.0), true);
    scene
        .world
        .get::<&mut Rigidbody>(frozen)
        .unwrap()
        .freeze_position = [false, true, false];
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    for _ in 0..60 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
    assert!((scene.world_position(floating).y - 5.0).abs() < 1e-3);
    assert!((scene.world_position(frozen).y - 5.0).abs() < 1e-3);
}

#[test]
fn zero_gravity_keeps_a_body_still() {
    let mut scene = Scene::new("t");
    let c = cube(&mut scene, "Cube", Vec3::new(0.0, 5.0, 0.0), true);
    let settings = PhysicsSettings {
        gravity: Vec3::ZERO,
        ..Default::default()
    };
    let mut physics = Physics::build_with(&scene, &settings);
    assert_eq!(physics.gravity(), Vec3::ZERO);
    for _ in 0..60 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
    assert!((scene.world_position(c).y - 5.0).abs() < 1e-4);
}

#[test]
fn fixed_timestep_and_max_steps_come_from_settings() {
    let scene = Scene::new("t");
    let mut fine = Physics::build_with(&scene, &PhysicsSettings::default());
    let coarse_settings = PhysicsSettings {
        fixed_timestep: 0.1,
        max_steps_per_frame: 2,
        ..Default::default()
    };
    let mut coarse = Physics::build_with(&scene, &coarse_settings);
    assert_eq!(fine.steps_due(0.1), 5, "0.1 s at 0.02 s per step");
    assert_eq!(coarse.steps_due(0.1), 1, "0.1 s at 0.1 s per step");
    assert_eq!(coarse.steps_due(1.0), 2, "capped at max steps per frame");
}

#[test]
fn scaled_box_collider_matches_object_size() {
    let mut scene = Scene::new("t");
    let ground = cube(&mut scene, "Ground", Vec3::ZERO, false);
    let mut t = scene.transform(ground);
    t.scale = Vec3::new(10.0, 1.0, 10.0);
    scene.set_transform(ground, t);
    let c = cube(&mut scene, "Cube", Vec3::new(4.0, 3.0, 4.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    for _ in 0..200 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
    // Ground top is at y = 0.5, cube half-height 0.5.
    assert!(
        (scene.world_position(c).y - 1.0).abs() < 0.05,
        "{}",
        scene.world_position(c).y
    );
}

#[test]
fn landing_reports_a_collision_and_triggers_are_flagged() {
    let mut scene = Scene::new("t");
    let ground = cube(&mut scene, "Ground", Vec3::ZERO, false);
    let c = cube(&mut scene, "Cube", Vec3::new(0.0, 2.0, 0.0), true);
    let zone = cube(&mut scene, "Zone", Vec3::new(0.0, 1.2, 0.0), false);
    scene
        .world
        .get::<&mut BoxCollider>(zone)
        .unwrap()
        .is_trigger = true;
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut contacts = Vec::new();
    for _ in 0..120 {
        contacts.extend(physics.step(&scene));
        physics.sync_out(&mut scene);
    }
    let hit = |x: Entity, y: Entity| {
        contacts
            .iter()
            .find(|k| (k.a == x && k.b == y) || (k.a == y && k.b == x))
    };
    let landing = hit(c, ground).expect("cube hits the ground");
    assert!(landing.started && !landing.trigger);
    assert!(hit(c, zone).expect("cube passes the trigger").trigger);
}

/// Absurd sizes (a damaged or hand-edited scene: scale 1e30 under a parent of scale 1e30, a
/// Box Collider 1e30 wide, a centre 1e30 away) no longer panic rapier or parry: the infinite
/// world scale made a Mesh Collider's zero centre NaN, which wrapped its triangles in a
/// compound ("nested composite shapes"), and infinite bounds broke the island manager.
#[test]
fn absurd_sizes_and_scales_build_and_step() {
    let mut scene = Scene::new("t");
    let big = scene.spawn("Big", None);
    let t = Transform {
        scale: Vec3::splat(1e30),
        ..Default::default()
    };
    scene.set_transform(big, t);
    let child = scene.spawn("Child", Some(big));
    scene.set_transform(child, t);
    scene.add(
        child,
        MeshFilter {
            mesh: Some(PrimitiveMesh::Cube.into()),
        },
    );
    scene.add(child, MeshCollider::default());
    scene.add(child, BoxCollider::default());
    let wide = cube(&mut scene, "Wide", Vec3::new(0.0, 3.0, 0.0), true);
    scene.world.get::<&mut BoxCollider>(wide).unwrap().size = Vec3::splat(1e30);
    let far = cube(&mut scene, "Far", Vec3::new(5.0, 3.0, 0.0), true);
    scene.world.get::<&mut BoxCollider>(far).unwrap().center = Vec3::new(1e30, 0.0, 0.0);
    cube(&mut scene, "Faller", Vec3::new(1e30, 3.0, 0.0), true);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    for _ in 0..5 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
}

/// A body whose world scale is zero on one axis (a flattened cube): the poses stay finite
/// and nothing writes NaN into the Transform (the matrix decomposition divided by zero).
#[test]
fn zero_scale_bodies_keep_finite_poses() {
    let mut scene = Scene::new("t");
    let c = cube(&mut scene, "Flat", Vec3::new(0.0, 3.0, 0.0), true);
    let mut t = scene.transform(c);
    t.scale = Vec3::new(1.0, 0.0, 1.0);
    t.set_rotation(Quat::from_rotation_y(0.5));
    scene.set_transform(c, t);
    // Squashed flat along X and spun: the rotation comes from the local rotations.
    let thin = cube(&mut scene, "Thin", Vec3::new(4.0, 3.0, 0.0), true);
    let mut tt = scene.transform(thin);
    tt.scale = Vec3::new(0.0, 1.0, 1.0);
    scene.set_transform(thin, tt);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    for _ in 0..30 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
    for e in [c, thin] {
        let (p, r) = physics.body_pose(e).unwrap();
        assert!(p.is_finite() && r.is_finite(), "{p} {r}");
        let t = scene.transform(e);
        assert!(t.position.is_finite() && t.rotation.is_finite(), "{:?}", t);
    }
    assert!(scene.world_position(c).y < 3.0, "still falls");
    assert!((scene.world_rotation(c).to_euler(glam::EulerRot::YXZ).0 - 0.5).abs() < 1e-3);
}

/// Removing an object takes exactly its own colliders, and a body takes those of the objects
/// under it; the tables that say whose collider is whose stay in line with the world.
#[test]
fn removed_objects_take_their_colliders_and_only_theirs() {
    let mut scene = Scene::new("t");
    let wall = cube(&mut scene, "Wall", Vec3::new(10.0, 0.0, 0.0), false);
    let other = cube(&mut scene, "Other", Vec3::new(20.0, 0.0, 0.0), false);
    let car = cube(&mut scene, "Car", Vec3::new(0.0, 5.0, 0.0), true);
    let bumper = scene.spawn("Bumper", Some(car));
    scene.add(bumper, BoxCollider::default());
    let mut physics = Physics::build(&scene, Vec3::ZERO);
    assert_eq!(physics.collider_count(), 4);
    let tables = |p: &Physics| (p.colliders.len(), p.colliders_of.values().flatten().count());
    assert_eq!(tables(&physics), (4, 4));

    physics.remove(wall);
    assert_eq!(physics.collider_count(), 3);
    assert_eq!(tables(&physics), (3, 3));
    let down = Vec3::new(0.0, -1.0, 0.0);
    let above = |x: f32| Vec3::new(x, 3.0, 0.0);
    assert!(physics.raycast(above(10.0), down, 10.0, u32::MAX).is_none());
    assert_eq!(
        physics
            .raycast(above(20.0), down, 10.0, u32::MAX)
            .map(|h| h.entity),
        Some(other)
    );

    // The bumper's collider sits on the car's body: it goes with the car.
    physics.remove(car);
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 1));
    assert_eq!(tables(&physics), (1, 1));
    physics.remove(bumper);
    physics.remove(other);
    assert_eq!(physics.collider_count(), 0);
    assert_eq!(tables(&physics), (0, 0));
}
