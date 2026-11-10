//! Inspector editors of the Saveable and Localized Text components.

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::localization::LocalizedText;
use misty_engine::save::Saveable;
use misty_engine::scene::{UiText, registry};

use crate::state::EditorState;

/// Names of the components handled here.
pub const NAMES: [&str; 2] = ["Saveable", "Localized Text"];

pub fn icon(name: &str) -> Option<&'static str> {
    match name {
        "Saveable" => Some("💾"),
        "Localized Text" => Some("🌐"),
        _ => None,
    }
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    (name == "Localized Text")
        .then(|| {
            state
                .scene
                .world
                .get::<&LocalizedText>(e)
                .ok()
                .map(|c| c.enabled)
        })
        .flatten()
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    if name == "Localized Text" {
        let _ = state
            .scene
            .world
            .get::<&mut LocalizedText>(e)
            .map(|mut c| c.enabled = on);
    }
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Saveable" => saveable(ui, state, e),
        _ => localized_text(ui, state, e),
    }
}

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.add_sized([110.0, 18.0], egui::Label::new(text).truncate())
        .on_hover_text(tip);
}

/// Components (and their fields) of `e` that a Saveable can store.
fn storable(state: &EditorState, e: Entity) -> Vec<(&'static str, Vec<String>)> {
    registry::components()
        .iter()
        .filter(|c| c.user_visible())
        .filter(|c| !matches!(c.name, "Transform" | "Saveable" | "Scripts"))
        .filter(|c| (c.has)(&state.scene.world, e))
        .map(|c| (c.name, (c.fields)()))
        .collect()
}

