use glam::{Quat, Vec2, Vec3};
use hecs::Entity;

use super::*;
use crate::scene::{
    BoxCollider2D, CapsuleCollider2D, CircleCollider2D, Collider2DCommon, EdgeCollider2D,
    PhysicsMaterial2D, PolygonCollider2D, Rigidbody2D, Scene, Transform,
};

fn object(scene: &mut Scene, name: &str, pos: Vec2) -> Entity {
    let e = scene.spawn(name, None);
    scene.set_transform(e, Transform::from_position(pos.extend(0.0)));
    e
}

fn ground(scene: &mut Scene, y: f32) -> Entity {
    let g = object(scene, "Ground", Vec2::new(0.0, y));
    scene.add(
        g,
        BoxCollider2D {
            size: Vec2::new(20.0, 1.0),
            ..Default::default()
        },
    );
    g
}

fn crate_box(scene: &mut Scene, pos: Vec2) -> Entity {
    let b = object(scene, "Box", pos);
    scene.add(b, Rigidbody2D::default());
    scene.add(b, BoxCollider2D::default());
    b
}

fn build(scene: &Scene) -> Physics2D {
    Physics2D::build(scene, &Default::default(), &Default::default())
}

fn run(p: &mut Physics2D, scene: &mut Scene, steps: usize) -> Vec<Contact2D> {
    let mut all = Vec::new();
    for _ in 0..steps {
        all.extend(p.step());
        p.sync_out(scene);
    }
    all
}

#[test]
fn bodies_fall_and_land_on_static_colliders() {
    let mut scene = Scene::new("t");
    ground(&mut scene, 0.0);
    let b = crate_box(&mut scene, Vec2::new(0.0, 5.0));
    // Depth (Z) is kept while the body moves.
    let mut t = scene.transform(b);
    t.position.z = 3.0;
    scene.set_transform(b, t);
    let mut p = build(&scene);
    assert_eq!(p.body_count(), 1);
    assert_eq!(p.collider_count(), 2);
    run(&mut p, &mut scene, 10);
    let falling = scene.world_position(b);
    assert!(falling.y < 5.0 && falling.y > 1.0, "{falling}");
    assert!(p.velocity(b).unwrap().y < 0.0);
    run(&mut p, &mut scene, 150);
    let rest = scene.world_position(b);
    // Ground top at 0.5, box half height 0.5.
    assert!((rest.y - 1.0).abs() < 0.05, "{rest}");
    assert_eq!(rest.z, 3.0);
}

#[test]
fn gravity_scale_static_and_kinematic_bodies() {
    let mut scene = Scene::new("t");
    let floaty = object(&mut scene, "Floaty", Vec2::ZERO);
    scene.add(
        floaty,
        Rigidbody2D {
            gravity_scale: 0.0,
            ..Default::default()
        },
    );
    let fixed = object(&mut scene, "Fixed", Vec2::new(3.0, 0.0));
    scene.add(
        fixed,
        Rigidbody2D {
            body_type: BodyType2D::Static,
            ..Default::default()
        },
    );
    let mover = object(&mut scene, "Mover", Vec2::new(-3.0, 0.0));
    scene.add(
        mover,
        Rigidbody2D {
            body_type: BodyType2D::Kinematic,
            ..Default::default()
        },
    );
    scene.add(mover, BoxCollider2D::default());
    let mut p = build(&scene);
    p.move_body(mover, Some(Vec2::new(-2.0, 0.0)), Some(90.0));
    run(&mut p, &mut scene, 1);
    let m = scene.world_position(mover);
    assert!(
        (m.x + 2.0).abs() < 1e-3,
        "kinematic MovePosition arrives in one step: {m}"
    );
    let angle = z_angle(scene.world_rotation(mover)).to_degrees();
    assert!((angle - 90.0).abs() < 0.1, "{angle}");
    assert_eq!(p.velocity(mover), Some(Vec2::ZERO), "velocity is restored");
    run(&mut p, &mut scene, 30);
    assert!(scene.world_position(floaty).y.abs() < 1e-4);
    assert_eq!(scene.world_position(fixed), Vec3::new(3.0, 0.0, 0.0));
    assert_eq!(p.body_type(fixed), Some(BodyType2D::Static));
}

