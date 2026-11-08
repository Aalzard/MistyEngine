//! File ▸ Build Profiles: the platform profiles (Windows, Linux, macOS, Dedicated Server) and
//! the project's own, each with its scene list (or the shared one), scripting defines,
//! texture and quality overrides, build options and output folder; Build / Build And Run and
//! the report of the last build. See [`misty_engine::build_profile`].
//!
//! The shared "Scenes In Build" list and the active profile are saved to
//! `ProjectSettings/BuildSettings.json`; each profile to its `.buildprofile` file, right away.
//!
//! Building runs in the background (see [`crate::build_pipeline`]): cargo compiles the player
//! for the platform, then the scenes and the assets they use are packed into
//! `<Product>_Data/data.pak` next to the renamed executable.

use std::path::Path;

use egui::{RichText, Ui};
use misty_engine::build::{BuildScene, BuildSettings, BuildTarget};
use misty_engine::build_profile::{BuildProfile, BuildProfiles, MAX_SIZES, ProfilePlatform};
use misty_engine::build_report::{self, BuildReport};
use misty_engine::sprites::import::TextureCompression;

use crate::build_pipeline::{BuildJob, PackageRequest, targets};
use crate::console::LogLevel;
use crate::state::EditorState;

/// What the right side of the window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// The shared scene list.
    SceneList,
    /// Profile at this index of [`EditorState::build_profiles`].
    #[default]
    Active,
    Profile(usize),
}

/// Window state.
#[derive(Default)]
pub struct BuildSettingsWindow {
    pub open: bool,
    pub page: Page,
    /// Selected row of the scene list shown.
    pub selected: Option<usize>,
    /// The running or last build.
    pub job: Option<BuildJob>,
    /// Custom profile being renamed and the name typed so far.
    pub renaming: Option<(usize, String)>,
    /// The define being typed.
    pub new_define: String,
    /// Report of the last build of the shown profile.
    pub report: Option<BuildReport>,
}

/// Build settings and profiles of the project at `s.project.root`; problems go to the Console.
pub fn load(s: &mut EditorState) {
    match BuildSettings::load(&s.project.root) {
        Ok(b) => s.build_settings = b,
        Err(e) => {
            s.build_settings = BuildSettings::default();
            s.console
                .warn(format!("BuildSettings.json: {e}. Using defaults."));
        }
    }
    s.build_profiles = BuildProfiles::load(&s.project.root, &s.build_settings);
    for w in std::mem::take(&mut s.build_profiles.warnings) {
        s.console.warn(format!("Build Profile: {w}"));
    }
    // Code that only knows Build Settings (asset bundles, tests) follows the active profile.
    let active = active_profile(s);
    s.build_settings.target = active.target;
}

/// Clamp and save the shared settings.
pub fn changed(s: &mut EditorState) {
    s.build_settings.sanitize();
    if let Err(e) = s.build_settings.save(&s.project.root) {
        s.console.log(
            LogLevel::Error,
            format!("Could not save Build Settings: {e}"),
        );
    }
}

/// Save profile `i`.
fn profile_changed(s: &mut EditorState, i: usize) {
    let Some(p) = s.build_profiles.profiles.get_mut(i) else {
        return;
    };
    p.sanitize();
    let is_active = std::ptr::eq(
        s.build_profiles.active(&s.build_settings) as *const _,
        &s.build_profiles.profiles[i] as *const _,
    );
    let p = &s.build_profiles.profiles[i];
    if let Err(e) = p.save(&s.project.root) {
        s.console.log(
            LogLevel::Error,
            format!("Could not save the Build Profile {}: {e}", p.name),
        );
    }
    if is_active && s.build_settings.target != p.target {
        s.build_settings.target = p.target;
        changed(s);
    }
}

/// The active profile (a copy).
pub fn active_profile(s: &EditorState) -> BuildProfile {
    s.build_profiles.active(&s.build_settings).clone()
}

fn active_index(s: &EditorState) -> usize {
    let a = s.build_profiles.active(&s.build_settings);
    s.build_profiles
        .profiles
        .iter()
        .position(|p| std::ptr::eq(p, a))
        .unwrap_or(0)
}

