//! Rock geometry: dense closed meshes made from icospheres pushed out by a radial shape
//! function (a superellipsoid with broad lumps, joint planes cut with soft edges, exfoliation
//! ledges, cracks and fine ridged detail), optional sedimentary layers, an octahedral UV
//! layout per stone (seams only on the buried underside) and per-vertex cavity values for the
//! texture bake.
//!
//! Object space: +Y up, metres, origin on the ground line (the part below y = 0 is meant to be
//! buried). Front faces wind so that `cross(b - a, c - a)` points out of the rock.

use std::collections::HashMap;

use glam::{Vec2, Vec3};
use misty_engine::render::mesh::{MeshData, Vertex};

// ---- noise ---------------------------------------------------------------------------------

fn hash(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f)
        ^ seed.wrapping_mul(0x1656_67b1);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

/// A number in 0..1 from a seed and a salt.
pub fn rand(seed: u32, salt: u32) -> f32 {
    (hash(salt as i32, 17, 3, seed) & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// 3D gradient noise, about -1..1.
pub fn perlin(p: Vec3, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - Vec3::splat(15.0)) + Vec3::splat(10.0));
    let (x, y, z) = (i.x as i32, i.y as i32, i.z as i32);
    let g = |dx: i32, dy: i32, dz: i32| {
        let h = hash(x + dx, y + dy, z + dz, seed);
        let d = f - Vec3::new(dx as f32, dy as f32, dz as f32);
        // 12 edge directions of a cube.
        match h % 12 {
            0 => d.x + d.y,
            1 => -d.x + d.y,
            2 => d.x - d.y,
            3 => -d.x - d.y,
            4 => d.x + d.z,
            5 => -d.x + d.z,
            6 => d.x - d.z,
            7 => -d.x - d.z,
            8 => d.y + d.z,
            9 => -d.y + d.z,
            10 => d.y - d.z,
            _ => -d.y - d.z,
        }
    };
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x0 = l(
        l(g(0, 0, 0), g(1, 0, 0), u.x),
        l(g(0, 1, 0), g(1, 1, 0), u.x),
        u.y,
    );
    let x1 = l(
        l(g(0, 0, 1), g(1, 0, 1), u.x),
        l(g(0, 1, 1), g(1, 1, 1), u.x),
        u.y,
    );
    l(x0, x1, u.z) * 0.9
}

pub fn fbm(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq) = (0.0, 0.5, 1.0);
    for o in 0..octaves {
        sum += amp * perlin(p * freq, seed.wrapping_add(o * 31));
        amp *= 0.5;
        freq *= 2.03;
    }
    sum
}

/// Ridged noise 0..~1: sharp creases where the noise crosses zero.
pub fn ridged(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq) = (0.0, 0.5, 1.0);
    for o in 0..octaves {
        let n = 1.0 - perlin(p * freq, seed.wrapping_add(o * 57)).abs();
        sum += amp * n * n;
        amp *= 0.5;
        freq *= 2.1;
    }
    sum
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Polynomial smooth minimum (rounded edge of radius ~`k`).
fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

/// A unit vector from two numbers in 0..1, within `max_tilt` radians of +Y when given.
fn direction(a: f32, b: f32, min_y: f32) -> Vec3 {
    let y = min_y + (1.0 - min_y) * a;
    let r = (1.0 - y * y).max(0.0).sqrt();
    let phi = b * std::f32::consts::TAU;
    Vec3::new(r * phi.cos(), y, r * phi.sin())
}

// ---- shape ---------------------------------------------------------------------------------

/// Sedimentary layers (outcrops): terraces of `thickness` metres dipping by `dip` (rise per
/// metre along +X), each layer's edge recessed by up to `recess` of its radius.
#[derive(Clone, Copy, Debug)]
pub struct Strata {
    pub thickness: f32,
    pub dip: f32,
    pub recess: f32,
}