#[test]
fn velocity_forces_and_gravity_setting() {
    let mut scene = Scene::new("t");
    let b = object(&mut scene, "B", Vec2::ZERO);
    scene.add(b, Rigidbody2D::default());
    scene.add(b, CircleCollider2D::default());
    let mut p = build(&scene);
    p.set_gravity(Vec2::ZERO);
    assert_eq!(p.gravity(), Vec2::ZERO);
    p.set_velocity(b, Vec2::new(2.0, 0.0));
    run(&mut p, &mut scene, 50); // one second
    assert!((scene.world_position(b).x - 2.0).abs() < 0.05);
    p.apply_impulse(b, Vec2::new(0.0, 3.0));
    assert!(
        (p.velocity(b).unwrap().y - 3.0).abs() < 1e-3,
        "1 kg: impulse = Δv"
    );
    p.set_angular_velocity(b, 90.0);
    assert!((p.angular_velocity(b).unwrap() - 90.0).abs() < 1e-3);
    assert!((p.mass(b).unwrap() - 1.0).abs() < 1e-3);
}

#[test]
fn freeze_rotation_keeps_the_angle() {
    let mut scene = Scene::new("t");
    ground(&mut scene, 0.0);
    // Lands on a corner: tips over unless rotation is frozen.
    let make = |scene: &mut Scene, x: f32, freeze: bool| {
        let b = object(scene, "B", Vec2::new(x, 2.0));
        scene.set_transform(
            b,
            Transform {
                rotation: Quat::from_rotation_z(0.6),
                ..Transform::from_position(Vec3::new(x, 2.0, 0.0))
            },
        );
        scene.add(
            b,
            Rigidbody2D {
                freeze_rotation: freeze,
                ..Default::default()
            },
        );
        scene.add(b, BoxCollider2D::default());
        b
    };
    let frozen = make(&mut scene, -3.0, true);
    let free = make(&mut scene, 3.0, false);
    let mut p = build(&scene);
    run(&mut p, &mut scene, 150);
    assert!((z_angle(scene.world_rotation(frozen)) - 0.6).abs() < 1e-3);
    assert!((z_angle(scene.world_rotation(free)) - 0.6).abs() > 0.1);
}

#[test]
fn collisions_enter_stay_exit_with_contact_data() {
    let mut scene = Scene::new("t");
    let g = ground(&mut scene, 0.0);
    let b = crate_box(&mut scene, Vec2::new(0.0, 1.5));
    let mut p = build(&scene);
    let contacts = run(&mut p, &mut scene, 40);
    let enter = contacts
        .iter()
        .find(|c| c.phase == Phase::Enter)
        .expect("lands");
    assert!(!enter.trigger);
    let pair = [enter.a, enter.b];
    assert!(pair.contains(&g) && pair.contains(&b));
    // Normal from a to b: the box sits above the ground.
    let up = if enter.a == g { 1.0 } else { -1.0 };
    assert!(enter.normal.y * up > 0.9, "{:?}", enter.normal);
    assert!((enter.point.y - 0.5).abs() < 0.1, "{:?}", enter.point);
    assert!(enter.relative_velocity.length() > 1.0, "impact speed");
    assert!(contacts.iter().any(|c| c.phase == Phase::Stay));
    // Lift it off: the contact ends.
    p.teleport(b, Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY);
    let after = run(&mut p, &mut scene, 2);
    assert!(after.iter().any(|c| c.phase == Phase::Exit && !c.trigger));
}

#[test]
fn triggers_report_without_blocking() {
    let mut scene = Scene::new("t");
    let coin = object(&mut scene, "Coin", Vec2::new(0.0, 2.0));
    scene.add(
        coin,
        CircleCollider2D {
            common: Collider2DCommon {
                is_trigger: true,
                ..Default::default()
            },
            radius: 0.3,
        },
    );
    let b = crate_box(&mut scene, Vec2::new(0.0, 4.0));
    let mut p = build(&scene);
    let contacts = run(&mut p, &mut scene, 60);
    let enter = contacts
        .iter()
        .find(|c| c.trigger && c.phase == Phase::Enter)
        .expect("falls through the coin");
    assert!([enter.a, enter.b].contains(&coin));
    assert!(contacts.iter().any(|c| c.trigger && c.phase == Phase::Exit));
    assert!(
        scene.world_position(b).y < 0.0,
        "triggers don't stop bodies"
    );
}

