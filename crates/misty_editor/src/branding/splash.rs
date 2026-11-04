//! Startup splash: the window opens on a centred logo, and the project loads on the next
//! frames while it is on screen (instead of a blank window during the load).
//!
//! Nothing waits on purpose: the load starts as soon as the splash has been presented, and the
//! editor replaces it in the same frame the load finishes. The last open time is kept in
//! `Library/EditorStartup.txt`; projects that opened quickly last time skip the splash.

use std::path::PathBuf;
use std::time::Instant;

use egui::{Align2, Color32, FontId, Rect, Vec2};

use crate::app::EditorApp;

/// Frames drawn before loading: the first presented frame also makes the window visible.
const SPLASH_FRAMES: u32 = 2;
/// Loads shorter than this (last time) open without the splash.
const SPLASH_MIN_MS: u64 = 400;
/// The last open time, per project.
const STARTUP_FILE: &str = "Library/EditorStartup.txt";

/// The eframe app: the splash until the editor is loaded, then the editor.
pub struct Launcher {
    stage: Stage,
    /// `MISTY_SPLASH=1` with `MISTY_SCREENSHOT`: capture the splash itself (never loads).
    splash_capture: Option<crate::capture::Capture>,
}

enum Stage {
    Splash {
        project: PathBuf,
        wgpu: Option<egui_wgpu::RenderState>,
        /// Splash frames still to draw.
        frames: u32,
    },
    Editor(Box<EditorApp>),
}

impl Launcher {
    pub fn new(cc: &eframe::CreationContext<'_>, project: PathBuf) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let frames = if wants_splash(&project) {
            SPLASH_FRAMES
        } else {
            0
        };
        Self {
            stage: Stage::Splash {
                project,
                wgpu: cc.wgpu_render_state.clone(),
                frames,
            },
            splash_capture: std::env::var_os("MISTY_SPLASH")
                .and_then(|_| crate::capture::Capture::from_env()),
        }
    }
}

impl eframe::App for Launcher {
    fn on_exit(&mut self) {
        if let Stage::Editor(app) = &mut self.stage {
            app.on_exit();
        }
    }

    /// Only the window position/size is persisted by eframe; editor state lives in the project.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let (Some(capture), Stage::Splash { project, .. }) =
            (&mut self.splash_capture, &self.stage)
        {
            paint(ui, project);
            capture.update(ui.ctx());
            return;
        }
        match &mut self.stage {
            Stage::Editor(app) => app.ui(ui, frame),
            Stage::Splash {
                frames, project, ..
            } if *frames > 0 => {
                *frames -= 1;
                paint(ui, project);
                ui.ctx().request_repaint();
            }
            Stage::Splash { project, wgpu, .. } => {
                let started = Instant::now();
                let project = std::mem::take(project);
                let mut app = EditorApp::new(wgpu.take(), project.clone());
                let took = started.elapsed();
                app.note_load_time(took);
                remember(&project, took.as_millis() as u64);
                self.stage = Stage::Editor(Box::new(app));
                // The editor's first frame right away.
                if let Stage::Editor(app) = &mut self.stage {
                    app.ui(ui, frame);
                }
            }
        }
    }
}

/// Show the splash unless the project opened quickly last time (unknown: show it).
fn wants_splash(project: &std::path::Path) -> bool {
    std::fs::read_to_string(project.join(STARTUP_FILE))
        .ok()
        .and_then(|t| t.trim().parse::<u64>().ok())
        .is_none_or(|ms| ms >= SPLASH_MIN_MS)
}

fn remember(project: &std::path::Path, ms: u64) {
    let path = project.join(STARTUP_FILE);
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(path, ms.to_string());
    }
}

/// The splash: logo, name and the project being opened, on the tab-bar background.
fn paint(ui: &mut egui::Ui, project: &std::path::Path) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, crate::theme::TAB_BAR_BG);
    let logo = super::logo_texture(ui.ctx());
    let side = (rect.height() * 0.28).clamp(96.0, 220.0);
    let center = rect.center() - Vec2::new(0.0, side * 0.25);
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(
        logo.id(),
        Rect::from_center_size(center, Vec2::splat(side)),
        uv,
        Color32::WHITE,
    );
    let below = center.y + side * 0.5 + 8.0;
    painter.text(
        egui::pos2(center.x, below),
        Align2::CENTER_TOP,
        super::PRODUCT,
        FontId::proportional(26.0),
        Color32::from_gray(235),
    );
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    painter.text(
        egui::pos2(center.x, below + 38.0),
        Align2::CENTER_TOP,
        format!("Opening {name}…"),
        FontId::proportional(13.0),
        crate::theme::TEXT_DIM,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_projects_skip_the_splash() {
        let root = std::env::temp_dir().join(format!("misty_splash_{}", std::process::id()));
        assert!(wants_splash(&root), "first open: unknown time");
        remember(&root, 120);
        assert!(!wants_splash(&root));
        remember(&root, 2500);
        assert!(wants_splash(&root));
        std::fs::remove_dir_all(root).ok();
    }
}
