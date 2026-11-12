//! Lua `SaveSystem` and the `OnSave(data)` / `OnLoad(data)` script callbacks.
//!
//! ```lua
//! SaveSystem.version = 2            -- the game's data version (default 1)
//! SaveSystem.compress = true        -- gzip the save file (default false)
//! SaveSystem.thumbnails = false     -- skip the PNG of the last frame (default true)
//! SaveSystem.OnMigrate = function(data, fromVersion) ... return data end
//! SaveSystem.Save("slot1")  SaveSystem.Load("slot1")  SaveSystem.Delete("slot1")
//! SaveSystem.Exists("slot1")  for _, s in ipairs(SaveSystem.ListSlots()) do ... end
//! ```
//! Scripts on objects with a Saveable component can store their own values:
//! `function Counter:OnSave(data) data.count = self.count end` and
//! `function Counter:OnLoad(data) self.count = data.count end` (numbers, strings, booleans,
//! Vector3 and nested tables).

use std::collections::HashMap;

use hecs::Entity;
use mlua::{Function, Lua, Result, Table, Value};

use super::api::{LVec3, with_world};
use super::{LogKind, World};
use crate::save::{self, CaptureInput};

/// Named registry list of `{ e = entity bits, key = script key, t = instance table }`.
const INSTANCES: &str = "misty.save.instances";

fn instances(lua: &Lua) -> Result<Table> {
    match lua.named_registry_value::<Option<Table>>(INSTANCES)? {
        Some(t) => Ok(t),
        None => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(INSTANCES, &t)?;
            Ok(t)
        }
    }
}

/// Remember a script instance so `Save` can call its `OnSave`.
pub fn track(lua: &Lua, e: Entity, key: &str, table: &Table) -> Result<()> {
    let list = instances(lua)?;
    let item = lua.create_table()?;
    item.set("e", e.to_bits().get() as i64)?;
    item.set("key", key)?;
    item.set("t", table)?;
    list.push(item)
}

/// Forget the instances of destroyed objects.
pub fn untrack(lua: &Lua, doomed: &[Entity]) {
    let Ok(list) = instances(lua) else { return };
    let bits: Vec<i64> = doomed.iter().map(|e| e.to_bits().get() as i64).collect();
    let Ok(new) = lua.create_table() else { return };
    for item in list.sequence_values::<Table>().flatten() {
        let e: i64 = item.get("e").unwrap_or(0);
        if !bits.contains(&e) {
            let _ = new.push(item);
        }
    }
    let _ = lua.set_named_registry_value(INSTANCES, new);
}

/// Every tracked instance: object, script key, table.
pub fn tracked(lua: &Lua) -> Vec<(Entity, String, Table)> {
    let Ok(list) = instances(lua) else {
        return Vec::new();
    };
    list.sequence_values::<Table>()
        .flatten()
        .filter_map(|item| {
            let bits: i64 = item.get("e").ok()?;
            let e = Entity::from_bits(bits as u64)?;
            Some((e, item.get("key").ok()?, item.get("t").ok()?))
        })
        .collect()
}

fn log(lua: &Lua, kind: LogKind, text: String) {
    let _ = with_world(lua, |w| w.logs.push((kind, text)));
}

/// A slot given as a string or a number.
fn slot_name(v: &Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.to_string_lossy()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) if n.fract() == 0.0 => Ok((*n as i64).to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(mlua::Error::runtime("a save slot is a name or a number")),
    }
}

/// `SaveSystem` settings read at call time.
struct Options {
    version: u32,
    compress: bool,
    thumbnails: bool,
    migrate: Option<Function>,
}

fn options(lua: &Lua) -> Options {
    let sys: Option<Table> = lua.globals().get("SaveSystem").ok();
    let get = |k: &str| sys.as_ref().and_then(|t| t.get::<Value>(k).ok());
    Options {
        version: match get("version") {
            // Clamped, not wrapped: 2^32 must not read as version 0 (older than every save).
            Some(Value::Integer(i)) => i.clamp(0, i64::from(u32::MAX)) as u32,
            Some(Value::Number(n)) => n.max(0.0) as u32,
            _ => 1,
        },
        compress: matches!(get("compress"), Some(Value::Boolean(true))),
        thumbnails: !matches!(get("thumbnails"), Some(Value::Boolean(false))),
        migrate: match get("OnMigrate") {
            Some(Value::Function(f)) => Some(f),
            _ => None,
        },
    }
}

