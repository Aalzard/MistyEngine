use glam::{Quat, Vec3};

use super::components::*;
use super::*;
use crate::scene::{MeshFilter, MeshRef, Scene};

fn wiggle(mode: TangentMode, closed: bool) -> Spline {
    let pts = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(4.0, 1.0, 3.0),
        Vec3::new(8.0, 0.0, -1.0),
        Vec3::new(12.0, 2.0, 4.0),
        Vec3::new(14.0, 0.0, 10.0),
    ];
    let knots = pts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut k = Knot::bezier(*p, Vec3::new(1.5, 0.3 * i as f32, 1.0));
            k.mode = mode;
            if mode == TangentMode::Broken || mode == TangentMode::Continuous {
                k.tangent_in = Vec3::new(0.4, 0.2, -2.0);
            }
            k
        })
        .collect();
    Spline::new(knots, closed)
}

fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
    (a - b).length() < eps
}

#[test]
fn passes_through_every_knot_in_every_mode() {
    for mode in TangentMode::ALL {
        for closed in [false, true] {
            let s = wiggle(mode, closed);
            let path = s.path();
            let ts = path.knot_ts();
            assert_eq!(
                ts.len(),
                s.knots.len() + usize::from(closed) - usize::from(closed)
            );
            for (k, t) in s.knots.iter().zip(&ts) {
                let p = path.position(*t);
                assert!(
                    close(p, k.position, 1e-3),
                    "{mode:?} {closed}: {p} vs {}",
                    k.position
                );
            }
            assert!(close(path.position(0.0), s.knots[0].position, 1e-5));
            let end = if closed {
                s.knots[0].position
            } else {
                s.knots.last().unwrap().position
            };
            assert!(close(path.position(1.0), end, 1e-3), "{mode:?} {closed}");
        }
    }
}

/// Direction (and for Mirrored, speed) on both sides of every inner knot.
#[test]
fn tangent_continuity_follows_the_mode() {
    for mode in TangentMode::ALL {
        let s = wiggle(mode, false);
        for i in 1..s.knots.len() - 1 {
            let before = s.curve(i - 1);
            let after = s.curve(i);
            let (a, b) = (before.derivative(1.0), after.derivative(0.0));
            let smooth = a.normalize().dot(b.normalize()) > 0.9999;
            match mode {
                TangentMode::AutoSmooth | TangentMode::Mirrored | TangentMode::Continuous => {
                    assert!(smooth, "{mode:?} knot {i}: {a} vs {b}")
                }
                TangentMode::Linear | TangentMode::Broken => {
                    assert!(!smooth, "{mode:?} knot {i} has a corner")
                }
            }
            if mode == TangentMode::Mirrored {
                assert!((a.length() - b.length()).abs() < 1e-4, "mirrored speed");
            }
            if mode == TangentMode::Continuous {
                assert!(
                    (a.length() - b.length()).abs() > 0.1,
                    "continuous keeps lengths"
                );
            }
        }
    }
    // Linear segments are straight lines.
    let s = wiggle(TangentMode::Linear, false);
    let c = s.curve(1);
    let mid = c.position(0.5);
    assert!(close(mid, (c.p0 + c.p3) * 0.5, 1e-4));
}

#[test]
fn arc_length_matches_numeric_integration() {
    for mode in TangentMode::ALL {
        let s = wiggle(mode, true);
        let path = s.path();
        // Simpson's rule on |B'(u)| per curve.
        let n = 2000;
        let mut exact = 0.0f64;
        for i in 0..s.curve_count() {
            let c = s.curve(i);
            let f = |u: f64| c.derivative(u as f32).length() as f64;
            let h = 1.0 / n as f64;
            let mut sum = f(0.0) + f(1.0);
            for k in 1..n {
                let w = if k % 2 == 1 { 4.0 } else { 2.0 };
                sum += w * f(k as f64 * h);
            }
            exact += sum * h / 3.0;
        }
        let err = (path.length as f64 - exact).abs() / exact;
        assert!(err < 1e-3, "{mode:?}: {} vs {exact} ({err})", path.length);
    }
}

#[test]
fn t_at_distance_is_by_arc_length() {
    let path = wiggle(TangentMode::AutoSmooth, false).path();
    for share in [0.1, 0.25, 0.5, 0.8] {
        let d = path.length * share;
        let t = path.t_at_distance(d);
        assert!((t - share).abs() < 1e-3, "{t} vs {share}");
        // Walking the curve to that point covers that distance.
        let mut walked = 0.0;
        let mut prev = path.position(0.0);
        for k in 1..=4000 {
            let p = path.position(t * k as f32 / 4000.0);
            walked += p.distance(prev);
            prev = p;
        }
        assert!((walked - d).abs() / d < 2e-3, "{walked} vs {d}");
    }
    // Closed splines wrap.
    let ring = wiggle(TangentMode::AutoSmooth, true).path();
    assert!(close(ring.position(1.25), ring.position(0.25), 1e-3));
}

