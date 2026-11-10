//! MCP: JSON-RPC protocol, the tools, undo of AI edits, the live bridge's token check and
//! confirmations, the stdio proxy and headless screenshots.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::bridge::{self, Server};
use super::live::Bridge;
use super::protocol::{self, Session, Step};
use super::shot::NoGpu;
use super::tools::{self, Cx, ToolResult};
use crate::project::ProjectBrowser;
use crate::state::EditorState;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_mcp_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    root
}

fn project(name: &str) -> EditorState {
    EditorState::new(ProjectBrowser::open(temp_root(name)))
}

fn cleanup(s: EditorState) {
    let _ = std::fs::remove_dir_all(&s.project.root);
}

/// Call a tool as `misty --mcp` would (confirmations taken, jobs run to the end).
fn call(s: &mut EditorState, tool: &str, args: Value) -> ToolResult {
    let mut shots = NoGpu;
    let mut cx = Cx {
        s,
        live: false,
        shots: &mut shots,
    };
    let t = tools::find(tool).unwrap_or_else(|| panic!("no tool {tool}"));
    super::headless::finish(tools::call(t, &args, &mut cx), &mut cx)
}

/// A successful call's JSON.
fn ok(s: &mut EditorState, tool: &str, args: Value) -> Value {
    let r = call(s, tool, args);
    assert!(!r.is_error, "{tool} failed: {}", r.text());
    r.structured.clone().unwrap_or(Value::Null)
}

fn err(s: &mut EditorState, tool: &str, args: Value) -> String {
    let r = call(s, tool, args);
    assert!(r.is_error, "{tool} should have failed: {}", r.text());
    r.text().to_owned()
}

/// One JSON-RPC message through the protocol layer.
fn rpc(session: &mut Session, s: &mut EditorState, msg: &str) -> Option<Value> {
    let mut shots = NoGpu;
    let mut cx = Cx {
        s,
        live: false,
        shots: &mut shots,
    };
    match protocol::handle(session, msg, &mut cx) {
        Step::Reply(v) => Some(v),
        Step::Silent => None,
        Step::Call { id, outcome } => Some(protocol::result(
            id,
            super::headless::finish(outcome, &mut cx).to_json(),
        )),
    }
}

fn names(s: &EditorState) -> Vec<String> {
    s.scene
        .walk()
        .into_iter()
        .map(|(e, _)| s.scene.name(e))
        .collect()
}

// ---- protocol --------------------------------------------------------------------------------

#[test]
fn initialize_negotiates_the_protocol_version() {
    let mut s = project("init");
    let mut session = Session::default();
    let r = rpc(
        &mut session,
        &mut s,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2"}}}"#,
    )
    .unwrap();
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(r["result"]["serverInfo"]["name"], "misty-engine");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert_eq!(session.client.as_deref(), Some("claude-code"));
    // An unknown revision gets the newest one this server speaks.
    let r = rpc(
        &mut Session::default(),
        &mut s,
        r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
    )
    .unwrap();
    assert_eq!(r["id"], "a");
    assert_eq!(
        r["result"]["protocolVersion"],
        protocol::PROTOCOL_VERSIONS[0]
    );
    // Notifications get no answer; ping an empty result.
    assert!(
        rpc(
            &mut session,
            &mut s,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
        )
        .is_none()
    );
    let r = rpc(
        &mut session,
        &mut s,
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    )
    .unwrap();
    assert_eq!(r["result"], json!({}));
    cleanup(s);
}

