//! Editor side of asset GUIDs: the Inspector's GUID row and the Project window's Reimport and
//! Find References In Scene commands. The GUID map itself is `misty_engine::asset_db`.

use std::path::Path;

use egui::{RichText, Ui};
use misty_engine::asset_db::{self, Guid, meta, refs};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Scene, registry};

use crate::state::EditorState;
use crate::theme;

/// GUID of a project asset: the open database's, else its meta file's.
pub fn guid(rel: &str) -> Option<Guid> {
    asset_db::guid_of(rel).or_else(|| meta::read_guid(&misty_engine::assets::absolute(rel)))
}

/// An asset Inspector's path line, with the GUID under it.
pub fn path_label(ui: &mut Ui, rel: &str) {
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
    guid_row(ui, rel);
}

/// Small read-only "GUID 0123…" row with a copy button.
pub fn guid_row(ui: &mut Ui, rel: &str) {
    let Some(g) = guid(rel) else {
        return;
    };
    let text = g.to_string();
    ui.horizontal(|ui| {
        ui.label(RichText::new("GUID").small().color(theme::TEXT_DIM));
        ui.add(
            egui::Label::new(
                RichText::new(&text)
                    .small()
                    .monospace()
                    .color(theme::TEXT_DIM),
            )
            .selectable(true),
        );
        if ui
            .small_button("⎘")
            .on_hover_text("Copy the GUID")
            .clicked()
        {
            ui.ctx().copy_text(text.clone());
        }
    });
}

/// Assets ▸ Reimport: mark the file changed so every importer and cache loads it again.
pub fn reimport(s: &mut EditorState, path: &Path) {
    let rel = s.assets.relative(path);
    let touched = std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(std::time::SystemTime::now()));
    match touched {
        Ok(()) => {
            s.assets.forget_prefabs();
            s.refresh_assets();
            s.console.info(format!("Reimported {rel}"));
        }
        Err(e) => s.console.warn(format!("Could not reimport {rel}: {e}")),
    }
}

/// Select the open scene's objects whose components reference the asset at `path`.
pub fn find_references(s: &mut EditorState, path: &Path) {
    let rel = s.assets.relative(path);
    let found = references(&s.scene, &rel);
    match found.first() {
        None => s
            .console
            .info(format!("No objects in the scene reference {rel}")),
        Some(&first) => {
            s.console
                .info(format!("{} object(s) reference {rel}", found.len()));
            s.selection.set_many(found);
            s.reveal(first);
        }
    }
}

/// Objects (Hierarchy order) with a component naming `rel` (or one of its sub-assets).
pub fn references(scene: &Scene, rel: &str) -> Vec<Entity> {
    let names = |s: &str| s == rel || s.strip_prefix(rel).is_some_and(|r| r.starts_with('#'));
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| {
            registry::components().iter().any(|info| {
                (info.to_json)(&scene.world, *e).is_some_and(|v| {
                    let mut strings = Vec::new();
                    refs::strings(&v, &mut strings);
                    strings.into_iter().any(names)
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use misty_engine::scene::MeshRenderer;

    #[test]
    fn finds_objects_using_an_asset() {
        let mut scene = Scene::new("Refs");
        let mut add = |name: &str, mat: &str| {
            let e = scene.spawn(name, None);
            let r = MeshRenderer {
                material: Some(mat.into()),
                ..Default::default()
            };
            scene.add(e, r);
            e
        };
        let red = add("Red", "Assets/Red.mat");
        add("Other", "Assets/Red.mat.bak");
        assert_eq!(references(&scene, "Assets/Red.mat"), [red]);
        assert!(references(&scene, "Assets/Green.mat").is_empty());
    }
}