#[test]
fn raycasts_and_overlaps() {
    let mut scene = Scene::new("t");
    let g = ground(&mut scene, 0.0);
    let wall = object(&mut scene, "Wall", Vec2::new(5.0, 2.0));
    scene.add(wall, BoxCollider2D::default());
    scene.game_object_mut(wall).layer = 3;
    let b = crate_box(&mut scene, Vec2::new(0.0, 3.0));
    let p = build(&scene);
    let hit = p
        .raycast(Vec2::new(0.0, 1.5), Vec2::NEG_Y, 10.0, u32::MAX)
        .unwrap();
    assert_eq!(hit.entity, g);
    assert!((hit.distance - 1.0).abs() < 1e-3 && hit.normal == Vec2::Y);
    assert_eq!(hit.body, None, "static colliders have no body");
    assert!(
        p.raycast(Vec2::new(0.0, 1.5), Vec2::NEG_Y, 0.5, u32::MAX)
            .is_none()
    );
    // Starting inside the box reports it at distance 0 (Queries Start In Colliders).
    let inside = p
        .raycast(Vec2::new(0.0, 3.0), Vec2::NEG_Y, 10.0, u32::MAX)
        .unwrap();
    assert_eq!(
        (inside.entity, inside.distance, inside.body),
        (b, 0.0, Some(b))
    );
    let all = p.raycast_all(Vec2::new(0.0, 3.0), Vec2::NEG_Y, 10.0, u32::MAX);
    assert_eq!(all.iter().map(|h| h.entity).collect::<Vec<_>>(), vec![b, g]);
    // Masks select layers.
    let side = Vec2::new(0.0, 2.0);
    assert_eq!(p.raycast(side, Vec2::X, 10.0, 1 << 3).unwrap().entity, wall);
    assert!(p.raycast(side, Vec2::X, 10.0, 1).is_none());
    assert_eq!(
        p.overlap_circle_all(Vec2::new(5.0, 2.6), 0.2, u32::MAX),
        vec![wall]
    );
    assert!(
        p.overlap_circle_all(Vec2::new(5.0, 3.0), 0.2, u32::MAX)
            .is_empty()
    );
    assert_eq!(p.overlap_point_all(Vec2::new(0.0, 3.2), u32::MAX), vec![b]);
    let boxed = p.overlap_box_all(Vec2::new(2.5, 2.0), Vec2::new(6.0, 2.5), 0.0, u32::MAX);
    assert!(boxed.contains(&wall) && boxed.contains(&b));
}

