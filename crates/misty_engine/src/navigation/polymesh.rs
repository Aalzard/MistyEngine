//! Contours → convex polygons: each simplified outline is ear-clipped into triangles (shortest
//! diagonal first, on exact integer corner coordinates), triangles of a region are merged into
//! convex polygons of up to [`MAX_VERTS`] corners (longest shared edge first), and every polygon
//! gets detail triangles: surface samples further than the detail error from it are inserted so
//! heights follow bumpy ground (terrain) inside large polygons.

use std::collections::HashMap;

use glam::{Vec2, Vec3, Vec3Swizzles};

use super::contours::{Contour, Vert};
use super::heightfield::{self, CompactField};
use super::navmesh::NavPoly;

/// Most corners of one polygon.
pub(super) const MAX_VERTS: usize = 6;
/// Most extra detail points inside a polygon.
const MAX_DETAIL_POINTS: usize = 32;
/// Most extra detail points along one polygon edge.
const MAX_EDGE_POINTS: usize = 8;

/// Grid corner (x, z) of the field → world x/z (from global indices, so neighbouring tiles
/// produce bit-identical shared edges).
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub origin: Vec2,
    pub offset: (i64, i64),
    pub cell: f32,
}

impl Frame {
    pub fn world(&self, x: i32, z: i32) -> Vec2 {
        Vec2::new(
            self.origin.x + (self.offset.0 + x as i64) as f32 * self.cell,
            self.origin.y + (self.offset.1 + z as i64) as f32 * self.cell,
        )
    }
}

/// Detail sampling in cells / world units.
#[derive(Clone, Copy, Debug)]
pub(super) struct Detail {
    pub sample_cells: i32,
    pub max_error: f32,
}

fn area2(a: &Vert, b: &Vert, c: &Vert) -> i64 {
    (b.x - a.x) as i64 * (c.z - a.z) as i64 - (b.z - a.z) as i64 * (c.x - a.x) as i64
}
fn left(a: &Vert, b: &Vert, c: &Vert) -> bool {
    area2(a, b, c) > 0
}
fn left_on(a: &Vert, b: &Vert, c: &Vert) -> bool {
    area2(a, b, c) >= 0
}
fn collinear(a: &Vert, b: &Vert, c: &Vert) -> bool {
    area2(a, b, c) == 0
}
fn same(a: &Vert, b: &Vert) -> bool {
    a.x == b.x && a.z == b.z
}

fn intersect_prop(a: &Vert, b: &Vert, c: &Vert, d: &Vert) -> bool {
    if collinear(a, b, c) || collinear(a, b, d) || collinear(c, d, a) || collinear(c, d, b) {
        return false;
    }
    (left(a, b, c) ^ left(a, b, d)) && (left(c, d, a) ^ left(c, d, b))
}

fn between(a: &Vert, b: &Vert, c: &Vert) -> bool {
    if !collinear(a, b, c) {
        return false;
    }
    if a.x != b.x {
        (a.x <= c.x && c.x <= b.x) || (a.x >= c.x && c.x >= b.x)
    } else {
        (a.z <= c.z && c.z <= b.z) || (a.z >= c.z && c.z >= b.z)
    }
}

fn intersect(a: &Vert, b: &Vert, c: &Vert, d: &Vert) -> bool {
    intersect_prop(a, b, c, d)
        || between(a, b, c)
        || between(a, b, d)
        || between(c, d, a)
        || between(c, d, b)
}

