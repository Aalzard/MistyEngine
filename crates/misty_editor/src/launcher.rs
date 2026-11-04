//! The window's app: the Hub until a project is chosen, then the editor, in the same process
//! and window (the Hub's look is reset and the window maximised for the editor).

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
    ) {
        // Drop the Hub's spacing and text sizes before the editor theme goes on.
        ctx.all_styles_mut(|s| *s = egui::Style::default());
        let editor = EditorApp::start(ctx, frame.wgpu_render_state().cloned(), project);
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            960.0, 540.0,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        ctx.request_repaint();
        *self = Self::Editor(Box::new(editor));
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
        match self {
            Self::Hub(hub) => {
                if let Some(project) = hub.ui(ui) {
                    let ctx = ui.ctx().clone();
                    self.hand_over(&ctx, frame, project);
                }
            }
            Self::Editor(editor) => editor.ui(ui, frame),
        }
    }
}
