use glam::{Mat4, Vec3};

use super::*;
use crate::scene::{Scene, Transform};

const DT: f32 = 1.0 / 60.0;

/// A seeded system with long-lived, motionless particles and no shape.
fn system() -> ParticleSystem {
    let mut s = ParticleSystem {
        seed: 42,
        ..Default::default()
    };
    s.main.start_lifetime = MinMax::constant(100.0);
    s.main.start_speed = MinMax::constant(0.0);
    s.shape.enabled = false;
    s
}

fn run(e: &mut ParticleEmitter, s: &ParticleSystem, seconds: f32) {
    let frames = (seconds / DT).round() as usize;
    for _ in 0..frames {
        e.step(s, &Mat4::IDENTITY, DEFAULT_GRAVITY, DT);
    }
}

fn started(s: &ParticleSystem) -> ParticleEmitter {
    let mut e = ParticleEmitter::new(s);
    e.play(s, &Mat4::IDENTITY, DEFAULT_GRAVITY);
    e
}

#[test]
fn rate_over_time_emits_rate_times_seconds() {
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(10.0);
    let mut e = started(&s);
    run(&mut e, &s, 2.0);
    assert!((19..=20).contains(&e.count()), "{}", e.count());
    // Frame rate doesn't change the count.
    let mut slow = started(&s);
    for _ in 0..8 {
        slow.step(&s, &Mat4::IDENTITY, DEFAULT_GRAVITY, 0.25);
    }
    assert_eq!(slow.count(), 20);
}

#[test]
fn bursts_fire_once_per_cycle_and_repeat() {
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(0.0);
    s.main.duration = 1.0;
    s.emission.bursts = vec![
        Burst {
            time: 0.0,
            count: 30,
            ..Default::default()
        },
        Burst {
            time: 0.5,
            count: 5,
            cycles: 3,
            interval: 0.1,
            ..Default::default()
        },
    ];
    let mut e = started(&s);
    e.step(&s, &Mat4::IDENTITY, DEFAULT_GRAVITY, DT);
    assert_eq!(e.count(), 30, "time-0 burst on the first frame");
    run(&mut e, &s, 0.95);
    assert_eq!(e.count(), 45, "3 cycles of 5");
    run(&mut e, &s, 1.0);
    assert_eq!(e.count(), 90, "everything again in the next loop");
}

#[test]
fn burst_probability_zero_never_fires() {
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(0.0);
    s.emission.bursts = vec![Burst {
        probability: 0.0,
        ..Default::default()
    }];
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    assert_eq!(e.count(), 0);
}

#[test]
fn particles_die_after_their_lifetime() {
    let mut s = system();
    s.main.start_lifetime = MinMax::constant(0.5);
    s.main.looping = false;
    s.main.duration = 0.2;
    s.emission.rate_over_time = MinMax::constant(0.0);
    s.emission.bursts = vec![Burst::default()];
    let mut e = started(&s);
    run(&mut e, &s, 0.4);
    assert_eq!(e.count(), 30);
    assert!(e.is_playing(), "one-shot plays until its particles die");
    run(&mut e, &s, 0.2);
    assert_eq!(e.count(), 0);
    assert!(!e.is_playing());
}

#[test]
fn max_particles_caps_emission() {
    let mut s = system();
    s.main.max_particles = 25;
    s.emission.rate_over_time = MinMax::constant(1000.0);
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    assert_eq!(e.count(), 25);
    e.emit(&s, &Mat4::IDENTITY, 10);
    assert_eq!(e.count(), 25, "Emit respects the cap too");
}

#[test]
fn gravity_modifier_pulls_particles_down() {
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(0.0);
    s.main.gravity_modifier = 1.0;
    let mut e = started(&s);
    e.emit(&s, &Mat4::IDENTITY, 1);
    run(&mut e, &s, 1.0);
    let p = e.particles[0];
    assert!((p.velocity.y + 9.81).abs() < 0.01, "{:?}", p.velocity);
    assert!(
        p.position.y < -4.5 && p.position.y > -5.2,
        "{:?}",
        p.position
    );
    // No modifier: no fall.
    s.main.gravity_modifier = 0.0;
    let mut e = started(&s);
    e.emit(&s, &Mat4::IDENTITY, 1);
    run(&mut e, &s, 1.0);
    assert_eq!(e.particles[0].position.y, 0.0);
}

