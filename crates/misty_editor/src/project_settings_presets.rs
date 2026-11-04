//! Project Settings ▸ Preset Manager: per component type (and Material), the default preset
//! applied when one is added, created or Reset. Lists every type that has a preset in the
//! project, plus types with a default whose preset is gone (shown as missing).

use std::collections::{BTreeMap, BTreeSet};

use egui::Ui;
use misty_engine::preset::{Preset, TEXTURE_IMPORTER};
use misty_engine::settings::ProjectSettings;

use crate::presets::preset_name;

/// Draw the page; true when a default changed.
pub fn page(ui: &mut Ui, p: &mut ProjectSettings, presets: &BTreeMap<String, Preset>) -> bool {
    // Texture import settings have no "new asset" moment to apply a default to.
    let mut kinds: BTreeSet<String> = presets
        .values()
        .map(|p| p.target.clone())
        .filter(|t| t != TEXTURE_IMPORTER)
        .collect();
    kinds.extend(p.default_presets.keys().cloned());
    if kinds.is_empty() {
        ui.weak("No presets in the project yet. Save one with the preset button in a component's header.");
        return false;
    }
    let mut edited = false;
    egui::Grid::new("preset_manager")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Type");
            ui.strong("Default Preset");
            ui.end_row();
            for kind in &kinds {
                ui.label(kind);
                edited |= default_combo(ui, p, presets, kind);
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.weak("Unticked fields of a preset are not applied; a Light's type comes from the Create menu.");
    edited
}

fn default_combo(
    ui: &mut Ui,
    p: &mut ProjectSettings,
    presets: &BTreeMap<String, Preset>,
    kind: &str,
) -> bool {
    let current = p.default_presets.get(kind).cloned();
    let text = match &current {
        None => "None".to_owned(),
        Some(rel) if presets.get(rel).is_some_and(|x| x.target == kind) => preset_name(rel),
        Some(rel) => format!("{} (missing)", preset_name(rel)),
    };
    let mut pick: Option<Option<String>> = None;
    egui::ComboBox::from_id_salt(("preset_default", kind))
        .width(200.0)
        .selected_text(text)
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "None").clicked() {
                pick = Some(None);
            }
            for (rel, _) in presets.iter().filter(|(_, x)| x.target == kind) {
                let on = current.as_deref() == Some(rel.as_str());
                if ui
                    .selectable_label(on, preset_name(rel))
                    .on_hover_text(rel)
                    .clicked()
                {
                    pick = Some(Some(rel.clone()));
                }
            }
        });
    match pick {
        Some(choice) if choice != current => {
            match choice {
                Some(rel) => p.default_presets.insert(kind.to_owned(), rel),
                None => p.default_presets.remove(kind),
            };
            true
        }
        _ => false,
    }
}
