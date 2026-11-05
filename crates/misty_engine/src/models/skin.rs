//! Skinning data of imported models: per-vertex bone influences, blend shapes (morph targets),
//! skins (joint nodes + inverse bind matrices) and animation clips read from the file.
//!
//! Everything is already in engine space (X mirrored like the rest of the import).

use glam::{Mat4, Vec3};

use crate::animation::AnimationClip;

/// Bone influences of a mesh: up to four joints per vertex (indices into the skin's joint list)
/// with weights that sum to 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshSkin {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    /// Inverse bind matrix per joint (mesh space → the joint's space at bind time).
    pub bind_poses: Vec<Mat4>,
    /// Per joint: bounds of the vertices it moves, in the joint's bind space (posed bounds).
    pub joint_bounds: Vec<Option<(Vec3, Vec3)>>,
}

impl MeshSkin {
    /// Influences with weights normalised to sum to 1 (vertices without any weight are bound
    /// fully to their first joint).
    pub fn new(joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]>) -> Self {
        let weights = weights
            .into_iter()
            .map(|w| {
                let w = w.map(|x| if x.is_finite() { x.max(0.0) } else { 0.0 });
                let sum: f32 = w.iter().sum();
                if sum > 1e-8 {
                    w.map(|x| x / sum)
                } else {
                    [1.0, 0.0, 0.0, 0.0]
                }
            })
            .collect();
        Self {
            joints,
            weights,
            bind_poses: Vec::new(),
            joint_bounds: Vec::new(),
        }
    }

    /// Highest joint index used (+1): the bone count the mesh needs.
    pub fn joint_count(&self) -> usize {
        self.joints
            .iter()
            .zip(&self.weights)
            .flat_map(|(j, w)| j.iter().zip(w).filter(|(_, w)| **w > 0.0).map(|(j, _)| *j))
            .max()
            .map_or(0, |j| j as usize + 1)
    }
}

/// A blend shape: per-vertex position and normal offsets, applied as `weight / 100`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendShape {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}

impl BlendShape {
    /// Largest position offset (bounds padding).
    pub fn max_offset(&self) -> f32 {
        self.positions
            .iter()
            .map(|p| Vec3::from(*p).length())
            .fold(0.0, f32::max)
    }
}

/// A glTF skin: the nodes acting as bones and the matrix taking each from mesh space into the
/// bone's space at bind time.
#[derive(Clone, Debug, Default)]
pub struct ModelSkin {
    pub name: String,
    /// Indices into [`super::Model::nodes`], in joint order.
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
    /// Common root of the joints (the Skinned Mesh Renderer's Root Bone).
    pub root: Option<usize>,
}

/// An animation of the file, as a clip whose curve paths start below the model's root object.
#[derive(Clone, Debug)]
pub struct ModelAnimation {
    pub name: String,
    pub clip: AnimationClip,
}

impl MeshSkin {
    /// Set the bind poses and work out each joint's bounds from the mesh positions (`pad` =
    /// extra room for blend shape offsets).
    pub fn bind(&mut self, inverse_bind: &[Mat4], positions: &[[f32; 3]], pad: f32) {
        let count = inverse_bind.len().max(self.joint_count());
        self.bind_poses = (0..count)
            .map(|j| inverse_bind.get(j).copied().unwrap_or(Mat4::IDENTITY))
            .collect();
        let mut bounds: Vec<Option<(Vec3, Vec3)>> = vec![None; count];
        for ((p, j), w) in positions.iter().zip(&self.joints).zip(&self.weights) {
            for (joint, weight) in j.iter().zip(w) {
                let k = *joint as usize;
                if *weight <= 0.0 || k >= count {
                    continue;
                }
                let local = self.bind_poses[k].transform_point3(Vec3::from(*p));
                let b = bounds[k].get_or_insert((local, local));
                b.0 = b.0.min(local);
                b.1 = b.1.max(local);
            }
        }
        self.joint_bounds = bounds
            .into_iter()
            .map(|b| b.map(|(lo, hi)| (lo - Vec3::splat(pad), hi + Vec3::splat(pad))))
            .collect();
    }
}
