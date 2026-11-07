//! Window ▸ Multiplayer ▸ Multiplayer Play Mode: Play runs the editor as the host and opens up
//! to three player windows (the `misty_player` next to the editor) that join it as clients on
//! this machine, so a multiplayer scene is tried in one click. A small overlay in the Game view
//! shows the session's round trip time, packets and bandwidth.
//!
//! The players load the scene as it was when Play was pressed (an unsaved copy in
//! `Library/MultiplayerPlayMode`), so edits don't need saving first.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use egui::{Color32, RichText, Ui};
use misty_engine::network::{AutoStart, NetworkManager};
use serde::{Deserialize, Serialize};

use crate::console::LogLevel;
use crate::state::EditorState;

/// Extra players besides the editor (Player 1).
pub const EXTRA_PLAYERS: usize = 3;
const SETTINGS_FILE: &str = "Library/MultiplayerPlayMode.json";

/// What the window remembers per project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MppmSettings {
    /// Players 2–4 join when Play starts.
    pub players: [bool; EXTRA_PLAYERS],
    /// The Game view's network overlay.
    pub show_stats: bool,
}

impl Default for MppmSettings {
    fn default() -> Self {
        Self {
            players: [false; EXTRA_PLAYERS],
            show_stats: true,
        }
    }
}

/// The window, its settings and the player processes it started.
#[derive(Default)]
pub struct MultiplayerPlayMode {
    pub open: bool,
    pub settings: MppmSettings,
    loaded_for: Option<PathBuf>,
    /// Running players: (player index 0..3, process).
    children: Vec<(usize, Child)>,
    /// Why the last launch failed (shown in the window).
    pub problem: Option<String>,
}

