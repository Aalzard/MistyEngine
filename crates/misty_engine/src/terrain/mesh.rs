//! Terrain mesh: one vertex per heightmap sample (no LOD), smooth normals from the heightmap,
//! uv 0..1 over the whole terrain (the splat map), tangents along +x for layer normal maps.

use glam::Vec3;

use super::data::TerrainData;
use crate::render::mesh::{MeshData, Vertex};

/// Mesh in terrain-local space (origin at the corner).
pub fn build(d: &TerrainData) -> MeshData {
    let r = d.heightmap_resolution;
    let s = d.spacing();
    let cells = (r - 1) as f32;
    let mut vertices = Vec::with_capacity(r * r);
    for iz in 0..r {
        for ix in 0..r {
            let n = d.vertex_normal(ix, iz);
            // Tangent along +x, lying in the surface.
            let t = (Vec3::X - n * n.x).normalize_or(Vec3::X);
            vertices.push(Vertex {
                position: [
                    ix as f32 * s.x,
                    d.height(ix, iz) * d.size.y,
                    iz as f32 * s.y,
                ],
                normal: n.into(),
                uv: [ix as f32 / cells, iz as f32 / cells],
                uv2: [0.0; 2],
                tangent: [t.x, t.y, t.z, 1.0],
            });
        }
    }
    let mut indices = Vec::with_capacity((r - 1) * (r - 1) * 6);
    for iz in 0..r - 1 {
        for ix in 0..r - 1 {
            let i00 = (iz * r + ix) as u32;
            let i10 = i00 + 1;
            let i01 = i00 + r as u32;
            let i11 = i01 + 1;
            // Diagonal (x0, z1)–(x1, z0); `cross(b - a, c - a)` points up.
            indices.extend([i00, i01, i10, i01, i11, i10]);
        }
    }
    MeshData { vertices, indices, colors: Vec::new() }
}
