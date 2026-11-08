//! Cloth solver: stretch and energy bounds, pinned vertices, collisions, wind, welding.

use super::*;
use crate::render::mesh::Vertex;

/// An `n`×`n` quad grid `size` metres wide in the XZ plane at height `y` (or hanging in XY when
/// `vertical`).
fn grid(n: usize, size: f32, vertical: bool) -> MeshData {
    let mut vertices = Vec::new();
    for r in 0..=n {
        for c in 0..=n {
            let (u, v) = (c as f32 / n as f32, r as f32 / n as f32);
            let p = if vertical {
                [u * size, -v * size, 0.0]
            } else {
                [u * size - size * 0.5, 0.0, v * size - size * 0.5]
            };
            vertices.push(Vertex {
                position: p,
                normal: [0.0, 1.0, 0.0],
                uv: [u, v],
                uv2: [0.0; 2],
                tangent: [1.0, 0.0, 0.0, 1.0],
            });
        }
    }
    let w = (n + 1) as u32;
    let mut indices = Vec::new();
    for r in 0..n as u32 {
        for c in 0..n as u32 {
            let a = r * w + c;
            indices.extend_from_slice(&[a, a + w, a + 1, a + 1, a + w, a + w + 1]);
        }
    }
    MeshData { vertices, indices }
}

/// Max distances pinning the top row of a vertical grid.
fn top_row_pinned(n: usize) -> Vec<f32> {
    (0..(n + 1) * (n + 1))
        .map(|i| if i <= n { 0.0 } else { -1.0 })
        .collect()
}

fn rest_targets(t: &Topology, offset: Vec3) -> Vec<Vec3> {
    t.rest.iter().map(|p| *p + offset).collect()
}

fn params() -> StepParams {
    StepParams {
        substeps: 8,
        ..Default::default()
    }
}

#[test]
fn welding_merges_seam_vertices_and_builds_constraints() {
    let mut mesh = grid(4, 1.0, false);
    // Split the grid along a seam: a copy of vertex 2 used by one triangle.
    let copy = mesh.vertices[2];
    mesh.vertices.push(copy);
    let last = (mesh.vertices.len() - 1) as u32;
    if let Some(i) = mesh.indices.iter().position(|i| *i == 2) {
        mesh.indices[i] = last;
    }
    let t = Topology::build(&mesh, &[], Vec3::ONE);
    assert_eq!(t.particle_count(), 25, "the copy welds onto vertex 2");
    assert_eq!(t.vertex_particle[last as usize], t.vertex_particle[2]);
    // A 4×4 grid: 40 edges along the sides plus 16 diagonals.
    assert_eq!(t.stretch.len(), 56);
    assert!(!t.bend.is_empty());
    assert!(t.tethers.is_empty(), "nothing pinned: no tethers");
    assert!(t.inv_mass.iter().all(|w| *w > 0.0));
}

#[test]
fn pinned_vertices_follow_their_targets_exactly() {
    let n = 8;
    let t = Topology::build(&grid(n, 1.0, true), &top_row_pinned(n), Vec3::ONE);
    let mut targets = rest_targets(&t, Vec3::ZERO);
    let mut s = State::new(&targets);
    for frame in 0..120 {
        let from = targets.clone();
        // The pinned row moves in a circle (the character carrying the cape walks around).
        let a = frame as f32 * 0.05;
        let offset = Vec3::new(a.cos() - 1.0, 0.0, a.sin());
        targets = rest_targets(&t, offset);
        step(&t, &mut s, &from, &targets, &[], &params());
    }
    for (i, target) in targets.iter().enumerate() {
        if t.inv_mass[i] == 0.0 {
            assert!((s.x[i] - *target).length() < 1e-5, "pinned {i} moved off");
        }
    }
    assert!(s.x.iter().all(|p| p.is_finite()));
}

#[test]
fn hanging_cloth_stays_within_stretch_and_settles() {
    let n = 12;
    let t = Topology::build(&grid(n, 1.2, true), &top_row_pinned(n), Vec3::ONE);
    assert!(!t.tethers.is_empty());
    let targets = rest_targets(&t, Vec3::ZERO);
    let mut s = State::new(&targets);
    // Start it swinging: the free part pushed sideways.
    for (v, w) in s.v.iter_mut().zip(&t.inv_mass) {
        if *w > 0.0 {
            *v = Vec3::new(0.0, 0.0, 2.0);
        }
    }
    let mut energy = Vec::new();
    let p = StepParams {
        damping: 0.6,
        ..params()
    };
    for _ in 0..360 {
        step(&t, &mut s, &targets, &targets, &[], &p);
        assert!(s.max_stretch(&t) < 1.1, "stretch {}", s.max_stretch(&t));
        energy.push(s.kinetic_energy(&t));
    }
    let late = energy[300..].iter().copied().fold(0.0, f32::max);
    let peak = energy.iter().copied().fold(0.0, f32::max);
    assert!(late < peak * 0.2, "damped down: late {late}, peak {peak}");
    // Hangs below its pins.
    let lowest = s.x.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!(lowest < -1.0 && lowest > -1.35, "{lowest}");
}