#[test]
fn nearest_point_finds_the_closest_spot() {
    let path = wiggle(TangentMode::Mirrored, false).path();
    for t in [0.05, 0.3, 0.62, 0.9] {
        let on = path.position(t);
        let off = path.up(t) * 0.4 + on;
        let n = path.nearest(off);
        assert!((n.t - t).abs() < 2e-3, "{} vs {t}", n.t);
        assert!(close(n.point, on, 1e-2), "{} vs {on}", n.point);
        assert!((n.distance - 0.4).abs() < 1e-2);
    }
    // Beyond the end clamps to the last knot.
    let n = path.nearest(Vec3::new(30.0, 0.0, 30.0));
    assert!(n.t > 0.999);
}

#[test]
fn circle_curvature_up_and_frames() {
    // Four Bézier knots approximate a circle of radius 5 in the XZ plane.
    let r = 5.0;
    let k = 0.552_284_8 * r;
    let knots = [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)]
        .iter()
        .map(|(x, z)| {
            let p = Vec3::new(*x, 0.0, *z) * r;
            // Clockwise seen from above: tangent = up × radial.
            let dir = Vec3::Y.cross(p).normalize();
            Knot::bezier(p, dir * k)
        })
        .collect();
    let path = Spline::new(knots, true).path();
    assert!((path.length - std::f32::consts::TAU * r).abs() < 0.02);
    for t in [0.1, 0.33, 0.7] {
        assert!(
            (path.curvature(t) - 1.0 / r).abs() < 0.01,
            "{}",
            path.curvature(t)
        );
        assert!(close(path.up(t), Vec3::Y, 1e-4));
        assert!(path.tangent(t).dot(path.position(t)).abs() < 0.05);
        let q = path.rotation(t);
        assert!(close(q * Vec3::Z, path.tangent(t), 1e-4));
    }
    // A straight spline has no curvature.
    let line = Spline::new(vec![Knot::at(Vec3::ZERO), Knot::at(Vec3::X * 10.0)], false).path();
    assert!(line.curvature(0.5).abs() < 1e-4);
    assert!(close(line.tangent(0.5), Vec3::X, 1e-4));
}

#[test]
fn rotated_knots_turn_the_up_vector() {
    let mut a = Knot::at(Vec3::ZERO);
    let mut b = Knot::at(Vec3::Z * 10.0);
    a.rotation = Quat::from_rotation_z(0.0);
    b.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let path = Spline::new(vec![a, b], false).path();
    assert!(close(path.up(0.0), Vec3::Y, 1e-4));
    assert!(close(path.up(1.0), -Vec3::X, 1e-4), "{}", path.up(1.0));
}

#[test]
fn inserting_on_a_bezier_curve_keeps_its_shape() {
    let mut s = wiggle(TangentMode::Mirrored, false);
    let before = s.curve(1);
    let samples: Vec<Vec3> = (0..=10).map(|k| before.position(k as f32 / 10.0)).collect();
    let i = s.insert_on_curve(1, 0.4);
    assert_eq!(i, 2);
    assert_eq!(s.knots.len(), 6);
    assert!(close(s.knots[2].position, before.position(0.4), 1e-4));
    // The two new curves trace the old one.
    for (k, p) in samples.iter().enumerate() {
        let u = k as f32 / 10.0;
        let q = if u <= 0.4 {
            s.curve(1).position(u / 0.4)
        } else {
            s.curve(2).position((u - 0.4) / 0.6)
        };
        assert!(close(*p, q, 1e-3), "{u}: {p} vs {q}");
    }
}

#[test]
fn enforce_keeps_mirrored_and_continuous_tangents_aligned() {
    let mut k = Knot::bezier(Vec3::ZERO, Vec3::Z * 2.0);
    k.tangent_out = Vec3::new(1.0, 0.0, 1.0);
    k.enforce(false);
    assert_eq!(k.tangent_in, Vec3::new(-1.0, 0.0, -1.0));
    k.mode = TangentMode::Continuous;
    k.tangent_in = Vec3::new(0.0, 0.0, -3.0);
    k.enforce(true);
    assert!(close(k.tangent_out, Vec3::Z * 2.0f32.sqrt(), 1e-5));
    assert!((k.tangent_in.length() - 3.0).abs() < 1e-5);
}

