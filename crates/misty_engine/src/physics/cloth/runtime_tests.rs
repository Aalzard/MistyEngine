//! Cloth while playing: jobs on the background pool, the drawn vertices, colour channels.

use super::*;
use crate::scene::{MeshRef, PrimitiveMesh, Transform};

/// A Plane with a Skinned Mesh Renderer and a Cloth pinned along its +X edge.
fn plane_cloth(scene: &mut Scene) -> Entity {
    let e = scene.spawn("Cape", None);
    scene.set_transform(e, Transform::from_position(Vec3::new(0.0, 3.0, 0.0)));
    let mesh = MeshRef::Primitive(PrimitiveMesh::Plane);
    scene.add(
        e,
        SkinnedMeshRenderer {
            mesh: Some(mesh.clone()),
            ..Default::default()
        },
    );
    let data = crate::models::mesh(&mesh).unwrap();
    let max_x = data
        .vertices
        .iter()
        .map(|v| v.position[0])
        .fold(f32::MIN, f32::max);
    scene.add(
        e,
        Cloth {
            max_distances: data
                .vertices
                .iter()
                .map(|v| {
                    if v.position[0] > max_x - 1e-3 {
                        0.0
                    } else {
                        Cloth::FREE
                    }
                })
                .collect(),
            ..Default::default()
        },
    );
    e
}

fn play(scene: &mut Scene, physics: &mut Physics, frames: usize) {
    for _ in 0..frames {
        physics.update(scene, 1.0 / 60.0);
        physics.after_animation(scene, 1.0 / 60.0);
        // Tests wait for each job; the game never does.
        physics.finish_cloth(scene);
    }
}

#[test]
fn a_plane_cloth_hangs_from_its_pinned_edge() {
    let mut scene = Scene::new("t");
    let e = plane_cloth(&mut scene);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert_eq!(physics.cloth_count(), 1);
    play(&mut scene, &mut physics, 120);
    let pose = scene.world.get::<&ClothPose>(e).unwrap().clone();
    let data = crate::models::mesh(&MeshRef::Primitive(PrimitiveMesh::Plane)).unwrap();
    assert_eq!(pose.vertices.len(), data.vertices.len());
    let max_x = data
        .vertices
        .iter()
        .map(|v| v.position[0])
        .fold(f32::MIN, f32::max);
    for (v, bind) in pose.vertices.iter().zip(&data.vertices) {
        if bind.position[0] > max_x - 1e-3 {
            let d = Vec3::from(v.position) - Vec3::from(bind.position);
            assert!(d.length() < 1e-3, "pinned vertex moved {d}");
        }
    }
    // The free side hangs down (renderer space: below the plane).
    let lowest = pose
        .vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MAX, f32::min);
    assert!(lowest < -3.0, "{lowest}");
    assert!(pose.bounds.0.y <= lowest + 1e-4);
    let stats = physics.cloth_stats();
    assert_eq!(stats.cloths, 1);
    assert!(stats.particles > 100, "{stats:?}");
    assert!(stats.solve_ms > 0.0);
    // The renderer draws these vertices.
    let r = scene.world.get::<&SkinnedMeshRenderer>(e).unwrap().clone();
    let skin = crate::skinning::compute_pose(&scene, e, &r, &|x| scene.world_matrix(x)).unwrap();
    let drawn = skin.vertices.expect("cloth vertices in the pose");
    assert!(Arc::ptr_eq(&drawn.0, &pose.vertices));
    assert_eq!(skin.bounds, pose.bounds);
}

#[test]
fn the_main_thread_only_starts_jobs() {
    let mut scene = Scene::new("t");
    plane_cloth(&mut scene);
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    // The first job builds the topology too; the frame doesn't wait for it: right after
    // starting it nothing has been published yet.
    physics.update(&mut scene, 1.0 / 60.0);
    physics.after_animation(&mut scene, 1.0 / 60.0);
    let e = scene.find_by_name("Cape").unwrap();
    assert!(scene.world.get::<&ClothPose>(e).is_err(), "no result yet");
    physics.finish_cloth(&mut scene);
    assert!(
        scene.world.get::<&ClothPose>(e).is_ok(),
        "published once done"
    );
}

