//! Apply an instance's overrides to its asset, one at a time or all together.
//!
//! The asset is loaded into a scratch scene, each override is copied over from the instance,
//! and the result is read back as prefab data. Objects the instance added get new ids, in the
//! asset and on the instance, so the instance matches the new asset afterwards.

use std::collections::HashMap;

use hecs::{Entity, EntityBuilder};

use super::super::file::LoadReport;
use super::super::{Node, PrefabSource, Scene, registry};
use super::diff::{Override, follows};
use super::ids;
use super::{OverrideItem, Prefab};

impl Scene {
    /// The asset `asset` with the instance's overrides `keys` applied (the instance at `root`
    /// gets ids for the objects it added). The root's name and placement stay the asset's.
    pub fn apply_prefab_overrides(
        &mut self,
        root: Entity,
        asset: &Prefab,
        keys: &[Override],
    ) -> Prefab {
        let items: Vec<OverrideItem> = self
            .prefab_overrides(root, asset)
            .into_iter()
            .filter(|o| keys.contains(&o.key))
            .collect();
        let (mut scratch, scratch_root) = Scene::new("prefab").with_objects(asset);
        // Instance object ↔ scratch object, through their level ids.
        let mine: HashMap<Entity, u64> = self
            .prefab_object_ids(root)
            .into_iter()
            .filter_map(|(e, id)| id.map(|id| (e, id)))
            .collect();
        let mut theirs: HashMap<u64, Entity> = scratch
            .prefab_object_ids(scratch_root)
            .into_iter()
            .filter_map(|(e, id)| id.map(|id| (id, e)))
            .collect();
        let mut copies: HashMap<Entity, Entity> = HashMap::new();

        let rank = |o: &Override| match o {
            Override::RemovedObject { .. } => 0,
            Override::RemovedComponent { .. } => 1,
            Override::AddedObject { .. } => 2,
            Override::AddedComponent { .. } => 3,
            Override::Property { .. } => 4,
        };
        let mut items = items;
        items.sort_by_key(|o| rank(&o.key));
        for item in &items {
            match &item.key {
                Override::RemovedObject { object } => {
                    if let Some(e) = theirs.remove(object) {
                        scratch.despawn(e);
                    }
                }
                Override::RemovedComponent { object, component } => {
                    if let Some(e) = theirs.get(object) {
                        scratch.remove_by_name(*e, component);
                    }
                }
                Override::AddedObject { parent, .. } => {
                    let (Some(added), Some(&under)) = (item.entity, theirs.get(parent)) else {
                        continue;
                    };
                    copy_into(self, added, &mut scratch, under, &mut copies);
                    // New ids for the objects directly in the asset, on both sides (an added
                    // prefab instance gets one for its root; its objects keep their own).
                    let (entries, ents) = self.instance_entries(added);
                    let (_, scope) = ids::scoped_level_ids(&entries);
                    let nested = self.world.get::<&super::super::PrefabLink>(added).is_ok();
                    for (i, e) in ents.iter().enumerate() {
                        if i > 0 && (nested || scope[i].is_some()) {
                            continue;
                        }
                        let id = ids::fresh_id();
                        let _ = self.world.insert_one(*e, PrefabSource { id });
                        if let Some(c) = copies.get(e) {
                            let _ = scratch.world.insert_one(*c, PrefabSource { id });
                        }
                    }
                }
                Override::AddedComponent { object, component } => {
                    let (Some(src), Some(&dst)) = (item.entity, theirs.get(object)) else {
                        continue;
                    };
                    let Some(info) = registry::find(component) else {
                        continue;
                    };
                    if let Some(v) = (info.to_json)(&self.world, src) {
                        let v = remap_value(component, v, &mine, &theirs, &copies);
                        let _ = (info.from_json)(&mut scratch.world, dst, v);
                        let order = &mut scratch.game_object_mut(dst).component_order;
                        if !order.contains(component) {
                            order.push(component.clone());
                        }
                    }
                }
                Override::Property {
                    object,
                    component,
                    property,
                } => {
                    let (Some(src), Some(&dst)) = (item.entity, theirs.get(object)) else {
                        continue;
                    };
                    copy_property(
                        self,
                        src,
                        &mut scratch,
                        dst,
                        component,
                        property,
                        &mine,
                        &theirs,
                        &copies,
                    );
                }
            }
        }
        // References held by copied objects point at the scratch asset's objects.
        for (orig, copy) in &copies {
            for name in super::super::file::ref_components() {
                let Some(info) = registry::find(name) else {
                    continue;
                };
                if let Some(v) = (info.to_json)(&self.world, *orig) {
                    let v = remap_value(name, v, &mine, &theirs, &copies);
                    let _ = (info.from_json)(&mut scratch.world, *copy, v);
                }
            }
        }
        let mut out = Prefab::from_scene(&scratch, scratch_root).with_ids();
        out.base = asset.base.clone();
        out
    }

