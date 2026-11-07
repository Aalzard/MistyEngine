//! Tool windows: dockable panels drawn by a script's `OnGUI`.
//!
//! ```lua
//! local Counter = ToolWindow("Counter")
//! Counter.count = 0                      -- state: kept across reloads and restarts
//! function Counter:OnGUI()
//!     self.count = GUI.IntField("Count", self.count)
//!     if GUI.Button("Add one") then self.count = self.count + 1 end
//! end
//! MenuItem("Tools/Counter", function() Counter:Open() end)
//! ```
//!
//! Each window is a dock tab (`Tab::Tool(id)`, the id coming from the title), so it docks,
//! floats and is saved in layouts like the built-in panels. Plain values on the window table
//! (numbers, booleans, strings, Vector3, Color) are saved in `Library/ToolWindows.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use mlua::{Function, Lua, Table, Value};

use super::api::{Registry, fetch, from_json, to_json};
use crate::state::EditorState;

const FILE: &str = "ToolWindows.json";
const AUTOSAVE: Duration = Duration::from_secs(2);

/// Saved state of every tool window, by title.
#[derive(Debug, Default)]
pub struct WindowStore {
    pub states: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    dirty: bool,
    last_save: Option<Instant>,
}

fn file(root: &Path) -> PathBuf {
    root.join("Library").join(FILE)
}

impl WindowStore {
    pub fn load(root: &Path) -> Self {
        let states = std::fs::read_to_string(file(root))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            states,
            ..Default::default()
        }
    }

    pub fn save(&mut self, root: &Path) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.last_save = Some(Instant::now());
        let path = file(root);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.states) {
            let _ = std::fs::write(path, text);
        }
    }

    fn put(&mut self, title: &str, state: serde_json::Map<String, serde_json::Value>) {
        if self.states.get(title) != Some(&state) {
            self.states.insert(title.to_owned(), state);
            self.dirty = true;
        }
    }
}

/// Save changed window state now and then.
pub(crate) fn autosave(state: &mut EditorState) {
    let store = &mut state.editor_scripts.window_store;
    if store.dirty && store.last_save.is_none_or(|t| t.elapsed() >= AUTOSAVE) {
        store.save(&state.project.root);
    }
}

/// The plain values set on a window instance (its own keys, not the class's).
fn snapshot(t: &Table) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    for (k, v) in t.pairs::<Value, Value>().flatten() {
        let Value::String(k) = k else { continue };
        let k = k.to_string_lossy();
        if k.starts_with('_') {
            continue;
        }
        if let Some(j) = to_json(&v) {
            out.insert(k, j);
        }
    }
    out
}

/// Save the state of every window instance into the store.
pub(crate) fn capture_all(state: &mut EditorState) {
    let snaps = super::call(state, super::CALL_BUDGET, |lua| {
        lua.app_data_ref::<Registry>()
            .map(|r| {
                r.windows
                    .iter()
                    .filter_map(|w| {
                        let t: Table = fetch(lua, w.instance.as_ref()?)?;
                        Some((w.info.title.clone(), snapshot(&t)))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    for (title, snap) in snaps.unwrap_or_default() {
        state.editor_scripts.window_store.put(&title, snap);
    }
}

/// Make an instance of each registered window class (restoring its saved state), then call
/// its `OnEnable`.
pub(crate) fn create_instances(state: &mut EditorState) {
    let saved = state.editor_scripts.window_store.states.clone();
    super::call(state, super::CALL_BUDGET, |lua| {
        let pending: Vec<(usize, Table, String)> = lua
            .app_data_ref::<Registry>()
            .map(|r| {
                r.windows
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| w.instance.is_none())
                    .filter_map(|(i, w)| {
                        Some((i, fetch::<Table>(lua, &w.class)?, w.info.title.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (i, class, title) in pending {
            let made = (|| -> mlua::Result<Table> {
                let t = lua.create_table()?;
                if let Some(values) = saved.get(&title) {
                    for (k, v) in values {
                        t.set(k.as_str(), from_json(lua, v)?)?;
                    }
                }
                let meta = lua.create_table()?;
                meta.set("__index", class.clone())?;
                t.set_metatable(Some(meta))?;
                Ok(t)
            })();
            let Ok(t) = made else { continue };
            if let Ok(key) = lua.create_registry_value(t.clone())
                && let Some(mut r) = lua.app_data_mut::<Registry>()
                && let Some(w) = r.windows.get_mut(i)
            {
                w.instance = Some(key);
            }
            if let Ok(Some(f)) = class.get::<Option<Function>>("OnEnable")
                && let Err(e) = f.call::<()>(t)
                && let Ok(st) = super::state_mut(lua)
            {
                st.console
                    .log(crate::console::LogLevel::Error, super::describe(&e));
            }
        }
    });
}

pub(crate) fn instance(lua: &Lua, id: u64) -> Option<Table> {
    let r = lua.app_data_ref::<Registry>()?;
    let w = r.windows.iter().find(|w| w.info.id == id)?;
    fetch(lua, w.instance.as_ref()?)
}

/// The tab title of a tool window.
pub fn title(state: &EditorState, id: u64) -> String {
    state
        .editor_scripts
        .window(id)
        .map(|w| w.title.clone())
        .unwrap_or_else(|| "Tool Window".into())
}

/// Draw tool window `id` (its dock tab).
pub fn show(ui: &mut Ui, state: &mut EditorState, id: u64) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(ui.available_height());
            body(ui, state, id);
        });
}

fn body(ui: &mut Ui, state: &mut EditorState, id: u64) {
    let Some(info) = state.editor_scripts.window(id).cloned() else {
        ui.weak("This tool window's script is not loaded (it may have been removed or renamed).");
        return;
    };
    let site = format!("window:{}", info.title);
    if state.editor_scripts.is_broken(&site) {
        ui.label(
            RichText::new("This window's script stopped with an error (see the Console).")
                .color(crate::console::LogLevel::Error.color()),
        );
        ui.horizontal(|ui| {
            if ui.button("Try again").clicked() {
                state.editor_scripts.retry(&site);
            }
            if ui.button("Edit script").clicked() {
                crate::panels::code_editor::open_script(state, &info.file, None);
            }
        });
        return;
    }
    let result = super::call(state, super::FRAME_BUDGET, |lua| {
        let Some(inst) = instance(lua, id) else {
            return Ok(None);
        };
        let Some(f) = inst.get::<Option<Function>>("OnGUI")? else {
            return Ok(None);
        };
        super::gui::run(lua, ui, || f.call::<()>(inst.clone()))?;
        Ok::<_, mlua::Error>(Some(snapshot(&inst)))
    });
    match result {
        Some(Ok(Some(snap))) => state.editor_scripts.window_store.put(&info.title, snap),
        Some(Ok(None)) => {
            ui.weak(format!("{} has no OnGUI function.", info.title));
        }
        Some(Err(e)) => super::fail(state, &site, &e),
        None => {
            ui.weak("Editor scripts are not loaded.");
        }
    }
    if std::mem::take(&mut state.editor_scripts.repaint) {
        ui.ctx().request_repaint();
    }
}