#[test]
fn wind_zone_blows_the_cloth() {
    let mut calm = Scene::new("t");
    let e = plane_cloth(&mut calm);
    let mut windy = Scene::new("t");
    let f = plane_cloth(&mut windy);
    let zone = windy.spawn("Wind", None);
    // Blowing towards +Z.
    windy.add(zone, crate::scene::WindZone::default());
    let mut a = Physics::build(&calm, Vec3::new(0.0, -9.81, 0.0));
    let mut b = Physics::build(&windy, Vec3::new(0.0, -9.81, 0.0));
    play(&mut calm, &mut a, 120);
    play(&mut windy, &mut b, 120);
    let mean_z = |s: &Scene, e: Entity| {
        let p = s.world.get::<&ClothPose>(e).unwrap().particles.clone();
        p.iter().map(|x| x.z).sum::<f32>() / p.len() as f32
    };
    let (still, blown) = (mean_z(&calm, e), mean_z(&windy, f));
    assert!(blown > still + 0.3, "downwind: {still} → {blown}");
}

#[test]
fn flag_colours_give_the_max_distances() {
    let dir = std::env::temp_dir().join(format!("misty_flag_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Flag.glb");
    std::fs::write(&path, flag::flag_glb()).unwrap();
    let rel = path.to_string_lossy().replace('\\', "/");
    let mesh = MeshRef::asset(&rel, 0);
    let data = crate::models::mesh(&mesh).expect("the importer reads the flag");
    let cols = flag::FLAG_COLUMNS + 1;
    assert_eq!(data.vertices.len(), cols * (flag::FLAG_ROWS + 1));
    let md = flag::max_distances_from_colors(&mesh, crate::scene::ColorChannel::Red, 0.5).unwrap();
    assert_eq!(md.len(), data.vertices.len());
    // The pole edge (x = 0 after the import's mirror) is pinned, the rest limited, whatever
    // order the import (lightmap UVs) left the vertices in.
    for (v, d) in data.vertices.iter().zip(&md) {
        let expect = if v.position[0].abs() < 1e-4 { 0.0 } else { 0.5 };
        assert_eq!(*d, expect, "vertex at {:?}", v.position);
    }
    assert!(
        data.vertices.iter().all(|v| v.position[0] >= -1e-4),
        "flag reaches +X"
    );
    let _ = cols;
    let err = flag::max_distances_from_colors(
        &MeshRef::Primitive(PrimitiveMesh::Plane),
        crate::scene::ColorChannel::Red,
        1.0,
    )
    .unwrap_err();
    assert!(err.contains("glTF"), "{err}");
}

/// Timings for the docs: `cargo test --release -p misty_engine --lib cloth_and_ragdoll_timings
/// -- --ignored --nocapture` (take the timing lock first).
#[test]
#[ignore = "timing report, run on purpose"]
fn cloth_and_ragdoll_timings() {
    use crate::physics::ragdoll::{RagdollOptions, auto_fill, create};
    use solver::{Shape, State, StepParams, Topology, WindInput};
    // Cloth: the sample flag (416 particles) and a 64×64 grid (4225), one frame at 60 Hz with
    // 240 sub-steps per second (4 per frame), a sphere collider and wind.
    for (name, cols, rows) in [("flag 24x15", 24usize, 15usize), ("grid 64x64", 64, 64)] {
        let mut vertices = Vec::new();
        for r in 0..=rows {
            for c in 0..=cols {
                vertices.push(Vertex {
                    position: [c as f32 * 0.1, -(r as f32) * 0.1, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0; 2],
                    uv2: [0.0; 2],
                    tangent: [1.0, 0.0, 0.0, 1.0],
                });
            }
        }
        let w = (cols + 1) as u32;
        let mut indices = Vec::new();
        for r in 0..rows as u32 {
            for c in 0..cols as u32 {
                let a = r * w + c;
                indices.extend_from_slice(&[a, a + w, a + 1, a + 1, a + w, a + w + 1]);
            }
        }
        let md: Vec<f32> = (0..vertices.len())
            .map(|i| if i % (cols + 1) == 0 { 0.0 } else { -1.0 })
            .collect();
        let mesh = MeshData { vertices, indices, colors: Vec::new() };
        let t0 = Instant::now();
        let topo = Topology::build(&mesh, &md, Vec3::ONE);
        let build_ms = t0.elapsed().as_secs_f32() * 1000.0;
        let targets = topo.rest.clone();
        let mut s = State::new(&targets);
        let p = StepParams {
            substeps: 4,
            wind: WindInput {
                velocity: Vec3::new(8.0, 0.0, 2.0),
                turbulence: 0.5,
                pulse_magnitude: 0.5,
                pulse_frequency: 0.5,
            },
            ..Default::default()
        };
        let shapes = [Shape::Sphere {
            center: Vec3::new(1.0, -1.0, 0.3),
            radius: 0.3,
        }];
        let frames = 300;
        let t0 = Instant::now();
        for _ in 0..frames {
            solver::step(&topo, &mut s, &targets, &targets, &shapes, &p);
        }
        let ms = t0.elapsed().as_secs_f32() * 1000.0 / frames as f32;
        println!(
            "cloth {name}: {} particles, {} stretch + {} bend constraints, build {build_ms:.2} ms, step {ms:.3} ms per frame",
            topo.particle_count(),
            topo.stretch.len(),
            topo.bend.len()
        );
    }
    // Main-thread cost of the cloth runtime for 8 flags on Planes.
    let mut scene = Scene::new("t");
    for _ in 0..8 {
        plane_cloth(&mut scene);
    }
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut main = 0.0;
    let mut worker = 0.0;
    for f in 0..240 {
        physics.update(&mut scene, 1.0 / 60.0);
        physics.after_animation(&mut scene, 1.0 / 60.0);
        if f >= 60 {
            main += physics.cloth_stats().main_thread_ms;
            worker += physics.cloth_stats().solve_ms;
        }
        // A 60 Hz frame's worth of other work.
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    physics.finish_cloth(&mut scene);
    let st = physics.cloth_stats();
    println!(
        "cloth runtime, 8 Plane cloths ({} particles): main thread {:.3} ms per frame, worker {:.3} ms per frame (all jobs), late frames {}",
        st.particles,
        main / 180.0,
        worker / 180.0,
        st.late_frames
    );
    // Ragdolls: physics step time with 1, 10 and 30 enabled ragdolls falling on a floor.
    for n in [1usize, 10, 30] {
        let mut scene = Scene::new("t");
        let floor = scene.spawn("Floor", None);
        let mut t = crate::scene::Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
        t.scale = Vec3::new(100.0, 1.0, 100.0);
        scene.set_transform(floor, t);
        scene.add(floor, crate::scene::BoxCollider::default());
        for i in 0..n {
            let root = crate::physics::ragdoll::test_skeleton(&mut scene);
            scene.set_transform(
                root,
                crate::scene::Transform::from_position(Vec3::new(
                    (i % 6) as f32 * 2.0,
                    0.0,
                    (i / 6) as f32 * 2.0,
                )),
            );
            let bones = auto_fill(&scene, root);
            create(&mut scene, root, &bones, &RagdollOptions::default()).unwrap();
            scene
                .world
                .get::<&mut crate::scene::Ragdoll>(root)
                .unwrap()
                .enabled_on_start = true;
        }
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        let steps = 150;
        let t0 = Instant::now();
        for _ in 0..steps {
            physics.step(&scene);
        }
        let step_ms = t0.elapsed().as_secs_f32() * 1000.0 / steps as f32;
        let t0 = Instant::now();
        for _ in 0..60 {
            physics.sync_out(&mut scene);
        }
        let sync_ms = t0.elapsed().as_secs_f32() * 1000.0 / 60.0;
        println!(
            "ragdolls x{n} ({} bodies): physics step {step_ms:.3} ms (50 Hz), pose write {sync_ms:.3} ms",
            n * 11
        );
    }
}
