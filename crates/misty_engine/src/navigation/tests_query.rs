//! Query tests: funnel output, path optimality against an exact reference, area costs and
//! masks, off-mesh links, raycasts, nearest points and walls.

use glam::{Vec2, Vec3, Vec3Swizzles};

use super::navmesh::OffMeshLink;
use super::tests::{block, floor};
use super::*;
use crate::scene::Scene;

fn v(x: f32, z: f32) -> Vec3 {
    Vec3::new(x, 0.0, z)
}

/// String pulling bends only at the portal ends the straight line can't pass.
#[test]
fn funnel_bends_at_portal_ends() {
    // Walking +z through a portal at z = 2 (x -1..1), then +x through one at x = 2 (z 3..5).
    // Seen walking, (left, right): facing +z left is -x; facing +x left is +z.
    let portals = [
        (v(0.0, 0.0), v(0.0, 0.0)),
        (v(-1.0, 2.0), v(1.0, 2.0)),
        (v(2.0, 5.0), v(2.0, 3.0)),
        (v(4.0, 4.0), v(4.0, 4.0)),
    ];
    let corners = funnel::pull(&portals);
    let expect = [v(0.0, 0.0), v(1.0, 2.0), v(2.0, 3.0), v(4.0, 4.0)];
    assert_eq!(corners.len(), expect.len(), "{corners:?}");
    for (c, e) in corners.iter().zip(expect) {
        assert!(c.distance(e) < 1e-5, "{c} vs {e}");
    }
    // Wide portals: a straight line.
    let open = [
        (v(0.0, 0.0), v(0.0, 0.0)),
        (v(-5.0, 2.0), v(5.0, 2.0)),
        (v(-5.0, 4.0), v(5.0, 4.0)),
        (v(1.0, 6.0), v(1.0, 6.0)),
    ];
    assert_eq!(funnel::pull(&open).len(), 2);
    // Heights come from the portal points.
    let up = [
        (v(0.0, 0.0), v(0.0, 0.0)),
        (Vec3::new(-1.0, 1.0, 2.0), Vec3::new(0.5, 1.0, 2.0)),
        (v(3.0, 4.0), v(3.0, 4.0)),
    ];
    let c = funnel::pull(&up);
    assert_eq!(c.len(), 3, "{c:?}");
    assert!((c[1].y - 1.0).abs() < 1e-5 && (c[1].x - 0.5).abs() < 1e-5);
}

/// Exact shortest path around axis-aligned squares (visibility graph over their corners).
fn reference_length(a: Vec2, b: Vec2, squares: &[(Vec2, f32)]) -> f32 {
    let blocked = |p: Vec2, q: Vec2| {
        squares.iter().any(|&(c, h)| {
            let h = h - 1e-3;
            // Liang–Barsky against the open square.
            let d = q - p;
            let (mut t0, mut t1) = (0.0f32, 1.0f32);
            for (pp, qq) in [
                (-d.x, p.x - (c.x - h)),
                (d.x, (c.x + h) - p.x),
                (-d.y, p.y - (c.y - h)),
                (d.y, (c.y + h) - p.y),
            ] {
                if pp.abs() < 1e-9 {
                    if qq < 0.0 {
                        return false;
                    }
                } else {
                    let r = qq / pp;
                    if pp < 0.0 {
                        t0 = t0.max(r);
                    } else {
                        t1 = t1.min(r);
                    }
                }
            }
            t0 < t1 - 1e-6
        })
    };
    let mut nodes = vec![a, b];
    for &(c, h) in squares {
        let h = h + 1e-3;
        for s in [
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ] {
            nodes.push(c + s * h);
        }
    }
    let n = nodes.len();
    let mut dist = vec![f32::INFINITY; n];
    let mut done = vec![false; n];
    dist[0] = 0.0;
    for _ in 0..n {
        let Some(u) = (0..n)
            .filter(|&i| !done[i])
            .min_by(|&x, &y| dist[x].total_cmp(&dist[y]))
        else {
            break;
        };
        done[u] = true;
        for w in 0..n {
            if !done[w] && !blocked(nodes[u], nodes[w]) {
                dist[w] = dist[w].min(dist[u] + nodes[u].distance(nodes[w]));
            }
        }
    }
    dist[1]
}

