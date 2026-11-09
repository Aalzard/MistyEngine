//! Files and folders dragged in from the OS file browser (Explorer, Finder, a desktop).
//!
//! Dropped on the Project window they are copied into the folder under the pointer (or the
//! open folder), folders with everything in them; the import then runs as for any new asset
//! (meta files and GUIDs, models' materials, scripts) and the window shows them. Dropped on the
//! Hierarchy or the Scene view they are imported into the open Project folder and the models
//! and prefabs among them are added to the scene. Dropped anywhere else, they go to the open
//! Project folder.
//!
//! While files hover over the window the target is outlined. (On Windows the pointer position
//! is not updated during an OS drag, so the panel and folder are the ones last under the
//! pointer; the open folder is used when that is unknown.)

use std::path::{Path, PathBuf};

use egui::{Color32, Id, Rect, Stroke, Ui};

use crate::actions::Action;
use crate::state::EditorState;

/// Outline colour of a drop target.
pub const HIGHLIGHT: Color32 = Color32::from_rgb(90, 150, 230);

/// Files hovering over the window now (not yet dropped).
pub fn hovering(ctx: &egui::Context) -> bool {
    ctx.input(|i| !i.raw.hovered_files.is_empty()) || preview()
}

/// `MISTY_DROP_PREVIEW=1` (screenshots): the Project window shows its drop outline as if files
/// hovered over it.
pub fn preview() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("MISTY_DROP_PREVIEW").is_some())
}

fn consumed_id() -> Id {
    Id::new("os_drop_consumed")
}

/// The paths dropped this frame, unless a panel already took them.
pub fn peek(ctx: &egui::Context) -> Vec<PathBuf> {
    let pass = ctx.cumulative_pass_nr();
    if ctx.data(|d| d.get_temp::<u64>(consumed_id())) == Some(pass) {
        return Vec::new();
    }
    ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    })
}

/// [`peek`], and no other panel sees them this frame.
pub fn take(ctx: &egui::Context) -> Vec<PathBuf> {
    let paths = peek(ctx);
    if !paths.is_empty() {
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(consumed_id(), pass));
    }
    paths
}

/// The pointer (last known position) is over `rect`.
pub fn pointer_in(ui: &Ui, rect: Rect) -> bool {
    ui.input(|i| i.pointer.latest_pos())
        .is_some_and(|p| rect.contains(p))
}

/// Outline `rect` as the drop target, with a caption at its top.
pub fn highlight(ui: &Ui, rect: Rect, caption: &str) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::new("os_drop_highlight"),
    ));
    painter.rect_filled(rect, 3.0, HIGHLIGHT.gamma_multiply(0.12));
    painter.rect_stroke(
        rect.shrink(1.0),
        3.0,
        Stroke::new(2.0, HIGHLIGHT),
        egui::StrokeKind::Inside,
    );
    if !caption.is_empty() {
        let galley = painter.layout_no_wrap(
            caption.to_owned(),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        let at = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 8.0);
        let bg = Rect::from_min_size(at, galley.size()).expand2(egui::vec2(6.0, 3.0));
        painter.rect_filled(bg, 4.0, HIGHLIGHT);
        painter.galley(at, galley, Color32::WHITE);
    }
}

/// Hierarchy and Scene view: outline while files hover over `rect`; dropped there, import them
/// into the open Project folder and add the models and prefabs among them to the scene.
pub fn scene_target(ui: &Ui, state: &mut EditorState, rect: Rect) {
    if !pointer_in(ui, rect) {
        return;
    }
    if hovering(ui.ctx()) {
        highlight(ui, rect, "Import and add to the scene");
    }
    if peek(ui.ctx()).is_empty() {
        return;
    }
    let paths = take(ui.ctx());
    state.push(Action::ImportFiles {
        paths,
        folder: state.project.current.clone(),
        instantiate: true,
    });
}

/// After the panels: files dropped where no panel took them go to the open Project folder.
pub fn fallback(ctx: &egui::Context, state: &mut EditorState) {
    let paths = take(ctx);
    if paths.is_empty() {
        return;
    }
    state.push(Action::ImportFiles {
        paths,
        folder: state.project.current.clone(),
        instantiate: false,
    });
    state.push(Action::FocusTab(crate::layout::Tab::Project));
}

