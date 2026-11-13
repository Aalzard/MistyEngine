//! Raycast / SphereCast / OverlapSphere tests, layer masks and the Layer Collision Matrix.

use super::*;
use crate::scene::{BoxCollider, PrimitiveMesh, Transform};

fn box_at(scene: &mut Scene, name: &str, pos: Vec3, layer: u8) -> Entity {
    let e = scene.spawn(name, None);
    scene.set_transform(e, Transform::from_position(pos));
    scene.add(e, crate::scene::MeshFilter::primitive(PrimitiveMesh::Cube));
    scene.add(e, BoxCollider::default());
    scene.game_object_mut(e).layer = layer;
    e
}

fn build(scene: &Scene) -> Physics {
    Physics::build(scene, Vec3::ZERO)
}

#[test]
fn raycast_hits_the_nearest_box_with_point_normal_and_distance() {
    let mut scene = Scene::new("t");
    let near = box_at(&mut scene, "Near", Vec3::new(0.0, 0.0, 5.0), 0);
    box_at(&mut scene, "Far", Vec3::new(0.0, 0.0, 9.0), 0);
    let physics = build(&scene);
    // Before any step: queries already see the colliders.
    let hit = physics
        .raycast(Vec3::ZERO, Vec3::Z * 3.0, f32::INFINITY, ALL_LAYERS)
        .expect("hits the near box");
    assert_eq!(hit.entity, near);
    assert!((hit.distance - 4.5).abs() < 1e-3, "{}", hit.distance);
    assert!(hit.point.distance(Vec3::new(0.0, 0.0, 4.5)) < 1e-3);
    assert!(hit.normal.distance(Vec3::NEG_Z) < 1e-3, "{:?}", hit.normal);
    assert_eq!(hit.body, None, "static collider");

    let all = physics.raycast_all(Vec3::ZERO, Vec3::Z, 100.0, ALL_LAYERS);
    assert_eq!(all.len(), 2);
    assert!(all[0].distance < all[1].distance);
}

#[test]
fn raycast_misses_beside_beyond_and_from_inside() {
    let mut scene = Scene::new("t");
    let a = box_at(&mut scene, "A", Vec3::new(0.0, 0.0, 5.0), 0);
    let b = box_at(&mut scene, "B", Vec3::new(0.0, 0.0, 9.0), 0);
    let physics = build(&scene);
    assert!(
        physics
            .raycast(Vec3::ZERO, Vec3::X, 100.0, ALL_LAYERS)
            .is_none()
    );
    assert!(
        physics
            .raycast(Vec3::ZERO, Vec3::Z, 4.0, ALL_LAYERS)
            .is_none()
    );
    assert!(
        physics
            .raycast(Vec3::ZERO, Vec3::ZERO, 10.0, ALL_LAYERS)
            .is_none()
    );
    // Starting inside A: A is ignored, B is hit.
    let hit = physics
        .raycast(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, 100.0, ALL_LAYERS)
        .unwrap();
    assert_eq!(hit.entity, b);
    assert_ne!(hit.entity, a);
}

#[test]
fn layer_masks_select_what_rays_hit() {
    let mut scene = Scene::new("t");
    let player = box_at(&mut scene, "Player", Vec3::new(0.0, 0.0, 3.0), 5);
    let wall = box_at(&mut scene, "Wall", Vec3::new(0.0, 0.0, 6.0), 0);
    let ignored = box_at(&mut scene, "Ignored", Vec3::new(3.0, 0.0, 0.0), 2);
    let physics = build(&scene);
    let ray = |mask| {
        physics
            .raycast(Vec3::ZERO, Vec3::Z, 100.0, mask)
            .map(|h| h.entity)
    };
    assert_eq!(ray(ALL_LAYERS), Some(player));
    assert_eq!(ray(!layers::get_mask(&["Player"])), Some(wall));
    assert_eq!(ray(layers::get_mask(&["Water"])), None);
    // The Ignore Raycast layer is skipped by the default mask only.
    let side = |mask| {
        physics
            .raycast(Vec3::ZERO, Vec3::X, 100.0, mask)
            .map(|h| h.entity)
    };
    assert_eq!(side(DEFAULT_RAYCAST_LAYERS), None);
    assert_eq!(side(ALL_LAYERS), Some(ignored));
}

#[test]
fn triggers_are_hit_unless_queries_hit_triggers_is_off() {
    let mut scene = Scene::new("t");
    let zone = box_at(&mut scene, "Zone", Vec3::new(0.0, 0.0, 3.0), 0);
    scene
        .world
        .get::<&mut BoxCollider>(zone)
        .unwrap()
        .is_trigger = true;
    let hits = build(&scene).raycast(Vec3::ZERO, Vec3::Z, 10.0, ALL_LAYERS);
    assert_eq!(hits.map(|h| h.entity), Some(zone));
    let settings = PhysicsSettings {
        queries_hit_triggers: false,
        ..Default::default()
    };
    let physics = Physics::build_with(&scene, &settings);
    assert!(
        physics
            .raycast(Vec3::ZERO, Vec3::Z, 10.0, ALL_LAYERS)
            .is_none()
    );
    assert!(
        physics
            .overlap_sphere(Vec3::new(0.0, 0.0, 3.0), 1.0, ALL_LAYERS)
            .is_empty()
    );
}

