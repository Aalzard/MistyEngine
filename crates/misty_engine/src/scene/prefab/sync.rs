//! Make an instance match its asset in place, keeping chosen overrides.
//!
//! Objects keep their entities (so the selection and references from other objects survive),
//! components keep the fields the instance overrides and take the asset's value for the rest,
//! objects the asset gained are created, objects it lost are deleted.

use std::collections::{HashMap, HashSet};

use hecs::Entity;
use serde_json::Value;

use super::super::file::remap_refs;
use super::super::{Node, PrefabSource, Scene, SceneComponent, registry};
use super::diff::{self, Override, kept_field};
use super::ids::{self, LINK, SOURCE};
use super::{OverrideItem, Prefab};

/// What an instance keeps when it is made to match its asset.
#[derive(Default)]
pub(crate) struct Keep {
    pub keys: HashSet<Override>,
    /// Added objects (tops of added subtrees) that stay.
    pub added: HashSet<Entity>,
}

impl Keep {
    /// Keep every override in `items` (an instance's overrides against the asset it had).
    pub fn from_items(items: &[OverrideItem]) -> Self {
        let mut keep = Self::default();
        for o in items {
            if let (Override::AddedObject { .. }, Some(e)) = (&o.key, o.entity) {
                keep.added.insert(e);
            }
            keep.keys.insert(o.key.clone());
        }
        keep
    }

    /// Keep what a saved `record` lists. `items` are the instance's overrides against the new
    /// asset: their added objects stay when the record lists them.
    pub fn from_record(record: &[Override], items: &[OverrideItem]) -> Self {
        let mut keep = Self {
            keys: record.iter().cloned().collect(),
            added: HashSet::new(),
        };
        for o in items {
            if let (Override::AddedObject { .. }, Some(e)) = (&o.key, o.entity)
                && keep.keys.contains(&o.key)
            {
                keep.added.insert(e);
            }
        }
        keep
    }
}

