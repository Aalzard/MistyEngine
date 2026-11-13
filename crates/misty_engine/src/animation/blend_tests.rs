//! Blend tree weight maths against known values, per blend type.

use glam::Vec2;

use super::blend::*;
use super::controller::{BlendChild, BlendTree, BlendType};

fn close(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 1e-4, "{a:?} vs {b:?}");
    }
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn one_d_mixes_neighbours_and_clamps_at_the_ends() {
    let th = [0.0, 1.0, 2.0];
    close(&simple_1d(&th, 0.5), &[0.5, 0.5, 0.0]);
    close(&simple_1d(&th, 1.75), &[0.0, 0.25, 0.75]);
    close(&simple_1d(&th, -3.0), &[1.0, 0.0, 0.0]);
    close(&simple_1d(&th, 9.0), &[0.0, 0.0, 1.0]);
    // Unsorted thresholds work the same.
    close(&simple_1d(&[2.0, 0.0, 1.0], 0.5), &[0.0, 0.5, 0.5]);
    assert!(simple_1d(&[], 1.0).is_empty());
}

#[test]
fn simple_directional_splits_between_neighbour_directions_and_the_centre() {
    // Centre, forward, right, back, left.
    let pts = [
        v(0.0, 0.0),
        v(0.0, 1.0),
        v(1.0, 0.0),
        v(0.0, -1.0),
        v(-1.0, 0.0),
    ];
    close(
        &simple_directional(&pts, v(0.0, 0.0)),
        &[1.0, 0.0, 0.0, 0.0, 0.0],
    );
    close(
        &simple_directional(&pts, v(0.0, 0.5)),
        &[0.5, 0.5, 0.0, 0.0, 0.0],
    );
    close(
        &simple_directional(&pts, v(0.5, 0.5)),
        &[0.0, 0.5, 0.5, 0.0, 0.0],
    );
    close(
        &simple_directional(&pts, v(0.25, 0.25)),
        &[0.5, 0.25, 0.25, 0.0, 0.0],
    );
    close(
        &simple_directional(&pts, v(-0.3, -0.1)),
        &[0.6, 0.0, 0.0, 0.1, 0.3],
    );
    // Beyond the line between two directions: just those two, normalised.
    close(
        &simple_directional(&pts, v(2.0, 2.0)),
        &[0.0, 0.5, 0.5, 0.0, 0.0],
    );
    close(
        &simple_directional(&pts, v(0.0, 3.0)),
        &[0.0, 1.0, 0.0, 0.0, 0.0],
    );
}

#[test]
fn simple_directional_without_a_centre_normalises() {
    let pts = [v(0.0, 1.0), v(1.0, 0.0), v(0.0, -1.0), v(-1.0, 0.0)];
    close(
        &simple_directional(&pts, v(0.0, 0.5)),
        &[1.0, 0.0, 0.0, 0.0],
    );
    close(
        &simple_directional(&pts, v(0.25, 0.25)),
        &[0.5, 0.5, 0.0, 0.0],
    );
    close(&simple_directional(&pts, v(0.0, 0.0)), &[0.25; 4]);
    // Only forward and back plus the centre: sideways input has nothing to blend but the centre.
    let fb = [v(0.0, 0.0), v(0.0, 1.0), v(0.0, -1.0)];
    close(&simple_directional(&fb, v(1.0, 0.0)), &[1.0, 0.0, 0.0]);
    close(&simple_directional(&fb, v(0.5, 0.5)), &[0.5, 0.5, 0.0]);
}

/// Walk and run in the same direction (idle at the centre) blend by distance: the input
/// between them played the idle alone, and a hair to one side gave NaN weights (rest pose).
#[test]
fn simple_directional_blends_motions_in_the_same_direction_by_distance() {
    let pts = [v(0.0, 0.0), v(0.0, 1.0), v(0.0, 2.0)];
    close(&simple_directional(&pts, v(0.0, 1.5)), &[0.0, 0.5, 0.5]);
    close(&simple_directional(&pts, v(0.0, 0.5)), &[0.5, 0.5, 0.0]);
    close(&simple_directional(&pts, v(0.0, 3.0)), &[0.0, 0.0, 1.0]);
    let w = simple_directional(&pts, v(-1e-6, 1.5));
    assert!(w.iter().all(|x| x.is_finite()), "{w:?}");
    assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-3, "{w:?}");
}

