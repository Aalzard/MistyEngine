//! Splitting triangles into clusters of at most `MAX_TRIANGLES` connected, compact triangles.

use std::collections::HashMap;

use glam::Vec3;

use super::MAX_TRIANGLES;

/// Edge between two position ids, smaller first.
pub(super) fn edge(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

/// 30-bit Morton code of a point inside `lo..hi` (spatial order for seeds).
pub(super) fn morton(p: Vec3, lo: Vec3, hi: Vec3) -> u32 {
    let n = ((p - lo) / (hi - lo).max(Vec3::splat(1e-12))).clamp(Vec3::ZERO, Vec3::ONE);
    let spread = |v: f32| {
        let mut x = (v * 1023.0) as u32;
        x = (x | (x << 16)) & 0x0300_00ff;
        x = (x | (x << 8)) & 0x0300_f00f;
        x = (x | (x << 4)) & 0x030c_30c3;
        (x | (x << 2)) & 0x0924_9249
    };
    spread(n.x) | (spread(n.y) << 1) | (spread(n.z) << 2)
}

/// Clusters grow to this many triangles, leaving room for the scraps a greedy growth strands
/// between full clusters (handed to their neighbours afterwards).
const GROW: usize = MAX_TRIANGLES - 8;

/// Group `tris` (vertex ids; `weld[v]` = position id, `pos[v]` = position) into clusters of
/// at most `MAX_TRIANGLES` triangles each. Returns triangle indices per cluster.
///
/// Greedy: a cluster grows from a seed by adding the frontier triangle that shares the most
/// corners with it (closest to its centre on ties), so clusters stay connected and round; the
/// next seed is a triangle left on the last cluster's frontier, so no islands are left behind.
pub(super) fn build(tris: &[[u32; 3]], pos: &[Vec3], weld: &[u32]) -> Vec<Vec<usize>> {
    let n = tris.len();
    if n == 0 {
        return Vec::new();
    }
    let corner = |t: usize, k: usize| weld[tris[t][k] as usize];
    let centroid: Vec<Vec3> = tris
        .iter()
        .map(|t| (pos[t[0] as usize] + pos[t[1] as usize] + pos[t[2] as usize]) / 3.0)
        .collect();
    // Edge neighbours.
    let mut by_edge: HashMap<(u32, u32), Vec<u32>> = HashMap::with_capacity(n * 2);
    for t in 0..n {
        for k in 0..3 {
            let e = edge(corner(t, k), corner(t, (k + 1) % 3));
            by_edge.entry(e).or_default().push(t as u32);
        }
    }
    let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); n];
    for list in by_edge.values() {
        for &a in list {
            for &b in list {
                if a != b && !neighbours[a as usize].contains(&b) {
                    neighbours[a as usize].push(b);
                }
            }
        }
    }
    let (lo, hi) = centroid.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(l, h), c| (l.min(*c), h.max(*c)),
    );
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&t| morton(centroid[t], lo, hi));

    let mut owner = vec![u32::MAX; n];
    let mut frontier_stamp = vec![u32::MAX; n];
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut cursor = 0;
    let mut leftover: Vec<usize> = Vec::new();
    loop {
        let seed = match leftover.iter().copied().find(|&t| owner[t] == u32::MAX) {
            Some(t) => t,
            None => {
                while cursor < n && owner[order[cursor]] != u32::MAX {
                    cursor += 1;
                }
                if cursor == n {
                    break;
                }
                order[cursor]
            }
        };
        let id = clusters.len() as u32;
        let mut members = vec![seed];
        owner[seed] = id;
        let mut corners: Vec<u32> = (0..3).map(|k| corner(seed, k)).collect();
        let mut sum = centroid[seed];
        let mut frontier: Vec<usize> = Vec::new();
        let push_neighbours =
            |t: usize, owner: &[u32], frontier: &mut Vec<usize>, stamp: &mut [u32]| {
                for &m in &neighbours[t] {
                    let m = m as usize;
                    if owner[m] == u32::MAX && stamp[m] != id {
                        stamp[m] = id;
                        frontier.push(m);
                    }
                }
            };
        push_neighbours(seed, &owner, &mut frontier, &mut frontier_stamp);
        while members.len() < GROW {
            frontier.retain(|&t| owner[t] == u32::MAX);
            let center = sum / members.len() as f32;
            // Holes first (all three corners in the cluster), then the closest to the centre:
            // round clusters strand fewer triangles than ragged ones.
            let score = |t: usize| {
                let shared = (0..3).filter(|&k| corners.contains(&corner(t, k))).count();
                (
                    (shared == 3) as u32,
                    -(centroid[t] - center).length_squared(),
                )
            };
            let Some(best) = frontier.iter().copied().max_by(|&a, &b| {
                let (sa, da) = score(a);
                let (sb, db) = score(b);
                sa.cmp(&sb).then(da.total_cmp(&db))
            }) else {
                break;
            };
            owner[best] = id;
            members.push(best);
            sum += centroid[best];
            for k in 0..3 {
                let c = corner(best, k);
                if !corners.contains(&c) {
                    corners.push(c);
                }
            }
            push_neighbours(best, &owner, &mut frontier, &mut frontier_stamp);
        }
        frontier.retain(|&t| owner[t] == u32::MAX);
        leftover = frontier;
        clusters.push(members);
    }
    merge_small(&mut clusters, &mut owner, &neighbours);
    dissolve_small(&mut clusters, &mut owner, &neighbours);
    merge_small(&mut clusters, &mut owner, &neighbours);
    // Cluster ids index `owner` until here: drop the emptied ones only now.
    clusters.retain(|c| !c.is_empty());
    clusters
}

