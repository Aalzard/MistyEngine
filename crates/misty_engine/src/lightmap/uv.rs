//! Lightmap UVs (the second UV set, `Vertex::uv2`).
//!
//! [`unwrap`] cuts a mesh into planar charts and packs them into [0, 1]²:
//! 1. Triangles are grown into charts by flood fill across shared edges (matched by position,
//!    so UV and normal seams of the first set don't cut charts) while their face normal stays
//!    within ~45° of the chart's first triangle.
//! 2. Each chart is projected onto the plane facing that normal (every triangle keeps its
//!    orientation, so no chart folds over itself) and turned so it is wider than tall.
//! 3. Charts are shelf-packed, tallest first; a binary search finds the largest scale at which
//!    everything fits with `margin` (in UV units) between charts and half of it at the border.
//!
//! Vertices shared by two charts are split. Charts keep their relative sizes, so texel density
//! is even over the mesh. Primitives use [`primitive`]: the Plane, Quad and Sphere already have
//! non-overlapping UVs, the rest are unwrapped.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::render::mesh::MeshData;
use crate::scene::PrimitiveMesh;

/// Gap between charts in UV units (two texels of a 64-texel renderer); the bake's dilation
/// fills it so bilinear sampling never reads a neighbour chart.
pub const MARGIN: f32 = 1.0 / 32.0;

/// Cosine of the largest angle between a triangle and its chart's first triangle.
const CHART_COS: f32 = 0.7;

/// One chart: its triangles and their 2D coordinates (mesh units, origin at the bbox corner).
struct Chart {
    tris: Vec<usize>,
    /// Per corner of `tris` (3 each).
    coords: Vec<Vec2>,
    size: Vec2,
}

/// Set `uv2` of a primitive (vertices may be split).
pub fn primitive(p: PrimitiveMesh, m: &mut MeshData) {
    match p {
        // Planar / lat-long UVs: already one non-overlapping chart, inset by half the margin.
        PrimitiveMesh::Plane | PrimitiveMesh::Quad | PrimitiveMesh::Sphere => {
            let k = 1.0 - MARGIN;
            for v in &mut m.vertices {
                v.uv2 = [MARGIN * 0.5 + v.uv[0] * k, MARGIN * 0.5 + v.uv[1] * k];
            }
        }
        PrimitiveMesh::Cube | PrimitiveMesh::Capsule | PrimitiveMesh::Cylinder => {
            *m = unwrap(m, MARGIN);
        }
    }
}

/// The mesh with generated lightmap UVs (see the module docs).
pub fn unwrap(mesh: &MeshData, margin: f32) -> MeshData {
    let charts = charts(mesh);
    if charts.is_empty() {
        return mesh.clone();
    }
    let placed = pack(&charts, margin);
    let mut out = MeshData::default();
    for (chart, (offset, scale)) in charts.iter().zip(placed) {
        // Original vertex → its copy in this chart.
        let mut copies: HashMap<u32, u32> = HashMap::new();
        for (k, &t) in chart.tris.iter().enumerate() {
            for c in 0..3 {
                let src = mesh.indices[t * 3 + c];
                let uv = offset + chart.coords[k * 3 + c] * scale;
                let index = *copies.entry(src).or_insert_with(|| {
                    let mut v = mesh.vertices[src as usize];
                    v.uv2 = uv.to_array();
                    out.vertices.push(v);
                    out.vertices.len() as u32 - 1
                });
                out.indices.push(index);
            }
        }
    }
    out
}

fn tri_positions(mesh: &MeshData, t: usize) -> [Vec3; 3] {
    let p = |c: usize| Vec3::from(mesh.vertices[mesh.indices[t * 3 + c] as usize].position);
    [p(0), p(1), p(2)]
}