/// One stone's shape.
#[derive(Clone, Debug)]
pub struct Stone {
    /// Half extents of the base superellipsoid (metres).
    pub radii: Vec3,
    /// Superellipsoid exponent: 2 = ellipsoid, higher = boxier.
    pub squareness: f32,
    /// Broad lumps: relative amplitude and feature size (metres).
    pub lumps: f32,
    pub lump_size: f32,
    /// Joint planes cut into the stone: how many, how deep (share of the radius along the
    /// plane normal left standing, min..max), how round their edges (metres), and the lowest
    /// y of their normals (0 = sides and top only).
    pub facets: u32,
    pub facet_keep: [f32; 2],
    pub facet_round: f32,
    pub facet_min_y: f32,
    /// Exfoliation: sheets peeling off as shallow ledges (metres).
    pub flakes: f32,
    /// Cracks: count, depth and width (metres).
    pub cracks: u32,
    pub crack_depth: f32,
    pub crack_width: f32,
    /// Fine ridged detail: amplitude and feature size (metres).
    pub detail: f32,
    pub detail_size: f32,
    pub strata: Option<Strata>,
    /// Share of the height below the ground line.
    pub bury: f32,
    /// Flattened underside: everything below this share of the height (from the bottom) is
    /// squashed, like a stone that settled.
    pub settle: f32,
    pub seed: u32,
    /// Placement inside a cluster (after the ground line is moved to y = 0) and a turn about Y.
    pub offset: Vec3,
    pub yaw: f32,
}

impl Default for Stone {
    fn default() -> Self {
        Self {
            radii: Vec3::ONE,
            squareness: 2.0,
            lumps: 0.15,
            lump_size: 1.5,
            facets: 0,
            facet_keep: [0.8, 0.95],
            facet_round: 0.2,
            facet_min_y: -0.2,
            flakes: 0.0,
            cracks: 0,
            crack_depth: 0.0,
            crack_width: 0.03,
            detail: 0.01,
            detail_size: 0.4,
            strata: None,
            bury: 0.2,
            settle: 0.25,
            seed: 1,
            offset: Vec3::ZERO,
            yaw: 0.0,
        }
    }
}

struct Plane {
    normal: Vec3,
    distance: f32,
}

struct Crack {
    normal: Vec3,
    point: Vec3,
    seed: u32,
}

impl Stone {
    /// Radius of the base superellipsoid along unit `q`.
    fn base_radius(&self, q: Vec3) -> f32 {
        let e = self.squareness;
        let s = (q.x / self.radii.x).abs().powf(e)
            + (q.y / self.radii.y).abs().powf(e)
            + (q.z / self.radii.z).abs().powf(e);
        1.0 / s.powf(1.0 / e).max(1e-6)
    }

    fn planes(&self) -> Vec<Plane> {
        (0..self.facets)
            .map(|k| {
                let s = self.seed.wrapping_mul(131).wrapping_add(k * 7);
                let normal = direction(rand(s, 1), rand(s, 2), self.facet_min_y);
                let keep =
                    self.facet_keep[0] + (self.facet_keep[1] - self.facet_keep[0]) * rand(s, 3);
                Plane {
                    normal,
                    distance: self.base_radius(normal) * keep,
                }
            })
            .collect()
    }

    fn crack_list(&self) -> Vec<Crack> {
        (0..self.cracks)
            .map(|k| {
                let s = self.seed.wrapping_mul(977).wrapping_add(k * 13);
                // Mostly vertical cracks (joints), through a point near the middle.
                let mut normal = direction(rand(s, 1) * 0.35, rand(s, 2), 0.0);
                normal = Vec3::new(normal.x, normal.y * 0.4, normal.z).normalize();
                let n2 = Vec3::new(normal.z, 0.0, -normal.x);
                let point = n2 * (rand(s, 3) - 0.5) * self.radii.max_element() * 0.6
                    + normal * (rand(s, 4) - 0.5) * self.radii.min_element() * 0.6;
                Crack {
                    normal,
                    point,
                    seed: s,
                }
            })
            .collect()
    }

    /// Surface point along unit direction `q` (centred, before the ground line moves).
    fn surface(&self, q: Vec3, planes: &[Plane], cracks: &[Crack]) -> Vec3 {
        let s = self.seed;
        let r0 = self.base_radius(q);
        let base = q * r0;
        // Broad lumps, plus a lopsided swell at the stone's own scale (no two ends alike).
        let swell = fbm(
            base / (self.radii.max_element() * 1.6) + Vec3::splat(2.1),
            2,
            s + 3,
        );
        let mut r = r0
            * (1.0
                + self.lumps * fbm(base / self.lump_size + Vec3::splat(7.3), 3, s)
                + self.lumps * 2.2 * swell);
        for p in planes {
            let c = q.dot(p.normal);
            if c > 0.05 {
                r = smin(r, p.distance / c, self.facet_round);
            }
        }
        if self.flakes > 0.0 {
            // Sheets: the radius drops by a step wherever a broad noise crosses a level.
            let n = fbm(base * 0.45 + Vec3::splat(3.1), 3, s + 5) * 3.0;
            let k = n.floor();
            let f = n - k;
            let step = k + smoothstep(0.75, 1.0, f);
            r -= self.flakes * step;
        }
        let at = q * r;
        for c in cracks {
            let wobble = fbm(at * 1.7, 2, c.seed) * self.crack_width * 3.0;
            let d = (at - c.point).dot(c.normal) + wobble;
            // Cracks fade out towards their ends.
            let along = smoothstep(-0.12, 0.2, fbm(at * 0.6, 2, c.seed + 1));
            let depth = self.crack_depth * along;
            r -= depth * (-(d / self.crack_width).powi(2)).exp();
        }
        if self.detail > 0.0 {
            let n = ridged(at / self.detail_size, 4, s + 11) - 0.45;
            r +=
                self.detail * n + self.detail * 0.3 * fbm(at / (self.detail_size * 0.3), 2, s + 13);
        }
        q * r.max(r0 * 0.3)
    }
}