/// Make profile `i` the one Build And Run and Play mode use.
pub fn switch_profile(s: &mut EditorState, i: usize) {
    let Some(p) = s.build_profiles.profiles.get(i) else {
        return;
    };
    s.build_settings.active_profile = p.name.clone();
    s.build_settings.target = p.target;
    let title = p.title();
    changed(s);
    s.console.info(format!("Switched to the {title} profile."));
}

/// The defines scripts see in Play mode: the active profile's, its platform's and
/// `MISTY_EDITOR`.
pub fn play_defines(s: &EditorState) -> Vec<String> {
    let mut p = active_profile(s);
    p.development = false;
    let mut d = p.build_defines();
    d.push("MISTY_EDITOR".into());
    d
}

/// "Add Open Scenes": append every open scene (unsaved ones must be saved first).
pub fn add_open_scene(s: &mut EditorState) {
    let paths = open_scene_paths(s);
    if add_paths(&mut s.build_settings.scenes, &paths) {
        changed(s);
    }
}

fn open_scene_paths(s: &mut EditorState) -> Vec<String> {
    let paths: Vec<Option<String>> = s.scene.scenes().iter().map(|e| e.path.clone()).collect();
    if paths.iter().any(Option::is_none) {
        s.console
            .warn("Save the scene before adding it to the build.");
    }
    paths.into_iter().flatten().collect()
}

fn add_paths(list: &mut Vec<BuildScene>, paths: &[String]) -> bool {
    let mut added = false;
    for p in paths {
        if !list.iter().any(|s| &s.path == p) {
            list.push(BuildScene::new(p.clone()));
            added = true;
        }
    }
    added
}

/// Draw the window if it is open, and follow a running build either way.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    poll(ctx, s);
    let mut open = s.build_window.open;
    egui::Window::new("Build Profiles")
        .open(&mut open)
        .default_size([820.0, 600.0])
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
    s.build_window.open = open;
}

/// The page shown, resolved to a profile index (`None`: the shared scene list).
fn shown(s: &EditorState) -> Option<usize> {
    match s.build_window.page {
        Page::SceneList => None,
        Page::Active => Some(active_index(s)),
        Page::Profile(i) if i < s.build_profiles.profiles.len() => Some(i),
        Page::Profile(_) => Some(active_index(s)),
    }
}

/// Profile list on the left, the selected profile on the right.
pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(210.0);
            sidebar(ui, s);
        });
        ui.separator();
        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .id_salt("build_profile_page")
                .auto_shrink([false, true])
                .show(ui, |ui| match shown(s) {
                    None => shared_scene_page(ui, s),
                    Some(i) => profile_page(ui, s, i),
                });
        });
    });
}