#[test]
fn freeform_cartesian_uses_gradient_bands() {
    let square = [v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0), v(1.0, 1.0)];
    close(&freeform_cartesian(&square, v(0.5, 0.5)), &[0.25; 4]);
    close(
        &freeform_cartesian(&square, v(1.0, 0.0)),
        &[0.0, 1.0, 0.0, 0.0],
    );
    // Halfway along an edge: the two ends share it.
    close(
        &freeform_cartesian(&square, v(0.5, 0.0)),
        &[0.5, 0.5, 0.0, 0.0],
    );
    let pair = [v(0.0, 0.0), v(2.0, 0.0)];
    close(&freeform_cartesian(&pair, v(0.5, 0.0)), &[0.75, 0.25]);
    close(&freeform_cartesian(&pair, v(5.0, 0.0)), &[0.0, 1.0]);
    // Weights always sum to 1.
    let scattered = [v(0.0, 0.0), v(1.0, 0.2), v(-0.7, 0.9), v(0.3, -1.1)];
    for p in [v(0.1, 0.3), v(-2.0, 0.5), v(0.9, -0.9)] {
        let s: f32 = freeform_cartesian(&scattered, p).iter().sum();
        assert!((s - 1.0).abs() < 1e-4, "{s}");
    }
}

#[test]
fn freeform_directional_blends_by_angle_and_speed() {
    // Idle, walk, run (forward), strafe right, strafe left, back.
    let pts = [
        v(0.0, 0.0),
        v(0.0, 1.0),
        v(0.0, 2.0),
        v(1.0, 0.0),
        v(-1.0, 0.0),
        v(0.0, -1.0),
    ];
    for (i, p) in pts.iter().enumerate() {
        let w = freeform_directional(&pts, *p);
        assert!(
            (w[i] - 1.0).abs() < 1e-4,
            "motion {i} alone at its point: {w:?}"
        );
    }
    // Between walk and run on the same direction: half each.
    close(
        &freeform_directional(&pts, v(0.0, 1.5)),
        &[0.0, 0.5, 0.5, 0.0, 0.0, 0.0],
    );
    // Between idle and walk.
    close(
        &freeform_directional(&pts, v(0.0, 0.5)),
        &[0.5, 0.5, 0.0, 0.0, 0.0, 0.0],
    );
    // Diagonal at walking speed: forward and right share it.
    let w = freeform_directional(&pts, v(0.70710677, 0.70710677));
    assert!((w[1] - w[3]).abs() < 1e-4 && w[1] > 0.4, "{w:?}");
    let s: f32 = w.iter().sum();
    assert!((s - 1.0).abs() < 1e-4);
}

#[test]
fn direct_uses_each_parameter_as_a_weight() {
    close(&direct(&[0.2, 0.5, -1.0], false), &[0.2, 0.5, 0.0]);
    close(&direct(&[0.2, 0.5], true), &[0.2 / 0.7, 0.5 / 0.7]);
    close(&direct(&[0.0, 0.0], true), &[0.0, 0.0]);
}

#[test]
fn blend_tree_reads_its_parameters() {
    let child = |x: f32, y: f32, p: &str| BlendChild {
        position: [x, y],
        direct_parameter: p.into(),
        ..BlendChild::clip("a.anim")
    };
    let mut tree = BlendTree {
        blend_type: BlendType::SimpleDirectional2D,
        parameter: "X".into(),
        parameter_y: "Y".into(),
        children: vec![child(0.0, 0.0, "A"), child(0.0, 1.0, "B")],
        ..Default::default()
    };
    let param = |n: &str| match n {
        "X" => 0.0,
        "Y" => 0.25,
        "A" => 0.3,
        "B" => 0.9,
        _ => 0.0,
    };
    assert_eq!(tree.weights(&param), vec![(0, 0.75), (1, 0.25)]);
    tree.blend_type = BlendType::Direct;
    let w = tree.weights(&param);
    assert!((w[0].1 - 0.3).abs() < 1e-6 && (w[1].1 - 0.9).abs() < 1e-6);
    tree.blend_type = BlendType::Simple1D;
    tree.children[1].threshold = 1.0;
    assert_eq!(tree.weights(&|_| 1.0), vec![(1, 1.0)]);
    assert!(BlendType::FreeformCartesian2D.is_2d() && !BlendType::Direct.is_2d());
}