/// Grow charts over the mesh.
fn charts(mesh: &MeshData) -> Vec<Chart> {
    let count = mesh.indices.len() / 3;
    if count == 0 {
        return Vec::new();
    }
    let key = |p: Vec3| (p * 1e4).round().as_ivec3().to_array();
    let positions: Vec<[Vec3; 3]> = (0..count).map(|t| tri_positions(mesh, t)).collect();
    let normals: Vec<Vec3> = positions
        .iter()
        .map(|p| (p[1] - p[0]).cross(p[2] - p[0]))
        .collect();
    let areas: Vec<f32> = normals.iter().map(|n| n.length() * 0.5).collect();
    let normals: Vec<Vec3> = normals.iter().map(|n| n.normalize_or_zero()).collect();
    // Undirected edge → triangles using it.
    let mut edges: HashMap<([i32; 3], [i32; 3]), Vec<usize>> = HashMap::new();
    for (t, p) in positions.iter().enumerate() {
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (ka, kb) = (key(p[a]), key(p[b]));
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            edges.entry(k).or_default().push(t);
        }
    }
    let neighbours = |t: usize| {
        let p = &positions[t];
        let mut out = Vec::new();
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (ka, kb) = (key(p[a]), key(p[b]));
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            out.extend(edges[&k].iter().copied().filter(|&o| o != t));
        }
        out
    };
    // Seeds: largest triangles first (charts start from the dominant faces).
    let mut seeds: Vec<usize> = (0..count).collect();
    seeds.sort_by(|a, b| areas[*b].total_cmp(&areas[*a]));
    let mut chart_of = vec![usize::MAX; count];
    let mut out = Vec::new();
    for seed in seeds {
        if chart_of[seed] != usize::MAX {
            continue;
        }
        let id = out.len();
        let n = if normals[seed] == Vec3::ZERO {
            Vec3::Y
        } else {
            normals[seed]
        };
        let mut tris = vec![seed];
        chart_of[seed] = id;
        let mut stack = vec![seed];
        while let Some(t) = stack.pop() {
            for o in neighbours(t) {
                // Degenerate triangles (sphere poles) join any chart.
                if chart_of[o] == usize::MAX
                    && (normals[o] == Vec3::ZERO || normals[o].dot(n) > CHART_COS)
                {
                    chart_of[o] = id;
                    tris.push(o);
                    stack.push(o);
                }
            }
        }
        out.push(project(&positions, &tris, n));
    }
    out
}

/// Planar projection of a chart facing `n`, turned wider than tall, corner at the origin.
fn project(positions: &[[Vec3; 3]], tris: &[usize], n: Vec3) -> Chart {
    let (t, b) = n.any_orthonormal_pair();
    let mut coords: Vec<Vec2> = tris
        .iter()
        .flat_map(|&i| positions[i].map(|p| Vec2::new(p.dot(t), p.dot(b))))
        .collect();
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for c in &coords {
        lo = lo.min(*c);
        hi = hi.max(*c);
    }
    let mut size = hi - lo;
    let turn = size.y > size.x;
    for c in &mut coords {
        let p = *c - lo;
        *c = if turn { Vec2::new(p.y, p.x) } else { p };
    }
    if turn {
        size = Vec2::new(size.y, size.x);
    }
    Chart {
        tris: tris.to_vec(),
        coords,
        size,
    }
}

