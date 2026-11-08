//! The MCP tools: `scene.*` (objects and components of the open scenes), `asset.*` (project
//! files) and `editor.*` (menus, Play mode, Console, screenshots, tests, builds, undo).
//!
//! Each tool has a JSON schema for its arguments (checked before it runs) and returns an
//! [`Outcome`]: a result now, a [`Job`] that finishes over the next frames (Play mode, tests,
//! builds), or a confirmation the live editor asks the user for (destructive tools).

mod asset;
mod editor;
mod scene;

use misty_engine::hecs::Entity;
use serde_json::{Map, Value, json};

use crate::state::EditorState;

/// What a tool runs against.
pub struct Cx<'a> {
    pub s: &'a mut EditorState,
    /// The live editor (Play mode, window commands, confirmations) rather than `misty --mcp`.
    pub live: bool,
    pub shots: &'a mut dyn super::shot::Shooter,
}

/// A tool's answer: content blocks, an optional JSON object, and whether it failed.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub content: Vec<Value>,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl ToolResult {
    /// A JSON value as text (and as `structuredContent` when it is an object).
    pub fn json(v: Value) -> Self {
        let text = serde_json::to_string_pretty(&v).unwrap_or_default();
        Self {
            content: vec![json!({ "type": "text", "text": text })],
            structured: v.is_object().then_some(v),
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": message.into() })],
            structured: None,
            is_error: true,
        }
    }

    /// A PNG image with a caption.
    pub fn image(png: &[u8], caption: String) -> Self {
        Self {
            content: vec![
                json!({ "type": "image", "data": super::base64(png), "mimeType": "image/png" }),
                json!({ "type": "text", "text": caption }),
            ],
            structured: None,
            is_error: false,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut out = json!({ "content": self.content, "isError": self.is_error });
        if let Some(s) = &self.structured {
            out["structuredContent"] = s.clone();
        }
        out
    }

    /// The first text block.
    #[cfg(test)]
    pub fn text(&self) -> &str {
        self.content
            .iter()
            .find_map(|c| c.get("text").and_then(Value::as_str))
            .unwrap_or("")
    }
}

/// Work that finishes over later frames.
pub trait Job {
    /// `Some` once finished. Called once per editor frame (or every few ms headless).
    fn poll(&mut self, cx: &mut Cx<'_>) -> Option<ToolResult>;
}

/// The rest of a tool call after the user said Allow.
pub type Then = Box<dyn FnOnce(&mut Cx<'_>) -> Outcome>;

pub enum Outcome {
    Done(ToolResult),
    Wait(Box<dyn Job>),
    /// Destructive: the live editor asks "AI wants to {what}" first. `misty --mcp` runs it
    /// right away (the MCP client asks the user before calling destructive tools).
    Confirm {
        what: String,
        then: Then,
    },
}

impl Outcome {
    pub fn ok(v: Value) -> Self {
        Outcome::Done(ToolResult::json(v))
    }
}

/// How a tool touches the project (MCP tool annotations).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Reads only.
    Read,
    /// Changes the scene or adds files; undoable or harmless.
    Edit,
    /// Deletes or overwrites, or runs a build: confirmed in the live editor.
    Destructive,
}

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub kind: Kind,
    /// Same arguments, same effect.
    pub idempotent: bool,
    pub schema: fn() -> Value,
    pub run: fn(&mut Cx<'_>, &Value) -> Result<Outcome, String>,
}

/// Every tool, in listing order.
pub fn all() -> Vec<&'static Tool> {
    scene::TOOLS
        .iter()
        .chain(asset::TOOLS)
        .chain(editor::TOOLS)
        .collect()
}

/// A tool by name; `scene_get_object` also finds `scene.get_object` (clients that only allow
/// `[A-Za-z0-9_-]` in names).
pub fn find(name: &str) -> Option<&'static Tool> {
    all()
        .into_iter()
        .find(|t| t.name == name || t.name.replace('.', "_") == name)
}

/// `tools/list` entries.
pub fn list() -> Vec<Value> {
    all()
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": {
                    "title": t.title,
                    "readOnlyHint": t.kind == Kind::Read,
                    "destructiveHint": t.kind == Kind::Destructive,
                    "idempotentHint": t.idempotent,
                    "openWorldHint": false,
                },
            })
        })
        .collect()
}

/// Check the arguments against the schema, then run the tool. Errors become error results the
/// model can read and correct.
pub fn call(tool: &Tool, args: &Value, cx: &mut Cx<'_>) -> Outcome {
    if let Err(e) = validate(&(tool.schema)(), args) {
        return Outcome::Done(ToolResult::error(format!("{}: {e}", tool.name)));
    }
    match (tool.run)(cx, args) {
        Ok(o) => o,
        Err(e) => Outcome::Done(ToolResult::error(e)),
    }
}

/// Top-level argument check: known names, required ones present, JSON types and enums.
pub fn validate(schema: &Value, args: &Value) -> Result<(), String> {
    let empty = Map::new();
    let props = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let args = args.as_object().ok_or("arguments must be an object")?;
    for name in args.keys() {
        if !props.contains_key(name) {
            let known: Vec<&str> = props.keys().map(String::as_str).collect();
            return Err(format!(
                "unknown argument \"{name}\" (expected: {})",
                known.join(", ")
            ));
        }
    }
    for req in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if args.get(req).is_none_or(Value::is_null) {
            return Err(format!("missing argument \"{req}\""));
        }
    }
    for (name, v) in args {
        if v.is_null() {
            continue;
        }
        let p = &props[name];
        let ok = match p.get("type").and_then(Value::as_str) {
            Some("string") => v.is_string(),
            Some("integer") => v.is_i64() || v.is_u64(),
            Some("number") => v.is_number(),
            Some("boolean") => v.is_boolean(),
            Some("array") => v.is_array(),
            Some("object") => v.is_object(),
            _ => true,
        };
        if !ok {
            return Err(format!(
                "\"{name}\" must be of type {}",
                p["type"].as_str().unwrap_or("?")
            ));
        }
        if let Some(choices) = p.get("enum").and_then(Value::as_array)
            && !choices.contains(v)
        {
            let list: Vec<String> = choices.iter().map(Value::to_string).collect();
            return Err(format!("\"{name}\" must be one of {}", list.join(", ")));
        }
    }
    Ok(())
}