fn sidebar(ui: &mut Ui, s: &mut EditorState) {
    let shown = shown(s);
    let active = active_index(s);
    if ui
        .selectable_label(shown.is_none(), "☰  Scene List")
        .on_hover_text("The scenes every profile builds unless it has its own list")
        .clicked()
    {
        s.build_window.page = Page::SceneList;
        s.build_window.selected = None;
    }
    ui.add_space(6.0);
    ui.weak("Platforms");
    let mut menu: Option<(usize, &'static str)> = None;
    let count = s.build_profiles.profiles.len();
    for i in 0..count {
        let p = &s.build_profiles.profiles[i];
        if p.custom {
            continue;
        }
        let text = profile_row_text(p, i == active);
        let r = ui.selectable_label(shown == Some(i), text);
        if r.clicked() {
            select(s, i);
        }
        r.context_menu(|ui| {
            if ui.button("Copy to New Profile").clicked() {
                menu = Some((i, "duplicate"));
                ui.close();
            }
        });
    }
    ui.add_space(6.0);
    ui.weak("Build Profiles");
    let mut any = false;
    for i in 0..count {
        let p = &s.build_profiles.profiles[i];
        if !p.custom {
            continue;
        }
        any = true;
        if let Some((at, name)) = s.build_window.renaming.as_mut()
            && *at == i
        {
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(190.0));
            r.request_focus();
            if r.lost_focus() {
                let commit = !ui.input(|inp| inp.key_pressed(egui::Key::Escape));
                let name = name.clone();
                s.build_window.renaming = None;
                if commit {
                    rename(s, i, &name);
                }
            }
            continue;
        }
        let text = profile_row_text(p, i == active);
        let name = p.name.clone();
        let r = ui.selectable_label(shown == Some(i), text);
        if r.clicked() {
            select(s, i);
        }
        if r.double_clicked() {
            s.build_window.renaming = Some((i, name));
        }
        r.context_menu(|ui| {
            for (label, what) in [
                ("Duplicate", "duplicate"),
                ("Rename", "rename"),
                ("Delete", "delete"),
            ] {
                if ui.button(label).clicked() {
                    menu = Some((i, what));
                    ui.close();
                }
            }
        });
    }
    if !any {
        ui.weak("None yet");
    }
    ui.add_space(4.0);
    ui.menu_button("＋ Add Build Profile", |ui| {
        for platform in ProfilePlatform::ALL {
            if ui.button(platform.label()).clicked() {
                add_profile(s, None, platform);
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("A new profile for a platform, saved in Assets/Settings/Build Profiles");
    match menu {
        Some((i, "duplicate")) => {
            let from = s.build_profiles.profiles[i].clone();
            add_profile(s, Some(&from), from.platform);
        }
        Some((i, "rename")) => {
            s.build_window.renaming = Some((i, s.build_profiles.profiles[i].name.clone()));
        }
        Some((i, "delete")) => delete(s, i),
        _ => {}
    }
}

fn profile_row_text(p: &BuildProfile, active: bool) -> String {
    let icon = match p.platform {
        ProfilePlatform::Windows => "⊞",
        ProfilePlatform::Linux => "🐧",
        ProfilePlatform::MacOs => "",
        ProfilePlatform::DedicatedServer => "🖧",
    };
    let mut t = format!("{icon}  {}", p.name);
    if active {
        t.push_str("   ● Active");
    }
    t
}

fn select(s: &mut EditorState, i: usize) {
    s.build_window.page = Page::Profile(i);
    s.build_window.selected = None;
    s.build_window.report = None;
}

fn add_profile(s: &mut EditorState, from: Option<&BuildProfile>, platform: ProfilePlatform) {
    let base = match from {
        Some(p) => p.name.clone(),
        None => format!("New {} Profile", platform.label()),
    };
    let root = s.project.root.clone();
    match s.build_profiles.add_custom(&root, &base, from, platform) {
        Ok(i) => {
            let name = s.build_profiles.profiles[i].name.clone();
            s.console
                .info(format!("Created the Build Profile \"{name}\"."));
            select(s, i);
            s.build_window.renaming = Some((i, name));
        }
        Err(e) => s.console.log(
            LogLevel::Error,
            format!("Could not create the Build Profile: {e}"),
        ),
    }
}

fn rename(s: &mut EditorState, i: usize, name: &str) {
    let was_active = active_index(s) == i;
    let root = s.project.root.clone();
    match s.build_profiles.rename(&root, i, name) {
        Ok(new) => {
            if was_active {
                s.build_settings.active_profile = new;
                changed(s);
            }
        }
        Err(e) => s.console.log(LogLevel::Error, format!("Rename: {e}")),
    }
}

fn delete(s: &mut EditorState, i: usize) {
    let was_active = active_index(s) == i;
    let root = s.project.root.clone();
    match s.build_profiles.delete(&root, i) {
        Ok(p) => {
            s.console
                .info(format!("Deleted the Build Profile \"{}\".", p.name));
            if was_active {
                s.build_settings.active_profile.clear();
                changed(s);
            }
            s.build_window.page = Page::Active;
        }
        Err(e) => s.console.log(LogLevel::Error, format!("Delete: {e}")),
    }
}

/// The shared "Scenes In Build" list.
fn shared_scene_page(ui: &mut Ui, s: &mut EditorState) {
    ui.heading("Scene List");
    ui.weak("Built by every profile that doesn't override it. Scene 0 starts the game.");
    ui.add_space(4.0);
    let paths_needed = scene_editor_buttons_clicked(ui, s, None);
    if paths_needed {
        add_open_scene(s);
    }
}

/// The scene list of profile `profile` (or the shared one) with its buttons; returns true
/// when "Add Open Scenes" was clicked.
fn scene_editor_buttons_clicked(ui: &mut Ui, s: &mut EditorState, profile: Option<usize>) -> bool {
    let mut edited = false;
    let mut add_open = false;
    {
        let selected = &mut s.build_window.selected;
        let list = match profile {
            Some(i) => &mut s.build_profiles.profiles[i].scenes,
            None => &mut s.build_settings.scenes,
        };
        edited |= scene_list(ui, list, selected);
        ui.horizontal(|ui| {
            let sel = *selected;
            let len = list.len();
            let can_up = sel.is_some_and(|i| i > 0 && i < len);
            let can_down = sel.is_some_and(|i| i + 1 < len);
            if ui
                .add_enabled(can_up, egui::Button::new("⏶"))
                .on_hover_text("Move Up")
                .clicked()
            {
                edited |= move_selected(list, selected, -1);
            }
            if ui
                .add_enabled(can_down, egui::Button::new("⏷"))
                .on_hover_text("Move Down")
                .clicked()
            {
                edited |= move_selected(list, selected, 1);
            }
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("Remove Selected"))
                .clicked()
                && let Some(i) = sel.filter(|i| *i < len)
            {
                list.remove(i);
                *selected = None;
                edited = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Add Open Scenes").clicked() {
                    add_open = true;
                }
            });
        });
    }
    if add_open && profile.is_some() {
        let paths = open_scene_paths(s);
        if let Some(i) = profile {
            edited |= add_paths(&mut s.build_profiles.profiles[i].scenes, &paths);
        }
        add_open = false;
    }
    if edited {
        match profile {
            Some(i) => profile_changed(s, i),
            None => changed(s),
        }
    }
    add_open
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong());
    ui.separator();
}

