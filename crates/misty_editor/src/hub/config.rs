//! `hub.json`: the Hub's recent projects and settings, kept in the user's config folder
//! (`%APPDATA%\MistyEngine\hub.json` on Windows; `MISTY_HUB_CONFIG=<file>` overrides it).

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

/// "just now", "5 minutes ago", "3 days ago", … for a time `secs` before now.
pub fn ago(secs: u64) -> String {
    let units = [
        (365 * 86400, "year"),
        (30 * 86400, "month"),
        (7 * 86400, "week"),
        (86400, "day"),
        (3600, "hour"),
        (60, "minute"),
    ];
    for (size, unit) in units {
        if secs >= size {
            let n = secs / size;
            return format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
        }
    }
    "just now".into()
}
