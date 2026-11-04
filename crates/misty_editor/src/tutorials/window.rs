//! Help ▸ Tutorials: the list of tutorials with their time and a ✓ for finished ones.

use egui::{Align, Layout, RichText};

use crate::state::EditorState;
use crate::theme;

pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    if !s.tutorials.window_open {
        return;
    }
    let mut open = true;
    let mut start = None;
    egui::Window::new("Tutorials")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(400.0)
        .default_pos(ctx.content_rect().center() - egui::vec2(200.0, 180.0))
        .show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(
                RichText::new("Learn step by step, right in the editor.").color(theme::TEXT_DIM),
            );
            ui.add_space(4.0);
            for t in super::list() {
                let done = s.tutorials.progress.is_done(t.id);
                if row(ui, &t, done) {
                    start = Some(t.id);
                }
            }
            ui.add_space(2.0);
            ui.label(
                RichText::new("Each tutorial opens a new, untitled scene.")
                    .small()
                    .color(theme::TEXT_DIM),
            );
        });
    s.tutorials.window_open = open;
    if let Some(id) = start {
        super::start(s, id);
    }
}

/// One tutorial; returns true when Start is clicked.
fn row(ui: &mut egui::Ui, t: &super::TutorialInfo, done: bool) -> bool {
    let mut clicked = false;
    egui::Frame::group(ui.style())
        .fill(theme::FIELD_BG)
        .corner_radius(6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(300.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t.title).strong().color(egui::Color32::WHITE));
                        if done {
                            ui.label(RichText::new("✓").color(theme::AXIS_Y).strong())
                                .on_hover_text("Finished");
                        }
                    });
                    ui.label(RichText::new(t.description).color(theme::TEXT));
                    ui.label(
                        RichText::new(format!("⏱ About {} min", t.minutes))
                            .small()
                            .color(theme::TEXT_DIM),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if done { "Again" } else { "Start" };
                    clicked = ui.button(label).clicked();
                });
            });
        });
    clicked
}
