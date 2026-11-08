//! End to end: `misty --mcp <project>` as an MCP client starts it. Create an object over stdio,
//! save, reload, check; then a second server process finds the saved object in the file.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl Server {
    fn start(project: &Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_misty"))
            .arg("--mcp")
            .arg(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start misty --mcp");
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Server {
            child,
            stdin,
            lines,
            next_id: 1,
        }
    }

    fn send(&mut self, msg: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    }

    /// A request and its response (every stdout line must be one JSON-RPC message).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(120))
            .unwrap_or_else(|_| panic!("no answer to {method}"));
        let v: Value = serde_json::from_str(&line)
            .unwrap_or_else(|_| panic!("stdout line is not JSON-RPC: {line:?}"));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], id, "{v}");
        v
    }

    /// A tool call's structured result (it must not fail).
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let v = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let result = &v["result"];
        assert_eq!(result["isError"], false, "{name}: {v}");
        result["structuredContent"].clone()
    }

    fn initialize(&mut self) {
        let v = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "e2e", "version": "1" },
            }),
        );
        assert_eq!(v["result"]["serverInfo"]["name"], "misty-engine");
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    }

    /// Close stdin; the server exits cleanly.
    fn finish(mut self) {
        drop(self.stdin.take());
        for _ in 0..600 {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "misty --mcp exited with {status}");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        panic!("misty --mcp did not exit after stdin closed");
    }
}

fn temp_project() -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_mcp_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    root
}

#[test]
fn mcp_stdio_create_save_reload() {
    let project = temp_project();
    let mut server = Server::start(&project);
    server.initialize();
    let tools = server.request("tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().len() >= 25);

    let created = server.tool(
        "scene.create_object",
        json!({ "name": "E2E Cube", "kind": "cube", "position": [1, 2, 3] }),
    );
    assert_eq!(created["name"], "E2E Cube");
    let saved = server.tool("scene.save", json!({ "path": "Assets/Scenes/E2E.scene" }));
    assert_eq!(saved["saved"], json!(["Assets/Scenes/E2E.scene"]));
    assert!(project.join("Assets/Scenes/E2E.scene").is_file());
    // Reload from the file and look for it.
    server.tool("scene.open", json!({ "path": "Assets/Scenes/E2E.scene" }));
    let found = server.tool("scene.list_hierarchy", json!({ "filter": "E2E Cube" }));
    assert_eq!(found["count"], 1, "{found}");
    // An error comes back as a tool error, and the server keeps going.
    let bad = server.request(
        "tools/call",
        json!({ "name": "scene.get_object", "arguments": { "object": "Missing" } }),
    );
    assert_eq!(bad["result"]["isError"], true);
    server.finish();

    // A new server process opens the last scene and finds the saved object.
    let mut again = Server::start(&project);
    again.initialize();
    let obj = again.tool("scene.get_object", json!({ "object": "E2E Cube" }));
    let transform = obj["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Transform")
        .unwrap()
        .clone();
    assert_eq!(transform["fields"]["position"], json!([1.0, 2.0, 3.0]));
    again.finish();
    let _ = std::fs::remove_dir_all(project);
}