/// On a grid of pillars, paths are within a few percent of the exact shortest path.
#[test]
fn paths_are_near_optimal_on_a_pillar_grid() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 30.0, 30.0);
    let mut squares = Vec::new();
    for iz in -2..=2 {
        for ix in -2..=2 {
            // Offset rows so paths have to weave.
            let c = Vec2::new(ix as f32 * 4.5 + (iz % 2) as f32 * 1.5, iz as f32 * 4.0);
            block(
                &mut s,
                Vec3::new(c.x, 1.0, c.y),
                Vec3::new(1.4, 2.0, 1.4),
                Vec3::ZERO,
            );
            // Pillar half size plus the agent radius.
            squares.push((c, 0.7 + 0.5));
        }
    }
    let m = bake::bake_scene(&s);
    let pairs = [
        (Vec2::new(-11.0, -10.0), Vec2::new(11.0, 10.0)),
        (Vec2::new(-11.0, 0.3), Vec2::new(11.0, -0.4)),
        (Vec2::new(-2.2, -11.0), Vec2::new(2.6, 11.0)),
        (Vec2::new(-11.0, 10.5), Vec2::new(8.0, -9.5)),
        (Vec2::new(0.0, -6.0), Vec2::new(0.4, 6.0)),
    ];
    for (a, b) in pairs {
        let path = m.find_path(Vec3::new(a.x, 0.0, a.y), Vec3::new(b.x, 0.0, b.y));
        assert_eq!(path.status, PathStatus::Complete);
        let len = path.length();
        let best = reference_length(a, b, &squares);
        assert!(
            len <= best * 1.06 + 0.1 && len >= best * 0.97,
            "{a} → {b}: nav mesh {len:.2} m, shortest {best:.2} m"
        );
    }
}

/// Expensive areas are walked round when that's cheaper; masked areas are never entered.
#[test]
fn area_costs_and_masks_shape_paths() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 12.0);
    let mud = s.spawn("Mud", None);
    s.add(
        mud,
        NavMeshModifierVolume {
            size: Vec3::new(4.0, 2.0, 6.0),
            area: 3,
            ..Default::default()
        },
    );
    let m = bake::bake_scene(&s);
    let (a, b) = (v(-8.0, 0.0), v(8.0, 0.0));
    let cheap = m.find_path(a, b);
    assert_eq!(cheap.corners.len(), 2, "cost 1: straight through");
    let mut costs = [1.0; AREA_COUNT];
    costs[3] = 5.0;
    let around = m.find_path_filtered(a, b, &QueryFilter::new(ALL_AREAS, costs));
    assert_eq!(around.status, PathStatus::Complete);
    assert!(around.corners.len() >= 3, "{:?}", around.corners);
    assert!(
        around.polys.iter().all(|&p| m.area_of(p) != 3),
        "never steps in the mud"
    );
    assert!(around.length() > cheap.length() + 1.0);
    // Mud cost 1.2: going round (≈ +1.4 m) costs more than wading 4 m at +20 %.
    costs[3] = 1.2;
    let wade = m.find_path_filtered(a, b, &QueryFilter::new(ALL_AREAS, costs));
    assert!(
        wade.polys.iter().any(|&p| m.area_of(p) == 3),
        "wades through"
    );
    // Mask without the mud area.
    let masked = m.find_path_filtered(a, b, &QueryFilter::new(!(1 << 3), [1.0; AREA_COUNT]));
    assert!(masked.polys.iter().all(|&p| m.area_of(p) != 3));
    assert!(
        m.nearest_filtered(
            v(0.0, 0.0),
            0.5,
            &QueryFilter::new(!(1 << 3), [1.0; AREA_COUNT])
        )
        .is_none()
    );
}

