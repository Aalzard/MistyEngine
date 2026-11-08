//! Region outlines: the boundary of each region is walked along cell edges (corner points with
//! the highest neighbouring surface as height), then simplified: points where the neighbouring
//! region changes always stay, wall edges are refined until no raw point is further than the
//! maximum error from them, and long wall edges are split.

use super::heightfield::{CompactField, DIRS};
use super::regions::Regions;

/// Neighbour "region" of cells in the tile border, one per side (`BORDER_REGION - d`): kept
/// straight like a portal, and the tile's corners stay where two sides meet.
pub(super) const BORDER_REGION: u32 = u32::MAX - 1;

/// A contour corner in grid corner units with its height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Vert {
    pub x: i32,
    pub z: i32,
    pub y: f32,
    /// Region across the edge that ends at this point (raw) or starts at it (simplified).
    pub region: u32,
    /// That region has another area: the edge is refined like a wall, so a region wrapped in
    /// one other region keeps its shape.
    pub area_border: bool,
}

/// Simplified outline of one region.
#[derive(Clone, Debug)]
pub(super) struct Contour {
    pub area: u8,
    pub verts: Vec<Vert>,
    /// Unsimplified outline (tests).
    #[cfg(test)]
    pub raw: Vec<Vert>,
}

/// Simplification settings in cells.
#[derive(Clone, Copy, Debug)]
pub(super) struct Simplify {
    pub max_error: f32,
    pub max_edge_len: i32,
}

/// Region across edge `d` of cell `i` (0 = wall, [`BORDER_REGION`] - d = tile border).
fn neighbour_region(f: &CompactField, r: &Regions, i: u32, d: usize) -> u32 {
    match f.neighbour(i, d) {
        None => 0,
        Some(n) if !f.in_core(&f.cells[n as usize]) => BORDER_REGION - d as u32,
        Some(n) => r.of_cell[n as usize],
    }
}

/// Mean surface height of the cells around the corner of cell `i` that starts edge `d` (raw
/// links, so carving never changes heights and neighbouring tiles agree; on a plane the mean
/// of the four cell centres is the height at the corner).
fn corner_height(f: &CompactField, i: u32, d: usize) -> f32 {
    let dp = (d + 1) % 4;
    let raw = |i: u32, d: usize| {
        let n = f.cells[i as usize].links[d];
        (n != super::heightfield::NONE).then_some(n)
    };
    // The cell, its neighbours across d and dp, and the diagonal one (reached either way).
    let mut cells = vec![i];
    let diag_a = raw(i, d)
        .inspect(|&a| cells.push(a))
        .and_then(|a| raw(a, dp));
    let diag_b = raw(i, dp)
        .inspect(|&b| cells.push(b))
        .and_then(|b| raw(b, d));
    if let Some(c) = diag_a.or(diag_b) {
        cells.push(c);
    }
    cells.iter().map(|&c| f.cells[c as usize].y).sum::<f32>() / cells.len() as f32
}

/// Outlines of every region (one per region: monotone regions have no holes).
pub(super) fn build(f: &CompactField, r: &Regions, s: Simplify) -> Vec<Contour> {
    let n = f.cells.len();
    // Boundary edges still to walk, per cell.
    let mut flags = vec![0u8; n];
    for i in 0..n {
        let reg = r.of_cell[i];
        if reg == 0 {
            continue;
        }
        for d in 0..4 {
            if neighbour_region(f, r, i as u32, d) != reg {
                flags[i] |= 1 << d;
            }
        }
    }
    let mut done = vec![false; r.count()];
    let mut out = Vec::new();
    for i in 0..n {
        let reg = r.of_cell[i];
        if reg == 0 || flags[i] == 0 {
            continue;
        }
        let Some(d) = (0..4).find(|d| flags[i] & (1 << d) != 0) else {
            continue;
        };
        let raw = walk(f, r, i as u32, d, &mut flags);
        if done[reg as usize] || raw.len() < 3 {
            // A second outline would be a hole; monotone regions have none.
            continue;
        }
        done[reg as usize] = true;
        let verts = simplify(&raw, s);
        if verts.len() >= 3 {
            out.push(Contour {
                area: r.area[reg as usize],
                verts,
                #[cfg(test)]
                raw,
            });
        }
    }
    out
}

/// Walk the boundary from edge `d` of cell `i`, clearing the edges walked.
fn walk(
    f: &CompactField,
    r: &Regions,
    start: u32,
    start_dir: usize,
    flags: &mut [u8],
) -> Vec<Vert> {
    let (mut i, mut d) = (start, start_dir);
    let mut out = Vec::new();
    for _ in 0..1_000_000 {
        if flags[i as usize] & (1 << d) != 0 {
            let c = f.cells[i as usize];
            let (mut x, mut z) = (c.x as i32, c.z as i32);
            match d {
                0 => z += 1,
                1 => {
                    x += 1;
                    z += 1
                }
                2 => x += 1,
                _ => {}
            }
            let region = neighbour_region(f, r, i, d);
            let own = r.area[r.of_cell[i as usize] as usize];
            let area_border =
                (region as usize) < r.area.len() && region != 0 && r.area[region as usize] != own;
            out.push(Vert {
                x,
                z,
                y: corner_height(f, i, d),
                region,
                area_border,
            });
            flags[i as usize] &= !(1 << d);
            d = (d + 1) % 4;
        } else {
            // Not a boundary: the neighbour is in the same region.
            let Some(n) = f.neighbour(i, d) else {
                break;
            };
            let (a, b) = (f.cells[i as usize], f.cells[n as usize]);
            debug_assert_eq!((b.x as i32 - a.x as i32, b.z as i32 - a.z as i32), DIRS[d]);
            i = n;
            d = (d + 3) % 4;
        }
        if i == start && d == start_dir {
            break;
        }
    }
    out
}