/// Write the current game state to `slot`.
pub fn save(lua: &Lua, slot: &str) -> std::result::Result<(), String> {
    let opts = options(lua);
    let (mut data, ids, folder) = with_world(lua, |w: &mut World| {
        let physics = &w.physics;
        let velocity = |e: Entity| {
            physics
                .velocity(e)
                .map(|v| (v, physics.angular_velocity(e).unwrap_or_default()))
        };
        let data = save::capture(
            &w.scene,
            &CaptureInput {
                slot,
                runtime: &w.save,
                velocity: &velocity,
                version: opts.version,
            },
        );
        let ids: HashMap<Entity, String> = save::by_id(&w.scene)
            .into_iter()
            .map(|(k, e)| (e, k))
            .collect();
        (data, ids, w.save.folder(&w.root))
    })
    .map_err(|e| e.to_string())?;
    // Scripts add their own tables (no world borrow held: OnSave may use the API).
    for (e, key, t) in tracked(lua) {
        let Some(id) = ids.get(&e) else { continue };
        let Ok(Some(f)) = t.get::<Option<Function>>("OnSave") else {
            continue;
        };
        let Ok(table) = lua.create_table() else {
            continue;
        };
        if let Err(err) = f.call::<()>((t.clone(), table.clone())) {
            log(lua, LogKind::Error, format!("{key} (OnSave): {err}"));
            continue;
        }
        let v = to_json(&Value::Table(table), 0);
        if v.as_object().is_some_and(|o| !o.is_empty()) || v.is_array() {
            data.objects
                .entry(id.clone())
                .or_default()
                .scripts
                .insert(key, v);
        }
    }
    data.info.thumbnail = opts.thumbnails;
    save::write_slot(&folder, slot, &data, opts.compress)?;
    if opts.thumbnails {
        save::request_thumbnail(save::thumbnail_path(&folder, slot));
    }
    Ok(())
}

/// Read `slot`, run migrations, and queue it to be restored after this frame.
pub fn load(lua: &Lua, slot: &str) -> std::result::Result<(), String> {
    let opts = options(lua);
    let folder = with_world(lua, |w| w.save.folder(&w.root)).map_err(|e| e.to_string())?;
    let mut v = save::read_slot_value(&folder, slot)?;
    let from = save::version_of(&v);
    if from < opts.version {
        save::migrate(&mut v, opts.version);
        if let Some(f) = &opts.migrate {
            let t = to_lua(lua, &v, 0).map_err(|e| e.to_string())?;
            let out: Value = f
                .call((t.clone(), from))
                .map_err(|e| format!("SaveSystem.OnMigrate: {e}"))?;
            v = to_json(if out.is_nil() { &t } else { &out }, 0);
            save::set_version(&mut v, opts.version);
        }
    } else if from > opts.version {
        log(
            lua,
            LogKind::Warning,
            format!(
                "slot '{slot}' was saved by a newer game version ({from} > {})",
                opts.version
            ),
        );
    }
    let data = save::from_value(v)?;
    with_world(lua, |w| w.save.pending_load = Some(data)).map_err(|e| e.to_string())
}