#[test]
fn tools_list_has_a_schema_for_every_tool() {
    let mut s = project("list");
    let r = rpc(
        &mut Session::default(),
        &mut s,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
    )
    .unwrap();
    let list = r["result"]["tools"].as_array().unwrap();
    let mut seen = std::collections::HashSet::new();
    for t in list {
        let name = t["name"].as_str().unwrap();
        assert!(seen.insert(name), "{name} listed twice");
        let prefix = name.split('.').next().unwrap();
        assert!(["scene", "asset", "editor"].contains(&prefix), "{name}");
        assert!(
            name.len() <= 64
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._".contains(c))
        );
        assert!(!t["description"].as_str().unwrap().is_empty(), "{name}");
        let schema = &t["inputSchema"];
        assert_eq!(schema["type"], "object", "{name}");
        assert!(schema["properties"].is_object(), "{name}");
        for req in schema["required"].as_array().unwrap() {
            assert!(
                schema["properties"].get(req.as_str().unwrap()).is_some(),
                "{name}: {req}"
            );
        }
        assert!(t["annotations"]["readOnlyHint"].is_boolean());
    }
    for wanted in [
        "scene.list_hierarchy",
        "scene.get_object",
        "scene.create_object",
        "scene.instantiate_prefab",
        "scene.set_field",
        "scene.add_component",
        "scene.remove_component",
        "scene.delete_objects",
        "scene.select",
        "scene.save",
        "scene.open",
        "asset.search",
        "asset.read_text",
        "asset.create_script",
        "asset.create_material",
        "asset.import",
        "editor.run_menu_item",
        "editor.play",
        "editor.stop",
        "editor.pause",
        "editor.read_console",
        "editor.screenshot",
        "editor.run_tests",
        "editor.build",
        "editor.undo",
    ] {
        assert!(seen.contains(wanted), "missing {wanted}");
    }
    // Destructive tools say so.
    let destructive: Vec<&str> = list
        .iter()
        .filter(|t| t["annotations"]["destructiveHint"] == true)
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(destructive, ["asset.delete", "editor.build"]);
    cleanup(s);
}

#[test]
fn protocol_errors() {
    let mut s = project("errors");
    let mut session = Session::default();
    let mut send = |msg: &str| rpc(&mut session, &mut s, msg).unwrap();
    assert_eq!(send("{not json")["error"]["code"], protocol::PARSE_ERROR);
    assert_eq!(send("[]")["error"]["code"], protocol::INVALID_REQUEST);
    assert_eq!(
        send(r#"{"id":1,"method":"ping"}"#)["error"]["code"],
        protocol::INVALID_REQUEST
    );
    let r = send(r#"{"jsonrpc":"2.0","id":7,"method":"resources/subscribe"}"#);
    assert_eq!(r["id"], 7);
    assert_eq!(r["error"]["code"], protocol::METHOD_NOT_FOUND);
    let r =
        send(r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"scene.nope"}}"#);
    assert_eq!(r["error"]["code"], protocol::INVALID_PARAMS);
    let r = send(r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{}}"#);
    assert_eq!(r["error"]["code"], protocol::INVALID_PARAMS);
    // Bad arguments are a tool error the model can read, not a protocol error.
    let r = send(
        r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"scene.get_object","arguments":{"objekt":"x"}}}"#,
    );
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("unknown argument \"objekt\""), "{text}");
    let r = send(
        r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"scene.create_object","arguments":{"kind":"dragon"}}}"#,
    );
    assert_eq!(r["result"]["isError"], true);
    let r = send(
        r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"scene.get_object","arguments":{"object":"Nobody"}}}"#,
    );
    assert_eq!(r["result"]["isError"], true);
    cleanup(s);
}

#[test]
fn tools_call_happy_path_and_underscore_names() {
    let mut s = project("happy");
    let mut session = Session::default();
    let r = rpc(
        &mut session,
        &mut s,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"scene_create_object","arguments":{"name":"Probe","kind":"cube","position":[1,2,3]}}}"#,
    )
    .unwrap();
    assert_eq!(r["id"], 3);
    assert_eq!(r["result"]["isError"], false);
    let created = &r["result"]["structuredContent"];
    assert_eq!(created["name"], "Probe");
    assert!(
        created["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "Mesh Renderer")
    );
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"Probe\""));
    let e = tools::resolve(&s, "Probe").unwrap();
    assert_eq!(
        s.scene.transform(e).position,
        glam::Vec3::new(1.0, 2.0, 3.0)
    );
    cleanup(s);
}

// ---- scene tools -----------------------------------------------------------------------------

