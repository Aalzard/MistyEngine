//! The Settings page (Learn is in `learn.rs`).

use egui::RichText;

use super::config::HubTheme;
use super::{Hub, style};

pub fn settings(ui: &mut egui::Ui, hub: &mut Hub) {
    let p = style::palette(hub.config.theme);
    ui.heading(RichText::new("Settings").strong());
    ui.add_space(14.0);
    let mut changed = false;
    section(ui, &p, "Projects", |ui| {
        ui.label("Default location for new projects");
        ui.horizontal(|ui| {
            let mut folder = hub.config.projects_folder().display().to_string();
            let edit = ui.add(egui::TextEdit::singleline(&mut folder).desired_width(420.0));
            if edit.changed() {
                hub.config.projects_folder = Some(folder.into());
                changed = true;
            }
            if ui.button("Browse…").clicked()
                && let Some(dir) = super::pick_folder(&hub.config.projects_folder())
            {
                hub.config.projects_folder = Some(dir);
                changed = true;
            }
            if hub.config.projects_folder.is_some() && ui.button("Reset").clicked() {
                hub.config.projects_folder = None;
                changed = true;
            }
        });
        ui.add_space(6.0);
        let recent = hub.config.recent.len();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{recent} recent projects")).color(p.dim));
            if ui
                .add_enabled(recent > 0, egui::Button::new("Clear list"))
                .clicked()
            {
                hub.config.recent.clear();
                changed = true;
            }
        });
    });
    section(ui, &p, "Appearance", |ui| {
        ui.horizontal(|ui| {
            ui.label("Theme");
            ui.add_space(8.0);
            for (theme, label) in [(HubTheme::Dark, "Dark"), (HubTheme::Light, "Light")] {
                if ui
                    .selectable_value(&mut hub.config.theme, theme, label)
                    .changed()
                {
                    changed = true;
                }
            }
        });
        ui.label(
            RichText::new("Applies to the Hub; the editor keeps its dark theme.")
                .small()
                .color(p.dim),
        );
    });
    if let Some(path) = &hub.config_path {
        ui.label(
            RichText::new(format!("Settings are saved in {}", path.display()))
                .small()
                .color(p.dim),
        );
    }
    if changed {
        hub.save_config();
    }
}

fn section(ui: &mut egui::Ui, p: &style::Palette, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            ui.label(RichText::new(title).size(15.0).strong());
            ui.add_space(4.0);
            body(ui);
        });
    ui.add_space(10.0);
}
