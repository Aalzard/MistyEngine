//! Prefab commands: create from the Hierarchy, instantiate, select, revert, apply, unpack.

use std::path::{Path, PathBuf};

use misty_engine::hecs::Entity;
use misty_engine::scene::prefab::{self, Prefab};

use crate::actions::Action;
use crate::state::EditorState;

/// Apply a prefab command. Returns the action back if it is not one.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::CreatePrefab { entity, folder } => create(s, entity, &folder),
        Action::InstantiatePrefab { path, parent } => {
            instantiate(s, &path, parent);
        }
        Action::SelectPrefabAsset(e) => {
            if let Some(rel) = s.scene.prefab_link(e) {
                select_asset(s, &rel);
            }
        }
        Action::RevertPrefab(e) => revert(s, e),
        Action::ApplyPrefab(e) => apply_all(s, e),
        Action::UnpackPrefab(e) => {
            if s.scene.prefab_link(e).is_some() {
                s.record("Unpack Prefab");
                s.scene.unlink_prefab(e);
            }
        }
        other => return Some(other),
    }
    s.selection.retain_existing(&s.scene);
    None
}

/// "Name.prefab" in `folder`, numbered if taken ("Name 1.prefab").
fn free_path(folder: &Path, name: &str) -> PathBuf {
    // Characters Windows does not allow in file names.
    let clean: String = name
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    (0..)
        .map(|n| {
            folder.join(if n == 0 {
                format!("{clean}.{}", prefab::EXTENSION)
            } else {
                format!("{clean} {n}.{}", prefab::EXTENSION)
            })
        })
        .find(|p| !p.exists())
        .expect("some name is free")
}

/// Save `e` and its children as a new prefab asset and turn `e` into an instance of it.
fn create(s: &mut EditorState, e: Entity, folder: &Path) {
    if !s.scene.contains(e) {
        return;
    }
    let path = free_path(folder, &s.scene.name(e));
    let rel = s.assets.relative(&path);
    if let Err(err) = Prefab::from_scene(&s.scene, e).save(&path) {
        s.console.warn(format!("Could not create prefab: {err}"));
        return;
    }
    s.record("Create Prefab");
    s.scene.link_prefab(e, &rel);
    s.project.refresh();
    s.console.info(format!("Created prefab {rel}"));
}

/// Instance of the asset at `rel`: under `parent`, or at the Scene view pivot. Undoable.
pub fn instantiate(s: &mut EditorState, rel: &str, parent: Option<Entity>) -> Option<Entity> {
    let prefab = match s.assets.prefab(rel) {
        Ok(p) => p,
        Err(err) => {
            s.console.warn(format!("Could not instantiate: {err}"));
            return None;
        }
    };
    let parent = parent.filter(|p| s.scene.contains(*p));
    s.record("Instantiate Prefab");
    let e = s.scene.instantiate_prefab(&prefab, Some(rel), parent);
    if parent.is_none() {
        let pivot = s.scene_view.camera.pivot().round();
        s.scene.set_world_position(e, pivot);
    }
    s.select(e);
    Some(e)
}

/// Show the asset in the Project window and the Inspector.
pub fn select_asset(s: &mut EditorState, rel: &str) {
    let abs = s.assets.absolute(rel);
    if let Some(dir) = abs.parent() {
        s.project.search.clear();
        s.project.open_folder(dir.to_path_buf());
    }
    s.project.selected = Some(abs);
    s.selection.clear();
    s.inspector_asset = Some(rel.to_owned());
}

fn revert(s: &mut EditorState, e: Entity) {
    let Some(rel) = s.scene.prefab_link(e) else {
        return;
    };
    match s.assets.prefab(&rel) {
        Ok(prefab) => {
            s.record("Revert Prefab");
            let new_root = s.scene.revert_prefab(e, &prefab);
            s.select(new_root);
        }
        Err(err) => s.console.warn(format!("Could not revert: {err}")),
    }
}

/// Write the instance back to its asset, then refresh the other instances in the scene.
fn apply_all(s: &mut EditorState, e: Entity) {
    let Some(rel) = s.scene.prefab_link(e) else {
        return;
    };
    let asset = match s.assets.prefab(&rel) {
        Ok(p) => p,
        Err(err) => {
            s.console.warn(format!("Could not apply: {err}"));
            return;
        }
    };
    let updated = s.scene.prefab_to_apply(e, &asset);
    if let Err(err) = s.assets.save_prefab(&rel, &updated) {
        s.console.warn(format!("Could not apply: {err}"));
        return;
    }
    let others: Vec<Entity> = s
        .scene
        .prefab_instances(&rel)
        .into_iter()
        .filter(|x| *x != e)
        .collect();
    if !others.is_empty() {
        s.record("Apply Prefab");
        for other in others {
            s.scene.revert_prefab(other, &updated);
        }
    }
    s.console.info(format!("Applied changes to {rel}"));
}

