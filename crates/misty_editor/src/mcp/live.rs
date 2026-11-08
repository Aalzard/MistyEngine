//! The tools in the running editor: Edit ▸ Preferences ▸ AI Tools ▸ "Allow AI tools (MCP)"
//! (off by default, a per-user setting) turns the loopback bridge on ([`super::bridge`]).
//!
//! Messages are handled on the main thread once per frame ([`frame`]), so tools see and change
//! the same state as the panels and their edits land on the normal undo stack. Destructive tools
//! wait for a toast, "AI wants to …  Allow / Deny"; unanswered requests are denied after
//! [`CONFIRM_TIMEOUT`].

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::bridge::{Event, Server};
use super::protocol::{self, Session, Step};
use super::shot::Shooter;
use super::tools::{Cx, Job, Outcome, Then, ToolResult};
use crate::state::EditorState;

/// A destructive request nobody answered is denied after this long.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);

/// The per-user setting, in the editor's config folder.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    allow_ai_tools: bool,
}

fn settings_path() -> Option<PathBuf> {
    crate::hub::config::config_dir().map(|d| d.join("mcp.json"))
}

fn load_allowed() -> bool {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
        .is_some_and(|s| s.allow_ai_tools)
}

fn save_allowed(on: bool) -> Result<(), String> {
    let path = settings_path().ok_or("no config folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Settings { allow_ai_tools: on })
        .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// A tool call still running.
struct Waiting {
    reply: Sender<String>,
    id: Value,
    job: Box<dyn Job>,
}

/// A destructive tool call waiting for Allow / Deny.
struct Asking {
    reply: Sender<String>,
    id: Value,
    what: String,
    client: String,
    then: Then,
    since: Instant,
}

/// Bridge state kept in the editor state.
#[derive(Default)]
pub struct Bridge {
    /// The preference (read on the first frame).
    pub allowed: bool,
    loaded: bool,
    server: Option<Server>,
    /// Why the listener could not start (shown in Preferences until the setting changes).
    pub error: Option<String>,
    sessions: HashMap<u64, Session>,
    waiting: Vec<Waiting>,
    asking: VecDeque<Asking>,
}

impl Bridge {
    /// Listening (the port), if on.
    pub fn port(&self) -> Option<u16> {
        self.server.as_ref().map(|s| s.port)
    }

    pub fn clients(&self) -> usize {
        self.sessions.len()
    }

    /// Requests waiting for the user.
    #[cfg(test)]
    pub fn pending_confirmations(&self) -> usize {
        self.asking.len()
    }

    /// Start listening for `project` (tests pass a server made with [`Server::listen`]).
    pub fn set_server(&mut self, server: Option<Server>) {
        if server.is_none() {
            self.deny_all("AI tools were turned off in the editor");
            self.sessions.clear();
        }
        self.server = server;
    }

    fn deny_all(&mut self, why: &str) {
        for a in self.asking.drain(..) {
            send(&a.reply, a.id, ToolResult::error(why));
        }
        for w in self.waiting.drain(..) {
            send(&w.reply, w.id, ToolResult::error(why));
        }
    }

    /// Handle what arrived and advance running jobs. True while anything is in flight.
    pub fn pump(&mut self, s: &mut EditorState, shots: &mut dyn Shooter) -> bool {
        let mut events = Vec::new();
        if let Some(server) = &self.server {
            while let Ok(e) = server.events.try_recv() {
                events.push(e);
            }
        }
        for e in events {
            match e {
                Event::Connected(conn) => {
                    self.sessions.insert(conn, Session::default());
                }
                Event::Closed(conn) => {
                    if let Some(session) = self.sessions.remove(&conn) {
                        let who = session.client.unwrap_or_else(|| "An AI client".into());
                        s.live.toast(format!("{who} disconnected from the editor."));
                    }
                }
                Event::Message { conn, line, reply } => {
                    let session = self.sessions.entry(conn).or_default();
                    let was_initialized = session.initialized;
                    let mut cx = Cx {
                        s: &mut *s,
                        live: true,
                        shots: &mut *shots,
                    };
                    let step = protocol::handle(session, &line, &mut cx);
                    if !was_initialized && session.initialized {
                        let who = session
                            .client
                            .clone()
                            .unwrap_or_else(|| "An AI client".into());
                        s.live
                            .toast(format!("{who} connected to the editor (AI tools)."));
                    }
                    let client = session.client.clone().unwrap_or_else(|| "AI".into());
                    match step {
                        Step::Reply(v) => {
                            let _ = reply.send(v.to_string());
                        }
                        Step::Silent => {}
                        Step::Call { id, outcome } => {
                            self.continue_call(reply, id, outcome, client)
                        }
                    }
                }
            }
        }
        // Jobs: one poll per frame.
        let mut still = Vec::new();
        for mut w in std::mem::take(&mut self.waiting) {
            let mut cx = Cx {
                s: &mut *s,
                live: true,
                shots: &mut *shots,
            };
            match w.job.poll(&mut cx) {
                Some(r) => send(&w.reply, w.id, r),
                None => still.push(w),
            }
        }
        still.append(&mut self.waiting);
        self.waiting = still;
        // Unanswered questions time out.
        while self
            .asking
            .front()
            .is_some_and(|a| a.since.elapsed() > CONFIRM_TIMEOUT)
        {
            if let Some(a) = self.asking.pop_front() {
                send(
                    &a.reply,
                    a.id,
                    ToolResult::error(format!("nobody answered in the editor: did not {}", a.what)),
                );
            }
        }
        !self.waiting.is_empty() || !self.asking.is_empty()
    }

    fn continue_call(
        &mut self,
        reply: Sender<String>,
        id: Value,
        outcome: Outcome,
        client: String,
    ) {
        match outcome {
            Outcome::Done(r) => send(&reply, id, r),
            Outcome::Wait(job) => self.waiting.push(Waiting { reply, id, job }),
            Outcome::Confirm { what, then } => {
                self.asking.push_back(Asking {
                    reply,
                    id,
                    what,
                    client,
                    then,
                    since: Instant::now(),
                });
            }
        }
    }

    /// The user's answer to the oldest question.
    pub fn answer(&mut self, s: &mut EditorState, shots: &mut dyn Shooter, allow: bool) {
        let Some(a) = self.asking.pop_front() else {
            return;
        };
        if !allow {
            s.console
                .info(format!("Denied an AI request to {}.", a.what));
            send(
                &a.reply,
                a.id,
                ToolResult::error(format!("the user denied this in the editor: {}", a.what)),
            );
            return;
        }
        s.console
            .info(format!("Allowed an AI request to {}.", a.what));
        let outcome = {
            let mut cx = Cx {
                s: &mut *s,
                live: true,
                shots: &mut *shots,
            };
            (a.then)(&mut cx)
        };
        self.continue_call(a.reply, a.id, outcome, a.client);
    }
}

fn send(reply: &Sender<String>, id: Value, r: ToolResult) {
    let _ = reply.send(protocol::result(id, r.to_json()).to_string());
}

/// Once per editor frame, before queued actions are applied.
pub fn frame(ctx: &egui::Context, s: &mut EditorState, gpu: Option<&mut crate::viewport::Gpu>) {
    let mut b = std::mem::take(&mut s.mcp);
    if !b.loaded {
        b.loaded = true;
        b.allowed = load_allowed();
    }
    match (b.allowed, b.server.is_some()) {
        (true, false) if b.error.is_none() => {
            let wake = ctx.clone();
            match Server::start(&s.project.root, move || wake.request_repaint()) {
                Ok(server) => {
                    s.console.info(format!(
                        "AI tools (MCP) listening on 127.0.0.1:{} for this project.",
                        server.port
                    ));
                    b.set_server(Some(server));
                }
                Err(e) => {
                    s.console
                        .warn(format!("AI tools (MCP) could not start: {e}"));
                    b.error = Some(e);
                }
            }
        }
        (false, true) => {
            b.set_server(None);
            s.console.info("AI tools (MCP) turned off.");
        }
        _ => {}
    }
    let mut shots = super::shot::LiveShots(gpu);
    let busy = b.pump(s, &mut shots);
    if let Some(allow) = confirm_ui(ctx, &b) {
        b.answer(s, &mut shots, allow);
    }
    if busy {
        ctx.request_repaint_after(Duration::from_millis(30));
    }
    s.mcp = b;
}

/// The editor is closing: stop listening (removes `Library/MistyMcp.json`).
pub fn shutdown(s: &mut EditorState) {
    s.mcp.set_server(None);
}

/// "AI wants to …" with Allow / Deny, bottom right, over everything. The answer, if clicked.
fn confirm_ui(ctx: &egui::Context, b: &Bridge) -> Option<bool> {
    let a = b.asking.front()?;
    let mut answer = None;
    egui::Area::new(egui::Id::new("misty_mcp_confirm"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.label(crate::theme::bold(
                    ui.ctx(),
                    format!("{} wants to {}", a.client, a.what),
                ));
                if b.asking.len() > 1 {
                    ui.weak(format!("{} more requests waiting", b.asking.len() - 1));
                }
                let left = CONFIRM_TIMEOUT.saturating_sub(a.since.elapsed()).as_secs();
                ui.weak(format!("Denied automatically in {left} s"));
                ui.horizontal(|ui| {
                    if ui.button("Allow").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Deny").clicked() {
                        answer = Some(false);
                    }
                });
            });
        });
    // The countdown.
    ctx.request_repaint_after(Duration::from_millis(500));
    answer
}

