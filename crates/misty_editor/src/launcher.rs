//! The window's app: the Hub until a project is chosen, then the editor, in the same process
//! and window (the Hub's look is reset and the window maximised for the editor). File ▸ Open
//! Misty Hub goes back the other way once the editor's save prompt is answered.

use eframe::App as _;

use crate::app::EditorApp;
use crate::hub::Hub;

pub enum Launcher {
    Hub(Box<Hub>),
    Editor(Box<EditorApp>),
}

impl Launcher {
    fn hand_over(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        project: std::path::PathBuf,
        tutorial: Option<String>,
    ) {
        // Drop the Hub's spacing and text sizes before the editor theme goes on.
        ctx.all_styles_mut(|s| *s = egui::Style::default());
        let mut editor = EditorApp::start(ctx, frame.wgpu_render_state().cloned(), project);
        if let Some(id) = tutorial {
            editor.start_tutorial(&id);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            960.0, 540.0,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        ctx.request_repaint();
        *self = Self::Editor(Box::new(editor));
    }

    /// Close the project (layout saved as on exit) and show the Hub at its own size.
    fn back_to_hub(&mut self, ctx: &egui::Context, new_project: bool) {
        if let Self::Editor(e) = self {
            e.on_exit();
        }
        crate::tutorials::targets::set_enabled(false);
        ctx.all_styles_mut(|s| *s = egui::Style::default());
        // No env hooks here: `MISTY_HUB_OPEN` would reopen the project straight away.
        let mut hub = Hub::with_config(ctx, crate::hub::config::config_path());
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

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        match self {
            Self::Hub(hub) => {
                if let Some(project) = hub.ui(ui) {
                    let tutorial = hub.take_tutorial();
                    self.hand_over(&ctx, frame, project, tutorial);
                }
            }
            Self::Editor(editor) => {
                editor.ui(ui, frame);
                if let Some(new_project) = editor.take_open_hub() {
                    self.back_to_hub(&ctx, new_project);
                }
            }
        }
    }
}