/// The settings of profile `i`, Build buttons and the last build.
fn profile_page(ui: &mut Ui, s: &mut EditorState, i: usize) {
    let active = active_index(s) == i;
    let p = s.build_profiles.profiles[i].clone();
    ui.horizontal(|ui| {
        ui.heading(&p.name);
        if active {
            ui.label(RichText::new("Active").color(crate::theme::OVERRIDE));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !active
                && ui
                    .button("Switch Profile")
                    .on_hover_text("Build And Run (Ctrl+B) and Play mode use the active profile")
                    .clicked()
            {
                switch_profile(s, i);
            }
        });
    });
    ui.weak(if p.custom {
        format!("{} profile · {}", p.platform.label(), p.rel_path())
    } else if p.is_server() {
        "A headless player for hosting: no window, audio or GPU. It runs the network \
         server and leaves textures, audio and video out of the build."
            .to_owned()
    } else {
        format!("Platform profile · {}", p.rel_path())
    });

    let mut edited = false;
    section(ui, "Platform");
    edited |= platform(ui, &mut s.build_profiles.profiles[i]);

    section(ui, "Scene List");
    {
        let prof = &mut s.build_profiles.profiles[i];
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut prof.override_scenes, "Override Global Scene List")
                .on_hover_text("Build this profile's own scenes instead of the shared Scene List")
                .changed()
            {
                if prof.override_scenes && prof.scenes.is_empty() {
                    prof.scenes = s.build_settings.scenes.clone();
                }
                s.build_window.selected = None;
                edited = true;
            }
        });
    }
    if edited {
        profile_changed(s, i);
        edited = false;
    }
    if s.build_profiles.profiles[i].override_scenes {
        scene_editor_buttons_clicked(ui, s, Some(i));
    } else {
        let n = s.build_settings.enabled().count();
        ui.horizontal(|ui| {
            ui.weak(format!(
                "Uses the shared Scene List ({n} scene{}).",
                if n == 1 { "" } else { "s" }
            ));
            if ui.small_button("Edit…").clicked() {
                s.build_window.page = Page::SceneList;
            }
        });
    }

    section(ui, "Scripting Defines");
    edited |= defines(ui, s, i);

    section(ui, "Texture Overrides");
    edited |= textures(ui, &mut s.build_profiles.profiles[i]);

    section(ui, "Quality");
    edited |= quality(ui, s, i);

    section(ui, "Build Options");
    edited |= options(ui, s, i);
    if edited {
        profile_changed(s, i);
    }

    ui.add_space(8.0);
    ui.separator();
    progress(ui, s);
    report(ui, s, i);
    let busy = s.build_window.job.as_ref().is_some_and(BuildJob::running);
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Build And Run"))
            .on_hover_text("Build into the output folder, then start the game")
            .clicked()
        {
            build_profile_at(s, i, true, false);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Build"))
            .on_hover_text("Build into the output folder (asks for one when it is empty)")
            .clicked()
        {
            build_profile_at(s, i, false, false);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Build To…"))
            .on_hover_text("Choose another output folder and build there")
            .clicked()
        {
            build_profile_at(s, i, false, true);
        }
        if ui.button("Player Settings…").clicked() {
            crate::project_settings::open_at(s, crate::project_settings::Category::Player);
        }
    });
}