    /// A scene holding `prefab`'s objects as in the asset (no link on the root).
    pub(crate) fn with_objects(mut self, prefab: &Prefab) -> (Scene, Entity) {
        let mut report = LoadReport::default();
        let root = self.spawn_entries(prefab.objects.clone(), None, &mut report)[0];
        (self, root)
    }
}

/// Copy `e` and its children from `from` under `parent` in `to`; `copies` maps originals to
/// copies. The caller points the copies' object references at `to`'s objects afterwards.
fn copy_into(
    from: &Scene,
    e: Entity,
    to: &mut Scene,
    parent: Entity,
    copies: &mut HashMap<Entity, Entity>,
) {
    let mut b = EntityBuilder::new();
    for info in registry::components() {
        if info.name != "Node" {
            (info.clone_into)(&from.world, e, &mut b);
        }
    }
    b.add(Node {
        scene: to.scene_of(parent),
        ..Default::default()
    });
    let copy = to.world.spawn(b.build());
    to.attach(copy, Some(parent), None);
    copies.insert(e, copy);
    for c in from.children(e) {
        copy_into(from, c, to, copy, copies);
    }
}

/// A component value read from the instance, with object references pointed at the scratch
/// asset's objects (copies first, then matched objects; others become none).
fn remap_value(
    component: &str,
    value: serde_json::Value,
    mine: &HashMap<Entity, u64>,
    theirs: &HashMap<u64, Entity>,
    copies: &HashMap<Entity, Entity>,
) -> serde_json::Value {
    let mut one = std::collections::BTreeMap::from([(component.to_owned(), value)]);
    super::super::file::remap_refs(&mut one, |bits| {
        let e = Entity::from_bits(bits)?;
        copies
            .get(&e)
            .copied()
            .or_else(|| mine.get(&e).and_then(|id| theirs.get(id).copied()))
            .map(|t| t.to_bits().get())
    });
    one.remove(component).unwrap_or_default()
}

/// Copy one property (and the hint that goes with it) from the instance object to the asset's.
#[allow(clippy::too_many_arguments)]
fn copy_property(
    from: &Scene,
    src: Entity,
    to: &mut Scene,
    dst: Entity,
    component: &str,
    property: &str,
    mine: &HashMap<Entity, u64>,
    theirs: &HashMap<u64, Entity>,
    copies: &HashMap<Entity, Entity>,
) {
    let Some(info) = registry::find(component) else {
        return;
    };
    let (Some(value), Some(target)) = (
        (info.to_json)(&from.world, src),
        (info.to_json)(&to.world, dst),
    ) else {
        return;
    };
    let value = remap_value(component, value, mine, theirs, copies);
    let merged = match (value, target) {
        (serde_json::Value::Object(v), serde_json::Value::Object(mut t)) => {
            for (k, x) in &v {
                if k == property || follows(component, k) == Some(property) {
                    t.insert(k.clone(), x.clone());
                }
            }
            if !v.contains_key(property) {
                t.remove(property);
            }
            serde_json::Value::Object(t)
        }
        (v, _) => v,
    };
    let _ = (info.from_json)(&mut to.world, dst, merged);
}