/// Copy each of `sources` (files or folders, folders with everything in them) into `dest`
/// under a free name ("Rock 1.png" when "Rock.png" is there). A file's `.meta` comes along
/// when it keeps its name (its GUID and import settings; a GUID already used in the project
/// gets a new one on the next scan). Dropped `.meta` files themselves are skipped, and so is
/// an item already in `dest`. Returns the copies (and items already there), and the errors.
pub fn copy_into(dest: &Path, sources: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    let mut copied = Vec::new();
    let mut errors = Vec::new();
    for src in sources {
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned());
        let Some(name) = name else {
            errors.push(format!("{}: not a file or folder", src.display()));
            continue;
        };
        if misty_engine::asset_db::meta::is_meta(src) || name.starts_with('.') {
            continue;
        }
        if !src.exists() {
            errors.push(format!("{name}: not found"));
            continue;
        }
        if src.parent() == Some(dest) {
            copied.push(src.clone());
            continue;
        }
        if dest.starts_with(src) {
            errors.push(format!("{name}: a folder cannot be copied into itself"));
            continue;
        }
        let to = if src.is_dir() {
            crate::asset_ops::unique_path(dest, &name, "")
        } else {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.clone());
            let ext = src
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            crate::asset_ops::unique_path(dest, &stem, &ext)
        };
        match crate::asset_ops::copy_all(src, &to) {
            Ok(()) => {
                let meta = misty_engine::asset_db::meta::meta_path(src);
                if to.file_name() == src.file_name() && meta.is_file() {
                    let _ = std::fs::copy(&meta, misty_engine::asset_db::meta::meta_path(&to));
                }
                copied.push(to);
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    (copied, errors)
}

/// [`Action::ImportFiles`]: copy, import (a refresh: meta files, GUIDs, models' materials,
/// scripts), select the first new item when it is in the open folder, and with `instantiate`
/// add the models and prefabs to the scene.
pub fn import_into(s: &mut EditorState, paths: &[PathBuf], folder: &Path, instantiate: bool) {
    let assets = s.project.assets_dir();
    let folder = if folder.starts_with(&assets) && folder.is_dir() {
        folder.to_path_buf()
    } else {
        assets
    };
    let (copied, errors) = copy_into(&folder, paths);
    for e in errors {
        s.console.warn(format!("Could not import {e}"));
    }
    if copied.is_empty() {
        return;
    }
    s.refresh_assets();
    let rel_folder = s.assets.relative(&folder);
    s.console.info(format!(
        "Imported {} into {rel_folder}",
        if copied.len() == 1 {
            s.assets.relative(&copied[0])
        } else {
            format!("{} items", copied.len())
        }
    ));
    if copied[0].parent() == Some(s.project.current.as_path()) {
        s.project.selected = Some(copied[0].clone());
        s.selection.clear();
        s.inspector_asset = Some(s.assets.relative(&copied[0]));
    }
    if instantiate {
        for p in copied.iter().filter(|p| p.is_file()) {
            let rel = s.assets.relative(p);
            if rel.ends_with(".prefab") {
                s.push(Action::InstantiatePrefab {
                    path: rel,
                    parent: None,
                });
            } else if misty_engine::models::is_model(&rel) {
                s.push(Action::InstantiateModel {
                    path: rel,
                    parent: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("misty os drop {tag} {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn folders_copy_with_everything_in_them_under_free_names() {
        let dir = temp("copy");
        let desktop = dir.join("Desktop");
        std::fs::create_dir_all(desktop.join("Pack/Textures/Sub")).unwrap();
        std::fs::write(desktop.join("Pack/Textures/Rock.png"), "png").unwrap();
        std::fs::write(desktop.join("Pack/Textures/Sub/Moss.png"), "png").unwrap();
        std::fs::write(desktop.join("Pack/readme.txt"), "hi").unwrap();
        std::fs::write(desktop.join("Barrel.obj"), "o").unwrap();
        std::fs::write(desktop.join("Barrel.obj.meta"), "guid: x").unwrap();
        let dest = dir.join("Project/Assets/Models");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("Barrel.obj"), "old").unwrap();

        let sources = [
            desktop.join("Pack"),
            desktop.join("Barrel.obj"),
            desktop.join("Barrel.obj.meta"),
            desktop.join("Missing.png"),
        ];
        let (copied, errors) = copy_into(&dest, &sources);
        assert_eq!(copied, [dest.join("Pack"), dest.join("Barrel 1.obj")]);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(dest.join("Pack/Textures/Sub/Moss.png").is_file());
        assert!(dest.join("Pack/readme.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(dest.join("Barrel.obj")).unwrap(),
            "old"
        );
        // Renamed copies leave the meta behind (it names the other file).
        assert!(!dest.join("Barrel 1.obj.meta").exists());
        assert!(!dest.join("Barrel.obj.meta").exists());
        // Into itself: refused.
        let (_, errors) = copy_into(&dest.join("Pack/Textures"), &[dest.join("Pack")]);
        assert_eq!(errors.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
