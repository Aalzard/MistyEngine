//! The preset button in component and asset headers, and the Select Preset window it opens:
//! None (back to the values from when it opened), the matching `.preset` assets, and
//! "Save current to…". Picking applies right away (one undo step for components); the window
//! stays open so presets can be compared, Escape or ✕ closes it.

use egui::{RichText, Ui};
use misty_engine::assets::Material;
use misty_engine::preset::{self, Preset};
use misty_engine::sprites::TextureImporter;
use serde_json::Value;

use crate::actions::Action;
use crate::presets::{Backup, PRESET_ICON, PickTarget, PresetPicker, preset_name};
use crate::state::EditorState;
use crate::theme;

/// The small preset icon; `current` is the asset's settings as JSON (unused for components).
pub fn button(ui: &mut Ui, state: &mut EditorState, target: PickTarget, current: Value) {
    let resp = ui
        .add(egui::Button::new(RichText::new(PRESET_ICON).color(theme::TEXT_DIM)).frame(false))
        .on_hover_text(format!("Presets for {}", target.kind()));
    let requested = state.preset_picker_request.as_deref() == Some(target.kind());
    if resp.clicked() || requested {
        state.preset_picker_request = None;
        open(state, target, current, resp.rect.left_bottom());
    }
}

/// Open the picker for `target` below `anchor`.
pub fn open(state: &mut EditorState, target: PickTarget, current: Value, anchor: egui::Pos2) {
    let backup = match &target {
        PickTarget::Component { name, targets } => {
            Backup::Component(crate::presets::snapshot(state, name, targets))
        }
        _ => Backup::Asset(current),
    };
    let save_name = crate::presets::suggested_name(target.kind());
    state.preset_picker = Some(PresetPicker {
        target,
        anchor,
        backup,
        chosen: None,
        save_name,
    });
}

/// What the user did in the window this frame.
enum Pick {
    None,
    Preset(String),
    Save,
}

