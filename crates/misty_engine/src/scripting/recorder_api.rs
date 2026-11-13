//! Lua `Recorder` (the optional Recorder package): `Start([name])`, `Stop()` and
//! `isRecording`. Scripts only ask; the editor's Recorder starts and stops after the frame (see
//! `crate::recorder::request`). A built game has no Recorder: the calls do nothing there.

use mlua::{Lua, Result, Table, Value};

use crate::recorder::{Request, request};

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let rec = lua.create_table()?;
    // Start every enabled recorder of the Recorder window, or the one called `name`.
    rec.set(
        "Start",
        lua.create_function(|_, name: Option<String>| {
            request(Request::Start(name));
            Ok(())
        })?,
    )?;
    // Stop recording; the files are written in the background.
    rec.set(
        "Stop",
        lua.create_function(|_, ()| {
            request(Request::Stop);
            Ok(())
        })?,
    )?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|_, (_, key): (Table, String)| {
            Ok(match key.as_str() {
                "isRecording" => Value::Boolean(crate::recorder::is_recording()),
                _ => Value::Nil,
            })
        })?,
    )?;
    rec.set_metatable(Some(meta))?;
    g.set("Recorder", rec)?;
    Ok(())
}
