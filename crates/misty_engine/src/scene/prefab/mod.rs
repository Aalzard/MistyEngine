//! Prefabs (`.prefab`): one object and its children saved as an asset, in the scene file format.
//!
//! An instance is a full copy of the asset whose root carries a [`PrefabLink`] to the file, and
//! whose objects carry the asset object they came from ([`PrefabSource`], see [`ids`]). Overrides
//! are the properties, components and objects that differ from the asset ([`diff`]), except the
//! root's name, position and rotation (every instance has its own). When the asset changes, an
//! instance follows it in place and keeps its overrides ([`sync`]); overrides can be applied to
//! the asset one at a time ([`apply`]).
//!
//! A **variant** is a prefab whose root is itself an instance of another prefab (its base): the
//! file's root object keeps a `"Prefab"` link to the base, which [`Prefab::base`] holds while the
//! asset is loaded. Instances inside a prefab (nested prefabs) keep their links like in a scene.

mod apply;
pub mod diff;
pub mod ids;
mod sync;

use std::path::Path;

use hecs::Entity;

pub use diff::Override;

use super::file::{LoadReport, ObjectEntry, read_json, write_json};
use super::{PrefabLink, PrefabSource, Scene, SceneComponent};

pub const EXTENSION: &str = "prefab";

/// A parsed prefab asset. Entries are canonical (as an instance would serialise),
/// so they can be compared with an instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefab {
    objects: Vec<ObjectEntry>,
    /// Variants: the base prefab (the root's link in the file) and the variant's override record.
    base: Option<PrefabLink>,
}

impl Prefab {
    /// Prefab data for `root` and its children (the root's own link is not stored).
    pub fn from_scene(scene: &Scene, root: Entity) -> Self {
        let mut objects = scene.entries(Some(root));
        objects[0].parent = None;
        objects[0].components.remove(PrefabLink::NAME);
        objects[0].components.remove(PrefabSource::NAME);
        Self {
            objects,
            base: None,
        }
    }

    /// Like [`Self::from_scene`], but a link on `root` makes the result a variant of that prefab.
    pub fn from_scene_keep_base(scene: &Scene, root: Entity) -> Self {
        let base = scene
            .world
            .get::<&PrefabLink>(root)
            .ok()
            .map(|l| (*l).clone());
        Self {
            base,
            ..Self::from_scene(scene, root)
        }
    }

    pub fn to_json(&self) -> String {
        let mut objects = self.objects.clone();
        if let Some(base) = &self.base {
            let link = serde_json::to_value(base).expect("link serialises");
            objects[0]
                .components
                .insert(PrefabLink::NAME.to_owned(), link);
        }
        write_json(objects, None, None)
    }

