//! The editor scripting API registered in the editor Lua state: `MenuItem`, `ToolWindow`,
//! `CustomInspector`, `AssetProcessor`, `Selection`, `Undo` and `Editor` (dialogs, progress,
//! update callbacks). `GUI`, `Gizmos`, `Handles`, `EditorPrefs`, `AssetDB` and `EditorScene`
//! live in their own modules.
//!
//! Registrations are kept in the [`Registry`] (Lua app data, holding the Lua functions) and
//! copied in plain form to [`EditorScripts`] after every call, so the menu bar and the dock
//! never need the Lua state to draw.

use std::collections::HashMap;

use misty_engine::hecs::Entity;
use misty_engine::scene::{ScriptRef, ScriptValue};
use misty_engine::scripting::api::{LGameObject, LTransform, LVec3, with_world};
use misty_engine::scripting::ui_api::LColor;
use mlua::{Function, Lua, RegistryKey, Result, Table, Value};

use super::{EditorScripts, MenuInfo, Progress, WindowInfo, with_state};
use crate::actions::Action;
use crate::state::EditorState;

pub(crate) struct MenuEntry {
    pub info: MenuInfo,
    pub run: RegistryKey,
    pub validate: Option<RegistryKey>,
}

pub(crate) struct WindowEntry {
    pub info: WindowInfo,
    pub class: RegistryKey,
    pub instance: Option<RegistryKey>,
}

pub(crate) struct InspectorEntry {
    /// Script name (file stem) it is for.
    pub script: String,
    pub class: RegistryKey,
    pub file: String,
}

/// What the editor scripts registered (Lua app data).
#[derive(Default)]
pub(crate) struct Registry {
    pub current_file: String,
    pub menus: Vec<MenuEntry>,
    pub windows: Vec<WindowEntry>,
    pub inspectors: Vec<InspectorEntry>,
    pub processors: Vec<(RegistryKey, String)>,
    /// `Editor.AddUpdate` functions.
    pub updates: Vec<RegistryKey>,
    /// Game script classes read for gizmos and Inspectors (`None` when it failed to load).
    pub game_classes: HashMap<String, Option<RegistryKey>>,
    /// Custom Inspector instances by (object, slot, script).
    pub inspector_instances: HashMap<(u64, usize, String), RegistryKey>,
    pub changed: bool,
}

pub(crate) fn set_current_file(lua: &Lua, rel: &str) {
    if let Some(mut r) = lua.app_data_mut::<Registry>() {
        r.current_file = rel.to_owned();
    }
}

fn current_file(lua: &Lua) -> String {
    lua.app_data_ref::<Registry>()
        .map(|r| r.current_file.clone())
        .unwrap_or_default()
}

fn registry_mut(lua: &Lua) -> Result<mlua::AppDataRefMut<'_, Registry>> {
    lua.app_data_mut::<Registry>()
        .ok_or_else(|| mlua::Error::runtime("editor scripts are not loaded"))
}

/// Copy what the scripts registered to `es` in plain form (after a call changed it).
pub(crate) fn sync_registry(lua: &Lua, es: &mut EditorScripts) {
    let Some(mut r) = lua.app_data_mut::<Registry>() else {
        return;
    };
    if !std::mem::take(&mut r.changed) {
        return;
    }
    es.menus = r.menus.iter().map(|m| m.info.clone()).collect();
    es.windows = r.windows.iter().map(|w| w.info.clone()).collect();
    es.inspectors = r.inspectors.iter().map(|i| i.script.clone()).collect();
}

/// Forget a game script class (its file changed); it is read again on next use.
pub(crate) fn forget_game_class(lua: &Lua, rel: &str) {
    if let Some(mut r) = lua.app_data_mut::<Registry>() {
        r.game_classes.remove(rel);
        r.inspector_instances.clear();
    }
}

/// A Lua value stored in the registry, fetched again.
pub(crate) fn fetch<T: mlua::FromLua>(lua: &Lua, key: &RegistryKey) -> Option<T> {
    lua.registry_value::<T>(key).ok()
}

