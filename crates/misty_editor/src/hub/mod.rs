//! Misty Hub: the launcher `misty` opens when started without a project path. Pages: Projects
//! (recent list, New Project, Open), Learn (the editor's tutorials) and Settings. Choosing a project
//! hands the window over to the editor (see `launcher`).
//!
//! Screenshots: `MISTY_HUB_PAGE=projects|new|learn|settings` opens a page and
//! `MISTY_HUB_TEMPLATE=<name>` picks the New Project template; `MISTY_HUB_OPEN=<project>` opens one.

pub mod config;
pub mod learn;
mod new_project;
mod pages;
mod projects;
pub mod style;
#[cfg(test)]
mod tests;
pub mod validate;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use config::{HubConfig, HubTheme};
use egui::{Color32, RichText, TextureHandle};

use crate::i18n::tr;
use crate::templates::Template;

/// The same version the About window and the exe's version info show.
pub const VERSION: &str = crate::branding::VERSION;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Projects,
    Learn,
    Settings,
}

pub struct Hub {
    config_path: Option<PathBuf>,
    pub config: HubConfig,
    page: Page,
    /// The New Project page, shown in place of Projects while open.
    new_project: Option<new_project::NewProject>,
    search: String,
    /// A problem to show above the project list (cleared on the next action).
    notice: Option<String>,
    textures: Textures,
    /// Set once a project is chosen; the launcher then starts the editor.
    open: Option<PathBuf>,
    capture: Option<crate::capture::Capture>,
    applied_theme: Option<HubTheme>,
    /// IDs of finished tutorials (Learn page ✓).
    tutorials_done: std::collections::BTreeSet<String>,
    /// Learn ▸ Start: asking which project to open the tutorial in.
    tutorial_pick: Option<&'static str>,
    /// The tutorial to start once the editor has opened `open`.
    open_tutorial: Option<String>,
    /// The process's updater (`launcher::Launcher::updater`; disabled in tests). The Hub
    /// shows its green Update button and windows, and hands it to the editor with the project.
    pub updater: crate::updater::Updater,
}

#[derive(Default)]
struct Textures {
    templates: HashMap<Template, TextureHandle>,
    /// Project thumbnails by folder (`None` = no thumbnail file).
    projects: HashMap<PathBuf, Option<TextureHandle>>,
}

impl Hub {
    pub fn new(ctx: &egui::Context) -> Self {
        let mut hub = Self::with_config(ctx, config::config_path());
        hub.apply_env();
        hub
    }

    /// A Hub reading and writing `config_path` (`None` = nothing saved).
    pub fn with_config(ctx: &egui::Context, config_path: Option<PathBuf>) -> Self {
        crate::theme::load_fonts(ctx);
        let config = config_path
            .as_deref()
            .map(HubConfig::load)
            .unwrap_or_default();
        let tutorials_done = learn::load_done(config_path.as_deref());
        Self {
            config_path,
            config,
            page: Page::Projects,
            new_project: None,
            search: String::new(),
            notice: None,
            textures: Textures::default(),
            open: None,
            capture: crate::capture::Capture::from_env(),
            applied_theme: None,
            tutorials_done,
            tutorial_pick: None,
            open_tutorial: None,
            updater: crate::updater::Updater::disabled(),
        }
    }

    fn apply_env(&mut self) {
        let page = std::env::var("MISTY_HUB_PAGE").unwrap_or_default();
        match page.trim().to_lowercase().as_str() {
            "learn" => self.page = Page::Learn,
            "settings" => self.page = Page::Settings,
            "new" => self.start_new_project(),
            _ => {}
        }
        if let (Some(np), Ok(name)) = (&mut self.new_project, std::env::var("MISTY_HUB_TEMPLATE"))
            && let Some(t) = Template::from_name(&name)
        {
            np.template = t;
        }
        // `MISTY_HUB_OPEN=<project>` opens it right away (checks the hand-over to the editor).
        if let Some(project) = std::env::var_os("MISTY_HUB_OPEN") {
            self.open_project(Path::new(&project));
        }
    }

    pub fn save_config(&mut self) {
        if let Some(path) = &self.config_path
            && let Err(e) = self.config.save(path)
        {
            self.notice = Some(tr!(
                "Could not save {file}: {error}",
                file = path.display(),
                error = e
            ));
        }
    }

    pub fn start_new_project(&mut self) {
        self.new_project = Some(new_project::NewProject::new(&self.config.projects_folder()));
        self.page = Page::Projects;
    }

    /// Open `project` in the editor (recorded as most recent).
    fn open_project(&mut self, project: &Path) {
        if !project.join("Assets").is_dir() {
            self.notice = Some(tr!(
                "{folder} is not a Misty project (it has no Assets folder).",
                folder = project.display()
            ));
            return;
        }
        self.config.touch(project, config::now());
        self.save_config();
        self.open = Some(project.to_path_buf());
    }

