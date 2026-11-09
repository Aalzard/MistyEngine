//! Terrain chunk LOD: shared index patterns and per-frame LOD selection.
//!
//! A terrain is split into square chunks of `cells` heightmap cells. Every chunk uses the same
//! index patterns (row stride = heightmap resolution) drawn with the chunk's first sample as
//! base vertex. LOD `l` skips `2^l - 1` samples; neighbouring chunks differ by at most one LOD,
//! and a chunk next to a coarser one snaps its odd edge vertices onto the coarse grid (16 edge
//! variants per LOD), so there are no cracks.

use std::ops::Range;

/// Edge bits of a pattern: the neighbour on that side is one LOD coarser.
pub const LEFT: u8 = 1;
pub const RIGHT: u8 = 2;
pub const BACK: u8 = 4;
pub const FRONT: u8 = 8;

/// Distance (in chunk sizes) covered by each LOD before the next one starts.
const LOD_DISTANCE: f32 = 1.5;

/// Cells per chunk side for a heightmap resolution.
pub fn chunk_cells(res: usize) -> usize {
    (res - 1).clamp(2, 64)
}

/// Coarsest LOD a chunk of `cells` supports (at least 2×2 cells).
pub fn max_lod(cells: usize) -> u8 {
    (cells.trailing_zeros().max(1) - 1) as u8
}

/// Index patterns of all LODs and edge variants: (indices, ranges[lod][mask]).
pub fn patterns(res: usize, cells: usize) -> (Vec<u32>, Vec<[Range<u32>; 16]>) {
    let mut indices = Vec::new();
    let mut ranges = Vec::new();
    for lod in 0..=max_lod(cells) {
        let step = 1usize << lod;
        let n = cells / step;
        let mut per_mask: [Range<u32>; 16] = Default::default();
        for (mask, range) in per_mask.iter_mut().enumerate() {
            let mask = mask as u8;
            let start = indices.len() as u32;
            let vid = |cx: usize, cz: usize| {
                let (mut cx, mut cz) = (cx, cz);
                let odd_z = cz % 2 == 1;
                let odd_x = cx % 2 == 1;
                if (cx == 0 && mask & LEFT != 0 || cx == n && mask & RIGHT != 0) && odd_z {
                    cz -= 1;
                }
                if (cz == 0 && mask & BACK != 0 || cz == n && mask & FRONT != 0) && odd_x {
                    cx -= 1;
                }
                (cz * step * res + cx * step) as u32
            };
            for cz in 0..n {
                for cx in 0..n {
                    let i00 = vid(cx, cz);
                    let i10 = vid(cx + 1, cz);
                    let i01 = vid(cx, cz + 1);
                    let i11 = vid(cx + 1, cz + 1);
                    // Same split as the collider; `cross(b - a, c - a)` points up.
                    for tri in [[i00, i01, i10], [i01, i11, i10]] {
                        if area2(res, tri) != 0 {
                            indices.extend(tri);
                        }
                    }
                }
            }
            *range = start..indices.len() as u32;
        }
        ranges.push(per_mask);
    }
    (indices, ranges)
}

