//! New Project page: template cards on the left; preview, name and location on the right.
//! The project is created on a worker thread (so the window stays responsive).

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use egui::{Align, Layout, RichText, Sense, UiBuilder, vec2};

use super::{Hub, style, validate};
use crate::i18n::tr;
use crate::templates::{self, Template};

pub struct NewProject {
    pub template: Template,
    pub name: String,
    pub location: String,
    /// Copy all sample scenes into the new project as well.
    pub include_samples: bool,
    error: Option<String>,
    job: Option<JoinHandle<Result<PathBuf, String>>>,
}

impl NewProject {
    pub fn new(location: &Path) -> Self {
        Self {
            template: Template::default(),
            name: validate::unique_name("My project", location),
            location: location.display().to_string(),
            include_samples: false,
            error: None,
            job: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// The folder to create, or why not.
    pub fn target(&self) -> Result<PathBuf, String> {
        validate::project_folder(&self.name, Path::new(self.location.trim()))
    }

    fn start(&mut self) {
        match self.target() {
            Ok(folder) => {
                let template = self.template;
                let samples = self.include_samples;
                self.error = None;
                self.job = Some(std::thread::spawn(move || {
                    let result = templates::create_with(&folder, template, samples);
                    if result.is_err() {
                        // Don't leave a half-made project behind (the folder was new).
                        let _ = std::fs::remove_dir_all(&folder);
                    }
                    result.map(|()| folder)
                }));
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The finished project, once the worker is done.
    fn poll(&mut self) -> Option<PathBuf> {
        if !self.job.as_ref().is_some_and(|j| j.is_finished()) {
            return None;
        }
        match self.job.take()?.join() {
            Ok(Ok(folder)) => Some(folder),
            Ok(Err(e)) => {
                self.error = Some(format!(
                    "Could not create the project: {e}{}",
                    blocked_hint(&e)
                ));
                None
            }
            Err(_) => {
                self.error = Some("Creating the project failed unexpectedly.".into());
                None
            }
        }
    }
}

/// Windows Security's "Controlled folder access" (ransomware protection) blocks unknown apps
/// from writing into Documents, Desktop and Pictures, and the app sees "file not found" or
/// "access denied". Say so instead of leaving the user with a bare OS error.
fn blocked_hint(error: &str) -> &'static str {
    let blocked = ["(os error 2)", "(os error 3)", "(os error 5)"]
        .iter()
        .any(|code| error.contains(code));
    if cfg!(windows) && blocked {
        "\nIf Windows Security's Controlled folder access is on, allow Misty Engine \
         (Windows Security ▸ Virus & threat protection ▸ Ransomware protection) or choose a \
         location outside Documents."
    } else {
        ""
    }
}

pub fn show(ui: &mut egui::Ui, hub: &mut Hub) {
    let Some(mut np) = hub.new_project.take() else {
        return;
    };
    let mut keep = true;
    if let Some(folder) = np.poll() {
        hub.open_project(&folder);
        if hub.open.is_none() {
            np.error = hub.notice.take();
        } else {
            keep = false;
        }
    }
    if np.busy() {
        ui.ctx().request_repaint();
    }
    let p = style::palette(hub.config.theme);
    ui.horizontal(|ui| {
        ui.heading(RichText::new(tr!("New project")).strong());
    });
    ui.label(RichText::new(tr!("Pick a template to start from.")).color(p.dim));
    ui.add_space(12.0);

    let footer = 52.0;
    let body = ui.available_size() - vec2(0.0, footer);
    let list_width = (body.x * 0.42).clamp(300.0, 420.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            vec2(list_width, body.y),
            Layout::top_down(Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("templates")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        for t in Template::ALL {
                            let available = t.available();
                            let resp = card(ui, hub, &p, t, np.template == t, available);
                            if resp.clicked() && !np.busy() && available {
                                np.template = t;
                                np.error = None;
                            }
                        }
                    });
            },
        );
        ui.add_space(16.0);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), body.y),
            Layout::top_down(Align::Min),
            |ui| details(ui, hub, &p, &mut np),
        );
    });

    // Footer: Cancel / Create.
    ui.add_space(8.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let ok = np.target().is_ok();
        let create = ui.add_enabled(ok && !np.busy(), style::primary(&tr!("Create project")));
        if create.clicked() {
            np.start();
        }
        if ui
            .add_enabled(!np.busy(), egui::Button::new(tr!("Cancel")))
            .clicked()
        {
            keep = false;
        }
        if np.busy() {
            ui.add(egui::Spinner::new());
            ui.label(RichText::new(tr!("Creating project…")).color(p.dim));
        }
    });
    if keep {
        hub.new_project = Some(np);
    }
}