#[test]
fn scene_tools_edit_objects_and_components() {
    let mut s = project("scene");
    let lamp = ok(
        &mut s,
        "scene.create_object",
        json!({"name": "Lamp", "kind": "point_light", "position": [0, 2, 0]}),
    );
    let id = lamp["id"].as_str().unwrap().to_owned();
    let child = ok(
        &mut s,
        "scene.create_object",
        json!({"name": "Shade", "parent": id, "components": ["Box Collider"]}),
    );
    assert_eq!(child["path"], "Lamp/Shade");
    // By path, by id, by name.
    let got = ok(&mut s, "scene.get_object", json!({"object": "Lamp/Shade"}));
    assert_eq!(got["parent"], json!(id));
    assert!(
        got["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "Box Collider")
    );
    // Fields: a component field, a nested element, GameObject and Transform shortcuts.
    let r = ok(
        &mut s,
        "scene.set_field",
        json!({"object": id, "component": "Light", "field": "intensity", "value": 3.5}),
    );
    assert_eq!(r["value"], 3.5);
    ok(
        &mut s,
        "scene.set_field",
        json!({"object": "Lamp", "component": "Light", "field": "color.1", "value": 0.25}),
    );
    let light = ok(&mut s, "scene.get_object", json!({"object": "Lamp"}));
    let fields = light["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Light")
        .unwrap()["fields"]
        .clone();
    assert_eq!(fields["intensity"], 3.5);
    assert_eq!(fields["color"][1], 0.25);
    ok(
        &mut s,
        "scene.set_field",
        json!({"object": "Lamp", "component": "GameObject", "field": "name", "value": "Floor Lamp"}),
    );
    ok(
        &mut s,
        "scene.set_field",
        json!({"object": "Floor Lamp", "component": "Transform", "field": "euler", "value": [0, 90, 0]}),
    );
    let e = tools::resolve(&s, "Floor Lamp").unwrap();
    assert!((s.scene.transform(e).euler().y - 90.0).abs() < 1e-3);
    // Wrong fields and values change nothing.
    let msg = err(
        &mut s,
        "scene.set_field",
        json!({"object": "Floor Lamp", "component": "Light", "field": "brightness", "value": 1}),
    );
    assert!(
        msg.contains("no field \"brightness\"") && msg.contains("intensity"),
        "{msg}"
    );
    let steps = s.undo.steps().len();
    err(
        &mut s,
        "scene.set_field",
        json!({"object": "Floor Lamp", "component": "Light", "field": "intensity", "value": "bright"}),
    );
    assert_eq!(
        s.undo.steps().len(),
        steps,
        "a refused edit records no undo step"
    );
    // Components.
    ok(
        &mut s,
        "scene.add_component",
        json!({"object": "Floor Lamp", "component": "rigidbody"}),
    );
    assert!(s.scene.component_names(e).contains(&"Rigidbody".to_owned()));
    err(
        &mut s,
        "scene.add_component",
        json!({"object": "Floor Lamp", "component": "Rigidbody"}),
    );
    ok(
        &mut s,
        "scene.remove_component",
        json!({"object": "Floor Lamp", "component": "Rigidbody"}),
    );
    assert!(!s.scene.component_names(e).contains(&"Rigidbody".to_owned()));
    err(
        &mut s,
        "scene.remove_component",
        json!({"object": "Floor Lamp", "component": "Transform"}),
    );
    // Scripts are components by path.
    std::fs::create_dir_all(s.project.root.join("Assets/Scripts")).unwrap();
    std::fs::write(
        s.project.root.join("Assets/Scripts/Spin.lua"),
        "local Spin = {}\nSpin.speed = 10\nreturn Spin\n",
    )
    .unwrap();
    ok(
        &mut s,
        "scene.add_component",
        json!({"object": "Floor Lamp", "component": "Assets/Scripts/Spin.lua"}),
    );
    ok(
        &mut s,
        "scene.set_field",
        json!({"object": "Floor Lamp", "component": "Spin", "field": "speed", "value": 42}),
    );
    let got = ok(&mut s, "scene.get_object", json!({"object": "Floor Lamp"}));
    let spin = got["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Spin")
        .unwrap()
        .clone();
    assert_eq!(spin["fields"]["speed"], 42.0);
    ok(
        &mut s,
        "scene.remove_component",
        json!({"object": "Floor Lamp", "component": "Spin"}),
    );
    // Hierarchy listing, filters, parenting, selection, deleting.
    let tree = ok(&mut s, "scene.list_hierarchy", json!({}));
    let roots = tree["scenes"][0]["objects"].as_array().unwrap();
    assert!(
        roots
            .iter()
            .any(|o| o["name"] == "Floor Lamp" && o["children"][0]["name"] == "Shade")
    );
    let lights = ok(&mut s, "scene.list_hierarchy", json!({"filter": "t:Light"}));
    assert_eq!(
        lights["count"], 2,
        "the default directional light and the lamp"
    );
    ok(&mut s, "scene.set_parent", json!({"object": "Shade"}));
    assert!(
        s.scene
            .parent(tools::resolve(&s, "Shade").unwrap())
            .is_none()
    );
    err(
        &mut s,
        "scene.set_parent",
        json!({"object": "Floor Lamp", "parent": "Floor Lamp"}),
    );
    let sel = ok(
        &mut s,
        "scene.select",
        json!({"objects": ["Shade", "Floor Lamp"]}),
    );
    assert_eq!(sel["selection"].as_array().unwrap().len(), 2);
    ok(
        &mut s,
        "scene.delete_objects",
        json!({"objects": ["Shade"]}),
    );
    assert!(!names(&s).contains(&"Shade".to_owned()));
    assert!(s.selection.list().len() == 1);
    let types = ok(
        &mut s,
        "scene.component_types",
        json!({"filter": "collider"}),
    );
    assert!(
        types["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "Box Collider")
    );
    cleanup(s);
}

#[test]
fn ai_edits_undo_and_redo() {
    let mut s = project("undo");
    let before = names(&s);
    ok(
        &mut s,
        "scene.create_object",
        json!({"name": "AI Crate", "kind": "cube"}),
    );
    ok(
        &mut s,
        "scene.set_field",
        json!({"object": "AI Crate", "component": "Transform", "field": "position", "value": [4, 0, 0]}),
    );
    assert_eq!(s.undo.undo_label(), Some("Set Transform position"));
    // The editor's own undo takes the AI's steps back, one at a time.
    assert_eq!(s.undo_step().as_deref(), Some("Set Transform position"));
    let e = tools::resolve(&s, "AI Crate").unwrap();
    assert_ne!(s.scene.transform(e).position.x, 4.0);
    // And so does editor.undo.
    let r = ok(&mut s, "editor.undo", json!({}));
    assert_eq!(r["undone"], json!(["Create Cube"]));
    assert_eq!(names(&s), before);
    let r = ok(&mut s, "editor.redo", json!({"steps": 2}));
    assert_eq!(r["redone"].as_array().unwrap().len(), 2);
    let e = tools::resolve(&s, "AI Crate").unwrap();
    assert_eq!(s.scene.transform(e).position.x, 4.0);
    ok(
        &mut s,
        "scene.delete_objects",
        json!({"objects": ["AI Crate"]}),
    );
    ok(&mut s, "editor.undo", json!({}));
    assert!(
        names(&s).contains(&"AI Crate".to_owned()),
        "a deleted object comes back"
    );
    cleanup(s);
}

#[test]
fn save_open_and_unsaved_changes() {
    let mut s = project("save");
    ok(
        &mut s,
        "scene.create_object",
        json!({"name": "Saved Thing", "kind": "sphere"}),
    );
    let msg = err(&mut s, "scene.save", json!({}));
    assert!(msg.contains("no file yet"), "{msg}");
    let r = ok(&mut s, "scene.save", json!({"path": "Assets/Scenes/Level"}));
    assert_eq!(r["saved"], json!(["Assets/Scenes/Level.scene"]));
    assert!(s.project.root.join("Assets/Scenes/Level.scene").is_file());
    assert!(!s.scene_dirty);
    // Changes after saving: opening refuses until saved or discarded.
    ok(
        &mut s,
        "scene.create_object",
        json!({"name": "Unsaved Thing"}),
    );
    let msg = err(
        &mut s,
        "scene.open",
        json!({"path": "Assets/Scenes/Level.scene"}),
    );
    assert!(msg.contains("unsaved changes"), "{msg}");
    ok(
        &mut s,
        "scene.open",
        json!({"path": "Assets/Scenes/Level.scene", "discard_changes": true}),
    );
    let all = names(&s);
    assert!(all.contains(&"Saved Thing".to_owned()));
    assert!(!all.contains(&"Unsaved Thing".to_owned()));
    // Plain saves go to the scene's file.
    ok(&mut s, "scene.create_object", json!({"name": "Second"}));
    ok(&mut s, "scene.save", json!({}));
    let text = std::fs::read_to_string(s.project.root.join("Assets/Scenes/Level.scene")).unwrap();
    assert!(text.contains("Second"));
    err(
        &mut s,
        "scene.open",
        json!({"path": "Assets/Scenes/Missing.scene"}),
    );
    err(&mut s, "scene.save", json!({"path": "../outside.scene"}));
    cleanup(s);
}

// ---- asset tools -----------------------------------------------------------------------------

#[test]
fn asset_tools_create_read_and_search() {
    let mut s = project("assets");
    let r = ok(
        &mut s,
        "asset.create_script",
        json!({"path": "Assets/Scripts/Mover"}),
    );
    assert_eq!(r["path"], "Assets/Scripts/Mover.lua");
    assert_eq!(r["syntax_error"], Value::Null);
    let text = std::fs::read_to_string(s.project.root.join("Assets/Scripts/Mover.lua")).unwrap();
    assert!(text.contains("Mover"), "the template uses the script name");
    // Existing files need overwrite (headless: no question to ask).
    err(
        &mut s,
        "asset.create_script",
        json!({"path": "Assets/Scripts/Mover.lua", "source": "x"}),
    );
    let r = ok(
        &mut s,
        "asset.create_script",
        json!({"path": "Assets/Scripts/Mover.lua", "source": "local function (", "overwrite": true}),
    );
    assert!(
        r["syntax_error"]
            .as_str()
            .is_some_and(|e| e.contains("Mover.lua")),
        "{r}"
    );
    assert_eq!(r["replaced"], true);
    err(
        &mut s,
        "asset.create_script",
        json!({"path": "Scripts/Outside.lua"}),
    );
    err(
        &mut s,
        "asset.create_script",
        json!({"path": "Assets/../x.lua"}),
    );
    // Materials.
    ok(
        &mut s,
        "asset.create_material",
        json!({"path": "Assets/Materials/Red", "color": [1, 0, 0], "fields": {"metallic": 0.5}}),
    );
    let mat: Value = serde_json::from_str(
        &std::fs::read_to_string(s.project.root.join("Assets/Materials/Red.mat")).unwrap(),
    )
    .unwrap();
    assert_eq!(mat["color"], json!([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(mat["metallic"], 0.5);
    err(
        &mut s,
        "asset.create_material",
        json!({"path": "Assets/Materials/Bad", "fields": {"glitter": 1}}),
    );
    // Reading and searching.
    let r = call(
        &mut s,
        "asset.read_text",
        json!({"path": "Assets/Scripts/Mover.lua"}),
    );
    assert!(!r.is_error);
    assert_eq!(r.content[1]["text"], "local function (");
    err(
        &mut s,
        "asset.read_text",
        json!({"path": "C:/Windows/win.ini"}),
    );
    let found = ok(&mut s, "asset.search", json!({"query": "t:material red"}));
    assert_eq!(found["paths"], json!(["Assets/Materials/Red.mat"]));
    let found = ok(
        &mut s,
        "asset.search",
        json!({"kind": "script", "folder": "Assets/Scripts"}),
    );
    assert_eq!(found["paths"], json!(["Assets/Scripts/Mover.lua"]));
    // Import copies a file in, never over another.
    let outside = std::env::temp_dir().join(format!("misty_mcp_import_{}.txt", std::process::id()));
    std::fs::write(&outside, "hello").unwrap();
    let r = ok(
        &mut s,
        "asset.import",
        json!({"source": outside.display().to_string(), "folder": "Assets/Data", "name": "notes.txt"}),
    );
    assert_eq!(r["path"], "Assets/Data/notes.txt");
    assert_eq!(
        std::fs::read_to_string(s.project.root.join("Assets/Data/notes.txt")).unwrap(),
        "hello"
    );
    err(
        &mut s,
        "asset.import",
        json!({"source": outside.display().to_string(), "folder": "Assets/Data", "name": "notes.txt"}),
    );
    err(&mut s, "asset.import", json!({"source": "relative.txt"}));
    let _ = std::fs::remove_file(outside);
    cleanup(s);
}

// ---- editor tools ----------------------------------------------------------------------------

#[test]
fn editor_tools_menus_console_and_state() {
    let mut s = project("editor");
    let items = ok(
        &mut s,
        "editor.list_menu_items",
        json!({"filter": "3D Object/Cube"}),
    );
    assert!(
        items["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["path"] == "Object/3D Object/Cube")
    );
    let r = ok(
        &mut s,
        "editor.run_menu_item",
        json!({"path": "Object > 3D Object > Cube"}),
    );
    assert!(
        r["console"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("Created 'Cube'"))
    );
    assert!(names(&s).contains(&"Cube".to_owned()));
    let msg = err(&mut s, "editor.run_menu_item", json!({"path": "File/Exit"}));
    assert!(msg.contains("closes"), "{msg}");
    err(
        &mut s,
        "editor.run_menu_item",
        json!({"path": "Object/3D Object/Dragon"}),
    );
    // Play mode needs the live editor.
    let msg = err(&mut s, "editor.play", json!({}));
    assert!(msg.contains("--mcp-connect"), "{msg}");
    // Console: newest lines, then only what is new.
    s.console.warn("first warning");
    let all = ok(&mut s, "editor.read_console", json!({"level": "warning"}));
    let next = all["next"].as_u64().unwrap();
    assert_eq!(
        all["entries"].as_array().unwrap().last().unwrap()["message"],
        "first warning"
    );
    s.console.log(crate::console::LogLevel::Error, "boom");
    s.console.info("chatter");
    let new = ok(
        &mut s,
        "editor.read_console",
        json!({"since": next, "level": "error"}),
    );
    let lines: Vec<&str> = new["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["message"].as_str().unwrap())
        .collect();
    assert_eq!(lines, ["boom"]);
    let state = ok(&mut s, "editor.get_state", json!({}));
    assert_eq!(state["play_state"], "edit");
    assert_eq!(state["live_editor"], false);
    assert_eq!(state["selection"][0]["name"], "Cube");
    // Without a GPU the screenshot fails cleanly.
    assert!(call(&mut s, "editor.screenshot", json!({"view": "game"})).is_error);
    // Menu paths from before the round 31 renames still run.
    for old in [
        "GameObject/3D Object/Sphere",
        "gameobject > 3d object > capsule",
        "Component/Physics/Rigidbody",
        "Window/General/Inspector",
        "Window/General/Hierarchy",
    ] {
        ok(&mut s, "editor.run_menu_item", json!({"path": old}));
    }
    assert!(names(&s).contains(&"Sphere".to_owned()));
    assert!(names(&s).contains(&"Capsule".to_owned()));
    cleanup(s);
}

#[test]
fn run_tests_reports_results() {
    let mut s = project("tests");
    std::fs::create_dir_all(s.project.root.join("Assets/Tests")).unwrap();
    std::fs::write(
        s.project.root.join("Assets/Tests/Math.test.lua"),
        r#"describe("Math", function()
    it("adds", function() assert.equal(1 + 1, 2) end)
    it("is wrong", function() assert.equal(1, 2) end)
end)
"#,
    )
    .unwrap();
    let r = ok(&mut s, "editor.run_tests", json!({}));
    assert_eq!(r["total"], 2, "{r}");
    assert_eq!(r["passed"], 1, "{r}");
    assert_eq!(r["failed"], 1, "{r}");
    let failed = r["tests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["status"] == "failed")
        .unwrap();
    assert!(
        failed["failure"].as_str().is_some_and(|f| !f.is_empty()),
        "{r}"
    );
    let one = ok(&mut s, "editor.run_tests", json!({"filter": "adds"}));
    assert_eq!(one["total"], 1, "{one}");
    let none = ok(
        &mut s,
        "editor.run_tests",
        json!({"filter": "no such test"}),
    );
    assert_eq!(none["total"], 0);
    cleanup(s);
}

// ---- headless stdio --------------------------------------------------------------------------

#[test]
fn headless_serves_newline_delimited_json() {
    let root = temp_root("stdio");
    let mut h = super::headless::Headless::open(&root).unwrap();
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"scene.create_object","arguments":{"name":"Piped"}}}"#,
    ]
    .join("\n");
    let mut out = Vec::new();
    super::headless::serve(&mut h, std::io::Cursor::new(input), &mut out).unwrap();
    let lines: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "one reply per request, none for the notification"
    );
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["result"]["structuredContent"]["name"], "Piped");
    let _ = std::fs::remove_dir_all(root);
}

// ---- the live bridge ---------------------------------------------------------------------------

/// Pump the bridge on this thread (the editor's main thread) until `done`.
fn pump_until(b: &mut Bridge, s: &mut EditorState, mut done: impl FnMut(&Bridge) -> bool) {
    let until = Instant::now() + Duration::from_secs(30);
    while !done(b) {
        assert!(Instant::now() < until, "timed out");
        b.pump(s, &mut NoGpu);
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn send_line(w: &mut impl Write, v: Value) {
    writeln!(w, "{v}").unwrap();
    w.flush().unwrap();
}

fn read_json(r: &mut impl BufRead) -> Value {
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON: {line:?}"))
}

#[test]
fn bridge_needs_the_token() {
    let mut s = project("bridge_token");
    let server = Server::listen("right-token".into(), || {}).unwrap();
    let port = server.port;
    assert_eq!(server.info().token, "right-token");
    let mut b = Bridge::default();
    b.set_server(Some(server));
    // A wrong token is refused and nothing it sends is handled.
    let msg = bridge::connect(port, "wrong-token").unwrap_err();
    assert!(msg.contains("refused"), "{msg}");
    let raw = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut w = raw.try_clone().unwrap();
    send_line(&mut w, json!({"token": "guess"}));
    send_line(
        &mut w,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "scene.create_object", "arguments": {"name": "Intruder"}}}),
    );
    let mut r = BufReader::new(raw);
    assert_eq!(read_json(&mut r)["ok"], false);
    let mut rest = String::new();
    assert_eq!(
        r.read_line(&mut rest).unwrap_or(0),
        0,
        "the connection is closed"
    );
    for _ in 0..20 {
        b.pump(&mut s, &mut NoGpu);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!names(&s).contains(&"Intruder".to_owned()));
    assert_eq!(b.clients(), 0);
    // The right token works, and the edit is on the editor's undo stack.
    let client = std::thread::spawn(move || {
        let stream = bridge::connect(port, "right-token").unwrap();
        let mut w = stream.try_clone().unwrap();
        let mut r = BufReader::new(stream);
        send_line(
            &mut w,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "clientInfo": {"name": "test-client"}}}),
        );
        let init = read_json(&mut r);
        send_line(
            &mut w,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "scene.create_object", "arguments": {"name": "Remote Cube", "kind": "cube"}}}),
        );
        let created = read_json(&mut r);
        (init, created)
    });
    let mut done = false;
    let until = Instant::now() + Duration::from_secs(30);
    while !done {
        assert!(Instant::now() < until, "timed out");
        b.pump(&mut s, &mut NoGpu);
        done = client.is_finished();
        std::thread::sleep(Duration::from_millis(2));
    }
    let (init, created) = client.join().unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "misty-engine");
    assert_eq!(created["result"]["isError"], false);
    assert!(names(&s).contains(&"Remote Cube".to_owned()));
    assert_eq!(s.undo_step().as_deref(), Some("Create Cube"));
    assert!(!names(&s).contains(&"Remote Cube".to_owned()));
    b.set_server(None);
    cleanup(s);
}

