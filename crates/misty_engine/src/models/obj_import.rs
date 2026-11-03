//! Wavefront `.obj` import: each object/group is one mesh and one node; `.mtl` materials are read
//! when present (diffuse colour, diffuse and normal textures).

use std::path::Path;

use glam::{Quat, Vec3};

use super::{Model, ModelMaterial, ModelMesh, ModelNode, TextureSource, mirror_x};
use crate::assets::Material;
use crate::render::mesh::{MeshData, Vertex};

pub fn load(path: &Path) -> Result<Model, String> {
    let options = tobj::LoadOptions {
        single_index: true,
        triangulate: true,
        ignore_points: true,
        ignore_lines: true,
    };
    let (objects, materials) =
        tobj::load_obj(path, &options).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));

    let mut model = Model::default();
    for (i, obj) in objects.into_iter().enumerate() {
        let m = obj.mesh;
        let count = m.positions.len() / 3;
        let vertices = (0..count)
            .map(|k| Vertex {
                position: [
                    m.positions[k * 3],
                    m.positions[k * 3 + 1],
                    m.positions[k * 3 + 2],
                ],
                normal: if m.normals.len() >= (k + 1) * 3 {
                    [m.normals[k * 3], m.normals[k * 3 + 1], m.normals[k * 3 + 2]]
                } else {
                    [0.0; 3]
                },
                // OBJ v grows upwards; ours grows downwards.
                uv: if m.texcoords.len() >= (k + 1) * 2 {
                    [m.texcoords[k * 2], 1.0 - m.texcoords[k * 2 + 1]]
                } else {
                    [0.0; 2]
                },
                tangent: [1.0, 0.0, 0.0, 1.0],
            })
            .collect();
        let mut data = MeshData {
            vertices,
            indices: m.indices,
        };
        if data.indices.iter().any(|i| *i as usize >= count) {
            return Err(format!("{}: index out of range", path.display()));
        }
        mirror_x(&mut data);
        let name = if obj.name.is_empty() {
            format!("Object{i}")
        } else {
            obj.name
        };
        model.nodes.push(ModelNode {
            name: name.clone(),
            parent: None,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            meshes: vec![model.meshes.len()],
        });
        model.meshes.push(ModelMesh::new(name, data, m.material_id));
    }

    // A missing or broken .mtl only loses the materials.
    for (i, m) in materials.unwrap_or_default().into_iter().enumerate() {
        let d = m.diffuse.unwrap_or([1.0; 3]);
        let texture = |t: Option<String>| {
            t.map(|t| dir.join(t.replace('\\', "/")))
                .filter(|p| p.is_file())
                .map(TextureSource::File)
        };
        model.materials.push(ModelMaterial {
            name: if m.name.is_empty() {
                format!("Material{i}")
            } else {
                m.name
            },
            material: Material {
                color: [d[0], d[1], d[2], m.dissolve.unwrap_or(1.0)],
                ..Default::default()
            },
            albedo: texture(m.diffuse_texture),
            normal: texture(m.normal_texture),
        });
    }
    Ok(model)
}
