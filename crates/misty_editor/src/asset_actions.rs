//! Project window commands: create, rename, duplicate, move, delete (to the OS trash) and Show in
//! Explorer. Renames and moves update every path reference (see `asset_ops`), both on disk and
//! in the editor's memory (open scene, undo history, Play-mode snapshot, Build Settings, recent
//! scenes, selection, open animation clips).

use std::path::{Path, PathBuf};

use misty_engine::scene::{Scene, registry};

use crate::actions::{Action, AssetKind};
use crate::asset_ops;
use crate::state::EditorState;

/// Apply a Project window command. Returns the action back if it is not one.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::CreateAsset(kind) => create(s, kind),
        Action::RenameAsset { path, name } => {
            let result = asset_ops::rename(&path, &name);
            finish_move(s, &path, result, "rename");
        }
        Action::MoveAsset { path, folder } => {
            let result = asset_ops::move_into(&path, &folder);
            finish_move(s, &path, result, "move");
        }
        Action::DuplicateAsset(path) => match asset_ops::duplicate(&path) {
            Ok(copy) => {
                s.refresh_assets();
                select(s, &copy);
                let rel = s.assets.relative(&copy);
                s.console.info(format!("Duplicated {rel}"));
            }
            Err(e) => s.console.warn(format!("Could not duplicate: {e}")),
        },
        Action::DeleteAsset(path) => delete(s, &path),
        Action::Refresh => s.refresh_assets(),
        Action::ShowInExplorer(path) => {
            if let Err(e) = show_in_explorer(&path) {
                s.console
                    .warn(format!("Could not show {}: {e}", path.display()));
            }
        }
        other => return Some(other),
    }
    None
}

/// With the Project window focused, Duplicate / Rename / Delete act on the selected asset.
pub fn redirect(s: &mut EditorState, action: Action) -> Option<Action> {
    if !s.project.focused {
        return Some(action);
    }
    let selected = s.project.selected.clone();
    match (action, selected) {
        (Action::DuplicateSelected, Some(p)) => Some(Action::DuplicateAsset(p)),
        (Action::RenameSelected, Some(p)) => {
            s.project.start_rename(p);
            None
        }
        (Action::DeleteSelected, Some(p)) => {
            s.project.confirm_delete = Some(p);
            None
        }
        (Action::DuplicateSelected | Action::RenameSelected | Action::DeleteSelected, None) => None,
        (other, _) => Some(other),
    }
}

/// Assets ▸ Create: make the asset in the current folder, select it and start renaming it.
fn create(s: &mut EditorState, kind: AssetKind) {
    let dir = s.project.current.clone();
    let made: Result<PathBuf, String> = match kind {
        AssetKind::Folder => {
            let path = asset_ops::unique_path(&dir, "New Folder", "");
            std::fs::create_dir(&path)
                .map(|_| path)
                .map_err(|e| e.to_string())
        }
        AssetKind::Material => s
            .assets
            .create_material(&dir)
            .map(|r| s.assets.absolute(&r)),
        AssetKind::Script => s.assets.create_script(&dir).map(|r| s.assets.absolute(&r)),
        AssetKind::Scene => {
            crate::scene_files::create_scene_asset(s, &dir).map(|r| s.assets.absolute(&r))
        }
        AssetKind::Animation => {
            let path = asset_ops::unique_path(&dir, "New Animation", "anim");
            let rel = s.assets.relative(&path);
            s.anim_clips.create(&rel, Default::default()).map(|_| path)
        }
        AssetKind::AnimatorController => {
            let path = asset_ops::unique_path(&dir, "New Animator Controller", "controller");
            let rel = s.assets.relative(&path);
            s.anim_clips
                .controllers
                .create(&rel, Default::default())
                .map(|_| path)
        }
        AssetKind::AudioMixer => {
            let path = asset_ops::unique_path(&dir, "New Audio Mixer", "mixer");
            let rel = s.assets.relative(&path);
            s.anim_clips
                .mixers
                .create(&rel, Default::default())
                .map(|_| path)
        }
    };
    match made {
        Ok(path) => {
            s.refresh_assets();
            select(s, &path);
            s.project.start_rename(path.clone());
            s.console
                .info(format!("Created {}", s.assets.relative(&path)));
        }
        Err(e) => s
            .console
            .warn(format!("Could not create {}: {e}", kind.label())),
    }
}

