//! Misty Engine editor entry point.

// Release builds are GUI apps: no console window behind the Hub and the editor. Started from a
// terminal (`--build`, `--run-tests`…), the editor attaches to that terminal's console instead
// (`attach_parent_console`), so its output still shows there.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod a11y;
mod actions;
mod anim_clips;
mod anim_controllers;
mod api_docs;
mod app;
mod area_light_handles;
mod asset_actions;
mod asset_guids;
mod asset_ops;
mod assets;
mod audio_mixers;
mod avatar_masks;
mod branding;
mod build_pipeline;
mod build_settings;
mod capture;
mod cloth_tools;
mod commands;
mod component_ops;
mod console;
mod editor_scripts;
mod fps_player;
mod game_input;
mod gizmos;
mod handles;
mod hub;
mod i18n;
mod launcher;
mod layout;
mod layouts;
mod light_probe_tools;
mod lightmap_tools;
mod live_reload;
mod material_preview;
mod mcp;
mod menu_bar;
mod mesh_builder;
mod model_import;
mod multiplayer;
mod object_clipboard;
mod open_scenes;
mod packages;
mod panels;
mod particle_preview;
mod physics_debug_view;
mod picking;
mod play;
mod prefab_mode;
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
mod ragdoll_wizard;
mod render_scene;
#[cfg(test)]
mod sample_2d;
#[cfg(test)]
mod sample_addressables;
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
mod sample_audio2;
#[cfg(test)]
mod sample_character_animator;
#[cfg(test)]
mod sample_clouds;
#[cfg(test)]
mod sample_cutscene;
#[cfg(test)]
mod sample_cycles_room;
#[cfg(test)]
mod sample_foliage_lighting;
#[cfg(test)]
mod sample_hdri;
#[cfg(test)]
mod sample_input;
#[cfg(test)]
mod sample_input_system;
#[cfg(test)]
mod sample_light_glow;
#[cfg(test)]
mod sample_light_probes;
#[cfg(test)]
mod sample_lighting;
#[cfg(test)]
mod sample_lightmap;
#[cfg(test)]
mod sample_localization2;
#[cfg(test)]
mod sample_materials;
#[cfg(test)]
mod sample_materials2;
#[cfg(test)]
mod sample_meadow;
#[cfg(test)]
mod sample_mesh_builder;
#[cfg(test)]
mod sample_multiscene;
#[cfg(test)]
mod sample_navigation;
#[cfg(test)]
mod sample_navigation2;
#[cfg(test)]
mod sample_network;
#[cfg(test)]
mod sample_occlusion;
#[cfg(test)]
mod sample_particles;
#[cfg(test)]
mod sample_physics;
#[cfg(test)]
mod sample_physics2;
#[cfg(test)]
mod sample_postfx;
#[cfg(test)]
mod sample_presets;
#[cfg(test)]
mod sample_reflections;
#[cfg(test)]
mod sample_render;
#[cfg(test)]
mod sample_rigging;
#[cfg(test)]
mod sample_rocks;
#[cfg(test)]
mod sample_saveloc;
#[cfg(test)]
mod sample_shader_graph;
#[cfg(test)]
mod sample_shader_graph_examples;
#[cfg(test)]
mod sample_shadows;
#[cfg(test)]
mod sample_skinning;
#[cfg(test)]
mod sample_soft_shadows;
#[cfg(test)]
mod sample_splines;
#[cfg(test)]
mod sample_ssgi;
#[cfg(test)]
mod sample_streaming;
#[cfg(test)]
mod sample_stress;
#[cfg(test)]
mod sample_terrain;
#[cfg(test)]
mod sample_terrain2;
#[cfg(test)]
mod sample_terrain_tools;
#[cfg(test)]
mod sample_tilemap;
#[cfg(test)]
mod sample_timeline;
#[cfg(test)]
mod sample_ui;
#[cfg(test)]
mod sample_ui_showcase;
#[cfg(test)]
mod sample_ui_widgets;
#[cfg(test)]
mod sample_vcam;
#[cfg(test)]
mod sample_vehicles;
#[cfg(test)]
mod sample_vfx;
#[cfg(test)]
mod sample_vfx2;
#[cfg(test)]
mod sample_video_text;
#[cfg(test)]
mod sample_visual_scripting;
#[cfg(test)]
mod sample_volumetric_fog;
#[cfg(test)]
mod sample_water;
#[cfg(test)]
mod sample_world_builder;
mod saveloc;
mod scene_camera;
mod scene_files;
mod scene_icons;
mod scene_quality;
mod search;
mod selection;
mod snap;
mod snap_window;
mod spline_tools;
mod sprite_handles;
mod state;
mod status_bar;
#[cfg(test)]
mod string_literal_lint;
mod templates;
mod terrain_details_ui;
mod terrain_flowers_ui;
mod terrain_place_ui;
mod terrain_rules;
mod terrain_tools;
mod terrain_trees_ui;
#[cfg(test)]
mod test_util;
mod theme;
mod thumbnails;
mod tile_palette;
mod timelines;
mod title_bar;
mod toolbar;
mod tree_lines;
mod tutorials;
mod ui_create;
mod ui_gizmos;
mod ui_rect_tool;
mod undo;
mod undo_names;
mod undo_terrain;
mod updater;
mod vcam_tools;
mod vcs;
mod vfx_preview;
mod vgeo_convert;
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

