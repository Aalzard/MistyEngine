//! glTF skins, morph targets and animations: bone influences and blend shapes of primitives,
//! skins with their inverse bind matrices, and animations resampled into engine clips.
//!
//! Animations are resampled at [`SAMPLE_RATE`] with linear keys: translations and scales map to
//! Position / Scale curves, rotations to continuous Euler curves (picking, per key, the Euler
//! triple closest to the previous one), morph target weights to blend shape curves (0–100).

use glam::{Mat4, Quat, Vec3};

use super::skin::{BlendShape, MeshSkin, ModelAnimation, ModelSkin};
use super::{Model, mirror_rotation};
use crate::animation::{AnimationClip, Property, TangentMode};
use crate::scene::Transform;

/// Keys per second of imported clips.
pub const SAMPLE_RATE: f32 = 30.0;

/// Morph target names per mesh (`mesh.extras.targetNames`, the common exporter convention).
pub fn target_names(bytes: &[u8]) -> Vec<Vec<String>> {
    let json: Option<serde_json::Value> = match gltf::Glb::from_slice(bytes) {
        Ok(glb) => serde_json::from_slice(&glb.json).ok(),
        Err(_) => serde_json::from_slice(bytes).ok(),
    };
    let Some(meshes) = json
        .as_ref()
        .and_then(|j| j.get("meshes")?.as_array().cloned())
    else {
        return Vec::new();
    };
    meshes
        .iter()
        .map(|m| {
            m.get("extras")
                .and_then(|e| e.get("targetNames"))
                .and_then(|n| n.as_array())
                .map(|names| {
                    names
                        .iter()
                        .map(|n| n.as_str().unwrap_or_default().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// JOINTS_0 / WEIGHTS_0 of a primitive with `n` vertices (weights normalised).
pub fn read_skin<'a, 's, F>(reader: &gltf::mesh::Reader<'a, 's, F>, n: usize) -> Option<MeshSkin>
where
    F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
{
    let joints: Vec<[u16; 4]> = reader.read_joints(0)?.into_u16().collect();
    let weights: Vec<[f32; 4]> = reader.read_weights(0)?.into_f32().collect();
    (joints.len() == n && weights.len() == n).then(|| MeshSkin::new(joints, weights))
}

/// Morph targets of a primitive as blend shapes (engine space: X mirrored).
pub fn read_shapes<'a, 's, F>(
    reader: &gltf::mesh::Reader<'a, 's, F>,
    n: usize,
    names: &[String],
) -> Vec<BlendShape>
where
    F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
{
    fn fill(it: Option<impl Iterator<Item = [f32; 3]>>, n: usize) -> Vec<[f32; 3]> {
        let mut out: Vec<[f32; 3]> = it
            .map(|i| i.map(|v| [-v[0], v[1], v[2]]).collect())
            .unwrap_or_default();
        out.resize(n, [0.0; 3]);
        out
    }
    reader
        .read_morph_targets()
        .enumerate()
        .map(|(i, (positions, normals, _))| BlendShape {
            name: names
                .get(i)
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("Shape{i}")),
            positions: fill(positions, n),
            normals: fill(normals, n),
        })
        .collect()
}

/// Right-handed matrix → engine space (mirror X on both sides).
pub(super) fn mirror_matrix(m: Mat4) -> Mat4 {
    let s = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    s * m * s
}

/// The file's skins, with joints as indices into `model.nodes` (`node_map`: glTF node → ours).
pub fn skins(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    node_map: &[Option<usize>],
) -> Vec<ModelSkin> {
    doc.skins()
        .map(|skin| {
            let joints: Vec<usize> = skin
                .joints()
                .map(|j| node_map.get(j.index()).copied().flatten().unwrap_or(0))
                .collect();
            let reader = skin.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let mut inverse_bind: Vec<Mat4> = reader
                .read_inverse_bind_matrices()
                .map(|it| {
                    it.map(|m| mirror_matrix(Mat4::from_cols_array_2d(&m)))
                        .collect()
                })
                .unwrap_or_default();
            inverse_bind.resize(joints.len(), Mat4::IDENTITY);
            let joint_nodes: Vec<gltf::Node> = skin.joints().collect();
            let root = skin
                .skeleton()
                .and_then(|n| node_map.get(n.index()).copied().flatten())
                .or_else(|| {
                    // The joint that is not a child of another joint.
                    joint_nodes
                        .iter()
                        .find(|j| {
                            !joint_nodes
                                .iter()
                                .any(|p| p.children().any(|c| c.index() == j.index()))
                        })
                        .and_then(|j| node_map.get(j.index()).copied().flatten())
                });
            ModelSkin {
                name: skin
                    .name()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Skin{}", skin.index())),
                joints,
                inverse_bind,
                root,
            }
        })
        .collect()
}

/// One channel's keyframes: times and values (`dim` floats per key; cubic splines hold
/// in-tangent, value, out-tangent per key).
struct Track {
    times: Vec<f32>,
    values: Vec<f32>,
    dim: usize,
    mode: gltf::animation::Interpolation,
}

impl Track {
    fn value(&self, k: usize) -> &[f32] {
        let stride = match self.mode {
            gltf::animation::Interpolation::CubicSpline => 3,
            _ => 1,
        };
        let start = (k * stride + usize::from(stride == 3)) * self.dim;
        self.values.get(start..start + self.dim).unwrap_or(&[])
    }

    fn tangent(&self, k: usize, out: bool) -> &[f32] {
        let start = (k * 3 + if out { 2 } else { 0 }) * self.dim;
        self.values.get(start..start + self.dim).unwrap_or(&[])
    }

    fn end(&self) -> f32 {
        self.times.last().copied().unwrap_or(0.0)
    }

    /// Value at `t` (rotations: `quat` = slerp between keys).
    fn sample(&self, t: f32, quat: bool) -> Vec<f32> {
        use gltf::animation::Interpolation as I;
        let n = self.times.len();
        if n == 0 {
            return vec![0.0; self.dim];
        }
        if t <= self.times[0] || n == 1 {
            return self.value(0).to_vec();
        }
        if t >= self.times[n - 1] {
            return self.value(n - 1).to_vec();
        }
        let k = self.times.partition_point(|x| *x <= t).saturating_sub(1);
        let (t0, t1) = (self.times[k], self.times[k + 1]);
        let dt = (t1 - t0).max(1e-6);
        let u = ((t - t0) / dt).clamp(0.0, 1.0);
        let (a, b) = (self.value(k), self.value(k + 1));
        match self.mode {
            I::Step => a.to_vec(),
            I::Linear if quat && self.dim == 4 => {
                let qa = Quat::from_slice(a);
                let qb = Quat::from_slice(b);
                qa.slerp(qb, u).to_array().to_vec()
            }
            I::Linear => a.iter().zip(b).map(|(x, y)| x + (y - x) * u).collect(),
            I::CubicSpline => {
                let (u2, u3) = (u * u, u * u * u);
                let (h00, h10) = (2.0 * u3 - 3.0 * u2 + 1.0, u3 - 2.0 * u2 + u);
                let (h01, h11) = (-2.0 * u3 + 3.0 * u2, u3 - u2);
                let (ob, ia) = (self.tangent(k, true), self.tangent(k + 1, false));
                (0..self.dim)
                    .map(|i| h00 * a[i] + h10 * dt * ob[i] + h01 * b[i] + h11 * dt * ia[i])
                    .collect()
            }
        }
    }
}

/// Euler angles of `q` closest to `prev` (both representations, unwrapped per axis).
pub fn continuous_euler(q: Quat, prev: Option<Vec3>) -> Vec3 {
    let mut t = Transform::default();
    t.set_rotation(q);
    let e = t.euler();
    let Some(prev) = prev else {
        return e;
    };
    let unwrap = |v: Vec3| v + ((prev - v) / 360.0).round() * 360.0;
    let alt = Vec3::new(180.0 - e.x, e.y + 180.0, e.z + 180.0);
    let same = |c: Vec3| {
        let mut t = Transform::default();
        t.set_euler(c);
        t.rotation.dot(q).abs() > 0.9999
    };
    [e, alt]
        .into_iter()
        .filter(|c| same(*c))
        .map(unwrap)
        .min_by(|a, b| (*a - prev).length().total_cmp(&(*b - prev).length()))
        .unwrap_or(e)
}

/// Drop keys that linear interpolation of their neighbours reproduces.
pub(super) fn simplify(clip: &mut AnimationClip) {
    for c in &mut clip.curves {
        let keys = c.keys.as_slice().to_vec();
        if keys.len() <= 2 {
            continue;
        }
        let mut kept = vec![keys[0]];
        for i in 1..keys.len() - 1 {
            let (a, k, b) = (kept[kept.len() - 1], keys[i], keys[i + 1]);
            let u = (k.time - a.time) / (b.time - a.time).max(1e-6);
            let lerp = a.value + (b.value - a.value) * u;
            if (lerp - k.value).abs() > 1e-4 * (1.0 + k.value.abs()) {
                kept.push(k);
            }
        }
        kept.push(keys[keys.len() - 1]);
        // A curve that never changes keeps a single key.
        if kept.len() == 2 && (kept[0].value - kept[1].value).abs() < 1e-5 {
            kept.pop();
        }
        c.keys = crate::animation::Keys::new(kept);
    }
}

/// The file's animations as clips on the model root (curve paths from [`Model::node_path`]).
pub fn animations(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    node_map: &[Option<usize>],
    model: &Model,
) -> Vec<ModelAnimation> {
    use gltf::animation::Property as P;
    use gltf::animation::util::ReadOutputs;
    doc.animations()
        .map(|anim| {
            // (our node, property, track)
            let mut tracks: Vec<(usize, P, Track)> = Vec::new();
            for ch in anim.channels() {
                let Some(node) = node_map.get(ch.target().node().index()).copied().flatten() else {
                    continue;
                };
                let reader = ch.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
                let Some(times) = reader.read_inputs().map(|i| i.collect::<Vec<f32>>()) else {
                    continue;
                };
                let (values, dim): (Vec<f32>, usize) = match reader.read_outputs() {
                    Some(ReadOutputs::Translations(it)) => (it.flatten().collect(), 3),
                    Some(ReadOutputs::Scales(it)) => (it.flatten().collect(), 3),
                    Some(ReadOutputs::Rotations(r)) => (r.into_f32().flatten().collect(), 4),
                    Some(ReadOutputs::MorphTargetWeights(w)) => {
                        let v: Vec<f32> = w.into_f32().collect();
                        let per_key = match ch.sampler().interpolation() {
                            gltf::animation::Interpolation::CubicSpline => times.len() * 3,
                            _ => times.len(),
                        };
                        let dim = v.len() / per_key.max(1);
                        (v, dim)
                    }
                    None => continue,
                };
                let per_key = match ch.sampler().interpolation() {
                    gltf::animation::Interpolation::CubicSpline => 3 * dim,
                    _ => dim,
                };
                // Malformed files: fewer values than keys, or key times that are not numbers.
                if dim == 0
                    || times.is_empty()
                    || values.len() < times.len() * per_key
                    || times.iter().any(|t| !t.is_finite())
                {
                    continue;
                }
                let track = Track {
                    times,
                    values,
                    dim,
                    mode: ch.sampler().interpolation(),
                };
                tracks.push((node, ch.target().property(), track));
            }
            let length = tracks.iter().map(|t| t.2.end()).fold(0.0, f32::max);
            let mut clip = AnimationClip {
                length,
                looping: true,
                sample_rate: SAMPLE_RATE,
                ..Default::default()
            };
            let frames = (length * SAMPLE_RATE).ceil().max(1.0) as usize;
            for (node, prop, track) in &tracks {
                let path = model.node_path(*node);
                let mut prev: Option<Vec3> = None;
                for f in 0..=frames {
                    let t = (f as f32 / SAMPLE_RATE).min(length);
                    let v = track.sample(t, *prop == P::Rotation);
                    match prop {
                        P::Translation => {
                            for (i, x) in [-v[0], v[1], v[2]].into_iter().enumerate() {
                                clip.set_key(&path, Property::Position(i as u8), t, x);
                            }
                        }
                        P::Scale => {
                            for (i, x) in v.iter().take(3).enumerate() {
                                clip.set_key(&path, Property::Scale(i as u8), t, *x);
                            }
                        }
                        P::Rotation => {
                            let q = mirror_rotation(Quat::from_slice(&v)).normalize();
                            let e = continuous_euler(q, prev);
                            prev = Some(e);
                            for (i, x) in e.to_array().into_iter().enumerate() {
                                clip.set_key(&path, Property::Rotation(i as u8), t, x);
                            }
                        }
                        P::MorphTargetWeights => {
                            for (i, w) in v.iter().enumerate() {
                                clip.set_key(&path, Property::BlendShape(i as u8), t, w * 100.0);
                            }
                        }
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
            ModelAnimation {
                name: anim
                    .name()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Animation{}", anim.index())),
                clip,
            }
        })
        .collect()
}
