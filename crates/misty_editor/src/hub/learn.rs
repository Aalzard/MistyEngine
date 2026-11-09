//! The Learn page: the editor's built-in tutorials (`crate::tutorials::list`), with a ✓ for the
//! ones finished (the tutorial progress file next to `hub.json`).
//!
//! Start asks which recent project to open the tutorial in (each tutorial starts in a new,
//! untitled scene, so any project will do), or, with no recent project, makes or reuses the
//! "Tutorial Playground" project (Floating Platform template) in the projects folder. The editor then
//! starts the tutorial once the project has loaded (see `launcher`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align, Layout, RichText};

use super::{Hub, config, style};
use crate::templates::Template;
use crate::tutorials::{self, TutorialInfo};

/// The scratch project tutorials open in when there is no recent project.
pub const PLAYGROUND: &str = "Tutorial Playground";

/// Most recent projects offered in the "Open in…" dialog.
const PICK_MAX: usize = 6;

/// IDs of finished tutorials for the Hub reading `config_path` (`None` = none finished).
pub fn load_done(config_path: Option<&Path>) -> BTreeSet<String> {
    match config_path {
        // The real per-user file (also reads the old location).
        Some(p) if config::config_path().as_deref() == Some(p) => {
            tutorials::Progress::load_default().done
        }
        Some(p) => tutorials::Progress::load(p.parent().map(tutorials::progress_file_in)).done,
        None => BTreeSet::new(),
    }
}

/// `<projects folder>/Tutorial Playground`, created from the Floating Platform template if needed.
pub fn playground(projects_folder: &Path) -> Result<PathBuf, String> {
    let dir = projects_folder.join(PLAYGROUND);
    if !dir.join("Assets").is_dir() {
        crate::templates::create(&dir, Template::FloatingPlatform)?;
    }
    Ok(dir)
}

impl Hub {
    /// Start on the Learn page: pick a project, or go straight to the playground.
    fn start_tutorial(&mut self, id: &'static str) {
        self.notice = None;
        if self.config.recent.iter().any(|r| r.exists()) {
            self.tutorial_pick = Some(id);
        } else {
            self.open_tutorial_in_playground(id);
        }
    }

    fn open_tutorial_in_playground(&mut self, id: &'static str) {
        match playground(&self.config.projects_folder()) {
            Ok(dir) => self.open_tutorial_in(&dir, id),
            Err(e) => self.notice = Some(format!("Could not create {PLAYGROUND}: {e}")),
        }
    }

    fn open_tutorial_in(&mut self, project: &Path, id: &'static str) {
        self.tutorial_pick = None;
        self.open_project(project);
        if self.open.is_some() {
            self.open_tutorial = Some(id.to_owned());
        }
    }
}

pub fn show(ui: &mut egui::Ui, hub: &mut Hub) {
    let p = style::palette(hub.config.theme);
    ui.heading(RichText::new("Learn").strong());
    ui.label(
        RichText::new("Step-by-step tutorials that teach the editor, right in the editor.")
            .color(p.dim),
    );
    ui.add_space(14.0);
    let mut start = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for t in tutorials::list() {
                let done = hub.tutorials_done.contains(t.id);
                if row(ui, &p, &t, done) {
                    start = Some(t.id);
                }
                ui.add_space(4.0);
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Each tutorial opens a project of yours (or the Tutorial Playground) on a new, \
                     untitled scene.",
                )
                .small()
                .color(p.dim),
            );
            if let Some(n) = &hub.notice {
                ui.colored_label(style::ERROR, n);
            }
        });
    if let Some(id) = start {
        hub.start_tutorial(id);
    }
    pick_project(ui, hub, &p);
}

/// One tutorial card; true when Start (or Again) is clicked.
fn row(ui: &mut egui::Ui, p: &style::Palette, t: &TutorialInfo, done: bool) -> bool {
    let mut clicked = false;
    egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t.title).size(15.0).strong());
                        if done {
                            ui.label(
                                RichText::new("✓ Done")
                                    .small()
                                    .strong()
                                    .color(egui::Color32::from_rgb(110, 196, 120)),
                            );
                        }
                    });
                    ui.label(RichText::new(t.description).color(p.dim));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if done { "Again" } else { "Start" };
                    let button = if done {
                        egui::Button::new(label)
                    } else {
                        style::primary(label)
                    };
                    clicked = ui.add(button).clicked();
                    ui.label(
                        RichText::new(format!("{} min", t.minutes))
                            .small()
                            .color(p.dim),
                    );
                });
            });
        });
    clicked
}

/// "Open the tutorial in…": recent projects plus the Tutorial Playground.
fn pick_project(ui: &mut egui::Ui, hub: &mut Hub, p: &style::Palette) {
    let Some(id) = hub.tutorial_pick else {
        return;
    };
    let title = tutorials::list()
        .into_iter()
        .find(|t| t.id == id)
        .map_or(id, |t| t.title);
    let recent: Vec<PathBuf> = hub
        .config
        .recent
        .iter()
        .filter(|r| r.exists())
        .take(PICK_MAX)
        .map(|r| r.path.clone())
        .collect();
    let playground = hub.config.projects_folder().join(PLAYGROUND);
    let mut choice: Option<Option<PathBuf>> = None;
    let modal = egui::Modal::new(egui::Id::new("hub_tutorial_pick")).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.label(
            RichText::new(format!("Start “{title}”"))
                .size(16.0)
                .strong(),
        );
        ui.label(
            RichText::new("Open it in which project? It starts on a new, untitled scene.")
                .color(p.dim),
        );
        ui.add_space(8.0);
        for path in &recent {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            let button = egui::Button::new(RichText::new(name).strong())
                .min_size(egui::vec2(ui.available_width(), 30.0));
            if ui
                .add(button)
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                choice = Some(Some(path.clone()));
            }
        }
        ui.add_space(6.0);
        let in_list = recent.iter().any(|r| config::same_path(r, &playground));
        if !in_list {
            let label = if playground.join("Assets").is_dir() {
                format!("Open {PLAYGROUND}")
            } else {
                format!("New {PLAYGROUND} (Floating Platform template)")
            };
            if ui.button(label).clicked() {
                choice = Some(None);
            }
        }
        ui.add_space(6.0);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if ui.button("Cancel").clicked() {
                ui.close();
            }
        });
    });
    match choice {
        Some(Some(path)) => hub.open_tutorial_in(&path, id),
        Some(None) => hub.open_tutorial_in_playground(id),
        None if modal.should_close() => hub.tutorial_pick = None,
        None => {}
    }
}
