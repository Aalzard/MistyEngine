//! `SendMessage`, `BroadcastMessage` and `SendMessageUpwards` called by scripts.
//!
//! Scripts of the caller's language run at once (Lua through the instance tables, C# by
//! reflection inside .NET); scripts of the other language on the same objects get the message
//! right after the calling callback returns ([`ScriptHost::deliver_messages`], from
//! `flush_spawned`), because neither runtime can call into the other mid-call.

use hecs::Entity;
use mlua::{Function, Lua, Table, Value};

use super::csharp::CsArg;
use super::{LogKind, ScriptHost, World};

/// A message for the scripts of one language on `target`.
#[derive(Debug)]
pub struct QueuedMessage {
    pub target: Entity,
    pub method: String,
    pub arg: Option<CsArg>,
    /// For the C# scripts (from Lua), or the Lua ones (from C#).
    pub to_csharp: bool,
}

/// The objects a message goes to: `dir` 0 the object, 1 it and everything under it, 2 it and
/// its parents. Inactive objects are left out.
pub(super) fn targets(w: &World, e: Entity, dir: i32) -> Vec<Entity> {
    if !w.scene.contains(e) {
        return Vec::new();
    }
    let all = match dir {
        1 => w.scene.subtree(e),
        2 => {
            let mut list = vec![e];
            let mut at = e;
            while let Some(p) = w.scene.parent(at) {
                list.push(p);
                at = p;
            }
            list
        }
        _ => vec![e],
    };
    all.into_iter()
        .filter(|t| w.scene.active_in_hierarchy(*t))
        .collect()
}

/// Whether `e` runs a script of the given language.
fn has_script(w: &World, e: Entity, csharp: bool) -> bool {
    w.scene
        .world
        .get::<&crate::scene::Scripts>(e)
        .is_ok_and(|s| s.list.iter().any(|r| r.path.ends_with(".cs") == csharp))
}

/// Queue a message from C# for the Lua scripts of `e`; whether it has any.
pub(super) fn queue_for_lua(w: &mut World, e: Entity, method: String, arg: Option<CsArg>) -> bool {
    if !w.scene.active_in_hierarchy(e) || !has_script(w, e, false) {
        return false;
    }
    w.messages.push(QueuedMessage {
        target: e,
        method,
        arg,
        to_csharp: false,
    });
    true
}

/// `gameObject:SendMessage(name [, value])` and friends from Lua. Returns whether some script
/// has the method (C# scripts count when the object has any: they are called afterwards).
pub(super) fn lua_send(
    lua: &Lua,
    e: Entity,
    method: &str,
    arg: Value,
    dir: i32,
) -> mlua::Result<bool> {
    let cs_arg = CsArg::from_lua(&arg);
    let (tables, mut found) = super::api::with_world(lua, |w| {
        let mut tables = Vec::new();
        let mut queued = false;
        for t in targets(w, e, dir) {
            if has_script(w, t, true) {
                queued = true;
                w.messages.push(QueuedMessage {
                    target: t,
                    method: method.to_owned(),
                    arg: cs_arg.clone(),
                    to_csharp: true,
                });
            }
            if let Ok(s) = w.scene.world.get::<&crate::scene::Scripts>(t) {
                for r in s.list.iter().filter(|r| !r.path.ends_with(".cs")) {
                    tables.push((t, r.name()));
                }
            }
        }
        (tables, queued)
    })?;
    for (t, name) in tables {
        let Some(table) = super::script_instance(lua, t, &name) else {
            continue;
        };
        let Some(f) = table.get::<Option<Function>>(method)? else {
            continue;
        };
        found = true;
        let result = if arg.is_nil() {
            f.call::<()>(table)
        } else {
            f.call::<()>((table, arg.clone()))
        };
        if let Err(err) = result {
            super::api::with_world(lua, |w| {
                w.logs
                    .push((LogKind::Error, format!("{name} ({method}): {err}")));
            })?;
        }
    }
    Ok(found)
}

fn to_lua(lua: &Lua, arg: &CsArg) -> mlua::Result<Value> {
    Ok(match arg {
        CsArg::Bool(b) => Value::Boolean(*b),
        CsArg::Number(n) => Value::Number(*n),
        CsArg::Int(i) => Value::Integer(*i),
        CsArg::Text(t) => Value::String(lua.create_string(t)?),
    })
}

impl ScriptHost {
    /// Deliver the messages queued for the other language. Whether there were any.
    pub(super) fn deliver_messages(&mut self) -> bool {
        let queued = self.with_world(|w| std::mem::take(&mut w.messages));
        if queued.is_empty() {
            return false;
        }
        for m in queued {
            let alive = self.with_world(|w| {
                w.scene.contains(m.target) && w.scene.active_in_hierarchy(m.target)
            });
            if !alive {
                continue;
            }
            for i in self.instances_of(m.target) {
                let inst = &self.instances[i];
                if inst.cs.is_some() != m.to_csharp {
                    continue;
                }
                if let Some(cs) = inst.cs {
                    let mut w = self
                        .lua
                        .app_data_mut::<World>()
                        .expect("world is installed");
                    self.csharp.message(&mut w, cs, &m.method, m.arg.as_ref());
                    continue;
                }
                let defined = self
                    .lua
                    .registry_value::<Table>(&inst.table)
                    .and_then(|t| t.get::<Option<Function>>(m.method.as_str()))
                    .is_ok_and(|f| f.is_some());
                if defined {
                    let arg = m.arg.as_ref().and_then(|a| to_lua(&self.lua, a).ok());
                    self.call(i, &m.method, arg);
                }
            }
        }
        true
    }
}
