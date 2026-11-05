//! Skinned Mesh Renderer: draws a mesh deformed by a list of bone objects and blend shape
//! weights. Bones are ordinary scene objects (usually the imported model's node hierarchy), so
//! anything that moves them (animation clips, scripts, constraints) bends the mesh.

use glam::Vec3;
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::MeshRef;
use super::components::entity_bits;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinnedMeshRenderer {
    pub enabled: bool,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// The skinned mesh ("Assets/…/Model.glb#0").
    pub mesh: Option<MeshRef>,
    /// Materials list (element 0 first); extra elements draw the mesh again on top.
    pub materials: Vec<Option<String>>,
    /// Bone the bounds are relative to (the skeleton's top joint).
    #[serde(with = "entity_bits::option")]
    pub root_bone: Option<Entity>,
    /// One object per joint of the mesh's skin (`None` = missing: the joint keeps its bind pose).
    #[serde(with = "bone_list")]
    pub bones: Vec<Option<Entity>>,
    /// Bounds in the root bone's space (used for culling unless Update When Offscreen is on).
    pub bounds_center: Vec3,
    pub bounds_extents: Vec3,
    /// Compute the bounds from the posed bones every frame instead of using the fixed box.
    pub update_when_offscreen: bool,
    /// Blend shape weights (0–100) by shape index.
    pub blend_shape_weights: Vec<f32>,
}

impl Default for SkinnedMeshRenderer {
    fn default() -> Self {
        Self {
            enabled: true,
            cast_shadows: true,
            receive_shadows: true,
            mesh: None,
            materials: vec![None],
            root_bone: None,
            bones: Vec::new(),
            bounds_center: Vec3::ZERO,
            bounds_extents: Vec3::splat(0.5),
            update_when_offscreen: false,
            blend_shape_weights: Vec::new(),
        }
    }
}

impl SkinnedMeshRenderer {
    /// Blend shapes of the mesh (the weight list's length when the mesh is missing).
    pub fn blend_shape_count(&self) -> usize {
        self.mesh
            .as_ref()
            .and_then(crate::skinning::blend_shape_names)
            .map_or(self.blend_shape_weights.len(), |n| n.len())
    }

    /// `GetBlendShapeWeight`: 0 for shapes never set.
    pub fn blend_shape_weight(&self, i: usize) -> f32 {
        self.blend_shape_weights.get(i).copied().unwrap_or(0.0)
    }

    /// `SetBlendShapeWeight` (non-finite values are ignored).
    pub fn set_blend_shape_weight(&mut self, i: usize, value: f32) {
        if !value.is_finite() || i >= self.blend_shape_count().max(self.blend_shape_weights.len()) {
            return;
        }
        if self.blend_shape_weights.len() <= i {
            self.blend_shape_weights.resize(i + 1, 0.0);
        }
        self.blend_shape_weights[i] = value;
    }

    /// Material path of element `i` (`None` = Default-Material).
    pub fn material_at(&self, i: usize) -> Option<&str> {
        self.materials.get(i).and_then(|m| m.as_deref())
    }
}

/// Bone lists stored as entity id bits, `null` for missing bones.
mod bone_list {
    use hecs::Entity;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[Option<Entity>], s: S) -> Result<S::Ok, S::Error> {
        v.iter()
            .map(|e| e.map(|e| e.to_bits().get()))
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Option<Entity>>, D::Error> {
        Ok(Vec::<Option<u64>>::deserialize(d)?
            .into_iter()
            .map(|b| b.and_then(Entity::from_bits))
            .collect())
    }
}
