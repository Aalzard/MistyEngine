//! `.fbx` import through ufbx (binary and ASCII FBX, 6.x and 7.x).
//!
//! ufbx converts the file to glTF's space (right-handed, Y up; metres with Convert Units on),
//! baking unit scale and geometric pivots into the geometry. From there the model is built like
//! a glTF one: X mirrored into engine space, one mesh per material part (like glTF primitives),
//! the node hierarchy, skins with inverse bind matrices, blend shapes, and every animation
//! stack baked and resampled into a clip ([`super::gltf_anim::SAMPLE_RATE`] keys per second).
//! Materials map FBX (Lambert/Phong) and PBR (Stingray, 3ds Max Physical, Blender) properties
//! to Standard materials; textures resolve next to the file, or are embedded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3, Vec4};

use super::gltf_anim::{SAMPLE_RATE, continuous_euler, mirror_matrix, simplify};
use super::settings::{AxisConversion, ModelImporter};
use super::skin::{BlendShape, MeshSkin, ModelAnimation, ModelSkin};
use super::{Model, ModelMaterial, ModelMesh, ModelNode, TextureSource, mirror_rotation, mirror_x};
use crate::animation::{AnimationClip, Property, TangentMode};
use crate::assets::Material;
use crate::render::mesh::{MeshData, Vertex};

pub fn load(path: &Path, settings: &ModelImporter) -> Result<Model, String> {
    let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let filename = path.to_string_lossy().into_owned();
    let axes = match settings.axis_conversion {
        // Right-handed Y up, like glTF (then mirrored into engine space).
        AxisConversion::Automatic => ufbx::CoordinateAxes {
            right: ufbx::CoordinateAxis::PositiveX,
            up: ufbx::CoordinateAxis::PositiveY,
            front: ufbx::CoordinateAxis::PositiveZ,
        },
        // Forced axes: the file is read as it is (Z Up is turned afterwards, like glTF).
        AxisConversion::YUp | AxisConversion::ZUp => ufbx::CoordinateAxes::default(),
    };
    let opts = ufbx::LoadOpts {
        target_axes: axes,
        target_unit_meters: if settings.convert_units { 1.0 } else { 0.0 },
        space_conversion: ufbx::SpaceConversion::ModifyGeometry,
        geometry_transform_handling: ufbx::GeometryTransformHandling::ModifyGeometry,
        inherit_mode_handling: ufbx::InheritModeHandling::Compensate,
        clean_skin_weights: true,
        ignore_missing_external_files: true,
        filename: filename.as_str().into(),
        ..Default::default()
    };
    let scene = ufbx::load_memory(&bytes, opts)
        .map_err(|e| format!("{}: {} {}", path.display(), &*e.description, e.info()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    build(&scene, dir)
}

fn v3(v: ufbx::Vec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

fn quat(q: ufbx::Quat) -> Quat {
    Quat::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)
}

fn mat4(m: &ufbx::Matrix) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(m.m00 as f32, m.m10 as f32, m.m20 as f32, 0.0),
        Vec4::new(m.m01 as f32, m.m11 as f32, m.m21 as f32, 0.0),
        Vec4::new(m.m02 as f32, m.m12 as f32, m.m22 as f32, 0.0),
        Vec4::new(m.m03 as f32, m.m13 as f32, m.m23 as f32, 1.0),
    )
}

/// File-space (right-handed) vector → engine space.
fn mirror(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, v.y, v.z)
}