fn select(s: &mut EditorState, path: &Path) {
    s.project.selected = Some(path.to_path_buf());
    s.selection.clear();
    s.inspector_asset = Some(s.assets.relative(path));
}

/// After a rename or move: update references, then the editor's own state.
fn finish_move(s: &mut EditorState, from: &Path, result: Result<PathBuf, String>, what: &str) {
    s.project.renaming = None;
    let to = match result {
        Ok(to) => to,
        Err(e) => {
            s.console
                .warn(format!("Could not {what} {}: {e}", from.display()));
            return;
        }
    };
    if to == from {
        return;
    }
    let (old, new) = (s.assets.relative(from), s.assets.relative(&to));
    match asset_ops::rewrite_references(&s.project.root, &old, &new) {
        Ok(files) if !files.is_empty() => s.console.info(format!(
            "Updated references to {old} in {}",
            files.join(", ")
        )),
        Ok(_) => {}
        Err(e) => s.console.warn(format!("Could not update references: {e}")),
    }
    update_memory(s, &old, &new, from, &to);
    s.refresh_assets();
    s.console.info(format!("{old} → {new}"));
}

/// Rewrite paths held in memory after `old` became `new` (absolute: `from` → `to`).
fn update_memory(s: &mut EditorState, old: &str, new: &str, from: &Path, to: &Path) {
    let remap = |p: &mut String| {
        if let Some(n) = asset_ops::remap(p, old, new) {
            *p = n;
        }
    };
    rewrite_scene(&mut s.scene, old, new);
    for scene in s.undo.scenes_mut() {
        rewrite_scene(scene, old, new);
    }
    if let Some(session) = s.play_session.as_mut() {
        for scene in session.scenes_mut() {
            rewrite_scene(scene, old, new);
        }
        if let Some(p) = session.path_mut() {
            remap(p);
        }
    }
    // The open scene's name follows its file name.
    if let Some(p) = s.files.path.as_mut()
        && let Some(n) = asset_ops::remap(p, old, new)
    {
        *p = n;
        s.scene.name = stem(&to.to_string_lossy());
    }
    s.files.settings.recent_scenes.iter_mut().for_each(&remap);
    s.files.settings.save(&s.project.root);
    crate::build_settings::load(s);
    s.anim_clips.rename(old, new);
    if let Some(c) = s.animation.clip.as_mut() {
        remap(c);
    }
    if let Some(a) = s.inspector_asset.as_mut() {
        remap(a);
    }
    let remap_abs = |p: &mut PathBuf| {
        if let Ok(rest) = p.strip_prefix(from) {
            *p = to.join(rest);
        }
    };
    if let Some(p) = s.project.selected.as_mut() {
        remap_abs(p);
    }
    remap_abs(&mut s.project.current);
}

/// Rewrite asset paths in every registered component of `scene`. Returns true if any changed.
pub fn rewrite_scene(scene: &mut Scene, old: &str, new: &str) -> bool {
    let mut changed = false;
    for (e, _) in scene.walk() {
        for info in registry::components() {
            if matches!(info.name, "Node" | "GameObject") {
                continue;
            }
            if let Some(mut v) = (info.to_json)(&scene.world, e)
                && asset_ops::rewrite_value(&mut v, old, new)
            {
                let _ = (info.from_json)(&mut scene.world, e, v);
                changed = true;
            }
        }
    }
    changed
}

fn delete(s: &mut EditorState, path: &Path) {
    let rel = s.assets.relative(path);
    match asset_ops::delete_to_trash(&[path.to_path_buf()]) {
        Ok(()) => {
            let under = |p: &Path| p.starts_with(path);
            if s.project.selected.as_deref().is_some_and(under) {
                s.project.selected = None;
            }
            if s.inspector_asset
                .as_deref()
                .is_some_and(|a| a == rel || a.starts_with(&format!("{rel}/")))
            {
                s.inspector_asset = None;
            }
            if under(&s.project.current) {
                s.project.current = path.parent().unwrap_or(path).to_path_buf();
            }
            s.refresh_assets();
            s.console.info(format!("Moved {rel} to the Recycle Bin."));
        }
        Err(e) => s.console.warn(format!("Could not delete {rel}: {e}")),
    }
}

fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Open the OS file browser with `path` selected.
fn show_in_explorer(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(format!(
            "/select,{}",
            path.display().to_string().replace('/', "\\")
        ));
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(path);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path.parent().unwrap_or(path));
        c
    };
    cmd.spawn().map(|_| ())
}
