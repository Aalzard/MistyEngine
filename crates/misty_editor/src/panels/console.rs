//! Console panel: toolbar with Clear/Collapse/Error Pause, type counters, list and detail pane.

use egui::{RichText, Ui};

use crate::console::LogLevel;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::{panel_toolbar, search_field, toggle_button};

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
        if ui.button("Clear").clicked() {
            c.clear();
        }
        ui.menu_button("⏷", |ui| {
            ui.checkbox(&mut c.clear_on_play, "Clear on Play");
        });
        if toggle_button(ui, c.collapse, "Collapse", "Merge identical messages").clicked() {
            c.collapse = !c.collapse;
        }
        if toggle_button(ui, c.error_pause, "Error Pause", "Pause Play mode on error").clicked() {
            c.error_pause = !c.error_pause;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let err = RichText::new(format!("⛔ {n_err}")).color(LogLevel::Error.color());
            if toggle_button(ui, c.show_errors, err, "Show errors").clicked() {
                c.show_errors = !c.show_errors;
            }
            let warn = RichText::new(format!("⚠ {n_warn}")).color(LogLevel::Warning.color());
            if toggle_button(ui, c.show_warnings, warn, "Show warnings").clicked() {
                c.show_warnings = !c.show_warnings;
            }
            if toggle_button(ui, c.show_info, format!("ℹ {n_info}"), "Show messages").clicked() {
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
                    if let Some(e) = c.selected.and_then(|i| c.entries.get(i)) {
                        ui.add(egui::Label::new(&e.message).selectable(true));
                    }
                });
        });

    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                // Rows: (index of first occurrence, repeat count).
                let mut rows: Vec<(usize, usize)> = Vec::new();
                for (i, e) in c.entries.iter().enumerate() {
                    if !c.is_visible(e) {
                        continue;
                    }
                    if c.collapse
                        && let Some(r) = rows.iter_mut().find(|(j, _)| {
                            let o = &c.entries[*j];
                            o.level == e.level && o.message == e.message
                        })
                    {
                        r.1 += 1;
                        continue;
                    }
                    rows.push((i, 1));
                }

                for (n, (i, count)) in rows.into_iter().enumerate() {
                    let e = &c.entries[i];
                    let selected = c.selected == Some(i);
                    let bg = if selected {
                        theme::SELECTION
                    } else if n % 2 == 0 {
                        egui::Color32::from_rgb(56, 56, 56)
                    } else {
                        egui::Color32::from_rgb(60, 60, 60)
                    };
                    let mut link_rect = None;
                    let resp = egui::Frame::new()
                        .fill(bg)
                        .inner_margin(egui::Margin::symmetric(4, 3))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(e.level.icon())
                                        .size(18.0)
                                        .color(e.level.color()),
                                );
                                let first = e.message.lines().next().unwrap_or("");
                                ui.label(format!("[{}] {first}", e.time));
                                if let Some(link) =
                                    crate::panels::code_editor::console_link(&e.message)
                                {
                                    link_rect = Some(
                                        ui.link(link).on_hover_text("Open in the Code Editor").rect,
                                    );
                                }
                                if c.collapse {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(count.to_string()).small().strong(),
                                            );
                                        },
                                    );
                                }
                            });
                        })
                        .response
                        .interact(egui::Sense::click());
                    if resp.clicked() {
                        c.selected = Some(i);
                    }
                    let on_link = resp
                        .interact_pointer_pos()
                        .is_some_and(|p| link_rect.is_some_and(|r: egui::Rect| r.contains(p)));
                    if resp.double_clicked() || (resp.clicked() && on_link) {
                        open_script = Some(e.message.clone());
                    }
                }
            });
    });
    if let Some(msg) = open_script {
        crate::panels::code_editor::open_console_location(state, &msg);
    }
}