/// Ear clipping of a counter-clockwise outline; triangles as indices into `v`.
pub(super) fn triangulate(v: &[Vert]) -> Vec<[usize; 3]> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    let mut out = Vec::new();
    let n_of = |idx: &Vec<usize>| idx.len();
    // Diagonal from position i to j of the current outline that stays inside it.
    let diagonalie = |idx: &Vec<usize>, i: usize, j: usize| {
        let (d0, d1) = (&v[idx[i]], &v[idx[j]]);
        let n = idx.len();
        for k in 0..n {
            let k1 = (k + 1) % n;
            if k == i || k1 == i || k == j || k1 == j {
                continue;
            }
            let (p0, p1) = (&v[idx[k]], &v[idx[k1]]);
            if same(d0, p0) || same(d1, p0) || same(d0, p1) || same(d1, p1) {
                continue;
            }
            if intersect(d0, d1, p0, p1) {
                return false;
            }
        }
        true
    };
    let in_cone = |idx: &Vec<usize>, i: usize, j: usize| {
        let n = idx.len();
        let (pi, pj) = (&v[idx[i]], &v[idx[j]]);
        let (pn, pp) = (&v[idx[(i + 1) % n]], &v[idx[(i + n - 1) % n]]);
        if left_on(pp, pi, pn) {
            left(pi, pj, pp) && left(pj, pi, pn)
        } else {
            !(left_on(pi, pj, pn) && left_on(pj, pi, pp))
        }
    };
    let diagonal =
        |idx: &Vec<usize>, i: usize, j: usize| in_cone(idx, i, j) && diagonalie(idx, i, j);
    let mut ear: Vec<bool> = (0..idx.len())
        .map(|i| {
            let n = idx.len();
            diagonal(&idx, (i + n - 1) % n, (i + 1) % n)
        })
        .collect();
    while n_of(&idx) > 3 {
        let n = idx.len();
        let mut best: Option<(i64, usize)> = None;
        for i in 0..n {
            if !ear[i] {
                continue;
            }
            let (p0, p2) = (&v[idx[(i + n - 1) % n]], &v[idx[(i + 1) % n]]);
            let (dx, dz) = ((p2.x - p0.x) as i64, (p2.z - p0.z) as i64);
            let len = dx * dx + dz * dz;
            if best.is_none_or(|(b, _)| len < b) {
                best = Some((len, i));
            }
        }
        let Some((_, i)) = best else {
            // Self-touching outline: keep what was triangulated.
            return out;
        };
        let (i0, i2) = ((i + n - 1) % n, (i + 1) % n);
        out.push([idx[i0], idx[i], idx[i2]]);
        idx.remove(i);
        ear.remove(i);
        let n = idx.len();
        let i0 = if i0 > i { i0 - 1 } else { i0 };
        let i1 = i % n;
        for k in [i0, i1] {
            ear[k] = diagonal(&idx, (k + n - 1) % n, (k + 1) % n);
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// Merge triangles (indices into `v`, counter-clockwise) into convex polygons.
fn merge(v: &[Vert], tris: Vec<[usize; 3]>) -> Vec<Vec<usize>> {
    let mut polys: Vec<Vec<usize>> = tris.into_iter().map(|t| t.to_vec()).collect();
    let key = |i: usize| (v[i].x, v[i].z);
    loop {
        type Key = (i32, i32);
        let mut edges: HashMap<(Key, Key), usize> = HashMap::new();
        for (p, poly) in polys.iter().enumerate() {
            for k in 0..poly.len() {
                edges.insert((key(poly[k]), key(poly[(k + 1) % poly.len()])), p);
            }
        }
        // (value, a, b, edge in a, edge in b)
        let mut best: Option<(i64, usize, usize, usize, usize)> = None;
        for (a, pa) in polys.iter().enumerate() {
            let na = pa.len();
            for ea in 0..na {
                let (k0, k1) = (key(pa[ea]), key(pa[(ea + 1) % na]));
                let Some(&b) = edges.get(&(k1, k0)) else {
                    continue;
                };
                if b <= a {
                    continue;
                }
                let pb = &polys[b];
                let nb = pb.len();
                if na + nb - 2 > MAX_VERTS {
                    continue;
                }
                let Some(eb) = (0..nb).find(|&j| key(pb[j]) == k1 && key(pb[(j + 1) % nb]) == k0)
                else {
                    continue;
                };
                let convex = left(
                    &v[pa[(ea + na - 1) % na]],
                    &v[pa[ea]],
                    &v[pb[(eb + 2) % nb]],
                ) && left(
                    &v[pb[(eb + nb - 1) % nb]],
                    &v[pb[eb]],
                    &v[pa[(ea + 2) % na]],
                );
                if !convex {
                    continue;
                }
                let (dx, dz) = ((k1.0 - k0.0) as i64, (k1.1 - k0.1) as i64);
                let value = dx * dx + dz * dz;
                if best.is_none_or(|bb| value > bb.0) {
                    best = Some((value, a, b, ea, eb));
                }
            }
        }
        let Some((_, a, b, ea, eb)) = best else {
            return polys;
        };
        let (pa, pb) = (&polys[a], &polys[b]);
        let (na, nb) = (pa.len(), pb.len());
        let mut merged = Vec::with_capacity(na + nb - 2);
        for i in 0..na - 1 {
            merged.push(pa[(ea + 1 + i) % na]);
        }
        for i in 0..nb - 1 {
            merged.push(pb[(eb + 1 + i) % nb]);
        }
        polys[a] = merged;
        polys.remove(b);
    }
}

/// Signed doubled area of an outline (counter-clockwise > 0).
fn outline_area(v: &[Vert]) -> i64 {
    (0..v.len())
        .map(|i| {
            let (a, b) = (&v[i], &v[(i + 1) % v.len()]);
            a.x as i64 * b.z as i64 - b.x as i64 * a.z as i64
        })
        .sum()
}

/// Convex polygons (world space, with detail triangles) for a tile's contours.
pub(super) fn build(
    contours: &[Contour],
    f: &CompactField,
    frame: Frame,
    d: Detail,
) -> Vec<NavPoly> {
    let mut out = Vec::new();
    for c in contours {
        let mut v = c.verts.clone();
        if outline_area(&v) < 0 {
            v.reverse();
        }
        let tris = triangulate(&v);
        for poly in merge(&v, tris) {
            let vertices: Vec<Vec3> = poly
                .iter()
                .map(|&i| {
                    let p = frame.world(v[i].x, v[i].z);
                    Vec3::new(p.x, v[i].y, p.y)
                })
                .collect();
            let mut np = NavPoly::new(vertices, c.area);
            add_detail(&mut np, f, d);
            out.push(np);
        }
    }
    out
}

/// Height of triangle `t` at x/z `p` and whether `p` is inside it.
fn tri_height(t: [Vec3; 3], p: Vec2) -> (f32, bool) {
    let (a, b, c) = (t[0].xz(), t[1].xz(), t[2].xz());
    let v0 = b - a;
    let v1 = c - a;
    let v2 = p - a;
    let den = v0.perp_dot(v1);
    if den.abs() < 1e-12 {
        return (t[0].y, false);
    }
    let v = v2.perp_dot(v1) / den;
    let w = v0.perp_dot(v2) / den;
    let u = 1.0 - v - w;
    let eps = -1e-4;
    let h = t[0].y * u + t[1].y * v + t[2].y * w;
    (h, u >= eps && v >= eps && w >= eps)
}

/// Insert surface samples that deviate from the polygon's current detail surface.
fn add_detail(p: &mut NavPoly, f: &CompactField, d: Detail) {
    let n = p.vertices.len();
    p.detail_tris = (1..n - 1).map(|i| [0, i as u8, (i + 1) as u8]).collect();
    if d.sample_cells <= 0 {
        return;
    }
    add_edge_detail(p, f, d);
    let (lo, hi) = p.vertices.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), v| (lo.min(v.xz()), hi.max(v.xz())),
    );
    let step = d.sample_cells as f32 * f.cell;
    let margin = f.cell * 0.5;
    let mut samples = Vec::new();
    let mut z = lo.y + step * 0.5;
    while z < hi.y {
        let mut x = lo.x + step * 0.5;
        while x < hi.x {
            let q = Vec2::new(x, z);
            if inside_with_margin(&p.vertices, q, margin) {
                samples.push(q);
            }
            x += step;
        }
        z += step;
    }
    for _ in 0..MAX_DETAIL_POINTS {
        let mut worst: Option<(f32, Vec3, usize)> = None;
        for &q in &samples {
            let Some((t, h)) = p.detail_at(q) else {
                continue;
            };
            let Some(y) = heightfield::sample_height(f, Vec3::new(q.x, h, q.y)) else {
                continue;
            };
            let err = (y - h).abs();
            if err > d.max_error && worst.is_none_or(|w| err > w.0) {
                worst = Some((err, Vec3::new(q.x, y, q.y), t));
            }
        }
        let Some((_, point, t)) = worst else {
            break;
        };
        let k = (n + p.detail_verts.len()) as u8;
        p.detail_verts.push(point);
        let [a, b, c] = p.detail_tris[t];
        p.detail_tris[t] = [a, b, k];
        p.detail_tris.push([b, c, k]);
        p.detail_tris.push([c, a, k]);
        samples.retain(|s| s.distance_squared(point.xz()) > 1e-6);
    }
}