fn line_container(len: f32) -> SplineContainer {
    SplineContainer::with(Spline::new(
        vec![
            Knot::at(Vec3::ZERO),
            Knot::at(Vec3::Z * len * 0.5),
            Knot::at(Vec3::Z * len),
        ],
        false,
    ))
}

#[test]
fn extruded_tube_counts_uvs_and_normals() {
    let c = line_container(10.0);
    let ex = SplineExtrude {
        radius: 0.5,
        sides: 6,
        segments_per_unit: 2.0,
        uv_tiling: 0.5,
        ..Default::default()
    };
    let mesh = extrude::build_mesh(&c, &ex);
    let rings = 21;
    let caps = 2 * (1 + 7);
    assert_eq!(mesh.vertices.len(), rings * 7 + caps);
    assert_eq!(mesh.indices.len(), (rings - 1) * 6 * 6 + 2 * 6 * 3);
    // V runs along the length (tiling per unit), U around.
    let last_ring = &mesh.vertices[(rings - 1) * 7..rings * 7];
    assert!(last_ring.iter().all(|v| (v.uv[1] - 5.0).abs() < 1e-3));
    assert!((last_ring[6].uv[0] - 1.0).abs() < 1e-6);
    // Points on the radius, normals outward, triangles facing their normals.
    for v in &mesh.vertices[..rings * 7] {
        let p = Vec3::from(v.position);
        let radial = Vec3::new(p.x, p.y, 0.0);
        assert!((radial.length() - 0.5).abs() < 1e-3);
        assert!(close(Vec3::from(v.normal), radial.normalize(), 1e-3));
    }
    for tri in mesh.indices.chunks(3) {
        let p = |i: u32| Vec3::from(mesh.vertices[i as usize].position);
        let n = Vec3::from(mesh.vertices[tri[0] as usize].normal);
        let face = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
        assert!(face.dot(n) > 0.0, "triangle faces its normal");
    }
    let no_caps = extrude::build_mesh(
        &c,
        &SplineExtrude {
            cap_ends: false,
            ..ex.clone()
        },
    );
    assert_eq!(no_caps.vertices.len(), rings * 7);
    // Half the range gives half the rings.
    let half = extrude::build_mesh(
        &c,
        &SplineExtrude {
            cap_ends: false,
            range: [0.0, 0.5],
            ..ex
        },
    );
    assert_eq!(half.vertices.len(), 11 * 7);
}

#[test]
fn road_profile_is_a_flat_strip_facing_up() {
    let c = line_container(8.0);
    let ex = SplineExtrude {
        profile: ExtrudeProfile::Road,
        width: 3.0,
        segments_per_unit: 1.0,
        ..Default::default()
    };
    let mesh = extrude::build_mesh(&c, &ex);
    assert_eq!(mesh.vertices.len(), 9 * 2);
    assert_eq!(mesh.indices.len(), 8 * 6);
    for v in &mesh.vertices {
        assert!(close(Vec3::from(v.normal), Vec3::Y, 1e-4));
        assert!((v.position[0].abs() - 1.5).abs() < 1e-4);
    }
    for tri in mesh.indices.chunks(3) {
        let p = |i: u32| Vec3::from(mesh.vertices[i as usize].position);
        let face = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
        assert!(face.y > 0.0, "road faces up");
    }
    // A closed loop has no caps and its seam ring repeats.
    let ring = SplineContainer::with(wiggle(TangentMode::AutoSmooth, true));
    let tube = extrude::build_mesh(&ring, &SplineExtrude::default());
    let first = Vec3::from(tube.vertices[0].position);
    let n = tube.vertices.len();
    assert!(close(
        first,
        Vec3::from(tube.vertices[n - 9].position),
        1e-3
    ));
}

#[test]
fn extrude_system_keys_meshes_by_content() {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Tube", None);
    scene.add(e, line_container(6.0));
    scene.add_by_name(e, "Spline Extrude");
    update(&mut scene);
    let key = scene.world.get::<&MeshFilter>(e).unwrap().mesh.clone();
    let Some(MeshRef::Asset(k)) = key.clone() else {
        panic!("extrude sets the mesh: {key:?}")
    };
    assert!(k.starts_with("runtime://spline-extrude/"));
    assert!(crate::models::mesh(key.as_ref().unwrap()).is_some());
    // Unchanged: same key. Moving a knot: a new one.
    update(&mut scene);
    assert_eq!(scene.world.get::<&MeshFilter>(e).unwrap().mesh, key);
    scene.world.get::<&mut SplineContainer>(e).unwrap().splines[0].knots[1]
        .position
        .x = 2.0;
    update(&mut scene);
    let moved = scene.world.get::<&MeshFilter>(e).unwrap().mesh.clone();
    assert_ne!(moved, key);
    // A dropped mesh is rebuilt under the same key.
    if let Some(MeshRef::Asset(k)) = &moved {
        crate::models::runtime::unregister_keys(std::slice::from_ref(k));
        update(&mut scene);
        assert!(crate::models::runtime::is_registered(k));
    }
}