/// The script name a `CustomInspector` names: "Orbiter" or "Assets/Scripts/Orbiter.lua".
fn script_name(s: &str) -> String {
    let file = s.rsplit(['/', '\\']).next().unwrap_or(s);
    file.strip_suffix(".lua").unwrap_or(file).to_owned()
}

/// The game script class of `rel`, read into the editor state once (until its file changes).
pub(crate) fn game_class(lua: &Lua, rel: &str) -> Result<Option<Table>> {
    let cached = lua.app_data_ref::<Registry>().and_then(|r| {
        r.game_classes
            .get(rel)
            .map(|k| k.as_ref().map(|k| lua.registry_value(k)))
    });
    match cached {
        Some(Some(t)) => return Ok(Some(t?)),
        Some(None) => return Ok(None),
        None => {}
    }
    let abs = with_state(lua, |st| {
        let abs = st.project.root.join(rel);
        st.editor_scripts
            .game_files
            .insert(rel.to_owned(), super::mtime(&abs));
        abs
    })?;
    let loaded = misty_engine::vfs::read_to_string(&abs)
        .map_err(|e| mlua::Error::runtime(format!("{rel}: {e}")))
        .and_then(|src| lua.load(&src).set_name(rel).eval::<Value>());
    let key = match loaded {
        Ok(Value::Table(t)) => Some(lua.create_registry_value(t)?),
        Ok(_) => None,
        // Not reported here: the asset database reports broken scripts, and the game reports
        // what fails at run time.
        Err(_) => None,
    };
    let table = key
        .as_ref()
        .map(|k| lua.registry_value::<Table>(k))
        .transpose()?;
    registry_mut(lua)?.game_classes.insert(rel.to_owned(), key);
    Ok(table)
}

/// Lua value of a script field.
pub(crate) fn field_to_lua(lua: &Lua, v: &ScriptValue) -> Result<Value> {
    Ok(match v {
        ScriptValue::Bool(b) => Value::Boolean(*b),
        ScriptValue::Number(n) => Value::Number(*n),
        ScriptValue::Text(s) => Value::String(lua.create_string(s)?),
        ScriptValue::Vector3(a) => {
            Value::UserData(lua.create_userdata(LVec3(glam::Vec3::from(*a)))?)
        }
    })
}

/// A Lua value as a script field of the same kind as `like` (None when it does not fit).
pub(crate) fn lua_to_field(v: &Value, like: &ScriptValue) -> Option<ScriptValue> {
    Some(match (like, v) {
        (ScriptValue::Bool(_), Value::Boolean(b)) => ScriptValue::Bool(*b),
        (ScriptValue::Number(_), Value::Integer(i)) => ScriptValue::Number(*i as f64),
        (ScriptValue::Number(_), Value::Number(n)) => ScriptValue::Number(*n),
        (ScriptValue::Text(_), Value::String(s)) => ScriptValue::Text(s.to_string_lossy()),
        (ScriptValue::Vector3(_), Value::UserData(ud)) => {
            ScriptValue::Vector3(ud.borrow::<LVec3>().ok()?.0.to_array())
        }
        _ => return None,
    })
}

/// `self` for a game script on `e` outside Play mode: its field values (saved overrides over
/// the class defaults), `gameObject` and `transform`, like a running instance.
pub(crate) fn script_self(
    lua: &Lua,
    class: &Table,
    e: Entity,
    script: &ScriptRef,
) -> Result<Table> {
    let t = lua.create_table()?;
    for (k, v) in &script.fields {
        t.set(k.as_str(), field_to_lua(lua, v)?)?;
    }
    t.set("gameObject", LGameObject(e))?;
    t.set("transform", LTransform(e))?;
    let meta = lua.create_table()?;
    meta.set("__index", class.clone())?;
    t.set_metatable(Some(meta))?;
    Ok(t)
}