pub fn register(lua: &Lua) -> Result<()> {
    let sys = lua.create_table()?;
    sys.set("version", 1)?;
    sys.set("compress", false)?;
    sys.set("thumbnails", true)?;
    sys.set(
        "Save",
        lua.create_function(|lua, slot: Value| {
            let slot = slot_name(&slot)?;
            match save(lua, &slot) {
                Ok(()) => Ok(true),
                Err(e) => {
                    log(lua, LogKind::Error, format!("SaveSystem.Save: {e}"));
                    Ok(false)
                }
            }
        })?,
    )?;
    sys.set(
        "Load",
        lua.create_function(|lua, slot: Value| {
            let slot = slot_name(&slot)?;
            match load(lua, &slot) {
                Ok(()) => Ok(true),
                Err(e) => {
                    log(lua, LogKind::Error, format!("SaveSystem.Load: {e}"));
                    Ok(false)
                }
            }
        })?,
    )?;
    sys.set(
        "Delete",
        lua.create_function(|lua, slot: Value| {
            let slot = slot_name(&slot)?;
            with_world(lua, |w| save::delete_slot(&w.save.folder(&w.root), &slot))
        })?,
    )?;
    sys.set(
        "Exists",
        lua.create_function(|lua, slot: Value| {
            let slot = slot_name(&slot)?;
            with_world(lua, |w| save::exists(&w.save.folder(&w.root), &slot))
        })?,
    )?;
    sys.set(
        "ListSlots",
        lua.create_function(|lua, ()| {
            let folder = with_world(lua, |w| w.save.folder(&w.root))?;
            let list = lua.create_table()?;
            for info in save::list_slots(&folder) {
                let t = lua.create_table()?;
                t.set("slot", info.slot.as_str())?;
                t.set("time", info.time)?;
                t.set("date", save::format_time(info.time))?;
                t.set("scene", info.scene.as_str())?;
                t.set("playTime", info.play_time)?;
                t.set("playTimeText", save::format_duration(info.play_time))?;
                t.set("version", info.version)?;
                t.set("size", info.size)?;
                if info.thumbnail {
                    let p = save::thumbnail_path(&folder, &info.slot);
                    t.set("thumbnail", p.to_string_lossy().into_owned())?;
                }
                list.push(t)?;
            }
            Ok(list)
        })?,
    )?;
    sys.set(
        "GetPlayTime",
        lua.create_function(|lua, ()| with_world(lua, |w| w.save.play_time()))?,
    )?;
    lua.globals().set("SaveSystem", sys)
}

// ---- Lua ↔ JSON ------------------------------------------------------------------------------

const MAX_DEPTH: usize = 64;

/// Most values one save writes: a table shared many times at each level would otherwise
/// be written out exponentially often (`t.a = s; t.b = s` at every level).
const MAX_VALUES: usize = 1_000_000;

/// A Lua value as JSON: tables with keys 1..n become arrays, others objects; Vector3 becomes
/// `{"$type":"Vector3","x":…}`; functions and other userdata are left out. A table that
/// contains itself is written as `null` where it loops back, and so is anything past
/// [`MAX_VALUES`].
pub fn to_json(v: &Value, depth: usize) -> serde_json::Value {
    let mut walk = Walk {
        path: Vec::new(),
        left: MAX_VALUES,
    };
    json_of(v, depth, &mut walk)
}

/// The tables being written (outermost first) and how many values may still be written.
struct Walk {
    path: Vec<*const std::ffi::c_void>,
    left: usize,
}

