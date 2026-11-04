//! File ▸ Build Settings: the "Scenes In Build" list, the platform, build options and
//! Build / Build And Run. Changes are saved to `ProjectSettings/BuildSettings.json` right away.
//!
//! Building runs in the background (see [`crate::build_pipeline`]): cargo compiles the player
//! for the platform, then the scenes and the assets they use are packed into
//! `<Product>_Data/data.pak` next to the renamed executable.

use std::path::Path;

use egui::{RichText, Ui};
use misty_engine::build::{BuildSettings, BuildTarget};

use crate::actions::Action;
use crate::build_pipeline::{BuildJob, PackageRequest, targets};
use crate::console::LogLevel;
use crate::state::EditorState;

/// Window state.
#[derive(Default)]
pub struct BuildSettingsWindow {
    pub open: bool,
    /// Selected row of the scene list.
    pub selected: Option<usize>,
    /// The running or last build.
    pub job: Option<BuildJob>,
}

/// Build settings of the project at `s.project.root`; problems go to the Console.
pub fn load(s: &mut EditorState) {
    match BuildSettings::load(&s.project.root) {
        Ok(b) => s.build_settings = b,
        Err(e) => {
            s.build_settings = BuildSettings::default();
            s.console
                .warn(format!("BuildSettings.json: {e}. Using defaults."));
        }
    }
}

/// Clamp and save.
pub fn changed(s: &mut EditorState) {
    s.build_settings.sanitize();
    if let Err(e) = s.build_settings.save(&s.project.root) {
        s.console.log(
            LogLevel::Error,
            format!("Could not save Build Settings: {e}"),
        );
    }
}

/// "Add Open Scenes": append every open scene (unsaved ones must be saved first).
pub fn add_open_scene(s: &mut EditorState) {
    let paths: Vec<Option<String>> = s.scene.scenes().iter().map(|e| e.path.clone()).collect();
    if paths.iter().any(Option::is_none) {
        s.console
            .warn("Save the scene before adding it to the build.");
    }
    let mut added = false;
    for rel in paths.into_iter().flatten() {
        added |= s.build_settings.add(&rel);
    }
    if added {
        changed(s);
    }
}

/// Draw the window if it is open, and follow a running build either way.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    poll(ctx, s);
    let mut open = s.build_window.open;
    egui::Window::new("Build Settings")
        .open(&mut open)
        .default_size([620.0, 560.0])
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| contents(ui, s))
        });
    s.build_window.open = open;
}

pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    ui.strong("Scenes In Build");
    let mut edited = scene_list(ui, s);
    ui.horizontal(|ui| {
        let sel = s.build_window.selected;
        let len = s.build_settings.scenes.len();
        let can_up = sel.is_some_and(|i| i > 0);
        let can_down = sel.is_some_and(|i| i + 1 < len);
        if ui
            .add_enabled(can_up, egui::Button::new("⏶"))
            .on_hover_text("Move Up")
            .clicked()
        {
            edited |= move_selected(s, -1);
        }
        if ui
            .add_enabled(can_down, egui::Button::new("⏷"))
            .on_hover_text("Move Down")
            .clicked()
        {
            edited |= move_selected(s, 1);
        }
        if ui
            .add_enabled(sel.is_some(), egui::Button::new("Remove Selected"))
            .clicked()
            && let Some(i) = sel.filter(|i| *i < len)
        {
            s.build_settings.scenes.remove(i);
            s.build_window.selected = None;
            edited = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Add Open Scenes").clicked() {
                add_open_scene(s);
            }
        });
    });
    ui.separator();
    edited |= platform(ui, &mut s.build_settings);
    ui.separator();
    edited |= options(ui, s);
    if edited {
        changed(s);
    }
    ui.separator();
    progress(ui, s);
    let busy = s.build_window.job.as_ref().is_some_and(BuildJob::running);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Build And Run"))
            .on_hover_text("Build into the last folder, then start the game (Ctrl+B)")
            .clicked()
        {
            s.push(Action::BuildAndRun);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Build"))
            .on_hover_text("Choose a folder and build there")
            .clicked()
        {
            s.push(Action::Build);
        }
        if ui.button("Player Settings…").clicked() {
            crate::project_settings::open_at(s, crate::project_settings::Category::Player);
        }
    });
}

