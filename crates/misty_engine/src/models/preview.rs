//! A model posed by one of its clips, for the import Inspector's clip preview: node matrices at
//! a time, the skeleton's lines and a sample of the (skinned) vertices.

use glam::{Mat4, Vec3};

use super::Model;
use crate::animation::{AnimationClip, Property};
use crate::scene::Transform;

/// Model-space matrix of every node with `clip` at time `t` (rest pose without a clip).
pub fn pose(model: &Model, clip: Option<&AnimationClip>, t: f32) -> Vec<Mat4> {
    let mut out: Vec<Mat4> = Vec::with_capacity(model.nodes.len());
    for (i, n) in model.nodes.iter().enumerate() {
        let (mut pos, mut rot, mut scale) = (n.position, n.rotation, n.scale);
        if let Some(clip) = clip {
            let path = model.node_path(i);
            let get = |p: Property, rest: f32| clip.curve(&path, p).map_or(rest, |c| c.evaluate(t));
            let animated =
                |f: fn(u8) -> Property| (0..3u8).any(|k| clip.curve(&path, f(k)).is_some());
            if animated(Property::Rotation) {
                let mut tr = Transform::default();
                tr.set_rotation(rot);
                let e = tr.euler();
                tr.set_euler(Vec3::new(
                    get(Property::Rotation(0), e.x),
                    get(Property::Rotation(1), e.y),
                    get(Property::Rotation(2), e.z),
                ));
                rot = tr.rotation;
            }
            pos = Vec3::new(
                get(Property::Position(0), pos.x),
                get(Property::Position(1), pos.y),
                get(Property::Position(2), pos.z),
            );
            scale = Vec3::new(
                get(Property::Scale(0), scale.x),
                get(Property::Scale(1), scale.y),
                get(Property::Scale(2), scale.z),
            );
        }
        let local = Mat4::from_scale_rotation_translation(scale, rot, pos);
        // Parents come before their children.
        let m = match n.parent.and_then(|p| out.get(p)) {
            Some(parent) => *parent * local,
            None => local,
        };
        out.push(m);
    }
    out
}

/// What the preview draws: bone lines (parent → child joint) and up to `max_points` vertices,
/// skinned meshes deformed by their bones.
pub struct PreviewShape {
    pub bones: Vec<(Vec3, Vec3)>,
    pub points: Vec<Vec3>,
}

pub fn preview_shape(
    model: &Model,
    clip: Option<&AnimationClip>,
    t: f32,
    max_points: usize,
) -> PreviewShape {
    let matrices = pose(model, clip, t);
    let joints: std::collections::HashSet<usize> = model
        .skins
        .iter()
        .flat_map(|s| s.joints.iter().copied())
        .collect();
    let bones = model
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| joints.contains(i))
        .filter_map(|(i, n)| {
            let p = n.parent.filter(|p| joints.contains(p))?;
            Some((
                matrices[p].transform_point3(Vec3::ZERO),
                matrices[i].transform_point3(Vec3::ZERO),
            ))
        })
        .collect();
    let total: usize = model
        .nodes
        .iter()
        .flat_map(|n| n.meshes.iter())
        .map(|&m| model.meshes[m].data.vertices.len())
        .sum();
    let step = total.div_ceil(max_points.max(1)).max(1);
    let mut points = Vec::new();
    for (i, n) in model.nodes.iter().enumerate() {
        let skin = n.skin.and_then(|s| model.skins.get(s));
        for &m in &n.meshes {
            let mesh = &model.meshes[m];
            for (v, vert) in mesh.data.vertices.iter().enumerate().step_by(step) {
                let p = Vec3::from(vert.position);
                let world = match (skin, &mesh.skin) {
                    (Some(skin), Some(ms)) if !ms.bind_poses.is_empty() => {
                        let mut out = Vec3::ZERO;
                        for k in 0..4 {
                            let w = ms.weights[v][k];
                            let j = ms.joints[v][k] as usize;
                            if w > 0.0
                                && let (Some(&node), Some(bind)) =
                                    (skin.joints.get(j), ms.bind_poses.get(j))
                            {
                                out += w * (matrices[node] * *bind).transform_point3(p);
                            }
                        }
                        out
                    }
                    _ => matrices[i].transform_point3(p),
                };
                points.push(world);
            }
        }
    }
    PreviewShape { bones, points }
}
