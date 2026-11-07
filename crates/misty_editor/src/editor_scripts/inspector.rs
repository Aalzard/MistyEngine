//! Custom Inspectors for script components.
//!
//! ```lua
//! local OrbiterInspector = CustomInspector("Orbiter")
//! function OrbiterInspector:OnInspectorGUI()
//!     DrawDefaultInspector()
//!     if GUI.Button("Centre on origin") then
//!         self.target.center = Vector3.zero
//!     end
//! end
//! ```
//!
//! `self.target` reads and writes the inspected script's fields (writes are undoable Inspector
//! edits, like typing in the field), and has `gameObject` / `transform`. One Inspector instance
//! lives per inspected script, so plain values on `self` last while it stays selected.

use misty_engine::hecs::Entity;
use misty_engine::scene::{ScriptValue, Scripts};
use misty_engine::scripting::api::{LGameObject, LTransform, with_world};
use mlua::{Function, Lua, MetaMethod, Result, Table, UserData, UserDataMethods, Value};

use super::api::{Registry, fetch, field_to_lua, lua_to_field};
use crate::state::EditorState;

/// `self.target` of a custom Inspector: one script slot on one object.
#[derive(Clone, Debug)]
pub struct ScriptTarget {
    pub entity: Entity,
    pub slot: usize,
    /// Project path of the script file.
    pub path: String,
}

impl ScriptTarget {
    /// The field's saved value, or the script's default.
    fn get(&self, lua: &Lua, key: &str) -> Result<Option<ScriptValue>> {
        let saved = with_world(lua, |w| {
            w.scene
                .world
                .get::<&Scripts>(self.entity)
                .ok()
                .and_then(|s| {
                    s.list
                        .get(self.slot)
                        .and_then(|r| r.fields.get(key).cloned())
                })
        })?;
        if saved.is_some() {
            return Ok(saved);
        }
        Ok(default_of(lua, &self.path, key))
    }
}

fn default_of(lua: &Lua, path: &str, key: &str) -> Option<ScriptValue> {
    super::with_state(lua, |st| script_default(st, path, key)).ok()?
}

fn script_default(st: &EditorState, path: &str, key: &str) -> Option<ScriptValue> {
    let meta = st.assets.scripts.get(path)?;
    let fields = meta.fields.as_ref().ok()?;
    fields
        .iter()
        .find(|(n, _)| n == key)
        .map(|(_, v)| v.clone())
}

impl UserData for ScriptTarget {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, t, key: String| -> Result<Value> {
            match key.as_str() {
                "gameObject" => {
                    return Ok(Value::UserData(lua.create_userdata(LGameObject(t.entity))?));
                }
                "transform" => {
                    return Ok(Value::UserData(lua.create_userdata(LTransform(t.entity))?));
                }
                "scriptPath" => return Ok(Value::String(lua.create_string(&t.path)?)),
                _ => {}
            }
            match t.get(lua, &key)? {
                Some(v) => field_to_lua(lua, &v),
                None => Ok(Value::Nil),
            }
        });
        m.add_meta_method(MetaMethod::NewIndex, |lua, t, (key, v): (String, Value)| {
            let Some(current) = t.get(lua, &key)? else {
                return Err(mlua::Error::runtime(format!(
                    "the script has no public field '{key}'"
                )));
            };
            let Some(new) = lua_to_field(&v, &current) else {
                return Err(mlua::Error::runtime(format!(
                    "'{key}' takes a {}, got {}",
                    kind_name(&current),
                    v.type_name()
                )));
            };
            if new == current {
                return Ok(());
            }
            with_world(lua, |w| {
                if let Ok(mut s) = w.scene.world.get::<&mut Scripts>(t.entity)
                    && let Some(r) = s.list.get_mut(t.slot)
                {
                    r.fields.insert(key.clone(), new);
                }
            })?;
            let label = format!("Change {}", crate::undo_names::humanize(&key));
            super::with_state(lua, |st| st.mark_edited(&label))
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| {
            Ok(format!("ScriptTarget({})", t.path))
        });
    }
}

fn kind_name(v: &ScriptValue) -> &'static str {
    match v {
        ScriptValue::Bool(_) => "boolean",
        ScriptValue::Number(_) => "number",
        ScriptValue::Text(_) => "string",
        ScriptValue::Vector3(_) => "Vector3",
    }
}

