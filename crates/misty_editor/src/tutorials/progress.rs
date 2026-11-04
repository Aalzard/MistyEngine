//! Finished tutorials, saved per user (not per project) in the editor's prefs folder:
//! `%APPDATA%\Misty Engine\Editor\tutorials.json` on Windows, `~/Library/Application Support/…`
//! on macOS, `~/.config/…` elsewhere. The Hub can read the same file.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    let prefs = misty_engine::prefs::default_path("Misty Engine", "Editor")?;
    Some(prefs.with_file_name("tutorials.json"))
}

impl Progress {
    /// Read `file` (missing or broken = nothing finished).
    pub fn load(file: Option<PathBuf>) -> Self {
        let mut p: Progress = file
            .as_deref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
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
