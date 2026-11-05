//! Turn existing meshes (primitives, imported meshes) into Editable Meshes.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use super::{EditableMesh, Face, edge_key, shapes};
use crate::render::mesh::MeshData;
use crate::scene::{MeshRef, PrimitiveMesh};

/// Editable copy of a Mesh Filter's mesh. Primitives with a matching shape are rebuilt from
/// clean polygons (same size and placement as the primitive); anything else is welded and its
/// coplanar triangle pairs merged into quads.
pub fn from_mesh_ref(r: &MeshRef) -> Option<EditableMesh> {
    if let Some(p) = r.primitive() {
        let shifted = |mut m: EditableMesh, dy: f32| {
            for v in &mut m.positions {
                v.y += dy;
            }
            m
        };
        match p {
            PrimitiveMesh::Cube => return Some(shifted(shapes::cube(Vec3::ONE), -0.5)),
            PrimitiveMesh::Plane => return Some(shapes::plane(10.0, 10.0, 10, 10)),
            PrimitiveMesh::Cylinder => {
                return Some(shifted(shapes::cylinder(0.5, 2.0, 24), -1.0));
            }
            PrimitiveMesh::Quad => {
                // Facing −Z, 1 × 1: swapping Y and Z mirrors, which turns the +Y side to −Z.
                let mut m = shapes::plane(1.0, 1.0, 1, 1);
                for v in &mut m.positions {
                    *v = Vec3::new(v.x, v.z, 0.0);
                }
                return Some(m);
            }
            _ => {}
        }
    }
    let data = crate::models::mesh(r)?;
    Some(from_mesh_data(&data))
}

/// Weld a triangle mesh by position, then merge coplanar triangle pairs into quads. Faces whose
/// vertex normals were smooth get smoothing group 1.
pub fn from_mesh_data(data: &MeshData) -> EditableMesh {
    let key = |p: Vec3| (p * 1e4).round().as_ivec3().to_array();
    let mut index: HashMap<[i32; 3], u32> = HashMap::new();
    let mut positions = Vec::new();
    let remap: Vec<u32> = data
        .vertices
        .iter()
        .map(|v| {
            let p = Vec3::from(v.position);
            *index.entry(key(p)).or_insert_with(|| {
                positions.push(p);
                positions.len() as u32 - 1
            })
        })
        .collect();
    let mut tris: Vec<([u32; 3], bool)> = Vec::new();
    for t in data.indices.as_chunks::<3>().0 {
        let v = [
            remap[t[0] as usize],
            remap[t[1] as usize],
            remap[t[2] as usize],
        ];
        if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
            continue;
        }
        let (a, b, c) = (
            positions[v[0] as usize],
            positions[v[1] as usize],
            positions[v[2] as usize],
        );
        let n = (b - a).cross(c - a).normalize_or_zero();
        let smooth = t.iter().any(|i| {
            let vn = Vec3::from(data.vertices[*i as usize].normal);
            vn.normalize_or_zero().dot(n) < 0.999
        });
        tris.push((v, smooth));
    }
    let mut m = EditableMesh::new(positions, Vec::new());
    // Pair triangles across shared edges when coplanar and the quad is convex.
    let mut by_edge: HashMap<[u32; 2], Vec<usize>> = HashMap::new();
    for (i, (t, _)) in tris.iter().enumerate() {
        for k in 0..3 {
            by_edge
                .entry(edge_key(t[k], t[(k + 1) % 3]))
                .or_default()
                .push(i);
        }
    }
    let normal = |t: &[u32; 3]| {
        let p = |k: usize| m.positions[t[k] as usize];
        (p(1) - p(0)).cross(p(2) - p(0)).normalize_or_zero()
    };
    let mut used = vec![false; tris.len()];
    let mut faces = Vec::new();
    for i in 0..tris.len() {
        if used[i] {
            continue;
        }
        let (t, smooth) = tris[i];
        used[i] = true;
        let mut merged = None;
        for k in 0..3 {
            let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            let Some(other) = by_edge[&edge_key(a, b)]
                .iter()
                .copied()
                .find(|j| *j != i && !used[*j] && tris[*j].1 == smooth)
            else {
                continue;
            };
            let o = tris[other].0;
            if normal(&t).dot(normal(&o)) < 0.9999 {
                continue;
            }
            // The other triangle has b → a; its third corner goes between them.
            let Some(d) = o.iter().copied().find(|x| *x != a && *x != b) else {
                continue;
            };
            let quad = [a, d, b, c];
            if is_convex(&quad.map(|v| m.positions[v as usize])) {
                used[other] = true;
                merged = Some(quad.to_vec());
                break;
            }
        }
        let verts = merged.unwrap_or_else(|| t.to_vec());
        faces.push(Face {
            smoothing: smooth as u32,
            ..Face::new(verts)
        });
    }
    m.faces = faces;
    m.compact();
    m
}

fn is_convex(q: &[Vec3; 4]) -> bool {
    let n = super::polygon_normal(q.iter().copied());
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let p: Vec<Vec2> = q.iter().map(|x| Vec2::new(x.dot(u), x.dot(v))).collect();
    (0..4).all(|i| (p[(i + 1) % 4] - p[i]).perp_dot(p[(i + 2) % 4] - p[(i + 1) % 4]) > 1e-9)
}