impl Scene {
    /// Make the instance at `root` match `prefab` except for `keep`. Returns whether anything
    /// changed.
    pub(crate) fn sync_prefab_instance(
        &mut self,
        root: Entity,
        prefab: &Prefab,
        keep: &Keep,
    ) -> bool {
        let (entries, ents) = self.instance_entries(root);
        let (inst_ids, inst_scope) = ids::scoped_level_ids(&entries);
        let mut map: HashMap<u64, Entity> = inst_ids
            .iter()
            .zip(&ents)
            .filter_map(|(id, e)| id.map(|id| (id, *e)))
            .collect();
        let base = &prefab.objects;
        let base_ids = ids::level_ids(base);
        let mut changed = false;

        // Asset objects the instance should have: not removed by a kept override, parent kept.
        let mut kept = vec![false; base.len()];
        for (j, entry) in base.iter().enumerate() {
            let Some(id) = base_ids[j] else {
                continue;
            };
            let parent_ok = j == 0 || entry.parent.is_some_and(|p| kept[p]);
            let removed = !map.contains_key(&id)
                && keep.keys.contains(&Override::RemovedObject { object: id });
            kept[j] = parent_ok && !removed;
        }
        let kept_ids: HashSet<u64> = (0..base.len())
            .filter(|j| kept[*j])
            .filter_map(|j| base_ids[j])
            .collect();

        // Delete objects the asset no longer has, unless they are kept added objects (or
        // inside one).
        let mut keep_subtree: HashSet<Entity> = HashSet::new();
        for (i, e) in ents.iter().enumerate().skip(1) {
            if !self.contains(*e) {
                continue;
            }
            let parent_kept = self.parent(*e).is_some_and(|p| keep_subtree.contains(&p));
            if parent_kept || keep.added.contains(e) {
                keep_subtree.insert(*e);
                continue;
            }
            if inst_ids[i].is_some_and(|id| kept_ids.contains(&id)) {
                continue;
            }
            if let Some(id) = inst_ids[i] {
                map.remove(&id);
            }
            self.despawn(*e);
            changed = true;
        }

        // Create objects the instance lacks (empty first, so references can point anywhere).
        let scene = self.scene_of(root);
        let mut fresh: HashSet<Entity> = HashSet::new();
        for (j, entry) in base.iter().enumerate().skip(1) {
            let Some(id) = base_ids[j].filter(|_| kept[j]) else {
                continue;
            };
            if map.contains_key(&id) {
                continue;
            }
            let parent = entry
                .parent
                .and_then(|p| base_ids[p])
                .and_then(|p| map.get(&p).copied())
                .unwrap_or(root);
            let e = self.world.spawn((Node {
                scene,
                ..Default::default()
            },));
            self.attach(e, Some(parent), None);
            map.insert(id, e);
            fresh.insert(e);
            changed = true;
        }

        // Asset object index → its entity in the instance, for references.
        let target: Vec<Option<Entity>> = (0..base.len())
            .map(|j| {
                base_ids[j]
                    .filter(|_| kept[j])
                    .and_then(|id| map.get(&id).copied())
            })
            .collect();
        for (j, entry) in base.iter().enumerate() {
            let Some(e) = target[j] else {
                continue;
            };
            let id = base_ids[j].unwrap_or(ids::ROOT);
            let mut desired = entry.components.clone();
            remap_refs(&mut desired, |index| {
                target
                    .get(index as usize)
                    .copied()
                    .flatten()
                    .map(|t| t.to_bits().get())
            });
            changed |= self.sync_components(e, id, j == 0, fresh.contains(&e), desired, keep);
        }

        // Objects that had no stored id get theirs (they stay matched after renames).
        for (i, e) in ents.iter().enumerate().skip(1) {
            if inst_scope[i].is_none()
                && self.contains(*e)
                && self.world.get::<&PrefabSource>(*e).is_err()
                && let Some(id) = inst_ids[i]
            {
                let _ = self.world.insert_one(*e, PrefabSource { id });
            }
        }

        changed |= self.sync_hierarchy(base, &base_ids, &kept, &map, &fresh);
        if !fresh.is_empty() {
            for e in &fresh {
                fresh_save_ids(self, *e);
            }
            crate::mesh_builder::sync(self);
        }
        changed
    }

    /// One object's components: the asset's values except kept fields; components the asset
    /// added appear, ones it dropped go (unless the instance added them itself).
    fn sync_components(
        &mut self,
        e: Entity,
        id: u64,
        is_root: bool,
        fresh: bool,
        desired: std::collections::BTreeMap<String, Value>,
        keep: &Keep,
    ) -> bool {
        let mut changed = false;
        for (name, value) in &desired {
            let Some(info) = registry::find(name) else {
                continue;
            };
            if info.name == Node::NAME || !fresh && (name == LINK || name == SOURCE) {
                continue;
            }
            match (info.to_json)(&self.world, e) {
                None => {
                    let removed = Override::RemovedComponent {
                        object: id,
                        component: name.clone(),
                    };
                    if !fresh && keep.keys.contains(&removed) {
                        continue;
                    }
                    let _ = (info.from_json)(&mut self.world, e, value.clone());
                    changed = true;
                }
                Some(current) => {
                    let merged = merge_kept(&current, value, |field| {
                        kept_field(&keep.keys, id, is_root, name, field)
                    });
                    if merged != current {
                        let _ = (info.from_json)(&mut self.world, e, merged);
                        changed = true;
                    }
                }
            }
        }
        if fresh {
            return changed;
        }
        for info in registry::components() {
            if matches!(info.name, "GameObject" | "Transform") || diff::ignored(info.name) {
                continue;
            }
            if !(info.has)(&self.world, e) || desired.contains_key(info.name) {
                continue;
            }
            let added = Override::AddedComponent {
                object: id,
                component: info.name.to_owned(),
            };
            if !keep.keys.contains(&added) {
                (info.remove)(&mut self.world, e);
                changed = true;
            }
        }
        // Inspector order: the asset's, then components the instance added.
        let asset_order: Vec<String> = desired
            .get("GameObject")
            .and_then(|g| g.get("component_order"))
            .and_then(|o| serde_json::from_value(o.clone()).ok())
            .unwrap_or_default();
        let has = |w: &hecs::World, n: &str| registry::find(n).is_some_and(|i| (i.has)(w, e));
        let current = self.game_object(e).component_order.clone();
        let mut order: Vec<String> = asset_order
            .into_iter()
            .filter(|n| has(&self.world, n))
            .collect();
        for n in current.iter() {
            if has(&self.world, n) && !order.contains(n) {
                order.push(n.clone());
            }
        }
        if order != current {
            self.game_object_mut(e).component_order = order;
            changed = true;
        }
        changed
    }