/// Target choice and the toolchain status of the profile's target.
fn platform(ui: &mut Ui, p: &mut BuildProfile) -> bool {
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
    ui.horizontal(|ui| {
        ui.label("Target");
        let label = |t: BuildTarget| {
            if Some(t) == host {
                format!("{}  (this PC)", t.label())
            } else {
                t.label().to_owned()
            }
        };
        egui::ComboBox::from_id_salt("build_profile_target")
            .selected_text(label(p.target))
            .width(230.0)
            .show_ui(ui, |ui| {
                for t in p.platform.targets() {
                    let mark = match &tools {
                        None => "…",
                        Some(_) if status(*t).ready() => "✔",
                        Some(_) => "⚠",
                    };
                    let text = format!("{mark}  {}", label(*t));
                    if ui.selectable_label(p.target == *t, text).clicked() && p.target != *t {
                        p.target = *t;
                        edited = true;
                    }
                }
            });
    });
    let t = p.target;
    ui.weak(format!("Rust target: {}", t.triple()));
    let Some(_) = &tools else {
        ui.weak("Checking installed Rust targets…");
        return edited;
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
    edited
}

fn defines(ui: &mut Ui, s: &mut EditorState, i: usize) -> bool {
    let mut edited = false;
    let p = &mut s.build_profiles.profiles[i];
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        if p.defines.is_empty() {
            ui.weak("No defines.");
        }
        for (k, d) in p.defines.iter().enumerate() {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.monospace(d);
                        if ui.small_button("✖").on_hover_text("Remove").clicked() {
                            remove = Some(k);
                        }
                    });
                });
        }
    });
    if let Some(k) = remove {
        p.defines.remove(k);
        edited = true;
    }
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut s.build_window.new_define)
                .hint_text("DEFINE_NAME")
                .desired_width(180.0),
        );
        let enter = r.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter));
        let name = misty_engine::build_profile::clean_define(&s.build_window.new_define);
        if (ui
            .add_enabled(!name.is_empty(), egui::Button::new("Add"))
            .clicked()
            || enter)
            && !name.is_empty()
        {
            if !p.defines.contains(&name) {
                p.defines.push(name);
                edited = true;
            }
            s.build_window.new_define.clear();
        }
    });
    ui.weak(
        "Lua sees them in --#if NAME … --#else … --#endif blocks and Application.HasDefine(\"NAME\"). \
         Built in: MISTY_STANDALONE_WIN/LINUX/OSX, MISTY_SERVER, DEVELOPMENT_BUILD, MISTY_EDITOR.",
    );
    edited
}

fn textures(ui: &mut Ui, p: &mut BuildProfile) -> bool {
    let mut edited = false;
    egui::Grid::new("build_profile_textures")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Compression").on_hover_text(
                "Format of every compressed texture in this build (sprites set to None stay \
                 uncompressed). \"None\" turns compression off for all textures.",
            );
            let text = |c: Option<TextureCompression>| {
                c.map_or("Use Texture Settings".to_owned(), |c| c.label().to_owned())
            };
            egui::ComboBox::from_id_salt("build_profile_tex_compression")
                .selected_text(text(p.texture_compression))
                .width(180.0)
                .show_ui(ui, |ui| {
                    let choices =
                        std::iter::once(None).chain(TextureCompression::ALL.into_iter().map(Some));
                    for c in choices {
                        if ui
                            .selectable_label(p.texture_compression == c, text(c))
                            .clicked()
                        {
                            p.texture_compression = c;
                            edited = true;
                        }
                    }
                });
            ui.end_row();
            ui.label("Max Texture Size").on_hover_text(
                "Textures bigger than this are scaled down in the build (smaller download and \
                 GPU memory).",
            );
            let size = |m: Option<u32>| m.map_or("No Override".to_owned(), |m| m.to_string());
            egui::ComboBox::from_id_salt("build_profile_tex_size")
                .selected_text(size(p.max_texture_size))
                .width(180.0)
                .show_ui(ui, |ui| {
                    let choices = std::iter::once(None).chain(MAX_SIZES.into_iter().map(Some));
                    for m in choices {
                        if ui
                            .selectable_label(p.max_texture_size == m, size(m))
                            .clicked()
                        {
                            p.max_texture_size = m;
                            edited = true;
                        }
                    }
                });
            ui.end_row();
        });
    edited
}