/// Outcrop layers on a centred surface point: terraced heights and recessed layer edges.
fn layered(p: Vec3, strata: Strata, radii: Vec3, seed: u32) -> Vec3 {
    let warp = fbm(p * 0.6, 2, seed + 41) * strata.thickness * 0.35;
    let s = (p.y + strata.dip * p.x + warp) / strata.thickness;
    let k = s.floor();
    let f = s - k;
    // Flat treads, steep risers.
    let stepped = k + smoothstep(0.62, 1.0, f);
    let mut q = p;
    q.y += (stepped - s) * strata.thickness * 0.45;
    // Each layer's edge pulled in by its own amount (softer layers erode further back).
    let recess = |layer: f32| {
        let h = (hash(layer as i32, 5, 9, seed) & 0xffff) as f32 / 65535.0;
        strata.recess * (0.25 + 0.75 * h * h)
    };
    let edge = smoothstep(0.5, 1.0, f);
    // Ledges come and go round the outline.
    let around = smoothstep(-0.25, 0.3, fbm(p * 0.35 + Vec3::splat(k), 2, seed + 47));
    let pull = (recess(k) * (1.0 - edge) + recess(k + 1.0) * edge) * (0.3 + 1.0 * around);
    // Only near the outline (top faces keep their place).
    let horizontal = Vec2::new(p.x / radii.x, p.z / radii.z).length();
    let rim = smoothstep(0.55, 0.95, horizontal);
    let scale = 1.0 - pull * rim * (1.0 + 0.4 * fbm(p * 1.3, 2, seed + 43));
    q.x *= scale;
    q.z *= scale;
    q
}

// ---- mesh ----------------------------------------------------------------------------------