    /// Parse `.prefab` JSON: exactly one root, listed first.
    pub fn from_json(text: &str) -> Result<(Prefab, LoadReport), String> {
        let objects = read_json(text, "prefab")?;
        let roots = objects.iter().filter(|o| o.parent.is_none()).count();
        if objects.first().is_none_or(|o| o.parent.is_some()) || roots != 1 {
            return Err("a prefab needs exactly one root object, listed first".into());
        }
        // Round-trip through a scene so defaults are filled in like on an instance.
        let mut scene = Scene::new("prefab");
        let mut report = LoadReport::default();
        let root = scene.spawn_entries(objects, None, &mut report)[0];
        Ok((Self::from_scene_keep_base(&scene, root), report))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        crate::asset_db::write_atomic(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Prefab, String> {
        Self::load_with_report(path).map(|(p, _)| p)
    }

    /// [`Self::load`] with what was skipped or fixed while reading (unknown components, bad
    /// values, objects whose parent comes later in the file): the prefab is incomplete then,
    /// and writing it back would lose those parts.
    pub fn load_with_report(path: &Path) -> Result<(Prefab, LoadReport), String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The base prefab's path, for variants.
    pub fn base(&self) -> Option<&str> {
        self.base.as_ref().map(|b| b.path.as_str())
    }

    /// The variant's override record (what it changed from its base when last saved).
    pub fn base_record(&self) -> Option<&[Override]> {
        self.base.as_ref().and_then(|b| b.overrides.as_deref())
    }

    /// Make this a variant of `path` (or a plain prefab with `None`).
    pub fn set_base(&mut self, path: Option<&str>, record: Option<Vec<Override>>) {
        self.base = path.map(|p| PrefabLink {
            path: p.to_owned(),
            overrides: record,
        });
    }

    /// Paths of the prefabs instanced inside this one (nested prefabs), without repeats.
    pub fn nested_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .objects
            .iter()
            .skip(1)
            .filter_map(|o| o.components.get(PrefabLink::NAME)?.get("path")?.as_str())
            .map(str::to_owned)
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The same prefab with every object directly in it carrying its id (see [`ids`]). Editor
    /// saves and instances use this, so objects stay matched after renames; files written by
    /// generators keep their plain form.
    pub fn with_ids(mut self) -> Self {
        ids::materialize(&mut self.objects);
        self
    }

    /// Level ids of the prefab's objects (an instance object whose id isn't here was added).
    pub fn object_ids(&self) -> std::collections::HashSet<u64> {
        ids::level_ids(&self.objects)
            .into_iter()
            .flatten()
            .collect()
    }

    /// Name of the object with level id `id` (removed objects in the Overrides list).
    pub fn object_name(&self, id: u64) -> Option<String> {
        let ids = ids::level_ids(&self.objects);
        let i = ids.iter().position(|x| *x == Some(id))?;
        Some(ids::name_of(&self.objects[i]).to_owned())
    }

    /// Objects as (name, depth) in Hierarchy order, for a read-only summary.
    pub fn outline(&self) -> Vec<(String, usize)> {
        let mut depth: Vec<usize> = Vec::with_capacity(self.objects.len());
        self.objects
            .iter()
            .map(|o| {
                let d = o.parent.map(|p| depth[p] + 1).unwrap_or(0);
                depth.push(d);
                (ids::name_of(o).to_owned(), d)
            })
            .collect()
    }

    /// Component names on the root, in Inspector order.
    pub fn root_components(&self) -> Vec<String> {
        self.objects[0].components["GameObject"]["component_order"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A new scene holding this prefab's objects as they are in the asset (Prefab Mode, and
    /// editing a prefab file). A variant's root is linked to its base, so its changes show as
    /// overrides of the base.
    pub fn to_scene(&self, name: &str) -> (Scene, Entity) {
        let mut scene = Scene::new(name);
        let root = self.spawn_into(&mut scene);
        (scene, root)
    }

    /// [`Self::to_scene`] into an existing scene (at its root). Returns the prefab's root.
    pub fn spawn_into(&self, scene: &mut Scene) -> Entity {
        let mut report = LoadReport::default();
        let root = scene.spawn_entries(self.objects.clone(), None, &mut report)[0];
        if let Some(base) = &self.base {
            let _ = scene.world.insert_one(root, base.clone());
        }
        root
    }

    /// Whether this prefab is a variant of `path` or holds instances of it.
    pub fn uses(&self, path: &str) -> bool {
        self.base() == Some(path) || self.nested_paths().iter().any(|p| p == path)
    }

    /// This prefab after the prefab at `path` changed from `old` to `new`: its nested instances
    /// of it, and its root if it is a variant of it, follow the change and keep what this prefab
    /// overrides. `None` when it doesn't use `path` or nothing changed.
    pub fn follow(&self, path: &str, old: &Prefab, new: &Prefab) -> Option<Prefab> {
        if !self.uses(path) {
            return None;
        }
        let (mut scene, root) = self.to_scene("prefab");
        let mut changed = false;
        // Innermost first, so an outer instance sees its inner ones already updated.
        let mut roots = scene.prefab_instances(path);
        roots.reverse();
        for r in roots {
            changed |= scene.resync_prefab(r, old, new);
            let record = scene.prefab_overrides(r, new).into_iter().map(|o| o.key);
            scene.set_prefab_record(r, Some(diff::record(record)));
        }
        changed.then(|| Prefab::from_scene_keep_base(&scene, root).with_ids())
    }

    /// Bring nested instances (and a variant's root) up to date with their prefabs using their
    /// saved override records, for prefabs that changed while this one wasn't loaded. `lookup`
    /// gives a prefab's current contents. `None` when nothing changed.
    pub fn sync_to_records(&self, lookup: &mut dyn FnMut(&str) -> Option<Prefab>) -> Option<Self> {
        if self.base.is_none() && self.nested_paths().is_empty() {
            return None;
        }
        let (mut scene, root) = self.to_scene("prefab");
        let mut changed = false;
        let mut roots = scene.all_prefab_instances();
        roots.reverse();
        for r in roots {
            let Some(path) = scene.prefab_link(r) else {
                continue;
            };
            if let Some(asset) = lookup(&path) {
                changed |= scene.sync_prefab_to_record(r, &asset);
            }
        }
        changed.then(|| Prefab::from_scene_keep_base(&scene, root))
    }

    /// The same prefab with fresh override records on its nested instances and its base link
    /// (written when the prefab is saved). `lookup` gives a prefab's current contents.
    pub fn with_records(&self, lookup: &mut dyn FnMut(&str) -> Option<Prefab>) -> Self {
        if self.base.is_none() && self.nested_paths().is_empty() {
            return self.clone();
        }
        let (mut scene, root) = self.to_scene("prefab");
        for r in scene.all_prefab_instances() {
            let Some(path) = scene.prefab_link(r) else {
                continue;
            };
            if let Some(asset) = lookup(&path) {
                let record = scene.prefab_overrides(r, &asset).into_iter().map(|o| o.key);
                scene.set_prefab_record(r, Some(diff::record(record)));
            }
        }
        Prefab::from_scene_keep_base(&scene, root)
    }

    /// A new variant of the prefab at `base_path` (`base` is its contents): the same objects
    /// and ids, nothing overridden yet, the root named `name`.
    pub fn variant_of(base_path: &str, base: &Prefab, name: &str) -> Prefab {
        let mut objects = base.clone().with_ids().objects;
        if let Some(go) = objects[0].components.get_mut("GameObject") {
            go["name"] = name.into();
        }
        Prefab {
            objects,
            base: Some(PrefabLink {
                path: base_path.to_owned(),
                overrides: Some(Vec::new()),
            }),
        }
    }
}

/// One override of an instance, with the instance object it is on (the added object for added
/// objects; `None` for removed objects).
#[derive(Clone, Debug, PartialEq)]
pub struct OverrideItem {
    pub key: Override,
    pub entity: Option<Entity>,
}

impl Scene {
    /// Instantiate `prefab` under `parent` (unique name among the siblings). `asset_path` is
    /// stored in the root's [`PrefabLink`]; pass `None` for an unlinked copy.
    pub fn instantiate_prefab(
        &mut self,
        prefab: &Prefab,
        asset_path: Option<&str>,
        parent: Option<Entity>,
    ) -> Entity {
        let name = self.unique_name(&prefab.outline()[0].0, parent);
        let mut report = LoadReport::default();
        let root = self.spawn_entries(prefab.objects.clone(), parent, &mut report)[0];
        self.game_object_mut(root).name = name;
        // Every instance is its own object for save games.
        crate::save::fresh_ids_in(self, root);
        if let Some(path) = asset_path {
            let _ = self.world.insert_one(root, PrefabLink::new(path));
        }
        root
    }

    /// The prefab asset path stored on `e` (only on instance roots).
    pub fn prefab_link(&self, e: Entity) -> Option<String> {
        self.world
            .get::<&PrefabLink>(e)
            .ok()
            .map(|l| l.path.clone())
    }

    /// The closest instance root at or above `e`.
    pub fn prefab_root(&self, e: Entity) -> Option<Entity> {
        self.prefab_roots(e).first().copied()
    }

    /// Instance roots at or above `e`, closest first (an object inside a nested prefab has the
    /// nested instance's root, then the outer instance's root, …).
    pub fn prefab_roots(&self, e: Entity) -> Vec<Entity> {
        let mut out = Vec::new();
        let mut cur = Some(e);
        while let Some(c) = cur {
            if self.world.get::<&PrefabLink>(c).is_ok() {
                out.push(c);
            }
            cur = self.parent(c);
        }
        out
    }

    /// The outermost instance root at or above `e` (the one whose asset holds all of it).
    pub fn outermost_prefab_root(&self, e: Entity) -> Option<Entity> {
        self.prefab_roots(e).last().copied()
    }

    /// Link `e` to a prefab asset (after saving it, or to re-link).
    pub fn link_prefab(&mut self, e: Entity, path: &str) {
        let _ = self.world.insert_one(e, PrefabLink::new(path));
    }

    /// Store `record` as the override record of the instance at `root`.
    pub fn set_prefab_record(&mut self, root: Entity, record: Option<Vec<Override>>) {
        if let Ok(mut l) = self.world.get::<&mut PrefabLink>(root) {
            l.overrides = record;
        }
    }

    /// The override record saved with the instance at `root`.
    pub fn prefab_record(&self, root: Entity) -> Option<Vec<Override>> {
        self.world
            .get::<&PrefabLink>(root)
            .ok()
            .and_then(|l| l.overrides.clone())
    }

    /// Store each object's level id on the objects directly in the instance at `root` that have
    /// none (after the instance was saved as a new prefab, the same ids the asset got).
    pub fn store_prefab_ids(&mut self, root: Entity) {
        let (entries, ents) = self.instance_entries(root);
        let (level, scope) = ids::scoped_level_ids(&entries);
        for i in 1..ents.len() {
            if scope[i].is_none()
                && ids::source_of(&entries[i]).is_none()
                && let Some(id) = level[i]
            {
                let _ = self.world.insert_one(ents[i], PrefabSource { id });
            }
        }
    }

    /// A copy of `e` was just made (Duplicate, Paste): drop the prefab ids its objects copied
    /// from the instance around the original. Kept, the copy claimed the original's place in
    /// the instance once it came first in the Hierarchy (Revert then deleted the original),
    /// and in Prefab Mode two objects of the asset shared one id. Objects inside a copied
    /// instance root keep theirs: those are ids in that instance's own asset.
    pub fn forget_copied_prefab_ids(&mut self, e: Entity) {
        fn walk(s: &mut Scene, e: Entity, inside_copied_instance: bool) {
            if !inside_copied_instance {
                let _ = s.world.remove_one::<PrefabSource>(e);
            }
            let inside = inside_copied_instance || s.world.get::<&PrefabLink>(e).is_ok();
            for c in s.children(e) {
                walk(s, c, inside);
            }
        }
        walk(self, e, false);
    }

    /// Unpack: the object becomes a plain GameObject again.
    pub fn unlink_prefab(&mut self, e: Entity) -> bool {
        self.world.remove_one::<PrefabLink>(e).is_ok()
    }

    /// Unpack the instance at `root`: its objects become plain objects. Nested instances stay
    /// instances of their own prefabs unless `completely`. Returns false if `root` isn't one.
    pub fn unpack_prefab(&mut self, root: Entity, completely: bool) -> bool {
        if self.world.get::<&PrefabLink>(root).is_err() {
            return false;
        }
        let (entries, ents) = self.instance_entries(root);
        let (_, scope) = ids::scoped_level_ids(&entries);
        let _ = self.world.remove_one::<PrefabLink>(root);
        for (i, e) in ents.iter().enumerate().skip(1) {
            if completely {
                let _ = self.world.remove_one::<PrefabLink>(*e);
            }
            // The ids of objects directly in this instance (nested roots included) were ids in
            // the asset that no longer applies.
            if completely || scope[i].is_none() {
                let _ = self.world.remove_one::<PrefabSource>(*e);
            }
        }
        true
    }

    /// Instance roots linked to `path`, in Hierarchy order.
    pub fn prefab_instances(&self, path: &str) -> Vec<Entity> {
        self.walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| self.prefab_link(*e).as_deref() == Some(path))
            .collect()
    }

    /// Every instance root (any prefab, nested ones too), in Hierarchy order.
    pub fn all_prefab_instances(&self) -> Vec<Entity> {
        self.walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| self.world.get::<&PrefabLink>(*e).is_ok())
            .collect()
    }

    /// `root` and its children in Hierarchy order.
    pub fn subtree(&self, root: Entity) -> Vec<Entity> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            out.push(e);
            let mut kids = self.children(e);
            kids.reverse();
            stack.extend(kids);
        }
        out
    }

