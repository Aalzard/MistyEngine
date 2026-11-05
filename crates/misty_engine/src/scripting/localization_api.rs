//! Lua `Localization` and the Localized Text / Saveable components from `GetComponent`.
//!
//! ```lua
//! Localization.locale = "fr"                         -- switch (Localized Texts follow)
//! local s = Localization.Get("score", { 12 })        -- "{0}" / "{name}" arguments
//! local t = Localization.Get("greeting", { name = "Ana" })
//! Localization.OnLocaleChanged:AddListener(function(code) print(code) end)
//! for _, l in ipairs(Localization.locales) do print(l.code, l.name) end
//! ```
//! Scripts may also define `OnLocaleChanged(code)`.

use hecs::Entity;
use mlua::{Function, Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{missing, with_world};
use crate::localization::{Args, LocalizedText, is_right_to_left};

const LISTENERS: &str = "misty.localization.listeners";

fn listener_table(lua: &Lua) -> Result<Table> {
    match lua.named_registry_value::<Option<Table>>(LISTENERS)? {
        Some(t) => Ok(t),
        None => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(LISTENERS, &t)?;
            Ok(t)
        }
    }
}

/// Functions added with `Localization.OnLocaleChanged:AddListener`.
pub fn listeners(lua: &Lua) -> Vec<Function> {
    listener_table(lua)
        .map(|t| t.sequence_values::<Function>().flatten().collect())
        .unwrap_or_default()
}

/// A Lua value as argument text (whole numbers without ".0").
fn arg_text(v: &Value) -> String {
    match v {
        Value::Integer(i) => i.to_string(),
        Value::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => (*n as i64).to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_string_lossy(),
        Value::Boolean(b) => b.to_string(),
        Value::Nil => String::new(),
        other => other.to_string().unwrap_or_default(),
    }
}

/// `{ 1, 2, name = "x" }` → positional and named arguments.
fn args_from(v: Option<&Value>) -> Args {
    let mut a = Args::none();
    match v {
        Some(Value::Table(t)) => {
            for item in t.clone().sequence_values::<Value>().flatten() {
                a.positional.push(arg_text(&item));
            }
            for (k, val) in t.clone().pairs::<Value, Value>().flatten() {
                if let Value::String(k) = k {
                    a.named.insert(k.to_string_lossy(), arg_text(&val));
                }
            }
        }
        Some(Value::Nil) | None => {}
        Some(other) => a.positional.push(arg_text(other)),
    }
    a
}

struct LLocalization;

impl UserData for LLocalization {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("locale", |lua, _| {
            with_world(lua, |w| w.localization.locale())
        });
        f.add_field_method_set("locale", |lua, _, code: String| {
            with_world(lua, |w| w.localization.set_locale(&code))?
                .map(|_| ())
                .map_err(mlua::Error::runtime)
        });
        f.add_field_method_get("locales", |lua, _| {
            let locales = with_world(lua, |w| w.localization.locales())?;
            let list = lua.create_table()?;
            for l in locales {
                let t = lua.create_table()?;
                t.set("code", l.code.as_str())?;
                t.set("name", l.label())?;
                list.push(t)?;
            }
            Ok(list)
        });
        f.add_field_method_get("isRightToLeft", |lua, _| {
            with_world(lua, |w| is_right_to_left(&w.localization.locale()))
        });
        f.add_field_method_get("OnLocaleChanged", |_, _| Ok(LLocaleEvent));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // Called with a dot: `Localization.Get(key, args, table)`.
        m.add_function(
            "Get",
            |lua, (key, args, table): (String, Option<Value>, Option<String>)| {
                let args = args_from(args.as_ref());
                with_world(lua, |w| w.localization.get(table.as_deref(), &key, &args))
            },
        );
        m.add_function("HasKey", |lua, (key, table): (String, Option<String>)| {
            with_world(lua, |w| w.localization.has_key(table.as_deref(), &key))
        });
        // Asset Tables: the asset path for the current locale.
        m.add_function("GetAsset", |lua, (key, table): (String, Option<String>)| {
            with_world(lua, |w| {
                let l = w.localization.locale();
                let fallback = w.localization.settings.fallback_locale.clone();
                w.localization
                    .lookup(table.as_deref(), &key, &l)
                    .or_else(|| w.localization.lookup(table.as_deref(), &key, &fallback))
                    .map(str::to_owned)
            })
        });
    }
}

