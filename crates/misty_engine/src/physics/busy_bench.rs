//! What the engine's side of physics costs in a busy scene (ignored: a benchmark).
//!
//! `cargo test --release -p misty_engine --lib physics::busy_bench -- --ignored --nocapture`
//!
//! 3,000 boxes fall on a floor between 3,000 static boxes: a frame while they tumble, and a
//! frame once they lie still (asleep).
use super::*;
use crate::scene::{BoxCollider, Transform};
use std::time::Instant;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

#[test]
#[ignore = "bench"]
fn busy_bench() {
    let mut scene = Scene::new("b");
    let floor = scene.spawn("Floor", None);
    scene.set_transform(floor, Transform::from_position(Vec3::new(0.0, -0.5, 0.0)));
    scene.add(
        floor,
        BoxCollider {
            size: Vec3::new(2000.0, 1.0, 2000.0),
            ..Default::default()
        },
    );
    let n = 3000;
    for i in 0..n {
        let (x, z) = ((i % 60) as f32 * 3.0, (i / 60) as f32 * 3.0);
        let wall = scene.spawn(&format!("Wall{i}"), None);
        scene.set_transform(wall, Transform::from_position(Vec3::new(x, 0.5, z)));
        scene.add(wall, BoxCollider::default());
        let b = scene.spawn(&format!("Box{i}"), None);
        // Off-centre above a wall: it lands, tips over and comes to rest on the floor.
        scene.set_transform(
            b,
            Transform::from_position(Vec3::new(x + 0.6, 2.0 + (i % 7) as f32 * 0.5, z + 0.3)),
        );
        scene.add(b, BoxCollider::default());
        scene.add(b, Rigidbody::default());
    }
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let dt = 1.0 / 60.0;
    let frame = |physics: &mut Physics, scene: &mut Scene| {
        let t = Instant::now();
        physics.update(scene, dt);
        t.elapsed().as_secs_f64() * 1000.0
    };
    let moving: Vec<f64> = (0..90).map(|_| frame(&mut physics, &mut scene)).collect();
    for _ in 0..600 {
        frame(&mut physics, &mut scene);
    }
    let frames = std::env::var("MISTY_PHYSICS_BENCH_FRAMES")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(120);
    let still: Vec<f64> = (0..frames)
        .map(|_| frame(&mut physics, &mut scene))
        .collect();
    eprintln!(
        "{n} boxes among {n} static ones: {:7.3} ms/frame falling, {:7.3} ms/frame at rest",
        median(moving),
        median(still)
    );
}