    /// The instance's objects as entries (references outside the instance kept apart from
    /// "none"), with their entities.
    pub(in crate::scene) fn instance_entries(
        &self,
        root: Entity,
    ) -> (Vec<ObjectEntry>, Vec<Entity>) {
        let order = self.subtree(root);
        let mut entries = self.entries_mapped(&order, Some(diff::OUTSIDE));
        if let Some(r) = entries.first_mut() {
            r.parent = None;
        }
        (entries, order)
    }

    /// Level ids of the instance's objects (see [`ids`]), by entity.
    pub fn prefab_object_ids(&self, root: Entity) -> Vec<(Entity, Option<u64>)> {
        let (entries, ents) = self.instance_entries(root);
        ents.into_iter().zip(ids::level_ids(&entries)).collect()
    }

    /// What the instance at `root` changes compared with `prefab`.
    pub fn prefab_overrides(&self, root: Entity, prefab: &Prefab) -> Vec<OverrideItem> {
        let (entries, ents) = self.instance_entries(root);
        diff::diff(&entries, &prefab.objects)
            .into_iter()
            .map(|(key, i)| OverrideItem {
                key,
                entity: i.map(|i| ents[i]),
            })
            .collect()
    }

    /// Whether the instance at `root` differs from `prefab`.
    pub fn has_prefab_overrides(&self, root: Entity, prefab: &Prefab) -> bool {
        !self.prefab_overrides(root, prefab).is_empty()
    }