impl MultiplayerPlayMode {
    fn load(&mut self, root: &Path) {
        if self.loaded_for.as_deref() == Some(root) {
            return;
        }
        self.loaded_for = Some(root.to_path_buf());
        self.settings = std::fs::read_to_string(root.join(SETTINGS_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
    }

    fn save(&self, root: &Path) {
        let path = root.join(SETTINGS_FILE);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.settings) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Players that join on Play.
    pub fn active_players(&self) -> Vec<usize> {
        (0..EXTRA_PLAYERS)
            .filter(|i| self.settings.players[*i])
            .collect()
    }

    /// Whether player `i`'s window is open.
    pub fn running(&mut self, i: usize) -> bool {
        self.children
            .retain_mut(|(_, c)| matches!(c.try_wait(), Ok(None)));
        self.children.iter().any(|(j, _)| *j == i)
    }

    /// Close every player window this editor opened.
    pub fn stop_all(&mut self) {
        for (_, mut c) in self.children.drain(..) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for MultiplayerPlayMode {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// The player executable shipped next to the editor.
pub fn player_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.with_file_name(format!("misty_player{}", std::env::consts::EXE_SUFFIX));
    p.is_file().then_some(p)
}

/// Arguments of player window `index` (0 = Player 2) joining `port` on this machine.
pub fn player_args(
    root: &Path,
    scene: Option<&str>,
    scene_file: Option<&Path>,
    port: u16,
    index: usize,
) -> Vec<String> {
    let mut args = vec![root.display().to_string()];
    if let Some(rel) = scene {
        args.push("--scene".into());
        args.push(rel.into());
        if let Some(f) = scene_file {
            args.push("--scene-file".into());
            args.push(f.display().to_string());
        }
    }
    args.extend([
        "--net-client".into(),
        format!("127.0.0.1:{port}"),
        "--title".into(),
        format!("Player {}", index + 2),
        "--size".into(),
        "800x450".into(),
        "--pos".into(),
        format!("{},{}", 80 + index as i32 * 60, 80 + index as i32 * 60),
    ]);
    args
}

/// Play just started: host and launch the chosen players (when the scene has a Network
/// Manager).
pub fn on_play_started(s: &mut EditorState) {
    let root = s.project.root.clone();
    s.multiplayer.load(&root);
    s.multiplayer.problem = None;
    let players = s.multiplayer.active_players();
    if players.is_empty() {
        return;
    }
    let has_manager = s
        .scene
        .walk()
        .into_iter()
        .any(|(e, _)| s.scene.world.get::<&NetworkManager>(e).is_ok());
    if !has_manager {
        let msg = "Multiplayer Play Mode: the scene has no Network Manager (Add Component ▸ \
                   Netcode ▸ Network Manager), so no players were started";
        s.console.warn(msg);
        s.multiplayer.problem = Some("The scene has no Network Manager.".into());
        return;
    }
    let Some(rt) = s.runtime.as_mut() else { return };
    let running = rt.with_network(|n| n.is_running());
    if !running && let Err(e) = rt.network_start(&mut s.scene, &AutoStart::Host) {
        s.console
            .log(LogLevel::Error, format!("Multiplayer Play Mode: {e}"));
        s.multiplayer.problem = Some(e);
        return;
    }
    let port = rt.with_network(|n| n.is_server().then(|| n.local_port()).flatten());
    let Some(port) = port else {
        s.console
            .warn("Multiplayer Play Mode: the game is not hosting; players not started");
        return;
    };
    let Some(exe) = player_exe() else {
        let msg = "Multiplayer Play Mode: misty_player was not found next to the editor \
                   (build it with `cargo build -p misty_player`)";
        s.console.log(LogLevel::Error, msg);
        s.multiplayer.problem = Some("misty_player was not found next to the editor.".into());
        return;
    };
    // The players load the scene as it was at Play, saved or not.
    let rel = s.scene.active_path();
    let copy = rel.as_ref().and_then(|_| {
        let snapshot = s.play_session.as_ref().map(|p| p.edit_scene())?;
        let file = root.join("Library/MultiplayerPlayMode/Play.scene");
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, snapshot.to_json()).ok()?;
        Some(file)
    });
    for i in players {
        if s.multiplayer.running(i) {
            continue;
        }
        let args = player_args(&root, rel.as_deref(), copy.as_deref(), port, i);
        match Command::new(&exe).args(&args).spawn() {
            Ok(child) => s.multiplayer.children.push((i, child)),
            Err(e) => {
                s.console.log(
                    LogLevel::Error,
                    format!(
                        "Multiplayer Play Mode: could not start Player {}: {e}",
                        i + 2
                    ),
                );
                s.multiplayer.problem = Some(e.to_string());
            }
        }
    }
    let n = s.multiplayer.children.len();
    if n > 0 {
        s.console.info(format!(
            "Multiplayer Play Mode: hosting on port {port}, {n} player window{} joining",
            if n == 1 { "" } else { "s" }
        ));
    }
}

/// Play stopped: the player windows close.
pub fn on_play_stopped(s: &mut EditorState) {
    s.multiplayer.stop_all();
}

pub fn open(s: &mut EditorState) {
    let root = s.project.root.clone();
    s.multiplayer.load(&root);
    s.multiplayer.open = true;
}

pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let mut open = s.multiplayer.open;
    if !open {
        return;
    }
    egui::Window::new("Multiplayer Play Mode")
        .open(&mut open)
        .default_width(320.0)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
    s.multiplayer.open = open;
}

fn dot(ui: &mut Ui, on: bool) {
    let color = if on {
        Color32::from_rgb(90, 190, 110)
    } else {
        Color32::from_gray(110)
    };
    ui.label(RichText::new("●").color(color));
}

pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    let root = s.project.root.clone();
    s.multiplayer.load(&root);
    let playing = s.play.in_play_mode();
    let hosting = s
        .runtime
        .as_ref()
        .and_then(|r| r.network_stats())
        .map(|st| st.mode.label());
    let before = s.multiplayer.settings.clone();
    egui::Grid::new("mppm_players")
        .num_columns(3)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(RichText::new("Player 1").strong());
            ui.horizontal(|ui| {
                dot(ui, hosting.is_some());
                ui.weak(match hosting {
                    Some(mode) => format!("Main Editor · {mode}"),
                    None => "Main Editor".to_owned(),
                });
            });
            ui.end_row();
            for i in 0..EXTRA_PLAYERS {
                let running = s.multiplayer.running(i);
                ui.checkbox(&mut s.multiplayer.settings.players[i], "")
                    .on_hover_text("Join as a client when Play starts");
                ui.label(format!("Player {}", i + 2));
                ui.horizontal(|ui| {
                    dot(ui, running);
                    ui.weak(if running { "Running" } else { "Off" });
                });
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.checkbox(
        &mut s.multiplayer.settings.show_stats,
        "Network stats in the Game view",
    );
    if s.multiplayer.settings != before {
        s.multiplayer.save(&root);
    }
    ui.add_space(4.0);
    ui.weak(if playing {
        "Players joined when Play started; stop and press Play again to change them."
    } else {
        "With players ticked, Play hosts in the editor and opens their windows as clients."
    });
    if let Some(p) = &s.multiplayer.problem {
        ui.colored_label(Color32::from_rgb(230, 150, 70), p);
    }
}

/// Lines of the Game view's network overlay (none while offline).
pub fn overlay_lines(s: &EditorState) -> Vec<String> {
    if !s.multiplayer.settings.show_stats {
        return Vec::new();
    }
    s.runtime
        .as_ref()
        .and_then(|r| r.network_stats())
        .map(|st| st.lines())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::project::ProjectBrowser;

    #[test]
    fn players_are_remembered_and_their_command_lines_join_the_host() {
        let root = std::env::temp_dir().join(format!("misty_mppm_window_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        crate::commands::apply(&mut s, crate::actions::Action::OpenMultiplayerPlayMode);
        assert!(s.multiplayer.open);
        let mut h = Harness::builder()
            .with_size([360.0, 260.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label("Player 2");
        h.get_by_label("Main Editor");
        // Tick Player 3 (the second checkbox).
        h.get_all_by_role(egui::accesskit::Role::CheckBox)
            .nth(1)
            .unwrap()
            .click();
        h.run();
        assert_eq!(h.state().multiplayer.active_players(), [1]);
        let mut fresh = MultiplayerPlayMode::default();
        fresh.load(&root);
        assert_eq!(fresh.settings.players, [false, true, false], "saved");

        let args = player_args(
            &root,
            Some("Assets/Scenes/Net.scene"),
            Some(&root.join("Library/MultiplayerPlayMode/Play.scene")),
            7777,
            1,
        );
        let joined = args.join(" ");
        assert!(
            joined.contains("--scene Assets/Scenes/Net.scene"),
            "{joined}"
        );
        assert!(joined.contains("--net-client 127.0.0.1:7777"));
        assert!(joined.contains("--title Player 3"));
        assert_eq!(args[0], root.display().to_string());
    }

    #[test]
    fn play_without_a_network_manager_starts_no_players() {
        let root = std::env::temp_dir().join(format!("misty_mppm_play_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root));
        s.multiplayer.load(&s.project.root.clone());
        s.multiplayer.settings.players[0] = true;
        crate::play::start(&mut s);
        assert!(s.multiplayer.problem.is_some());
        assert!(!s.multiplayer.running(0));
        crate::play::stop(&mut s);

        // With a Network Manager the editor hosts (the player exe may be missing in tests).
        let m = s.scene.spawn("Network Manager", None);
        s.scene.add(
            m,
            NetworkManager {
                port: 0,
                lan_discovery: false,
                ..Default::default()
            },
        );
        crate::play::start(&mut s);
        let mode = s
            .runtime
            .as_ref()
            .and_then(|r| r.network_stats())
            .map(|st| st.mode);
        assert_eq!(mode, Some(misty_engine::network::NetMode::Host));
        crate::play::tick(&mut s, 1.0 / 60.0);
        assert!(!overlay_lines(&s).is_empty());
        crate::play::stop(&mut s);
        assert!(s.runtime.is_none());
    }
}