struct LLocaleEvent;

impl UserData for LLocaleEvent {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("AddListener", |lua, _, f: Function| {
            listener_table(lua)?.push(f)
        });
        m.add_method("RemoveListener", |lua, _, f: Function| {
            let t = listener_table(lua)?;
            let kept: Vec<Function> = t
                .sequence_values::<Function>()
                .flatten()
                .filter(|g| *g != f)
                .collect();
            let new = lua.create_table()?;
            for g in kept {
                new.push(g)?;
            }
            lua.set_named_registry_value(LISTENERS, new)
        });
        m.add_method("RemoveAllListeners", |lua, _, ()| {
            lua.set_named_registry_value(LISTENERS, lua.create_table()?)
        });
    }
}

/// `GetComponent("LocalizedText")`.
#[derive(Clone, Copy)]
pub struct LLocalizedText(pub Entity);

fn edit(lua: &Lua, e: Entity, f: impl FnOnce(&mut LocalizedText)) -> Result<()> {
    with_world(lua, |w| {
        let Ok(mut c) = w.scene.world.get::<&mut LocalizedText>(e) else {
            return Err(missing());
        };
        f(&mut c);
        drop(c);
        // Show the new string right away.
        crate::localization::refresh_texts(&mut w.scene, &w.localization);
        Ok(())
    })?
}

fn read<R>(lua: &Lua, e: Entity, f: impl FnOnce(&LocalizedText) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&LocalizedText>(e)
            .map(|c| f(&c))
            .map_err(|_| missing())
    })?
}

impl UserData for LLocalizedText {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("key", |lua, c| read(lua, c.0, |l| l.key.clone()));
        f.add_field_method_set("key", |lua, c, key: String| edit(lua, c.0, |l| l.key = key));
        f.add_field_method_get("table", |lua, c| read(lua, c.0, |l| l.table.clone()));
        f.add_field_method_set("table", |lua, c, t: String| edit(lua, c.0, |l| l.table = t));
        f.add_field_method_get("enabled", |lua, c| read(lua, c.0, |l| l.enabled));
        f.add_field_method_set("enabled", |lua, c, on: bool| {
            edit(lua, c.0, |l| l.enabled = on)
        });
        f.add_field_method_get("args", |lua, c| {
            let args = read(lua, c.0, |l| l.args.clone())?;
            lua.create_sequence_from(args)
        });
        // `{ 3, name = "Ana" }`: positional values, then `name=value` entries.
        f.add_field_method_set("args", |lua, c, v: Value| {
            let a = args_from(Some(&v));
            let mut list = a.positional;
            list.extend(a.named.into_iter().map(|(k, v)| format!("{k}={v}")));
            edit(lua, c.0, |l| l.args = list)
        });
        f.add_field_method_get("text", |lua, c| {
            with_world(lua, |w| {
                let l = w
                    .scene
                    .world
                    .get::<&LocalizedText>(c.0)
                    .map(|l| (*l).clone())
                    .map_err(|_| missing())?;
                Ok(l.resolve(&w.localization, &w.localization.locale()))
            })?
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("RefreshString", |lua, c, ()| edit(lua, c.0, |_| {}));
    }
}

/// `GetComponent("Saveable")`: its Save ID.
#[derive(Clone, Copy)]
pub struct LSaveable(pub Entity);

impl UserData for LSaveable {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("saveId", |lua, c| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&crate::save::Saveable>(c.0)
                    .map(|s| s.save_id.clone())
                    .map_err(|_| missing())
            })?
        });
    }
}

/// `GetComponent` for the components handled here.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = |f: fn(&hecs::World, Entity) -> bool| with_world(lua, |w| f(&w.scene.world, e));
    let ud = match name {
        "LocalizedText" | "Localized Text" if has(|w, e| w.get::<&LocalizedText>(e).is_ok())? => {
            lua.create_userdata(LLocalizedText(e))?
        }
        "Saveable" if has(|w, e| w.get::<&crate::save::Saveable>(e).is_ok())? => {
            lua.create_userdata(LSaveable(e))?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::UserData(ud)))
}

pub fn register(lua: &Lua) -> Result<()> {
    lua.globals().set("Localization", LLocalization)
}