fn build(scene: &ufbx::Scene, dir: &Path) -> Result<Model, String> {
    let mut model = Model::default();
    // ufbx node typed id → our node.
    let mut node_map: HashMap<u32, usize> = HashMap::new();
    // ufbx mesh typed id → our meshes (one per material part), shared by instances.
    let mut mesh_map: HashMap<u32, Vec<usize>> = HashMap::new();
    // Blend channel element id → (our node, shape index) for animation.
    let mut channels: HashMap<u32, (usize, u8)> = HashMap::new();
    // Skin deformer typed id → our skin.
    let mut skin_map: HashMap<u32, usize> = HashMap::new();

    // Depth first from the root's children: parents before children.
    let mut stack: Vec<(&ufbx::Node, Option<usize>)> = scene
        .root_node
        .children
        .iter()
        .rev()
        .map(|n| (n.as_ref(), None))
        .collect();
    while let Some((node, parent)) = stack.pop() {
        let index = model.nodes.len();
        node_map.insert(node.element.typed_id, index);
        let t = &node.local_transform;
        let name = if node.element.name.is_empty() {
            format!("Node{}", node.element.typed_id)
        } else {
            node.element.name.to_string()
        };
        let mut meshes = Vec::new();
        if let Some(mesh) = &node.mesh {
            let id = mesh.element.typed_id;
            meshes = mesh_map
                .entry(id)
                .or_insert_with(|| convert_mesh(mesh, node, &mut model))
                .clone();
            // Blend shape channels, in the order the meshes list them.
            let mut k = 0u8;
            for d in mesh.blend_deformers.iter() {
                for ch in d.channels.iter() {
                    channels.insert(ch.element.element_id, (index, k));
                    k = k.saturating_add(1);
                }
            }
        }
        model.nodes.push(ModelNode {
            name,
            parent,
            position: mirror(v3(t.translation)),
            rotation: mirror_rotation(quat(t.rotation)).normalize(),
            scale: v3(t.scale),
            meshes,
            skin: None,
            lods: Vec::new(),
        });
        for c in node.children.iter().rev() {
            stack.push((c.as_ref(), Some(index)));
        }
    }

    // Skins: one per skin deformer, joints are the clusters' bone nodes.
    for node in scene.nodes.iter() {
        let Some(&ours) = node_map.get(&node.element.typed_id) else {
            continue;
        };
        let Some(deformer) = node.mesh.as_ref().and_then(|m| m.skin_deformers.first()) else {
            continue;
        };
        let id = deformer.element.typed_id;
        let skin = match skin_map.get(&id) {
            Some(s) => *s,
            None => {
                let joints: Vec<usize> = deformer
                    .clusters
                    .iter()
                    .map(|c| {
                        c.bone_node
                            .as_ref()
                            .and_then(|b| node_map.get(&b.element.typed_id).copied())
                            .unwrap_or(ours)
                    })
                    .collect();
                let inverse_bind = deformer
                    .clusters
                    .iter()
                    .map(|c| mirror_matrix(mat4(&c.geometry_to_bone)))
                    .collect();
                // The joint whose parent is not a joint.
                let root = joints
                    .iter()
                    .copied()
                    .find(|j| model.nodes[*j].parent.is_none_or(|p| !joints.contains(&p)));
                model.skins.push(ModelSkin {
                    name: if deformer.element.name.is_empty() {
                        format!("Skin{}", model.skins.len())
                    } else {
                        deformer.element.name.to_string()
                    },
                    joints,
                    inverse_bind,
                    root,
                });
                skin_map.insert(id, model.skins.len() - 1);
                model.skins.len() - 1
            }
        };
        model.nodes[ours].skin = Some(skin);
    }
    model.bind_skins();

    for m in scene.materials.iter() {
        model.materials.push(material(m, dir));
    }
    model.animations = animations(scene, &node_map, &channels, &model);
    Ok(model)
}

/// Position and normal offsets of a blend shape, per control point.
type Offsets = (Vec<[f32; 3]>, Vec<[f32; 3]>);

/// Vertex key for welding polygon corners: control point and attribute bits.
type CornerKey = (u32, [u32; 3], [u32; 2], [u32; 2], [u32; 3]);

