//! `%APPDATA%\MistyEngine\updater.json`: when the last check ran, what it found and the version
//! the user chose to skip. `MISTY_UPDATER_DIR=<folder>` puts the file elsewhere (tests).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::release::Update;
use super::version::is_newer;

/// Automatic checks run at most this often (manual checks always run).
pub const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    /// Unix seconds of the last finished check (0 = never).
    #[serde(default)]
    pub last_check: u64,
    /// The latest release seen then (`None` = none or no newer one).
    #[serde(default)]
    pub latest: Option<Update>,
    /// "Skip this version": the version not to offer again automatically.
    #[serde(default)]
    pub skipped: Option<String>,
}

impl Cache {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// The last check is recent enough to reuse (a clock set back counts as stale).
    pub fn fresh(&self, now: u64) -> bool {
        self.last_check != 0
            && now >= self.last_check
            && now - self.last_check < CHECK_INTERVAL_SECS
    }
}

/// Whether `update` should be offered: newer than `current`, and not skipped unless the user
/// asked (Help ▸ Check for Updates… still shows a skipped version).
pub fn offer(update: &Update, current: &str, skipped: Option<&str>, manual: bool) -> bool {
    is_newer(&update.version, current) && (manual || skipped != Some(update.version.as_str()))
}

pub fn path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MISTY_UPDATER_DIR") {
        return Some(PathBuf::from(dir).join("updater.json"));
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }?;
    Some(base.join("MistyEngine").join("updater.json"))
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
