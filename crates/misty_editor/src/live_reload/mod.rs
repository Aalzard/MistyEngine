//! Live reload: the open editor follows what changes on disk while the project is being worked
//! on (by the user in other tools, or by automated agents that regenerate files and rebuild the
//! editor), the way the familiar editor re-imports and reloads.
//!
//! - **Scenes** ([`scenes`]): an open scene whose file changed on disk reloads in place when it
//!   has no unsaved changes (camera, selection and Hierarchy state kept); with unsaved changes a
//!   bar asks "Reload / Keep mine". The editor's own saves never count as changes.
//! - **Assets** ([`assets`]): materials, prefabs (instances refresh), terrain data (with grass
//!   and detail settings), timelines, animation clips, Animator Controllers and Audio Mixers
//!   reload here; textures, models, shaders, Shader Graphs and effect graphs reload in the
//!   renderer. A status-bar toast says how many assets were reloaded.
//! - **Dev builds** ([`dev_build`]): run from a source checkout, a newer `misty.exe` in
//!   `target/` shows a "New build — Restart" pill; Help ▸ Developer ▸ Rebuild && Restart runs
//!   cargo in the background. Restarting writes a restore file ([`restore`]) that the new
//!   process reads back (`misty --restore <file>`).
//!
//! Everything polls file times every half second and waits until a file has stopped changing
//! before reading it, so half-written files are never loaded. The `stat` calls themselves run on
//! the engine's background file watch (`misty_engine::file_watch`): a poll reads its last stamps
//! and checks a file on disk only when its stamp differs ([`changed_stamp`]). Nothing reloads in
//! Play mode; changes are picked up after Stop.

pub mod assets;
pub mod dev_build;
pub mod paths;
pub mod restore;
pub mod scenes;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use crate::state::EditorState;

/// How often files are checked.
const POLL: Duration = Duration::from_millis(500);
/// A changed file must keep the same time and size this long before it is read.
const SETTLE: Duration = Duration::from_millis(300);
/// How long the "Reloaded 3 assets" toast stays.
const TOAST: Duration = Duration::from_secs(4);

/// A file's modification time and size: what changes when it is rewritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

/// The file's current stamp (`None` when it doesn't exist).
pub fn stamp(path: &Path) -> Option<Stamp> {
    misty_engine::profile_scope!("LiveReload.Stat");
    let m = std::fs::metadata(path).ok()?;
    m.is_file().then(|| Stamp {
        modified: m.modified().ok(),
        len: m.len(),
    })
}

/// The file's stamp now, or `None` when it is the same as `seen`: the background file watch's
/// stamp is looked at first (a map lookup) and only a difference is checked on disk, so
/// nothing is decided on a stale stamp.
pub fn changed_stamp(path: &Path, seen: Option<Stamp>) -> Option<Option<Stamp>> {
    let cached = misty_engine::file_watch::cached_metadata(path).and_then(|m| {
        m.is_file.then_some(Stamp {
            modified: m.modified,
            len: m.len,
        })
    });
    if cached == seen {
        return None;
    }
    let now = stamp(path);
    (now != seen).then_some(now)
}

/// Waits for changed files to settle: a file counts as changed once its stamp has stayed the
/// same for [`LiveReload::settle`] (writers that save in several steps are done by then).
#[derive(Default)]
pub struct Settle {
    pending: HashMap<String, (Option<Stamp>, Instant)>,
}

impl Settle {
    /// True once `now` (the file's current stamp) has been seen unchanged for `wait`.
    pub fn ready(&mut self, key: &str, now: Option<Stamp>, wait: Duration) -> bool {
        match self.pending.get(key) {
            Some((seen, since)) if *seen == now => {
                if since.elapsed() >= wait {
                    self.pending.remove(key);
                    true
                } else {
                    false
                }
            }
            _ => {
                self.pending.insert(key.to_owned(), (now, Instant::now()));
                false
            }
        }
    }

    pub fn forget(&mut self, key: &str) {
        self.pending.remove(key);
    }
}

/// Live-reload state held by the editor.
pub struct LiveReload {
    pub scenes: scenes::SceneWatch,
    pub assets: assets::AssetWatch,
    pub dev: dev_build::DevBuild,
    /// "Reloaded 3 assets" and when it was shown.
    pub toast: Option<(String, Instant)>,
    /// The save prompt was answered for a restart: the app writes the restore file and exits.
    pub restart_now: bool,
    /// How long a changed file must stay unchanged (tests use zero).
    pub settle: Duration,
    last_poll: Option<Instant>,
}

impl Default for LiveReload {
    fn default() -> Self {
        Self {
            scenes: Default::default(),
            assets: Default::default(),
            dev: Default::default(),
            toast: None,
            restart_now: false,
            settle: SETTLE,
            last_poll: None,
        }
    }
}

impl LiveReload {
    /// Show "Reloaded N assets" in the status bar.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// The toast while it is still showing.
    pub fn current_toast(&self) -> Option<&str> {
        self.toast
            .as_ref()
            .filter(|(_, at)| at.elapsed() < TOAST)
            .map(|(t, _)| t.as_str())
    }
}

/// "Reloaded 1 asset" / "Reloaded 3 assets".
pub fn reloaded_text(n: usize) -> String {
    if n == 1 {
        "Reloaded 1 asset".to_owned()
    } else {
        format!("Reloaded {n} assets")
    }
}

/// Once per editor frame in Edit mode. `renderer` = paths the renderer reloaded this frame
/// (textures, models, shaders, effect graphs). Returns true when something was reloaded.
pub fn frame(s: &mut EditorState, renderer: Vec<String>) -> bool {
    let due = s.live.last_poll.is_none_or(|t| t.elapsed() >= POLL);
    if !due && renderer.is_empty() {
        return false;
    }
    let mut reloaded = renderer;
    if due {
        s.live.last_poll = Some(Instant::now());
        {
            misty_engine::profile_scope!("LiveReload.Scenes");
            scenes::poll(s);
        }
        misty_engine::profile_scope!("LiveReload.Assets");
        reloaded.extend(assets::poll(s));
    }
    report(s, reloaded)
}

/// Check everything now, whatever the poll interval (File ▸ Reload Scene, tests).
pub fn poll_now(s: &mut EditorState) -> bool {
    s.live.last_poll = None;
    frame(s, Vec::new())
}

/// Console line and toast for reloaded assets.
fn report(s: &mut EditorState, mut reloaded: Vec<String>) -> bool {
    reloaded.sort();
    reloaded.dedup();
    if reloaded.is_empty() {
        return false;
    }
    let text = reloaded_text(reloaded.len());
    let names: Vec<&str> = reloaded
        .iter()
        .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p))
        .collect();
    s.console.info(format!("{text}: {}", names.join(", ")));
    s.live.toast(text);
    true
}

/// The status bar's right side: the toast and the rebuild progress.
pub fn status_items(ui: &mut egui::Ui, s: &mut EditorState) {
    dev_build::status(ui, s);
    if let Some(text) = s.live.current_toast() {
        ui.label(
            egui::RichText::new(format!("⟳ {text}")).color(egui::Color32::from_rgb(120, 200, 140)),
        );
        // Repaint so the toast goes away on time.
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }
}
