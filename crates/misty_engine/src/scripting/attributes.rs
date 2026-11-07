//! Attributes on script fields: the Inspector's `Header`, `Tooltip`, `Space`, `Range` and
//! `HideInInspector`, and the network's `NetworkVariable`, `ServerRpc` and `ClientRpc`, listed
//! per field (or function) in the class's `_attributes` table.
//!
//! ```lua
//! local Orbiter = { radius = 3, speed = 45, label = "" }
//! Orbiter._attributes = {
//!     radius = { Header("Orbit"), Range(0.5, 10), Tooltip("Metres from the centre") },
//!     label = { HideInInspector() },
//! }
//! Player._attributes = {
//!     score = NetworkVariable(),                                  -- server writes, all read
//!     nickname = NetworkVariable({ write = "Owner", onChanged = "OnNicknameChanged" }),
//!     MoveServerRpc = ServerRpc({ requireOwnership = true, reliable = false }),
//! }
//! ```
//!
//! The functions only build small tables, so they cost nothing in a built game; the editor reads
//! them when it inspects the script.

use std::collections::HashMap;

use mlua::{Lua, Result, Table, Value};

/// How the Inspector shows one script field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldAttributes {
    /// A bold heading drawn above the field.
    pub header: Option<String>,
    /// Hover text of the field's label.
    pub tooltip: Option<String>,
    /// Extra pixels above the field (and its header).
    pub space: f32,
    /// Numbers get a slider between these.
    pub range: Option<(f64, f64)>,
    /// Not shown at all (the value still saves and runs).
    pub hidden: bool,
    /// Replicated by the network (`NetworkVariable(...)`).
    pub network: Option<crate::network::NetVarSpec>,
    /// A function called across the network (`ServerRpc(...)` / `ClientRpc(...)`).
    pub rpc: Option<crate::network::RpcSpec>,
}

/// The class key holding the attributes.
pub const KEY: &str = "_attributes";

/// Register the attribute functions as globals.
pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let tag = |lua: &Lua, kind: &str| -> Result<Table> {
        let t = lua.create_table()?;
        t.set("attribute", kind)?;
        Ok(t)
    };
    g.set(
        "Header",
        lua.create_function(move |lua, text: String| {
            let t = tag(lua, "Header")?;
            t.set("text", text)?;
            Ok(t)
        })?,
    )?;
    g.set(
        "Tooltip",
        lua.create_function(move |lua, text: String| {
            let t = tag(lua, "Tooltip")?;
            t.set("text", text)?;
            Ok(t)
        })?,
    )?;
    g.set(
        "Space",
        lua.create_function(move |lua, px: Option<f64>| {
            let t = tag(lua, "Space")?;
            t.set("pixels", px.unwrap_or(8.0))?;
            Ok(t)
        })?,
    )?;
    g.set(
        "Range",
        lua.create_function(move |lua, (min, max): (f64, f64)| {
            let t = tag(lua, "Range")?;
            t.set("min", min.min(max))?;
            t.set("max", max.max(min))?;
            Ok(t)
        })?,
    )?;
    g.set(
        "HideInInspector",
        lua.create_function(move |lua, ()| tag(lua, "HideInInspector"))?,
    )?;
    // Network attributes keep their option table: `NetworkVariable({ write = "Owner" })`.
    for kind in ["NetworkVariable", "ServerRpc", "ClientRpc"] {
        g.set(
            kind,
            lua.create_function(move |lua, options: Option<Table>| {
                let t = tag(lua, kind)?;
                if let Some(o) = options {
                    t.set("options", o)?;
                }
                Ok(t)
            })?,
        )?;
    }
    Ok(())
}

fn option_str(a: &Table, key: &str) -> Option<String> {
    let o: Table = a.get("options").ok()?;
    o.get::<Option<String>>(key).ok().flatten()
}

fn option_bool(a: &Table, key: &str) -> Option<bool> {
    let o: Table = a.get("options").ok()?;
    o.get::<Option<bool>>(key).ok().flatten()
}

fn network_variable(a: &Table) -> crate::network::NetVarSpec {
    use crate::network::{NetVarSpec, ReadPermission, WritePermission};
    let is = |key: &str, v: &str| option_str(a, key).is_some_and(|s| s.eq_ignore_ascii_case(v));
    NetVarSpec {
        read: if is("read", "Owner") {
            ReadPermission::Owner
        } else {
            ReadPermission::Everyone
        },
        write: if is("write", "Owner") {
            WritePermission::Owner
        } else {
            WritePermission::Server
        },
        on_changed: option_str(a, "onChanged"),
    }
}

fn rpc(a: &Table, kind: crate::network::RpcKind) -> crate::network::RpcSpec {
    let mut spec = crate::network::RpcSpec::new(kind);
    if let Some(b) = option_bool(a, "requireOwnership") {
        spec.require_ownership = b;
    }
    if let Some(b) = option_bool(a, "reliable") {
        spec.reliable = b;
    }
    spec
}

/// The attributes `class._attributes` lists, by field name. Unknown entries are ignored.
pub fn parse(class: &Table) -> HashMap<String, FieldAttributes> {
    let mut out = HashMap::new();
    let Ok(Some(all)) = class.raw_get::<Option<Table>>(KEY) else {
        return out;
    };
    for pair in all.pairs::<String, Value>() {
        let Ok((field, list)) = pair else { continue };
        let mut attrs = FieldAttributes::default();
        // One attribute on its own, or a list of them.
        let items: Vec<Table> = match list {
            Value::Table(t) if t.contains_key("attribute").unwrap_or(false) => vec![t],
            Value::Table(t) => t.sequence_values::<Table>().flatten().collect(),
            _ => continue,
        };
        for a in items {
            let kind: String = a.get("attribute").unwrap_or_default();
            match kind.as_str() {
                "Header" => attrs.header = a.get("text").ok(),
                "Tooltip" => attrs.tooltip = a.get("text").ok(),
                "Space" => attrs.space += a.get::<f64>("pixels").unwrap_or(8.0) as f32,
                "Range" => {
                    if let (Ok(min), Ok(max)) = (a.get::<f64>("min"), a.get::<f64>("max")) {
                        attrs.range = Some((min, max));
                    }
                }
                "HideInInspector" => attrs.hidden = true,
                "NetworkVariable" => attrs.network = Some(network_variable(&a)),
                "ServerRpc" => attrs.rpc = Some(rpc(&a, crate::network::RpcKind::Server)),
                "ClientRpc" => attrs.rpc = Some(rpc(&a, crate::network::RpcKind::Client)),
                _ => {}
            }
        }
        out.insert(field, attrs);
    }
    out
}
