//! `misty --mcp [project]`: the project open without a window, the tools served over stdio.
//!
//! stdout carries only protocol messages: anything else the engine prints goes to stderr
//! ([`protocol_stdout`]). Requests are handled one at a time; jobs (tests, builds) block until
//! they finish, and destructive tools run without a question in the editor (there is none):
//! the MCP client asks the user before calling a tool marked destructive.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::protocol::{self, Session, Step};
use super::shot::HeadlessGpu;
use super::tools::{Cx, Outcome, ToolResult};
use crate::project::ProjectBrowser;
use crate::state::EditorState;

pub const USAGE: &str = "misty --mcp [<project folder>]";

/// The editor state of a project opened without a window.
pub struct Headless {
    pub state: EditorState,
    pub gpu: HeadlessGpu,
}

impl Headless {
    /// Open `root` as the editor does: asset GUIDs, project settings, the last open scenes.
    pub fn open(root: &Path) -> Result<Self, String> {
        if !root.join("Assets").is_dir() {
            return Err(format!(
                "{} is not a Misty project (no Assets folder)",
                root.display()
            ));
        }
        let root = std::path::absolute(root).map_err(|e| e.to_string())?;
        let guids = misty_engine::asset_db::open(&root);
        let mut state = EditorState::new(ProjectBrowser::open(root));
        state.assets.note_guids(guids);
        crate::scene_files::open_initial(&mut state);
        crate::scene_files::restore_scene_view(&mut state);
        let mut h = Self {
            state,
            gpu: HeadlessGpu::default(),
        };
        h.tick();
        Ok(h)
    }

    /// What the editor does between frames that tools rely on: new files get meta files,
    /// script changes reload, editor scripts load (their menu items), asset messages log.
    pub fn tick(&mut self) {
        let s = &mut self.state;
        for msg in std::mem::take(&mut s.assets.messages) {
            s.console.info(msg);
        }
        if s.assets.poll_guids() {
            s.refresh_assets();
        }
        for err in s.assets.poll_scripts() {
            s.console.log(crate::console::LogLevel::Error, err);
        }
        crate::editor_scripts::frame(s);
        crate::scene_files::refresh_dirty(s);
    }

    /// Handle one message; the response line, if any.
    pub fn handle(&mut self, session: &mut Session, line: &str) -> Option<Value> {
        self.tick();
        let mut cx = Cx {
            s: &mut self.state,
            live: false,
            shots: &mut self.gpu,
        };
        let reply = match protocol::handle(session, line, &mut cx) {
            Step::Reply(v) => Some(v),
            Step::Silent => None,
            Step::Call { id, outcome } => {
                let r = finish(outcome, &mut cx);
                Some(protocol::result(id, r.to_json()))
            }
        };
        self.tick();
        reply
    }
}

/// Run a tool's remaining steps to the end: confirmations are taken as given, jobs polled.
pub fn finish(mut outcome: Outcome, cx: &mut Cx<'_>) -> ToolResult {
    loop {
        outcome = match outcome {
            Outcome::Done(r) => return r,
            Outcome::Confirm { then, .. } => then(cx),
            Outcome::Wait(mut job) => loop {
                if let Some(r) = job.poll(cx) {
                    break Outcome::Done(r);
                }
                std::thread::sleep(Duration::from_millis(5));
            },
        };
    }
}

/// Serve messages from `input` until it ends.
pub fn serve(h: &mut Headless, input: impl BufRead, mut out: impl Write) -> std::io::Result<()> {
    let mut session = Session::default();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = h.handle(&mut session, &line) {
            let mut text = serde_json::to_string(&reply).unwrap_or_default();
            text.push('\n');
            out.write_all(text.as_bytes())?;
            out.flush()?;
        }
    }
    Ok(())
}

/// `misty --mcp [project]`; returns the exit code.
pub fn main(args: &[String]) -> i32 {
    let root = match args {
        [] => std::env::current_dir().unwrap_or_default(),
        [p] => PathBuf::from(p),
        _ => {
            eprintln!("usage: {USAGE}");
            return 2;
        }
    };
    let out = match protocol_stdout() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("misty --mcp: {e}");
            return 2;
        }
    };
    let mut h = match Headless::open(&root) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("misty --mcp: {e}");
            return 2;
        }
    };
    eprintln!(
        "Misty Engine MCP server: {} (stdio)",
        h.state.project.root.display()
    );
    let stdin = std::io::stdin();
    let result = serve(&mut h, stdin.lock(), out);
    crate::editor_scripts::on_exit(&mut h.state);
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("misty --mcp: {e}");
            1
        }
    }
}

/// The process's real stdout for protocol messages, with the standard output handle pointed
/// at stderr so stray prints from anywhere in the engine can't corrupt the stream.
#[cfg(windows)]
fn protocol_stdout() -> Result<std::fs::File, String> {
    use std::os::windows::io::{FromRawHandle, RawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> RawHandle;
        fn SetStdHandle(which: u32, handle: RawHandle) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    // SAFETY: plain Win32 calls; the stdout handle is taken over by the returned File (nothing
    // else uses it once the standard handle points at stderr, which std re-reads per write).
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out as isize == -1 {
            return Err("no standard output to talk on (start it from an MCP client)".into());
        }
        let err = GetStdHandle(STD_ERROR_HANDLE);
        SetStdHandle(STD_OUTPUT_HANDLE, err);
        Ok(std::fs::File::from_raw_handle(out))
    }
}

#[cfg(unix)]
fn protocol_stdout() -> Result<std::fs::File, String> {
    use std::os::unix::io::FromRawFd;
    unsafe extern "C" {
        fn dup(fd: i32) -> i32;
        fn dup2(from: i32, to: i32) -> i32;
    }
    // SAFETY: libc calls on the standard descriptors; the duplicate of fd 1 is owned by the
    // returned File, and fd 1 then refers to stderr.
    unsafe {
        let saved = dup(1);
        if saved < 0 {
            return Err("no standard output to talk on".into());
        }
        dup2(2, 1);
        Ok(std::fs::File::from_raw_fd(saved))
    }
}

#[cfg(not(any(windows, unix)))]
fn protocol_stdout() -> Result<std::io::Stdout, String> {
    Ok(std::io::stdout())
}
