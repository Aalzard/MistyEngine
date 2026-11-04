//! Project window context menus, the inline rename field and the Delete confirmation.

use std::path::{Path, PathBuf};

use egui::{Rect, Ui};

use crate::actions::{Action, AssetKind};
use crate::project::ProjectBrowser;

/// What a menu or field asked for this frame (applied by the Project window).
#[derive(Default)]
pub struct Requests {
    pub actions: Vec<Action>,
    pub rename: Option<PathBuf>,
    pub delete: Option<PathBuf>,
    pub copy_path: Option<String>,
}

/// Create ▸ entries (Project "+" button, Assets ▸ Create and the context menus).
pub fn create_items(ui: &mut Ui, actions: &mut Vec<Action>) {
    for kind in AssetKind::ALL {
        if ui.button(kind.label()).clicked() {
            actions.push(Action::CreateAsset(kind));
            ui.close();
        }
        if kind == AssetKind::Folder {
            ui.separator();
        }
        if kind == AssetKind::Material {
            create_shader_items(ui, actions);
        }
    }
}

/// Shader ▸ Surface / Unlit Shader, and Material from Shader.
fn create_shader_items(ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.menu_button("Shader", |ui| {
        for kind in [AssetKind::SurfaceShader, AssetKind::UnlitShader] {
            if ui.button(kind.label()).clicked() {
                actions.push(Action::CreateAsset(kind));
                ui.close();
            }
        }
    });
    if ui
        .button(AssetKind::MaterialFromShader.label())
        .on_hover_text("A new material using the shader selected in the Project window")
        .clicked()
    {
        actions.push(Action::CreateAsset(AssetKind::MaterialFromShader));
        ui.close();
    }
}

/// Right-click menu of an asset or folder.
pub fn item_menu(ui: &mut Ui, path: &Path, rel: Option<String>, req: &mut Requests) {
    ui.menu_button("Create", |ui| create_items(ui, &mut req.actions));
    let p = path.to_path_buf();
    if ui.button(explorer_label()).clicked() {
        req.actions.push(Action::ShowInExplorer(p.clone()));
        ui.close();
    }
    ui.separator();
    if ui.button("Rename").clicked() {
        req.rename = Some(p.clone());
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        req.actions.push(Action::DuplicateAsset(p.clone()));
        ui.close();
    }
    if ui.button("Delete").clicked() {
        req.delete = Some(p);
        ui.close();
    }
    ui.separator();
    if let Some(rel) = rel
        && ui.button("Copy Path").clicked()
    {
        req.copy_path = Some(rel);
        ui.close();
    }
    if ui.button("Refresh").clicked() {
        req.actions.push(Action::Refresh);
        ui.close();
    }
}

/// Right-click menu of the empty area of a folder.
pub fn folder_menu(ui: &mut Ui, folder: &Path, req: &mut Requests) {
    ui.menu_button("Create", |ui| create_items(ui, &mut req.actions));
    if ui.button(explorer_label()).clicked() {
        req.actions
            .push(Action::ShowInExplorer(folder.to_path_buf()));
        ui.close();
    }
    if ui.button("Refresh").clicked() {
        req.actions.push(Action::Refresh);
        ui.close();
    }
}

fn explorer_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Show in Explorer"
    }
}

/// Inline name field over `rect` while `browser.renaming` is this item: Enter or clicking away
/// renames, Escape cancels.
pub fn rename_field(ui: &mut Ui, rect: Rect, browser: &mut ProjectBrowser, req: &mut Requests) {
    let Some((path, text)) = browser.renaming.as_mut() else {
        return;
    };
    let edit = ui.put(
        rect,
        egui::TextEdit::singleline(text).margin(egui::vec2(2.0, 0.0)),
    );
    if !edit.has_focus() && !edit.lost_focus() {
        edit.request_focus();
    }
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if escape {
        browser.renaming = None;
    } else if edit.lost_focus() || enter {
        let name = text.trim().to_owned();
        let path = path.clone();
        browser.renaming = None;
        let current = if path.is_dir() {
            path.file_name()
        } else {
            path.file_stem()
        };
        if current.is_none_or(|c| c.to_string_lossy() != name) {
            req.actions.push(Action::RenameAsset { path, name });
        }
    }
}

/// "Delete selected asset?" dialog; Delete moves the asset to the OS trash.
pub fn delete_dialog(
    ctx: &egui::Context,
    browser: &mut ProjectBrowser,
    rel: &str,
) -> Option<Action> {
    let path = browser.confirm_delete.clone()?;
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("project_delete_confirm")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading("Delete selected asset?");
        ui.label(rel);
        ui.add_space(4.0);
        ui.weak("It will be moved to the Recycle Bin / Trash.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    if modal.should_close() && answer.is_none() {
        answer = Some(false);
    }
    match answer {
        Some(yes) => {
            browser.confirm_delete = None;
            yes.then_some(Action::DeleteAsset(path))
        }
        None => None,
    }
}