/// `{"type":"object", properties, required}` with no other arguments allowed.
pub fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

// ---- argument helpers ------------------------------------------------------------------------

pub fn str_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing argument \"{name}\""))
}

pub fn opt_str<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str)
}

pub fn opt_bool(args: &Value, name: &str) -> Option<bool> {
    args.get(name).and_then(Value::as_bool)
}

pub fn opt_u64(args: &Value, name: &str) -> Option<u64> {
    args.get(name).and_then(Value::as_u64)
}

/// A `[x, y, z]` argument.
pub fn opt_vec3(args: &Value, name: &str) -> Result<Option<glam::Vec3>, String> {
    let Some(v) = args.get(name).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let nums: Option<Vec<f32>> = v
        .as_array()
        .map(|a| a.iter().map(|x| x.as_f64().map(|f| f as f32)).collect())
        .unwrap_or(None);
    match nums.as_deref() {
        Some([x, y, z]) => Ok(Some(glam::Vec3::new(*x, *y, *z))),
        _ => Err(format!("\"{name}\" must be three numbers [x, y, z]")),
    }
}

// ---- objects -----------------------------------------------------------------------------------

/// `#<bits>`: how tools name an object (stable while the scene stays open).
pub fn object_id(e: Entity) -> String {
    format!("#{}", e.to_bits())
}

/// "Parent/Child/Name".
pub fn object_path(s: &EditorState, e: Entity) -> String {
    let mut parts = vec![s.scene.name(e)];
    let mut cur = s.scene.parent(e);
    while let Some(p) = cur {
        parts.push(s.scene.name(p));
        cur = s.scene.parent(p);
    }
    parts.reverse();
    parts.join("/")
}

/// An object from an id (`#…`), a path (`A/B`) or a unique name.
pub fn resolve(s: &EditorState, r: &str) -> Result<Entity, String> {
    let r = r.trim();
    if let Some(bits) = r.strip_prefix('#') {
        return bits
            .parse::<u64>()
            .ok()
            .and_then(Entity::from_bits)
            .filter(|e| s.scene.contains(*e))
            .ok_or_else(|| format!("no object with id {r} (ids change when a scene reloads)"));
    }
    let matches: Vec<Entity> = if r.contains('/') {
        let walk = s.scene.walk();
        walk.into_iter()
            .map(|(e, _)| e)
            .filter(|e| object_path(s, *e) == r)
            .collect()
    } else {
        s.scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| s.scene.name(*e) == r)
            .collect()
    };
    match matches.as_slice() {
        [e] => Ok(*e),
        [] => Err(format!(
            "no object \"{r}\" (use an id like \"#4294967297\" or a path from scene.list_hierarchy)"
        )),
        many => Err(format!(
            "{} objects match \"{r}\": use an id ({})",
            many.len(),
            many.iter()
                .take(5)
                .map(|e| object_id(*e))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The objects named by a string or a list of strings.
pub fn resolve_many(s: &EditorState, v: &Value) -> Result<Vec<Entity>, String> {
    match v {
        Value::String(r) => Ok(vec![resolve(s, r)?]),
        Value::Array(items) => items
            .iter()
            .map(|i| {
                i.as_str()
                    .ok_or_else(|| "objects must be strings".to_owned())
                    .and_then(|r| resolve(s, r))
            })
            .collect(),
        _ => Err("expected an object reference or a list of them".into()),
    }
}

/// One line about an object: id, name, path, active, component names.
pub fn summary(s: &EditorState, e: Entity) -> Value {
    json!({
        "id": object_id(e),
        "name": s.scene.name(e),
        "path": object_path(s, e),
        "active": s.scene.game_object(e).active,
        "components": s.scene.component_names(e),
    })
}

/// A project-relative path inside the project folder (no `..`, not absolute).
pub fn project_path(path: &str) -> Result<String, String> {
    let p = path.trim().replace('\\', "/");
    let p = p.trim_start_matches("./").to_owned();
    if p.is_empty()
        || p.starts_with('/')
        || p.contains(':')
        || p.split('/').any(|seg| seg == ".." || seg.is_empty())
    {
        return Err(format!(
            "\"{path}\" must be a project-relative path like \"Assets/Scripts/Player.lua\""
        ));
    }
    Ok(p)
}

/// A project path under `Assets/`.
pub fn assets_path(path: &str) -> Result<String, String> {
    let p = project_path(path)?;
    if !p.starts_with("Assets/") {
        return Err(format!(
            "\"{path}\" must be inside Assets/, like \"Assets/Scripts/Player.lua\""
        ));
    }
    Ok(p)
}

/// Shared schema snippets.
pub fn object_ref_schema(what: &str) -> Value {
    json!({
        "type": "string",
        "description": format!("{what}: an id (\"#4294967297\"), a path (\"Parent/Child\") or a unique name"),
    })
}

pub fn vec3_schema(what: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "number" },
        "minItems": 3,
        "maxItems": 3,
        "description": what,
    })
}
