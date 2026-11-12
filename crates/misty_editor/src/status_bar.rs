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
            background_progress(ui);
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

/// What the background queues are doing: the renderer starting, shader variants compiling,
/// models and textures importing (a project's first open; round 44).
fn background_progress(ui: &mut Ui) {
    use misty_engine::load_queue;
    let mut busy = false;
    let imports = load_queue::imports().progress();
    if imports.busy() {
        let text = crate::i18n::tr!(
            "Importing {done}/{total}",
            done = imports.done,
            total = imports.total
        );
        ui.label(RichText::new(text).weak());
        busy = true;
    }
    if let Some(compiled) = crate::viewport::renderer_starting() {
        let text = crate::i18n::tr!(
            "Starting the renderer: compiling shaders ({count} done)",
            count = compiled
        );
        ui.label(RichText::new(text).weak());
        busy = true;
    } else {
        let shaders = load_queue::shaders().progress();
        if shaders.busy() {
            let text = crate::i18n::tr!(
                "Compiling shaders {done}/{total}",
                done = shaders.done,
                total = shaders.total
            );
            ui.label(RichText::new(text).weak());
            busy = true;
        }
    }
    if busy {
        ui.spinner();
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}