/// The object a Lua value stands for (GameObject, Transform or an Inspector target).
pub(crate) fn entity_of(v: &Value) -> Option<Entity> {
    match v {
        Value::UserData(ud) if ud.is::<LGameObject>() => {
            ud.borrow::<LGameObject>().ok().map(|g| g.0)
        }
        Value::UserData(ud) if ud.is::<LTransform>() => ud.borrow::<LTransform>().ok().map(|t| t.0),
        Value::UserData(ud) if ud.is::<super::inspector::ScriptTarget>() => ud
            .borrow::<super::inspector::ScriptTarget>()
            .ok()
            .map(|t| t.entity),
        _ => None,
    }
}

/// Register every editor API table and function.
pub fn register(lua: &Lua) -> Result<()> {
    lua.set_app_data(Registry::default());
    let g = lua.globals();
    g.set("MenuItem", lua.create_function(menu_item)?)?;
    g.set("ToolWindow", lua.create_function(tool_window)?)?;
    g.set("CustomInspector", lua.create_function(custom_inspector)?)?;
    g.set("AssetProcessor", lua.create_function(asset_processor)?)?;
    register_selection(lua)?;
    register_undo(lua)?;
    register_editor(lua)?;
    super::gui::register(lua)?;
    super::gizmos::register(lua)?;
    super::handles::register(lua)?;
    super::inspector::register(lua)?;
    super::prefs::register(lua)?;
    super::scene_api::register(lua)?;
    Ok(())
}

/// `MenuItem(path, fn [, { validate = fn, shortcut = "Ctrl+Shift+T", priority = n }])`.
fn menu_item(lua: &Lua, (path, run, opts): (String, Function, Option<Table>)) -> Result<()> {
    let segments: Vec<&str> = path.split('/').map(str::trim).collect();
    if segments.len() < 2 || segments.iter().any(|s| s.is_empty()) {
        return Err(mlua::Error::runtime(format!(
            "MenuItem: \"{path}\" needs a menu and an item, like \"Tools/My Tool\""
        )));
    }
    let path = segments.join("/");
    let (validate, shortcut, priority) = match &opts {
        Some(o) => (
            o.get::<Option<Function>>("validate")?,
            o.get::<Option<String>>("shortcut")?.unwrap_or_default(),
            o.get::<Option<i32>>("priority")?.unwrap_or(0),
        ),
        None => (None, String::new(), 0),
    };
    if !shortcut.is_empty() && super::menus::parse_shortcut(&shortcut).is_none() {
        return Err(mlua::Error::runtime(format!(
            "MenuItem: unknown shortcut \"{shortcut}\" (write it like \"Ctrl+Shift+T\")"
        )));
    }
    let entry = MenuEntry {
        info: MenuInfo {
            path: path.clone(),
            shortcut,
            has_validate: validate.is_some(),
            priority,
            file: current_file(lua),
        },
        run: lua.create_registry_value(run)?,
        validate: validate.map(|v| lua.create_registry_value(v)).transpose()?,
    };
    let mut r = registry_mut(lua)?;
    r.menus.retain(|m| m.info.path != path);
    r.menus.push(entry);
    r.changed = true;
    Ok(())
}

/// `ToolWindow(title)`: a window class; define `OnGUI` (and `OnEnable`) on it. Plain values
/// on the window (`self.count = 3`) are its state, kept across reloads and editor restarts.
fn tool_window(lua: &Lua, title: String) -> Result<Table> {
    let title = title.trim().to_owned();
    if title.is_empty() {
        return Err(mlua::Error::runtime("ToolWindow: the title is empty"));
    }
    let class = lua.create_table()?;
    class.set("title", title.clone())?;
    let id = super::window_id(&title);
    class.set(
        "Open",
        lua.create_function(move |lua, _: mlua::Variadic<Value>| {
            with_state(lua, |st| {
                st.push(Action::FocusTab(crate::layout::Tab::Tool(id)))
            })
        })?,
    )?;
    class.set(
        "Close",
        lua.create_function(move |lua, _: mlua::Variadic<Value>| {
            with_state(lua, |st| {
                st.push(Action::CloseTab(crate::layout::Tab::Tool(id)))
            })
        })?,
    )?;
    class.set(
        "Repaint",
        lua.create_function(|lua, _: mlua::Variadic<Value>| {
            with_state(lua, |st| st.editor_scripts.repaint = true)
        })?,
    )?;
    let entry = WindowEntry {
        info: WindowInfo {
            id,
            title: title.clone(),
            file: current_file(lua),
        },
        class: lua.create_registry_value(class.clone())?,
        instance: None,
    };
    let mut r = registry_mut(lua)?;
    r.windows.retain(|w| w.info.id != id);
    r.windows.push(entry);
    r.changed = true;
    Ok(class)
}