/// The AI Tools section of Edit ▸ Preferences.
pub fn preferences_ui(ui: &mut egui::Ui, b: &mut Bridge, project: &Path) {
    ui.strong("AI Tools");
    let r = ui
        .checkbox(&mut b.allowed, "Allow AI tools (MCP)")
        .on_hover_text(
            "Lets AI agents on this computer (Claude, Codex…) drive the editor through a local, token-protected connection. Deleting, overwriting and building ask you first.",
        );
    if r.changed() {
        b.error = None;
        if let Err(e) = save_allowed(b.allowed) {
            b.error = Some(format!("setting not saved: {e}"));
        }
    }
    match (b.port(), &b.error) {
        (_, Some(e)) => {
            ui.colored_label(egui::Color32::from_rgb(255, 120, 100), e);
        }
        (Some(port), None) => {
            ui.weak(format!(
                "Listening on 127.0.0.1:{port}, {} client(s) connected.",
                b.clients()
            ));
            ui.weak("Client command:");
            let cmd = format!("misty --mcp-connect \"{}\"", project.display());
            ui.horizontal(|ui| {
                ui.monospace(&cmd);
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(cmd.clone());
                }
            });
        }
        (None, None) => {
            ui.weak("Off: no AI client can reach this editor.");
        }
    }
}
