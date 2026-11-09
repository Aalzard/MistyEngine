//! Several scenes open at once (additive loading, like the familiar multi-scene editing).
//!
//! All loaded scenes share one [`Scene`] world, so rendering, physics and scripts see every
//! object without changes. Each object's [`Node::scene`] names the scene it belongs to; children
//! always share their parent's. The *active* scene's name, lighting and navigation settings live
//! in `Scene`'s own fields (everything that reads `scene.render_settings` gets the active
//! scene's); the other scenes keep theirs in their [`SceneEntry`] and swap in when made active.

use hecs::Entity;

use super::file::LoadReport;
use super::render_settings::RenderSettings;
use super::{Node, Scene};
use crate::navigation::NavigationSettings;

/// Id of an open scene inside one [`Scene`] world (not saved; ids are never reused).
pub type SceneId = u32;

/// One scene in the open set.
#[derive(Clone, Debug)]
pub struct SceneEntry {
    pub id: SceneId,
    /// File name without extension (the active scene's live copy is `Scene::name`).
    name: String,
    /// Project-relative file path (`None` = never saved).
    pub path: Option<String>,
    /// Unloaded scenes stay listed (greyed out) but have no objects.
    pub loaded: bool,
    /// Objects are still being added a slice at a time (a streamed World Partition cell).
    pub partial: bool,
    /// Loaded by World Partition streaming (a grid cell), not opened by hand.
    pub streamed: bool,
    /// Lighting and navigation of a loaded scene that is not the active one.
    stash: Option<Box<(RenderSettings, NavigationSettings)>>,
}

impl SceneEntry {
    pub(super) fn new(id: SceneId, name: &str, path: Option<String>) -> Self {
        Self {
            id,
            name: name.to_owned(),
            path,
            loaded: true,
            partial: false,
            streamed: false,
            stash: None,
        }
    }

    /// Keep the lighting and navigation of a scene that is not the active one.
    pub(super) fn set_stash(&mut self, lighting: RenderSettings, navigation: NavigationSettings) {
        self.stash = Some(Box::new((lighting, navigation)));
    }
}

/// "Assets/Scenes/Level.scene" → "Level".
pub fn name_from_path(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .to_owned()
}

impl Scene {
    // ---- the open set ----------------------------------------------------------------------

    /// Every open scene (loaded or not) in Hierarchy order.
    pub fn scenes(&self) -> &[SceneEntry] {
        &self.scenes
    }

    pub fn entry(&self, id: SceneId) -> Option<&SceneEntry> {
        self.scenes.iter().find(|s| s.id == id)
    }

    fn entry_mut(&mut self, id: SceneId) -> Option<&mut SceneEntry> {
        self.scenes.iter_mut().find(|s| s.id == id)
    }

    /// Ids of the loaded scenes, in order.
    pub fn loaded_scenes(&self) -> Vec<SceneId> {
        self.scenes
            .iter()
            .filter(|s| s.loaded)
            .map(|s| s.id)
            .collect()
    }

    pub fn scene_name(&self, id: SceneId) -> String {
        if id == self.active {
            return self.name.clone();
        }
        self.entry(id).map(|s| s.name.clone()).unwrap_or_default()
    }

    pub fn scene_path(&self, id: SceneId) -> Option<String> {
        self.entry(id).and_then(|s| s.path.clone())
    }

    /// Point a scene at a file (after Save As); its name follows the file name.
    pub fn set_scene_path(&mut self, id: SceneId, path: &str) {
        let name = name_from_path(path);
        if id == self.active {
            self.name = name.clone();
        }
        if let Some(s) = self.entry_mut(id) {
            s.path = Some(path.to_owned());
            s.name = name;
        }
    }

    /// The scene is no longer tied to a file (Untitled until saved again).
    pub fn forget_scene_path(&mut self, id: SceneId) {
        if let Some(s) = self.entry_mut(id) {
            s.path = None;
        }
    }

    /// Follow renamed or moved scene files (`f` maps an old path to its new one).
    pub fn remap_scene_paths(&mut self, f: impl Fn(&str) -> Option<String>) {
        let moved: Vec<(SceneId, String)> = self
            .scenes
            .iter()
            .filter_map(|s| Some((s.id, f(s.path.as_deref()?)?)))
            .collect();
        for (id, path) in moved {
            self.set_scene_path(id, &path);
        }
    }

