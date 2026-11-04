//! Prefabs (`.prefab`): one object and its children saved as an asset, in the scene file format.
//!
//! An instance is a full copy of the asset whose root carries a [`PrefabLink`] to the file.
//! Overrides are whatever differs from the asset, except the root's name, position and rotation
//! (every instance has its own).

use std::path::Path;

use hecs::Entity;

use super::file::{LoadReport, ObjectEntry, read_json, write_json};
use super::{PrefabLink, Scene, SceneComponent, Transform};

pub const EXTENSION: &str = "prefab";

/// A parsed prefab asset. Entries are canonical (as an instance would serialise),
/// so they can be compared with an instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefab {
    objects: Vec<ObjectEntry>,
}

impl Prefab {
    /// Prefab data for `root` and its children (the root's own link is not stored).
    pub fn from_scene(scene: &Scene, root: Entity) -> Self {
        let mut objects = scene.entries(Some(root));
        objects[0].parent = None;
        objects[0].components.remove(PrefabLink::NAME);
        Self { objects }
    }

    pub fn to_json(&self) -> String {
        write_json(self.objects.clone(), None, None)
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
        Ok((Self::from_scene(&scene, root), report))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Prefab, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text)
            .map(|(p, _)| p)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Objects as (name, depth) in Hierarchy order, for a read-only summary.
    pub fn outline(&self) -> Vec<(String, usize)> {
        let mut depth: Vec<usize> = Vec::with_capacity(self.objects.len());
        self.objects
            .iter()
            .map(|o| {
                let d = o.parent.map(|p| depth[p] + 1).unwrap_or(0);
                depth.push(d);
                let name = o.components["GameObject"]["name"]
                    .as_str()
                    .unwrap_or("GameObject")
                    .to_owned();
                (name, d)
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

    /// Entries with the root's per-instance values blanked, for comparing.
    fn comparable(&self) -> Vec<ObjectEntry> {
        let mut objects = self.objects.clone();
        let root = &mut objects[0].components;
        if let Some(go) = root.get_mut("GameObject") {
            go["name"] = serde_json::Value::Null;
        }
        if let Some(t) = root.get_mut(Transform::NAME) {
            for key in ["position", "rotation", "euler_hint"] {
                t[key] = serde_json::Value::Null;
            }
        }
        objects
    }

    /// Copy the root's name, position and rotation from `other`.
    fn with_root_of(mut self, other: &Prefab) -> Self {
        let src = &other.objects[0].components;
        let dst = &mut self.objects[0].components;
        if let (Some(s), Some(d)) = (src.get("GameObject"), dst.get_mut("GameObject")) {
            d["name"] = s["name"].clone();
        }
        if let (Some(s), Some(d)) = (src.get(Transform::NAME), dst.get_mut(Transform::NAME)) {
            for key in ["position", "rotation", "euler_hint"] {
                d[key] = s[key].clone();
            }
        }
        self
    }
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
        if let Some(path) = asset_path {
            let _ = self
                .world
                .insert_one(root, PrefabLink { path: path.into() });
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
        let mut cur = Some(e);
        while let Some(c) = cur {
            if self.world.get::<&PrefabLink>(c).is_ok() {
                return Some(c);
            }
            cur = self.parent(c);
        }
        None
    }

    /// Link `e` to a prefab asset (after saving it, or to re-link).
    pub fn link_prefab(&mut self, e: Entity, path: &str) {
        let _ = self.world.insert_one(e, PrefabLink { path: path.into() });
    }

    /// Unpack: the object becomes a plain GameObject again.
    pub fn unlink_prefab(&mut self, e: Entity) -> bool {
        self.world.remove_one::<PrefabLink>(e).is_ok()
    }

    /// Instance roots linked to `path`, in Hierarchy order.
    pub fn prefab_instances(&self, path: &str) -> Vec<Entity> {
        self.walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| self.prefab_link(*e).as_deref() == Some(path))
            .collect()
    }

    /// Whether the instance at `root` differs from `prefab`.
    pub fn has_prefab_overrides(&self, root: Entity, prefab: &Prefab) -> bool {
        Prefab::from_scene(self, root).comparable() != prefab.comparable()
    }

    /// Prefab data to write back for the instance at `root`: its contents, with the
    /// asset's own root name, position and rotation kept from `asset`.
    pub fn prefab_to_apply(&self, root: Entity, asset: &Prefab) -> Prefab {
        Prefab::from_scene(self, root).with_root_of(asset)
    }

    /// Replace the instance at `root` with a fresh copy of `prefab`, keeping its place in the
    /// Hierarchy, name, position, rotation and link. Returns the new root.
    pub fn revert_prefab(&mut self, root: Entity, prefab: &Prefab) -> Entity {
        let parent = self.parent(root);
        let index = self.sibling_index(root);
        let mine = Prefab::from_scene(self, root);
        let link = self.prefab_link(root);
        let scene = self.scene_of(root);
        let fresh = prefab.clone().with_root_of(&mine);
        self.despawn(root);
        let mut report = LoadReport::default();
        let new_root = self.spawn_entries(fresh.objects, parent, &mut report)[0];
        self.set_scene_of_subtree(new_root, scene);
        self.set_parent(new_root, parent, Some(index), false);
        if let Some(path) = link {
            self.link_prefab(new_root, &path);
        }
        new_root
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::scene::{BoxCollider, MeshFilter, PrimitiveMesh};

    const PATH: &str = "Assets/Crate.prefab";

    /// A "Crate" with a child "Lid" and a Box Collider.
    fn source() -> (Scene, Entity) {
        let mut s = Scene::new("t");
        let root = s.spawn("Crate", None);
        s.set_transform(root, Transform::from_position(Vec3::new(4.0, 0.0, 0.0)));
        s.add(root, BoxCollider::default());
        let lid = s.spawn("Lid", Some(root));
        s.add(
            lid,
            MeshFilter {
                mesh: Some(PrimitiveMesh::Cube.into()),
            },
        );
        (s, root)
    }

    fn names(s: &Scene) -> Vec<(String, usize)> {
        s.walk().into_iter().map(|(e, d)| (s.name(e), d)).collect()
    }

    #[test]
    fn round_trip_and_instantiate() {
        let (s, root) = source();
        let prefab = Prefab::from_scene(&s, root);
        let (loaded, report) = Prefab::from_json(&prefab.to_json()).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(loaded, prefab);
        assert_eq!(
            loaded.outline(),
            [("Crate".to_owned(), 0), ("Lid".to_owned(), 1)]
        );
        assert_eq!(loaded.root_components(), ["Transform", "Box Collider"]);

        let mut scene = Scene::new("level");
        let a = scene.instantiate_prefab(&loaded, Some(PATH), None);
        let b = scene.instantiate_prefab(&loaded, Some(PATH), None);
        assert_eq!(
            names(&scene),
            [
                ("Crate".to_owned(), 0),
                ("Lid".to_owned(), 1),
                ("Crate (1)".to_owned(), 0),
                ("Lid".to_owned(), 1)
            ]
        );
        assert_eq!(scene.prefab_link(a).as_deref(), Some(PATH));
        assert_eq!(scene.prefab_instances(PATH), [a, b]);
        let lid = scene.children(a)[0];
        assert_eq!(scene.prefab_root(lid), Some(a));
        assert_eq!(scene.prefab_link(lid), None, "only the root is linked");
        assert_eq!(scene.transform(a).position, Vec3::new(4.0, 0.0, 0.0));
        assert!(
            !scene.component_names(a).iter().any(|n| n == "Prefab"),
            "the link is not an Inspector component"
        );
    }

    #[test]
    fn link_survives_scene_save() {
        let (s, root) = source();
        let prefab = Prefab::from_scene(&s, root);
        let mut scene = Scene::new("level");
        let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
        let (loaded, _) = Scene::from_json("level", &scene.to_json()).unwrap();
        let root = loaded.roots()[0];
        assert_eq!(loaded.prefab_link(root).as_deref(), Some(PATH));
        assert!(!loaded.has_prefab_overrides(root, &prefab));
        assert!(scene.unlink_prefab(a));
        assert_eq!(scene.prefab_root(a), None);
    }

    #[test]
    fn root_name_and_position_are_not_overrides() {
        let (s, root) = source();
        let prefab = Prefab::from_scene(&s, root);
        let mut scene = Scene::new("level");
        let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
        scene.game_object_mut(a).name = "Renamed".into();
        scene.set_transform(
            a,
            Transform::from_position(Vec3::ONE).with_euler(Vec3::new(0.0, 90.0, 0.0)),
        );
        assert!(!scene.has_prefab_overrides(a, &prefab));

        let mut t = scene.transform(a);
        t.scale = Vec3::splat(2.0);
        scene.set_transform(a, t);
        assert!(
            scene.has_prefab_overrides(a, &prefab),
            "scale is an override"
        );
    }

    #[test]
    fn revert_restores_contents_but_keeps_placement() {
        let (s, root) = source();
        let prefab = Prefab::from_scene(&s, root);
        let mut scene = Scene::new("level");
        scene.spawn("Before", None);
        let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
        scene.spawn("After", None);
        scene.set_transform(a, Transform::from_position(Vec3::new(0.0, 5.0, 0.0)));
        let lid = scene.children(a)[0];
        scene.despawn(lid);
        scene.spawn("Extra", Some(a));
        assert!(scene.has_prefab_overrides(a, &prefab));

        let a = scene.revert_prefab(a, &prefab);
        assert!(!scene.has_prefab_overrides(a, &prefab));
        assert_eq!(scene.sibling_index(a), 1, "same place in the Hierarchy");
        assert_eq!(scene.transform(a).position, Vec3::new(0.0, 5.0, 0.0));
        assert_eq!(scene.prefab_link(a).as_deref(), Some(PATH));
        assert_eq!(scene.name(scene.children(a)[0]), "Lid");
    }

    #[test]
    fn apply_keeps_the_asset_root_placement() {
        let (s, root) = source();
        let prefab = Prefab::from_scene(&s, root);
        let mut scene = Scene::new("level");
        let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
        scene.set_transform(a, Transform::from_position(Vec3::new(9.0, 9.0, 9.0)));
        scene.game_object_mut(a).name = "Moved".into();
        scene.spawn("Handle", Some(a));

        let applied = scene.prefab_to_apply(a, &prefab);
        assert_eq!(applied.outline()[0].0, "Crate");
        assert_eq!(applied.outline().len(), 3);
        let mut other = Scene::new("o");
        let b = other.instantiate_prefab(&applied, None, None);
        assert_eq!(other.transform(b).position, Vec3::new(4.0, 0.0, 0.0));
        assert!(!scene.has_prefab_overrides(a, &applied));
    }

    #[test]
    fn invalid_prefabs_are_rejected() {
        assert!(Prefab::from_json(r#"{"version": 1, "objects": []}"#).is_err());
        let two_roots = r#"{"version": 1, "objects": [
            {"parent": null, "components": {}}, {"parent": null, "components": {}}]}"#;
        assert!(Prefab::from_json(two_roots).is_err());
    }
}