    /// Draw the Hub; returns the project to open once one is chosen.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let ctx = ui.ctx().clone();
        if self.applied_theme != Some(self.config.theme) {
            style::apply(&ctx, self.config.theme);
            self.applied_theme = Some(self.config.theme);
        }
        let p = style::palette(self.config.theme);
        egui::Panel::top("hub_title_bar")
            .exact_size(crate::title_bar::HEIGHT)
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| title_bar(ui, &p, self.config.theme));
        // The page links sit on the reading side: the right in a right-to-left UI.
        let nav = if crate::i18n::is_rtl() {
            egui::Panel::right("hub_nav")
        } else {
            egui::Panel::left("hub_nav")
        };
        nav.exact_size(216.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.nav).inner_margin(egui::Margin {
                left: 16,
                right: 16,
                top: 10,
                bottom: 14,
            }))
            .show(ui, |ui| self.nav(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(32, 24)),
            )
            .show(ui, |ui| {
                if self.new_project.is_some() {
                    new_project::show(ui, self);
                } else {
                    match self.page {
                        Page::Projects => projects::show(ui, self),
                        Page::Learn => learn::show(ui, self),
                        Page::Settings => pages::settings(ui, self),
                    }
                }
            });
        // Update now: the Hub has no scenes to save, so it installs straight away.
        if crate::updater::ui::windows(&ctx, &mut self.updater) {
            self.updater.install();
        }
        if let Some(capture) = &mut self.capture {
            capture.update(&ctx);
        }
        self.open.take()
    }

    /// The tutorial to start in the project [`Hub::ui`] just returned (Learn ▸ Start).
    pub fn take_tutorial(&mut self) -> Option<String> {
        self.open_tutorial.take()
    }

    /// Logo slot, page links and the version footer.
    fn nav(&mut self, ui: &mut egui::Ui) {
        let p = style::palette(self.config.theme);
        ui.horizontal(|ui| {
            // The official logo (the small cut reads better at this size).
            let logo = crate::branding::small_logo_texture(ui.ctx());
            ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::vec2(30.0, 30.0)));
            ui.label(
                RichText::new(crate::branding::PRODUCT)
                    .size(18.0)
                    .strong()
                    .color(p.text),
            );
        });
        ui.add_space(26.0);
        let links = [
            (Page::Projects, "🗀", tr!("Projects")),
            (Page::Learn, "🎓", tr!("Learn")),
            (Page::Settings, "⚙", tr!("Settings")),
        ];
        for (page, icon, label) in links {
            let selected =
                self.page == page && (self.new_project.is_none() || page == Page::Projects);
            if nav_item(ui, &p, icon, &label, selected).clicked() {
                self.page = page;
                if page != Page::Projects && self.new_project.as_ref().is_some_and(|n| !n.busy()) {
                    self.new_project = None;
                }
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("v{VERSION}")).small().color(p.dim));
                if crate::updater::ui::update_button(ui, &self.updater) {
                    self.updater.install();
                }
            });
        });
    }

    fn template_texture(&mut self, ctx: &egui::Context, t: Template) -> Option<TextureHandle> {
        use std::collections::hash_map::Entry;
        match self.textures.templates.entry(t) {
            Entry::Occupied(e) => Some(e.get().clone()),
            Entry::Vacant(e) => {
                let name = format!("hub:template:{}", t.name());
                Some(e.insert(load_png(ctx, &name, t.thumbnail_png())?).clone())
            }
        }
    }

    /// `<project>/Library/HubThumbnail.png`, read once.
    fn project_texture(&mut self, ctx: &egui::Context, project: &Path) -> Option<TextureHandle> {
        if let Some(t) = self.textures.projects.get(project) {
            return t.clone();
        }
        let tex = std::fs::read(project.join(crate::templates::PROJECT_THUMBNAIL))
            .ok()
            .and_then(|bytes| load_png(ctx, &format!("hub:project:{}", project.display()), &bytes));
        self.textures
            .projects
            .insert(project.to_path_buf(), tex.clone());
        tex
    }
}

/// A full-width page link with a highlight when selected.
/// The frameless window's bar: "Misty Hub" in the centre and the window buttons.
fn title_bar(ui: &mut egui::Ui, p: &style::Palette, theme: config::HubTheme) {
    let hover = match theme {
        config::HubTheme::Dark => egui::Color32::from_white_alpha(18),
        config::HubTheme::Light => egui::Color32::from_black_alpha(18),
    };
    let bar = crate::title_bar::BarStyle {
        bg: p.nav,
        text: p.text,
        dim: p.dim,
        hover,
    };
    let title = crate::title_bar::title_job("Misty Hub", "", &bar);
    // No menus: their edge is the near end of the bar.
    crate::title_bar::show(ui, &bar, title, |ui| {
        if crate::i18n::is_rtl() {
            ui.max_rect().right()
        } else {
            ui.max_rect().left()
        }
    });
}

fn nav_item(
    ui: &mut egui::Ui,
    p: &style::Palette,
    icon: &str,
    label: &str,
    selected: bool,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());
    let fill = if selected {
        style::ACCENT.gamma_multiply(0.22)
    } else if resp.hovered() {
        p.card
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    let color = if selected { p.text } else { p.dim };
    let font = egui::FontId::proportional(14.5);
    let y = rect.center().y;
    // Icon then label from the reading side (mirrored in a right-to-left UI).
    let (icon_pos, label_pos, align) = if crate::i18n::is_rtl() {
        (
            egui::pos2(rect.right() - 12.0, y),
            egui::pos2(rect.right() - 40.0, y),
            egui::Align2::RIGHT_CENTER,
        )
    } else {
        (
            egui::pos2(rect.left() + 12.0, y),
            egui::pos2(rect.left() + 40.0, y),
            egui::Align2::LEFT_CENTER,
        )
    };
    ui.painter().text(
        icon_pos,
        align,
        icon,
        font.clone(),
        if selected { style::ACCENT } else { p.dim },
    );
    ui.painter().text(label_pos, align, label, font, color);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn load_png(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<TextureHandle> {
    let img = misty_engine::image::load_from_memory(bytes)
        .ok()?
        .into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let options = egui::TextureOptions {
        mipmap_mode: Some(egui::TextureFilter::Linear),
        ..egui::TextureOptions::LINEAR
    };
    Some(ctx.load_texture(name, color, options))
}

/// A folder picker starting in `dir`.
fn pick_folder(dir: &Path) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new();
    if dir.is_dir() {
        d = d.set_directory(dir);
    }
    d.pick_folder()
}