/// Offset and scale of every chart in the unit square.
fn pack(charts: &[Chart], margin: f32) -> Vec<(Vec2, f32)> {
    let mut order: Vec<usize> = (0..charts.len()).collect();
    order.sort_by(|a, b| charts[*b].size.y.total_cmp(&charts[*a].size.y));
    let sizes: Vec<Vec2> = order.iter().map(|&i| charts[i].size).collect();
    let area: f32 = sizes.iter().map(|s| s.x * s.y).sum();
    let longest = sizes.iter().fold(0.0f32, |m, s| m.max(s.x).max(s.y));
    // Scale bounds: everything fits at `lo`; `hi` can't (the charts' area exceeds the square).
    let usable = (1.0 - margin).max(0.05);
    let mut hi = if area > 0.0 {
        (1.0 / area).sqrt()
    } else {
        1.0 / longest.max(1e-6)
    };
    let mut lo = 0.0;
    for _ in 0..40 {
        let mid = (lo + hi) * 0.5;
        if shelf(&sizes, mid, margin, usable).is_some() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let scale = lo.max(1e-6);
    let spots =
        shelf(&sizes, scale, margin, usable).unwrap_or_else(|| vec![Vec2::ZERO; sizes.len()]);
    let mut out = vec![(Vec2::ZERO, scale); charts.len()];
    for (k, &i) in order.iter().enumerate() {
        out[i] = (spots[k] + Vec2::splat(margin * 0.5), scale);
    }
    out
}

/// Shelf placement of `sizes` (sorted tallest first) scaled by `scale` in a `usable`² square
/// with `margin` between them; `None` when they don't fit.
fn shelf(sizes: &[Vec2], scale: f32, margin: f32, usable: f32) -> Option<Vec<Vec2>> {
    let mut out = Vec::with_capacity(sizes.len());
    let (mut x, mut y, mut row) = (0.0f32, 0.0f32, 0.0f32);
    for s in sizes {
        let s = *s * scale;
        if s.x > usable + 1e-6 {
            return None;
        }
        if x > 0.0 && x + s.x > usable + 1e-6 {
            y += row + margin;
            x = 0.0;
            row = 0.0;
        }
        if y + s.y > usable + 1e-6 {
            return None;
        }
        out.push(Vec2::new(x, y));
        x += s.x + margin;
        row = row.max(s.y);
    }
    Some(out)
}

/// UV2 boxes of the charts of a mesh unwrapped by [`unwrap`]: triangles connected through
/// corners at the same uv2 point form one box.
pub fn chart_boxes(mesh: &MeshData) -> Vec<(Vec2, Vec2)> {
    let n = mesh.vertices.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    // Vertices at the same uv2 point (split only for normals or the first UV set) are one.
    let mut at: HashMap<[i32; 2], usize> = HashMap::new();
    for (i, v) in mesh.vertices.iter().enumerate() {
        let k = v.uv2.map(|c| (c * 1e5).round() as i32);
        let first = *at.entry(k).or_insert(i);
        let (a, b) = (find(&mut parent, first), find(&mut parent, i));
        parent[b] = a;
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        let a = find(&mut parent, t[0] as usize);
        for &o in &t[1..] {
            let b = find(&mut parent, o as usize);
            parent[b] = a;
        }
    }
    let mut boxes: HashMap<usize, (Vec2, Vec2)> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        let uv = Vec2::from(mesh.vertices[i].uv2);
        let e = boxes
            .entry(r)
            .or_insert((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)));
        e.0 = e.0.min(uv);
        e.1 = e.1.max(uv);
    }
    boxes.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::mesh;

    /// Every chart box inside [0, 1] and at least `gap` away from every other one.
    fn assert_packed(m: &MeshData, gap: f32) {
        let boxes = chart_boxes(m);
        assert!(!boxes.is_empty());
        for (lo, hi) in &boxes {
            assert!(
                lo.min_element() >= -1e-5 && hi.max_element() <= 1.0 + 1e-5,
                "{lo} {hi}"
            );
        }
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                let dx = (b.0.x - a.1.x).max(a.0.x - b.1.x);
                let dy = (b.0.y - a.1.y).max(a.0.y - b.1.y);
                assert!(
                    dx.max(dy) >= gap * 0.99,
                    "charts {a:?} and {b:?} are {} apart",
                    dx.max(dy)
                );
            }
        }
    }

    #[test]
    fn primitive_charts_are_inside_the_square_with_padding() {
        for p in PrimitiveMesh::ALL {
            let m = mesh::generate(p);
            assert_packed(&m, MARGIN);
            // Every triangle keeps a non-zero uv2 area (no chart collapsed).
            let used: f32 = m
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let uv = |i: u32| Vec2::from(m.vertices[i as usize].uv2);
                    (uv(t[1]) - uv(t[0])).perp_dot(uv(t[2]) - uv(t[0])).abs() * 0.5
                })
                .sum();
            assert!(used > 0.1, "{p:?} uses {used} of the square");
        }
    }

    #[test]
    fn cube_unwraps_into_six_planar_charts() {
        let m = mesh::generate(PrimitiveMesh::Cube);
        assert_eq!(chart_boxes(&m).len(), 6);
        // Texel density: every face the same size.
        let sizes: Vec<Vec2> = chart_boxes(&m).iter().map(|(lo, hi)| *hi - *lo).collect();
        for s in &sizes {
            assert!((s.x - sizes[0].x).abs() < 1e-4 && (s.y - sizes[0].y).abs() < 1e-4);
        }
    }

    #[test]
    fn charts_never_overlap_on_a_bumpy_mesh() {
        // A wavy grid: many charts of different sizes.
        let mut m = MeshData::default();
        let n = 12;
        for z in 0..=n {
            for x in 0..=n {
                let (fx, fz) = (x as f32, z as f32);
                m.vertices.push(crate::render::mesh::Vertex {
                    position: [fx, (fx * 0.9).sin() * 2.0 + (fz * 0.7).cos() * 1.5, fz],
                    normal: [0.0, 1.0, 0.0],
                    uv: [0.0; 2],
                    uv2: [0.0; 2],
                    tangent: [1.0, 0.0, 0.0, 1.0],
                });
            }
        }
        for z in 0..n {
            for x in 0..n {
                let a = z * (n + 1) + x;
                m.indices
                    .extend([a, a + n + 1, a + 1, a + 1, a + n + 1, a + n + 2]);
            }
        }
        let out = unwrap(&m, 0.02);
        assert!(chart_boxes(&out).len() > 1);
        assert_packed(&out, 0.02);
        assert_eq!(out.indices.len(), m.indices.len());
    }
}
