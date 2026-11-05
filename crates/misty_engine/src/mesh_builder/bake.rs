//! Editable Mesh → render meshes: per-corner vertices with flat or smoothing-group normals,
//! per-face projected UVs, triangulated polygons.

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec3};

use super::{EditableMesh, FaceUv};
use crate::render::mesh::{MeshData, Vertex};

/// Render meshes of `mesh`: element 0 has every face, element `k + 1` the faces of material
/// `k`. `world` = the object's matrix, for faces with world-space UVs.
pub fn bake(mesh: &EditableMesh, world: Option<&Mat4>) -> Vec<MeshData> {
    let normals: Vec<Vec3> = (0..mesh.faces.len()).map(|f| mesh.face_normal(f)).collect();
    // Smooth normals per (vertex, smoothing group).
    let mut smooth: HashMap<(u32, u32), Vec3> = HashMap::new();
    for (f, face) in mesh.faces.iter().enumerate() {
        if face.smoothing == 0 {
            continue;
        }
        for v in &face.verts {
            *smooth.entry((*v, face.smoothing)).or_insert(Vec3::ZERO) += normals[f];
        }
    }
    let count = mesh.material_count();
    let mut all = MeshData::default();
    let mut parts = vec![MeshData::default(); count];
    for (f, face) in mesh.faces.iter().enumerate() {
        let n = normals[f];
        let pts: Vec<Vec3> = face.verts.iter().map(|v| mesh.position(*v)).collect();
        let uvs = face_uvs(&pts, n, &face.uv, world);
        let tris = super::triangulate(&pts);
        for out in [&mut all, &mut parts[face.material as usize]] {
            let base = out.vertices.len() as u32;
            for (k, p) in pts.iter().enumerate() {
                let normal = match face.smoothing {
                    0 => n,
                    s => smooth[&(face.verts[k], s)].normalize_or(n),
                };
                out.vertices.push(Vertex {
                    position: p.to_array(),
                    normal: normal.to_array(),
                    uv: uvs[k].to_array(),
                    uv2: [0.0; 2],
                    tangent: [1.0, 0.0, 0.0, 1.0],
                });
            }
            for t in &tris {
                out.indices
                    .extend([base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]);
            }
        }
    }
    let mut out = Vec::with_capacity(count + 1);
    out.push(all);
    out.extend(parts);
    for m in &mut out {
        m.compute_tangents();
    }
    out
}

/// Planar projection axes (u, v) for a face normal: the axis plane closest to the face, seen
/// from outside with +Y (or +Z for floors) up. Image v grows downwards.
pub fn projection_axes(n: Vec3) -> (Vec3, Vec3) {
    let a = n.abs();
    if a.y >= a.x && a.y >= a.z {
        if n.y >= 0.0 {
            (Vec3::X, -Vec3::Z)
        } else {
            (-Vec3::X, -Vec3::Z)
        }
    } else if a.x >= a.z {
        if n.x >= 0.0 {
            (Vec3::Z, -Vec3::Y)
        } else {
            (-Vec3::Z, -Vec3::Y)
        }
    } else if n.z >= 0.0 {
        (-Vec3::X, -Vec3::Y)
    } else {
        (Vec3::X, -Vec3::Y)
    }
}

/// UVs of a face's corners (local positions `pts`, local normal `n`).
pub fn face_uvs(pts: &[Vec3], n: Vec3, uv: &FaceUv, world: Option<&Mat4>) -> Vec<Vec2> {
    let (pts, n): (Vec<Vec3>, Vec3) = match (uv.world, world) {
        (true, Some(m)) => (
            pts.iter().map(|p| m.transform_point3(*p)).collect(),
            m.inverse().transpose().transform_vector3(n).normalize_or(n),
        ),
        _ => (pts.to_vec(), n),
    };
    let (u, v) = projection_axes(n);
    let mut out: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p.dot(u), p.dot(v))).collect();
    if !uv.world {
        // Object space: anchored at the face's corner, so each face starts a fresh tile.
        let min = out.iter().fold(Vec2::splat(f32::MAX), |a, b| a.min(*b));
        for p in &mut out {
            *p -= min;
        }
    }
    let rot = Vec2::from_angle(uv.rotation.to_radians());
    for p in &mut out {
        *p = rot.rotate(*p) * uv.scale + uv.offset;
    }
    out
}