    /// The scene new objects go into and whose lighting is used.
    pub fn active_scene(&self) -> SceneId {
        self.active
    }

    /// File of the active scene (baked lighting and nav data are stored next to it).
    pub fn active_path(&self) -> Option<String> {
        self.scene_path(self.active)
    }

    /// The loaded scene with this name or project-relative path.
    pub fn find_loaded(&self, name_or_path: &str) -> Option<SceneId> {
        self.scenes
            .iter()
            .filter(|s| s.loaded)
            .find(|s| {
                s.path.as_deref() == Some(name_or_path) || self.scene_name(s.id) == name_or_path
            })
            .map(|s| s.id)
    }

    /// The open scene (loaded or not) saved at `path`.
    pub fn find_by_path(&self, path: &str) -> Option<SceneId> {
        self.scenes
            .iter()
            .find(|s| s.path.as_deref() == Some(path))
            .map(|s| s.id)
    }

    /// Make a loaded scene active: its lighting and navigation become the world's.
    pub fn set_active_scene(&mut self, id: SceneId) -> bool {
        if id == self.active {
            return true;
        }
        if !self.entry(id).is_some_and(|s| s.loaded) {
            return false;
        }
        let old = self.active;
        let settings = (
            std::mem::take(&mut self.render_settings),
            std::mem::take(&mut self.navigation),
        );
        let name = self.name.clone();
        if let Some(e) = self.entry_mut(old) {
            e.name = name;
            e.stash = Some(Box::new(settings));
        }
        let entry = self.entry_mut(id).expect("checked above");
        let (render, nav) = *entry.stash.take().unwrap_or_default();
        let name = entry.name.clone();
        self.render_settings = render;
        self.navigation = nav;
        self.name = name;
        self.active = id;
        true
    }

    /// Undo and redo put back a whole copy of the world. Which scene is active and which file
    /// each scene is saved to are not undoable edits (Set Active Scene, Save As), so the restored
    /// copy keeps `current`'s for every scene both know about.
    pub fn keep_open_set_from(&mut self, current: &Scene) {
        if current.active != self.active {
            self.set_active_scene(current.active);
        }
        for entry in &current.scenes {
            let Some(mine) = self.entry(entry.id) else {
                continue;
            };
            if mine.path == entry.path {
                continue;
            }
            match &entry.path {
                Some(path) => self.set_scene_path(entry.id, path),
                None => self.forget_scene_path(entry.id),
            }
        }
    }

    /// Lighting and navigation of any loaded scene (the active one's are the world's).
    pub fn scene_settings(&self, id: SceneId) -> (RenderSettings, NavigationSettings) {
        if id == self.active {
            return (self.render_settings.clone(), self.navigation.clone());
        }
        self.entry(id)
            .and_then(|s| s.stash.as_deref().cloned())
            .unwrap_or_default()
    }

    // ---- objects of a scene ----------------------------------------------------------------

    /// Scene `e` belongs to.
    pub fn scene_of(&self, e: Entity) -> SceneId {
        self.world.get::<&Node>(e).map_or(self.active, |n| n.scene)
    }

    /// Scene new root objects go into.
    pub(crate) fn spawn_target(&self) -> SceneId {
        self.spawn_into.unwrap_or(self.active)
    }

    /// Tag `e` and its children as part of `scene`.
    pub(crate) fn set_scene_of_subtree(&mut self, e: Entity, scene: SceneId) {
        if let Ok(mut n) = self.world.get::<&mut Node>(e) {
            n.scene = scene;
        }
        for c in self.children(e) {
            self.set_scene_of_subtree(c, scene);
        }
    }

    /// Top-level objects of one scene, in Hierarchy order.
    pub fn roots_in(&self, id: SceneId) -> Vec<Entity> {
        self.roots
            .iter()
            .copied()
            .filter(|r| self.scene_of(*r) == id)
            .collect()
    }