/// A template card: thumbnail, name and a one-line description (dimmed when this install
/// can't make the template).
fn card(
    ui: &mut egui::Ui,
    hub: &mut Hub,
    p: &style::Palette,
    t: Template,
    selected: bool,
    available: bool,
) -> egui::Response {
    let height = 96.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let fill = if (selected || resp.hovered()) && available {
        p.card_hover
    } else {
        p.card
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let stroke = if selected {
        egui::Stroke::new(2.0, style::ACCENT)
    } else {
        egui::Stroke::new(1.0, p.border)
    };
    ui.painter()
        .rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Inside);
    // Mirrored in a right-to-left UI: the picture on the right, the text right-aligned.
    let rtl = crate::i18n::is_rtl();
    let row = if rtl {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink(10.0)).layout(row));
    if !available {
        child.multiply_opacity(0.45);
    }
    if let Some(tex) = hub.template_texture(child.ctx(), t) {
        child.add(
            egui::Image::new(&tex)
                .fit_to_exact_size(vec2(112.0, 63.0))
                .corner_radius(5.0),
        );
    }
    child.add_space(4.0);
    let column = Layout::top_down(if rtl { Align::Max } else { Align::Min });
    child.with_layout(column, |ui| {
        ui.add_space(2.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        // Not selectable: a click on the text picks the card.
        let name = RichText::new(tr(t.name())).size(15.0).strong();
        ui.add(egui::Label::new(name).selectable(false));
        let text = crate::i18n::tr_wrapped(ui, t.description(), ui.available_width());
        let description = RichText::new(text).small().color(p.dim);
        ui.add(egui::Label::new(description).wrap().selectable(false));
    });
    if available {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp.on_hover_text(tr!("This install has no sample project to copy it from."))
    }
}

/// Large preview, description and the name / location fields.
fn details(ui: &mut egui::Ui, hub: &mut Hub, p: &style::Palette, np: &mut NewProject) {
    let t = np.template;
    let width = ui.available_width();
    let preview_h = (width * 9.0 / 16.0)
        .min(ui.available_height() - 210.0)
        .max(120.0);
    if let Some(tex) = hub.template_texture(ui.ctx(), t) {
        let w = preview_h * 16.0 / 9.0;
        ui.add(
            egui::Image::new(&tex)
                .fit_to_exact_size(vec2(w.min(width), preview_h))
                .corner_radius(8.0),
        );
    }
    ui.add_space(4.0);
    ui.label(RichText::new(tr(t.name())).size(18.0).strong());
    let text = crate::i18n::tr_wrapped(ui, t.description(), width);
    ui.label(RichText::new(text).color(p.dim));
    ui.add_space(10.0);
    let busy = np.busy();
    egui::Grid::new("new_project_fields")
        .num_columns(2)
        .spacing(vec2(12.0, 10.0))
        .show(ui, |ui| {
            ui.label(tr!("Project name"));
            let name = ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut np.name).desired_width(width - 130.0),
            );
            if name.changed() {
                np.error = None;
            }
            ui.end_row();
            ui.label(tr!("Location"));
            ui.horizontal(|ui| {
                let location = ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut np.location).desired_width(width - 230.0),
                );
                if location.changed() {
                    np.error = None;
                }
                if ui
                    .add_enabled(!busy, egui::Button::new(tr!("Browse…")))
                    .clicked()
                    && let Some(dir) = super::pick_folder(Path::new(np.location.trim()))
                {
                    np.location = dir.display().to_string();
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    // Off by default: a fresh project stays small.
    let have_samples = templates::samples::source().is_some();
    let samples = ui
        .add_enabled(
            !busy && have_samples,
            egui::Checkbox::new(&mut np.include_samples, tr!("Include sample scenes")),
        )
        .on_hover_text(tr!(
            "Adds every demo scene (vehicles, Timeline, terrain, effects…) and what they use \
             under Assets, and lists them in Build Settings (unticked). About 30 MB."
        ));
    if !have_samples {
        samples.on_disabled_hover_text(tr!("This install has no sample scenes."));
    }
    ui.add_space(4.0);
    match np.error.clone().map(Err).unwrap_or_else(|| np.target()) {
        Err(e) => {
            ui.colored_label(style::ERROR, e);
        }
        Ok(folder) => {
            let text = RichText::new(tr!("Creates {folder}", folder = folder.display()))
                .small()
                .color(p.dim);
            ui.add(egui::Label::new(text).truncate());
        }
    }
}