/// Draw the picker window if it is open.
pub fn window(ctx: &egui::Context, state: &mut EditorState) {
    let Some(picker) = state.preset_picker.as_mut() else {
        return;
    };
    let kind = picker.target.kind().to_owned();
    let list = state.assets.presets_for(&kind);
    let mut open = true;
    let mut pick = None;
    egui::Window::new("Select Preset")
        .id(egui::Id::new("preset_picker"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(picker.anchor)
        .default_width(240.0)
        .show(ctx, |ui| {
            ui.label(RichText::new(format!("{kind} presets")).color(theme::TEXT_DIM));
            ui.add_space(2.0);
            if ui
                .selectable_label(picker.chosen.is_none(), "None")
                .on_hover_text("Keep the values from before the picker opened")
                .clicked()
            {
                pick = Some(Pick::None);
            }
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for rel in &list {
                        let on = picker.chosen.as_deref() == Some(rel.as_str());
                        let label = format!("{PRESET_ICON} {}", preset_name(rel));
                        if ui.selectable_label(on, label).on_hover_text(rel).clicked() {
                            pick = Some(Pick::Preset(rel.clone()));
                        }
                    }
                });
            if list.is_empty() {
                ui.weak(format!("No {kind} presets yet."));
            }
            ui.separator();
            ui.label("Save current to…");
            ui.horizontal(|ui| {
                let field = egui::TextEdit::singleline(&mut picker.save_name).desired_width(150.0);
                let enter =
                    ui.add(field).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Save").clicked() || enter {
                    pick = Some(Pick::Save);
                }
            });
            ui.weak(format!("in {}", preset::FOLDER));
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    if let Some(p) = pick {
        act(ctx, state, p);
    }
    if !open {
        state.preset_picker = None;
    }
}

fn act(ctx: &egui::Context, state: &mut EditorState, pick: Pick) {
    let Some(picker) = state.preset_picker.as_ref() else {
        return;
    };
    let target = picker.target.clone();
    match pick {
        Pick::Preset(rel) => {
            apply(ctx, state, &target, &rel);
            if let Some(p) = state.preset_picker.as_mut() {
                p.chosen = Some(rel);
            }
        }
        Pick::None => {
            if picker.chosen.is_some() {
                let backup = picker.backup.clone();
                restore(ctx, state, &target, backup);
            }
            if let Some(p) = state.preset_picker.as_mut() {
                p.chosen = None;
            }
        }
        Pick::Save => {
            let file = picker.save_name.clone();
            save(ctx, state, &target, &file);
        }
    }
}

/// Apply preset `rel` to the picker's target.
fn apply(ctx: &egui::Context, state: &mut EditorState, target: &PickTarget, rel: &str) {
    let Some(p) = state.assets.presets.get(rel).cloned() else {
        return;
    };
    match target {
        PickTarget::Component { targets, .. } => state.push(Action::ApplyPreset {
            entities: targets.clone(),
            preset: rel.to_owned(),
        }),
        PickTarget::Material(mat) => {
            let Some(m) = state.assets.materials.get(mat).cloned() else {
                return;
            };
            match p.apply_value(&m) {
                Ok(m) => {
                    let label = format!("Apply Preset {}", preset_name(rel));
                    save_material(state, mat, m, &label);
                }
                Err(e) => state.console.warn(format!("Preset {rel}: {e}")),
            }
        }
        PickTarget::Texture(tex) => {
            let now = super::texture_import::pending_settings(ctx, state, tex);
            match p.apply_value(&now) {
                Ok(s) => super::texture_import::set_pending_settings(ctx, state, tex, s),
                Err(e) => state.console.warn(format!("Preset {rel}: {e}")),
            }
        }
    }
}

/// None: put the values from when the picker opened back.
fn restore(ctx: &egui::Context, state: &mut EditorState, target: &PickTarget, backup: Backup) {
    match (target, backup) {
        (PickTarget::Component { name, .. }, Backup::Component(values)) => {
            state.push(Action::RestoreComponentValues {
                name: name.clone(),
                values,
            });
        }
        (PickTarget::Material(mat), Backup::Asset(v)) => {
            if let Ok(m) = serde_json::from_value::<Material>(v) {
                save_material(state, mat, m, "Revert Material Preset");
            }
        }
        (PickTarget::Texture(tex), Backup::Asset(v)) => {
            if let Ok(s) = serde_json::from_value::<TextureImporter>(v) {
                super::texture_import::set_pending_settings(ctx, state, tex, s);
            }
        }
        _ => {}
    }
}

/// "Save current to…": a new preset from the target's current values.
fn save(ctx: &egui::Context, state: &mut EditorState, target: &PickTarget, file: &str) {
    let preset = match target {
        PickTarget::Component { name, targets } => {
            if let Some(&e) = targets.first() {
                state.push(Action::SavePreset {
                    entity: e,
                    name: name.clone(),
                    file: file.to_owned(),
                });
            }
            return;
        }
        PickTarget::Material(mat) => match state.assets.materials.get(mat) {
            Some(m) => Preset::of_value(preset::MATERIAL, m),
            None => return,
        },
        PickTarget::Texture(tex) => {
            let s = super::texture_import::pending_settings(ctx, state, tex);
            let mut p = Preset::of_value(preset::TEXTURE_IMPORTER, &s);
            // Sprite slices belong to one image.
            p.set_included("sprites", false);
            p
        }
    };
    crate::presets::save_new(state, preset, file);
}

/// Write material `rel` back as one undo step named `label` (picking a preset, or None).
fn save_material(state: &mut EditorState, rel: &str, m: Material, label: &str) {
    if let Some(before) = state.assets.materials.get(rel).cloned() {
        let (scene, selection) = (&state.scene, &state.selection);
        state
            .undo
            .push_material(label, scene, selection, rel, &before);
    }
    if let Err(e) = state.assets.update_material(rel, m) {
        state.console.warn(format!("Could not save {rel}: {e}"));
    }
}