    /// Every object of one scene, depth-first.
    pub fn objects_in(&self, id: SceneId) -> Vec<Entity> {
        self.walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| self.scene_of(*e) == id)
            .collect()
    }

    /// Move a top-level object (and its children) into another loaded scene, after that
    /// scene's last root. Objects inside a parent are moved to the top level first.
    pub fn move_to_scene(&mut self, e: Entity, id: SceneId) -> bool {
        if !self.contains(e) || !self.entry(id).is_some_and(|s| s.loaded) {
            return false;
        }
        if self.parent(e).is_some() {
            self.set_parent(e, None, None, true);
        }
        if self.scene_of(e) == id {
            return true;
        }
        self.roots.retain(|r| *r != e);
        let at = self
            .roots
            .iter()
            .rposition(|r| self.scene_of(*r) == id)
            .map_or(self.roots.len(), |i| i + 1);
        self.roots.insert(at, e);
        self.set_scene_of_subtree(e, id);
        true
    }

    // ---- loading and unloading -------------------------------------------------------------

    /// Add a scene file's objects to the world as another loaded scene (`path` is its
    /// project-relative path). An unloaded entry with the same path is loaded again in place.
    pub fn load_additive(
        &mut self,
        path: &str,
        text: &str,
    ) -> Result<(SceneId, Vec<Entity>, LoadReport), String> {
        let existing = self.find_by_path(path);
        if let Some(id) = existing
            && self.entry(id).is_some_and(|s| s.loaded)
        {
            return Err(format!("'{}' is already open", name_from_path(path)));
        }
        let (file, mut report) = super::file::read_scene_parts(text)?;
        let id = existing.unwrap_or_else(|| {
            let id = self.next_scene_id;
            self.next_scene_id += 1;
            self.scenes.push(SceneEntry::new(
                id,
                &name_from_path(path),
                Some(path.into()),
            ));
            id
        });
        let entry = self.entry_mut(id).expect("just added");
        entry.loaded = true;
        entry.stash = Some(Box::new((file.lighting, file.navigation)));
        self.spawn_into = Some(id);
        let spawned = self.spawn_entries(file.objects, None, &mut report);
        self.spawn_into = None;
        let roots = spawned
            .into_iter()
            .filter(|e| self.parent(*e).is_none())
            .collect();
        Ok((id, roots, report))
    }

    /// Replace a loaded scene's objects and settings with a new version of its file (the file
    /// changed on disk). The scene keeps its id, name, path, place in the open set and whether
    /// it is active; its objects are new entities. A file that doesn't parse leaves the scene
    /// alone. Returns the new root objects.
    pub fn reload_scene(
        &mut self,
        id: SceneId,
        text: &str,
    ) -> Result<(Vec<Entity>, LoadReport), String> {
        if !self.entry(id).is_some_and(|s| s.loaded) {
            return Err("the scene is not loaded".into());
        }
        let (file, mut report) = super::file::read_scene_parts(text)?;
        for r in self.roots_in(id) {
            self.despawn(r);
        }
        if id == self.active {
            self.render_settings = file.lighting;
            self.navigation = file.navigation;
        } else if let Some(e) = self.entry_mut(id) {
            e.stash = Some(Box::new((file.lighting, file.navigation)));
        }
        self.spawn_into = Some(id);
        let spawned = self.spawn_entries(file.objects, None, &mut report);
        self.spawn_into = None;
        let roots = spawned
            .into_iter()
            .filter(|e| self.parent(*e).is_none())
            .collect();
        Ok((roots, report))
    }

    /// Remove a scene's objects but keep it listed (greyed out). The last loaded scene can't
    /// be unloaded; unloading the active scene makes the first other loaded scene active.
    pub fn unload_scene(&mut self, id: SceneId) -> Result<(), String> {
        let Some(entry) = self.entry(id) else {
            return Err("no such scene".into());
        };
        if !entry.loaded {
            return Ok(());
        }
        if self.loaded_scenes().len() <= 1 {
            return Err("the last loaded scene can't be unloaded".into());
        }
        if id == self.active {
            let next = self
                .loaded_scenes()
                .into_iter()
                .find(|s| *s != id)
                .expect("another scene is loaded");
            self.set_active_scene(next);
        }
        for r in self.roots_in(id) {
            self.despawn(r);
        }
        if let Some(e) = self.entry_mut(id) {
            e.loaded = false;
            e.stash = None;
        }
        Ok(())
    }

    /// Unload a scene (if loaded) and take it out of the list.
    pub fn remove_scene(&mut self, id: SceneId) -> Result<(), String> {
        self.unload_scene(id)?;
        if self.scenes.len() <= 1 {
            return Err("the last scene can't be removed".into());
        }
        self.scenes.retain(|s| s.id != id);
        Ok(())
    }

    /// Unloaded entry for a scene file that isn't open (restoring a saved open set).
    pub fn add_unloaded(&mut self, path: &str) -> SceneId {
        if let Some(id) = self.find_by_path(path) {
            return id;
        }
        let id = self.next_scene_id;
        self.next_scene_id += 1;
        let mut entry = SceneEntry::new(id, &name_from_path(path), Some(path.into()));
        entry.loaded = false;
        self.scenes.push(entry);
        id
    }

    // ---- saving ------------------------------------------------------------------------------

    /// One scene's `.scene` JSON (only its objects; references to other scenes become null).
    pub fn scene_to_json(&self, id: SceneId) -> String {
        let (render, nav) = self.scene_settings(id);
        let lighting = Some(&render).filter(|r| **r != RenderSettings::default());
        let navigation = Some(&nav).filter(|n| **n != NavigationSettings::default());
        let objects = self.entries_where(|e| self.scene_of(e) == id);
        super::file::write_json(objects, lighting, navigation)
    }

    /// Write one scene to disk.
    pub fn save_scene(&self, id: SceneId, path: &std::path::Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // With a project open, the asset GUID table goes in too (like `Scene::save`).
        let text = crate::asset_db::embed_json(&self.scene_to_json(id));
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// References from objects of `id` to objects in other scenes (not allowed: they are
    /// saved as None). One line per reference, for the Console.
    pub fn cross_scene_references(&self, id: SceneId) -> Vec<String> {
        let mut out = Vec::new();
        for e in self.objects_in(id) {
            for (component, target) in super::file::object_refs(&self.world, e) {
                if self.contains(target) && self.scene_of(target) != id {
                    out.push(format!(
                        "'{}' ({component}) references '{}' in scene '{}'",
                        self.name(e),
                        self.name(target),
                        self.scene_name(self.scene_of(target)),
                    ));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::render_settings::SkyboxKind;

    fn level() -> String {
        let mut s = Scene::new("Level");
        let floor = s.spawn("Floor", None);
        s.spawn("Crate", Some(floor));
        s.render_settings.skybox.kind = SkyboxKind::SolidColor;
        s.to_json()
    }

    fn names(s: &Scene, list: &[Entity]) -> Vec<String> {
        list.iter().map(|e| s.name(*e)).collect()
    }

    #[test]
    fn additive_load_tags_objects_and_keeps_lighting_aside() {
        let mut s = Scene::default_3d("Main");
        let (id, roots, report) = s.load_additive("Assets/Level.scene", &level()).unwrap();
        assert!(report.warnings.is_empty());
        assert_eq!(names(&s, &roots), ["Floor"]);
        assert_eq!(s.loaded_scenes(), [0, id]);
        let crate_e = s.find_by_name("Crate").unwrap();
        assert_eq!(s.scene_of(crate_e), id, "children share their root's scene");
        assert_eq!(s.roots_in(0).len(), 2);
        assert_eq!(s.scene_name(id), "Level");
        // Lighting stays the active (first) scene's until Level is made active.
        assert_eq!(s.render_settings.skybox.kind, SkyboxKind::default());
        assert!(s.set_active_scene(id));
        assert_eq!(s.render_settings.skybox.kind, SkyboxKind::SolidColor);
        assert_eq!(s.name, "Level");
        assert_eq!(s.scene_name(0), "Main");
        // New objects go into the active scene.
        let e = s.spawn("New", None);
        assert_eq!(s.scene_of(e), id);
        assert!(s.load_additive("Assets/Level.scene", &level()).is_err());
    }

    #[test]
    fn each_scene_saves_only_its_objects() {
        let mut s = Scene::default_3d("Main");
        let main_json = s.scene_to_json(0);
        let (id, _, _) = s.load_additive("Assets/Level.scene", &level()).unwrap();
        assert_eq!(s.scene_to_json(0), main_json, "main is unchanged");
        assert_eq!(s.scene_to_json(id), level(), "Level saves byte-identical");
    }

    #[test]
    fn moving_objects_between_scenes_retags_children() {
        let mut s = Scene::default_3d("Main");
        let (id, roots, _) = s.load_additive("Assets/Level.scene", &level()).unwrap();
        let cam = s.roots()[0];
        assert!(s.move_to_scene(cam, id));
        assert_eq!(s.scene_of(cam), id);
        assert_eq!(names(&s, &s.roots_in(id)), ["Floor", "Main Camera"]);
        // Re-parenting into another scene's object moves it there too.
        let light = s.roots_in(0)[0];
        s.set_parent(light, Some(roots[0]), None, true);
        assert_eq!(s.scene_of(light), id);
        assert!(s.roots_in(0).is_empty());
    }

    #[test]
    fn reload_scene_replaces_only_that_scenes_objects() {
        let mut s = Scene::default_3d("Main");
        let (id, _, _) = s.load_additive("Assets/Level.scene", &level()).unwrap();
        // The file changed on disk: a new object and other lighting.
        let mut changed = Scene::new("Level");
        changed.spawn("Floor", None);
        changed.spawn("Barrel", None);
        let text = changed.to_json();
        let (roots, report) = s.reload_scene(id, &text).unwrap();
        assert!(report.warnings.is_empty());
        assert_eq!(names(&s, &roots), ["Floor", "Barrel"]);
        assert!(s.find_by_name("Crate").is_none());
        assert_eq!(s.roots_in(0).len(), 2, "Main keeps its objects");
        assert_eq!(s.scene_settings(id).0.skybox.kind, SkyboxKind::default());
        assert_eq!(s.scene_name(id), "Level");
        assert_eq!(s.scene_path(id).as_deref(), Some("Assets/Level.scene"));
        // The active scene's lighting is the world's.
        assert!(s.reload_scene(0, &level()).is_ok());
        assert_eq!(s.render_settings.skybox.kind, SkyboxKind::SolidColor);
        assert_eq!(s.name, "Main");
        assert!(s.reload_scene(id, "{ broken").is_err());
        assert!(
            s.find_by_name("Barrel").is_some(),
            "a broken file changes nothing"
        );
    }

    #[test]
    fn unload_keeps_the_entry_and_reload_restores_objects() {
        let mut s = Scene::default_3d("Main");
        let (id, _, _) = s.load_additive("Assets/Level.scene", &level()).unwrap();
        s.set_active_scene(id);
        assert!(s.unload_scene(0).is_ok());
        assert_eq!(s.active_scene(), id, "another loaded scene becomes active");
        assert!(s.find_by_name("Main Camera").is_none());
        assert_eq!(s.scenes().len(), 2);
        assert!(s.unload_scene(id).is_err(), "last loaded scene stays");
        // Removing / reloading.
        let back = s.add_unloaded("Assets/Main.scene");
        assert_eq!(back, s.find_by_path("Assets/Main.scene").unwrap());
        assert!(s.remove_scene(0).is_ok());
        assert_eq!(s.scenes().len(), 2);
    }

    #[test]
    fn cross_scene_references_are_reported_and_saved_as_null() {
        let mut s = Scene::default_3d("Main");
        let (id, roots, _) = s.load_additive("Assets/Level.scene", &level()).unwrap();
        let button = s.spawn("Hinge", None);
        s.add_by_name(button, "Rigidbody");
        s.add_by_name(button, "Hinge Joint");
        s.world
            .get::<&mut crate::scene::HingeJoint>(button)
            .unwrap()
            .connected_body = Some(roots[0]);
        let refs = s.cross_scene_references(0);
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert!(refs[0].contains("'Floor' in scene 'Level'"));
        assert!(s.cross_scene_references(id).is_empty());
        assert!(s.scene_to_json(0).contains("\"connected_body\": null"));
    }

    #[test]
    fn restored_copy_keeps_the_current_active_scene_and_paths() {
        let mut s = Scene::default_3d("Main");
        s.set_scene_path(0, "Assets/Main.scene");
        let (id, _, _) = s.load_additive("Assets/Level.scene", &level()).unwrap();
        let before = s.clone();
        // Not undoable: Level becomes active, Main is saved under a new name.
        assert!(s.set_active_scene(id));
        s.set_scene_path(0, "Assets/Main 2.scene");
        let mut restored = before;
        restored.keep_open_set_from(&s);
        assert_eq!(restored.active_scene(), id);
        assert_eq!(restored.name, "Level");
        assert_eq!(restored.render_settings.skybox.kind, SkyboxKind::SolidColor);
        assert_eq!(
            restored.scene_path(0).as_deref(),
            Some("Assets/Main 2.scene")
        );
        assert_eq!(restored.scene_name(0), "Main 2");
    }
}