#[test]
fn start_delay_waits_before_emitting() {
    let mut s = system();
    s.main.start_delay = MinMax::constant(1.0);
    s.emission.rate_over_time = MinMax::constant(10.0);
    let mut e = started(&s);
    run(&mut e, &s, 0.9);
    assert_eq!(e.count(), 0);
    run(&mut e, &s, 0.6);
    assert!(e.count() >= 4);
}

#[test]
fn prewarm_starts_with_a_full_cycle() {
    let mut s = system();
    s.main.prewarm = true;
    s.main.duration = 2.0;
    s.emission.rate_over_time = MinMax::constant(10.0);
    let e = started(&s);
    assert!((19..=21).contains(&e.count()), "{}", e.count());
}

#[test]
fn stop_pause_clear_and_play() {
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(10.0);
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    let n = e.count();
    e.pause();
    run(&mut e, &s, 1.0);
    assert_eq!(e.count(), n, "paused: nothing changes");
    assert!(e.is_paused() && !e.is_playing());
    e.play(&s, &Mat4::IDENTITY, DEFAULT_GRAVITY);
    assert!(e.is_playing(), "Play resumes");
    e.stop();
    run(&mut e, &s, 1.0);
    assert_eq!(e.count(), n, "stopped: live particles stay, no new ones");
    e.clear();
    assert_eq!(e.count(), 0);
}

#[test]
fn same_seed_same_particles() {
    let mut s = system();
    s.main.start_speed = MinMax::between(1.0, 5.0);
    s.shape.enabled = true;
    s.shape.shape = ShapeKind::Sphere;
    let mut a = started(&s);
    let mut b = started(&s);
    run(&mut a, &s, 1.0);
    run(&mut b, &s, 1.0);
    assert_eq!(a.particles, b.particles);
}

#[test]
fn random_between_two_constants_stays_in_range() {
    let mut s = system();
    s.main.start_size = MinMax::between(0.2, 0.6);
    s.emission.rate_over_time = MinMax::constant(500.0);
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    let sizes: Vec<f32> = e.particles.iter().map(|p| p.size).collect();
    assert!(sizes.iter().all(|v| (0.2..=0.6).contains(v)));
    let spread =
        sizes.iter().cloned().fold(0.0f32, f32::max) - sizes.iter().cloned().fold(1.0f32, f32::min);
    assert!(spread > 0.3, "values vary: {spread}");
}

