//! File ▸ Build Settings: the "Scenes In Build" list, player options, and Build / Build And Run.
//! Changes are saved to `ProjectSettings/BuildSettings.json` right away.
//!
//! A build is a folder holding the player executable (named after the product) and copies of
//! the project's `Assets` and `ProjectSettings`; the player plays the folder it sits in.

use std::path::{Path, PathBuf};

use egui::Ui;
use misty_engine::build::BuildSettings;

use crate::actions::Action;
use crate::console::LogLevel;
use crate::state::EditorState;

/// Window state.
#[derive(Default)]
pub struct BuildSettingsWindow {
    pub open: bool,
    /// Selected row of the scene list.
    pub selected: Option<usize>,
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

/// "Add Open Scenes": append the open scene (it must be saved first).
pub fn add_open_scene(s: &mut EditorState) {
    match s.files.path.clone() {
        Some(rel) => {
            if s.build_settings.add(&rel) {
                changed(s);
            }
        }
        None => s
            .console
            .warn("Save the scene before adding it to the build."),
    }
}

/// Draw the window if it is open.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let mut open = s.build_window.open;
    egui::Window::new("Build Settings")
        .open(&mut open)
        .default_size([560.0, 420.0])
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
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
    ui.strong("Player");
    let b = &mut s.build_settings;
    egui::Grid::new("build_player")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Product Name");
            let hint = s
                .project
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            edited |= ui
                .add(
                    egui::TextEdit::singleline(&mut b.product_name)
                        .hint_text(hint.unwrap_or_default()),
                )
                .changed();
            ui.end_row();
            ui.label("Resolution");
            ui.horizontal(|ui| {
                edited |= ui
                    .add(egui::DragValue::new(&mut b.width).range(64..=16384))
                    .changed();
                ui.label("×");
                edited |= ui
                    .add(egui::DragValue::new(&mut b.height).range(64..=16384))
                    .changed();
            });
            ui.end_row();
            ui.label("Fullscreen");
            edited |= ui.checkbox(&mut b.fullscreen, "").changed();
            ui.end_row();
        });
    if edited {
        changed(s);
    }
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Build And Run").clicked() {
            s.push(Action::BuildAndRun);
        }
        if ui.button("Build").clicked() {
            s.push(Action::Build);
        }
    });
}