#[cfg(test)]
mod tests {
    use misty_engine::scene::BoxCollider;

    use super::*;
    use crate::project::ProjectBrowser;

    fn state(tag: &str) -> EditorState {
        let root =
            std::env::temp_dir().join(format!("misty_prefab_editor_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        EditorState::new(ProjectBrowser::open(root))
    }

    fn folder(s: &EditorState) -> PathBuf {
        s.project.assets_dir()
    }

    #[test]
    fn create_links_and_instantiate_is_undoable() {
        let mut s = state("create");
        let cube = s.scene.spawn("Cube", None);
        s.scene.spawn("Child", Some(cube));
        let dir = folder(&s);
        apply(
            &mut s,
            Action::CreatePrefab {
                entity: cube,
                folder: dir.clone(),
            },
        );
        assert!(dir.join("Cube.prefab").exists());
        assert_eq!(
            s.scene.prefab_link(cube).as_deref(),
            Some("Assets/Cube.prefab")
        );
        assert!(
            s.project.entries.iter().any(|e| e.name == "Cube.prefab"),
            "Project window shows the new asset"
        );

        // A second prefab from the same name gets a free file name.
        apply(
            &mut s,
            Action::CreatePrefab {
                entity: cube,
                folder: dir.clone(),
            },
        );
        assert!(dir.join("Cube 1.prefab").exists());

        let before = s.scene.len();
        let e = instantiate(&mut s, "Assets/Cube.prefab", None).unwrap();
        assert_eq!(s.scene.len(), before + 2);
        assert_eq!(
            s.scene.world_position(e),
            s.scene_view.camera.pivot().round(),
            "placed at the Scene view pivot"
        );
        assert_eq!(s.selection.active(), Some(e));
        s.undo
            .undo(&mut s.scene, &mut s.selection, &mut s.anim_clips);
        assert_eq!(s.scene.len(), before);
        assert!(instantiate(&mut s, "Assets/Nope.prefab", None).is_none());
    }

    #[test]
    fn apply_updates_asset_and_other_instances_revert_restores() {
        let mut s = state("apply");
        let crate_e = s.scene.spawn("Crate", None);
        let dir = folder(&s);
        apply(
            &mut s,
            Action::CreatePrefab {
                entity: crate_e,
                folder: dir,
            },
        );
        let rel = "Assets/Crate.prefab";
        instantiate(&mut s, rel, None).unwrap();

        s.scene.add(crate_e, BoxCollider::default());
        let prefab = s.assets.prefab(rel).unwrap();
        assert!(s.scene.has_prefab_overrides(crate_e, &prefab));
        apply(&mut s, Action::ApplyPrefab(crate_e));
        let prefab = s.assets.prefab(rel).unwrap();
        assert!(!s.scene.has_prefab_overrides(crate_e, &prefab));
        let other = s.scene.prefab_instances(rel)[1];
        assert!(
            s.scene
                .component_names(other)
                .contains(&"Box Collider".into()),
            "other instances pick up the change"
        );

        s.scene.remove_by_name(other, "Box Collider");
        apply(&mut s, Action::RevertPrefab(other));
        let other = s.selection.active().unwrap();
        assert!(
            s.scene
                .component_names(other)
                .contains(&"Box Collider".into())
        );

        apply(&mut s, Action::UnpackPrefab(other));
        assert_eq!(s.scene.prefab_link(other), None);
        s.undo
            .undo(&mut s.scene, &mut s.selection, &mut s.anim_clips);
        assert!(s.scene.prefab_link(other).is_some(), "unpack is undoable");
    }

    #[test]
    fn select_shows_the_asset() {
        let mut s = state("select");
        let e = s.scene.spawn("Thing", None);
        let sub = folder(&s).join("Prefabs");
        std::fs::create_dir_all(&sub).unwrap();
        apply(
            &mut s,
            Action::CreatePrefab {
                entity: e,
                folder: sub.clone(),
            },
        );
        apply(&mut s, Action::SelectPrefabAsset(e));
        assert_eq!(s.project.current, sub);
        assert_eq!(s.project.selected, Some(sub.join("Thing.prefab")));
        assert_eq!(
            s.inspector_asset.as_deref(),
            Some("Assets/Prefabs/Thing.prefab")
        );
        assert!(s.selection.is_empty());
    }
}