#[test]
fn gradient_blends_color_and_alpha_keys() {
    let g = Gradient {
        color_keys: vec![
            ColorKey {
                time: 0.0,
                color: [1.0, 0.0, 0.0],
            },
            ColorKey {
                time: 1.0,
                color: [0.0, 0.0, 1.0],
            },
        ],
        alpha_keys: vec![
            AlphaKey {
                time: 0.0,
                alpha: 1.0,
            },
            AlphaKey {
                time: 0.5,
                alpha: 0.0,
            },
        ],
    };
    assert_eq!(g.evaluate(0.0), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(g.evaluate(0.5), [0.5, 0.0, 0.5, 0.0]);
    assert_eq!(g.evaluate(1.0), [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(g.evaluate(0.25)[3], 0.5);
}

#[test]
fn scene_update_plays_on_awake_and_moves_local_particles_with_the_object() {
    let mut scene = Scene::new("Particles");
    let e = scene.spawn("FX", None);
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(0.0);
    s.emission.bursts = vec![Burst {
        count: 3,
        ..Default::default()
    }];
    scene.add(e, s.clone());
    let quiet = scene.spawn("Quiet", None);
    s.main.play_on_awake = false;
    scene.add(quiet, s);

    update(&mut scene, DT, DEFAULT_GRAVITY);
    let count = |sc: &Scene, e| sc.world.get::<&ParticleEmitter>(e).unwrap().count();
    assert_eq!(count(&scene, e), 3);
    assert_eq!(count(&scene, quiet), 0, "no Play On Awake");

    // Local space: particles follow the object.
    scene.set_transform(e, Transform::from_position(Vec3::new(5.0, 0.0, 0.0)));
    let b = batches(&scene);
    assert_eq!(b.len(), 1);
    assert!(b[0].particles.iter().all(|p| p.position[0] == 5.0));

    // Inactive objects don't simulate or draw.
    scene.game_object_mut(e).active = false;
    assert!(batches(&scene).is_empty());
    clear_all(&mut scene);
    assert!(scene.world.get::<&ParticleEmitter>(e).is_err());
}

#[test]
fn world_space_particles_stay_behind() {
    let mut scene = Scene::new("Particles");
    let e = scene.spawn("FX", None);
    let mut s = system();
    s.main.simulation_space = SimulationSpace::World;
    s.emission.rate_over_time = MinMax::constant(0.0);
    scene.add(e, s);
    with_emitter(&mut scene, e, DEFAULT_GRAVITY, |em, sys, m| {
        em.emit(sys, m, 2)
    });
    scene.set_transform(e, Transform::from_position(Vec3::new(5.0, 0.0, 0.0)));
    let b = batches(&scene);
    assert!(b[0].particles.iter().all(|p| p.position[0] == 0.0));
}

#[test]
fn particle_system_loads_from_partial_json() {
    let s: ParticleSystem =
        serde_json::from_str(r#"{"main": {"duration": 2.0}, "renderer": {"blend": "Additive"}}"#)
            .unwrap();
    assert_eq!(s.main.duration, 2.0);
    assert!(s.main.looping, "missing fields use defaults");
    assert_eq!(s.renderer.blend, ParticleBlend::Additive);
    assert_eq!(s.shape.shape, ShapeKind::Cone);
}

// ---- Round 20: curves and gradients, Noise, Collision, Trails, Texture Sheet Animation,
// Sub Emitters and the over-lifetime / by-speed modules.

/// A system emitting `n` particles at once at the origin (no shape), moving at `speed` along
/// +Z, living `life` seconds.
fn burst(n: u32, speed: f32, life: f32) -> ParticleSystem {
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(0.0);
    s.emission.bursts = vec![Burst {
        count: n,
        ..Default::default()
    }];
    s.main.start_lifetime = MinMax::constant(life);
    s.main.start_speed = MinMax::constant(speed);
    s.main.start_size = MinMax::constant(0.2);
    s.shape.enabled = true;
    s.shape.shape = ShapeKind::Cone;
    s.shape.angle = 0.0;
    s.shape.radius = 0.0;
    s
}

#[test]
fn min_max_curves_evaluate_every_mode() {
    let c = Curve::linear(0.0, 2.0);
    let mut m = MinMax::curve(3.0, c.clone());
    assert_eq!(m.mode(), CurveMode::Curve);
    assert_eq!(m.evaluate(0.5, 0.9), 3.0, "curve × multiplier, no random");
    m = MinMax::between_curves(1.0, Curve::linear(0.0, 0.0), c);
    assert_eq!(m.mode(), CurveMode::RandomBetweenTwoCurves);
    assert_eq!(m.evaluate(1.0, 0.25), 0.5);
    m = MinMax::between(2.0, 4.0);
    assert_eq!(m.evaluate(0.7, 0.5), 3.0);
    assert_eq!(m.max(), 4.0);
    for mode in CurveMode::ALL {
        m.set_mode(mode);
        assert_eq!(m.mode(), mode);
    }
    // Old files (a / b / random) still read, and constants write no curve fields.
    let old: MinMax = serde_json::from_str(r#"{"a": 1.5, "b": 2.0, "random": true}"#).unwrap();
    assert_eq!(old, MinMax::between(1.5, 2.0));
    let text = serde_json::to_string(&MinMax::constant(1.0)).unwrap();
    assert!(!text.contains("curve"), "{text}");
}

#[test]
fn min_max_colors_evaluate_every_mode() {
    let g = Gradient::from_keys(
        &[(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])],
        &[(0.0, 1.0), (1.0, 1.0)],
    );
    let mut c = MinMaxColor::between([0.0; 4], [1.0; 4]);
    assert_eq!(c.evaluate(0.3, 0.5), [0.5; 4]);
    c.set_mode(ColorMode::Gradient);
    c.gradient_a = g.clone();
    assert_eq!(c.evaluate(1.0, 0.0), [0.0, 0.0, 1.0, 1.0]);
    c.set_mode(ColorMode::RandomBetweenTwoGradients);
    c.gradient_b = Gradient::from_keys(&[(0.0, [0.0; 3])], &[(0.0, 0.0)]);
    assert_eq!(c.evaluate(0.0, 0.5), [0.5, 0.0, 0.0, 0.5]);
    assert_eq!(c.mode(), ColorMode::RandomBetweenTwoGradients);
}

#[test]
fn start_values_and_rate_follow_their_curves_over_the_cycle() {
    let mut s = system();
    s.main.duration = 1.0;
    // 0 particles / s in the first half of the cycle, 100 / s in the second.
    s.emission.rate_over_time = MinMax::curve(
        100.0,
        Curve::from_keys(&[[0.0, 0.0], [0.5, 0.0], [0.51, 1.0], [1.0, 1.0]]),
    );
    s.main.start_size = MinMax::curve(1.0, Curve::linear(1.0, 3.0));
    let mut e = started(&s);
    run(&mut e, &s, 0.45);
    assert_eq!(e.count(), 0);
    run(&mut e, &s, 0.5);
    assert!((40..=50).contains(&e.count()), "{}", e.count());
    // Born late in the cycle: start sizes from the curve's second half.
    assert!(e.particles.iter().all(|p| p.size > 1.9));
}

#[test]
fn velocity_force_and_limit_modules_shape_the_motion() {
    let mut s = burst(1, 0.0, 10.0);
    s.velocity_over_lifetime.enabled = true;
    s.velocity_over_lifetime.x = MinMax::constant(2.0);
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    let p = e.particles[0];
    assert!((p.position.x - 2.0).abs() < 0.05, "{}", p.position.x);
    assert_eq!(p.velocity, Vec3::ZERO, "the animated velocity isn't stored");

    let mut s = burst(1, 0.0, 10.0);
    s.force_over_lifetime.enabled = true;
    s.force_over_lifetime.y = MinMax::constant(4.0);
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    assert!((e.particles[0].velocity.y - 4.0).abs() < 0.1);

    let mut s = burst(1, 10.0, 10.0);
    s.limit_velocity_over_lifetime.enabled = true;
    s.limit_velocity_over_lifetime.limit = MinMax::constant(3.0);
    s.limit_velocity_over_lifetime.dampen = 0.5;
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    let speed = e.particles[0].velocity.length();
    assert!(speed < 3.05 && speed >= 3.0 - 1e-3, "{speed}");
}

#[test]
fn noise_moves_particles_deterministically() {
    let mut s = burst(50, 0.0, 10.0);
    s.shape.shape = ShapeKind::Sphere;
    s.shape.radius = 2.0;
    let mut quiet = started(&s);
    run(&mut quiet, &s, 0.5);
    s.noise.enabled = true;
    s.noise.strength = MinMax::constant(2.0);
    s.noise.frequency = 0.8;
    s.noise.scroll_speed = 0.5;
    s.noise.octaves = 2;
    s.noise.size_amount = 0.5;
    let mut a = started(&s);
    run(&mut a, &s, 0.5);
    let mut b = started(&s);
    run(&mut b, &s, 0.5);
    assert_eq!(a.particles, b.particles, "same seed, same noise");
    let moved = a
        .particles
        .iter()
        .zip(&quiet.particles)
        .map(|(x, y)| x.position.distance(y.position))
        .fold(0.0, f32::max);
    assert!(moved > 0.2, "noise moved particles: {moved}");
    assert!(
        a.particles
            .iter()
            .any(|p| (p.noise_size - 1.0).abs() > 0.01)
    );
    // Zero strength: the same motion as without noise.
    s.noise.strength = MinMax::constant(0.0);
    s.noise.size_amount = 0.0;
    let mut z = started(&s);
    run(&mut z, &s, 0.5);
    for (x, y) in z.particles.iter().zip(&quiet.particles) {
        assert!(x.position.distance(y.position) < 1e-5);
    }
}

#[test]
fn collision_planes_bounce_dampen_and_kill() {
    let mut s = burst(1, 0.0, 10.0);
    s.main.gravity_modifier = 1.0;
    s.main.simulation_space = SimulationSpace::World;
    s.collision.enabled = true;
    s.collision.planes = vec![CollisionPlane {
        point: Vec3::new(0.0, -1.0, 0.0),
        normal: Vec3::Y,
    }];
    s.collision.bounce = 0.5;
    let mut e = started(&s);
    let mut lowest = f32::MAX;
    let mut rose = false;
    let mut prev_vy = 0.0;
    for _ in 0..120 {
        e.step(&s, &Mat4::IDENTITY, DEFAULT_GRAVITY, DT);
        let p = e.particles[0];
        lowest = lowest.min(p.position.y);
        if p.velocity.y > 0.5 && prev_vy < 0.0 {
            rose = true;
        }
        prev_vy = p.velocity.y;
    }
    assert!(
        lowest >= -1.0 + 0.1 - 1e-3,
        "stays on the plane (radius 0.1): {lowest}"
    );
    assert!(rose, "bounced back up");
    // Lifetime loss and the minimum kill speed end particles at the first hit.
    s.collision.lifetime_loss = 1.0;
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    assert_eq!(e.count(), 0, "lifetime loss 1 kills on impact");
    s.collision.lifetime_loss = 0.0;
    s.collision.min_kill_speed = 100.0;
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    assert_eq!(e.count(), 0, "slower than the kill speed");
}

#[test]
fn world_collision_pushes_particles_out_of_colliders() {
    for shape in [
        ColliderShape::Sphere {
            center: Vec3::new(0.0, -2.0, 0.0),
            radius: 1.0,
        },
        ColliderShape::Box {
            center: Vec3::new(0.0, -2.0, 0.0),
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half: Vec3::splat(1.0),
        },
        ColliderShape::Capsule {
            a: Vec3::new(-1.0, -2.0, 0.0),
            b: Vec3::new(1.0, -2.0, 0.0),
            radius: 1.0,
        },
    ] {
        let mut s = burst(1, 0.0, 10.0);
        s.main.gravity_modifier = 1.0;
        s.collision.enabled = true;
        s.collision.kind = CollisionType::World;
        s.collision.bounce = 0.0;
        let colliders = [Collider { shape, layer: 0 }];
        let mut e = started(&s);
        for _ in 0..90 {
            let env = StepEnv {
                gravity: DEFAULT_GRAVITY,
                colliders: &colliders,
                emit: true,
            };
            e.step_with(&s, &Mat4::IDENTITY, env, DT);
        }
        let y = e.particles[0].position.y;
        assert!(y > -1.0 + 0.1 - 1e-3, "{shape:?}: rests on top at {y}");
        assert!(y < -0.85, "{shape:?}: fell onto it: {y}");
        // A layer the module ignores lets it through.
        s.collision.collides_with = 0b10;
        let mut e = started(&s);
        for _ in 0..90 {
            let env = StepEnv {
                gravity: DEFAULT_GRAVITY,
                colliders: &colliders,
                emit: true,
            };
            e.step_with(&s, &Mat4::IDENTITY, env, DT);
        }
        assert!(e.particles[0].position.y < -3.0, "{shape:?}: ignored layer");
    }
}

#[test]
fn scene_colliders_become_world_collision_shapes() {
    use crate::scene::{BoxCollider, MeshCollider, MeshFilter, PrimitiveMesh, SphereCollider};
    let mut scene = Scene::new("Colliders");
    let floor = scene.spawn("Floor", None);
    scene.set_transform(floor, Transform::from_position(Vec3::new(0.0, -1.0, 0.0)));
    scene.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    scene.add(floor, MeshCollider::default());
    let ball = scene.spawn("Ball", None);
    scene.add(ball, SphereCollider::default());
    let crate_ = scene.spawn("Crate", None);
    scene.add(
        crate_,
        BoxCollider {
            is_trigger: true,
            ..Default::default()
        },
    );
    assert!(world_colliders(&scene).is_empty(), "nobody needs them yet");
    let fx = scene.spawn("FX", None);
    let mut s = burst(1, 0.0, 10.0);
    s.main.gravity_modifier = 1.0;
    s.main.simulation_space = SimulationSpace::World;
    s.collision.enabled = true;
    s.collision.kind = CollisionType::World;
    s.collision.bounce = 0.0;
    scene.set_transform(fx, Transform::from_position(Vec3::new(3.0, 1.0, 0.0)));
    scene.add(fx, s);
    let colliders = world_colliders(&scene);
    assert_eq!(
        colliders.len(),
        2,
        "the plane and the ball (not the trigger)"
    );
    for _ in 0..90 {
        update(&mut scene, DT, DEFAULT_GRAVITY);
    }
    let y = scene.world.get::<&ParticleEmitter>(fx).unwrap().particles[0]
        .position
        .y;
    assert!((y + 0.9).abs() < 0.05, "rests on the floor plane: {y}");
}

#[test]
fn texture_sheet_animation_picks_frames() {
    let mut ts = TextureSheetAnimation {
        enabled: true,
        tiles_x: 4,
        tiles_y: 2,
        ..Default::default()
    };
    // Whole sheet, linear over life: frame = floor(t × 8).
    assert_eq!(ts.uv(0.0, [0.0; 3]), [0.0, 0.0, 0.25, 0.5]);
    assert_eq!(ts.uv(0.3, [0.0; 3]), [0.5, 0.0, 0.25, 0.5], "frame 2");
    assert_eq!(
        ts.uv(0.6, [0.0; 3]),
        [0.0, 0.5, 0.25, 0.5],
        "frame 4 starts row 2"
    );
    assert_eq!(
        ts.uv(1.0, [0.0; 3]),
        [0.75, 0.5, 0.25, 0.5],
        "ends on the last frame"
    );
    // Two cycles: halfway is the last frame again.
    ts.cycles = 2.0;
    assert_eq!(ts.uv(0.49, [0.0; 3]), [0.75, 0.5, 0.25, 0.5]);
    // A single fixed row, a start frame offset.
    ts.cycles = 1.0;
    ts.animation = SheetAnimation::SingleRow;
    ts.random_row = false;
    ts.row = 1;
    ts.start_frame = MinMax::constant(1.0);
    assert_eq!(ts.uv(0.0, [0.0; 3]), [0.25, 0.5, 0.25, 0.5]);
    // Random rows follow the particle's pick.
    ts.random_row = true;
    assert_eq!(ts.uv(0.0, [0.0, 0.0, 0.1])[1], 0.0);
    assert_eq!(ts.uv(0.0, [0.0, 0.0, 0.9])[1], 0.5);
}

#[test]
fn trails_follow_particles_and_fade() {
    let mut s = burst(3, 2.0, 1.0);
    s.trails.enabled = true;
    s.trails.min_vertex_distance = 0.1;
    s.trails.lifetime = MinMax::constant(0.25);
    s.trails.die_with_particles = false;
    let mut e = started(&s);
    run(&mut e, &s, 0.5);
    assert_eq!(e.trails.len(), 3);
    let t = e.trails.values().next().unwrap();
    // 2 m/s, a point every 0.1 m, points live 0.25 s: about 5 points.
    assert!((4..=7).contains(&t.points.len()), "{}", t.points.len());
    let gaps: Vec<f32> = t
        .points
        .iter()
        .zip(t.points.iter().skip(1))
        .map(|(a, b)| a.position.distance(b.position))
        .collect();
    assert!(gaps.iter().all(|g| *g >= 0.1 - 1e-4));
    let segs = crate::particles::trail_instances(&s, &e, &Mat4::IDENTITY);
    assert_eq!(segs.len(), 3 * t.points.len(), "head + points per trail");
    assert!(segs.iter().all(|p| p.params[2] == 4.0), "segments");
    // Particles die; their trails fade out after.
    run(&mut e, &s, 0.6);
    assert_eq!(e.count(), 0);
    assert!(!e.trails.is_empty(), "still fading");
    run(&mut e, &s, 0.3);
    assert!(e.trails.is_empty(), "faded");
    // Die With Particles drops them at once; Ratio 0 makes none.
    s.trails.die_with_particles = true;
    let mut e = started(&s);
    run(&mut e, &s, 1.1);
    assert!(e.trails.is_empty());
    s.trails.ratio = 0.0;
    let mut e = started(&s);
    run(&mut e, &s, 0.3);
    assert!(e.trails.is_empty());
}

#[test]
fn ribbon_trails_join_particles_in_birth_order() {
    let mut s = system();
    s.emission.rate_over_time = MinMax::constant(20.0);
    s.main.start_speed = MinMax::constant(1.0);
    s.shape.enabled = true;
    s.shape.angle = 0.0;
    s.shape.radius = 0.0;
    s.trails.enabled = true;
    s.trails.mode = TrailMode::Ribbon;
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    let n = e.count();
    let segs = crate::particles::trail_instances(&s, &e, &Mat4::IDENTITY);
    assert_eq!(segs.len(), n - 1);
    // Newest first: each segment starts nearer the emitter than it ends.
    assert!(segs.iter().all(|p| p.axis[2] > 0.0), "along +Z, head first");
}

#[test]
fn rotation_size_and_color_modules() {
    let mut s = burst(1, 4.0, 2.0);
    s.main.start_rotation = MinMax::constant(10.0);
    s.rotation_over_lifetime.enabled = true;
    s.rotation_over_lifetime.angular_velocity = MinMax::constant(90.0);
    s.size_over_lifetime.enabled = true;
    s.size_over_lifetime.size = Some(MinMax::curve(1.0, Curve::linear(1.0, 0.0)));
    s.color_by_speed.enabled = true;
    s.color_by_speed.range = [0.0, 8.0];
    s.color_by_speed.gradient = Gradient::from_keys(
        &[(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 1.0, 1.0])],
        &[(0.0, 1.0)],
    );
    s.size_by_speed.enabled = true;
    s.size_by_speed.range = [0.0, 8.0];
    let mut e = started(&s);
    run(&mut e, &s, 1.0);
    let p = e.particles[0];
    assert!((p.rotation - 100.0).abs() < 2.0, "{}", p.rotation);
    // Half its life: size × 0.5 (lifetime curve) × 0.5 (4 of 8 m/s).
    let size = particle_size(&s, &p);
    assert!((size - 0.2 * 0.5 * 0.5).abs() < 0.01, "{size}");
    let c = particle_color_srgb(&s, &p);
    assert!(
        (c[0] - 0.5).abs() < 0.02,
        "half speed: half brightness {c:?}"
    );
    // Old start / end size files keep their line.
    let old = SizeOverLifetime {
        enabled: true,
        start: 1.0,
        end: 0.25,
        size: None,
    };
    assert_eq!(old.curve().evaluate(1.0, 0.0), 0.25);
}

#[test]
fn sub_emitters_fire_on_birth_collision_and_death() {
    let mut scene = Scene::new("Sub Emitters");
    let parent = scene.spawn("Fireworks", None);
    let mut s = burst(4, 3.0, 0.3);
    s.main.simulation_space = SimulationSpace::World;
    s.main.gravity_modifier = 1.0;
    s.collision.enabled = true;
    s.collision.planes = vec![CollisionPlane {
        point: Vec3::new(0.0, -10.0, 0.0),
        normal: Vec3::Y,
    }];
    s.sub_emitters.enabled = true;
    s.sub_emitters.list = vec![
        SubEmitter {
            trigger: SubEmitterTrigger::Death,
            emitter: "Sparks".into(),
            count: 5,
            inherit_color: true,
            ..Default::default()
        },
        SubEmitter {
            trigger: SubEmitterTrigger::Birth,
            emitter: "Puffs".into(),
            count: 1,
            ..Default::default()
        },
    ];
    s.main.start_color = MinMaxColor::constant([1.0, 0.0, 0.0, 1.0]);
    scene.add(parent, s);
    let child = |scene: &mut Scene, name: &str| {
        let c = scene.spawn(name, Some(parent));
        let mut cs = system();
        cs.main.simulation_space = SimulationSpace::World;
        cs.main.start_lifetime = MinMax::constant(5.0);
        cs.main.play_on_awake = false;
        // Its own emission is ignored: it only emits when triggered.
        cs.emission.rate_over_time = MinMax::constant(100.0);
        scene.add(c, cs);
        c
    };
    let sparks = child(&mut scene, "Sparks");
    let puffs = child(&mut scene, "Puffs");
    let count = |scene: &Scene, e| {
        scene
            .world
            .get::<&ParticleEmitter>(e)
            .map_or(0, |em| em.count())
    };
    update(&mut scene, DT, DEFAULT_GRAVITY);
    assert_eq!(count(&scene, puffs), 4, "one puff per birth");
    assert_eq!(count(&scene, sparks), 0);
    for _ in 0..30 {
        update(&mut scene, DT, DEFAULT_GRAVITY);
    }
    assert_eq!(count(&scene, parent), 0, "parents died");
    assert_eq!(count(&scene, sparks), 20, "five sparks per death");
    assert_eq!(count(&scene, puffs), 4, "no emission of their own");
    let em = scene.world.get::<&ParticleEmitter>(sparks).unwrap();
    // Sparks start red (inherited colour) where their parents died, about 0.9 m up +Z.
    assert!(
        em.particles
            .iter()
            .all(|p| p.color[1] == 0.0 && p.color[0] > 0.9)
    );
    assert!(em.particles.iter().all(|p| p.position.z > 0.6));
    drop(em);
    // Collision trigger.
    let hit = scene.spawn("Hit", None);
    let mut s = burst(3, 0.0, 10.0);
    s.main.simulation_space = SimulationSpace::World;
    s.main.gravity_modifier = 1.0;
    s.collision.enabled = true;
    s.collision.planes = vec![CollisionPlane {
        point: Vec3::new(0.0, -0.5, 0.0),
        normal: Vec3::Y,
    }];
    s.collision.bounce = 0.0;
    s.sub_emitters.enabled = true;
    s.sub_emitters.list = vec![SubEmitter {
        trigger: SubEmitterTrigger::Collision,
        emitter: "Splash".into(),
        count: 2,
        ..Default::default()
    }];
    scene.add(hit, s);
    let splash = scene.spawn("Splash", Some(hit));
    let mut cs = system();
    cs.main.play_on_awake = false;
    cs.main.simulation_space = SimulationSpace::World;
    scene.add(splash, cs);
    for _ in 0..40 {
        update(&mut scene, DT, DEFAULT_GRAVITY);
    }
    let n = count(&scene, splash);
    assert_eq!(
        n, 6,
        "two splashes per impact (resting contact doesn't count)"
    );
    let em = scene.world.get::<&ParticleEmitter>(splash).unwrap();
    assert!(
        em.particles.iter().all(|p| p.position.y < 0.0),
        "at the floor"
    );
}

#[test]
fn renderer_modes_and_sheet_uvs_reach_the_instances() {
    let mut scene = Scene::new("Render");
    let e = scene.spawn("FX", None);
    let mut s = burst(2, 3.0, 5.0);
    s.renderer.render_mode = RenderMode::Stretched;
    s.renderer.speed_scale = 0.5;
    s.texture_sheet_animation.enabled = true;
    s.texture_sheet_animation.tiles_x = 2;
    s.texture_sheet_animation.tiles_y = 2;
    s.trails.enabled = true;
    scene.add(e, s);
    for _ in 0..10 {
        update(&mut scene, DT, DEFAULT_GRAVITY);
    }
    let b = batches(&scene);
    assert_eq!(b.len(), 2, "particles and their trails");
    let p = &b[0].particles[0];
    assert_eq!(p.params[2], 1.0, "stretched");
    assert_eq!(p.axis[3], 0.5);
    assert!((p.axis[2] - 3.0).abs() < 1e-3, "velocity along +Z");
    assert_eq!([p.uv[2], p.uv[3]], [0.5, 0.5], "a quarter of the sheet");
    assert!(
        b[1].particles.iter().all(|t| t.params[0] == 2.0),
        "soft strips"
    );
}

#[test]
fn older_scenes_keep_their_text() {
    // A system with only the original modules serialises without the new ones.
    let s = ParticleSystem::default();
    let text = serde_json::to_string(&s).unwrap();
    for name in [
        "noise",
        "collision",
        "trails",
        "sub_emitters",
        "texture_sheet_animation",
        "velocity_over_lifetime",
        "start_rotation",
        "render_mode",
        "curve",
    ] {
        assert!(!text.contains(name), "{name} in {text}");
    }
    let back: ParticleSystem = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
}
