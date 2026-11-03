//! `.gltf` / `.glb` import: triangle primitives become meshes, the default scene's nodes become
//! the hierarchy, PBR metallic-roughness materials become Standard materials.

use std::path::Path;

use glam::{Quat, Vec3};

use super::{Model, ModelMaterial, ModelMesh, ModelNode, TextureSource, mirror_rotation, mirror_x};
use crate::assets::Material;
use crate::render::mesh::{MeshData, Vertex};

pub fn load(path: &Path) -> Result<Model, String> {
    let err = |e: gltf::Error| format!("{}: {e}", path.display());
    let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let gltf = gltf::Gltf::from_slice(&bytes).map_err(err)?;
    let buffers = buffers(&gltf, path).map_err(err)?;
    let doc = &gltf.document;
    let dir = path.parent().unwrap_or(Path::new("."));

    let mut model = Model::default();
    // glTF mesh index → our mesh indices (one per triangle primitive).
    let mut mesh_map: Vec<Vec<usize>> = Vec::new();
    for mesh in doc.meshes() {
        let base = mesh
            .name()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Mesh{}", mesh.index()));
        let count = mesh.primitives().len();
        let mut ours = Vec::new();
        for (k, prim) in mesh.primitives().enumerate() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let mut vertices: Vec<Vertex> = positions
                .map(|p| Vertex {
                    position: p,
                    normal: [0.0; 3],
                    uv: [0.0; 2],
                    tangent: [1.0, 0.0, 0.0, 1.0],
                })
                .collect();
            if let Some(normals) = reader.read_normals() {
                for (v, n) in vertices.iter_mut().zip(normals) {
                    v.normal = n;
                }
            }
            if let Some(uvs) = reader.read_tex_coords(0) {
                for (v, uv) in vertices.iter_mut().zip(uvs.into_f32()) {
                    v.uv = uv;
                }
            }
            let mut indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..vertices.len() as u32).collect(),
            };
            indices.truncate(indices.len() / 3 * 3);
            let mut data = MeshData { vertices, indices };
            if data
                .indices
                .iter()
                .any(|i| *i as usize >= data.vertices.len())
            {
                return Err(format!("{}: index out of range", path.display()));
            }
            mirror_x(&mut data);
            let name = if count > 1 {
                format!("{base} ({k})")
            } else {
                base.clone()
            };
            ours.push(model.meshes.len());
            model
                .meshes
                .push(ModelMesh::new(name, data, prim.material().index()));
        }
        mesh_map.push(ours);
    }

    // Hierarchy of the default (or first) scene; all root nodes if there is no scene.
    let roots: Vec<gltf::Node> = match doc.default_scene().or_else(|| doc.scenes().next()) {
        Some(scene) => scene.nodes().collect(),
        None => {
            let children: Vec<usize> = doc
                .nodes()
                .flat_map(|n| n.children().map(|c| c.index()).collect::<Vec<_>>())
                .collect();
            doc.nodes()
                .filter(|n| !children.contains(&n.index()))
                .collect()
        }
    };
    let mut stack: Vec<(gltf::Node, Option<usize>)> =
        roots.into_iter().rev().map(|n| (n, None)).collect();
    while let Some((node, parent)) = stack.pop() {
        let (t, r, s) = node.transform().decomposed();
        let index = model.nodes.len();
        model.nodes.push(ModelNode {
            name: node
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Node{}", node.index())),
            parent,
            position: Vec3::new(-t[0], t[1], t[2]),
            rotation: mirror_rotation(Quat::from_array(r)).normalize(),
            scale: Vec3::from(s),
            meshes: node
                .mesh()
                .map(|m| mesh_map[m.index()].clone())
                .unwrap_or_default(),
        });
        let children: Vec<_> = node.children().collect();
        for c in children.into_iter().rev() {
            stack.push((c, Some(index)));
        }
    }

    // Listed in index order, so `model.materials[i]` is glTF material i.
    for m in doc.materials() {
        let i = model.materials.len();
        let pbr = m.pbr_metallic_roughness();
        let [r, g, b, a] = pbr.base_color_factor();
        let e = m.emissive_factor();
        let mut material = Material {
            // glTF factors are linear; materials store sRGB.
            color: [to_srgb(r), to_srgb(g), to_srgb(b), a],
            metallic: pbr.metallic_factor(),
            smoothness: 1.0 - pbr.roughness_factor(),
            ..Default::default()
        };
        if e.iter().any(|c| *c > 0.0) {
            material.emission_enabled = true;
            material.emission = e.map(to_srgb);
        }
        let normal = m.normal_texture();
        if let Some(n) = &normal {
            material.normal_scale = n.scale();
        }
        model.materials.push(ModelMaterial {
            name: m
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Material{i}")),
            material,
            albedo: pbr
                .base_color_texture()
                .and_then(|t| image_source(t.texture().source(), &buffers, dir)),
            normal: normal.and_then(|t| image_source(t.texture().source(), &buffers, dir)),
        });
    }
    Ok(model)
}

fn to_srgb(c: f32) -> f32 {
    c.max(0.0).powf(1.0 / 2.2)
}

fn image_source(
    image: gltf::Image,
    buffers: &[gltf::buffer::Data],
    dir: &Path,
) -> Option<TextureSource> {
    match image.source() {
        gltf::image::Source::View { view, .. } => {
            let data = &buffers.get(view.buffer().index())?.0;
            let bytes = data.get(view.offset()..view.offset() + view.length())?;
            Some(TextureSource::Embedded(bytes.to_vec()))
        }
        // Data URIs are not supported for images; external files are referenced in place.
        gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => {
            Some(TextureSource::File(dir.join(uri.replace("%20", " "))))
        }
        gltf::image::Source::Uri { .. } => None,
    }
}

/// Buffer data: the GLB blob, data URIs, or `.bin` files next to the model. When a pak is
/// mounted the files are read from it (the glTF crate itself only reads the disk).
fn buffers(gltf: &gltf::Gltf, path: &Path) -> gltf::Result<Vec<gltf::buffer::Data>> {
    use gltf::buffer::Source;
    let is_file = |s: &Source| matches!(s, Source::Uri(u) if !u.starts_with("data:"));
    let doc = &gltf.document;
    if !crate::vfs::is_mounted() || !doc.buffers().any(|b| is_file(&b.source())) {
        return gltf::import_buffers(doc, path.parent(), gltf.blob.clone());
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    doc.buffers()
        .map(|b| {
            let mut data = match b.source() {
                Source::Bin => gltf.blob.clone().unwrap_or_default(),
                Source::Uri(uri) if !uri.starts_with("data:") => {
                    crate::vfs::read(dir.join(uri.replace("%20", " "))).map_err(gltf::Error::Io)?
                }
                // Data URIs mixed with external files are not supported in packed builds.
                Source::Uri(_) => return Err(gltf::Error::UnsupportedScheme),
            };
            while data.len() % 4 != 0 {
                data.push(0);
            }
            Ok(gltf::buffer::Data(data))
        })
        .collect()
}
