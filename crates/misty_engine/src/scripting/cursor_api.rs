//! Lua `Cursor` (`lockState`, `visible`) and `CursorLockMode` (`None`, `Locked`, `Confined`).
//! Scripts only record a wish here; the editor's Game view and the player apply it to their
//! window each frame (see `ScriptHost::cursor`).

use mlua::{Lua, Result, Table, Value};

use super::CursorLockMode;
use super::api::with_world;

fn mode_from_name(name: &str) -> Option<CursorLockMode> {
    CursorLockMode::ALL.into_iter().find(|m| m.name() == name)
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let modes = lua.create_table()?;
    for m in CursorLockMode::ALL {
        modes.set(m.name(), m.name())?;
    }
    g.set("CursorLockMode", modes)?;

    let cursor = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| {
            let state = with_world(lua, |w| w.cursor).unwrap_or_default();
            Ok(match key.as_str() {
                "lockState" => Value::String(lua.create_string(state.lock.name())?),
                "visible" => Value::Boolean(state.visible),
                _ => Value::Nil,
            })
        })?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(|lua, (_, key, value): (Table, String, Value)| {
            match key.as_str() {
                "lockState" => {
                    let name = match &value {
                        Value::String(s) => s.to_string_lossy(),
                        other => other.to_string().unwrap_or_default(),
                    };
                    let Some(mode) = mode_from_name(&name) else {
                        return Err(mlua::Error::runtime(format!(
                            "Cursor.lockState: use CursorLockMode.None, Locked or Confined \
                             (got '{name}')"
                        )));
                    };
                    with_world(lua, |w| {
                        // Locking hides the cursor unless a script says otherwise after.
                        if mode == CursorLockMode::Locked && w.cursor.lock != mode {
                            w.cursor.visible = false;
                        }
                        if mode == CursorLockMode::None && w.cursor.lock == CursorLockMode::Locked {
                            w.cursor.visible = true;
                        }
                        w.cursor.lock = mode;
                    })?;
                }
                "visible" => {
                    let on = matches!(value, Value::Boolean(true));
                    with_world(lua, |w| w.cursor.visible = on)?;
                }
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "Cursor has no property '{key}'"
                    )));
                }
            }
            Ok(())
        })?,
    )?;
    cursor.set_metatable(Some(meta))?;
    g.set("Cursor", cursor)?;
    Ok(())
}
