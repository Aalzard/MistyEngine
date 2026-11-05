//! Misty Engine editor entry point.

mod actions;
mod anim_clips;
mod anim_controllers;
mod app;
mod area_light_handles;
mod asset_actions;
mod asset_guids;
mod asset_ops;
mod assets;
mod audio_mixers;
mod branding;
mod build_pipeline;
mod build_settings;
mod capture;
mod commands;
mod component_ops;
mod console;
mod game_input;
mod gizmos;
mod handles;
mod hub;
mod launcher;
mod layout;
mod layouts;
mod material_preview;
mod menu_bar;
mod model_import;
mod object_clipboard;
mod open_scenes;
mod panels;
mod particle_preview;
mod picking;
mod play;
mod prefabs;
mod presets;
mod probe_gizmos;
mod probe_tools;
mod project;
mod project_settings;
mod project_settings_2d;
mod project_settings_input;
mod project_settings_player;
mod project_settings_presets;
mod project_settings_quality;
mod project_settings_tags;
mod project_thumbnail;
mod render_scene;
#[cfg(test)]
mod sample_2d;
#[cfg(test)]
mod sample_animation;
#[cfg(test)]
mod sample_animator;
#[cfg(test)]
mod sample_ao;
#[cfg(test)]
mod sample_area_lights;
#[cfg(test)]
mod sample_assets;
#[cfg(test)]
mod sample_audio;
#[cfg(test)]
mod sample_clouds;
#[cfg(test)]
mod sample_hdri;
#[cfg(test)]
mod sample_input;
#[cfg(test)]
mod sample_light_glow;
#[cfg(test)]
mod sample_lighting;
#[cfg(test)]
mod sample_materials;
#[cfg(test)]
mod sample_multiscene;
#[cfg(test)]
mod sample_navigation;
#[cfg(test)]
mod sample_particles;
#[cfg(test)]
mod sample_physics;
#[cfg(test)]
mod sample_presets;
#[cfg(test)]
mod sample_reflections;
#[cfg(test)]
mod sample_render;
#[cfg(test)]
mod sample_shadows;
#[cfg(test)]
mod sample_skinning;
#[cfg(test)]
mod sample_stress;
#[cfg(test)]
mod sample_terrain;
#[cfg(test)]
mod sample_terrain2;
#[cfg(test)]
mod sample_ui;
#[cfg(test)]
mod sample_ui_widgets;
#[cfg(test)]
mod sample_volumetric_fog;
#[cfg(test)]
mod sample_water;
#[cfg(test)]
mod sample_world_builder;
mod scene_camera;
mod scene_files;
mod scene_icons;
mod scene_quality;
mod search;
mod selection;
mod snap;
mod snap_window;
mod sprite_handles;
mod state;
mod status_bar;
mod templates;
mod terrain_rules;
mod terrain_tools;
mod theme;
mod thumbnails;
mod title_bar;
mod toolbar;
mod tree_lines;
mod tutorials;
mod ui_create;
mod ui_gizmos;
mod undo;
mod undo_names;
mod undo_terrain;
mod updater;
mod viewport;
mod widgets;

use std::path::{Path, PathBuf};

/// What the command line asks for.
enum Start {
    /// `misty`: the Hub.
    Hub,
    /// `misty <project folder>`: the editor directly.
    Editor(PathBuf),
}

/// `misty [<project>]`, `misty --new <Template> <folder>` (creates the project, then opens it)
/// or `misty --build <project> <output folder>` (builds the game without a window, then exits).
fn parse_args() -> Result<Start, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(Start::Hub),
        [flag, template, folder] if flag == "--new" => {
            let t = templates::Template::from_name(template).ok_or_else(|| {
                let names: Vec<&str> = templates::Template::ALL.iter().map(|t| t.name()).collect();
                format!(
                    "unknown template {template:?} (one of {})",
                    names.join(", ")
                )
            })?;
            let folder = PathBuf::from(folder);
            templates::create(&folder, t)?;
            Ok(Start::Editor(folder))
        }
        [flag, project, out] if flag == "--build" => {
            match build_pipeline::build_from_cli(Path::new(project), Path::new(out)) {
                Ok(exe) => {
                    println!("Built {}", exe.display());
                    std::process::exit(0)
                }
                Err(e) => Err(format!("build failed: {e}")),
            }
        }
        [flag, ..] if flag.starts_with("--") => Err(format!(
            "usage: misty [<project folder>] | misty --new <Template> <folder> | misty --build <project folder> <output folder> ({flag}?)"
        )),
        [project, ..] => Ok(Start::Editor(PathBuf::from(project))),
    }
}

fn main() -> eframe::Result<()> {
    let start = match parse_args() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("misty: {e}");
            std::process::exit(2);
        }
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title(branding::PRODUCT)
        .with_icon(branding::window_icon())
        // Frameless: the Hub and the editor draw their own title bar (`title_bar`). The title
        // is still set, for the taskbar.
        .with_decorations(false)
        .with_resizable(true);
    let (viewport, persist_window) = match &start {
        Start::Editor(project) => {
            // Automated captures don't touch the user's recent list.
            if std::env::var_os("MISTY_SCREENSHOT").is_none() {
                hub::config::record_open(project);
            }
            let v = viewport
                .with_inner_size([1600.0, 900.0])
                .with_min_inner_size([960.0, 540.0])
                .with_maximized(true);
            (v, true)
        }
        // The Hub always opens at its own size (the editor's saved window would be maximised).
        Start::Hub => {
            let v = viewport
                .with_title("Misty Hub")
                .with_inner_size([1180.0, 720.0])
                .with_min_inner_size([880.0, 540.0]);
            (v, false)
        }
    };
    let options = eframe::NativeOptions {
        viewport,
        persist_window,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_options(),
        ..Default::default()
    };

    // eframe restores the window geometry saved under the app name, so the Hub keeps its own
    // name (and saves none) to always open at its size rather than the editor's maximised one.
    let app_name = match start {
        Start::Hub => "Misty Hub",
        Start::Editor(_) => branding::PRODUCT,
    };
    eframe::run_native(
        app_name,
        options,
        Box::new(move |cc| {
            Ok(Box::new(match start {
                Start::Hub => {
                    let mut hub = hub::Hub::new(&cc.egui_ctx);
                    hub.updater = launcher::Launcher::updater(&cc.egui_ctx);
                    launcher::Launcher::Hub(Box::new(hub))
                }
                Start::Editor(project) => {
                    // The editor's look under the splash (the Hub sets its own).
                    theme::apply(&cc.egui_ctx);
                    let updater = launcher::Launcher::updater(&cc.egui_ctx);
                    launcher::Launcher::open(project, None, updater)
                }
            }))
        }),
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
