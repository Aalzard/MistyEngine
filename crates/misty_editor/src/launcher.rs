//! The window's app, one for every way the editor starts:
//!
//! - `misty` → the **Hub** until a project is chosen;
//! - a chosen project, or `misty <project>` → **Loading**: the splash (`branding::splash`) while
//!   the project loads, skipped for projects that opened quickly last time;
//! - → the **Editor**. File ▸ Open Misty Hub goes back to the Hub once the save prompt is
//!   answered.
//!
//! All in the same process and window: the Hub's look is reset and the window maximised for the
//! editor, and the other way round.

use std::path::PathBuf;
use std::time::Instant;

use eframe::App as _;

use crate::app::EditorApp;
use crate::branding::splash;
use crate::hub::Hub;
use crate::updater::Updater;

pub enum Launcher {
    Hub(Box<Hub>),
    /// The project being opened, with the splash on screen for `frames` more frames first.
    Loading {
        project: PathBuf,
        /// Learn ▸ Start in the Hub: the tutorial to start once loaded.
        tutorial: Option<String>,
        frames: u32,
        /// `MISTY_SPLASH=1` with `MISTY_SCREENSHOT`: capture the splash itself (never loads).
        capture: Option<Box<crate::capture::Capture>>,
        /// The process's one updater, handed to the editor (boxed: it is large).
        updater: Box<Updater>,
    },
    Editor(Box<EditorApp>),
}

impl Launcher {
    /// The process's updater: from the environment, checking in the background. Made once, at
    /// launch, then passed Hub -> editor -> Hub so only one check ever runs.
    pub fn updater(ctx: &egui::Context) -> Updater {
        let mut u = Updater::from_env();
        u.start(ctx);
        u
    }

    /// Open `project` (the splash first unless it opened quickly last time).
    pub fn open(project: PathBuf, tutorial: Option<String>, updater: Updater) -> Self {
        let frames = if splash::wants_splash(&project) {
            splash::SPLASH_FRAMES
        } else {
            0
        };
        Self::Loading {
            project,
            tutorial,
            frames,
            capture: std::env::var_os("MISTY_SPLASH")
                .and_then(|_| crate::capture::Capture::from_env())
                .map(Box::new),
            updater: Box::new(updater),
        }
    }

    /// The Hub chose a project: the editor's window, then load it.
    fn hand_over(
        &mut self,
        ctx: &egui::Context,
        project: PathBuf,
        tutorial: Option<String>,
        updater: Updater,
    ) {
        // Drop the Hub's spacing and text sizes; the editor theme goes on with the editor.
        ctx.all_styles_mut(|s| *s = egui::Style::default());
        crate::theme::apply(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            960.0, 540.0,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            crate::branding::PRODUCT.into(),
        ));
        ctx.request_repaint();
        *self = Self::open(project, tutorial, updater);
    }

    /// Load the project now and show the editor (its first frame in this one).
    fn load(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let Self::Loading {
            project,
            tutorial,
            updater,
            ..
        } = self
        else {
            return;
        };
        let (project, tutorial) = (std::mem::take(project), tutorial.take());
        let updater = std::mem::take(&mut **updater);
        let started = Instant::now();
        let mut editor = EditorApp::start(
            ui.ctx(),
            frame.wgpu_render_state().cloned(),
            project.clone(),
            updater,
        );
        if let Some(id) = tutorial {
            editor.start_tutorial(&id);
        }
        let took = started.elapsed();
        editor.note_load_time(took);
        splash::remember(&project, took.as_millis() as u64);
        editor.ui(ui, frame);
        *self = Self::Editor(Box::new(editor));
    }

    /// Close the project (layout saved as on exit) and show the Hub at its own size.
    fn back_to_hub(&mut self, ctx: &egui::Context, new_project: bool) {
        let mut updater = Updater::disabled();
        if let Self::Editor(e) = self {
            e.on_exit();
            updater = e.take_updater();
        }
        crate::tutorials::targets::set_enabled(false);
        ctx.all_styles_mut(|s| *s = egui::Style::default());
        // No env hooks here: `MISTY_HUB_OPEN` would reopen the project straight away.
        let mut hub = Hub::with_config(ctx, crate::hub::config::config_path());
        hub.updater = updater;
        if new_project {
            hub.start_new_project();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            880.0, 540.0,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1180.0, 720.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("Misty Hub".into()));
        ctx.request_repaint();
        *self = Self::Hub(Box::new(hub));
    }
}

impl eframe::App for Launcher {
    fn on_exit(&mut self) {
        if let Self::Editor(e) = self {
            e.on_exit();
        }
    }

    /// Only the window position/size is persisted by eframe; editor state lives in the project.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        match self {
            Self::Hub(hub) => {
                if let Some(project) = hub.ui(ui) {
                    let tutorial = hub.take_tutorial();
                    let updater = std::mem::take(&mut hub.updater);
                    // Loads on the next frame (this one already has the Hub's panels).
                    self.hand_over(&ctx, project, tutorial, updater);
                }
            }
            Self::Loading {
                project,
                frames,
                capture,
                ..
            } => {
                if let Some(capture) = capture {
                    splash::paint(ui, project);
                    capture.update(&ctx);
                } else if *frames > 0 {
                    *frames -= 1;
                    splash::paint(ui, project);
                    ctx.request_repaint();
                } else {
                    self.load(ui, frame);
                }
            }
            Self::Editor(editor) => {
                editor.ui(ui, frame);
                if let Some(new_project) = editor.take_open_hub() {
                    self.back_to_hub(&ctx, new_project);
                }
            }
        }
        // The window is frameless (`main.rs`): its border resize zones, over everything.
        crate::title_bar::resize_edges(&ctx);
    }
}
