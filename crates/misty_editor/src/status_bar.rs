//! Bottom status bar: mirrors the latest console message; click opens the Console.

use egui::{RichText, Sense, Ui};

use crate::actions::Action;
use crate::layout::Tab;
use crate::state::EditorState;

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    ui.horizontal(|ui| {
        // Right: "Reloaded 3 assets" and rebuild progress; the console line fills the rest.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::live_reload::status_items(ui, state);
            // A scene's first launch: dense meshes' GI distance fields bake in the background.
            let bakes = misty_engine::render::mesh_sdf_bakes_pending();
            if bakes > 0 {
                let text =
                    crate::i18n::tr!("Baking GI distance fields ({count} left)", count = bakes);
                ui.label(RichText::new(text).weak());
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(500));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let text = match state.console.last() {
                    Some(e) => {
                        let line = crate::i18n::display(first_line(&e.message));
                        RichText::new(format!("{} {line}", e.level.icon())).color(e.level.color())
                    }
                    None => RichText::new(""),
                };
                let resp = ui.add(egui::Label::new(text).truncate().sense(Sense::click()));
                if resp.clicked() {
                    state.push(Action::FocusTab(Tab::Console));
                }
            });
        });
    });
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}
