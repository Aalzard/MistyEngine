//! String pulling: the shortest path through a chain of portals (the "simple stupid funnel"
//! algorithm), on x/z with the portal points' heights.

use glam::{Vec3, Vec3Swizzles};

use super::navmesh::cross2;

/// Signed area on x/z; negative when `c` is left of `a → b` (x right, z forward).
fn area(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    -cross2(a.xz(), b.xz(), c.xz())
}

fn same(a: Vec3, b: Vec3) -> bool {
    a.xz().distance_squared(b.xz()) < 1e-8
}

/// Corners from `portals[0]` (start, start) to the last portal (end, end). Each portal is
/// (left, right) as seen walking along the path.
pub(super) fn pull(portals: &[(Vec3, Vec3)]) -> Vec<Vec3> {
    let Some(&(start, _)) = portals.first() else {
        return Vec::new();
    };
    let mut out = vec![start];
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut left_i, mut right_i) = (0, 0);
    let mut i = 1;
    while i < portals.len() {
        let (l, r) = portals[i];
        // Tighten the right side.
        if area(apex, right, r) <= 0.0 {
            if same(apex, right) || area(apex, left, r) > 0.0 {
                right = r;
                right_i = i;
            } else {
                // Right crossed over left: left is a corner.
                out.push(left);
                apex = left;
                (left, right) = (apex, apex);
                right_i = left_i;
                i = left_i + 1;
                continue;
            }
        }
        // Tighten the left side.
        if area(apex, left, l) >= 0.0 {
            if same(apex, left) || area(apex, right, l) < 0.0 {
                left = l;
                left_i = i;
            } else {
                out.push(right);
                apex = right;
                (left, right) = (apex, apex);
                left_i = right_i;
                i = right_i + 1;
                continue;
            }
        }
        i += 1;
    }
    let end = portals[portals.len() - 1].0;
    if !out.last().is_some_and(|p| same(*p, end)) {
        out.push(end);
    } else if let Some(last) = out.last_mut() {
        *last = end;
    }
    out
}