/// `CustomInspector("ScriptName")`: an Inspector class for that script; define
/// `OnInspectorGUI` (and `OnSceneGUI` for handles). `self.target` is the inspected script.
fn custom_inspector(lua: &Lua, script: String) -> Result<Table> {
    let name = script_name(script.trim());
    if name.is_empty() {
        return Err(mlua::Error::runtime("CustomInspector: name a script"));
    }
    let class = lua.create_table()?;
    class.set("script", name.clone())?;
    let draw: Function = lua.globals().get("DrawDefaultInspector")?;
    class.set(
        "DrawDefaultInspector",
        lua.create_function(move |_, _: mlua::Variadic<Value>| draw.call::<()>(()))?,
    )?;
    let entry = InspectorEntry {
        script: name.clone(),
        class: lua.create_registry_value(class.clone())?,
        file: current_file(lua),
    };
    let mut r = registry_mut(lua)?;
    if let Some(old) = r.inspectors.iter().find(|i| i.script == name)
        && old.file != entry.file
    {
        let msg = format!(
            "{}: replaces the custom Properties view of {name} from {}",
            entry.file, old.file
        );
        drop(r);
        with_state(lua, |st| st.console.warn(msg))?;
        r = registry_mut(lua)?;
    }
    r.inspectors.retain(|i| i.script != name);
    r.inspector_instances.clear();
    r.inspectors.push(entry);
    r.changed = true;
    Ok(class)
}

/// `AssetProcessor()`: define `OnPostprocessAsset(path)` on it; called for each asset that
/// appears in the project (copied in, or created by an editor script).
fn asset_processor(lua: &Lua, _: ()) -> Result<Table> {
    let t = lua.create_table()?;
    let key = lua.create_registry_value(t.clone())?;
    let file = current_file(lua);
    registry_mut(lua)?.processors.push((key, file));
    Ok(t)
}

/// Call each asset processor's `OnPostprocessAsset(path)` for `paths`.
pub(crate) fn postprocess(state: &mut EditorState, paths: &[String]) {
    let paths: Vec<String> = paths
        .iter()
        .filter(|p| !p.ends_with(".meta"))
        .cloned()
        .collect();
    if paths.is_empty() {
        return;
    }
    super::call(state, super::CALL_BUDGET, |lua| {
        let tables: Vec<Table> = lua
            .app_data_ref::<Registry>()
            .map(|r| {
                r.processors
                    .iter()
                    .filter_map(|(k, _)| fetch(lua, k))
                    .collect()
            })
            .unwrap_or_default();
        for t in tables {
            let Ok(Some(f)) = t.get::<Option<Function>>("OnPostprocessAsset") else {
                continue;
            };
            for p in &paths {
                if let Err(e) = f.call::<()>((t.clone(), p.as_str())) {
                    let _ = with_state(lua, |st| {
                        st.console
                            .log(crate::console::LogLevel::Error, super::describe(&e))
                    });
                }
            }
        }
    });
}