/// An off-mesh link joins two islands; the path marks the corner where the link starts.
#[test]
fn off_mesh_links_join_islands() {
    let mut s = Scene::new("T");
    floor(&mut s, -5.0, 0.0, 6.0, 6.0);
    floor(&mut s, 5.0, 0.0, 6.0, 6.0);
    let mut m = bake::bake_scene(&s);
    let (a, b) = (v(-6.0, 1.0), v(6.0, -1.0));
    assert_eq!(m.find_path(a, b).status, PathStatus::Partial);
    m.set_off_mesh_links(vec![OffMeshLink {
        start: v(-2.6, 0.0),
        end: v(2.6, 0.0),
        bidirectional: false,
        cost_modifier: -1.0,
        area: AREA_JUMP,
        ..Default::default()
    }]);
    assert!(m.off_links[0].connected());
    let path = m.find_path(a, b);
    assert_eq!(path.status, PathStatus::Complete);
    let i = path
        .links
        .iter()
        .position(|l| l.is_some())
        .expect("a link corner");
    assert!(
        path.corners[i].distance(v(-2.5, 0.0)) < 0.2,
        "{:?}",
        path.corners
    );
    assert!(
        path.corners[i + 1].distance(v(2.5, 0.0)) < 0.2,
        "{:?}",
        path.corners
    );
    assert_eq!(path.corners.len(), path.links.len());
    // One way only.
    assert_eq!(m.find_path(b, a).status, PathStatus::Partial);
}

/// Raycasts stop at walls (with the wall's normal) and run free in the open.
#[test]
fn raycasts_stop_at_walls() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
    block(
        &mut s,
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(20.0, 2.0, 0.5),
        Vec3::ZERO,
    );
    let m = bake::bake_scene(&s);
    let f = QueryFilter::default();
    let hit = m.raycast(v(1.0, -5.0), v(1.0, 5.0), &f).unwrap();
    assert!(hit.hit);
    // The wall's face is at z = -0.25, the mesh edge a radius before it.
    assert!((hit.position.z - -0.75).abs() < 0.2, "{hit:?}");
    assert!((hit.distance - 4.25).abs() < 0.2, "{hit:?}");
    assert!(
        hit.normal.z < -0.9,
        "points back towards the start: {hit:?}"
    );
    let free = m.raycast(v(-5.0, -5.0), v(5.0, -3.0), &f).unwrap();
    assert!(!free.hit);
    assert!((free.distance - Vec2::new(10.0, 2.0).length()).abs() < 1e-3);
    assert!(free.position.xz().distance(Vec2::new(5.0, -3.0)) < 1e-4);
    // Off the mesh entirely: nothing to cast from.
    assert!(
        m.raycast(Vec3::new(0.0, 30.0, 0.0), v(1.0, 1.0), &f)
            .is_none()
    );
}

/// SamplePosition finds the surface below; FindClosestEdge the nearest wall.
#[test]
fn sample_position_and_closest_edge() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 10.0, 10.0);
    let m = bake::bake_scene(&s);
    let hit = m.nearest(Vec3::new(1.0, 1.5, 2.0), 2.0).unwrap();
    assert!(hit.position.distance(v(1.0, 2.0)) < 1e-4);
    assert!((hit.distance - 1.5).abs() < 1e-4);
    assert!(
        m.nearest(Vec3::new(1.0, 5.0, 2.0), 2.0).is_none(),
        "too far"
    );
    // Outside the mesh: the closest point on its edge (the floor eroded to ±4.5).
    let out = m.nearest(v(6.0, 0.0), 3.0).unwrap();
    assert!((out.position.x - 4.5).abs() < 0.1, "{out:?}");
    let f = QueryFilter::default();
    let edge = m.closest_edge(v(3.8, 1.0), &f).unwrap();
    assert!((edge.distance - 0.7).abs() < 0.1, "{edge:?}");
    assert!((edge.position.x - 4.5).abs() < 0.1, "{edge:?}");
    assert!(edge.normal.x < -0.9, "points inwards: {edge:?}");
    // In the middle of a floor split in tiles, inner tile seams are not walls.
    let mut big = Scene::new("T");
    floor(&mut big, 0.0, 0.0, 40.0, 40.0);
    let mb = bake::bake_scene(&big);
    assert!(mb.tiles().len() > 4);
    let e = mb.closest_edge(v(0.3, 0.2), &f).unwrap();
    assert!((e.distance - 19.2).abs() < 0.3, "{e:?}");
}