#[test]
fn hits_report_the_rigidbody_of_a_parent() {
    let mut scene = Scene::new("t");
    let parent = scene.spawn("Body", None);
    scene.set_transform(parent, Transform::from_position(Vec3::new(0.0, 0.0, 4.0)));
    scene.add(parent, Rigidbody::default());
    let child = box_at(&mut scene, "Part", Vec3::ZERO, 0);
    scene.set_parent(child, Some(parent), None, false);
    let physics = build(&scene);
    let hit = physics
        .raycast(Vec3::ZERO, Vec3::Z, 10.0, ALL_LAYERS)
        .unwrap();
    assert_eq!(hit.entity, child);
    assert_eq!(hit.body, Some(parent));
}

#[test]
fn sphere_cast_and_overlap_sphere() {
    let mut scene = Scene::new("t");
    let a = box_at(&mut scene, "A", Vec3::new(0.8, 0.0, 5.0), 0);
    let b = box_at(&mut scene, "B", Vec3::new(4.0, 0.0, 0.0), 3);
    let physics = build(&scene);
    // The ray passes beside A, a sphere of radius 0.5 touches it.
    assert!(
        physics
            .raycast(Vec3::ZERO, Vec3::Z, 10.0, ALL_LAYERS)
            .is_none()
    );
    let hit = physics
        .sphere_cast(Vec3::ZERO, 0.5, Vec3::Z, 10.0, ALL_LAYERS)
        .expect("sphere touches A");
    assert_eq!(hit.entity, a);
    // The sphere's side meets the near face at x = 0.3: centre at z = 4.5 − √(0.5² − 0.3²).
    assert!((hit.distance - 4.1).abs() < 0.02, "{}", hit.distance);
    assert!(hit.point.z > 4.4 && hit.point.z < 4.6, "{:?}", hit.point);

    let near_b = physics.overlap_sphere(Vec3::new(3.0, 0.0, 0.0), 0.6, ALL_LAYERS);
    assert_eq!(near_b, vec![b]);
    assert!(
        physics
            .overlap_sphere(Vec3::new(3.0, 0.0, 0.0), 0.4, ALL_LAYERS)
            .is_empty()
    );
    assert!(
        physics
            .overlap_sphere(
                Vec3::new(3.0, 0.0, 0.0),
                0.6,
                layers::get_mask(&["Default"])
            )
            .is_empty()
    );
}

#[test]
fn layer_collision_matrix_lets_objects_pass_through() {
    let run = |ignore: bool| {
        let mut scene = Scene::new("t");
        box_at(&mut scene, "Ground", Vec3::ZERO, 0);
        let c = box_at(&mut scene, "Player", Vec3::new(0.0, 2.0, 0.0), 5);
        scene.add(c, Rigidbody::default());
        let mut settings = PhysicsSettings::default();
        settings.set_layers_collide(0, 5, !ignore);
        let mut physics = Physics::build_with(&scene, &settings);
        let mut contacts = 0;
        for _ in 0..100 {
            contacts += physics.step(&scene).len();
            physics.sync_out(&mut scene);
        }
        (scene.world_position(c).y, contacts)
    };
    let (landed, touched) = run(false);
    assert!((landed - 1.0).abs() < 0.05, "rests on the ground: {landed}");
    assert!(touched > 0);
    let (fell, none) = run(true);
    assert!(fell < -2.0, "falls through: {fell}");
    assert_eq!(none, 0, "ignored layers don't report contacts");
}

#[test]
fn changing_the_layer_while_playing_updates_queries() {
    let mut scene = Scene::new("t");
    let e = box_at(&mut scene, "Box", Vec3::new(0.0, 0.0, 3.0), 0);
    let mut physics = build(&scene);
    let water = layers::get_mask(&["Water"]);
    assert!(physics.raycast(Vec3::ZERO, Vec3::Z, 10.0, water).is_none());
    physics.set_layer(e, 3);
    assert_eq!(
        physics
            .raycast(Vec3::ZERO, Vec3::Z, 10.0, water)
            .map(|h| h.entity),
        Some(e)
    );
}

/// A mirrored object (scale x -1) mirrors its collider's centre, like its mesh: the collider
/// sat on the other side.
#[test]
fn mirrored_objects_mirror_their_collider_centres() {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Mirrored", None);
    let mut t = Transform::from_position(Vec3::ZERO);
    t.scale = Vec3::new(-1.0, 1.0, 1.0);
    scene.set_transform(e, t);
    scene.add(
        e,
        BoxCollider {
            center: Vec3::new(2.0, 0.0, 0.0),
            ..Default::default()
        },
    );
    let physics = build(&scene);
    let down = |x: f32| physics.raycast(Vec3::new(x, 5.0, 0.0), Vec3::NEG_Y, 10.0, ALL_LAYERS);
    assert!(down(-2.0).is_some(), "the box is drawn at x = -2");
    assert!(down(2.0).is_none());
}
