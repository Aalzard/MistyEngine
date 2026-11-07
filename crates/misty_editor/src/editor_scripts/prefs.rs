//! `EditorPrefs`: small values editor scripts keep per user, across projects and restarts
//! (`editor_prefs.json` in the per-user Misty Engine folder). Same calls as the game's
//! `PlayerPrefs`: `GetInt` / `SetInt`, `GetFloat` / `SetFloat`, `GetString` / `SetString`,
//! `GetBool` / `SetBool`, `HasKey`, `DeleteKey`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mlua::{Lua, Result};
use serde_json::Value as J;

use super::with_state;

#[derive(Debug, Default)]
pub struct EditorPrefs {
    path: Option<PathBuf>,
    values: BTreeMap<String, J>,
}

/// The user's prefs file.
pub fn default_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::hub::config::config_dir().map(|d| d.join("editor_prefs.json"))
}

impl EditorPrefs {
    pub fn load(path: Option<PathBuf>) -> Self {
        let values = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self { path, values }
    }

    fn get(&self, key: &str) -> Option<&J> {
        self.values.get(key)
    }

    /// Store a value; the file is written only when it changes.
    fn set(&mut self, key: &str, v: J) -> std::result::Result<(), String> {
        if self.values.get(key) == Some(&v) {
            return Ok(());
        }
        self.values.insert(key.to_owned(), v);
        self.save()
    }

    fn delete(&mut self, key: &str) -> std::result::Result<(), String> {
        if self.values.remove(key).is_some() {
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> std::result::Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&self.values).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn set(lua: &Lua, key: &str, v: J) -> Result<()> {
    with_state(lua, |st| st.editor_scripts.prefs.set(key, v))?
        .map_err(|e| mlua::Error::runtime(format!("EditorPrefs: {e}")))
}

fn get(lua: &Lua, key: &str) -> Result<Option<J>> {
    with_state(lua, |st| st.editor_scripts.prefs.get(key).cloned())
}

pub fn register(lua: &Lua) -> Result<()> {
    let p = lua.create_table()?;
    p.set(
        "GetInt",
        lua.create_function(|lua, (k, d): (String, Option<i64>)| {
            Ok(get(lua, &k)?
                .and_then(|v| v.as_f64())
                .map(|f| f.round() as i64)
                .unwrap_or(d.unwrap_or(0)))
        })?,
    )?;
    p.set(
        "SetInt",
        lua.create_function(|lua, (k, v): (String, f64)| set(lua, &k, J::from(v.round() as i64)))?,
    )?;
    p.set(
        "GetFloat",
        lua.create_function(|lua, (k, d): (String, Option<f64>)| {
            Ok(get(lua, &k)?
                .and_then(|v| v.as_f64())
                .unwrap_or(d.unwrap_or(0.0)))
        })?,
    )?;
    p.set(
        "SetFloat",
        lua.create_function(|lua, (k, v): (String, f64)| {
            let n = serde_json::Number::from_f64(v)
                .ok_or_else(|| mlua::Error::runtime("EditorPrefs.SetFloat: not a finite number"))?;
            set(lua, &k, J::Number(n))
        })?,
    )?;
    p.set(
        "GetString",
        lua.create_function(|lua, (k, d): (String, Option<String>)| {
            Ok(get(lua, &k)?
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or(d.unwrap_or_default()))
        })?,
    )?;
    p.set(
        "SetString",
        lua.create_function(|lua, (k, v): (String, String)| set(lua, &k, J::from(v)))?,
    )?;
    p.set(
        "GetBool",
        lua.create_function(|lua, (k, d): (String, Option<bool>)| {
            Ok(get(lua, &k)?
                .and_then(|v| v.as_bool())
                .unwrap_or(d.unwrap_or(false)))
        })?,
    )?;
    p.set(
        "SetBool",
        lua.create_function(|lua, (k, v): (String, bool)| set(lua, &k, J::from(v)))?,
    )?;
    p.set(
        "HasKey",
        lua.create_function(|lua, k: String| Ok(get(lua, &k)?.is_some()))?,
    )?;
    p.set(
        "DeleteKey",
        lua.create_function(|lua, k: String| {
            with_state(lua, |st| st.editor_scripts.prefs.delete(&k))?
                .map_err(|e| mlua::Error::runtime(format!("EditorPrefs: {e}")))
        })?,
    )?;
    lua.globals().set("EditorPrefs", p)?;
    Ok(())
}
