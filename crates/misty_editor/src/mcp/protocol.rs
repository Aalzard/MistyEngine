//! JSON-RPC 2.0 for MCP: one message per line, requests answered with a result or an error.
//!
//! [`handle`] turns one incoming line into a [`Step`]: a reply to send now, nothing (a
//! notification), or a tool call whose result comes later (a job still running, or a
//! destructive tool waiting for the user). The transports ([`super::headless`],
//! [`super::live`]) decide how to wait.

use serde_json::{Value, json};

use super::tools::{self, Cx, Outcome};

/// Protocol revisions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// What the server tells the model about the tools as a whole.
const INSTRUCTIONS: &str = concat!(
    "Misty Engine editor tools. Objects are named by id (\"#4294967297\", from scene.list_hierarchy), ",
    "by path (\"Environment/Lamp\") or by a unique name. Scene edits are undoable (editor.undo) and ",
    "mark the scene changed: call scene.save to write the file. Component fields use the names ",
    "scene.get_object shows; Transform also takes \"euler\" (degrees). Paths are project-relative ",
    "(\"Assets/Scripts/Player.lua\")."
);

/// One client connection's state.
#[derive(Default)]
pub struct Session {
    /// `initialize` was answered.
    pub initialized: bool,
    pub protocol_version: Option<String>,
    /// `clientInfo.name` from `initialize`.
    pub client: Option<String>,
}

/// What to do with one incoming message.
pub enum Step {
    /// Send this response now.
    Reply(Value),
    /// A notification or a response: nothing to send.
    Silent,
    /// A `tools/call` still waiting (a job, or the user's Allow / Deny).
    Call { id: Value, outcome: Outcome },
}

pub fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// Handle one line from the client.
pub fn handle(session: &mut Session, line: &str, cx: &mut Cx<'_>) -> Step {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Step::Reply(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
    };
    if msg.is_array() {
        return Step::Reply(error(
            Value::Null,
            INVALID_REQUEST,
            "JSON-RPC batches are not supported",
        ));
    }
    let Some(obj) = msg.as_object() else {
        return Step::Reply(error(Value::Null, INVALID_REQUEST, "expected an object"));
    };
    let id = obj.get("id").cloned();
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return match id {
            Some(id) => Step::Reply(error(id, INVALID_REQUEST, "\"jsonrpc\" must be \"2.0\"")),
            None => Step::Silent,
        };
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response to a request: this server sends none.
        return Step::Silent;
    };
    let Some(id) = id else {
        if method == "notifications/initialized" {
            session.initialized = true;
        }
        return Step::Silent;
    };
    if !(id.is_string() || id.is_i64() || id.is_u64()) {
        return Step::Reply(error(
            Value::Null,
            INVALID_REQUEST,
            "\"id\" must be a string or an integer",
        ));
    }
    let params = obj.get("params").cloned().unwrap_or(json!({}));
    match method {
        "initialize" => Step::Reply(result(id, initialize(session, &params))),
        "ping" => Step::Reply(result(id, json!({}))),
        "tools/list" => Step::Reply(result(id, json!({ "tools": tools::list() }))),
        "tools/call" => call(id, &params, cx),
        _ => Step::Reply(error(
            id,
            METHOD_NOT_FOUND,
            format!("method not found: {method}"),
        )),
    }
}

fn initialize(session: &mut Session, params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    // The client's revision when this server speaks it, else the newest one.
    let version = asked
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    session.protocol_version = Some(version.to_owned());
    session.client = params
        .pointer("/clientInfo/name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    session.initialized = true;
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "misty-engine",
            "title": "Misty Engine",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": INSTRUCTIONS,
    })
}

fn call(id: Value, params: &Value, cx: &mut Cx<'_>) -> Step {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Step::Reply(error(id, INVALID_PARAMS, "tools/call needs \"name\""));
    };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Step::Reply(error(id, INVALID_PARAMS, "\"arguments\" must be an object"));
    }
    let Some(tool) = tools::find(name) else {
        return Step::Reply(error(id, INVALID_PARAMS, format!("unknown tool: {name}")));
    };
    match tools::call(tool, &args, cx) {
        Outcome::Done(r) => Step::Reply(result(id, r.to_json())),
        outcome => Step::Call { id, outcome },
    }
}
