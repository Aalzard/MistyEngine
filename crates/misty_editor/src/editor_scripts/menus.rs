//! Script menu items in the menu bar, their keyboard shortcuts and the Ctrl+K search.
//!
//! `MenuItem("Tools/Scatter Tool", fn)` adds a "Tools" menu after Component; a path under a
//! built-in menu ("GameObject/Align to Floor") goes at the end of that menu. A `validate`
//! function greys the item out while it returns false.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use egui::{Key, KeyboardShortcut, Modifiers, Ui};
use mlua::Function;

use super::MenuInfo;
use super::api::{Registry, fetch};
use crate::actions::Action;
use crate::state::EditorState;

/// The menu bar's own menus, in order.
pub const BUILTIN: [&str; 7] = [
    "File",
    "Edit",
    "Assets",
    "GameObject",
    "Component",
    "Window",
    "Help",
];

/// "Ctrl+Shift+T", "Alt+F5", "Ctrl+," … as a shortcut.
pub fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => mods |= Modifiers::COMMAND,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            "" => return None,
            _ => {
                if key.is_some() {
                    return None;
                }
                key = Key::from_name(part).or_else(|| Key::from_name(&part.to_ascii_uppercase()));
                key?;
            }
        }
    }
    Some(KeyboardShortcut::new(mods, key?))
}

/// Top-level menus the scripts add (not built-in ones), in the order they first appear.
pub fn extra_tops(state: &EditorState) -> Vec<String> {
    let mut seen = Vec::new();
    for m in &state.editor_scripts.menus {
        let top = m.path.split('/').next().unwrap_or("");
        if !BUILTIN.contains(&top) && !seen.iter().any(|s| s == top) {
            seen.push(top.to_owned());
        }
    }
    seen
}

/// Items under menu `top`, in menu order.
fn items_under(state: &EditorState, top: &str) -> Vec<MenuInfo> {
    let mut items: Vec<(usize, MenuInfo)> = state
        .editor_scripts
        .menus
        .iter()
        .enumerate()
        .filter(|(_, m)| m.path.split('/').next() == Some(top))
        .map(|(i, m)| (i, m.clone()))
        .collect();
    items.sort_by_key(|(i, m)| (m.priority, *i));
    items.into_iter().map(|(_, m)| m).collect()
}

/// The script items of menu `top`. Under a built-in menu they follow a separator.
pub fn menu_ui(ui: &mut Ui, state: &mut EditorState, top: &str) {
    let items = items_under(state, top);
    if items.is_empty() {
        return;
    }
    if BUILTIN.contains(&top) {
        ui.separator();
    }
    let rest: Vec<(Vec<String>, MenuInfo)> = items
        .into_iter()
        .map(|m| {
            let segs = m.path.split('/').skip(1).map(str::to_owned).collect();
            (segs, m)
        })
        .collect();
    level(ui, state, &rest);
}

/// One menu level: items, then submenus for longer paths (each once, where it first appears).
fn level(ui: &mut Ui, state: &mut EditorState, items: &[(Vec<String>, MenuInfo)]) {
    let mut done: HashSet<String> = HashSet::new();
    for (segs, m) in items {
        match segs.as_slice() {
            [label] => entry(ui, state, label, m),
            [sub, ..] => {
                if !done.insert(sub.clone()) {
                    continue;
                }
                let inner: Vec<(Vec<String>, MenuInfo)> = items
                    .iter()
                    .filter(|(s, _)| s.len() > 1 && s[0] == *sub)
                    .map(|(s, m)| (s[1..].to_vec(), m.clone()))
                    .collect();
                ui.menu_button(sub, |ui| level(ui, state, &inner));
            }
            [] => {}
        }
    }
}

fn entry(ui: &mut Ui, state: &mut EditorState, label: &str, m: &MenuInfo) {
    let enabled = !m.has_validate || valid(state, &m.path);
    let mut button = egui::Button::new(label);
    if !m.shortcut.is_empty() {
        button = button.shortcut_text(&m.shortcut);
    }
    if ui.add_enabled(enabled, button).clicked() {
        state.push(Action::EditorMenu(m.path.clone()));
        ui.close();
    }
}

fn functions(state: &EditorState, path: &str) -> Option<(Function, Option<Function>)> {
    let lua = state.editor_scripts.lua.as_ref()?;
    let r = lua.app_data_ref::<Registry>()?;
    let m = r.menus.iter().find(|m| m.info.path == path)?;
    let run = fetch(lua, &m.run)?;
    let validate = m.validate.as_ref().and_then(|k| fetch(lua, k));
    Some((run, validate))
}

/// Whether the item's `validate` function allows it now (true without one; false when it
/// errors, which is logged once).
pub fn valid(state: &mut EditorState, path: &str) -> bool {
    let Some((_, Some(validate))) = functions(state, path) else {
        return true;
    };
    let site = format!("validate:{path}");
    if state.editor_scripts.is_broken(&site) {
        return false;
    }
    match super::call(state, super::FRAME_BUDGET, |_| {
        validate.call::<mlua::Value>(())
    }) {
        Some(Ok(v)) => !matches!(v, mlua::Value::Nil | mlua::Value::Boolean(false)),
        Some(Err(e)) => {
            super::fail(state, &site, &e);
            false
        }
        None => false,
    }
}

/// Run a script menu item (after checking its `validate`). False when there is no such item.
pub fn run(state: &mut EditorState, path: &str) -> bool {
    let Some((run, _)) = functions(state, path) else {
        state
            .console
            .warn(format!("No editor script menu item \"{path}\"."));
        return false;
    };
    if !valid(state, path) {
        return true;
    }
    if let Some(Err(e)) = super::call(state, super::CALL_BUDGET, |_| run.call::<()>(())) {
        state
            .console
            .log(crate::console::LogLevel::Error, super::describe(&e));
    }
    true
}

/// Script menu shortcuts (after the built-in ones had their turn; skipped while typing).
pub fn shortcuts(ctx: &egui::Context, state: &mut EditorState) {
    let wanted: Vec<(KeyboardShortcut, String)> = state
        .editor_scripts
        .menus
        .iter()
        .filter_map(|m| Some((parse_shortcut(&m.shortcut)?, m.path.clone())))
        .collect();
    for (shortcut, path) in wanted {
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
            state.push(Action::EditorMenu(path));
        }
    }
}

/// A shortcut label that lives for the program (the search palette's rows want `'static`).
pub fn intern(text: &str) -> &'static str {
    static TEXTS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut set = TEXTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(s) = set.get(text) {
        return s;
    }
    let s: &'static str = Box::leak(text.to_owned().into_boxed_str());
    set.insert(s);
    s
}
