//! Static batching: draws of Static objects that share a material (and shadow settings) are
//! merged into one mesh per region chunk, with the vertices moved into world space. Each chunk
//! is still culled on its own, and it is one draw instead of one per object.
//!
//! Like any static batching, the merged vertices are transformed on the CPU, so positions can
//! differ from the GPU's per-object transform by float rounding.

use glam::{IVec3, Mat4, Vec3, Vec4Swizzles};

use super::MeshDraw;
use super::mesh::{MeshData, Vertex};
use crate::models;

/// Edge of a batching region, in metres.
pub const CHUNK_SIZE: f32 = 16.0;
/// Largest merged mesh (vertices); bigger groups start another batch.
const MAX_VERTICES: usize = 1 << 20;

struct Group {
    template: MeshDraw,
    chunk: IVec3,
    data: MeshData,
}

/// Merge `draws` (all of Static objects) into combined draws registered under `name`.
/// Draws whose mesh can't be read are kept as they are.
pub fn combine(name: &str, draws: &[MeshDraw]) -> Vec<MeshDraw> {
    models::runtime::unregister(&format!("{name}/"));
    let mut groups: Vec<Group> = Vec::new();
    let mut out = Vec::new();
    for d in draws {
        let Some(mesh) = models::mesh(&d.mesh) else {
            out.push(d.clone());
            continue;
        };
        let (lo, hi) = mesh.bounds();
        let center = d.model.transform_point3((lo + hi) * 0.5);
        let chunk = (center / CHUNK_SIZE).floor().as_ivec3();
        let fits = |g: &Group| {
            g.chunk == chunk
                && g.template.material == d.material
                && g.template.cast_shadows == d.cast_shadows
                && g.template.receive_shadows == d.receive_shadows
                && g.data.vertices.len() + mesh.vertices.len() <= MAX_VERTICES
        };
        let group = match groups.iter().position(fits) {
            Some(i) => &mut groups[i],
            None => {
                groups.push(Group {
                    template: d.clone(),
                    chunk,
                    data: MeshData::default(),
                });
                groups.last_mut().expect("just pushed")
            }
        };
        append(&mut group.data, &mesh, &d.model);
    }
    for (i, g) in groups.into_iter().enumerate() {
        let key = models::runtime::register(&format!("{name}/{i}"), g.data);
        out.push(MeshDraw {
            mesh: crate::scene::MeshRef::Asset(key),
            model: Mat4::IDENTITY,
            outline: false,
            lod: None,
            ..g.template
        });
    }
    out
}

/// Append `src` moved by `model` (normals and tangents like the vertex shader does them).
fn append(dst: &mut MeshData, src: &MeshData, model: &Mat4) {
    let normal = model.inverse().transpose();
    let mirror = if model.determinant() < 0.0 { -1.0 } else { 1.0 };
    let base = dst.vertices.len() as u32;
    dst.vertices.extend(src.vertices.iter().map(|v| {
        let p = *model * Vec3::from(v.position).extend(1.0);
        let n = normal * Vec3::from(v.normal).extend(0.0);
        let t = *model * Vec3::from_slice(&v.tangent[..3]).extend(0.0);
        Vertex {
            position: p.xyz().into(),
            normal: n.xyz().into(),
            uv: v.uv,
            tangent: [t.x, t.y, t.z, v.tangent[3] * mirror],
        }
    }));
    // Same winding as the GPU sees under the object's own matrix.
    dst.indices.extend(src.indices.iter().map(|i| base + i));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::MaterialParams;
    use crate::scene::PrimitiveMesh;

    fn cube(x: f32, color: f32) -> MeshDraw {
        MeshDraw {
            mesh: PrimitiveMesh::Cube.into(),
            model: Mat4::from_translation(Vec3::new(x, 0.0, 0.0)),
            material: MaterialParams {
                color: [color, 1.0, 1.0, 1.0],
                ..Default::default()
            },
            cast_shadows: true,
            receive_shadows: true,
            outline: false,
            sorting: Default::default(),
            lod: None,
        }
    }

    #[test]
    fn groups_by_material_and_chunk() {
        // Two materials near the origin, one far away: three batches.
        let draws = [
            cube(0.0, 1.0),
            cube(2.0, 1.0),
            cube(4.0, 0.5),
            cube(40.0, 1.0),
        ];
        let out = combine("test-groups", &draws);
        assert_eq!(out.len(), 3);
        let first = models::mesh(&out[0].mesh).expect("registered");
        let one = models::mesh(&PrimitiveMesh::Cube.into()).unwrap();
        assert_eq!(first.vertices.len(), one.vertices.len() * 2);
        assert_eq!(first.indices.len(), one.indices.len() * 2);
        // Second copy moved by 2 m, indices offset.
        let n = one.vertices.len();
        let moved = Vec3::from(first.vertices[n].position) - Vec3::from(one.vertices[0].position);
        assert!(moved.abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-6));
        assert_eq!(first.indices[one.indices.len()], one.indices[0] + n as u32);
        assert_eq!(out[0].model, Mat4::IDENTITY);
        models::runtime::unregister("test-groups/");
        assert!(models::mesh(&out[0].mesh).is_none());
    }
}