/// Surface height on a polygon edge at x/z `q` near height `y`: the mean of the cells either
/// side of the edge (so both polygons sharing it, even in neighbouring tiles, agree).
fn edge_height(f: &CompactField, q: Vec2, normal: Vec2, y: f32) -> Option<f32> {
    let off = normal * f.cell * 0.25;
    let a = heightfield::sample_height(f, Vec3::new(q.x + off.x, y, q.y + off.y));
    let b = heightfield::sample_height(f, Vec3::new(q.x - off.x, y, q.y - off.y));
    match (a, b) {
        (Some(a), Some(b)) => Some((a + b) * 0.5),
        (a, b) => a.or(b),
    }
}

/// Split polygon edges where the surface bends away from them (refined like a polyline, from
/// the lower endpoint, so a shared edge gets the same points from both sides).
fn add_edge_detail(p: &mut NavPoly, f: &CompactField, d: Detail) {
    let n = p.vertices.len();
    let step = d.sample_cells as f32 * f.cell;
    for k in 0..n {
        let (a, b) = (p.vertices[k], p.vertices[(k + 1) % n]);
        let (s, e) = if (a.x, a.z) <= (b.x, b.z) {
            (a, b)
        } else {
            (b, a)
        };
        let len = s.xz().distance(e.xz());
        let steps = (len / step).floor() as usize;
        if steps < 2 {
            continue;
        }
        let dir = (e - s).xz() / len;
        let normal = Vec2::new(-dir.y, dir.x);
        let samples: Vec<Option<Vec3>> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                let at = s.lerp(e, t);
                if i == 0 || i == steps {
                    return Some(at);
                }
                edge_height(f, at.xz(), normal, at.y).map(|y| Vec3::new(at.x, y, at.z))
            })
            .collect();
        let mut chosen = vec![0usize, steps];
        for _ in 0..MAX_EDGE_POINTS {
            let mut worst: Option<(f32, usize)> = None;
            for w in chosen.windows(2) {
                let (i0, i1) = (w[0], w[1]);
                let (p0, p1) = (samples[i0].expect("ends"), samples[i1].expect("ends"));
                for (i, q) in samples.iter().enumerate().take(i1).skip(i0 + 1) {
                    let Some(q) = q else {
                        continue;
                    };
                    let t = (i - i0) as f32 / (i1 - i0) as f32;
                    let err = (q.y - (p0.y + (p1.y - p0.y) * t)).abs();
                    if err > d.max_error && worst.is_none_or(|x| err > x.0) {
                        worst = Some((err, i));
                    }
                }
            }
            let Some((_, i)) = worst else {
                break;
            };
            let at = chosen.partition_point(|&c| c < i);
            chosen.insert(at, i);
        }
        for &i in &chosen[1..chosen.len() - 1] {
            let q = samples[i].expect("chosen");
            insert_on_edge(p, q);
        }
    }
}

