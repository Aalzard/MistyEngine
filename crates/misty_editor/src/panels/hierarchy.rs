//! Hierarchy panel: scene root row and the list of objects.

use egui::{RichText, Sense, Ui};

use crate::actions::Action;
use crate::menu_bar::create_menu_items;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::{panel_toolbar, search_field};

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    panel_toolbar(ui, |ui| {
        ui.menu_button("➕ ⏷", |ui| create_menu_items(ui, state));
        search_field(ui, &mut state.hierarchy_search, ui.available_width());
    });

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // Scene root row.
            let dirty = if state.scene_dirty { "*" } else { "" };
            ui.label(RichText::new(format!("🎬 {}{dirty}", state.scene_name)).strong());

            let search = state.hierarchy_search.to_lowercase();
            let mut clicked = None;
            for (i, obj) in state.objects.iter().enumerate() {
                if !search.is_empty() && !obj.name.to_lowercase().contains(&search) {
                    continue;
                }
                let selected = state.selected == Some(i);
                let color = if obj.active {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                };
                let resp = row(
                    ui,
                    selected,
                    &format!("{} {}", obj.icon, obj.name),
                    color,
                    1,
                );
                if resp.clicked() {
                    clicked = Some(i);
                }
                if resp.double_clicked() {
                    state.actions.push(Action::FrameSelected);
                }
                resp.context_menu(|ui| {
                    if ui.button("Duplicate").clicked() {
                        state.actions.push(Action::DuplicateSelected);
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        state.actions.push(Action::DeleteSelected);
                        ui.close();
                    }
                });
            }
            if let Some(i) = clicked {
                state.selected = Some(i);
            }

            // Clicking empty space deselects; right-click offers the create menu.
            let rest = ui.allocate_response(ui.available_size(), Sense::click());
            if rest.clicked() {
                state.selected = None;
            }
            rest.context_menu(|ui| create_menu_items(ui, state));
        });
}

/// Full-width selectable row with indentation.
pub fn row(
    ui: &mut Ui,
    selected: bool,
    text: &str,
    color: egui::Color32,
    depth: usize,
) -> egui::Response {
    let height = 18.0;
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 0.0, theme::SELECTION);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_rgb(69, 69, 69));
    }
    let pos = rect.left_center() + egui::vec2(6.0 + depth as f32 * 14.0, 0.0);
    ui.painter().text(
        pos,
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(13.0),
        if selected {
            egui::Color32::WHITE
        } else {
            color
        },
    );
    resp
}