#[test]
fn energy_never_grows_without_forces() {
    let n = 10;
    let t = Topology::build(&grid(n, 1.0, false), &[], Vec3::ONE);
    let targets = rest_targets(&t, Vec3::ZERO);
    let mut s = State::new(&targets);
    for (i, v) in s.v.iter_mut().enumerate() {
        *v = Vec3::new((i as f32 * 0.7).sin(), (i as f32 * 1.3).cos(), 0.3) * 0.5;
    }
    let p = StepParams {
        gravity: Vec3::ZERO,
        damping: 0.0,
        substeps: 8,
        ..Default::default()
    };
    let start = s.kinetic_energy(&t);
    for _ in 0..120 {
        step(&t, &mut s, &targets, &targets, &[], &p);
        let e = s.kinetic_energy(&t);
        assert!(e <= start * 1.02, "energy grew: {start} → {e}");
    }
    assert!(
        s.kinetic_energy(&t) < start,
        "constraint projection only loses energy"
    );
}

#[test]
fn a_flat_cloth_at_rest_stays_at_rest() {
    let t = Topology::build(&grid(6, 1.0, false), &[], Vec3::ONE);
    let targets = rest_targets(&t, Vec3::ZERO);
    let mut s = State::new(&targets);
    let p = StepParams {
        gravity: Vec3::ZERO,
        ..params()
    };
    for _ in 0..60 {
        step(&t, &mut s, &targets, &targets, &[], &p);
    }
    for (a, b) in s.x.iter().zip(&targets) {
        assert!((*a - *b).length() < 1e-5);
    }
}

#[test]
fn colliders_push_the_cloth_out() {
    let n = 16;
    let t = Topology::build(&grid(n, 2.0, false), &[], Vec3::ONE);
    let targets = rest_targets(&t, Vec3::new(0.0, 1.0, 0.0));
    let sphere = Shape::Sphere {
        center: Vec3::ZERO,
        radius: 0.5,
    };
    let capsule = Shape::Capsule {
        a: Vec3::new(-1.5, -0.2, 0.8),
        b: Vec3::new(1.5, -0.2, 0.8),
        radius: 0.2,
    };
    let p = StepParams {
        thickness: 0.02,
        ..params()
    };
    let mut s = State::new(&targets);
    for _ in 0..180 {
        step(&t, &mut s, &targets, &targets, &[sphere, capsule], &p);
        for x in &s.x {
            assert!(
                x.length() >= 0.52 - 1e-3,
                "inside the sphere: {} from its centre",
                x.length()
            );
        }
    }
    // Draped: the middle rests on top of the sphere, the edges hang lower.
    let top = s.x.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    assert!((top - 0.52).abs() < 0.05, "{top}");
    let low = s.x.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!(low < 0.0, "{low}");
    // And nothing is inside the capsule either.
    for x in &s.x {
        let d = Vec3::new(0.0, x.y + 0.2, x.z - 0.8).length();
        if x.x.abs() < 1.5 {
            assert!(d >= 0.22 - 1e-3, "inside the capsule: {d}");
        }
    }
}

#[test]
fn max_distance_keeps_vertices_near_the_skinned_mesh() {
    let n = 6;
    let mut md = top_row_pinned(n);
    for d in md.iter_mut().skip(n + 1) {
        *d = 0.1;
    }
    let t = Topology::build(&grid(n, 1.0, true), &md, Vec3::ONE);
    let targets = rest_targets(&t, Vec3::ZERO);
    let mut s = State::new(&targets);
    for _ in 0..120 {
        step(&t, &mut s, &targets, &targets, &[], &params());
    }
    for (i, x) in s.x.iter().enumerate() {
        assert!((*x - targets[i]).length() <= 0.1 + 1e-4, "vertex {i}");
    }
}

#[test]
fn wind_blows_a_flag_downwind() {
    let n = 10;
    let mut md = vec![-1.0; (n + 1) * (n + 1)];
    // Pinned along the left edge (the pole).
    for r in 0..=n {
        md[r * (n + 1)] = 0.0;
    }
    let t = Topology::build(&grid(n, 1.0, true), &md, Vec3::ONE);
    let targets = rest_targets(&t, Vec3::ZERO);
    let mut s = State::new(&targets);
    let p = StepParams {
        wind: WindInput {
            velocity: Vec3::new(0.0, 0.0, 6.0),
            turbulence: 0.5,
            pulse_magnitude: 0.5,
            pulse_frequency: 0.5,
        },
        ..params()
    };
    for f in 0..180 {
        let p = StepParams {
            time: f as f32 / 60.0,
            ..p.clone()
        };
        step(&t, &mut s, &targets, &targets, &[], &p);
    }
    let mean_z = s.x.iter().map(|x| x.z).sum::<f32>() / s.x.len() as f32;
    assert!(mean_z > 0.2, "pushed downwind: {mean_z}");
    assert!(s.max_stretch(&t) < 1.15);
}

#[test]
fn self_collision_keeps_layers_apart() {
    let t = Topology::build(&grid(2, 1.0, false), &[], Vec3::ONE);
    let mut x = t.rest.clone();
    // Fold: the far row lands on the near row.
    let n = 3;
    for c in 0..n {
        x[2 * n + c] = x[c] + Vec3::new(0.0, 0.001, 0.0);
    }
    self_collide(&t, &mut x, 0.03);
    for c in 0..n {
        let d = (x[2 * n + c] - x[c]).length();
        assert!(d >= 0.03 - 1e-5, "{d}");
    }
}

#[test]
fn wind_field_is_smooth_and_follows_its_direction() {
    let w = WindInput {
        velocity: Vec3::new(4.0, 0.0, 0.0),
        turbulence: 0.0,
        pulse_magnitude: 0.0,
        pulse_frequency: 1.0,
    };
    assert_eq!(
        w.at(Vec3::new(1.0, 2.0, 3.0), 0.7),
        Vec3::new(4.0, 0.0, 0.0)
    );
    assert_eq!(WindInput::default().at(Vec3::ONE, 1.0), Vec3::ZERO);
}