/// Hand the triangles of clusters under three quarters full, one at a time, to the neighbouring
/// clusters with room (the one sharing the most edges with the triangle): the gaps left
/// between round clusters end up in their neighbours instead of in small clusters.
fn dissolve_small(clusters: &mut [Vec<usize>], owner: &mut [u32], neighbours: &[Vec<u32>]) {
    let mut order: Vec<usize> = (0..clusters.len())
        .filter(|&c| !clusters[c].is_empty() && clusters[c].len() < MAX_TRIANGLES * 3 / 4)
        .collect();
    order.sort_by_key(|&c| clusters[c].len());
    for c in order {
        loop {
            let mut moved = false;
            let members = std::mem::take(&mut clusters[c]);
            let mut kept = Vec::with_capacity(members.len());
            for t in members {
                let mut best: Option<(u32, usize)> = None;
                for &m in &neighbours[t] {
                    let o = owner[m as usize];
                    if o == c as u32 || clusters[o as usize].len() >= MAX_TRIANGLES {
                        continue;
                    }
                    let shared = neighbours[t]
                        .iter()
                        .filter(|&&k| owner[k as usize] == o)
                        .count();
                    if best.is_none_or(|(b, n)| shared > n || (shared == n && o < b)) {
                        best = Some((o, shared));
                    }
                }
                match best {
                    Some((o, _)) => {
                        owner[t] = o;
                        clusters[o as usize].push(t);
                        moved = true;
                    }
                    None => kept.push(t),
                }
            }
            clusters[c] = kept;
            if !moved || clusters[c].is_empty() {
                break;
            }
        }
    }
}

/// Merge each small cluster into the edge neighbour it shares the most edges with, when both
/// fit in one cluster (smallest first, until nothing fits): fewer, fuller clusters.
fn merge_small(clusters: &mut [Vec<usize>], owner: &mut [u32], neighbours: &[Vec<u32>]) {
    loop {
        let mut merged = false;
        let mut order: Vec<usize> = (0..clusters.len())
            .filter(|&c| !clusters[c].is_empty() && clusters[c].len() < MAX_TRIANGLES)
            .collect();
        order.sort_by_key(|&c| clusters[c].len());
        for c in order {
            let size = clusters[c].len();
            if size == 0 || size >= MAX_TRIANGLES {
                continue;
            }
            let mut shared: HashMap<u32, u32> = HashMap::new();
            for &t in &clusters[c] {
                for &m in &neighbours[t] {
                    let o = owner[m as usize];
                    if o != c as u32 {
                        *shared.entry(o).or_default() += 1;
                    }
                }
            }
            let best = shared
                .into_iter()
                .filter(|(o, _)| clusters[*o as usize].len() + size <= MAX_TRIANGLES)
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
            if let Some((o, _)) = best {
                let moved = std::mem::take(&mut clusters[c]);
                for &t in &moved {
                    owner[t] = o;
                }
                clusters[o as usize].extend(moved);
                merged = true;
            }
        }
        if !merged {
            break;
        }
    }
}