/// `--build`'s arguments after the project: `[<output folder>] [--profile <name>]`.
fn build_args(rest: &[String]) -> Result<(Option<PathBuf>, Option<String>), String> {
    let mut out = None;
    let mut profile = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--profile" => {
                profile = Some(it.next().ok_or("--profile needs a profile name")?.clone());
            }
            flag if flag.starts_with("--") => return Err(format!("unknown --build option {flag}")),
            _ if out.is_none() => out = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    Ok((out, profile))
}

/// `misty [<project>]`, `misty --new <Template> <folder>` (creates the project, then opens it)
/// or `misty --build <project> <output folder>` (builds the game without a window, then exits)
/// or `misty --run-tests <project> [--filter X] [--results out.xml]` (runs the project's tests)
/// or `misty --api-docs <folder>` (writes the Lua API reference).
fn parse_args() -> Result<Start, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(Start::Hub),
        [flag, template, folder, rest @ ..]
            if flag == "--new" && rest.iter().all(|a| a == "--with-samples") =>
        {
            let t = templates::Template::from_name(template).ok_or_else(|| {
                let names: Vec<&str> = templates::Template::ALL.iter().map(|t| t.name()).collect();
                format!(
                    "unknown template {template:?} (one of {})",
                    names.join(", ")
                )
            })?;
            let folder = PathBuf::from(folder);
            templates::create_with(&folder, t, !rest.is_empty())?;
            Ok(Start::Editor(folder))
        }
        [flag, project, rest @ ..] if flag == "--build" => {
            let (out, profile) = build_args(rest)?;
            match build_pipeline::build_from_cli(
                Path::new(project),
                out.as_deref(),
                profile.as_deref(),
            ) {
                Ok(exe) => {
                    println!("Built {}", exe.display());
                    std::process::exit(0)
                }
                Err(e) => Err(format!("build failed: {e}")),
            }
        }
        // A restart into a new build (`live_reload::dev_build`): the session to put back.
        [flag, file] if flag == "--restore" => {
            let project = live_reload::restore::set_pending(Path::new(file))?;
            Ok(Start::Editor(project))
        }
        // MCP server for AI agents over stdio (see `mcp`): the project without a window, or
        // a proxy to the running editor.
        [flag, rest @ ..] if flag == "--mcp" => std::process::exit(mcp::headless::main(rest)),
        [flag, rest @ ..] if flag == "--mcp-connect" => {
            std::process::exit(mcp::bridge::connect_main(rest))
        }
        // The Lua API reference (Markdown + one HTML page) into a folder.
        [flag, out] if flag == "--api-docs" => std::process::exit(api_docs::cli(Path::new(out))),
        [flag, rest @ ..] if flag == "--run-tests" => {
            // Headless test run for CI: JUnit XML and the exit code (see `testing::cli`).
            let code = misty_engine::testing::cli::run(rest, &mut std::io::stdout());
            std::process::exit(code)
        }
        [flag, ..] if flag.starts_with("--") => Err(format!(
            "usage: misty [<project folder>] | misty --new <Template> <folder> [--with-samples] | misty --build <project folder> [<output folder>] [--profile <name>] | misty --api-docs <output folder> | {} | {} | {} ({flag}?)",
            misty_engine::testing::cli::USAGE,
            mcp::headless::USAGE,
            mcp::bridge::CONNECT_USAGE
        )),
        [project, ..] => Ok(Start::Editor(PathBuf::from(project))),
    }
}