#[test]
fn instantiate_by_count_and_spacing() {
    let path = line_container(10.0).path(0);
    let mut inst = SplineInstantiate {
        count: 6,
        ..Default::default()
    };
    let d = instantiate::distances(&path, &inst);
    assert_eq!(d.len(), 6);
    assert!((d[1] - 2.0).abs() < 1e-3 && (d[5] - 10.0).abs() < 1e-3);
    inst.method = InstantiateMethod::SplineDistance;
    inst.spacing = [2.5, 2.5];
    let d = instantiate::distances(&path, &inst);
    assert_eq!(d.len(), 5, "{d:?}");
    assert!(d.windows(2).all(|w| (w[1] - w[0] - 2.5).abs() < 1e-4));
    // Random spacing stays in range and repeats with the seed.
    inst.spacing = [1.0, 3.0];
    let a = instantiate::distances(&path, &inst);
    assert!(a.windows(2).all(|w| (1.0..=3.0).contains(&(w[1] - w[0]))));
    assert_eq!(a, instantiate::distances(&path, &inst));
    // A closed loop doesn't double up the first copy.
    let ring = SplineContainer::with(wiggle(TangentMode::AutoSmooth, true)).path(0);
    inst.method = InstantiateMethod::InstanceCount;
    inst.count = 4;
    let d = instantiate::distances(&ring, &inst);
    assert!((d[3] - ring.length * 0.75).abs() < 1e-3);
}

#[test]
fn instantiate_offsets_rotations_and_scales() {
    let path = line_container(10.0).path(0);
    let inst = SplineInstantiate {
        count: 3,
        position_min: Vec3::new(2.0, 0.0, 0.0),
        position_max: Vec3::new(2.0, 0.0, 0.0),
        rotation_min: Vec3::new(0.0, 90.0, 0.0),
        rotation_max: Vec3::new(0.0, 90.0, 0.0),
        scale_min: Vec3::splat(0.5),
        scale_max: Vec3::splat(2.0),
        ..Default::default()
    };
    let p = instantiate::placements(&path, &inst, Quat::IDENTITY);
    assert_eq!(p.len(), 3);
    for x in &p {
        // Along +Z the copy's right (+X offset) is world +X.
        assert!((x.position.x - 2.0).abs() < 1e-3, "{}", x.position);
        assert!(close(x.rotation * Vec3::Z, Vec3::X, 1e-3));
        assert!((0.5..=2.0).contains(&x.scale.x));
    }
}

#[test]
fn instantiate_system_rebuilds_children_on_change() {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Posts", None);
    scene.add(e, line_container(10.0));
    scene.add(
        e,
        SplineInstantiate {
            count: 4,
            ..Default::default()
        },
    );
    update(&mut scene);
    let first = instantiate::instances(&scene, e);
    assert_eq!(first.len(), 4);
    assert!((scene.world_position(first[3]).z - 10.0).abs() < 1e-3);
    // Nothing changed: the same copies stay.
    update(&mut scene);
    assert_eq!(instantiate::instances(&scene, e), first);
    scene.world.get::<&mut SplineInstantiate>(e).unwrap().count = 7;
    update(&mut scene);
    assert_eq!(instantiate::instances(&scene, e).len(), 7);
    // Other children are left alone; disabling removes the copies.
    let own = scene.spawn("Mine", Some(e));
    scene
        .world
        .get::<&mut SplineInstantiate>(e)
        .unwrap()
        .enabled = false;
    update(&mut scene);
    assert!(instantiate::instances(&scene, e).is_empty());
    assert!(scene.contains(own));
}

