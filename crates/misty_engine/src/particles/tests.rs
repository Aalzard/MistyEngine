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
