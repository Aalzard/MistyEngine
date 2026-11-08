//! Import settings applied to a freshly read model ([`super::ModelImporter`]), in order: axis
//! conversion, Scale Factor, normals and tangents, animation clip splitting, Generate LODs,
//! lightmap UVs, Virtual Geometry.

use std::collections::HashMap;

use glam::{Quat, Vec3};

use super::settings::{AxisConversion, ClipSettings, LodSetting, ModelImporter, VectorSource};
use super::{Model, ModelAnimation, ModelMesh, gltf_anim, virtual_build};
use crate::animation::{AnimationClip, Curve, Keyframe, Keys, Property, TangentMode};
use crate::render::mesh::MeshData;
use crate::scene::Transform;

/// Apply import settings to a freshly read model (`rel` = its project path).
pub fn apply(mut model: Model, settings: &ModelImporter, rel: &str) -> Model {
    // FBX files with Automatic axes were converted while reading (from their scene settings).
    if settings.axis_conversion == AxisConversion::ZUp {
        z_up_to_y_up(&mut model);
    }
    if settings.scale_factor != 1.0 {
        scale(&mut model, settings.scale_factor);
    }
    vectors(&mut model, settings);
    if !settings.clips.is_empty() {
        split_clips(&mut model, &settings.clips);
    }
    model.read_write = settings.read_write;
    model.material_folder = settings.material_folder.trim_end_matches('/').to_owned();
    if settings.generate_lods {
        generate_lods(&mut model, &settings.lods);
    }
    // Skinned meshes deform: no clusters for them.
    model.virtual_geometry = settings.virtual_geometry && model.skins.is_empty();
    if settings.generate_lightmap_uvs {
        for m in &mut model.meshes {
            // Skins and blend shapes index the vertices: those meshes keep their own uv2.
            if m.skin.is_none() && m.blend_shapes.is_empty() {
                m.data = crate::lightmap::uv::unwrap(&m.data, crate::lightmap::uv::MARGIN);
                m.refresh();
            }
        }
    }
    if model.virtual_geometry {
        model.virtual_builds = virtual_build::start(&model, rel);
    }
    model
}

// ---- axes and scale ------------------------------------------------------------------------

/// Turn a Z-up model Y-up: its root nodes (and their animation) rotate -90° about X.
pub fn z_up_to_y_up(model: &mut Model) {
    let r = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    for i in 0..model.nodes.len() {
        if model.nodes[i].parent.is_some() {
            continue;
        }
        let path = model.node_path(i);
        let n = &model.nodes[i];
        let (pos, rot) = (n.position, n.rotation);
        for a in &mut model.animations {
            transform_curves(&mut a.clip, &path, pos, rot, |p, q| (r * p, r * q));
        }
        let n = &mut model.nodes[i];
        n.position = r * n.position;
        n.rotation = (r * n.rotation).normalize();
    }
}

fn euler_of(q: Quat) -> Vec3 {
    let mut t = Transform::default();
    t.set_rotation(q);
    t.euler()
}

fn quat_of(euler: Vec3) -> Quat {
    let mut t = Transform::default();
    t.set_euler(euler);
    t.rotation
}