/// Target list on the left, the selected target's status on the right.
fn platform(ui: &mut Ui, b: &mut BuildSettings) -> bool {
    let tools = targets::toolchain();
    let host = BuildTarget::host();
    let status = |t| {
        let installed = tools.as_ref().and_then(|t| t.installed.as_deref());
        targets::status(
            t,
            host,
            installed,
            tools.as_ref().is_some_and(|t| t.zigbuild),
        )
    };
    let mut edited = false;
    ui.strong("Platform");
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(190.0);
            for t in BuildTarget::ALL {
                let mark = match &tools {
                    None => "…",
                    Some(_) if status(t).ready() => "✔",
                    Some(_) => "⚠",
                };
                let mut text = format!("{mark}  {}", t.label());
                if Some(t) == host {
                    text.push_str("  (this PC)");
                }
                if ui.selectable_label(b.target == t, text).clicked() && b.target != t {
                    b.target = t;
                    edited = true;
                }
            }
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            let t = b.target;
            ui.label(RichText::new(t.label()).strong());
            ui.weak(format!("Rust target: {}", t.triple()));
            let Some(_) = &tools else {
                ui.weak("Checking installed Rust targets…");
                return;
            };
            let st = status(t);
            if !st.installed {
                ui.colored_label(
                    crate::theme::WARN,
                    "This Rust target isn't installed. Run this in a terminal:",
                );
                let cmd = targets::install_command(t);
                ui.horizontal(|ui| {
                    ui.code(&cmd);
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(cmd.clone());
                    }
                });
                if ui.small_button("Check again").clicked() {
                    targets::refresh_toolchain();
                }
            }
            if let Some(blocker) = &st.blocker {
                ui.colored_label(crate::theme::WARN, blocker);
            } else if st.installed {
                ui.label("Ready to build.");
            }
            if let Some(note) = &st.note {
                ui.weak(note);
            }
        });
    });
    edited
}

fn options(ui: &mut Ui, s: &mut EditorState) -> bool {
    let b = &mut s.build_settings;
    let mut edited = false;
    egui::Grid::new("build_options")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Development Build").on_hover_text(
                "Opens a console window with the log and writes Player.log next to the game. \
                 Turn it off for releases.",
            );
            edited |= ui.checkbox(&mut b.development, "").changed();
            ui.end_row();
            ui.label("Compress Data")
                .on_hover_text("LZ4-compress the packed assets: a smaller download.");
            edited |= ui.checkbox(&mut b.compression, "").changed();
            ui.end_row();
            ui.label("Create .zip").on_hover_text(
                "Also write <Product>-<version>-<platform>.zip next to the build folder, \
                 ready for itch.io and similar stores.",
            );
            edited |= ui.checkbox(&mut b.create_zip, "").changed();
            ui.end_row();
            ui.label("Steam App ID").on_hover_text(
                "Optional. Adds SteamPipe upload scripts in steam/ and, for development \
                 builds, steam_appid.txt.",
            );
            edited |= ui
                .add(
                    egui::TextEdit::singleline(&mut b.steam_app_id)
                        .hint_text("none")
                        .desired_width(120.0),
                )
                .changed();
            ui.end_row();
        });
    let player = s.project_settings.player_or(&s.build_settings);
    ui.weak(format!(
        "Product \"{}\" {} · {}×{} · {}",
        player.product_name_or(&s.project.root),
        player.version,
        player.width,
        player.height,
        player.screen_mode.label()
    ));
    edited
}

/// Progress bar and log of the current / last build.
fn progress(ui: &mut Ui, s: &mut EditorState) {
    let Some(job) = &s.build_window.job else {
        return;
    };
    ui.add(
        egui::ProgressBar::new(job.progress)
            .text(job.stage.clone())
            .animate(job.running()),
    );
    egui::CollapsingHeader::new(format!("Build Log ({} lines)", job.log.len()))
        .id_salt("build_log")
        .default_open(false)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &job.log {
                        let text = RichText::new(&line.text).monospace().small();
                        if line.warning {
                            ui.colored_label(crate::theme::WARN, text);
                        } else {
                            ui.label(text);
                        }
                    }
                });
        });
}