/// Run the `Editor.AddUpdate` functions once (each frame). A function that errors, or
/// returns `false`, is removed.
pub(crate) fn run_updates(state: &mut EditorState) {
    let any = state
        .editor_scripts
        .lua
        .as_ref()
        .and_then(|l| l.app_data_ref::<Registry>().map(|r| !r.updates.is_empty()))
        .unwrap_or(false);
    if !any {
        return;
    }
    state.editor_scripts.repaint = true;
    super::call(state, super::CALL_BUDGET, |lua| {
        let fns: Vec<Function> = lua
            .app_data_ref::<Registry>()
            .map(|r| r.updates.iter().filter_map(|k| fetch(lua, k)).collect())
            .unwrap_or_default();
        for f in fns {
            let stop = match f.call::<Value>(()) {
                Ok(Value::Boolean(false)) => true,
                Ok(_) => false,
                Err(e) => {
                    let _ = with_state(lua, |st| {
                        st.console
                            .log(crate::console::LogLevel::Error, super::describe(&e))
                    });
                    true
                }
            };
            if stop {
                remove_update(lua, &f);
            }
        }
    });
}

fn remove_update(lua: &Lua, f: &Function) {
    let Some(mut r) = lua.app_data_mut::<Registry>() else {
        return;
    };
    let p = f.to_pointer();
    let keys = std::mem::take(&mut r.updates);
    r.updates = keys
        .into_iter()
        .filter(|k| {
            lua.registry_value::<Function>(k)
                .map(|g| g.to_pointer() != p)
                .unwrap_or(false)
        })
        .collect();
}

// ---- Selection ---------------------------------------------------------------------------------

/// `Selection.active` (GameObject), `Selection.objects` (list) and `Selection.asset` (a path),
/// readable and assignable; `Selection.Clear()`, `Selection.Contains(go)`.
fn register_selection(lua: &Lua) -> Result<()> {
    let sel = lua.create_table()?;
    sel.set(
        "Clear",
        lua.create_function(|lua, ()| with_state(lua, |st| st.selection.clear()))?,
    )?;
    sel.set(
        "Contains",
        lua.create_function(|lua, v: Value| {
            let e = entity_of(&v);
            Ok(e.is_some_and(|e| with_state(lua, |s| s.selection.contains(e)).unwrap_or(false)))
        })?,
    )?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| -> Result<Value> {
            let (active, list, asset) = with_state(lua, |st| {
                let sel = &st.selection;
                (sel.active(), sel.list().to_vec(), selected_asset(st))
            })?;
            match key.as_str() {
                "active" => match active {
                    Some(e) => Ok(Value::UserData(lua.create_userdata(LGameObject(e))?)),
                    None => Ok(Value::Nil),
                },
                "objects" => {
                    let t = lua.create_table()?;
                    for (i, e) in list.iter().enumerate() {
                        t.set(i + 1, LGameObject(*e))?;
                    }
                    Ok(Value::Table(t))
                }
                "asset" => match asset {
                    Some(p) => Ok(Value::String(lua.create_string(p)?)),
                    None => Ok(Value::Nil),
                },
                "count" => Ok(Value::Integer(list.len() as i64)),
                _ => Ok(Value::Nil),
            }
        })?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(|lua, (_, key, v): (Table, String, Value)| {
            with_state(lua, |st| set_selection(st, &key, &v))?
        })?,
    )?;
    sel.set_metatable(Some(meta))?;
    lua.globals().set("Selection", sel)
}

fn set_selection(st: &mut EditorState, key: &str, v: &Value) -> Result<()> {
    match key {
        "active" => match entity_of(v) {
            Some(e) => st.selection.set(e),
            None if v.is_nil() => st.selection.clear(),
            None => {
                return Err(mlua::Error::runtime("Selection.active takes a GameObject"));
            }
        },
        "objects" => {
            let list: Vec<Entity> = match v {
                Value::Table(t) => t
                    .sequence_values::<Value>()
                    .filter_map(|v| v.ok().as_ref().and_then(entity_of))
                    .collect(),
                _ => Vec::new(),
            };
            st.selection.set_many(list);
        }
        "asset" => match v {
            Value::String(s) => {
                let rel = s.to_string_lossy();
                crate::prefabs::select_asset(st, &rel);
            }
            _ => st.inspector_asset = None,
        },
        _ => {
            return Err(mlua::Error::runtime(format!(
                "Selection.{key} is read-only"
            )));
        }
    }
    Ok(())
}