/// Rewrite the Position and Rotation curves of `path` through `f` (key times kept; values
/// missing from the clip are the rest pose `pos` / `rot`).
fn transform_curves(
    clip: &mut AnimationClip,
    path: &str,
    pos: Vec3,
    rot: Quat,
    f: impl Fn(Vec3, Quat) -> (Vec3, Quat),
) {
    let is = |c: &Curve, rotation: bool| {
        c.path == path
            && if rotation {
                matches!(c.property, Property::Rotation(_))
            } else {
                matches!(c.property, Property::Position(_))
            }
    };
    let has_pos = clip.curves.iter().any(|c| is(c, false));
    let has_rot = clip.curves.iter().any(|c| is(c, true));
    if !has_pos && !has_rot {
        return;
    }
    let mut times: Vec<f32> = clip
        .curves
        .iter()
        .filter(|c| is(c, false) || is(c, true))
        .flat_map(|c| c.keys.as_slice().iter().map(|k| k.time))
        .collect();
    times.sort_by(f32::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < crate::animation::curve::TIME_EPSILON);
    let rest_euler = euler_of(rot);
    let value = |clip: &AnimationClip, p: Property, t: f32, rest: f32| {
        clip.curve(path, p).map_or(rest, |c| c.evaluate(t))
    };
    let mut new_pos = Vec::new();
    let mut new_rot = Vec::new();
    let mut prev: Option<Vec3> = None;
    for &t in &times {
        let p = Vec3::from_array(std::array::from_fn(|i| {
            value(clip, Property::Position(i as u8), t, pos[i])
        }));
        let e = Vec3::from_array(std::array::from_fn(|i| {
            value(clip, Property::Rotation(i as u8), t, rest_euler[i])
        }));
        let (p2, q2) = f(p, quat_of(e));
        new_pos.push((t, p2));
        let e2 = gltf_anim::continuous_euler(q2.normalize(), prev);
        prev = Some(e2);
        new_rot.push((t, e2));
    }
    clip.curves
        .retain(|c| !((has_pos && is(c, false)) || (has_rot && is(c, true))));
    let mut add = |prop: Property, keys: Vec<Keyframe>| {
        clip.curves.push(Curve {
            path: path.to_owned(),
            property: prop,
            keys: Keys::new(keys),
        });
    };
    let linear = |t: f32, v: f32| Keyframe {
        time: t,
        value: v,
        tangent: TangentMode::Linear,
    };
    for i in 0..3 {
        if has_pos {
            add(
                Property::Position(i as u8),
                new_pos.iter().map(|(t, p)| linear(*t, p[i])).collect(),
            );
        }
        if has_rot {
            add(
                Property::Rotation(i as u8),
                new_rot.iter().map(|(t, e)| linear(*t, e[i])).collect(),
            );
        }
    }
}

/// Scale Factor: multiply every length of the model (meshes, blend shapes, node positions,
/// bind poses, Position curves) by `k`.
pub fn scale(model: &mut Model, k: f32) {
    let scale_bind = |m: glam::Mat4| {
        let mut m = m;
        m.w_axis.x *= k;
        m.w_axis.y *= k;
        m.w_axis.z *= k;
        m
    };
    for m in &mut model.meshes {
        for v in &mut m.data.vertices {
            v.position = (Vec3::from(v.position) * k).into();
        }
        for s in &mut m.blend_shapes {
            for p in &mut s.positions {
                *p = (Vec3::from(*p) * k).into();
            }
        }
        if let Some(skin) = &mut m.skin {
            for b in &mut skin.bind_poses {
                *b = scale_bind(*b);
            }
            for (lo, hi) in skin.joint_bounds.iter_mut().flatten() {
                *lo *= k;
                *hi *= k;
            }
        }
        m.refresh();
    }
    for s in &mut model.skins {
        for b in &mut s.inverse_bind {
            *b = scale_bind(*b);
        }
    }
    for n in &mut model.nodes {
        n.position *= k;
    }
    for a in &mut model.animations {
        for c in &mut a.clip.curves {
            if matches!(c.property, Property::Position(_)) {
                let keys: Vec<Keyframe> = c
                    .keys
                    .as_slice()
                    .iter()
                    .map(|key| Keyframe {
                        value: key.value * k,
                        ..*key
                    })
                    .collect();
                c.keys = Keys::new(keys);
            }
        }
    }
}

// ---- normals and tangents ------------------------------------------------------------------

