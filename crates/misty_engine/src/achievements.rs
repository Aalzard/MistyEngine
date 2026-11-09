//! Achievements: `Achievements.Unlock("FIRST_WIN")`, `Achievements.IsUnlocked(id)`,
//! `Achievements.Clear(id)` from Lua, stored by a pluggable [`AchievementBackend`].
//!
//! The built-in backend keeps unlocks in a local JSON file: the project's
//! `Library/Achievements.json` in the editor, the per-user folder next to PlayerPrefs in built
//! games. A store integration (Steamworks and the like) implements [`AchievementBackend`] and
//! is installed with [`set_backend`]; Misty has no Steam backend because there is no pure-Rust,
//! permissively licensed Steamworks client to build it on (the usual crates wrap Valve's
//! proprietary C++ SDK).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Project-relative file of the editor's unlocks.
pub const EDITOR_FILE: &str = "Library/Achievements.json";
/// File name of a built game's unlocks (in its per-user folder).
pub const PLAYER_FILE: &str = "achievements.json";

/// Where achievements are stored.
pub trait AchievementBackend: Send {
    /// "Local", "Steam"…
    fn name(&self) -> &str;
    /// Unlock `id`; true when it was not unlocked before.
    fn unlock(&mut self, id: &str) -> Result<bool, String>;
    fn is_unlocked(&self, id: &str) -> bool;
    /// Lock `id` again (testing an achievement).
    fn clear(&mut self, id: &str) -> Result<(), String>;
    /// Every unlocked id.
    fn unlocked(&self) -> Vec<String>;
}

/// The file the local backend writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct LocalFile {
    /// id → Unix time it was unlocked.
    unlocked: BTreeMap<String, u64>,
}

/// Unlocks kept in a JSON file (or only in memory without one).
#[derive(Debug, Default)]
pub struct LocalAchievements {
    path: Option<PathBuf>,
    data: LocalFile,
}

impl LocalAchievements {
    /// Unlocks stored in `path` (read now if it exists).
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            path: Some(path),
            data,
        }
    }

    /// Unlocks that last until the program ends.
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }
}

impl AchievementBackend for LocalAchievements {
    fn name(&self) -> &str {
        "Local"
    }

    fn unlock(&mut self, id: &str) -> Result<bool, String> {
        if self.data.unlocked.contains_key(id) {
            return Ok(false);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.data.unlocked.insert(id.to_owned(), now);
        self.save()?;
        Ok(true)
    }

    fn is_unlocked(&self, id: &str) -> bool {
        self.data.unlocked.contains_key(id)
    }

    fn clear(&mut self, id: &str) -> Result<(), String> {
        if self.data.unlocked.remove(id).is_some() {
            self.save()?;
        }
        Ok(())
    }

    fn unlocked(&self) -> Vec<String> {
        self.data.unlocked.keys().cloned().collect()
    }
}

static BACKEND: Mutex<Option<Box<dyn AchievementBackend>>> = Mutex::new(None);

/// Install the backend achievements go to (a built game's local file, a store integration).
pub fn set_backend(backend: Box<dyn AchievementBackend>) {
    *BACKEND.lock().unwrap_or_else(|e| e.into_inner()) = Some(backend);
}

/// Go back to the default: the open project's `Library/Achievements.json`.
pub fn reset_backend() {
    *BACKEND.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Run `f` on the installed backend (the project's local file when none is installed).
pub fn with_backend<R>(f: impl FnOnce(&mut dyn AchievementBackend) -> R) -> R {
    let mut guard = BACKEND.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(b) => f(b.as_mut()),
        None => {
            let root = crate::assets::absolute("");
            let mut local = if root.as_os_str().is_empty() {
                LocalAchievements::in_memory()
            } else {
                LocalAchievements::open(root.join(EDITOR_FILE))
            };
            f(&mut local)
        }
    }
}

pub fn unlock(id: &str) -> Result<bool, String> {
    with_backend(|b| b.unlock(id))
}

pub fn is_unlocked(id: &str) -> bool {
    with_backend(|b| b.is_unlocked(id))
}

pub fn clear(id: &str) -> Result<(), String> {
    with_backend(|b| b.clear(id))
}

pub fn unlocked() -> Vec<String> {
    with_backend(|b| b.unlocked())
}

pub fn backend_name() -> String {
    with_backend(|b| b.name().to_owned())
}

/// Lua `Achievements`.
pub(crate) fn register_lua(lua: &mlua::Lua) -> mlua::Result<()> {
    use crate::scripting::LogKind;
    let t = lua.create_table()?;
    t.set(
        "Unlock",
        lua.create_function(|lua, id: String| match unlock(&id) {
            Ok(new) => {
                if new {
                    crate::scripting::api::with_world(lua, |w| {
                        w.logs
                            .push((LogKind::Info, format!("Achievement unlocked: {id}")))
                    })?;
                }
                Ok(new)
            }
            Err(e) => Err(mlua::Error::runtime(format!("Achievements.Unlock: {e}"))),
        })?,
    )?;
    t.set(
        "IsUnlocked",
        lua.create_function(|_, id: String| Ok(is_unlocked(&id)))?,
    )?;
    t.set(
        "Clear",
        lua.create_function(|_, id: String| {
            clear(&id).map_err(|e| mlua::Error::runtime(format!("Achievements.Clear: {e}")))
        })?,
    )?;
    t.set(
        "GetUnlocked",
        lua.create_function(|lua, ()| lua.create_sequence_from(unlocked()))?,
    )?;
    t.set(
        "GetBackend",
        lua.create_function(|_, ()| Ok(backend_name()))?,
    )?;
    lua.globals().set("Achievements", t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_unlocks_survive_reopening_and_clear() {
        let path =
            std::env::temp_dir().join(format!("misty_achievements_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut a = LocalAchievements::open(&path);
        assert!(!a.is_unlocked("WIN"));
        assert!(a.unlock("WIN").unwrap());
        assert!(!a.unlock("WIN").unwrap(), "only the first unlock is new");
        let mut b = LocalAchievements::open(&path);
        assert!(b.is_unlocked("WIN"));
        assert_eq!(b.unlocked(), ["WIN"]);
        b.clear("WIN").unwrap();
        assert!(!LocalAchievements::open(&path).is_unlocked("WIN"));
        std::fs::remove_file(path).ok();
    }
}