fn quality(ui: &mut Ui, s: &mut EditorState, i: usize) -> bool {
    let mut edited = false;
    let q = &s.project_settings.quality;
    let default = q
        .levels
        .get(q.default_level)
        .map_or(String::new(), |l| l.name.clone());
    let names: Vec<String> = q.levels.iter().map(|l| l.name.clone()).collect();
    let p = &mut s.build_profiles.profiles[i];
    ui.horizontal(|ui| {
        ui.label("Quality Level").on_hover_text(
            "The level the built game starts on (players can still change it in game).",
        );
        let text = |l: &Option<String>| match l {
            None => format!("Project Default ({default})"),
            Some(n) if names.contains(n) => n.clone(),
            Some(n) => format!("{n} (missing)"),
        };
        egui::ComboBox::from_id_salt("build_profile_quality")
            .selected_text(text(&p.quality_level))
            .width(200.0)
            .show_ui(ui, |ui| {
                let choices = std::iter::once(None).chain(names.iter().cloned().map(Some));
                for c in choices {
                    if ui
                        .selectable_label(p.quality_level == c, text(&c))
                        .clicked()
                    {
                        p.quality_level = c;
                        edited = true;
                    }
                }
            });
    });
    edited
}

fn options(ui: &mut Ui, s: &mut EditorState, i: usize) -> bool {
    let mut edited = false;
    let mut shared_edited = false;
    let root = s.project.root.clone();
    let b = &mut s.build_settings;
    let p = &mut s.build_profiles.profiles[i];
    egui::Grid::new("build_options")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Development Build").on_hover_text(
                "Opens a console window with the log and writes Player.log next to the game. \
                 Turn it off for releases.",
            );
            edited |= ui.checkbox(&mut p.development, "").changed();
            ui.end_row();
            ui.label("Compress Data")
                .on_hover_text("LZ4-compress the packed assets: a smaller download.");
            edited |= ui.checkbox(&mut p.compression, "").changed();
            ui.end_row();
            ui.label("Create .zip").on_hover_text(
                "Also write <Product>-<version>-<platform>.zip next to the build folder, \
                 ready for itch.io and similar stores.",
            );
            edited |= ui.checkbox(&mut p.create_zip, "").changed();
            ui.end_row();
            ui.label("Output Folder").on_hover_text(
                "Where Build puts the game: absolute or relative to the project folder. \
                 Empty = ask on the first build.",
            );
            ui.horizontal(|ui| {
                edited |= ui
                    .add(
                        egui::TextEdit::singleline(&mut p.output_folder)
                            .hint_text("ask")
                            .desired_width(260.0),
                    )
                    .changed();
                if ui.button("Browse…").clicked() {
                    let start = p
                        .output_path(&root)
                        .or_else(|| root.parent().map(Path::to_path_buf))
                        .unwrap_or_default();
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title("Build Folder")
                        .set_directory(start)
                        .pick_folder()
                    {
                        p.output_folder = folder_text(&root, &dir);
                        edited = true;
                    }
                }
            });
            ui.end_row();
            ui.label("Steam App ID").on_hover_text(
                "Optional, shared by every profile. Adds SteamPipe upload scripts in steam/ \
                 and, for development builds, steam_appid.txt.",
            );
            shared_edited |= ui
                .add(
                    egui::TextEdit::singleline(&mut b.steam_app_id)
                        .hint_text("none")
                        .desired_width(120.0),
                )
                .changed();
            ui.end_row();
        });
    if shared_edited {
        changed(s);
    }
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