/// Rows: enabled checkbox, path, build index on the right. Returns true if edited.
fn scene_list(ui: &mut Ui, s: &mut EditorState) -> bool {
    let mut edited = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(140.0);
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .max_height(200.0)
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

/// Where the editor looks for the player: next to its own executable.
pub fn player_exe(dir: &Path) -> PathBuf {
    dir.join(format!("misty_player{}", std::env::consts::EXE_SUFFIX))
}

/// Executable name for a product ("Rolling Ball.exe"), without characters file systems reject.
pub fn exe_name(product: &str) -> String {
    let clean: String = product
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_end_matches('.');
    let stem = if clean.is_empty() { "Game" } else { clean };
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// Write a build of the project at `root` into `out`; returns the built executable.
pub fn make_build(
    root: &Path,
    player: &Path,
    out: &Path,
    product: &str,
) -> Result<PathBuf, String> {
    if !player.is_file() {
        return Err(format!(
            "Player not found at {}: build `misty_player` first: \
             `cargo build -p misty_player --release`",
            player.display()
        ));
    }
    let out_abs = std::path::absolute(out).map_err(|e| e.to_string())?;
    let root_abs = std::path::absolute(root).map_err(|e| e.to_string())?;
    let inside = ["Assets", "ProjectSettings"]
        .iter()
        .any(|d| out_abs.starts_with(root_abs.join(d)));
    if out_abs == root_abs || inside {
        return Err(
            "Choose a build folder outside the project's Assets and ProjectSettings.".into(),
        );
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for dir in ["Assets", "ProjectSettings"] {
        let dst = out.join(dir);
        // Start clean so deleted assets don't linger in the build.
        if dst.exists() {
            std::fs::remove_dir_all(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        }
        copy_dir(&root.join(dir), &dst)?;
    }
    let exe = out.join(exe_name(product));
    std::fs::copy(player, &exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    Ok(exe)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let entries = match std::fs::read_dir(src) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", src.display())),
    };
    for entry in entries.flatten() {
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// Build (and optionally run). `pick` = ask for the folder even if one was used before.
pub fn build(s: &mut EditorState, run: bool, pick: bool) {
    if s.play.in_play_mode() {
        s.console.warn("Stop Play mode before building.");
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

/// Build into `out` with the player next to the editor executable.
pub fn build_into(s: &mut EditorState, out: &Path, run: bool) {
    let player_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    build_with_player(s, &player_exe(&player_dir), out, run);
}

fn build_with_player(s: &mut EditorState, player: &Path, out: &Path, run: bool) {
    if s.scene_dirty {
        s.console
            .warn("The open scene has unsaved changes; the build uses the saved file.");
    }
    let product = s.build_settings.product_name_or(&s.project.root);
    match make_build(&s.project.root, player, out, &product) {
        Ok(exe) => {
            s.files.settings.last_build_folder = Some(out.to_path_buf());
            s.files.settings.save(&s.project.root);
            s.console
                .info(format!("Build succeeded: {}", exe.display()));
            if run {
                match std::process::Command::new(&exe).current_dir(out).spawn() {
                    Ok(_) => s.console.info(format!("Running {product}")),
                    Err(e) => s.console.log(
                        LogLevel::Error,
                        format!("Could not run {}: {e}", exe.display()),
                    ),
                }
            }
        }
        Err(e) => s.console.log(LogLevel::Error, format!("Build failed: {e}")),
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
        s.files.path = Some("Assets/Scenes/A.scene".into());
        let mut h = Harness::builder()
            .with_size([560.0, 420.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label("Add Open Scenes").click();
        h.run();
        h.state_mut().files.path = Some("Assets/Scenes/B.scene".into());
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
    fn build_copies_the_player_and_project_and_needs_the_player() {
        let mut s = state("misty_build_copy");
        let root = s.project.root.clone();
        std::fs::write(root.join("Assets/Scenes/A.scene"), "[]").unwrap();
        s.files.path = Some("Assets/Scenes/A.scene".into());
        s.build_settings.product_name = "My: Game".into();
        let out = root.with_extension("build");
        let _ = std::fs::remove_dir_all(&out);

        // No player executable: a clear Console error, nothing built.
        let missing = root.join("nowhere").join("misty_player.exe");
        build_with_player(&mut s, &missing, &out, false);
        let last = s.console.last().unwrap();
        assert_eq!(last.level, LogLevel::Error);
        assert!(
            last.message
                .contains("cargo build -p misty_player --release")
        );

        let player = root.with_extension("player");
        std::fs::write(&player, "fake exe").unwrap();
        // An empty scene list builds the open scene.
        assert!(ensure_scenes(&mut s));
        assert_eq!(s.build_settings.scenes.len(), 1);
        build_with_player(&mut s, &player, &out, false);
        let exe = out.join(exe_name("My_ Game"));
        assert!(exe.is_file(), "{:?}", s.console.last());
        assert!(out.join("Assets/Scenes/A.scene").is_file());
        assert!(out.join("ProjectSettings/BuildSettings.json").is_file());
        assert_eq!(
            s.files.settings.last_build_folder.as_deref(),
            Some(out.as_path())
        );

        // Building into the project's own Assets is refused.
        assert!(make_build(&root, &player, &root.join("Assets/Build"), "x").is_err());
    }

    #[test]
    fn exe_names_are_file_system_safe() {
        let suffix = std::env::consts::EXE_SUFFIX;
        assert_eq!(exe_name("Rolling Ball"), format!("Rolling Ball{suffix}"));
        assert_eq!(exe_name("a/b?"), format!("a_b_{suffix}"));
        assert_eq!(exe_name("  "), format!("Game{suffix}"));
    }
}