/// The asset selected in the Project window, as a project path.
fn selected_asset(st: &EditorState) -> Option<String> {
    if let Some(a) = &st.inspector_asset {
        return Some(a.clone());
    }
    let abs = st.project.selected.as_ref()?;
    abs.is_file().then(|| st.assets.relative(abs))
}

// ---- Undo --------------------------------------------------------------------------------------

/// Snapshot the scene for undo, once per call (or per open group).
fn record(lua: &Lua, name: &str) -> Result<()> {
    let label = with_state(lua, |st| {
        st.scene_dirty = true;
        let u = &mut st.editor_scripts.undo;
        if u.recorded {
            return None;
        }
        u.recorded = true;
        Some(u.open.clone().unwrap_or_else(|| name.to_owned()))
    })?;
    let Some(label) = label else {
        return Ok(());
    };
    with_world(lua, |w| {
        with_state(lua, |st| st.undo.push(&label, &w.scene, &st.selection))
    })??;
    Ok(())
}

/// A new call starts a new undo step unless a group is open.
pub(crate) fn begin_call(es: &mut EditorScripts) {
    if es.undo.open.is_none() {
        es.undo.recorded = false;
    }
}

/// `Undo.Record(obj, "Name")` before changing objects (every change until the call returns
/// is one step); `Undo.BeginGroup("Name")` … `Undo.EndGroup()` spans several calls;
/// `Undo.Undo()` / `Undo.Redo()`.
fn register_undo(lua: &Lua) -> Result<()> {
    let undo = lua.create_table()?;
    undo.set(
        "Record",
        lua.create_function(|lua, (_obj, name): (Value, Option<String>)| {
            record(lua, name.as_deref().unwrap_or("Editor Script"))
        })?,
    )?;
    undo.set(
        "BeginGroup",
        lua.create_function(|lua, name: String| {
            with_state(lua, |st| {
                st.editor_scripts.undo.open = Some(name);
                st.editor_scripts.undo.recorded = false;
            })
        })?,
    )?;
    undo.set(
        "EndGroup",
        lua.create_function(|lua, ()| with_state(lua, |st| st.editor_scripts.undo.open = None))?,
    )?;
    undo.set(
        "Undo",
        lua.create_function(|lua, ()| with_state(lua, |st| st.push(Action::Undo)))?,
    )?;
    undo.set(
        "Redo",
        lua.create_function(|lua, ()| with_state(lua, |st| st.push(Action::Redo)))?,
    )?;
    lua.globals().set("Undo", undo)
}

// ---- Editor ------------------------------------------------------------------------------------

/// `Editor.DisplayDialog(title, message, ok [, cancel])` → true for OK;
/// `Editor.ShowProgress(title, info, fraction)` / `Editor.ClearProgress()`;
/// `Editor.AddUpdate(fn)` / `Editor.RemoveUpdate(fn)` (called every editor frame);
/// `Editor.IsPlaying()`, `Editor.ProjectPath()`, `Editor.Repaint()`.
fn register_editor(lua: &Lua) -> Result<()> {
    let ed = lua.create_table()?;
    ed.set(
        "DisplayDialog",
        lua.create_function(
            |lua, (title, message, ok, cancel): (String, String, Option<String>, Option<String>)| {
                display_dialog(lua, &title, &message, ok.as_deref().unwrap_or("OK"), cancel)
            },
        )?,
    )?;
    ed.set(
        "ShowProgress",
        lua.create_function(
            |lua, (title, info, fraction): (String, Option<String>, f32)| {
                with_state(lua, |st| {
                    st.editor_scripts.progress = Some(Progress {
                        title,
                        info: info.unwrap_or_default(),
                        fraction: fraction.clamp(0.0, 1.0),
                    })
                })
            },
        )?,
    )?;
    ed.set(
        "ClearProgress",
        lua.create_function(|lua, ()| with_state(lua, |st| st.editor_scripts.progress = None))?,
    )?;
    ed.set(
        "AddUpdate",
        lua.create_function(|lua, f: Function| {
            let key = lua.create_registry_value(f)?;
            registry_mut(lua)?.updates.push(key);
            Ok(())
        })?,
    )?;
    ed.set(
        "RemoveUpdate",
        lua.create_function(|lua, f: Function| {
            remove_update(lua, &f);
            Ok(())
        })?,
    )?;
    ed.set(
        "IsPlaying",
        lua.create_function(|lua, ()| with_state(lua, |st| st.play.in_play_mode()))?,
    )?;
    ed.set(
        "ProjectPath",
        lua.create_function(|lua, ()| {
            with_state(lua, |st| st.project.root.to_string_lossy().into_owned())
        })?,
    )?;
    ed.set(
        "Repaint",
        lua.create_function(|lua, ()| with_state(lua, |st| st.editor_scripts.repaint = true))?,
    )?;
    lua.globals().set("Editor", ed)
}

