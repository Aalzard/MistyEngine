//! Project window commands: create, rename, duplicate, move, delete (to the OS trash) and Show in
//! Explorer. Renames and moves update every path reference (see `asset_ops`), both on disk and
//! in the editor's memory (open scene, undo history, Play-mode snapshot, Build Settings, recent
//! scenes, selection, open animation clips).

use std::path::{Path, PathBuf};

use misty_engine::scene::{Scene, registry};
use misty_engine::shader_asset;

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
        Action::ReimportAsset(path) => crate::asset_guids::reimport(s, &path),
        Action::FindReferencesInScene(path) => crate::asset_guids::find_references(s, &path),
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
    // Registry packages are read-only.
    let selected = s
        .project
        .selected
        .clone()
        .filter(|p| !s.project.is_read_only(p) || matches!(action, Action::DuplicateSelected));
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
        AssetKind::Material => s.assets.create_material(&dir).map(|r| {
            crate::presets::apply_default_material(s, &r);
            s.assets.absolute(&r)
        }),
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
        AssetKind::Timeline => {
            let ext = misty_engine::timeline::EXTENSION;
            let path = asset_ops::unique_path(&dir, "New Timeline", ext);
            let rel = s.assets.relative(&path);
            s.anim_clips
                .timelines
                .create(&rel, Default::default())
                .map(|_| path)
        }
        AssetKind::Tile2D(k) => crate::tile_palette::create_asset(&dir, k),
        AssetKind::SurfaceShader | AssetKind::UnlitShader => {
            let (stem, text) = if kind == AssetKind::SurfaceShader {
                ("New Surface Shader", shader_asset::templates::SURFACE)
            } else {
                ("New Unlit Shader", shader_asset::templates::UNLIT)
            };
            let path = asset_ops::unique_path(&dir, stem, shader_asset::EXTENSION);
            std::fs::write(&path, text)
                .map(|_| path)
                .map_err(|e| e.to_string())
        }
        AssetKind::MaterialFromShader => material_from_shader(s, &dir),
        AssetKind::LitShaderGraph | AssetKind::UnlitShaderGraph => {
            use misty_engine::shader_graph::{EXTENSION, TargetMaterial};
            let (stem, material) = if kind == AssetKind::LitShaderGraph {
                ("New Lit Shader Graph", TargetMaterial::Lit)
            } else {
                ("New Unlit Shader Graph", TargetMaterial::Unlit)
            };
            let path = asset_ops::unique_path(&dir, stem, EXTENSION);
            crate::panels::shader_graph::new_graph(material)
                .save(&path)
                .map(|_| path)
        }
        AssetKind::EffectGraph => {
            let ext = misty_engine::vfx::EXTENSION;
            let path = asset_ops::unique_path(&dir, "New Effect Graph", ext);
            crate::panels::effect_graph::new_graph()
                .save(&path)
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
            // New scripts and shaders open in the Code Editor; the name field keeps the keyboard.
            if matches!(
                kind,
                AssetKind::Script | AssetKind::SurfaceShader | AssetKind::UnlitShader
            ) {
                let rel = s.assets.relative(&path);
                crate::panels::code_editor::open_script(s, &rel, None);
                s.code_editor.ui.focus_text = false;
            }
        }
        Err(e) => s
            .console
            .warn(format!("Could not create {}: {e}", kind.label())),
    }
}

/// A material next to the others in `dir` using the selected `.shader` asset.
fn material_from_shader(s: &mut EditorState, dir: &Path) -> Result<PathBuf, String> {
    let shader = s
        .project
        .selected
        .as_ref()
        .map(|p| s.assets.relative(p))
        .filter(|rel| shader_asset::is_shader_path(rel))
        .ok_or("select a .shader asset in the Project window first")?;
    let stem = Path::new(&shader)
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "New".into());
    let path = asset_ops::unique_path(dir, &format!("{stem} Material"), "mat");
    let m = misty_engine::assets::Material {
        shader,
        ..Default::default()
    };
    m.save(&path)?;
    Ok(path)
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
    // GUIDs follow first, so files saved with them resolve to the new path.
    misty_engine::asset_db::moved(&old, &new);
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
    // Cached prefabs may hold the old path (their files are only rewritten without GUIDs).
    s.assets.forget_prefabs();
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
    }
    s.files.settings.recent_scenes.iter_mut().for_each(&remap);
    s.files.settings.save(&s.project.root);
    crate::build_settings::load(s);
    s.anim_clips.rename(old, new);
    s.code_editor.rename(old, new);
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
    // Open scenes follow their own files too.
    scene.remap_scene_paths(|p| asset_ops::remap(p, old, new));
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
            misty_engine::asset_db::removed(&rel);
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

/// Open the OS file browser with `path` selected.
pub(crate) fn show_in_explorer(path: &Path) -> std::io::Result<()> {
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