#[test]
fn animate_timing_wraps_by_loop_mode() {
    let mut a = SplineAnimate {
        duration: 4.0,
        ..Default::default()
    };
    a.elapsed = 1.0;
    assert!((animate::spline_t(&a, animate::normalized_time(&a, 4.0)) - 0.25).abs() < 1e-6);
    a.elapsed = 5.0;
    assert!((animate::spline_t(&a, animate::normalized_time(&a, 4.0)) - 0.25).abs() < 1e-6);
    a.loop_mode = LoopMode::PingPong;
    assert!((animate::spline_t(&a, animate::normalized_time(&a, 4.0)) - 0.75).abs() < 1e-6);
    a.loop_mode = LoopMode::Once;
    assert_eq!(
        animate::spline_t(&a, animate::normalized_time(&a, 4.0)),
        1.0
    );
    a.easing = Easing::EaseInOut;
    a.elapsed = 1.0;
    let eased = animate::spline_t(&a, animate::normalized_time(&a, 4.0));
    assert!(eased < 0.25 && eased > 0.1);
    // Speed sets the duration from the length.
    a.method = AnimateMethod::Speed;
    a.speed = 5.0;
    assert_eq!(animate::duration(&a, 20.0), 4.0);
    animate::set_normalized_time(&mut a, 0.5, 4.0);
    assert_eq!(a.elapsed, 2.0);
}

#[test]
fn animate_moves_and_stops_in_scene() {
    let mut scene = Scene::new("t");
    let road = scene.spawn("Road", None);
    scene.add(road, line_container(10.0));
    scene.set_transform(
        road,
        crate::scene::Transform::from_position(Vec3::new(5.0, 0.0, 0.0)),
    );
    let car = scene.spawn("Car", None);
    scene.add(
        car,
        SplineAnimate {
            container: Some(road),
            duration: 2.0,
            loop_mode: LoopMode::Once,
            ..Default::default()
        },
    );
    for _ in 0..30 {
        animate::update(&mut scene, 1.0 / 30.0);
    }
    let p = scene.world_position(car);
    assert!(close(p, Vec3::new(5.0, 0.0, 5.0), 0.05), "{p}");
    assert!(close(scene.world_rotation(car) * Vec3::Z, Vec3::Z, 1e-3));
    for _ in 0..60 {
        animate::update(&mut scene, 1.0 / 30.0);
    }
    let a = (*scene.world.get::<&SplineAnimate>(car).unwrap()).clone();
    assert!(!a.playing, "Once stops at the end");
    assert!(close(
        scene.world_position(car),
        Vec3::new(5.0, 0.0, 10.0),
        1e-3
    ));
    // Not playing on awake: stays put.
    let idle = scene.spawn("Idle", None);
    scene.add(
        idle,
        SplineAnimate {
            container: Some(road),
            play_on_awake: false,
            ..Default::default()
        },
    );
    animate::update(&mut scene, 1.0);
    assert!(close(
        scene.world_position(idle),
        Vec3::new(5.0, 0.0, 0.0),
        1e-4
    ));
}

#[test]
fn spline_components_round_trip_through_scene_files() {
    let mut scene = Scene::new("t");
    let road = scene.spawn("Road", None);
    let mut s = wiggle(TangentMode::Broken, true);
    s.knots[1].mode = TangentMode::Linear;
    s.knots[2].rotation = Quat::from_rotation_y(0.7);
    scene.add(
        road,
        SplineContainer {
            splines: vec![s, wiggle(TangentMode::Continuous, false)],
        },
    );
    scene.add(
        road,
        SplineExtrude {
            profile: ExtrudeProfile::Road,
            width: 5.0,
            range: [0.1, 0.9],
            ..Default::default()
        },
    );
    let car = scene.spawn("Car", None);
    scene.add(
        car,
        SplineAnimate {
            container: Some(road),
            loop_mode: LoopMode::PingPong,
            easing: Easing::EaseOut,
            ..Default::default()
        },
    );
    let posts = scene.spawn("Posts", None);
    scene.add(
        posts,
        SplineInstantiate {
            container: Some(road),
            method: InstantiateMethod::SplineDistance,
            align: AlignMode::World,
            ..Default::default()
        },
    );
    let json = scene.to_json();
    let (back, report) = Scene::from_json("t", &json).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let road2 = back.find_by_name("Road").unwrap();
    assert_eq!(
        *back.world.get::<&SplineContainer>(road2).unwrap(),
        *scene.world.get::<&SplineContainer>(road).unwrap()
    );
    let car2 = back.find_by_name("Car").unwrap();
    let anim = back.world.get::<&SplineAnimate>(car2).unwrap().clone();
    assert_eq!(anim.container, Some(road2), "reference remapped");
    assert_eq!(anim.loop_mode, LoopMode::PingPong);
    let posts2 = back.find_by_name("Posts").unwrap();
    assert_eq!(
        back.world
            .get::<&SplineInstantiate>(posts2)
            .unwrap()
            .container,
        Some(road2)
    );
    assert_eq!(back.to_json(), json);
}