/// Twice the signed area of a triangle of samples (0 = collapsed by edge snapping).
fn area2(res: usize, tri: [u32; 3]) -> i64 {
    let p = |i: u32| ((i as usize % res) as i64, (i as usize / res) as i64);
    let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// LOD per chunk from its distance (`dist(chunk)`, metres) and size, then limited so
/// neighbours differ by at most one level.
pub fn select(per_side: usize, chunk_m: f32, top: u8, dist: impl Fn(usize) -> f32) -> Vec<u8> {
    let lods: Vec<u8> = (0..per_side * per_side)
        .map(|c| {
            let d = dist(c) / (chunk_m * LOD_DISTANCE).max(1e-3);
            (d.max(1.0).log2().floor() as u8).min(top)
        })
        .collect();
    limit(per_side, lods)
}

/// `lods` made finer where needed so that neighbours differ by at most one level.
pub fn limit(per_side: usize, mut lods: Vec<u8>) -> Vec<u8> {
    loop {
        let mut changed = false;
        for z in 0..per_side {
            for x in 0..per_side {
                let mut limit = u8::MAX;
                for (nx, nz) in neighbours(per_side, x, z) {
                    limit = limit.min(lods[nz * per_side + nx] + 1);
                }
                let l = &mut lods[z * per_side + x];
                if *l > limit {
                    *l = limit;
                    changed = true;
                }
            }
        }
        if !changed {
            break lods;
        }
    }
}

/// LOD of each chunk in the shadow maps: the finest level, whatever the camera's distance, so
/// a caster's shape (a hill shadowing the ground far from it) never changes as the camera
/// moves; chunks that `receive(c)` sun shadows keep their camera LOD (`lods`) so their own
/// shadow-map surface matches the surface the camera sees (no self-shadowing between two
/// levels of detail). The other chunks next to coarse receivers step up to them one level at
/// a time, so neighbours differ by at most one level, as in the camera's pass.
pub fn shadow_lods(per_side: usize, lods: &[u8], receive: impl Fn(usize) -> bool) -> Vec<u8> {
    let fixed: Vec<bool> = (0..lods.len()).map(receive).collect();
    let mut out: Vec<u8> = (0..lods.len())
        .map(|c| if fixed[c] { lods[c] } else { 0 })
        .collect();
    loop {
        let mut changed = false;
        for z in 0..per_side {
            for x in 0..per_side {
                let c = z * per_side + x;
                if fixed[c] {
                    continue;
                }
                let need = neighbours(per_side, x, z)
                    .map(|(nx, nz)| out[nz * per_side + nx].saturating_sub(1))
                    .max()
                    .unwrap_or(0);
                if out[c] < need {
                    out[c] = need;
                    changed = true;
                }
            }
        }
        if !changed {
            break out;
        }
    }
}

fn neighbours(per_side: usize, x: usize, z: usize) -> impl Iterator<Item = (usize, usize)> {
    let n = per_side as i64;
    [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .map(move |(dx, dz)| (x as i64 + dx, z as i64 + dz))
        .filter(move |(x, z)| (0..n).contains(x) && (0..n).contains(z))
        .map(|(x, z)| (x as usize, z as usize))
}

/// Edge mask of each chunk: which neighbours are coarser.
pub fn masks(per_side: usize, lods: &[u8]) -> Vec<u8> {
    (0..per_side * per_side)
        .map(|c| {
            let (x, z) = (c % per_side, c / per_side);
            let l = lods[c];
            let coarser = |nx: usize, nz: usize| lods[nz * per_side + nx] > l;
            let mut m = 0;
            if x > 0 && coarser(x - 1, z) {
                m |= LEFT;
            }
            if x + 1 < per_side && coarser(x + 1, z) {
                m |= RIGHT;
            }
            if z > 0 && coarser(x, z - 1) {
                m |= BACK;
            }
            if z + 1 < per_side && coarser(x, z + 1) {
                m |= FRONT;
            }
            m
        })
        .collect()
}

/// Where a terrain lies, for finding the tiles next to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileGeom {
    /// World x/z of the corner.
    pub corner: [f32; 2],
    pub size: [f32; 2],
    pub per_side: usize,
}

/// Sides in mask-bit order.
const SIDES: [(u8, i32, i32); 4] = [(LEFT, -1, 0), (RIGHT, 1, 0), (BACK, 0, -1), (FRONT, 0, 1)];

/// For each terrain, the terrain sharing each whole edge (left, right, back, front): same
/// size and chunk grid, corners touching (tiles of a larger terrain).
pub fn tile_neighbours(geoms: &[Option<TileGeom>]) -> Vec<[Option<usize>; 4]> {
    let close = |a: f32, b: f32, size: f32| (a - b).abs() <= size * 1e-4 + 1e-3;
    geoms
        .iter()
        .map(|g| {
            let mut out = [None; 4];
            let Some(g) = g else {
                return out;
            };
            for (k, (_, dx, dz)) in SIDES.iter().enumerate() {
                let want = [
                    g.corner[0] + *dx as f32 * g.size[0],
                    g.corner[1] + *dz as f32 * g.size[1],
                ];
                out[k] = geoms.iter().position(|o| {
                    o.is_some_and(|o| {
                        o.per_side == g.per_side
                            && close(o.size[0], g.size[0], g.size[0])
                            && close(o.size[1], g.size[1], g.size[1])
                            && close(o.corner[0], want[0], g.size[0])
                            && close(o.corner[1], want[1], g.size[1])
                    })
                });
            }
            out
        })
        .collect()
}

/// Chunk index of the `k`th chunk along `side` of a `per`-wide grid, and of the chunk facing it
/// across that side in the neighbour.
fn edge(side: u8, per: usize, k: usize) -> (usize, usize) {
    match side {
        LEFT => (k * per, k * per + per - 1),
        RIGHT => (k * per + per - 1, k * per),
        BACK => (k, (per - 1) * per + k),
        _ => ((per - 1) * per + k, k),
    }
}

/// Make chunk LODs finer where needed so chunks across tile edges also differ by at most one
/// level (then within each tile again, until nothing changes).
pub fn limit_across(
    lods: &mut [Vec<u8>],
    geoms: &[Option<TileGeom>],
    neighbours: &[[Option<usize>; 4]],
) {
    loop {
        let mut changed = false;
        for i in 0..lods.len() {
            let Some(g) = geoms[i] else {
                continue;
            };
            let per = g.per_side;
            for (s, (bit, _, _)) in SIDES.iter().enumerate() {
                let Some(j) = neighbours[i][s] else {
                    continue;
                };
                if lods[j].len() != per * per || lods[i].len() != per * per {
                    continue;
                }
                for k in 0..per {
                    let (mine, theirs) = edge(*bit, per, k);
                    let limit = lods[j][theirs] + 1;
                    if lods[i][mine] > limit {
                        lods[i][mine] = limit;
                        changed = true;
                    }
                }
            }
            if changed {
                lods[i] = limit(per, std::mem::take(&mut lods[i]));
            }
        }
        if !changed {
            break;
        }
    }
}

/// Edge bits of terrain `i`'s chunks whose neighbour across a tile edge is coarser.
pub fn masks_across(
    i: usize,
    lods: &[Vec<u8>],
    geoms: &[Option<TileGeom>],
    neighbours: &[[Option<usize>; 4]],
) -> Vec<u8> {
    let Some(g) = geoms[i] else {
        return Vec::new();
    };
    let per = g.per_side;
    let mut out = vec![0u8; per * per];
    if lods[i].len() != per * per {
        return out;
    }
    for (s, (bit, _, _)) in SIDES.iter().enumerate() {
        let Some(j) = neighbours[i][s] else {
            continue;
        };
        if lods[j].len() != per * per {
            continue;
        }
        for k in 0..per {
            let (mine, theirs) = edge(*bit, per, k);
            if lods[j][theirs] > lods[i][mine] {
                out[mine] |= bit;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// Two tiles side by side at different distances: the LODs across their shared edge
    /// differ by at most one level, and the finer side snaps onto the coarser one.
    #[test]
    fn tiles_stitch_across_their_shared_edge() {
        let per = 4;
        let geoms = [
            Some(TileGeom {
                corner: [0.0, 0.0],
                size: [256.0, 256.0],
                per_side: per,
            }),
            Some(TileGeom {
                corner: [256.0, 0.0],
                size: [256.0, 256.0],
                per_side: per,
            }),
            // Not touching: no neighbour.
            Some(TileGeom {
                corner: [1000.0, 0.0],
                size: [256.0, 256.0],
                per_side: per,
            }),
        ];
        let n = tile_neighbours(&geoms);
        assert_eq!(n[0], [None, Some(1), None, None]);
        assert_eq!(n[1], [Some(0), None, None, None]);
        assert_eq!(n[2], [None; 4]);
        // Tile 0 all finest, tile 1 all coarse.
        let mut lods = vec![
            vec![0u8; per * per],
            vec![3u8; per * per],
            vec![3u8; per * per],
        ];
        limit_across(&mut lods, &geoms, &n);
        for k in 0..per {
            let (mine, theirs) = edge(RIGHT, per, k);
            assert!(lods[1][theirs] <= lods[0][mine] + 1, "{lods:?}");
        }
        // Within tile 1 neighbours still differ by at most one level.
        assert_eq!(limit(per, lods[1].clone()), lods[1]);
        let masks = masks_across(0, &lods, &geoms, &n);
        for k in 0..per {
            let (mine, _) = edge(RIGHT, per, k);
            assert_eq!(
                masks[mine] & RIGHT,
                RIGHT,
                "the finer edge snaps to the coarser"
            );
        }
        assert!(masks_across(1, &lods, &geoms, &n).iter().all(|m| *m == 0));
        assert_eq!(lods[2], vec![3u8; per * per], "far tile untouched");
    }
    use super::*;
    use std::collections::BTreeSet;

    const RES: usize = 129;

    /// Heightmap samples on one edge of a chunk drawn with `(lod, mask)` at `base`.
    fn edge_samples(
        lod: u8,
        mask: u8,
        base: usize,
        on_edge: impl Fn(usize, usize) -> bool,
    ) -> BTreeSet<u32> {
        let cells = chunk_cells(RES);
        let (idx, ranges) = patterns(RES, cells);
        let r = &ranges[lod as usize][mask as usize];
        idx[r.start as usize..r.end as usize]
            .iter()
            .map(|i| *i + base as u32)
            .filter(|i| on_edge(*i as usize % RES, *i as usize / RES))
            .collect()
    }

    #[test]
    fn stitched_edges_match_the_coarser_neighbour() {
        // Chunk (0, 0) at LOD 1 next to chunk (1, 0) at LOD 2: the shared column x = 64.
        let shared = |x: usize, _z: usize| x == 64;
        let fine = edge_samples(1, RIGHT, 0, shared);
        let coarse = edge_samples(2, 0, 64, shared);
        assert_eq!(fine, coarse);
        // Without stitching the fine side would have extra vertices (a T-junction crack).
        assert!(edge_samples(1, 0, 0, shared).len() > coarse.len());
        // The same along z.
        let shared = |_x: usize, z: usize| z == 64;
        assert_eq!(
            edge_samples(0, FRONT, 0, shared),
            edge_samples(1, 0, 64 * RES, shared)
        );
    }

    #[test]
    fn patterns_cover_the_whole_chunk() {
        let cells = chunk_cells(RES);
        let (idx, ranges) = patterns(RES, cells);
        let sample = |i: u32| ((i as usize % RES) as f32, (i as usize / RES) as f32);
        for (lod, per_mask) in ranges.iter().enumerate() {
            for (mask, r) in per_mask.iter().enumerate() {
                // Triangle areas add up to the chunk's area, all facing up.
                let mut area = 0.0;
                for tri in idx[r.start as usize..r.end as usize].chunks(3) {
                    let (a, b, c) = (sample(tri[0]), sample(tri[1]), sample(tri[2]));
                    let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                    assert!(cross < 0.0, "lod {lod} mask {mask}: winding");
                    area += -cross * 0.5;
                }
                assert!(
                    (area - (cells * cells) as f32).abs() < 1e-3,
                    "lod {lod} mask {mask}"
                );
            }
        }
    }

    #[test]
    fn shadow_lods_follow_the_receivers_only() {
        let n = 8;
        let far = |c: usize| (c % n) as f32 * 300.0;
        let lods = select(n, 64.0, 5, far);
        // Nothing within the shadow distance is coarse: every chunk at full detail, whatever
        // the camera's distance (so the casters can't change shape as it moves).
        let near = shadow_lods(n, &lods, |c| far(c) < 150.0);
        assert!(near.iter().all(|&l| l == 0), "{near:?}");
        let moved = select(n, 64.0, 5, |c| far(c) + 250.0);
        assert_ne!(moved, lods);
        assert_eq!(shadow_lods(n, &moved, |c| far(c) + 250.0 < 150.0), near);
        // Receivers keep their camera detail; the rest step down to full detail one level at a time.
        let wide = shadow_lods(n, &lods, |c| far(c) < 1000.0);
        for c in 0..n * n {
            if far(c) < 1000.0 {
                assert_eq!(wide[c], lods[c]);
            }
        }
        for z in 0..n {
            for x in 1..n {
                assert!(
                    wide[z * n + x - 1].abs_diff(wide[z * n + x]) <= 1,
                    "{wide:?}"
                );
            }
        }
    }

    #[test]
    fn lods_grow_with_distance_one_step_at_a_time() {
        let n = 8;
        let lods = select(n, 64.0, 5, |c| (c % n) as f32 * 300.0);
        assert_eq!(lods[0], 0);
        assert!(lods[n - 1] > 2);
        for z in 0..n {
            for x in 1..n {
                let (a, b) = (lods[z * n + x - 1], lods[z * n + x]);
                assert!(a.abs_diff(b) <= 1, "{lods:?}");
            }
        }
        let m = masks(n, &lods);
        for x in 0..n - 1 {
            assert_eq!(m[x] & RIGHT != 0, lods[x + 1] > lods[x]);
        }
    }
}