/// Normals and Tangents settings.
fn vectors(model: &mut Model, settings: &ModelImporter) {
    for m in &mut model.meshes {
        let mut normals_changed = false;
        match settings.normals {
            VectorSource::Import => {}
            VectorSource::Calculate => {
                let (data, remap) = smooth_normals(&m.data, settings.smoothing_angle);
                remap_mesh(m, data, &remap);
                normals_changed = true;
            }
            VectorSource::None => {
                for v in &mut m.data.vertices {
                    v.normal = [0.0, 1.0, 0.0];
                }
                normals_changed = true;
            }
        }
        match settings.tangents {
            VectorSource::Calculate => {
                if normals_changed || m.file_tangents {
                    m.data.compute_tangents();
                    m.file_tangents = false;
                }
            }
            VectorSource::Import if m.file_tangents => {
                // The file's directions, with the side of the bitangent the engine expects
                // (files differ in their UV and handedness conventions).
                let mut computed = m.data.clone();
                computed.compute_tangents();
                for (v, c) in m.data.vertices.iter_mut().zip(&computed.vertices) {
                    let n = Vec3::from(v.normal);
                    let t = Vec3::from_slice(&v.tangent[..3]);
                    let t = (t - n * n.dot(t)).try_normalize();
                    match t {
                        Some(t) => v.tangent = [t.x, t.y, t.z, c.tangent[3]],
                        None => v.tangent = c.tangent,
                    }
                }
            }
            VectorSource::Import => {
                if normals_changed {
                    m.data.compute_tangents();
                }
            }
            VectorSource::None => {
                for v in &mut m.data.vertices {
                    v.tangent = [1.0, 0.0, 0.0, 1.0];
                }
            }
        }
    }
}

/// Replace a mesh's vertices with `data`, whose vertex `i` copies the old vertex `remap[i]`
/// (skin influences and blend shape offsets follow).
fn remap_mesh(m: &mut ModelMesh, data: MeshData, remap: &[u32]) {
    if let Some(skin) = &mut m.skin {
        skin.joints = remap.iter().map(|&i| skin.joints[i as usize]).collect();
        skin.weights = remap.iter().map(|&i| skin.weights[i as usize]).collect();
    }
    for s in &mut m.blend_shapes {
        s.positions = remap.iter().map(|&i| s.positions[i as usize]).collect();
        s.normals = remap.iter().map(|&i| s.normals[i as usize]).collect();
    }
    m.data = data;
    m.file_normals = false;
    m.refresh();
}

/// Normals calculated from the faces: faces meeting at a vertex at less than `angle_deg` are
/// smoothed together, sharper edges split the vertex. Returns the mesh and, per new vertex,
/// the old vertex it copies.
pub fn smooth_normals(data: &MeshData, angle_deg: f32) -> (MeshData, Vec<u32>) {
    let positions: Vec<Vec3> = data.vertices.iter().map(|v| v.position.into()).collect();
    let weld = crate::vgeo::weld(&positions);
    let tris = data.indices.as_chunks::<3>().0;
    let face: Vec<Vec3> = tris
        .iter()
        .map(|t| {
            let p = |i: u32| positions[i as usize];
            (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]))
        })
        .collect();
    let unit: Vec<Vec3> = face.iter().map(|f| f.normalize_or_zero()).collect();
    let mut incident: HashMap<u32, Vec<u32>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for &v in t {
            let list = incident.entry(weld[v as usize]).or_default();
            if !list.contains(&(i as u32)) {
                list.push(i as u32);
            }
        }
    }
    let limit = if angle_deg >= 180.0 {
        -1.0
    } else {
        angle_deg.to_radians().cos() - 1e-5
    };
    let mut out = MeshData::default();
    let mut remap = Vec::new();
    let mut seen: HashMap<(u32, [i32; 3]), u32> = HashMap::new();
    // Angle of triangle `o` at its corner on welded position `w` (weights the face normals,
    // so the result does not depend on how polygons were triangulated).
    let corner_angle = |o: usize, w: u32| {
        let t = tris[o];
        let k = (0..3).find(|&k| weld[t[k] as usize] == w).unwrap_or(0);
        let p = |k: usize| positions[t[k % 3] as usize];
        let (a, b) = (p(k + 1) - p(k), p(k + 2) - p(k));
        a.angle_between(b)
    };
    for (i, t) in tris.iter().enumerate() {
        for &v in t {
            let w = weld[v as usize];
            let mut n = Vec3::ZERO;
            for &o in &incident[&w] {
                if unit[i].dot(unit[o as usize]) >= limit {
                    let angle = corner_angle(o as usize, w);
                    if angle.is_finite() {
                        n += unit[o as usize] * angle;
                    }
                }
            }
            let n = n.try_normalize().unwrap_or(unit[i]);
            let key = (v, (n * 1e4).round().as_ivec3().to_array());
            let index = *seen.entry(key).or_insert_with(|| {
                let mut vert = data.vertices[v as usize];
                vert.normal = n.into();
                out.vertices.push(vert);
                remap.push(v);
                out.vertices.len() as u32 - 1
            });
            out.indices.push(index);
        }
    }
    (out, remap)
}