/// Rows: enabled checkbox, path, build index on the right. Returns true if edited.
fn scene_list(ui: &mut Ui, s: &mut EditorState) -> bool {
    let mut edited = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(100.0);
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .max_height(160.0)
            .show(ui, |ui| {
                if s.build_settings.scenes.is_empty() {
                    ui.weak("Add the open scene with \"Add Open Scenes\".");
                }
                let mut index = 0;
                for i in 0..s.build_settings.scenes.len() {
                    let row = &mut s.build_settings.scenes[i];
                    ui.horizontal(|ui| {
                        edited |= ui.checkbox(&mut row.enabled, "").changed();
                        let selected = s.build_window.selected == Some(i);
                        if ui.selectable_label(selected, &row.path).clicked() {
                            s.build_window.selected = Some(i);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if row.enabled {
                                ui.weak(index.to_string());
                            }
                        });
                    });
                    if row.enabled {
                        index += 1;
                    }
                }
            });
    });
    edited
}

fn move_selected(s: &mut EditorState, dir: isize) -> bool {
    let Some(i) = s.build_window.selected else {
        return false;
    };
    let j = i as isize + dir;
    if j < 0 || j as usize >= s.build_settings.scenes.len() {
        return false;
    }
    s.build_settings.scenes.swap(i, j as usize);
    s.build_window.selected = Some(j as usize);
    true
}

