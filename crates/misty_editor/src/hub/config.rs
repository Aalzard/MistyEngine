//! `hub.json`: the Hub's recent projects and settings, kept in the user's config folder
//! (`%APPDATA%\MistyEngine\hub.json` on Windows; `MISTY_HUB_CONFIG=<file>` overrides it). The
//! tutorial progress is kept in the same folder (see [`config_dir`]).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Longest recent list kept.
pub const MAX_RECENT: usize = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HubTheme {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: PathBuf,
    /// Seconds since the Unix epoch.
    pub last_opened: u64,
}

impl RecentProject {
    /// The folder name.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// A project folder has an `Assets` folder.
    pub fn exists(&self) -> bool {
        self.path.join("Assets").is_dir()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HubConfig {
    /// Most recently opened first.
    pub recent: Vec<RecentProject>,
    /// Where New Project puts projects; `None` = [`default_projects_folder`].
    pub projects_folder: Option<PathBuf>,
    pub theme: HubTheme,
    /// The editor window waits for the display's refresh (VSync). Off (the default, like
    /// Unity's Game view) the editor draws as fast as it can, so the Game view's frame rate
    /// shows what the game really costs (a 60 Hz display would cap it at 60 and halve it to
    /// 30 whenever a frame misses 16.7 ms). The Game view's VSync toggle.
    pub editor_vsync: bool,
    /// Edit ▸ Preferences ▸ UI Scale (the editor's zoom factor); `None` = 100 %.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_ui_scale: Option<f32>,
    /// Edit ▸ Preferences ▸ Language: the editor UI's language code ("tr", "ar"); `None` =
    /// English.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_language: Option<String>,
}

impl HubConfig {
    /// Missing or unreadable file = defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())
    }

    pub fn projects_folder(&self) -> PathBuf {
        self.projects_folder
            .clone()
            .unwrap_or_else(default_projects_folder)
    }

    /// Put `project` first with the time `now` (adding it if new), keeping the list capped.
    pub fn touch(&mut self, project: &Path, now: u64) {
        self.recent.retain(|r| !same_path(&r.path, project));
        self.recent.insert(
            0,
            RecentProject {
                path: project.to_path_buf(),
                last_opened: now,
            },
        );
        self.recent.truncate(MAX_RECENT);
    }

    pub fn remove(&mut self, project: &Path) {
        self.recent.retain(|r| !same_path(&r.path, project));
    }

    /// A moved project: point its entry at `new` (keeping its place and time).
    pub fn relocate(&mut self, old: &Path, new: &Path) {
        if old != new {
            self.recent.retain(|r| !same_path(&r.path, new));
        }
        if let Some(r) = self.recent.iter_mut().find(|r| same_path(&r.path, old)) {
            r.path = new.to_path_buf();
        }
    }

    /// Recent projects whose name or path contains `query` (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&RecentProject> {
        let q = query.trim().to_lowercase();
        self.recent
            .iter()
            .filter(|r| q.is_empty() || r.path.to_string_lossy().to_lowercase().contains(&q))
            .collect()
    }
}

/// The per-user Misty Engine folder (`%APPDATA%\MistyEngine` on Windows): `hub.json`, the
/// tutorial progress (`tutorials.json`). With `MISTY_HUB_CONFIG=<file>`, that file's folder.
pub fn config_dir() -> Option<PathBuf> {
    config_path()?.parent().map(Path::to_path_buf)
}

/// The editor's VSync preference ([`HubConfig::editor_vsync`]); `MISTY_VSYNC=1` / `0`
/// overrides it.
pub fn editor_vsync() -> bool {
    match std::env::var("MISTY_VSYNC").as_deref() {
        Ok("1") | Ok("on") | Ok("true") => true,
        Ok("0") | Ok("off") | Ok("false") => false,
        _ => config_path().is_some_and(|p| HubConfig::load(&p).editor_vsync),
    }
}

/// Save the editor's VSync preference.
pub fn set_editor_vsync(on: bool) -> Result<(), String> {
    let path = config_path().ok_or("no config folder")?;
    let mut c = HubConfig::load(&path);
    c.editor_vsync = on;
    c.save(&path)
}

/// The editor's UI scale preference (1 = 100 %).
pub fn editor_ui_scale() -> f32 {
    config_path()
        .and_then(|p| HubConfig::load(&p).editor_ui_scale)
        .unwrap_or(1.0)
}

/// Save the editor's UI scale preference.
pub fn set_editor_ui_scale(scale: f32) -> Result<(), String> {
    let path = config_path().ok_or("no config folder")?;
    let mut c = HubConfig::load(&path);
    c.editor_ui_scale = ((scale - 1.0).abs() > 1e-3).then_some(scale);
    c.save(&path)
}

/// The editor's UI language preference (a code: "tr"), if one was chosen.
pub fn editor_language() -> Option<String> {
    config_path().and_then(|p| HubConfig::load(&p).editor_language)
}

/// Save the editor's UI language preference ("en" is the default and is not stored).
pub fn set_editor_language(code: &str) -> Result<(), String> {
    let path = config_path().ok_or("no config folder")?;
    let mut c = HubConfig::load(&path);
    c.editor_language = (code != "en").then(|| code.to_owned());
    c.save(&path)
}

/// Where `hub.json` lives.
pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MISTY_HUB_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }?;
    Some(base.join("MistyEngine").join("hub.json"))
}

/// `Documents\Misty Projects` in the user's home folder.
pub fn default_projects_folder() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents").join("Misty Projects")
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Same folder, ignoring case and slash direction on Windows and a trailing separator.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.trim_end_matches('/').to_owned();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

/// Record that `project` was opened (the editor calls this however it was started).
pub fn record_open(project: &Path) {
    let Some(path) = config_path() else {
        return;
    };
    let project = std::path::absolute(project).unwrap_or_else(|_| project.to_path_buf());
    let mut config = HubConfig::load(&path);
    config.touch(&project, now());
    let _ = config.save(&path);
}

/// "just now", "5 minutes ago", "3 days ago", … for a time `secs` before now (in the editor's
/// language).
pub fn ago(secs: u64) -> String {
    use crate::i18n::tr;
    let n = |size: u64| secs / size;
    match secs {
        s if s >= 365 * 86400 => match n(365 * 86400) {
            1 => tr!("1 year ago"),
            c => tr!("{count} years ago", count = c),
        },
        s if s >= 30 * 86400 => match n(30 * 86400) {
            1 => tr!("1 month ago"),
            c => tr!("{count} months ago", count = c),
        },
        s if s >= 7 * 86400 => match n(7 * 86400) {
            1 => tr!("1 week ago"),
            c => tr!("{count} weeks ago", count = c),
        },
        s if s >= 86400 => match n(86400) {
            1 => tr!("1 day ago"),
            c => tr!("{count} days ago", count = c),
        },
        s if s >= 3600 => match n(3600) {
            1 => tr!("1 hour ago"),
            c => tr!("{count} hours ago", count = c),
        },
        s if s >= 60 => match n(60) {
            1 => tr!("1 minute ago"),
            c => tr!("{count} minutes ago", count = c),
        },
        _ => tr!("just now"),
    }
}