/// `dir` as typed in Output Folder: relative when it is inside the project folder.
fn folder_text(root: &Path, dir: &Path) -> String {
    dir.strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| dir.to_string_lossy().into_owned())
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

/// The last build report of profile `i` (this session's, else the one saved in Library).
fn report(ui: &mut Ui, s: &mut EditorState, i: usize) {
    let name = s.build_profiles.profiles[i].name.clone();
    if s.build_window
        .report
        .as_ref()
        .is_none_or(|r| r.profile != name)
    {
        s.build_window.report =
            BuildReport::load(&BuildReport::library_path(&s.project.root, &name)).ok();
    }
    let Some(r) = &s.build_window.report else {
        return;
    };
    egui::CollapsingHeader::new(format!(
        "Build Report · {} · {}",
        build_report::size_text(r.total_bytes),
        r.output.display()
    ))
    .id_salt("build_report")
    .default_open(true)
    .show(ui, |ui| report_view(ui, r));
}

/// Sizes by asset type and the largest assets.
pub fn report_view(ui: &mut Ui, r: &BuildReport) {
    ui.weak(format!(
        "{} · {} files · executable {} · data {}{} · {:.1} s",
        r.target,
        r.files,
        build_report::size_text(r.executable_bytes),
        build_report::size_text(r.pak_bytes),
        if r.stripped > 0 {
            format!(
                " · {} files stripped for the server ({})",
                r.stripped,
                build_report::size_text(r.stripped_bytes)
            )
        } else {
            String::new()
        },
        r.seconds
    ));
    let total: u64 = r.categories.iter().map(|c| c.packed_bytes).sum();
    ui.columns(2, |cols| {
        cols[0].label(RichText::new("By type").strong());
        egui::Grid::new("build_report_types")
            .num_columns(4)
            .striped(true)
            .spacing([10.0, 2.0])
            .show(&mut cols[0], |ui| {
                for c in &r.categories {
                    ui.label(&c.name);
                    ui.weak(c.files.to_string());
                    ui.label(build_report::size_text(c.packed_bytes));
                    let share = build_report::percent(c.packed_bytes, total) as f32 / 100.0;
                    ui.add(
                        egui::ProgressBar::new(share)
                            .desired_width(70.0)
                            .text(format!("{:.0}%", share * 100.0)),
                    );
                    ui.end_row();
                }
            });
        cols[1].label(RichText::new("Largest assets").strong());
        egui::Grid::new("build_report_largest")
            .num_columns(2)
            .striped(true)
            .spacing([10.0, 2.0])
            .show(&mut cols[1], |ui| {
                for a in r.largest.iter().take(10) {
                    ui.label(build_report::size_text(a.packed_bytes));
                    let name = a.path.rsplit('/').next().unwrap_or(&a.path);
                    ui.label(name)
                        .on_hover_text(format!("{} ({})", a.path, a.category));
                    ui.end_row();
                }
            });
    });
    let file = r.output.join(build_report::TEXT_FILE);
    if file.is_file() && ui.small_button("Open BuildReport.txt").clicked() {
        let _ = open_path(&file);
    }
}

fn open_path(path: &Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
}