/// Build (and optionally run). `pick` = ask for the folder even if one was used before.
pub fn build(s: &mut EditorState, run: bool, pick: bool) {
    if s.play.in_play_mode() {
        s.console.warn("Stop Play mode before building.");
        return;
    }
    if s.build_window.job.as_ref().is_some_and(BuildJob::running) {
        s.console.warn("A build is already running.");
        return;
    }
    if !ensure_scenes(s) {
        return;
    }
    let last = s.files.settings.last_build_folder.clone();
    let out = match last {
        Some(dir) if !pick => dir,
        _ => {
            let start = last
                .or_else(|| s.project.root.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            match rfd::FileDialog::new()
                .set_title("Build Folder")
                .set_directory(start)
                .pick_folder()
            {
                Some(dir) => dir,
                None => return,
            }
        }
    };
    build_into(s, &out, run);
}

/// An empty scene list builds the open scene (like the reference editor). False = nothing to build.
fn ensure_scenes(s: &mut EditorState) -> bool {
    if s.build_settings.enabled().next().is_none() {
        add_open_scene(s);
    }
    let ok = s.build_settings.enabled().next().is_some();
    if !ok {
        s.console.log(
            LogLevel::Error,
            "Build failed: no scenes in Build Settings.",
        );
    }
    ok
}

/// What the build thread needs, copied from the editor now.
pub fn request(s: &EditorState, out: &Path) -> PackageRequest {
    PackageRequest {
        project: s.project.root.clone(),
        out: out.to_path_buf(),
        build: s.build_settings.clone(),
        settings: s.project_settings.clone(),
    }
}

/// Start a background build into `out`.
pub fn build_into(s: &mut EditorState, out: &Path, run: bool) {
    if s.scene_dirty {
        s.console
            .warn("An open scene has unsaved changes; the build uses the saved files.");
    }
    s.files.settings.last_build_folder = Some(out.to_path_buf());
    s.files.settings.save(&s.project.root);
    s.console.info(format!(
        "Building for {} into {}…",
        s.build_settings.target.label(),
        out.display()
    ));
    s.build_window.job = Some(BuildJob::start(request(s, out), run));
    s.build_window.open = true;
}

/// Follow the build: report the end in the Console and start the game for Build And Run.
fn poll(ctx: &egui::Context, s: &mut EditorState) {
    let Some(job) = s.build_window.job.as_mut() else {
        return;
    };
    let run = job.run;
    match job.poll() {
        None if job.running() => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        None => {}
        Some(Ok(built)) => {
            let warnings = job.log.iter().filter(|l| l.warning).count();
            s.console.info(format!(
                "Build succeeded: {} ({} assets, {} warnings)",
                built.exe.display(),
                built.files,
                warnings
            ));
            if run {
                launch(s, &built.exe);
            }
        }
        Some(Err(e)) => s.console.log(LogLevel::Error, format!("Build failed: {e}")),
    }
}

fn launch(s: &mut EditorState, exe: &Path) {
    let dir = exe.parent().unwrap_or(Path::new("."));
    match std::process::Command::new(exe).current_dir(dir).spawn() {
        Ok(_) => s.console.info(format!("Running {}", exe.display())),
        Err(e) => s.console.log(
            LogLevel::Error,
            format!("Could not run {}: {e}", exe.display()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::project::ProjectBrowser;

    fn state(name: &str) -> EditorState {
        let root = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
        EditorState::new(ProjectBrowser::open(root))
    }

    #[test]
    fn menu_action_opens_the_window_and_scenes_are_added_and_reordered() {
        let mut s = state("misty_build_window");
        crate::commands::apply(&mut s, Action::OpenBuildSettings);
        assert!(s.build_window.open);
        s.scene.set_scene_path(0, "Assets/Scenes/A.scene");
        let mut h = Harness::builder()
            .with_size([620.0, 560.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label("Add Open Scenes").click();
        h.run();
        h.state_mut()
            .scene
            .set_scene_path(0, "Assets/Scenes/B.scene");
        h.get_by_label("Add Open Scenes").click();
        h.run();
        h.get_by_label("Add Open Scenes").click();
        h.run();
        let paths = |s: &EditorState| -> Vec<String> {
            s.build_settings
                .scenes
                .iter()
                .map(|b| b.path.clone())
                .collect()
        };
        assert_eq!(
            paths(h.state()),
            ["Assets/Scenes/A.scene", "Assets/Scenes/B.scene"]
        );

        h.get_by_label("Assets/Scenes/B.scene").click();
        h.run();
        h.get_by_label("⏶").click();
        h.run();
        assert_eq!(
            paths(h.state()),
            ["Assets/Scenes/B.scene", "Assets/Scenes/A.scene"]
        );
        h.get_by_label("Remove Selected").click();
        h.run();
        assert_eq!(paths(h.state()), ["Assets/Scenes/A.scene"]);

        // Saved to the project and loaded with it.
        let reopened = EditorState::new(ProjectBrowser::open(h.state().project.root.clone()));
        assert_eq!(paths(&reopened), ["Assets/Scenes/A.scene"]);
    }

    #[test]
    fn platform_and_options_are_saved() {
        let s = state("misty_build_options");
        let mut h = Harness::builder()
            .with_size([620.0, 700.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label_contains("Linux ARM64").click();
        h.run();
        h.get_by_label("Rust target: aarch64-unknown-linux-gnu");
        h.get_by_label("Development Build");
        h.get_by_label("Steam App ID");
        let saved = BuildSettings::load(&h.state().project.root).unwrap();
        assert_eq!(saved.target, BuildTarget::LinuxArm64);
    }

    #[test]
    fn builds_start_in_the_background_and_report_to_the_console() {
        let mut s = state("misty_build_job");
        let root = s.project.root.clone();
        std::fs::write(root.join("Assets/Scenes/A.scene"), "[]").unwrap();
        s.scene.set_scene_path(0, "Assets/Scenes/A.scene");
        // An empty scene list builds the open scene.
        assert!(ensure_scenes(&mut s));
        assert_eq!(s.build_settings.scenes.len(), 1);
        let req = request(&s, &root.join("Out"));
        assert_eq!(req.build.scenes.len(), 1);
        assert_eq!(req.product(), root.file_name().unwrap().to_string_lossy());

        // Building into the project's own Assets is refused (by the build thread).
        build_into(&mut s, &root.join("Assets/Build"), false);
        assert_eq!(
            s.files.settings.last_build_folder.as_deref(),
            Some(root.join("Assets/Build").as_path())
        );
        let ctx = egui::Context::default();
        while s.build_window.job.as_ref().is_some_and(BuildJob::running) {
            poll(&ctx, &mut s);
            std::thread::yield_now();
        }
        let last = s.console.last().unwrap();
        assert_eq!(last.level, LogLevel::Error);
        assert!(
            last.message.contains("outside the project"),
            "{}",
            last.message
        );
    }
}