fn json_of(v: &Value, depth: usize, walk: &mut Walk) -> serde_json::Value {
    use serde_json::Value as J;
    if depth > MAX_DEPTH || walk.left == 0 {
        return J::Null;
    }
    walk.left -= 1;
    match v {
        Value::Nil => J::Null,
        Value::Boolean(b) => J::Bool(*b),
        Value::Integer(i) => J::from(*i),
        Value::Number(n) => serde_json::Number::from_f64(*n).map_or(J::Null, J::Number),
        Value::String(s) => J::String(s.to_string_lossy()),
        Value::UserData(ud) if ud.is::<LVec3>() => match ud.borrow::<LVec3>() {
            Ok(p) => serde_json::json!({ "$type": "Vector3", "x": p.0.x, "y": p.0.y, "z": p.0.z }),
            Err(_) => J::Null,
        },
        Value::Table(t) => {
            let id = t.to_pointer();
            if walk.path.contains(&id) {
                return J::Null;
            }
            walk.path.push(id);
            let len = t.raw_len();
            let pairs: Vec<(Value, Value)> = t.clone().pairs::<Value, Value>().flatten().collect();
            let is_array = len > 0 && pairs.len() == len;
            let out = if is_array {
                J::Array(
                    (1..=len)
                        .map(|i| {
                            let item = t.raw_get::<Value>(i).unwrap_or(Value::Nil);
                            json_of(&item, depth + 1, walk)
                        })
                        .collect(),
                )
            } else {
                let mut map = serde_json::Map::new();
                for (k, val) in pairs {
                    let key = match &k {
                        Value::String(s) => s.to_string_lossy(),
                        Value::Integer(i) => i.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    if matches!(val, Value::Function(_)) {
                        continue;
                    }
                    map.insert(key, json_of(&val, depth + 1, walk));
                }
                J::Object(map)
            };
            walk.path.pop();
            out
        }
        _ => J::Null,
    }
}

/// JSON back to Lua (integer-looking object keys become integer keys again).
pub fn to_lua(lua: &Lua, v: &serde_json::Value, depth: usize) -> Result<Value> {
    use serde_json::Value as J;
    if depth > MAX_DEPTH {
        return Ok(Value::Nil);
    }
    Ok(match v {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Number(n.as_f64().unwrap_or(0.0)),
        },
        J::String(s) => Value::String(lua.create_string(s)?),
        J::Array(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, to_lua(lua, item, depth + 1)?)?;
            }
            Value::Table(t)
        }
        J::Object(map) if map.get("$type").and_then(J::as_str) == Some("Vector3") => {
            let f = |k: &str| map.get(k).and_then(J::as_f64).unwrap_or(0.0) as f32;
            Value::UserData(lua.create_userdata(LVec3(glam::Vec3::new(f("x"), f("y"), f("z"))))?)
        }
        J::Object(map) => {
            let t = lua.create_table()?;
            for (k, item) in map {
                let val = to_lua(lua, item, depth + 1)?;
                match k.parse::<i64>() {
                    Ok(i) => t.raw_set(i, val)?,
                    Err(_) => t.raw_set(k.as_str(), val)?,
                }
            }
            Value::Table(t)
        }
    })
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Value};

    /// `SaveSystem.version = 2^32` is the largest version, not wrapped round to 0.
    #[test]
    fn a_huge_save_version_is_clamped() {
        let lua = Lua::new();
        lua.load("SaveSystem = { version = 4294967296 }")
            .exec()
            .unwrap();
        assert_eq!(super::options(&lua).version, u32::MAX);
        lua.load("SaveSystem = { version = -3 }").exec().unwrap();
        assert_eq!(super::options(&lua).version, 0);
        lua.load("SaveSystem = { version = 3 }").exec().unwrap();
        assert_eq!(super::options(&lua).version, 3);
    }

    /// `OnSave` returning a table that holds itself, or one shared twice at every level,
    /// saves in bounded time (the shared one cost 2^64 visits: the editor froze on Save).
    #[test]
    fn shared_and_self_holding_tables_save_in_bounded_time() {
        let lua = Lua::new();
        let t: Value = lua
            .load("local t = { n = 1 } t.a = t t.b = t return t")
            .eval()
            .unwrap();
        let j = super::to_json(&t, 0);
        assert_eq!(j["n"], 1);
        assert!(j["a"].is_null() && j["b"].is_null(), "{j}");
        let wide: Value = lua
            .load(
                "local cur = { x = 1 } for _ = 1, 60 do cur = { a = cur, b = cur } end return cur",
            )
            .eval()
            .unwrap();
        let start = std::time::Instant::now();
        let j = super::to_json(&wide, 0);
        assert!(j.is_object());
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
        // Plain data is written whole.
        let plain: Value = lua
            .load("return { 1, 2, { x = 3, list = { 4, 5 } } }")
            .eval()
            .unwrap();
        assert_eq!(
            super::to_json(&plain, 0),
            serde_json::json!([1, 2, { "x": 3, "list": [4, 5] }])
        );
    }
}
