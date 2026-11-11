//! Console panel: toolbar with Clear/Collapse/Error Pause, type counters, list and detail pane.
//!
//! The list draws only the rows in view (`ScrollArea::show_rows`), from the rows the console
//! keeps ([`crate::console::Console::rows`]), so a frame costs the same however much was
//! logged.

use egui::{RichText, Ui};

use crate::console::{Console, LogLevel};
use crate::i18n::tr;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::{panel_toolbar, search_field, toggle_button};

/// Size of the level icon at the start of a row.
const ICON_SIZE: f32 = 18.0;
/// Space around a row's text (left/right, top/bottom).
const ROW_MARGIN: egui::Vec2 = egui::vec2(4.0, 3.0);

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    // A message naming a script line opens it in the Code Editor (link or double-click).
    let mut open_script: Option<String> = None;
    let c = &mut state.console;
    let (n_info, n_warn, n_err) = (
        c.count(LogLevel::Info),
        c.count(LogLevel::Warning),
        c.count(LogLevel::Error),
    );

    panel_toolbar(ui, |ui| {
        if ui.button(tr!("Clear")).clicked() {
            c.clear();
        }
        ui.menu_button("⏷", |ui| {
            ui.checkbox(&mut c.clear_on_play, tr!("Clear on Play"));
        });
        let (name, tip) = (tr!("Collapse"), tr!("Merge identical messages"));
        if toggle_button(ui, c.collapse, name, &tip).clicked() {
            c.collapse = !c.collapse;
        }
        let (name, tip) = (tr!("Error Pause"), tr!("Pause Play mode on error"));
        if toggle_button(ui, c.error_pause, name, &tip).clicked() {
            c.error_pause = !c.error_pause;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let err = RichText::new(format!("⛔ {n_err}")).color(LogLevel::Error.color());
            if toggle_button(ui, c.show_errors, err, &tr!("Show errors")).clicked() {
                c.show_errors = !c.show_errors;
            }
            let warn = RichText::new(format!("⚠ {n_warn}")).color(LogLevel::Warning.color());
            if toggle_button(ui, c.show_warnings, warn, &tr!("Show warnings")).clicked() {
                c.show_warnings = !c.show_warnings;
            }
            let tip = tr!("Show messages");
            if toggle_button(ui, c.show_info, format!("ℹ {n_info}"), &tip).clicked() {
                c.show_info = !c.show_info;
            }
            search_field(ui, &mut c.search, 160.0);
        });
    });

    // Detail pane for the selected message.
    egui::Panel::bottom("console_detail")
        .resizable(true)
        .default_size(60.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    if let Some(e) = c.selected.and_then(|i| c.get(i)) {
                        ui.add(egui::Label::new(&e.message).selectable(true));
                    }
                });
        });

    egui::CentralPanel::default().show(ui, |ui| {
        // Filtered (and collapsed) rows: kept by the console, not rebuilt here every frame.
        let total = c.rows().len();
        let mut select = None;
        let row_height = row_height(ui);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show_rows(ui, row_height, total, |ui, range| {
                for n in range {
                    let (clicked, open) = row(ui, c, n, row_height);
                    select = clicked.or(select);
                    open_script = open.or(open_script.take());
                }
            });
        if select.is_some() {
            c.selected = select;
        }
    });
    if let Some(msg) = open_script {
        crate::panels::code_editor::open_console_location(state, &msg);
    }
}

/// Preferences ▸ General: how many lines the Console keeps; the size picked this frame, if
/// any.
pub fn capacity_row(ui: &mut Ui, now: usize) -> Option<usize> {
    let mut picked = None;
    ui.horizontal(|ui| {
        let label = ui.label(tr!("Console Lines"));
        let r = egui::ComboBox::from_id_salt("pref_console_lines")
            .selected_text(now.to_string())
            .show_ui(ui, |ui| {
                for lines in crate::console::CAPACITIES {
                    if ui
                        .selectable_label(now == lines, lines.to_string())
                        .clicked()
                    {
                        picked = Some(lines);
                    }
                }
            });
        r.response.labelled_by(label.id).on_hover_text(tr!(
            "How many messages the Console keeps. Older ones are dropped; the counters \
             still count them"
        ));
    });
    picked
}

/// Every row has this height, whatever its text (the list shows first lines only).
fn row_height(ui: &Ui) -> f32 {
    let icon = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(ICON_SIZE)));
    icon.max(ui.spacing().interact_size.y) + 2.0 * ROW_MARGIN.y
}

/// Draw the `n`th row. Returns the line to select (clicked) and the message whose script to
/// open (double-click, or a click on its link).
fn row(ui: &mut Ui, c: &Console, n: usize, height: f32) -> (Option<u64>, Option<String>) {
    let size = egui::vec2(ui.available_width(), height);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let Some((row, e)) = c
        .built_rows()
        .get(n)
        .and_then(|r| Some((*r, c.get(r.index)?)))
    else {
        return (None, None);
    };
    let bg = if c.selected == Some(row.index) {
        theme::SELECTION
    } else if n % 2 == 0 {
        egui::Color32::from_rgb(56, 56, 56)
    } else {
        egui::Color32::from_rgb(60, 60, 60)
    };
    ui.painter().rect_filled(rect, 0.0, bg);
    let mut link_rect = None;
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("console_row", row.index))
            .max_rect(rect.shrink2(ROW_MARGIN)),
    );
    inner.set_clip_rect(rect.intersect(ui.clip_rect()));
    inner.horizontal(|ui| {
        ui.label(
            RichText::new(e.level.icon())
                .size(ICON_SIZE)
                .color(e.level.color()),
        );
        let first = e.message.lines().next().unwrap_or("");
        ui.label(format!("[{}] {first}", e.time));
        if let Some(link) = crate::panels::code_editor::console_link(&e.message) {
            link_rect = Some(
                ui.link(link)
                    .on_hover_text(tr!("Open in the Code Editor"))
                    .rect,
            );
        }
        if c.collapse {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(row.count.to_string()).small().strong());
            });
        }
    });
    // After the labels, so the row gets the clicks.
    let id = ui.id().with(("console_row_click", row.index));
    let resp = ui.interact(rect, id, egui::Sense::click());
    let on_link = resp
        .interact_pointer_pos()
        .is_some_and(|p| link_rect.is_some_and(|r: egui::Rect| r.contains(p)));
    let open = (resp.double_clicked() || (resp.clicked() && on_link)).then(|| e.message.clone());
    (resp.clicked().then_some(row.index), open)
}

#[cfg(test)]
#[path = "console_tests.rs"]
mod tests;
