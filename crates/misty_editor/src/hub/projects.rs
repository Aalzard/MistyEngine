//! Projects page: recent projects with search, New Project and Open.

use std::path::PathBuf;

use egui::{Align, Color32, Layout, RichText, Sense, UiBuilder, vec2};

use super::config::{self, RecentProject};
use super::{Hub, style};

const ROW_HEIGHT: f32 = 68.0;
const THUMB: egui::Vec2 = egui::vec2(88.0, 50.0);

/// What a row asked for.
enum RowAction {
    Open(PathBuf),
    Reveal(PathBuf),
    Remove(PathBuf),
    Locate(PathBuf),
}

pub fn show(ui: &mut egui::Ui, hub: &mut Hub) {
    let p = style::palette(hub.config.theme);
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Projects").strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(style::primary("New project")).clicked() {
                hub.notice = None;
                hub.start_new_project();
            }
            if ui.button("Open…").clicked() {
                hub.notice = None;
                if let Some(dir) = super::pick_folder(&hub.config.projects_folder()) {
                    hub.open_project(&dir);
                }
            }
        });
    });
    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(&mut hub.search)
            .hint_text("🔍  Search projects")
            .desired_width(320.0),
    );
    if let Some(n) = &hub.notice {
        ui.colored_label(style::ERROR, n);
    }
    ui.add_space(8.0);

    let rows: Vec<RecentProject> = hub
        .config
        .search(&hub.search)
        .into_iter()
        .cloned()
        .collect();
    if hub.config.recent.is_empty() {
        empty_state(ui, hub, &p);
        return;
    }
    // Column headers.
    ui.horizontal(|ui| {
        ui.add_space(12.0 + THUMB.x + 14.0);
        ui.label(RichText::new("Name").small().color(p.dim));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(52.0);
            ui.label(RichText::new("Last opened").small().color(p.dim));
        });
    });
    ui.add(egui::Separator::default().spacing(2.0));
    if rows.is_empty() {
        ui.add_space(20.0);
        ui.label(RichText::new("No projects match your search.").color(p.dim));
    }
    let mut action = None;
    let now = config::now();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for r in &rows {
                if let Some(a) = row(ui, hub, &p, r, now) {
                    action = Some(a);
                }
            }
        });
    match action {
        Some(RowAction::Open(path)) => hub.open_project(&path),
        Some(RowAction::Reveal(path)) => {
            if let Err(e) = crate::asset_actions::show_in_explorer(&path) {
                hub.notice = Some(format!("Could not show {}: {e}", path.display()));
            }
        }
        Some(RowAction::Remove(path)) => {
            hub.config.remove(&path);
            hub.save_config();
        }
        Some(RowAction::Locate(old)) => {
            let start = old.parent().map(PathBuf::from).unwrap_or_default();
            if let Some(new) = super::pick_folder(&start) {
                if new.join("Assets").is_dir() {
                    hub.config.relocate(&old, &new);
                    hub.save_config();
                    hub.textures.projects.remove(&old);
                } else {
                    hub.notice = Some(format!("{} has no Assets folder.", new.display()));
                }
            }
        }
        None => {}
    }
}

fn empty_state(ui: &mut egui::Ui, hub: &mut Hub, p: &style::Palette) {
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("No projects yet").size(18.0).strong());
        ui.label(
            RichText::new("Create a new project from a template, or open an existing folder.")
                .color(p.dim),
        );
        ui.add_space(10.0);
        if ui.add(style::primary("New project")).clicked() {
            hub.start_new_project();
        }
    });
}

/// One project row: thumbnail, name and path, last opened, and a ⋯ menu.
fn row(
    ui: &mut egui::Ui,
    hub: &mut Hub,
    p: &style::Palette,
    r: &RecentProject,
    now: u64,
) -> Option<RowAction> {
    let exists = r.exists();
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let hovered = resp.hovered() && exists;
    ui.painter()
        .rect_filled(rect, 8.0, if hovered { p.card_hover } else { p.card });
    let mut action = None;
    if exists && resp.clicked() {
        action = Some(RowAction::Open(r.path.clone()));
    }
    if exists {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let inner = rect.shrink2(vec2(12.0, 9.0));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut child;
    // Thumbnail, or a tile with the project's initial.
    let thumb = if exists {
        hub.project_texture(ui.ctx(), &r.path)
    } else {
        None
    };
    match thumb {
        Some(tex) => {
            ui.add(
                egui::Image::new(&tex)
                    .fit_to_exact_size(THUMB)
                    .corner_radius(4.0),
            );
        }
        None => initial_tile(ui, p, &r.name(), exists),
    }
    ui.add_space(6.0);
    let (name_color, path_color) = if exists {
        (p.text, p.dim)
    } else {
        (p.missing, p.missing)
    };
    // Name and path, leaving room on the right for the time and the menu.
    let text_width = (ui.available_width() - 200.0).max(80.0);
    let text_size = vec2(text_width, ui.available_height());
    ui.allocate_ui_with_layout(text_size, Layout::top_down(Align::Min), |ui| {
        ui.set_max_width(text_width);
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new(r.name())
                .size(15.0)
                .strong()
                .color(name_color),
        );
        let path = r.path.display().to_string();
        let path = if exists {
            path
        } else {
            format!("{path}  (folder not found)")
        };
        ui.add(egui::Label::new(RichText::new(path).small().color(path_color)).truncate());
    });
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.menu_button(RichText::new("⋯").size(18.0), |ui| {
            if exists
                && ui
                    .button(crate::panels::project_menus::explorer_label())
                    .clicked()
            {
                action = Some(RowAction::Reveal(r.path.clone()));
            }
            if !exists && ui.button("Locate…").clicked() {
                action = Some(RowAction::Locate(r.path.clone()));
            }
            if ui.button("Remove from list").clicked() {
                action = Some(RowAction::Remove(r.path.clone()));
            }
        });
        if exists {
            let when = config::ago(now.saturating_sub(r.last_opened));
            ui.label(RichText::new(when).color(p.dim));
        } else if ui.button("Locate…").clicked() {
            action = Some(RowAction::Locate(r.path.clone()));
        }
    });
    action
}

fn initial_tile(ui: &mut egui::Ui, p: &style::Palette, name: &str, exists: bool) {
    let (rect, _) = ui.allocate_exact_size(THUMB, Sense::hover());
    // A stable hue per name.
    let h = name
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let hue = (h % 360) as f32 / 360.0;
    let fill: Color32 = if exists {
        egui::ecolor::Hsva::new(hue, 0.35, 0.42, 1.0).into()
    } else {
        p.border
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    let letter = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(22.0),
        Color32::from_white_alpha(if exists { 230 } else { 120 }),
    );
}
