//! `PlayerPrefs`: small per-user values a game keeps between runs (settings, high scores), and
//! the player's chosen quality level and resolution.
//!
//! Stored as JSON in the user's settings folder, one file per game:
//! Windows `%APPDATA%\<Company>\<Product>\prefs.json`, macOS
//! `~/Library/Application Support/<Company>/<Product>/prefs.json`, Linux
//! `$XDG_CONFIG_HOME/<Company>/<Product>/prefs.json` (or `~/.config/…`).
//! Nothing is written until `Save()` (or quitting the player).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrefValue {
    Int(i64),
    Float(f64),
    String(String),
}

#[derive(Default)]
struct Store {
    file: Option<PathBuf>,
    values: BTreeMap<String, PrefValue>,
    dirty: bool,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

/// Tests that reopen the store hold this so they don't wipe each other's values.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut s = STORE.lock().unwrap_or_else(|e| e.into_inner());
    f(s.get_or_insert_with(Store::default))
}

/// Folder name without characters file systems reject.
fn clean(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_end_matches('.').to_owned();
    if s.is_empty() { "Game".into() } else { s }
}

/// The per-user prefs file of a game.
pub fn default_path(company: &str, product: &str) -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }?;
    Some(
        base.join(clean(company))
            .join(clean(product))
            .join("prefs.json"),
    )
}

/// Load the prefs file (missing or broken = empty). Later `save` calls write back to it.
pub fn open(file: &Path) {
    let values = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    with(|s| {
        *s = Store {
            file: Some(file.to_path_buf()),
            values,
            dirty: false,
        }
    });
}

pub fn get(key: &str) -> Option<PrefValue> {
    with(|s| s.values.get(key).cloned())
}

pub fn set(key: &str, value: PrefValue) {
    with(|s| {
        if s.values.get(key) != Some(&value) {
            s.values.insert(key.to_owned(), value);
            s.dirty = true;
        }
    });
}

pub fn get_int(key: &str, default: i64) -> i64 {
    match get(key) {
        Some(PrefValue::Int(i)) => i,
        Some(PrefValue::Float(f)) => f as i64,
        _ => default,
    }
}

pub fn get_float(key: &str, default: f64) -> f64 {
    match get(key) {
        Some(PrefValue::Float(f)) => f,
        Some(PrefValue::Int(i)) => i as f64,
        _ => default,
    }
}

pub fn get_string(key: &str, default: &str) -> String {
    match get(key) {
        Some(PrefValue::String(s)) => s,
        _ => default.to_owned(),
    }
}

pub fn has_key(key: &str) -> bool {
    with(|s| s.values.contains_key(key))
}

pub fn delete_key(key: &str) {
    with(|s| s.dirty |= s.values.remove(key).is_some());
}

pub fn delete_all() {
    with(|s| {
        s.dirty |= !s.values.is_empty();
        s.values.clear();
    });
}

/// Write the file if anything changed (no file opened = kept in memory only).
pub fn save() -> Result<(), String> {
    with(|s| {
        let Some(file) = s.file.clone().filter(|_| s.dirty) else {
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&s.values).map_err(|e| e.to_string())?;
        std::fs::write(&file, text + "\n").map_err(|e| format!("{}: {e}", file.display()))?;
        s.dirty = false;
        Ok(())
    })
}

/// Keys the player itself keeps (chosen quality level and window).
pub mod keys {
    pub const QUALITY: &str = "Misty.QualityLevel";
    pub const WIDTH: &str = "Misty.ScreenWidth";
    pub const HEIGHT: &str = "Misty.ScreenHeight";
    pub const SCREEN_MODE: &str = "Misty.ScreenMode";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_through_the_file() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let file = std::env::temp_dir().join(format!("misty_prefs_{}/p.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        open(&file);
        assert_eq!(get_int("score", 7), 7);
        set("score", PrefValue::Int(42));
        set("volume", PrefValue::Float(0.5));
        set("name", PrefValue::String("Ana".into()));
        assert_eq!(get_float("score", 0.0), 42.0);
        assert_eq!(get_string("score", "x"), "x", "wrong type = default");
        save().unwrap();
        open(&file);
        assert_eq!(get_int("score", 0), 42);
        assert_eq!(get_float("volume", 0.0), 0.5);
        assert_eq!(get_string("name", ""), "Ana");
        delete_key("score");
        assert!(!has_key("score"));
        save().unwrap();
        open(&file);
        assert!(!has_key("score") && has_key("name"));
        delete_all();
        assert!(!has_key("name"));
    }

    #[test]
    fn per_user_paths_use_company_and_product() {
        if let Some(p) = default_path("My: Co", "Ball") {
            let s = p.to_string_lossy().replace('\\', "/");
            assert!(s.ends_with("My_ Co/Ball/prefs.json"), "{s}");
        }
    }
}
