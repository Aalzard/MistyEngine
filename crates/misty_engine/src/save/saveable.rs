//! The Saveable component: marks an object whose state goes into save games, under a stable
//! Save ID (a GUID) that survives renames, reparenting and scene reloads.

use std::collections::HashSet;

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::asset_db::Guid;
use crate::scene::Scene;

/// What the Save System stores for one object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saveable {
    /// Stable id (32 hex digits); new objects get a fresh one, duplicates are given new ones.
    pub save_id: String,
    /// Local position, rotation and scale.
    pub save_transform: bool,
    /// The GameObject's active checkbox.
    pub save_active: bool,
    /// Rigidbody linear and angular velocity (when the object has one).
    pub save_velocity: bool,
    /// Extra component data: `"Light"` stores the whole component, `"Light.intensity"` one field.
    pub components: Vec<String>,
}

impl Default for Saveable {
    fn default() -> Self {
        Self {
            save_id: new_id(),
            save_transform: true,
            save_active: true,
            save_velocity: true,
            components: Vec::new(),
        }
    }
}

/// A fresh Save ID.
pub fn new_id() -> String {
    Guid::new().to_string()
}

/// Give every Saveable in `root`'s subtree a fresh Save ID (copies, prefab instances).
pub fn fresh_ids_in(scene: &mut Scene, root: Entity) {
    let subtree: Vec<Entity> = scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.is_ancestor(root, *e))
        .collect();
    for e in subtree {
        if let Ok(mut s) = scene.world.get::<&mut Saveable>(e) {
            s.save_id = new_id();
        }
    }
}

/// Empty or repeated Save IDs get fresh ones; the first object (in Hierarchy order) keeps its id.
/// Returns how many were changed.
pub fn fix_duplicate_ids(scene: &mut Scene) -> usize {
    // Cheap common case: no Saveables at all.
    if scene.world.query::<&Saveable>().iter().next().is_none() {
        return 0;
    }
    let mut seen = HashSet::new();
    let mut changed = 0;
    for (e, _) in scene.walk() {
        let Ok(mut s) = scene.world.get::<&mut Saveable>(e) else {
            continue;
        };
        if s.save_id.trim().is_empty() || !seen.insert(s.save_id.clone()) {
            s.save_id = new_id();
            seen.insert(s.save_id.clone());
            changed += 1;
        }
    }
    changed
}

/// Save ID → object, for every Saveable in the scene.
pub fn by_id(scene: &Scene) -> std::collections::HashMap<String, Entity> {
    scene
        .world
        .query::<(Entity, &Saveable)>()
        .iter()
        .map(|(e, s)| (s.save_id.clone(), e))
        .collect()
}

impl crate::scene::SceneComponent for Saveable {
    const NAME: &'static str = "Saveable";
    const CATEGORY: &'static str = "Miscellaneous";
}
