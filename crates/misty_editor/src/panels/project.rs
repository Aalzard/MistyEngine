//! Project window: folder tree on the left, folder contents on the right.

use std::path::PathBuf;

use egui::{Align2, Color32, FontId, Sense, Ui};

use crate::project::{AssetEntry, FolderNode};
use crate::state::EditorState;
use crate::theme;
use crate::widgets::{panel_toolbar, search_field};

use super::hierarchy::row_simple as row;

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let browser = &mut state.project;
    panel_toolbar(ui, |ui| {
        ui.menu_button("➕ ⏷", |ui| {
            ui.weak("Create menu arrives with assets (step 5)");
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("⟳").on_hover_text("Refresh (Ctrl+R)").clicked() {
                browser.refresh();
            }
            search_field(ui, &mut browser.search, 180.0);
        });
    });

    // Bottom bar: selected path and icon-size slider.
    egui::Panel::bottom("project_bottom_bar")
        .exact_size(22.0)
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                if let Some(sel) = &browser.selected {
                    ui.label(browser.display_path(sel));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Slider::new(&mut browser.icon_size, 0.0..=1.0).show_value(false));
                });
            });
        });

    egui::Panel::left("project_tree")
        .default_size(180.0)
        .resizable(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("★ Favorites").color(theme::TEXT_DIM));
                    let tree = browser.tree.clone();
                    let mut open = None;
                    folder_tree(ui, &tree, &browser.current, 0, &mut open);
                    ui.label(egui::RichText::new("🗀 Packages").color(theme::TEXT_DIM));
                    if let Some(path) = open {
                        browser.open_folder(path);
                    }
                });
        });

    egui::CentralPanel::default().show(ui, |ui| {
        // Breadcrumb.
        let mut jump = None;
        ui.horizontal(|ui| {
            if browser.search.is_empty() {
                for (i, (name, path)) in browser.breadcrumb().into_iter().enumerate() {
                    if i > 0 {
                        ui.label("›");
                    }
                    if ui.add(egui::Button::new(name).frame(false)).clicked() {
                        jump = Some(path);
                    }
                }
            } else {
                ui.label(format!("Search: '{}'", browser.search));
            }
        });
        ui.separator();
        if let Some(path) = jump {
            browser.open_folder(path);
        }

        let entries = if browser.search.is_empty() {
            browser.entries.clone()
        } else {
            browser.search_results()
        };
        let mut open = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if browser.icon_size < 0.05 {
                    list_view(ui, &entries, &mut browser.selected, &mut open);
                } else {
                    grid_view(
                        ui,
                        &entries,
                        browser.icon_size,
                        &mut browser.selected,
                        &mut open,
                    );
                }
            });
        if let Some(path) = open {
            browser.search.clear();
            browser.open_folder(path);
        }
    });
}

fn folder_tree(
    ui: &mut Ui,
    node: &FolderNode,
    current: &PathBuf,
    depth: usize,
    open: &mut Option<PathBuf>,
) {
    let selected = &node.path == current;
    let icon = if selected { "🗁" } else { "🗀" };
    if row(
        ui,
        selected,
        &format!("{icon} {}", node.name),
        theme::TEXT,
        depth,
    )
    .clicked()
    {
        *open = Some(node.path.clone());
    }
    for child in &node.children {
        folder_tree(ui, child, current, depth + 1, open);
    }
}

fn list_view(
    ui: &mut Ui,
    entries: &[AssetEntry],
    selected: &mut Option<PathBuf>,
    open: &mut Option<PathBuf>,
) {
    for e in entries {
        let is_sel = selected.as_ref() == Some(&e.path);
        let resp = row(
            ui,
            is_sel,
            &format!("{} {}", e.icon(), e.name),
            theme::TEXT,
            0,
        );
        handle_click(&resp, e, selected, open);
    }
}

fn grid_view(
    ui: &mut Ui,
    entries: &[AssetEntry],
    icon_size: f32,
    selected: &mut Option<PathBuf>,
    open: &mut Option<PathBuf>,
) {
    let cell = 48.0 + icon_size * 80.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        for e in entries {
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(cell, cell + 18.0), Sense::click());
            let is_sel = selected.as_ref() == Some(&e.path);
            if is_sel {
                ui.painter().rect_filled(rect, 3.0, theme::SELECTION);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, 3.0, Color32::from_rgb(69, 69, 69));
            }
            ui.painter().text(
                rect.center_top() + egui::vec2(0.0, cell * 0.5),
                Align2::CENTER_CENTER,
                e.icon(),
                FontId::proportional(cell * 0.6),
                Color32::from_gray(200),
            );
            ui.painter().text(
                rect.center_bottom() - egui::vec2(0.0, 9.0),
                Align2::CENTER_CENTER,
                truncate(&e.name, (cell / 7.0) as usize),
                FontId::proportional(12.0),
                theme::TEXT,
            );
            handle_click(&resp, e, selected, open);
        }
    });
}

fn handle_click(
    resp: &egui::Response,
    e: &AssetEntry,
    selected: &mut Option<PathBuf>,
    open: &mut Option<PathBuf>,
) {
    if resp.clicked() {
        *selected = Some(e.path.clone());
    }
    if resp.double_clicked() && e.is_dir {
        *open = Some(e.path.clone());
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