/// What `DrawDefaultInspector` draws: the script being inspected (Lua app data, innermost last).
#[derive(Default)]
struct Inspecting(Vec<ScriptTarget>);

pub fn register(lua: &Lua) -> Result<()> {
    lua.globals().set(
        "DrawDefaultInspector",
        lua.create_function(|lua, _: mlua::Variadic<Value>| draw_default(lua))?,
    )
}

/// The built-in field list of the script being inspected, with the editor scene put back
/// for the duration (the Inspector code reads it from the editor state).
fn draw_default(lua: &Lua) -> Result<()> {
    let target = lua
        .app_data_ref::<Inspecting>()
        .and_then(|i| i.0.last().cloned())
        .ok_or_else(|| mlua::Error::runtime("DrawDefaultInspector only works in OnInspectorGUI"))?;
    let scene = with_world(lua, |w| std::mem::take(&mut w.scene))?;
    let scene = super::with_state(lua, |st| {
        st.scene = scene;
        let drawn = super::gui::with_ui(lua, |ui| {
            crate::panels::script_fields(ui, st, target.entity, target.slot, &target.path)
        });
        (std::mem::take(&mut st.scene), drawn)
    })?;
    with_world(lua, |w| w.scene = scene.0)?;
    scene.1
}

/// The Inspector instance for `name` on (`e`, `slot`), created on first use.
pub(crate) fn instance(lua: &Lua, e: Entity, slot: usize, name: &str, path: &str) -> Result<Table> {
    let key = (e.to_bits().get(), slot, name.to_owned());
    let found = lua.app_data_ref::<Registry>().and_then(|r| {
        r.inspector_instances
            .get(&key)
            .and_then(|k| fetch::<Table>(lua, k))
    });
    if let Some(t) = found {
        return Ok(t);
    }
    let class: Table = lua
        .app_data_ref::<Registry>()
        .and_then(|r| {
            r.inspectors
                .iter()
                .find(|i| i.script == name)
                .and_then(|i| fetch(lua, &i.class))
        })
        .ok_or_else(|| mlua::Error::runtime(format!("no custom Inspector for {name}")))?;
    let t = lua.create_table()?;
    t.set(
        "target",
        ScriptTarget {
            entity: e,
            slot,
            path: path.to_owned(),
        },
    )?;
    let meta = lua.create_table()?;
    meta.set("__index", class)?;
    t.set_metatable(Some(meta))?;
    let k = lua.create_registry_value(t.clone())?;
    if let Some(mut r) = lua.app_data_mut::<Registry>() {
        r.inspector_instances.insert(key, k);
    }
    Ok(t)
}

/// Draw the custom Inspector of the script in `slot` of `e`, if there is one. False when the
/// built-in field list should be drawn instead (no custom Inspector, or it failed).
pub fn draw(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    e: Entity,
    slot: usize,
    path: &str,
) -> bool {
    let name = misty_engine::scene::ScriptRef {
        path: path.to_owned(),
        ..Default::default()
    }
    .name();
    if !state.editor_scripts.inspectors.contains(&name) || state.editor_scripts.lua.is_none() {
        return false;
    }
    let site = format!("inspector:{name}");
    if state.editor_scripts.is_broken(&site) {
        ui.horizontal(|ui| {
            ui.colored_label(
                crate::console::LogLevel::Error.color(),
                "Custom Inspector failed",
            );
            if ui.small_button("Try again").clicked() {
                state.editor_scripts.retry(&site);
            }
        });
        return false;
    }
    let target = ScriptTarget {
        entity: e,
        slot,
        path: path.to_owned(),
    };
    let result = super::call(state, super::FRAME_BUDGET, |lua| {
        lua.set_app_data(Inspecting(vec![target]));
        let r = super::gui::run(lua, ui, || {
            let inst = instance(lua, e, slot, &name, path)?;
            match inst.get::<Option<Function>>("OnInspectorGUI")? {
                Some(f) => f.call::<()>(inst).map(|_| true),
                None => Ok(false),
            }
        });
        lua.remove_app_data::<Inspecting>();
        r
    });
    match result {
        Some(Ok(drawn)) => drawn,
        Some(Err(err)) => {
            super::fail(state, &site, &err);
            false
        }
        None => false,
    }
}