/// Rows: enabled checkbox, path, build index on the right. Returns true if edited.
fn scene_list(ui: &mut Ui, scenes: &mut [BuildScene], selected: &mut Option<usize>) -> bool {
    let mut edited = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(80.0);
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .id_salt("build_scene_list")
            .max_height(160.0)
            .show(ui, |ui| {
                if scenes.is_empty() {
                    ui.weak("Add the open scene with \"Add Open Scenes\".");
                }
                let mut index = 0;
                for (i, row) in scenes.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        edited |= ui.checkbox(&mut row.enabled, "").changed();
                        if ui
                            .selectable_label(*selected == Some(i), &row.path)
                            .clicked()
                        {
                            *selected = Some(i);
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

fn move_selected(list: &mut [BuildScene], selected: &mut Option<usize>, dir: isize) -> bool {
    let Some(i) = *selected else {
        return false;
    };
    let j = i as isize + dir;
    if j < 0 || j as usize >= list.len() || i >= list.len() {
        return false;
    }
    list.swap(i, j as usize);
    *selected = Some(j as usize);
    true
}

/// Build the active profile (and optionally run). `pick` = ask for the folder even if the
/// profile has one.
pub fn build(s: &mut EditorState, run: bool, pick: bool) {
    let i = active_index(s);
    build_profile_at(s, i, run, pick);
}

/// Build profile `i` into its output folder (asked for when it has none or `pick`).
pub fn build_profile_at(s: &mut EditorState, i: usize, run: bool, pick: bool) {
    if s.play.in_play_mode() {
        s.console.warn("Stop Play mode before building.");
        return;
    }
    if s.build_window.job.as_ref().is_some_and(BuildJob::running) {
        s.console.warn("A build is already running.");
        return;
    }
    if !ensure_scenes(s, i) {
        return;
    }
    let root = s.project.root.clone();
    let profile_out = s.build_profiles.profiles[i].output_path(&root);
    let out = match profile_out {
        Some(dir) if !pick => dir,
        _ => {
            let start = profile_out
                .or_else(|| s.files.settings.last_build_folder.clone())
                .or_else(|| root.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            match rfd::FileDialog::new()
                .set_title("Build Folder")
                .set_directory(start)
                .pick_folder()
            {
                Some(dir) => {
                    // Remembered by the profile.
                    s.build_profiles.profiles[i].output_folder = folder_text(&root, &dir);
                    profile_changed(s, i);
                    dir
                }
                None => return,
            }
        }
    };
    build_into_profile(s, i, &out, run);
}

/// An empty scene list builds the open scene (like the reference editor). False = nothing to build.
fn ensure_scenes(s: &mut EditorState, i: usize) -> bool {
    let p = &s.build_profiles.profiles[i];
    let has = |list: &[BuildScene]| list.iter().any(|b| b.enabled);
    if !has(p.scenes(&s.build_settings)) {
        if p.override_scenes {
            let paths = open_scene_paths(s);
            if add_paths(&mut s.build_profiles.profiles[i].scenes, &paths) {
                profile_changed(s, i);
            }
        } else {
            add_open_scene(s);
        }
    }
    let ok = has(s.build_profiles.profiles[i].scenes(&s.build_settings));
    if !ok {
        s.console.log(
            LogLevel::Error,
            "Build failed: no scenes in the Scene List.",
        );
    }
    ok
}

/// What the build thread needs for the active profile, copied from the editor now.
pub fn request(s: &EditorState, out: &Path) -> PackageRequest {
    request_for(s, active_index(s), out)
}

/// What the build thread needs for profile `i`.
pub fn request_for(s: &EditorState, i: usize, out: &Path) -> PackageRequest {
    PackageRequest::with_profile(
        &s.project.root,
        out,
        &s.build_settings,
        &s.project_settings,
        &s.build_profiles.profiles[i],
    )
}

/// Start a background build of the active profile into `out`.
#[cfg(test)]
pub fn build_into(s: &mut EditorState, out: &Path, run: bool) {
    let i = active_index(s);
    build_into_profile(s, i, out, run);
}

fn build_into_profile(s: &mut EditorState, i: usize, out: &Path, run: bool) {
    if s.scene_dirty {
        s.console
            .warn("An open scene has unsaved changes; the build uses the saved files.");
    }
    s.files.settings.last_build_folder = Some(out.to_path_buf());
    s.files.settings.save(&s.project.root);
    let p = &s.build_profiles.profiles[i];
    s.console.info(format!(
        "Building the {} profile ({}) into {}…",
        p.name,
        p.target.label(),
        out.display()
    ));
    s.build_window.job = Some(BuildJob::start(request_for(s, i, out), run));
    s.build_window.page = Page::Profile(i);
    s.build_window.report = None;
    s.build_window.open = true;
}

/// Follow the build: report the end in the Console, show the report and start the game for
/// Build And Run.
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
                "Build succeeded: {} ({} assets, {}, {} warnings)",
                built.exe.display(),
                built.files,
                build_report::size_text(built.report.total_bytes),
                warnings
            ));
            s.build_window.report = Some(built.report.clone());
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
mod tests;