/// A mesh's material parts as engine meshes (appended to `model.meshes`); returns their indices.
fn convert_mesh(mesh: &ufbx::Mesh, node: &ufbx::Node, model: &mut Model) -> Vec<usize> {
    let base = if mesh.element.name.is_empty() {
        node.element.name.to_string()
    } else {
        mesh.element.name.to_string()
    };
    // Per-instance materials of the first node using the mesh (FBX keeps them on the node).
    let materials: Vec<u32> = if node.materials.count > 0 {
        node.materials.iter().map(|m| m.element.typed_id).collect()
    } else {
        mesh.materials.iter().map(|m| m.element.typed_id).collect()
    };
    // (material slot, faces) per part; one part with every face when there are no materials.
    let parts: Vec<(Option<u32>, Vec<u32>)> = if mesh.material_parts.count > 0 {
        mesh.material_parts
            .iter()
            .filter(|p| p.num_triangles > 0)
            .map(|p| (Some(p.index), p.face_indices.to_vec()))
            .collect()
    } else {
        vec![(None, (0..mesh.num_faces as u32).collect())]
    };
    let uv2 = mesh.uv_sets.get(1).map(|s| &s.vertex_uv);
    let has_tangents = mesh.vertex_tangent.exists && mesh.vertex_bitangent.exists;
    let skin = mesh.skin_deformers.first();
    let shapes: Vec<(String, f32, Option<&ufbx::BlendShape>)> = mesh
        .blend_deformers
        .iter()
        .flat_map(|d| d.channels.iter())
        .map(|ch| {
            let shape = ch
                .target_shape
                .as_ref()
                .or_else(|| ch.keyframes.last().map(|k| &k.shape))
                .map(|s| s.as_ref());
            (ch.element.name.to_string(), ch.weight as f32 * 100.0, shape)
        })
        .collect();
    // Per control point offsets of each shape.
    let shape_offsets: Vec<Offsets> = shapes
        .iter()
        .map(|(_, _, s)| {
            let mut pos = vec![[0.0f32; 3]; mesh.num_vertices];
            let mut nrm = vec![[0.0f32; 3]; mesh.num_vertices];
            if let Some(s) = s {
                for (k, &v) in s.offset_vertices.iter().enumerate() {
                    let v = v as usize;
                    if v < pos.len() {
                        if let Some(p) = s.position_offsets.get(k) {
                            pos[v] = mirror(v3(*p)).into();
                        }
                        if let Some(n) = s.normal_offsets.get(k) {
                            nrm[v] = mirror(v3(*n)).into();
                        }
                    }
                }
            }
            (pos, nrm)
        })
        .collect();

    let mut tri = vec![0u32; mesh.max_face_triangles.max(1) * 3];
    let count = parts.len();
    let mut ours = Vec::new();
    for (k, (slot, faces)) in parts.into_iter().enumerate() {
        let mut data = MeshData::default();
        // Control point of each vertex (skin weights and blend shapes are per control point).
        let mut points: Vec<u32> = Vec::new();
        let mut seen: HashMap<CornerKey, u32> = HashMap::new();
        for &f in &faces {
            let Some(face) = mesh.faces.get(f as usize).copied() else {
                continue;
            };
            if face.num_indices < 3 {
                continue;
            }
            let n = ufbx::triangulate_face(&mut tri, mesh, face) as usize;
            for &ix in &tri[..n * 3] {
                let ix = ix as usize;
                let cp = mesh.vertex_indices[ix];
                let p = v3(mesh.vertex_position[ix]);
                let normal = if mesh.vertex_normal.exists {
                    v3(mesh.vertex_normal[ix])
                } else {
                    Vec3::ZERO
                };
                // FBX v grows upwards; ours grows downwards.
                let uv = if mesh.vertex_uv.exists {
                    let t = mesh.vertex_uv[ix];
                    [t.x as f32, 1.0 - t.y as f32]
                } else {
                    [0.0; 2]
                };
                let uv2 = uv2.filter(|u| u.exists).map_or([0.0; 2], |u| {
                    let t = u[ix];
                    [t.x as f32, 1.0 - t.y as f32]
                });
                let tangent = if has_tangents {
                    let t = v3(mesh.vertex_tangent[ix]);
                    let b = v3(mesh.vertex_bitangent[ix]);
                    let w = if normal.cross(t).dot(b) < 0.0 {
                        -1.0
                    } else {
                        1.0
                    };
                    [t.x, t.y, t.z, w]
                } else {
                    [1.0, 0.0, 0.0, 1.0]
                };
                let key: CornerKey = (
                    cp,
                    normal.to_array().map(f32::to_bits),
                    uv.map(f32::to_bits),
                    uv2.map(f32::to_bits),
                    [tangent[0], tangent[1], tangent[2]].map(f32::to_bits),
                );
                let index = *seen.entry(key).or_insert_with(|| {
                    data.vertices.push(Vertex {
                        position: p.into(),
                        normal: normal.into(),
                        uv,
                        uv2,
                        tangent,
                    });
                    points.push(cp);
                    data.vertices.len() as u32 - 1
                });
                data.indices.push(index);
            }
        }
        if data.indices.is_empty() {
            continue;
        }
        // ufbx triangulates counter-clockwise like glTF: same mirroring as glTF.
        mirror_x(&mut data);
        let name = if count > 1 {
            format!("{base} ({k})")
        } else {
            base.clone()
        };
        let material = slot
            .and_then(|s| materials.get(s as usize))
            .map(|m| *m as usize);
        let mut m = ModelMesh::with_vectors(name, data, material, has_tangents);
        if let Some(d) = skin {
            let mut joints = Vec::with_capacity(points.len());
            let mut weights = Vec::with_capacity(points.len());
            for &cp in &points {
                let mut j = [0u16; 4];
                let mut w = [0.0f32; 4];
                if let Some(sv) = d.vertices.get(cp as usize) {
                    let begin = sv.weight_begin as usize;
                    let mut list: Vec<(u32, f32)> = (begin..begin + sv.num_weights as usize)
                        .filter_map(|i| d.weights.get(i))
                        .map(|sw| (sw.cluster_index, sw.weight as f32))
                        .collect();
                    list.sort_by(|a, b| b.1.total_cmp(&a.1));
                    for (slot, (c, wt)) in list.into_iter().take(4).enumerate() {
                        j[slot] = c as u16;
                        w[slot] = wt;
                    }
                }
                joints.push(j);
                weights.push(w);
            }
            m.skin = Some(MeshSkin::new(joints, weights));
        }
        for ((name, weight, _), (pos, nrm)) in shapes.iter().zip(&shape_offsets) {
            m.blend_shapes.push(BlendShape {
                name: name.clone(),
                positions: points.iter().map(|&cp| pos[cp as usize]).collect(),
                normals: points.iter().map(|&cp| nrm[cp as usize]).collect(),
            });
            m.default_weights.push(*weight);
        }
        ours.push(model.meshes.len());
        model.meshes.push(m);
    }
    // Several parts (one per material): one virtual geometry build of parts, like glTF.
    if ours.len() > 1 {
        for (slot, &i) in ours.iter().enumerate() {
            model.meshes[i].primitive = Some((ours[0], slot as u32));
        }
    }
    ours
}