    /// Parents and sibling order of the asset's objects (objects the instance added keep their
    /// slots).
    fn sync_hierarchy(
        &mut self,
        base: &[super::super::file::ObjectEntry],
        base_ids: &[Option<u64>],
        kept: &[bool],
        map: &HashMap<u64, Entity>,
        fresh: &HashSet<Entity>,
    ) -> bool {
        let mut changed = false;
        let entity = |j: usize| {
            base_ids[j]
                .filter(|_| kept[j])
                .and_then(|id| map.get(&id).copied())
        };
        let mut rank: HashMap<Entity, usize> = HashMap::new();
        for (j, entry) in base.iter().enumerate().skip(1) {
            let (Some(e), Some(p)) = (entity(j), entry.parent.and_then(entity)) else {
                continue;
            };
            rank.insert(e, j);
            if self.parent(e) != Some(p) && !self.is_ancestor(e, p) {
                self.detach(e);
                self.attach(e, Some(p), None);
                changed |= !fresh.contains(&e);
            }
        }
        // Asset children sorted into the asset's order within the slots they occupy.
        for j in 0..base.len() {
            let Some(p) = entity(j) else {
                continue;
            };
            let Ok(mut node) = self.world.get::<&mut Node>(p) else {
                continue;
            };
            let slots: Vec<usize> = (0..node.children.len())
                .filter(|i| rank.contains_key(&node.children[*i]))
                .collect();
            let mut ordered: Vec<Entity> = slots.iter().map(|i| node.children[*i]).collect();
            ordered.sort_by_key(|e| rank[e]);
            for (slot, e) in slots.into_iter().zip(ordered) {
                if node.children[slot] != e {
                    node.children[slot] = e;
                    changed = true;
                }
            }
        }
        changed
    }
}

/// `desired` with the fields `kept` accepts taken from `current` (left out where `current`
/// doesn't have them).
fn merge_kept(current: &Value, desired: &Value, kept: impl Fn(&str) -> bool) -> Value {
    match (current, desired) {
        (Value::Object(cur), Value::Object(want)) => {
            let mut out = want.clone();
            let keys: Vec<&String> = cur.keys().chain(want.keys()).collect();
            for k in keys {
                if !kept(k) {
                    continue;
                }
                match cur.get(k) {
                    Some(v) => {
                        out.insert(k.clone(), v.clone());
                    }
                    None => {
                        out.remove(k);
                    }
                }
            }
            Value::Object(out)
        }
        _ if kept("") => current.clone(),
        _ => desired.clone(),
    }
}

/// A new object of an instance gets its own save / network ids.
fn fresh_save_ids(scene: &mut Scene, e: Entity) {
    if let Ok(mut s) = scene.world.get::<&mut crate::save::Saveable>(e) {
        s.save_id = crate::save::new_id();
    }
    if let Ok(mut n) = scene.world.get::<&mut crate::network::NetworkObject>(e) {
        n.global_id = crate::save::new_id();
    }
}