/// An icosphere subdivided `n` times: unit vertices and outward triangles (20 · 4ⁿ).
pub fn icosphere(n: u32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut points: Vec<Vec3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut faces: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..n {
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        let mut half = |a: u32, b: u32, points: &mut Vec<Vec3>| {
            let key = (a.min(b), a.max(b));
            *mid.entry(key).or_insert_with(|| {
                points.push(((points[a as usize] + points[b as usize]) * 0.5).normalize());
                points.len() as u32 - 1
            })
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for [a, b, c] in faces {
            let ab = half(a, b, &mut points);
            let bc = half(b, c, &mut points);
            let ca = half(c, a, &mut points);
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        faces = next;
    }
    (points, faces)
}

/// A rock mesh with what the bake needs per vertex.
pub struct RockMesh {
    pub data: MeshData,
    /// Per vertex: how far the surface lies below a smoothed copy of itself (metres; > 0 in
    /// hollows, < 0 on ridges), at a fine and a broad scale.
    pub cavity: Vec<[f32; 2]>,
    /// Horizontal radius of the footprint at the ground line (largest stone's, metres).
    pub footprint: f32,
    /// Highest point (metres above the ground line).
    pub top: f32,
}

/// UV rectangle of stone `i` of `n` (a grid of squares with a small margin).
fn uv_cell(i: usize, n: usize) -> (Vec2, f32) {
    let cols = (n as f32).sqrt().ceil() as usize;
    let size = 1.0 / cols as f32;
    let (cx, cy) = (i % cols, i / cols);
    let margin = size * 0.012;
    (
        Vec2::new(cx as f32 * size + margin, cy as f32 * size + margin),
        size - 2.0 * margin,
    )
}

/// Octahedral layout of unit `d` into -1..1 (+Y hemisphere = the inner diamond); `sx`, `sz`
/// pick the side of the folded lower half (the triangle's, so seams split cleanly).
fn octahedral(d: Vec3, sx: f32, sz: f32) -> Vec2 {
    let p = d / (d.x.abs() + d.y.abs() + d.z.abs());
    if p.y >= 0.0 {
        Vec2::new(p.x, p.z)
    } else {
        Vec2::new(sx * (1.0 - p.z.abs()), sz * (1.0 - p.x.abs()))
    }
}

/// UV (0..1) of the icosphere direction `d`: the directions are first bent upwards
/// (`normalize(d + lift · Y)`, a smooth bijection) so everything above the ground line falls in
/// the octahedral layout's upper half; that half (a diamond) is turned 45° to fill the whole
/// square, and the buried lower half folds into a thin strip along the border.
fn layout(d: Vec3, lift: f32, side: (f32, f32)) -> Vec2 {
    let w = (d + Vec3::Y * lift).normalize();
    let o = octahedral(w, side.0, side.1);
    let r = Vec2::new(o.x + o.y, o.y - o.x);
    let m = r.x.abs().max(r.y.abs());
    let squeezed = if m <= 1.0 {
        m * 0.96
    } else {
        0.96 + (m - 1.0) * 0.04
    };
    let r = if m > 1e-9 { r * (squeezed / m) } else { r };
    r * 0.5 + Vec2::splat(0.5)
}

/// Build the rock of `stones` (one shell each) from icospheres of `subdivisions[i]`.
pub fn build(stones: &[Stone], subdivisions: &[u32]) -> RockMesh {
    let mut data = MeshData::default();
    let mut cavity = Vec::new();
    let mut footprint: f32 = 0.0;
    let mut top: f32 = f32::MIN;
    for (i, stone) in stones.iter().enumerate() {
        let (dirs, faces) = icosphere(subdivisions[i.min(subdivisions.len() - 1)]);
        let planes = stone.planes();
        let cracks = stone.crack_list();
        let mut pos: Vec<Vec3> = dirs
            .iter()
            .map(|&q| stone.surface(q, &planes, &cracks))
            .collect();
        if let Some(strata) = stone.strata {
            for p in &mut pos {
                *p = layered(*p, strata, stone.radii, stone.seed);
            }
        }
        // Settled underside, then the ground line to y = 0.
        let (lo, hi) = pos
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
        let floor = lo + (hi - lo) * stone.settle;
        for p in &mut pos {
            if p.y < floor {
                p.y = floor + (p.y - floor) * 0.35;
            }
        }
        let lo = floor + (lo - floor) * 0.35;
        let ground = lo + (hi - lo) * stone.bury;
        let (sin, cos) = stone.yaw.sin_cos();
        for p in &mut pos {
            p.y -= ground;
            *p = Vec3::new(p.x * cos + p.z * sin, p.y, -p.x * sin + p.z * cos) + stone.offset;
        }
        // Normals on the welded surface (no creases at UV seams).
        let mut normals = vec![Vec3::ZERO; pos.len()];
        let mut tris = Vec::with_capacity(faces.len());
        for &[a, b, c] in &faces {
            let (pa, pb, pc) = (pos[a as usize], pos[b as usize], pos[c as usize]);
            let mut n = (pb - pa).cross(pc - pa);
            let centre = dirs[a as usize] + dirs[b as usize] + dirs[c as usize];
            let t = if n.dot(centre) >= 0.0 {
                [a, b, c]
            } else {
                n = -n;
                [a, c, b]
            };
            for v in t {
                normals[v as usize] += n;
            }
            tris.push(t);
        }
        let normals: Vec<Vec3> = normals.iter().map(|n| n.normalize_or(Vec3::Y)).collect();
        let cav = cavities(&pos, &normals, &tris);
        // Footprint: stones reaching the ground line.
        let ring = pos
            .iter()
            .filter(|p| p.y.abs() < 0.05 * (hi - lo).max(0.1))
            .map(|p| Vec2::new(p.x - stone.offset.x, p.z - stone.offset.z).length())
            .fold(0.0f32, f32::max);
        footprint = footprint.max(ring);
        top = top.max(pos.iter().map(|p| p.y).fold(f32::MIN, f32::max));
        // UVs: each stone its square, the part above the ground filling it; split vertices
        // along the folded seams (under the ground).
        let lowest = dirs
            .iter()
            .zip(&pos)
            .filter(|(_, p)| p.y > -0.02)
            .map(|(d, _)| d.y)
            .fold(1.0f32, f32::min);
        let lift = (0.06 - lowest).clamp(0.0, 0.85);
        let (corner, size) = uv_cell(i, stones.len());
        let mut split: HashMap<(u32, u32, u32), u32> = HashMap::new();
        for t in &tris {
            let bent = |v: u32| (dirs[v as usize] + Vec3::Y * lift).normalize();
            let centre = bent(t[0]) + bent(t[1]) + bent(t[2]);
            let side = (centre.x.signum(), centre.z.signum());
            let mut out = [0u32; 3];
            for (k, &v) in t.iter().enumerate() {
                let o = layout(dirs[v as usize], lift, side);
                let uv = corner + o * size;
                let key = (v, uv.x.to_bits(), uv.y.to_bits());
                out[k] = *split.entry(key).or_insert_with(|| {
                    data.vertices.push(Vertex {
                        position: pos[v as usize].into(),
                        normal: normals[v as usize].into(),
                        uv: uv.into(),
                        tangent: [1.0, 0.0, 0.0, 1.0],
                        uv2: [0.0; 2],
                    });
                    cavity.push(cav[v as usize]);
                    data.vertices.len() as u32 - 1
                });
            }
            data.indices.extend(out);
        }
    }
    data.compute_tangents();
    RockMesh {
        data,
        cavity,
        footprint,
        top,
    }
}

/// Cavity at two scales: the distance along the normal from each vertex to a Laplacian-smoothed
/// copy of the surface (a few and many smoothing passes).
fn cavities(pos: &[Vec3], normals: &[Vec3], tris: &[[u32; 3]]) -> Vec<[f32; 2]> {
    let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); pos.len()];
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if !neighbours[a as usize].contains(&b) {
                neighbours[a as usize].push(b);
                neighbours[b as usize].push(a);
            }
        }
    }
    let smooth = |src: &[Vec3]| -> Vec<Vec3> {
        src.iter()
            .enumerate()
            .map(|(i, p)| {
                let n = &neighbours[i];
                if n.is_empty() {
                    return *p;
                }
                let avg = n.iter().map(|&j| src[j as usize]).sum::<Vec3>() / n.len() as f32;
                p.lerp(avg, 0.6)
            })
            .collect()
    };
    let mut cur = pos.to_vec();
    let mut fine = Vec::new();
    for pass in 1..=48 {
        cur = smooth(&cur);
        if pass == 4 {
            fine = cur.clone();
        }
    }
    (0..pos.len())
        .map(|i| {
            [
                (fine[i] - pos[i]).dot(normals[i]),
                (cur[i] - pos[i]).dot(normals[i]),
            ]
        })
        .collect()
}