#[test]
fn bridge_asks_before_destructive_tools() {
    let mut s = project("bridge_confirm");
    std::fs::create_dir_all(s.project.root.join("Assets/Scripts")).unwrap();
    let script = s.project.root.join("Assets/Scripts/Keep.lua");
    std::fs::write(&script, "-- mine").unwrap();
    let server = Server::listen("t".into(), || {}).unwrap();
    let port = server.port;
    let mut b = Bridge::default();
    b.set_server(Some(server));
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let client = std::thread::spawn(move || {
        let stream = bridge::connect(port, "t").unwrap();
        let mut w = stream.try_clone().unwrap();
        let mut r = BufReader::new(stream);
        for (id, name, args) in [
            (
                1,
                "asset.delete",
                json!({"path": "Assets/Scripts/Keep.lua"}),
            ),
            (
                2,
                "asset.create_script",
                json!({"path": "Assets/Scripts/Keep.lua", "source": "-- ai", "overwrite": true}),
            ),
        ] {
            send_line(
                &mut w,
                json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}}),
            );
            tx.send(read_json(&mut r)).unwrap();
        }
    });
    // Deny the delete: the file stays and the client hears why.
    pump_until(&mut b, &mut s, |b| b.pending_confirmations() == 1);
    assert!(script.exists());
    b.answer(&mut s, &mut NoGpu, false);
    let denied = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(denied["result"]["isError"], true);
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("denied")
    );
    assert!(script.exists());
    // Allow the overwrite.
    pump_until(&mut b, &mut s, |b| b.pending_confirmations() == 1);
    assert_eq!(std::fs::read_to_string(&script).unwrap(), "-- mine");
    b.answer(&mut s, &mut NoGpu, true);
    let allowed = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(allowed["result"]["isError"], false, "{allowed}");
    assert_eq!(std::fs::read_to_string(&script).unwrap(), "-- ai");
    client.join().unwrap();
    // Turning the bridge off denies what is still waiting and stops listening.
    b.set_server(None);
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err()
            || bridge::connect(port, "t").is_err()
    );
    cleanup(s);
}

