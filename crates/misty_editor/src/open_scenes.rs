//! The set of open scenes in the editor: open additively, unload / reload, remove, set the
//! active scene, move objects between scenes, and the open set remembered per project.
//!
//! Loading or unloading a scene is not an undoable edit: the undo history is cleared, like
//! opening a scene (an undo snapshot would otherwise bring back a scene's objects behind the
//! Hierarchy's back).

use misty_engine::hecs::Entity;
use misty_engine::scene::SceneId;

use crate::actions::SceneOp;
use crate::console::LogLevel;
use crate::scene_files::{self, AfterPrompt, OpenSceneRecord, mark_saved};
use crate::state::EditorState;

fn read(s: &EditorState, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(s.project.root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

/// After the open set changed: drop stale selection and history, remember the set.
fn after_change(s: &mut EditorState) {
    s.selection.retain_existing(&s.scene);
    let scene = &s.scene;
    s.inspector_targets.retain(|e| scene.contains(*e));
    s.undo.clear();
    s.files.dirty = scene_files::dirty_now(s);
    remember_open_set(s);
}

/// Project window ▸ Open Scene Additive / a scene dropped on the Hierarchy. Returns the scene.
pub fn open_additive(s: &mut EditorState, rel: &str) -> Option<SceneId> {
    if s.play.in_play_mode() {
        s.console.warn("Stop Play mode first.");
        return None;
    }
    if let Some(id) = s.scene.find_loaded(rel) {
        s.console
            .warn(format!("{} is already open.", s.scene.scene_name(id)));
        return Some(id);
    }
    let loaded = read(s, rel).and_then(|text| s.scene.load_additive(rel, &text));
    match loaded {
        Ok((id, _, report)) => {
            for w in report.warnings {
                s.console.warn(format!("{rel}: {w}"));
            }
            mark_saved(s, id);
            s.hierarchy.collapsed_scenes.remove(&id);
            scene_files::remember(s, rel);
            after_change(s);
            s.console.info(format!("Opened {rel} (additive)"));
            Some(id)
        }
        Err(e) => {
            s.console
                .log(LogLevel::Error, format!("Could not open scene: {e}"));
            None
        }
    }
}

/// Load an unloaded scene of the open set again.
pub fn reload(s: &mut EditorState, id: SceneId) {
    if let Some(rel) = s.scene.scene_path(id) {
        open_additive(s, &rel);
    }
}

/// Unload a scene (it stays listed, greyed out). Ask first through `scene_files::guard`.
pub fn unload(s: &mut EditorState, id: SceneId) {
    match s.scene.unload_scene(id) {
        Ok(()) => {
            s.files.saved.remove(&id);
            after_change(s);
        }
        Err(e) => s.console.warn(format!("Can't unload the scene: {e}.")),
    }
}

/// Unload a scene and take it out of the Hierarchy.
pub fn remove(s: &mut EditorState, id: SceneId) {
    match s.scene.remove_scene(id) {
        Ok(()) => {
            s.files.saved.remove(&id);
            after_change(s);
        }
        Err(e) => s.console.warn(format!("Can't remove the scene: {e}.")),
    }
}

/// Keep only `id` open (loading it if needed) and make it active.
pub fn close_others(s: &mut EditorState, id: SceneId) {
    if !s.scene.entry(id).is_some_and(|e| e.loaded) {
        reload(s, id);
    }
    if !s.scene.set_active_scene(id) {
        return;
    }
    let others: Vec<SceneId> = s
        .scene
        .scenes()
        .iter()
        .map(|e| e.id)
        .filter(|o| *o != id)
        .collect();
    for o in others {
        let _ = s.scene.remove_scene(o);
        s.files.saved.remove(&o);
    }
    after_change(s);
}

/// New objects go into the active scene; its lighting is used.
pub fn set_active(s: &mut EditorState, id: SceneId) {
    if s.scene.set_active_scene(id) {
        remember_open_set(s);
    }
}

/// Objects dragged onto a scene header: move them (top level) into that scene.
pub fn move_objects(s: &mut EditorState, objects: &[Entity], id: SceneId) {
    let moving: Vec<Entity> = objects
        .iter()
        .copied()
        .filter(|e| s.scene.contains(*e) && s.scene.scene_of(*e) != id)
        .collect();
    if moving.is_empty() || !s.scene.entry(id).is_some_and(|e| e.loaded) {
        return;
    }
    let who = match moving.as_slice() {
        [one] => s.scene.name(*one),
        many => format!("{} Objects", many.len()),
    };
    let to = s.scene.scene_name(id);
    s.record(&format!("Move {who} To {to}"));
    for e in moving {
        s.scene.move_to_scene(e, id);
    }
    // References between scenes are not allowed (they would be lost on save).
    for scene in s.scene.loaded_scenes() {
        for w in s.scene.cross_scene_references(scene) {
            s.console.warn(format!(
                "Cross-scene references aren't supported and won't be saved: {w}"
            ));
        }
    }
}

/// Carry out a scene header command (unload / remove / close others ask about changes).
pub fn apply_op(s: &mut EditorState, id: SceneId, op: SceneOp) {
    let guarded = |s: &mut EditorState, then| {
        if let Some(a) = scene_files::guard(s, then) {
            scene_files::perform(s, a);
        }
    };
    match op {
        SceneOp::Save => {
            scene_files::save_scene(s, id);
        }
        SceneOp::SaveAs => {
            scene_files::save_scene_as(s, id);
        }
        SceneOp::SetActive => set_active(s, id),
        SceneOp::Unload => guarded(s, AfterPrompt::Unload(id)),
        SceneOp::Load => reload(s, id),
        SceneOp::Remove => guarded(s, AfterPrompt::Remove(id)),
        SceneOp::CloseOthers => guarded(s, AfterPrompt::CloseOthers(id)),
        SceneOp::SelectAll => {
            let objects = s.scene.objects_in(id);
            s.selection.set_many(objects);
        }
    }
}

/// Write the open set (saved scenes only) to the project's editor settings.
pub fn remember_open_set(s: &mut EditorState) {
    let scene = &s.scene;
    let set = &mut s.files.settings;
    set.open_scenes = scene
        .scenes()
        .iter()
        .filter_map(|e| {
            Some(OpenSceneRecord {
                path: e.path.clone()?,
                loaded: e.loaded,
            })
        })
        .collect();
    set.active_scene = scene.active_path();
    set.save(&s.project.root);
}

/// Reopen the remembered open set. Returns false when there is none (or none of it loads).
pub fn restore_open_set(s: &mut EditorState) -> bool {
    let set = s.files.settings.open_scenes.clone();
    let active = s.files.settings.active_scene.clone();
    let root = s.project.root.clone();
    let set: Vec<OpenSceneRecord> = set
        .into_iter()
        .filter(|r| root.join(&r.path).is_file())
        .collect();
    let Some(first) = set.iter().find(|r| r.loaded) else {
        return false;
    };
    if !scene_files::load(s, &first.path) {
        return false;
    }
    for r in set.iter().filter(|r| r.path != first.path) {
        if r.loaded {
            open_additive(s, &r.path);
        } else {
            s.scene.add_unloaded(&r.path);
        }
    }
    if let Some(id) = active.and_then(|a| s.scene.find_loaded(&a)) {
        s.scene.set_active_scene(id);
    }
    remember_open_set(s);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;
    use crate::scene_files::{guard, perform};
    use misty_engine::scene::Scene;

    /// A project with two saved scenes, "A" open.
    fn state(name: &str) -> EditorState {
        let root =
            std::env::temp_dir().join(format!("misty_open_scenes_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let scenes = root.join("Assets/Scenes");
        Scene::default_3d("A")
            .save(&scenes.join("A.scene"))
            .unwrap();
        let mut b = Scene::new("B");
        b.spawn("Ground", None);
        b.save(&scenes.join("B.scene")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root));
        assert!(scene_files::load(&mut s, "Assets/Scenes/A.scene"));
        s
    }

    #[test]
    fn additive_open_marks_only_the_edited_scene_dirty() {
        let mut s = state("dirty");
        let b = open_additive(&mut s, "Assets/Scenes/B.scene").unwrap();
        assert_eq!(s.scene.loaded_scenes(), [0, b]);
        assert!(scene_files::dirty_now(&s).is_empty());
        // An edit inside B only.
        let ground = s.scene.find_by_name("Ground").unwrap();
        s.record("Rename Ground");
        s.scene.game_object_mut(ground).name = "Floor".into();
        assert_eq!(scene_files::dirty_now(&s), [b]);
        // Unloading B asks first; A alone can be unloaded only while B is loaded.
        assert_eq!(guard(&mut s, AfterPrompt::Unload(b)), None);
        assert_eq!(
            guard(&mut s, AfterPrompt::Unload(0)),
            Some(AfterPrompt::Unload(0))
        );
        // File ▸ Save saves just B.
        assert!(scene_files::save(&mut s));
        assert!(!s.scene_dirty);
        let text = std::fs::read_to_string(s.project.root.join("Assets/Scenes/B.scene")).unwrap();
        assert!(text.contains("Floor"));
    }

    #[test]
    fn unload_reload_and_close_others() {
        let mut s = state("unload");
        let b = open_additive(&mut s, "Assets/Scenes/B.scene").unwrap();
        perform(&mut s, AfterPrompt::Unload(b));
        assert!(!s.scene.entry(b).unwrap().loaded);
        assert!(s.scene.find_by_name("Ground").is_none());
        reload(&mut s, b);
        assert!(s.scene.entry(b).unwrap().loaded);
        assert!(s.scene.find_by_name("Ground").is_some());
        close_others(&mut s, b);
        assert_eq!(s.scene.scenes().len(), 1);
        assert_eq!(s.scene.name, "B");
    }

    #[test]
    fn open_set_is_restored_on_the_next_launch() {
        let mut s = state("restore");
        let b = open_additive(&mut s, "Assets/Scenes/B.scene").unwrap();
        set_active(&mut s, b);
        let root = s.project.root.clone();
        let mut again = EditorState::new(ProjectBrowser::open(root));
        scene_files::open_initial(&mut again);
        let names: Vec<String> = again
            .scene
            .scenes()
            .iter()
            .map(|e| again.scene.scene_name(e.id))
            .collect();
        assert_eq!(names, ["A", "B"]);
        assert_eq!(again.scene.name, "B", "B is active again");
    }

    #[test]
    fn moving_objects_between_scenes_is_undoable() {
        let mut s = state("move");
        let b = open_additive(&mut s, "Assets/Scenes/B.scene").unwrap();
        let cam = s.scene.find_by_name("Main Camera").unwrap();
        move_objects(&mut s, &[cam], b);
        assert_eq!(s.scene.scene_of(cam), b);
        assert_eq!(s.undo.undo_label(), Some("Move Main Camera To B"));
        assert_eq!(s.undo_step().as_deref(), Some("Move Main Camera To B"));
        assert_eq!(s.scene.scene_of(cam), 0);
        assert!(s.redo_step().is_some());
        assert_eq!(s.scene.scene_of(cam), b);
        // Set Active Scene is not a step: undoing the move keeps B active.
        set_active(&mut s, b);
        assert!(s.undo_step().is_some());
        assert_eq!(s.scene.active_scene(), b);
        assert_eq!(s.scene.scene_of(cam), 0);
    }
}