/// `data` simplified to about `ratio` of its triangles (seams kept; see
/// [`misty_engine::vgeo::rock::simplified`]), with the per-vertex normals of the dense mesh.
pub fn lod(data: &MeshData, ratio: f32) -> MeshData {
    let mut out = misty_engine::vgeo::rock::simplified(data, ratio);
    out.compute_tangents();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rock_is_closed_outward_and_grounded() {
        let stone = Stone {
            radii: Vec3::new(1.2, 0.8, 1.0),
            facets: 4,
            cracks: 1,
            crack_depth: 0.04,
            flakes: 0.03,
            ..Default::default()
        };
        let rock = build(&[stone], &[4]);
        let d = &rock.data;
        assert_eq!(d.indices.len(), 20 * 256 * 3);
        // Outward: face normals agree with the vertex normals.
        let mut agree = 0;
        for t in d.indices.chunks(3) {
            let p = |i: u32| Vec3::from(d.vertices[i as usize].position);
            let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
            agree += usize::from(n.dot(Vec3::from(d.vertices[t[0] as usize].normal)) > 0.0);
        }
        assert!(agree * 100 > d.indices.len() / 3 * 98, "{agree}");
        // Partly buried: some of it below the ground line, most above.
        let below = d.vertices.iter().filter(|v| v.position[1] < 0.0).count();
        assert!(below > 0 && below < d.vertices.len() / 2, "{below}");
        assert!(rock.footprint > 0.5 && rock.top > 0.5);
        // UVs inside the unit square; seams split only a few vertices.
        assert!(
            d.vertices
                .iter()
                .all(|v| (0.0..=1.0).contains(&v.uv[0]) && (0.0..=1.0).contains(&v.uv[1]))
        );
        let welded = 10 * 4usize.pow(4) + 2;
        assert!(
            d.vertices.len() < welded + welded / 10,
            "{}",
            d.vertices.len()
        );
        assert_eq!(rock.cavity.len(), d.vertices.len());
    }

    #[test]
    fn lod_levels_shrink() {
        let rock = build(&[Stone::default()], &[5]);
        let l1 = lod(&rock.data, 0.2);
        let tris = rock.data.indices.len() / 3;
        assert!(l1.indices.len() / 3 < tris / 3, "{}", l1.indices.len() / 3);
        assert!(l1.indices.len() / 3 > tris / 10);
    }
}