/// `Write` into a shared buffer (the proxy's stdout in tests).
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn connect_proxy_forwards_stdio() {
    let mut s = project("bridge_proxy");
    let server = Server::listen("proxy-token".into(), || {}).unwrap();
    let port = server.port;
    let mut b = Bridge::default();
    b.set_server(Some(server));
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"editor.get_state","arguments":{}}}"#,
    ]
    .join("\n")
        + "\n";
    let out = Shared::default();
    let out2 = out.clone();
    let proxy = std::thread::spawn(move || {
        let stream = bridge::connect(port, "proxy-token").unwrap();
        bridge::proxy(stream, std::io::Cursor::new(input.into_bytes()), out2)
    });
    let until = Instant::now() + Duration::from_secs(30);
    while !proxy.is_finished() {
        assert!(Instant::now() < until, "timed out");
        b.pump(&mut s, &mut NoGpu);
        std::thread::sleep(Duration::from_millis(2));
    }
    proxy.join().unwrap().unwrap();
    let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["result"]["structuredContent"]["live_editor"], true);
    b.set_server(None);
    cleanup(s);
}

#[test]
fn bridge_writes_and_removes_its_info_file() {
    let root = temp_root("bridge_info");
    let server = Server::start(&root, || {}).unwrap();
    let info = bridge::Info::read(&root).unwrap();
    assert_eq!(info.port, server.port);
    assert_eq!(info.token.len(), 64);
    assert_eq!(info.pid, std::process::id());
    assert!(bridge::connect(info.port, &info.token).is_ok());
    drop(server);
    assert!(!root.join(bridge::INFO_FILE).exists());
    assert!(
        bridge::Info::read(&root)
            .unwrap_err()
            .contains("Allow AI tools")
    );
    let _ = std::fs::remove_dir_all(root);
}