/// Split the detail triangle whose outer edge holds `q` in two.
fn insert_on_edge(p: &mut NavPoly, q: Vec3) {
    let k = (p.vertices.len() + p.detail_verts.len()) as u8;
    for t in 0..p.detail_tris.len() {
        let tri = p.detail_tris[t];
        for e in 0..3 {
            let (u, v, w) = (tri[e], tri[(e + 1) % 3], tri[(e + 2) % 3]);
            let (pu, pv) = (
                p.detail_vertex(u as usize).xz(),
                p.detail_vertex(v as usize).xz(),
            );
            let along = pv - pu;
            let len = along.length();
            if len < 1e-6 {
                continue;
            }
            let rel = q.xz() - pu;
            let t_on = rel.dot(along) / (len * len);
            if along.perp_dot(rel).abs() / len < 1e-4 && t_on > 1e-4 && t_on < 1.0 - 1e-4 {
                p.detail_verts.push(q);
                p.detail_tris[t] = [u, k, w];
                p.detail_tris.push([k, v, w]);
                return;
            }
        }
    }
}

fn inside_with_margin(v: &[Vec3], q: Vec2, margin: f32) -> bool {
    (0..v.len()).all(|i| {
        let (a, b) = (v[i].xz(), v[(i + 1) % v.len()].xz());
        let e = b - a;
        let len = e.length();
        len < 1e-6 || e.perp_dot(q - a) / len >= margin
    })
}

impl NavPoly {
    /// Detail triangle under x/z `q` and the surface height there.
    pub(super) fn detail_at(&self, q: Vec2) -> Option<(usize, f32)> {
        let mut nearest: Option<(f32, usize, f32)> = None;
        for (i, t) in self.detail_tris.iter().enumerate() {
            let tri = t.map(|k| self.detail_vertex(k as usize));
            let (h, inside) = tri_height(tri, q);
            if inside {
                return Some((i, h));
            }
            let c = (tri[0].xz() + tri[1].xz() + tri[2].xz()) / 3.0;
            let dist = c.distance_squared(q);
            if nearest.is_none_or(|n| dist < n.0) {
                nearest = Some((dist, i, h));
            }
        }
        nearest.map(|(_, i, h)| (i, h))
    }

    /// Vertex `k` of the detail mesh (polygon corners first, then extra samples).
    pub fn detail_vertex(&self, k: usize) -> Vec3 {
        let n = self.vertices.len();
        if k < n {
            self.vertices[k]
        } else {
            self.detail_verts[k - n]
        }
    }
}