/// A GUI-subsystem exe has no console; when a terminal started it, write to that terminal's.
/// Does nothing when started from Explorer or a shortcut, or in debug builds (console exe).
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: a plain Win32 call with a constant argument; failure (no parent console) is fine.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_parent_console() {}

fn main() -> eframe::Result<()> {
    capture::mark_process_start();
    // The editor UI's language (`MISTY_EDITOR_LANGUAGE`, else the saved preference).
    i18n::init();
    // Textures draw uncompressed until their compressed blocks are made on a worker (then
    // read from Library/TextureCache).
    misty_engine::sprites::compress::set_mode(misty_engine::sprites::compress::Mode::Background);
    // Live reload's file checks read stamps a background thread keeps fresh (a `stat` per
    // watched file on the main thread made a 10-25 ms frame every half second).
    misty_engine::file_watch::set_background(true);
    // MCP modes talk on the pipes their client gave them; a terminal's console must not replace
    // them.
    if !std::env::args()
        .nth(1)
        .is_some_and(|a| a.starts_with("--mcp"))
    {
        attach_parent_console();
    }
    // `MISTY_PROFILE=capture.jsonl misty …` records the profiler from the first frame into a
    // capture file (JSON Lines, one frame per line), as the player does.
    if let Some(path) = std::env::var_os("MISTY_PROFILE")
        && let Err(e) = misty_engine::profiler::capture::start(std::path::Path::new(&path))
    {
        eprintln!("profiler capture: {e}");
    }
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

/// Frames the CPU may queue ahead of the display (egui's high-throughput 2).
/// `MISTY_FRAME_LATENCY=<1..3>` overrides it for measuring: in MeadowDemo the Scene view is
/// CPU-bound while flying, so 1 measured no lower latency (docs/pending/scene-camera.md).
fn frame_latency() -> u32 {
    std::env::var("MISTY_FRAME_LATENCY")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|n| (1..=3).contains(n))
        .unwrap_or(2)
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
        desc.required_features |= misty_engine::render::pipeline_cache_features(adapter);
        desc.required_limits = misty_engine::render::device_limits(adapter, desc.required_limits);
        desc
    });
    // The Game view's VSync toggle (saved in hub.json; `app::apply_vsync` follows changes).
    let present_mode = match hub::config::editor_vsync() {
        true => egui_wgpu::wgpu::PresentMode::AutoVsync,
        false => egui_wgpu::wgpu::PresentMode::AutoNoVsync,
    };
    egui_wgpu::WgpuConfiguration {
        wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup),
        surface: egui_wgpu::SurfaceConfig {
            present_mode,
            desired_maximum_frame_latency: Some(frame_latency()),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn build_arguments_take_a_folder_and_a_profile() {
        let args = |v: &[&str]| build_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            args(&["C:/Out", "--profile", "Dedicated Server"]).unwrap(),
            (
                Some(PathBuf::from("C:/Out")),
                Some("Dedicated Server".into())
            )
        );
        assert_eq!(
            args(&["--profile", "Demo"]).unwrap(),
            (None, Some("Demo".into()))
        );
        assert_eq!(args(&["Out"]).unwrap(), (Some("Out".into()), None));
        assert!(args(&["--profile"]).is_err());
        assert!(args(&["a", "b"]).is_err());
        assert!(args(&["--zip"]).is_err());
    }
}
