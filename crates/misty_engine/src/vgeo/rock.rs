//! A dense procedural rock (the virtual geometry demo mesh) and whole-mesh LOD levels made with
//! the same simplifier (the LOD Group path it is compared against).

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::render::mesh::{MeshData, Vertex};

/// Hash of an integer lattice point to 0..1.
fn hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f)
        ^ seed.wrapping_mul(0x1656_67b1);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// Smooth value noise, -1..1.
fn noise(p: Vec3, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let s = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let (x, y, z) = (i.x as i32, i.y as i32, i.z as i32);
    let c = |dx, dy, dz| hash(x + dx, y + dy, z + dz, seed);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), s.x);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), s.x);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), s.x);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), s.x);
    let v = lerp(lerp(x00, x10, s.y), lerp(x01, x11, s.y), s.z);
    v * 2.0 - 1.0
}

fn fbm(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq) = (0.0, 0.5, 1.0);
    for o in 0..octaves {
        sum += amp * noise(p * freq, seed.wrapping_add(o));
        amp *= 0.5;
        freq *= 2.03;
    }
    sum
}

/// Ridged detail: sharp creases like cracks and strata.
fn ridged(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq) = (0.0, 0.5, 1.0);
    for o in 0..octaves {
        sum += amp * (1.0 - noise(p * freq, seed.wrapping_add(o * 7)).abs());
        amp *= 0.5;
        freq *= 2.1;
    }
    sum
}

/// An icosphere subdivided `subdivisions` times (20 · 4ⁿ triangles: 6 → 82 k, 7 → 328 k),
/// pushed into a rock about 2 m across with crags and fine cracks. No seams: one vertex per
/// position, so the whole surface simplifies.
pub fn rock(subdivisions: u32, seed: u32) -> MeshData {
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
    for _ in 0..subdivisions {
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        let mut half = |a: u32, b: u32, points: &mut Vec<Vec3>| {
            let key = if a < b { (a, b) } else { (b, a) };
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
    let s = seed.wrapping_mul(97);
    let offset = Vec3::new(hash(1, 2, 3, s), hash(4, 5, 6, s), hash(7, 8, 9, s)) * 50.0;
    let shaped: Vec<Vec3> = points
        .iter()
        .map(|&d| {
            let p = d * 1.1 + offset;
            let r = 1.0 + 0.32 * fbm(p, 4, s) + 0.12 * ridged(p * 2.5, 4, s + 11) - 0.06
                + 0.025 * fbm(p * 14.0, 3, s + 23);
            // Flatter underneath and a little squat, like a rock that settled.
            let mut q = d * r;
            q.y *= 0.72;
            if q.y < -0.35 {
                q.y = -0.35 + (q.y + 0.35) * 0.3;
            }
            q
        })
        .collect();
    let mut data = MeshData {
        vertices: shaped
            .iter()
            .zip(&points)
            .map(|(&p, &d)| Vertex {
                position: p.into(),
                normal: d.into(),
                uv: Vec2::new(
                    0.5 + d.z.atan2(d.x) / std::f32::consts::TAU,
                    0.5 - d.y.asin() / std::f32::consts::PI,
                )
                .into(),
                tangent: [1.0, 0.0, 0.0, 1.0],
                uv2: [0.0; 2],
            })
            .collect(),
        indices: Vec::with_capacity(faces.len() * 3),
    };
    for [a, b, c] in faces {
        // Outward faces: (p1 - p0) × (p2 - p0) points away from the centre.
        let (pa, pb, pc) = (shaped[a as usize], shaped[b as usize], shaped[c as usize]);
        let n = (pb - pa).cross(pc - pa);
        if n.dot(pa + pb + pc) >= 0.0 {
            data.indices.extend([a, b, c]);
        } else {
            data.indices.extend([a, c, b]);
        }
    }
    data.compute_normals();
    data.compute_tangents();
    data
}

/// The mesh simplified to about `ratio` of its triangles (open borders and seams kept), with
/// unused vertices dropped: a LOD level an artist would make.
pub fn simplified(data: &MeshData, ratio: f32) -> MeshData {
    let positions: Vec<Vec3> = data.vertices.iter().map(|v| v.position.into()).collect();
    let weld = super::weld(&positions);
    let tris: Vec<[u32; 3]> = data.indices.as_chunks::<3>().0.to_vec();
    let border = super::dag::border_positions(&tris, &weld);
    let target = ((tris.len() as f32 * ratio) as usize).max(4);
    let s = super::simplify::simplify(
        &tris,
        &positions,
        &weld,
        |p| border.contains_key(&p),
        target,
    );
    let mut remap = vec![u32::MAX; data.vertices.len()];
    let mut out = MeshData::default();
    for t in &s.tris {
        for &v in t {
            if remap[v as usize] == u32::MAX {
                remap[v as usize] = out.vertices.len() as u32;
                out.vertices.push(data.vertices[v as usize]);
            }
            out.indices.push(remap[v as usize]);
        }
    }
    out
}
