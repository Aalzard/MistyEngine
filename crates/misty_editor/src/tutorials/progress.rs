//! Finished tutorials, saved per user (not per project) next to the Hub's `hub.json`:
//! `%APPDATA%\MistyEngine\tutorials.json` on Windows (see `hub::config::config_dir`). The Hub
//! reads the same file for its ✓ marks. Older editors saved it in the PlayerPrefs-style folder
//! `%APPDATA%\Misty Engine\Editor\tutorials.json`; that file is still read while the new one
//! doesn't exist (the next finished tutorial writes the new one).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "tutorials.json";

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    /// IDs of finished tutorials.
    pub done: BTreeSet<String>,
    /// Where it is saved; `None` keeps it in memory (tests).
    #[serde(skip)]
    pub file: Option<PathBuf>,
}

/// The per-user progress file.
pub fn default_file() -> Option<PathBuf> {
    Some(crate::hub::config::config_dir()?.join(FILE_NAME))
}

/// Where the tutorials branch first saved progress (read when [`default_file`] is missing).
/// Not used while `MISTY_HUB_CONFIG` points somewhere else (tests, screenshots).
fn legacy_file() -> Option<PathBuf> {
    if std::env::var_os("MISTY_HUB_CONFIG").is_some() {
        return None;
    }
    let prefs = misty_engine::prefs::default_path("Misty Engine", "Editor")?;
    Some(prefs.with_file_name(FILE_NAME))
}

fn read(file: &Path) -> Option<Progress> {
    let text = std::fs::read_to_string(file).ok()?;
    serde_json::from_str(&text).ok()
}

impl Progress {
    /// Read `file` (missing or broken = nothing finished).
    pub fn load(file: Option<PathBuf>) -> Self {
        Self::load_or(file, None)
    }

    /// The per-user progress, falling back to the old location.
    pub fn load_default() -> Self {
        Self::load_or(default_file(), legacy_file())
    }

    /// Read `file`, or `fallback` while `file` doesn't exist; saves always go to `file`.
    pub fn load_or(file: Option<PathBuf>, fallback: Option<PathBuf>) -> Self {
        let from_file = file.as_deref().and_then(read);
        let missing = file.as_deref().is_none_or(|f| !f.exists());
        let mut p = match from_file {
            Some(p) => p,
            None if missing => fallback.as_deref().and_then(read).unwrap_or_default(),
            None => Progress::default(),
        };
        p.file = file;
        p
    }

    pub fn is_done(&self, id: &str) -> bool {
        self.done.contains(id)
    }

    /// Mark `id` finished and save.
    pub fn complete(&mut self, id: &str) -> Result<(), String> {
        self.done.insert(id.to_owned());
        match &self.file {
            Some(f) => save(f, self),
            None => Ok(()),
        }
    }
}

fn save(file: &Path, p: &Progress) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    std::fs::write(file, text).map_err(|e| e.to_string())
}