#[test]
fn queries_can_skip_triggers_and_start_points() {
    let mut scene = Scene::new("t");
    let t = object(&mut scene, "T", Vec2::ZERO);
    scene.add(
        t,
        BoxCollider2D {
            common: Collider2DCommon {
                is_trigger: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let settings = crate::settings::Physics2DSettings {
        queries_hit_triggers: false,
        queries_start_in_colliders: false,
        ..Default::default()
    };
    let p = Physics2D::build(&scene, &settings, &Default::default());
    assert!(
        p.raycast(Vec2::new(-3.0, 0.0), Vec2::X, 10.0, u32::MAX)
            .is_none()
    );
    let p = build(&scene);
    assert!(
        p.raycast(Vec2::new(-3.0, 0.0), Vec2::X, 10.0, u32::MAX)
            .is_some()
    );
}

#[test]
fn every_shape_builds_with_scale_offset_and_material() {
    let mut scene = Scene::new("t");
    let e = object(&mut scene, "Shapes", Vec2::ZERO);
    let mut t = scene.transform(e);
    t.scale = Vec3::new(2.0, 2.0, 1.0);
    scene.set_transform(e, t);
    let bouncy = Collider2DCommon {
        material: PhysicsMaterial2D {
            friction: 0.0,
            bounciness: 1.0,
        },
        offset: Vec2::new(1.0, 0.0),
        ..Default::default()
    };
    scene.add(
        e,
        BoxCollider2D {
            common: bouncy,
            ..Default::default()
        },
    );
    scene.add(e, CapsuleCollider2D::default());
    scene.add(
        e,
        PolygonCollider2D {
            // Concave "L": decomposed into convex parts.
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(2.0, 0.0),
                Vec2::new(2.0, 1.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(1.0, 2.0),
                Vec2::new(0.0, 2.0),
            ],
            ..Default::default()
        },
    );
    scene.add(
        e,
        EdgeCollider2D {
            edge_radius: 0.1,
            ..Default::default()
        },
    );
    let p = build(&scene);
    assert_eq!(p.collider_count(), 4);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    // Box offset (1, 0) × scale 2 → centred at x = 2, half size 1.
    let hit = p
        .raycast(Vec2::new(5.0, -0.5), Vec2::NEG_X, 10.0, u32::MAX)
        .unwrap();
    assert!((hit.point.x - 3.0).abs() < 1e-3, "{:?}", hit.point);
    // The L's notch (1.5, 1.5) × 2 is empty.
    assert!(
        p.overlap_point_all(Vec2::new(3.0, 3.0), u32::MAX)
            .is_empty()
    );
    assert!(
        !p.overlap_point_all(Vec2::new(1.0, 3.0), u32::MAX)
            .is_empty()
    );

    // Polygon without points uses a unit square; too few points warn.
    let mut s2 = Scene::new("t");
    let a = object(&mut s2, "A", Vec2::ZERO);
    s2.add(a, PolygonCollider2D::default());
    let b = object(&mut s2, "B", Vec2::new(5.0, 0.0));
    s2.add(
        b,
        EdgeCollider2D {
            points: vec![Vec2::ZERO],
            ..Default::default()
        },
    );
    let p = build(&s2);
    assert!(
        !p.overlap_point_all(Vec2::new(0.4, 0.4), u32::MAX)
            .is_empty()
    );
    assert_eq!(p.warnings.len(), 1);
}

#[test]
fn bounciness_and_destroy() {
    let mut scene = Scene::new("t");
    let g = ground(&mut scene, 0.0);
    {
        let mut c = scene.world.get::<&mut BoxCollider2D>(g).unwrap();
        c.common.material.bounciness = 1.0;
    }
    let ball = object(&mut scene, "Ball", Vec2::new(0.0, 3.0));
    scene.add(ball, Rigidbody2D::default());
    scene.add(
        ball,
        CircleCollider2D {
            common: Collider2DCommon {
                material: PhysicsMaterial2D {
                    friction: 0.4,
                    bounciness: 1.0,
                },
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut p = build(&scene);
    let mut bounced = false;
    for _ in 0..120 {
        p.step();
        if p.velocity(ball).unwrap().y > 2.0 {
            bounced = true;
        }
    }
    assert!(bounced);
    p.remove(ball);
    assert!(!p.has_body(ball));
    assert_eq!(p.collider_count(), 1);
}

#[test]
fn colliders_attach_to_a_parent_body_and_layers_filter() {
    let mut scene = Scene::new("t");
    let parent = object(&mut scene, "Parent", Vec2::new(0.0, 5.0));
    scene.add(parent, Rigidbody2D::default());
    let child = scene.spawn("Child", Some(parent));
    scene.set_transform(child, Transform::from_position(Vec3::new(1.0, 0.0, 0.0)));
    scene.add(child, BoxCollider2D::default());
    ground(&mut scene, 0.0);
    // Layers 0 and 4 don't collide: the body falls through the ground.
    scene.game_object_mut(child).layer = 4;
    let mut layers = crate::settings::PhysicsSettings::default();
    layers.set_layers_collide(0, 4, false);
    let mut p = Physics2D::build(&scene, &Default::default(), &layers);
    let hit = p
        .raycast(Vec2::new(1.0, 8.0), Vec2::NEG_Y, 10.0, 1 << 4)
        .unwrap();
    assert_eq!((hit.entity, hit.body), (child, Some(parent)));
    run(&mut p, &mut scene, 100);
    assert!(scene.world_position(parent).y < 0.0);
    p.set_layer(child, 0);
}