fn display_dialog(
    lua: &Lua,
    title: &str,
    message: &str,
    ok: &str,
    cancel: Option<String>,
) -> Result<bool> {
    let scripted = with_state(lua, |st| match &mut st.editor_scripts.dialogs {
        super::DialogMode::Scripted { answers, shown } => {
            shown.push((title.to_owned(), message.to_owned()));
            Some(answers.pop_front().unwrap_or(true))
        }
        super::DialogMode::Native => None,
    })?;
    if let Some(answer) = scripted {
        return Ok(answer);
    }
    let buttons = match cancel {
        Some(c) => rfd::MessageButtons::OkCancelCustom(ok.to_owned(), c),
        None => rfd::MessageButtons::OkCustom(ok.to_owned()),
    };
    let result = super::paused(lua, || {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title(title)
            .set_description(message)
            .set_buttons(buttons)
            .show()
    });
    Ok(match result {
        rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes => true,
        rfd::MessageDialogResult::Custom(s) => s == ok,
        _ => false,
    })
}

/// A plain value of a window or prefs (numbers, booleans, strings, Vector3, Color) as JSON.
pub(crate) fn to_json(v: &Value) -> Option<serde_json::Value> {
    use serde_json::json;
    Some(match v {
        Value::Boolean(b) => json!(b),
        Value::Integer(i) => json!(i),
        Value::Number(n) if n.is_finite() => json!(n),
        Value::String(s) => json!(s.to_string_lossy()),
        Value::UserData(ud) if ud.is::<LVec3>() => {
            let v = ud.borrow::<LVec3>().ok()?.0;
            json!({ "type": "Vector3", "x": v.x, "y": v.y, "z": v.z })
        }
        Value::UserData(ud) if ud.is::<LColor>() => {
            let [r, g, b, a] = ud.borrow::<LColor>().ok()?.0;
            json!({ "type": "Color", "r": r, "g": g, "b": b, "a": a })
        }
        _ => return None,
    })
}

/// The Lua value of [`to_json`]'s output.
pub(crate) fn from_json(lua: &Lua, v: &serde_json::Value) -> Result<Value> {
    use serde_json::Value as J;
    let f = |o: &serde_json::Map<String, J>, k: &str| {
        o.get(k).and_then(J::as_f64).unwrap_or(0.0) as f32
    };
    Ok(match v {
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Number(n.as_f64().unwrap_or(0.0)),
        },
        J::String(s) => Value::String(lua.create_string(s)?),
        J::Object(o) if o.get("type").and_then(J::as_str) == Some("Vector3") => Value::UserData(
            lua.create_userdata(LVec3(glam::Vec3::new(f(o, "x"), f(o, "y"), f(o, "z"))))?,
        ),
        J::Object(o) if o.get("type").and_then(J::as_str) == Some("Color") => {
            Value::UserData(lua.create_userdata(LColor([
                f(o, "r"),
                f(o, "g"),
                f(o, "b"),
                f(o, "a"),
            ]))?)
        }
        _ => Value::Nil,
    })
}