// ---- materials -----------------------------------------------------------------------------

fn material(m: &ufbx::Material, dir: &Path) -> ModelMaterial {
    let pbr = &m.pbr;
    let fbx = &m.fbx;
    let rgb = |map: &ufbx::MaterialMap| {
        let v = map.value_vec4;
        [v.x as f32, v.y as f32, v.z as f32]
    };
    let scalar = |map: &ufbx::MaterialMap, default: f32| {
        if map.has_value {
            map.value_vec4.x as f32
        } else {
            default
        }
    };
    // FBX colours are display (sRGB) values, as material colours are stored.
    let base = if pbr.base_color.has_value {
        rgb(&pbr.base_color)
    } else {
        rgb(&fbx.diffuse_color)
    };
    let factor = scalar(&pbr.base_factor, 1.0);
    let alpha = scalar(&pbr.opacity, 1.0).clamp(0.0, 1.0);
    let mut material = Material {
        color: [base[0] * factor, base[1] * factor, base[2] * factor, alpha],
        metallic: scalar(&pbr.metalness, 0.0).clamp(0.0, 1.0),
        smoothness: if pbr.roughness.has_value {
            (1.0 - pbr.roughness.value_vec4.x as f32).clamp(0.0, 1.0)
        } else {
            0.5
        },
        ..Default::default()
    };
    let emission = rgb(&pbr.emission_color);
    let e = scalar(&pbr.emission_factor, 1.0);
    if pbr.emission_color.has_value && emission.iter().any(|c| *c * e > 0.0) {
        material.emission_enabled = true;
        material.emission = emission.map(|c| c * e);
    }
    let texture = |maps: &[&ufbx::MaterialMap]| {
        maps.iter()
            .find_map(|map| map.texture.as_ref())
            .and_then(|t| texture_source(t, dir))
    };
    let albedo = texture(&[&pbr.base_color, &fbx.diffuse_color]);
    let normal = texture(&[&pbr.normal_map, &fbx.normal_map, &fbx.bump]);
    if normal.is_some() {
        material.normal_scale = scalar(&fbx.bump_factor, 1.0);
    }
    ModelMaterial {
        name: if m.element.name.is_empty() {
            format!("Material{}", m.element.typed_id)
        } else {
            m.element.name.to_string()
        },
        material,
        albedo,
        normal,
        extra_maps: Vec::new(),
    }
}