fn saveable(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut s) = state.scene.world.get::<&Saveable>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let mut ch = false;
    ui.horizontal(|ui| {
        label(
            ui,
            "Save ID",
            "Stable id of this object in save games. Copies get a new one.",
        );
        ui.monospace(RichText::new(short_id(&s.save_id)).weak())
            .on_hover_text(&s.save_id);
        if ui
            .small_button("Copy")
            .on_hover_text("Copy the full Save ID")
            .clicked()
        {
            ui.ctx().copy_text(s.save_id.clone());
        }
        if ui
            .small_button("Regenerate")
            .on_hover_text("Give this object a new Save ID (old saves won't find it)")
            .clicked()
        {
            s.save_id = misty_engine::save::new_id();
            ch = true;
        }
    });
    ui.horizontal(|ui| {
        label(
            ui,
            "Save",
            "What goes into a save game besides script data.",
        );
        ch |= ui
            .checkbox(&mut s.save_transform, "Transform")
            .on_hover_text("Local position, rotation and scale")
            .changed();
        ch |= ui.checkbox(&mut s.save_active, "Active State").changed();
        ch |= ui
            .checkbox(&mut s.save_velocity, "Rigidbody Velocity")
            .changed();
    });
    ui.horizontal(|ui| {
        label(
            ui,
            "Archetypes",
            "Whole archetypes or single fields to store, like \"Light.intensity\".",
        );
        let options = storable(state, e);
        ui.menu_button("Add...", |ui| {
            if options.is_empty() {
                ui.weak("No other archetypes on this object.");
            }
            for (comp, fields) in &options {
                ui.menu_button(*comp, |ui| {
                    if ui.button("Whole Archetype").clicked() {
                        add_entry(&mut s.components, comp);
                        ch = true;
                        ui.close();
                    }
                    ui.separator();
                    for f in fields {
                        if ui.button(f).clicked() {
                            add_entry(&mut s.components, &format!("{comp}.{f}"));
                            ch = true;
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    let mut remove = None;
    for (i, entry) in s.components.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(114.0);
            ui.monospace(entry);
            if ui
                .small_button("✖")
                .on_hover_text("Stop saving this")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        s.components.remove(i);
        ch = true;
    }
    ui.weak("Scripts on this object can add their own data with OnSave(data) and OnLoad(data).");
    if ch {
        let _ = state.scene.world.insert_one(e, s);
    }
    ch
}

fn add_entry(list: &mut Vec<String>, entry: &str) {
    if !list.iter().any(|x| x == entry) {
        list.push(entry.to_owned());
    }
}

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…{}", &id[..8], &id[id.len() - 4..])
    } else {
        id.to_owned()
    }
}

fn localized_text(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut l) = state
        .scene
        .world
        .get::<&LocalizedText>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let loc = crate::saveloc::localizer(state);
    let mut ch = false;
    ui.horizontal(|ui| {
        label(
            ui,
            "Table",
            "String Table to read from (empty = any table).",
        );
        let shown = if l.table.is_empty() {
            "Any Table".to_owned()
        } else {
            l.table.clone()
        };
        egui::ComboBox::from_id_salt(("lt_table", e))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(l.table.is_empty(), "Any Table")
                    .clicked()
                {
                    l.table.clear();
                    ch = true;
                }
                for t in &loc.tables {
                    if ui.selectable_label(l.table == t.name, &t.name).clicked() {
                        l.table = t.name.clone();
                        ch = true;
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        label(ui, "Key", "Entry of the String Table shown by the Text.");
        ch |= ui
            .add(egui::TextEdit::singleline(&mut l.key).desired_width(150.0))
            .changed();
        let keys: Vec<String> = loc
            .tables
            .iter()
            .filter(|t| l.table.is_empty() || t.name == l.table)
            .flat_map(|t| t.entries.iter().map(|x| x.key.clone()))
            .collect();
        ui.menu_button("▾", |ui| {
            if keys.is_empty() {
                ui.weak("No keys: add some in Localization Tables.");
            }
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for k in &keys {
                        if ui.selectable_label(*k == l.key, k).clicked() {
                            l.key = k.clone();
                            ch = true;
                            ui.close();
                        }
                    }
                });
        })
        .response
        .on_hover_text("Pick a key");
    });
    ui.horizontal(|ui| {
        label(
            ui,
            "Arguments",
            "Fill {0}, {1}… in order; entries written name=value fill {name}.",
        );
        if ui
            .small_button("+")
            .on_hover_text("Add an argument")
            .clicked()
        {
            l.args.push(String::new());
            ch = true;
        }
    });
    let mut remove = None;
    for (i, a) in l.args.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(114.0);
            ui.weak(format!("{{{i}}}"));
            ch |= ui
                .add(egui::TextEdit::singleline(a).desired_width(140.0))
                .changed();
            if ui.small_button("✖").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        l.args.remove(i);
        ch = true;
    }
    if state.scene.world.get::<&UiText>(e).is_err() {
        ui.colored_label(
            egui::Color32::from_rgb(230, 170, 60),
            "Needs a Text on this object to show anything.",
        );
    }
    if !l.key.is_empty() {
        let table = Some(l.table.as_str()).filter(|t| !t.is_empty());
        if !loc.has_key(table, &l.key) {
            ui.colored_label(
                egui::Color32::from_rgb(230, 120, 110),
                format!("No table has the key '{}'.", l.key),
            );
        } else {
            egui::CollapsingHeader::new("Preview")
                .id_salt(("lt_preview", e))
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new(("lt_preview_grid", e))
                        .num_columns(2)
                        .show(ui, |ui| {
                            for locale in loc.locales() {
                                ui.weak(locale.label());
                                let own = loc.lookup(table, &l.key, &locale.code).is_some();
                                let text = l.resolve(&loc, &locale.code);
                                if own {
                                    ui.label(text);
                                } else {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(230, 120, 110),
                                        format!("{text}  (missing, fallback)"),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                });
        }
    }
    if ch {
        // Edit mode shows the default locale's string right away.
        let text = l.resolve(&loc, &loc.settings.default_locale);
        if l.enabled
            && !l.key.is_empty()
            && let Ok(mut t) = state.scene.world.get::<&mut UiText>(e)
        {
            t.text = text;
        }
        let _ = state.scene.world.insert_one(e, l);
    }
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::localization::StringTable;

    use super::*;
    use crate::project::ProjectBrowser;

    #[test]
    fn saveable_and_localized_text_inspectors() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_saveloc_insp_{n}"));
        let mut t = StringTable::new("UI", &["en", "fr"]);
        t.set("hello", "en", "Hello {0}");
        t.set("hello", "fr", "Bonjour {0}");
        t.set("bye", "en", "Bye");
        t.save(&root.join("Assets/Localization/UI.strings"))
            .unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let e = s.scene.spawn("Label", None);
        s.scene.add_by_name(e, "Saveable");
        s.scene.add_by_name(e, "Light");
        s.scene.add_by_name(e, "Localized Text");
        {
            let mut l = s.scene.world.get::<&mut LocalizedText>(e).unwrap();
            l.key = "bye".into();
        }
        s.selection.set(e);
        let id = s.scene.world.get::<&Saveable>(e).unwrap().save_id.clone();
        let mut h = Harness::builder()
            .with_size([460.0, 1400.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        for label in [
            "Save ID",
            "Active State",
            "Rigidbody Velocity",
            "Needs a Text on this object to show anything.",
            "Bye  (missing, fallback)",
        ] {
            h.get_by_label(label);
        }
        h.get_by_label("Regenerate").click();
        h.run();
        let s = h.state();
        let new_id = s.scene.world.get::<&Saveable>(e).unwrap().save_id.clone();
        assert_ne!(new_id, id);
        assert_eq!(new_id.len(), 32);
        std::fs::remove_dir_all(root).ok();
    }
}
