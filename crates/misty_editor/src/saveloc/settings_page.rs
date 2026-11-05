//! Project Settings ▸ Localization: the game's locales, default and fallback locale, and how
//! the locale is picked at startup.

use egui::Ui;
use misty_engine::localization::{Locale, StartupLocale};
use misty_engine::settings::ProjectSettings;

/// Common locales offered by "Add Locale".
pub const COMMON: [(&str, &str); 12] = [
    ("en", "English"),
    ("fr", "Français"),
    ("es", "Español"),
    ("de", "Deutsch"),
    ("it", "Italiano"),
    ("pt-BR", "Português (Brasil)"),
    ("ru", "Русский"),
    ("ja", "日本語"),
    ("zh-CN", "中文（简体）"),
    ("ko", "한국어"),
    ("ar", "العربية"),
    ("tr", "Türkçe"),
];

/// The page. Returns true when a value changed.
pub fn page(ui: &mut Ui, p: &mut ProjectSettings) -> bool {
    let loc = &mut p.localization;
    let mut edited = false;
    ui.strong("Locales");
    if loc.locales.is_empty() {
        ui.weak("None listed: every locale column of the String Tables is available.");
    }
    let mut remove = None;
    egui::Grid::new("loc_locales")
        .num_columns(3)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.weak("Code");
            ui.weak("Name");
            ui.label("");
            ui.end_row();
            for (i, l) in loc.locales.iter_mut().enumerate() {
                edited |= ui
                    .add(egui::TextEdit::singleline(&mut l.code).desired_width(70.0))
                    .changed();
                edited |= ui
                    .add(egui::TextEdit::singleline(&mut l.name).desired_width(160.0))
                    .changed();
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        loc.locales.remove(i);
        edited = true;
    }
    ui.horizontal(|ui| {
        ui.menu_button("Add Locale", |ui| {
            for (code, name) in COMMON {
                let present = loc.locales.iter().any(|l| l.code == code);
                if ui
                    .add_enabled(!present, egui::Button::new(format!("{name} ({code})")))
                    .clicked()
                {
                    loc.locales.push(Locale::new(code, name));
                    edited = true;
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Custom").clicked() {
                loc.locales.push(Locale::new("xx", "New Locale"));
                edited = true;
                ui.close();
            }
        });
    });
    ui.add_space(8.0);
    let codes: Vec<String> = loc.locales.iter().map(|l| l.code.clone()).collect();
    let mut locale_combo = |ui: &mut Ui, id: &str, label: &str, tip: &str, v: &mut String| {
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 18.0], egui::Label::new(label))
                .on_hover_text(tip);
            egui::ComboBox::from_id_salt(id)
                .selected_text(v.as_str())
                .show_ui(ui, |ui| {
                    for c in &codes {
                        if ui.selectable_label(v == c, c).clicked() {
                            *v = c.clone();
                            edited = true;
                        }
                    }
                });
            if codes.is_empty() {
                edited |= ui
                    .add(egui::TextEdit::singleline(v).desired_width(60.0))
                    .changed();
            }
        });
    };
    locale_combo(
        ui,
        "loc_default",
        "Default Locale",
        "Used at startup when nothing better is known.",
        &mut loc.default_locale,
    );
    locale_combo(
        ui,
        "loc_fallback",
        "Fallback Locale",
        "Shown when the current locale has no text for a key.",
        &mut loc.fallback_locale,
    );
    ui.horizontal(|ui| {
        ui.add_sized([150.0, 18.0], egui::Label::new("Startup Locale"))
            .on_hover_text("How a game picks its locale when it starts.");
        egui::ComboBox::from_id_salt("loc_startup")
            .selected_text(loc.startup.label())
            .show_ui(ui, |ui| {
                for m in StartupLocale::ALL {
                    if ui.selectable_label(loc.startup == m, m.label()).clicked() {
                        loc.startup = m;
                        edited = true;
                    }
                }
            });
    });
    ui.weak(match loc.startup {
        StartupLocale::SavedPreference => {
            "The player's last choice, else the system language, else the default locale."
        }
        StartupLocale::SystemLanguage => "The system language when the game has it.",
        StartupLocale::Default => "Always starts in the default locale.",
    });
    ui.add_space(6.0);
    ui.weak("Edit texts in Window ▸ General ▸ Localization Tables.");
    edited
}