/// Where a texture's image is: embedded bytes, or the file found next to the model (its
/// relative path, its own folder, a Textures folder beside or above it, or its absolute path).
/// Images outside the project are read so extraction copies them in.
fn texture_source(t: &ufbx::Texture, dir: &Path) -> Option<TextureSource> {
    let t = t.file_textures.first().map(|f| f.as_ref()).unwrap_or(t);
    if !t.content.is_empty() {
        return Some(TextureSource::Embedded(t.content.to_vec()));
    }
    let name_of = |s: &str| {
        Path::new(&s.replace('\\', "/"))
            .file_name()
            .map(|n| n.to_owned())
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    for s in [&*t.relative_filename, &*t.filename] {
        if !s.is_empty() {
            candidates.push(dir.join(s.replace('\\', "/")));
        }
    }
    if let Some(name) = [&*t.relative_filename, &*t.filename, &*t.absolute_filename]
        .into_iter()
        .find_map(name_of)
    {
        candidates.push(dir.join(&name));
        candidates.push(dir.join("Textures").join(&name));
        if let Some(up) = dir.parent() {
            candidates.push(up.join("Textures").join(&name));
        }
    }
    if !t.absolute_filename.is_empty() {
        candidates.push(PathBuf::from(t.absolute_filename.replace('\\', "/")));
    }
    let found = candidates.into_iter().find(|p| crate::vfs::is_file(p))?;
    let inside = !Path::new(&crate::assets::relative(&found)).is_absolute();
    if inside || crate::vfs::is_mounted() {
        Some(TextureSource::File(found))
    } else {
        crate::vfs::read(&found).ok().map(TextureSource::Embedded)
    }
}

// ---- animation -----------------------------------------------------------------------------

/// Every animation stack as a clip: node transforms and blend shape weights, baked by ufbx and
/// resampled at [`SAMPLE_RATE`] with linear keys (rotations as continuous Euler curves).
fn animations(
    scene: &ufbx::Scene,
    node_map: &HashMap<u32, usize>,
    channels: &HashMap<u32, (usize, u8)>,
    model: &Model,
) -> Vec<ModelAnimation> {
    let mut out = Vec::new();
    for stack in scene.anim_stacks.iter() {
        let opts = ufbx::BakeOpts {
            resample_rate: SAMPLE_RATE as f64,
            ..Default::default()
        };
        let Ok(bake) = ufbx::bake_anim(scene, &stack.anim, opts) else {
            continue;
        };
        // The stack's time span; files without one use the span of their keys.
        let (begin, end) = if stack.time_end > stack.time_begin {
            (stack.time_begin, stack.time_end)
        } else {
            (bake.key_time_min, bake.key_time_max)
        };
        let length = ((end - begin) as f32)
            .max(0.0)
            .min(crate::animation::MAX_IMPORT_LENGTH);
        let mut clip = AnimationClip {
            length,
            looping: true,
            sample_rate: SAMPLE_RATE,
            ..Default::default()
        };
        let frames = (length * SAMPLE_RATE).ceil().max(1.0) as usize;
        let time = |f: usize| (f as f32 / SAMPLE_RATE).min(length);
        for bn in bake.nodes.iter() {
            let Some(&node) = node_map.get(&bn.typed_id) else {
                continue;
            };
            let path = model.node_path(node);
            let rest = &model.nodes[node];
            let moves =
                |keys: usize, constant: bool, differs: bool| keys > 0 && (!constant || differs);
            let t0 = begin;
            let first_t = mirror(v3(ufbx::evaluate_baked_vec3(&bn.translation_keys, t0)));
            let first_r = mirror_rotation(quat(ufbx::evaluate_baked_quat(&bn.rotation_keys, t0)));
            let first_s = v3(ufbx::evaluate_baked_vec3(&bn.scale_keys, t0));
            let anim_t = moves(
                bn.translation_keys.count,
                bn.constant_translation,
                first_t.distance(rest.position) > 1e-5,
            );
            let anim_r = moves(
                bn.rotation_keys.count,
                bn.constant_rotation,
                first_r.normalize().dot(rest.rotation).abs() < 0.99999,
            );
            let anim_s = moves(
                bn.scale_keys.count,
                bn.constant_scale,
                first_s.distance(rest.scale) > 1e-5,
            );
            let mut prev: Option<Vec3> = None;
            for f in 0..=frames {
                let t = time(f);
                let at = begin + t as f64;
                if anim_t {
                    let v = mirror(v3(ufbx::evaluate_baked_vec3(&bn.translation_keys, at)));
                    for (i, x) in v.to_array().into_iter().enumerate() {
                        clip.set_key(&path, Property::Position(i as u8), t, x);
                    }
                }
                if anim_r {
                    let q = mirror_rotation(quat(ufbx::evaluate_baked_quat(&bn.rotation_keys, at)));
                    let e = continuous_euler(q.normalize(), prev);
                    prev = Some(e);
                    for (i, x) in e.to_array().into_iter().enumerate() {
                        clip.set_key(&path, Property::Rotation(i as u8), t, x);
                    }
                }
                if anim_s {
                    let v = v3(ufbx::evaluate_baked_vec3(&bn.scale_keys, at));
                    for (i, x) in v.to_array().into_iter().enumerate() {
                        clip.set_key(&path, Property::Scale(i as u8), t, x);
                    }
                }
            }
        }
        // Blend shape weights: the channels' DeformPercent (0–100).
        for be in bake.elements.iter() {
            let Some(&(node, shape)) = channels.get(&be.element_id) else {
                continue;
            };
            let path = model.node_path(node);
            for prop in be.props.iter() {
                if &*prop.name != "DeformPercent" || prop.keys.count == 0 {
                    continue;
                }
                for f in 0..=frames {
                    let t = time(f);
                    let w = ufbx::evaluate_baked_vec3(&prop.keys, begin + t as f64).x as f32;
                    clip.set_key(&path, Property::BlendShape(shape), t, w);
                }
            }
        }
        for c in &mut clip.curves {
            for i in 0..c.keys.len() {
                c.keys.set_tangent(i, TangentMode::Linear);
            }
        }
        simplify(&mut clip);
        clip.length = length;
        out.push(ModelAnimation {
            name: if stack.element.name.is_empty() {
                format!("Take{}", out.len())
            } else {
                stack.element.name.to_string()
            },
            clip,
        });
    }
    out
}