    /// Prefab data to write back for the instance at `root` (Apply All): its contents, with the
    /// asset's own root name, position and rotation (and base, for variants) kept from `asset`.
    /// Objects the instance added get ids in both.
    pub fn prefab_to_apply(&mut self, root: Entity, asset: &Prefab) -> Prefab {
        let keys: Vec<Override> = self
            .prefab_overrides(root, asset)
            .into_iter()
            .map(|o| o.key)
            .collect();
        self.apply_prefab_overrides(root, asset, &keys)
    }

    /// Make the instance at `root` an unchanged copy of `prefab` again (Revert All), in place:
    /// objects keep their entities, the root keeps its name, position, rotation and place.
    pub fn revert_prefab(&mut self, root: Entity, prefab: &Prefab) -> Entity {
        self.sync_prefab_instance(root, prefab, &sync::Keep::default());
        root
    }

    /// Revert some of the instance's overrides (each back to the asset's value).
    pub fn revert_prefab_overrides(&mut self, root: Entity, prefab: &Prefab, keys: &[Override]) {
        let items: Vec<OverrideItem> = self
            .prefab_overrides(root, prefab)
            .into_iter()
            .filter(|o| !keys.contains(&o.key))
            .collect();
        self.sync_prefab_instance(root, prefab, &sync::Keep::from_items(&items));
    }

    /// The asset of the instance at `root` changed from `old` to `new`: follow it, keeping what
    /// the instance overrode (compared with `old`). Returns whether anything changed.
    pub fn resync_prefab(&mut self, root: Entity, old: &Prefab, new: &Prefab) -> bool {
        let items = self.prefab_overrides(root, old);
        self.sync_prefab_instance(root, new, &sync::Keep::from_items(&items))
    }

    /// Bring the instance at `root` up to date with `prefab` using its saved override record
    /// (an asset that changed while the scene was closed). Instances without a record (saved
    /// before records existed) and instances already matching are left alone. Returns whether
    /// it changed.
    pub fn sync_prefab_to_record(&mut self, root: Entity, prefab: &Prefab) -> bool {
        let Some(record) = self.prefab_record(root) else {
            return false;
        };
        let items = self.prefab_overrides(root, prefab);
        let now = diff::record(items.iter().map(|o| o.key.clone()));
        if now == record {
            return false;
        }
        // Added objects the record knows stay; anything else the asset lacks goes.
        let keep = sync::Keep::from_record(&record, &items);
        self.sync_prefab_instance(root, prefab, &keep)
    }
}

#[cfg(test)]
mod tests;