/// Squared distance from (px, pz) to segment a..b.
fn dist_pt_seg(px: f32, pz: f32, ax: f32, az: f32, bx: f32, bz: f32) -> f32 {
    let (dx, dz) = (bx - ax, bz - az);
    let d = dx * dx + dz * dz;
    let mut t = if d > 0.0 {
        ((px - ax) * dx + (pz - az) * dz) / d
    } else {
        0.0
    };
    t = t.clamp(0.0, 1.0);
    let (qx, qz) = (ax + t * dx - px, az + t * dz - pz);
    qx * qx + qz * qz
}

/// Simplified outline of `raw` (indices into it are kept to look up the region of each edge).
pub(super) fn simplify(raw: &[Vert], s: Simplify) -> Vec<Vert> {
    let pn = raw.len();
    // (vertex, raw index)
    let mut simp: Vec<(Vert, usize)> = Vec::new();
    let connected = raw.iter().any(|v| v.region != 0);
    if connected {
        for i in 0..pn {
            let ii = (i + 1) % pn;
            if raw[i].region != raw[ii].region {
                simp.push((raw[i], i));
            }
        }
    }
    if simp.is_empty() {
        // Only walls: start from the lower-left and upper-right corners.
        let ll = (0..pn)
            .min_by_key(|&i| (raw[i].x, raw[i].z))
            .expect("contour");
        let ur = (0..pn)
            .max_by_key(|&i| (raw[i].x, raw[i].z))
            .expect("contour");
        simp.push((raw[ll], ll));
        if ur != ll {
            simp.push((raw[ur], ur));
        }
    }
    // Refine wall edges until every raw point is close enough.
    let max_err2 = s.max_error * s.max_error;
    let mut i = 0;
    while i < simp.len() {
        let ii = (i + 1) % simp.len();
        let (a, ai) = simp[i];
        let (b, bi) = simp[ii];
        // Walk the segment in a fixed order so shared edges simplify alike from both sides.
        let (mut ci, cinc, endi, (ax, az, bx, bz)) = if b.x > a.x || (b.x == a.x && b.z > a.z) {
            ((ai + 1) % pn, 1, bi, (a.x, a.z, b.x, b.z))
        } else {
            ((bi + pn - 1) % pn, pn - 1, ai, (b.x, b.z, a.x, a.z))
        };
        let mut worst: Option<(f32, usize)> = None;
        let next = raw[(ai + 1) % pn];
        if next.region == 0 || next.area_border {
            while ci != endi {
                let d = dist_pt_seg(
                    raw[ci].x as f32,
                    raw[ci].z as f32,
                    ax as f32,
                    az as f32,
                    bx as f32,
                    bz as f32,
                );
                if worst.is_none_or(|(w, _)| d > w) {
                    worst = Some((d, ci));
                }
                ci = (ci + cinc) % pn;
            }
        }
        match worst {
            Some((d, wi)) if d > max_err2 => simp.insert(i + 1, (raw[wi], wi)),
            _ => i += 1,
        }
    }
    // Split long wall edges.
    if s.max_edge_len > 0 {
        let max2 = s.max_edge_len * s.max_edge_len;
        let mut i = 0;
        while i < simp.len() && simp.len() < pn {
            let ii = (i + 1) % simp.len();
            let (a, ai) = simp[i];
            let (b, bi) = simp[ii];
            let wall = raw[(ai + 1) % pn].region == 0;
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            if wall && dx * dx + dz * dz > max2 {
                let n = if bi < ai { bi + pn - ai } else { bi - ai };
                if n > 1 {
                    let mi = if b.x > a.x || (b.x == a.x && b.z > a.z) {
                        (ai + n / 2) % pn
                    } else {
                        (ai + n.div_ceil(2)) % pn
                    };
                    simp.insert(i + 1, (raw[mi], mi));
                    continue;
                }
            }
            i += 1;
        }
    }
    // Each kept point carries the region of the edge that starts at it.
    let mut out: Vec<Vert> = simp
        .iter()
        .map(|&(v, ri)| Vert {
            region: raw[(ri + 1) % pn].region,
            ..v
        })
        .collect();
    out.dedup_by(|b, a| a.x == b.x && a.z == b.z);
    while out.len() > 1 && out[0].x == out[out.len() - 1].x && out[0].z == out[out.len() - 1].z {
        out.pop();
    }
    out
}