// ---- clips ---------------------------------------------------------------------------------

/// Clip splitting: each clip is a frame range of a take (frames at [`gltf_anim::SAMPLE_RATE`]).
pub fn split_clips(model: &mut Model, clips: &[ClipSettings]) {
    let sources = std::mem::take(&mut model.animations);
    let eps = crate::animation::curve::TIME_EPSILON;
    for c in clips {
        let Some(src) = sources
            .iter()
            .find(|a| a.name == c.take)
            .or(sources.first())
        else {
            continue;
        };
        let t0 = c.first_frame / gltf_anim::SAMPLE_RATE;
        let t1 = (c.last_frame / gltf_anim::SAMPLE_RATE).max(t0);
        let length = t1 - t0;
        let mut clip = AnimationClip {
            length,
            looping: c.looping,
            sample_rate: gltf_anim::SAMPLE_RATE,
            curves: Vec::new(),
            events: Vec::new(),
        };
        let linear = |t: f32, v: f32| Keyframe {
            time: t,
            value: v,
            tangent: TangentMode::Linear,
        };
        for curve in &src.clip.curves {
            let mut keys = vec![linear(0.0, curve.evaluate(t0))];
            for k in curve.keys.as_slice() {
                if k.time > t0 + eps && k.time < t1 - eps {
                    keys.push(Keyframe {
                        time: k.time - t0,
                        ..*k
                    });
                }
            }
            if length > eps {
                keys.push(linear(length, curve.evaluate(t1)));
            }
            clip.curves.push(Curve {
                path: curve.path.clone(),
                property: curve.property,
                keys: Keys::new(keys),
            });
        }
        gltf_anim::simplify(&mut clip);
        clip.length = length;
        model.animations.push(ModelAnimation {
            name: c.name.clone(),
            clip,
        });
    }
}

// ---- LODs ----------------------------------------------------------------------------------

/// Generate LODs: per level after LOD 0, a simplified copy of every static mesh (the vgeo
/// edge-collapse simplifier, borders and UV seams kept) and the levels' screen heights.
pub fn generate_lods(model: &mut Model, levels: &[LodSetting]) {
    if levels.len() < 2 {
        return;
    }
    for n in 0..model.nodes.len() {
        let meshes = model.nodes[n].meshes.clone();
        let static_node = model.nodes[n].skin.is_none()
            && meshes.iter().all(|&m| {
                model.meshes[m].skin.is_none() && model.meshes[m].blend_shapes.is_empty()
            });
        if meshes.is_empty() || !static_node {
            continue;
        }
        let mut lods = Vec::new();
        for (level, setting) in levels.iter().enumerate().skip(1) {
            let mut ids = Vec::new();
            for &m in &meshes {
                let src = &model.meshes[m];
                let data = crate::vgeo::rock::simplified(&src.data, setting.quality);
                let mut lod = ModelMesh::with_vectors(
                    format!("{} LOD{level}", src.name),
                    data,
                    src.material,
                    true,
                );
                lod.file_tangents = src.file_tangents;
                lod.file_normals = src.file_normals;
                ids.push(model.meshes.len());
                model.meshes.push(lod);
            }
            lods.push(ids);
        }
        model.nodes[n].lods = lods;
    }
    model.lod_heights = levels.iter().map(|l| l.screen_height).collect();
}
