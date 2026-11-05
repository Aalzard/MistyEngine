//! Polygon triangulation by ear clipping (handles concave faces such as a stair side).

use glam::{Vec2, Vec3};

use super::polygon_normal;

/// Triangles (corner indices) of a planar polygon, keeping its winding. Convex polygons come
/// out as a fan from corner 0.
pub fn triangulate(points: &[Vec3]) -> Vec<[usize; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }
    // Project onto the polygon's plane; `normal` follows the winding.
    let normal = polygon_normal(points.iter().copied());
    let u = normal.any_orthonormal_vector();
    let v = normal.cross(u);
    let p: Vec<Vec2> = points
        .iter()
        .map(|q| Vec2::new(q.dot(u), q.dot(v)))
        .collect();
    // In (u, v) with v = n × u the polygon winds counter-clockwise: corners turn left.
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    // Corners lying on a straight edge (split points from loop cuts and subdivisions) would
    // only make zero-area triangles: leave them out, the edge they sit on covers them.
    let mut idx: Vec<usize> = (0..n)
        .filter(|&i| {
            let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            let scale = (b - a).length() * (c - b).length();
            cross(a, b, c).abs() > 1e-6 * scale.max(1e-12)
        })
        .collect();
    if idx.len() < 3 {
        idx = (0..n).collect();
    }
    let m = idx.len();
    let convex =
        (0..m).all(|k| cross(p[idx[(k + m - 1) % m]], p[idx[k]], p[idx[(k + 1) % m]]) >= 0.0);
    if convex {
        return (1..m - 1).map(|k| [idx[0], idx[k], idx[k + 1]]).collect();
    }
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (a, b, c) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            if cross(p[a], p[b], p[c]) <= 1e-9 {
                continue; // reflex or flat corner
            }
            let inside = idx
                .iter()
                .any(|&o| o != a && o != b && o != c && point_in_triangle(p[o], p[a], p[b], p[c]));
            if inside {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    // Leftovers (degenerate input): fan what remains.
    for i in 1..idx.len().saturating_sub(1) {
        out.push([idx[0], idx[i], idx[i + 1]]);
    }
    out
}

fn point_in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = (b - a).perp_dot(p - a);
    let d2 = (c - b).perp_dot(p - b);
    let d3 = (a - c).perp_dot(p - c);
    d1 >= -1e-9 && d2 >= -1e-9 && d3 >= -1e-9
}