// ---- helpers -------------------------------------------------------------------------------------

#[test]
fn base64_and_secrets() {
    assert_eq!(super::base64(b""), "");
    assert_eq!(super::base64(b"f"), "Zg==");
    assert_eq!(super::base64(b"fo"), "Zm8=");
    assert_eq!(super::base64(b"foo"), "Zm9v");
    assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    assert!(super::same_secret("abc", "abc"));
    assert!(!super::same_secret("abc", "abd"));
    assert!(!super::same_secret("abc", "abcd"));
    let (a, b) = (
        super::random_token().unwrap(),
        super::random_token().unwrap(),
    );
    assert_ne!(a, b);
    assert!(tools::project_path("Assets/A.lua").is_ok());
    for bad in ["/etc/passwd", "C:/x", "Assets/../x", "", "Assets//x"] {
        assert!(tools::project_path(bad).is_err(), "{bad}");
    }
}

#[test]
fn headless_screenshot_renders_a_png() {
    let Some((device, queue)) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut s = project("shot");
    let mut renderer = misty_engine::render::Renderer::new(&device);
    for view in [super::shot::View::Scene, super::shot::View::Game] {
        let (w, h, rgba) =
            super::shot::render_offscreen(&device, &queue, &mut renderer, &mut s, view, (64, 36))
                .unwrap();
        assert_eq!((w, h), (64, 36));
        assert_eq!(rgba.len(), 64 * 36 * 4);
        let png = super::png(w, h, &rgba).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let r = ToolResult::image(&png, "x".into()).to_json();
        assert_eq!(r["content"][0]["mimeType"], "image/png");
    }
    // No camera: the Game view says so.
    let cam = tools::resolve(&s, "Main Camera").unwrap();
    s.scene.despawn(cam);
    let e = super::shot::render_offscreen(
        &device,
        &queue,
        &mut renderer,
        &mut s,
        super::shot::View::Game,
        (32, 32),
    )
    .unwrap_err();
    assert!(e.contains("Camera"), "{e}");
    cleanup(s);
}
