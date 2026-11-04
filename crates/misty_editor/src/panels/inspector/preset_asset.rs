//! Inspector of a `.preset` asset: what it is for, Apply (to the selected objects, or to a
//! chosen material / texture), the Preset Manager default toggle, and the stored values with a
//! checkbox per field (unticked = left alone when applying). Valid edits save right away.

use egui::{RichText, Ui};
use misty_engine::preset::{self, Preset};

use super::preset_fields::{Fields, Seg, nice};
use crate::actions::Action;
use crate::presets::{PRESET_ICON, preset_name};
use crate::state::EditorState;
use crate::theme;

fn draft_id(rel: &str) -> egui::Id {
    egui::Id::new(("preset_draft", rel.to_owned()))
}

pub fn preset_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let Some(saved) = state.assets.presets.get(rel).cloned() else {
        ui.weak(format!("Could not read {rel}"));
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(PRESET_ICON).size(22.0));
        ui.label(RichText::new(format!("{} (Preset)", preset_name(rel))).strong());
    });
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Preset for");
        ui.label(RichText::new(&saved.target).strong());
    });
    apply_row(ui, state, rel, &saved);
    // Import settings have no "new asset" moment for a default.
    if saved.target != preset::TEXTURE_IMPORTER {
        default_row(ui, state, rel, &saved.target);
    }
    ui.separator();

    let id = draft_id(rel);
    let mut draft = ui
        .ctx()
        .data(|d| d.get_temp::<Preset>(id))
        .unwrap_or_else(|| saved.clone());
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| values(ui, &mut draft));
    ui.add_space(4.0);
    ui.weak("Unticked fields are left alone when the preset is applied. Default = the type's default value.");
    if draft == saved {
        ui.ctx().data_mut(|d| d.remove::<Preset>(id));
        return;
    }
    match draft.check() {
        Ok(()) => {
            if let Err(e) = state.assets.save_preset(rel, draft) {
                state.console.warn(format!("Could not save {rel}: {e}"));
            }
            ui.ctx().data_mut(|d| d.remove::<Preset>(id));
        }
        Err(e) => {
            // Keep the edit (e.g. half-typed) without saving it.
            ui.colored_label(theme::AXIS_X, format!("Not saved: {e}"));
            ui.ctx().data_mut(|d| d.insert_temp(id, draft));
        }
    }
}

/// One row per field of the target type: include checkbox, name, value (or Default).
fn values(ui: &mut Ui, draft: &mut Preset) {
    let start = draft.clone();
    let fields = Fields { preset: &start };
    for field in start.fields() {
        ui.horizontal_top(|ui| {
            let mut on = draft.includes(&field);
            if ui
                .checkbox(&mut on, "")
                .on_hover_text("Apply this field")
                .changed()
            {
                draft.set_included(&field, on);
            }
            ui.add_enabled_ui(on, |ui| match draft.values.get_mut(&field) {
                Some(v) => {
                    let mut path = vec![Seg::Key(field.clone())];
                    fields.row(ui, &mut path, &nice(&field), &field, v);
                }
                None => {
                    ui.add_sized([130.0, 18.0], egui::Label::new(nice(&field)).truncate());
                    ui.weak("Default");
                }
            });
        });
    }
}

/// Apply to Selection (components) or Apply to <asset> (materials, textures).
fn apply_row(ui: &mut Ui, state: &mut EditorState, rel: &str, p: &Preset) {
    match p.target.as_str() {
        preset::MATERIAL | preset::TEXTURE_IMPORTER => asset_apply(ui, state, rel, p),
        name => {
            let targets = state.selection.list().to_vec();
            let info = misty_engine::scene::registry::find(name);
            let count = targets
                .iter()
                .filter(|e| info.is_some_and(|i| (i.has)(&state.scene.world, **e)))
                .count();
            ui.horizontal(|ui| {
                let button = egui::Button::new("Apply to Selection");
                if ui
                    .add_enabled(count > 0, button)
                    .on_hover_text(format!("Apply to the {name} of every selected object"))
                    .clicked()
                {
                    state.push(Action::ApplyPreset {
                        entities: targets.clone(),
                        preset: rel.to_owned(),
                    });
                }
                if count == 0 {
                    ui.weak(format!("Lock the Inspector and select objects with a {name}."));
                } else {
                    ui.weak(format!("{count} selected"));
                }
            });
        }
    }
}

/// Pick a material or texture and apply (written to its file right away).
fn asset_apply(ui: &mut Ui, state: &mut EditorState, rel: &str, p: &Preset) {
    let material = p.target == preset::MATERIAL;
    let choices: Vec<String> = if material {
        state.assets.materials.keys().cloned().collect()
    } else {
        state.assets.textures.clone()
    };
    let id = egui::Id::new(("preset_apply_to", rel.to_owned()));
    let mut chosen: String = ui
        .ctx()
        .data(|d| d.get_temp(id))
        .or_else(|| choices.first().cloned())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id)
            .width(170.0)
            .selected_text(preset_name(&chosen))
            .show_ui(ui, |ui| {
                for c in &choices {
                    ui.selectable_value(&mut chosen, c.clone(), preset_name(c))
                        .on_hover_text(c);
                }
            });
        if ui
            .add_enabled(!chosen.is_empty(), egui::Button::new("Apply"))
            .clicked()
        {
            let result = if material {
                apply_material(state, p, &chosen)
            } else {
                apply_texture(state, p, &chosen)
            };
            match result {
                Ok(()) => state.console.info(format!("Applied {rel} to {chosen}")),
                Err(e) => state.console.warn(format!("Could not apply {rel}: {e}")),
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, chosen));
}

fn apply_material(state: &mut EditorState, p: &Preset, rel: &str) -> Result<(), String> {
    let m = state.assets.materials.get(rel).ok_or("missing material")?;
    let m = p.apply_value(m)?;
    state.assets.update_material(rel, m)
}

fn apply_texture(state: &mut EditorState, p: &Preset, rel: &str) -> Result<(), String> {
    use misty_engine::sprites::{self, TextureImporter};
    let abs = state.assets.absolute(rel);
    let s = p.apply_value(&TextureImporter::load(&abs))?;
    s.save(&abs)?;
    sprites::invalidate(rel);
    Ok(())
}

/// "Default for new Light": the Preset Manager entry of the target type.
fn default_row(ui: &mut Ui, state: &mut EditorState, rel: &str, target: &str) {
    let defaults = &state.project_settings.default_presets;
    let mut on = defaults.get(target).is_some_and(|d| d == rel);
    let what = if misty_engine::scene::registry::find(target).is_some() {
        format!("Default for new {target} components")
    } else {
        format!("Default for new {target} assets")
    };
    if ui
        .checkbox(&mut on, what)
        .on_hover_text("Project Settings ▸ Preset Manager")
        .changed()
    {
        let defaults = &mut state.project_settings.default_presets;
        if on {
            defaults.insert(target.to_owned(), rel.to_owned());
        } else {
            defaults.remove(target);
        }
        crate::project_settings::changed(state);
    }
}
