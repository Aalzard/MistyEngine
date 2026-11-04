//! Misty Engine editor entry point.

mod actions;
mod anim_clips;
mod anim_controllers;
mod app;
mod asset_actions;
mod asset_ops;
mod assets;
mod audio_mixers;
mod build_settings;
mod capture;
mod commands;
mod component_ops;
mod console;
mod game_input;
mod gizmos;
mod handles;
mod layout;
mod layouts;
mod material_preview;
mod menu_bar;
mod model_import;
mod object_clipboard;
mod panels;
mod particle_preview;
mod picking;
mod play;
mod prefabs;
mod probe_gizmos;
mod probe_tools;
mod project;
mod project_settings;
mod project_settings_2d;
mod project_settings_input;
mod project_settings_quality;
mod project_settings_tags;
mod render_scene;
#[cfg(test)]
mod sample_2d;
#[cfg(test)]
mod sample_animation;
#[cfg(test)]
mod sample_animator;
#[cfg(test)]
mod sample_assets;
#[cfg(test)]
mod sample_audio;
#[cfg(test)]
mod sample_input;
#[cfg(test)]
mod sample_lighting;
#[cfg(test)]
mod sample_materials;
#[cfg(test)]
mod sample_navigation;
#[cfg(test)]
mod sample_particles;
#[cfg(test)]
mod sample_physics;
#[cfg(test)]
mod sample_reflections;
#[cfg(test)]
mod sample_shadows;
#[cfg(test)]
mod sample_stress;
#[cfg(test)]
mod sample_terrain;
#[cfg(test)]
mod sample_ui;
#[cfg(test)]
mod sample_ui_widgets;
#[cfg(test)]
mod sample_volumetric_fog;
mod scene_camera;
mod scene_files;
mod selection;
mod snap;
mod snap_window;
mod sprite_handles;
mod state;
mod status_bar;
mod terrain_tools;
mod theme;
mod thumbnails;
mod toolbar;
mod ui_create;
mod ui_gizmos;
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
        wgpu_options: wgpu_options(),
        ..Default::default()
    };

    eframe::run_native(
        "Misty Engine",
        options,
        Box::new(move |cc| Ok(Box::new(app::EditorApp::new(cc, project)))),
    )
}

/// egui's device setup plus timestamp queries when available (Profiler GPU times).
fn wgpu_options() -> egui_wgpu::WgpuConfiguration {
    let egui_wgpu::WgpuSetup::CreateNew(mut setup) = egui_wgpu::WgpuSetup::without_display_handle()
    else {
        return Default::default();
    };
    let base = setup.device_descriptor.clone();
    setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut desc = base(adapter);
        desc.required_features |= misty_engine::render::timestamp_features(adapter);
        desc
    });
    egui_wgpu::WgpuConfiguration {
        wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
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
