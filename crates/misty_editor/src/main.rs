//! Misty Engine editor entry point.

mod actions;
mod app;
mod assets;
mod capture;
mod commands;
mod console;
mod game_input;
mod gizmos;
mod handles;
mod layout;
mod menu_bar;
mod panels;
mod picking;
mod play;
mod project;
mod project_settings;
mod render_scene;
mod scene_camera;
mod scene_files;
mod selection;
mod state;
mod status_bar;
mod theme;
mod toolbar;
mod undo;
mod viewport;
mod widgets;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // Project folder: first CLI argument, or the bundled sample project.
    let project = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_project);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Misty Engine")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([960.0, 540.0])
            .with_maximized(true),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Misty Engine",
        options,
        Box::new(move |cc| Ok(Box::new(app::EditorApp::new(cc, project)))),
    )
}

/// Find `SampleProject` next to the working directory or the executable, searching upwards.
fn default_project() -> PathBuf {
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from)),
    ];
    starts
        .into_iter()
        .flatten()
        .flat_map(|start| {
            start
                .ancestors()
                .map(|a| a.join("SampleProject"))
                .collect::<Vec<_>>()
        })
        .find(|p| p.join("Assets").is_dir())
        .unwrap_or_else(|| PathBuf::from("SampleProject"))
}
