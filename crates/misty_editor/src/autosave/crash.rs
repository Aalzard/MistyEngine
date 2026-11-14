//! Panics of the editor's UI thread: a crash log and an emergency backup, then a clean exit.
//!
//! The panic hook ([`install_hook`]) runs first, while the stack is still there: it writes
//! `Library/Logs/crash-<time>.txt` with the panic message, the backtrace, the versions and the
//! GPU adapter, and marks the session crashed. The editor frame runs inside `catch_unwind`
//! (`launcher`); when it unwinds, [`after_panic`] writes an emergency backup of every document
//! with unsaved changes (straight to the disk, not through the saver), adds the recent Console
//! lines to the log, and the process exits. The next start offers the backups and the log
//! folder. Nothing is ever sent anywhere.

use std::any::Any;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use crate::state::EditorState;

use super::session::{self, Session};

/// Crash logs, under the project.
pub const LOG_DIR: &str = "Library/Logs";
/// Console lines a crash log keeps.
pub const CONSOLE_LINES: usize = 60;

/// The open project, for a crash log written from the panic hook.
#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub session: Option<Session>,
}

static CONTEXT: Mutex<Option<Context>> = Mutex::new(None);
static GPU: Mutex<Option<String>> = Mutex::new(None);
/// What the hook wrote for the panic being handled: the message and the log file.
static LAST: Mutex<Option<(String, Option<PathBuf>)>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_context(c: Context) {
    *lock(&CONTEXT) = Some(c);
}

pub fn clear_context() {
    *lock(&CONTEXT) = None;
}

/// The GPU adapter (name and backend) for crash logs.
pub fn set_gpu(text: String) {
    *lock(&GPU) = Some(text);
}

/// The panic's message ("text" at file:line).
pub fn panic_message(payload: &(dyn Any + Send), location: Option<String>) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned());
    match location {
        Some(l) => format!("{text} (at {l})"),
        None => text,
    }
}

/// The first part of a crash log: what happened and on what.
pub fn report_head(message: &str, thread: &str, backtrace: &str) -> String {
    let mut t = String::new();
    let now = session::now_ms();
    let _ = writeln!(t, "Misty Engine crash report");
    let _ = writeln!(t, "=========================");
    let _ = writeln!(t, "Time: {}", session::local_date_time(now));
    let _ = writeln!(t, "Editor version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        t,
        "System: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let gpu = lock(&GPU).clone().unwrap_or_else(|| "(not started)".into());
    let _ = writeln!(t, "GPU adapter: {gpu}");
    if let Some(c) = lock(&CONTEXT).as_ref() {
        let _ = writeln!(t, "Project: {}", c.root.display());
    }
    let _ = writeln!(t, "Thread: {thread}");
    let _ = writeln!(t);
    let _ = writeln!(t, "Panic: {message}");
    let _ = writeln!(t);
    let _ = writeln!(t, "Backtrace:");
    let _ = writeln!(t, "{backtrace}");
    let _ = writeln!(
        t,
        "This file stays on this computer: Misty Engine never sends it anywhere."
    );
    t
}

/// Write a crash log into the project's `Library/Logs` and return its path.
pub fn write_log(root: &Path, text: &str) -> std::io::Result<PathBuf> {
    let dir = root.join(LOG_DIR);
    std::fs::create_dir_all(&dir)?;
    let stamp = session::local_stamp(session::now_ms());
    let mut path = dir.join(format!("crash-{stamp}.txt"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("crash-{stamp}-{n}.txt"));
        n += 1;
    }
    misty_engine::asset_db::write_atomic(&path, text)?;
    Ok(path)
}

/// Install the panic hook (`main`): the previous hook prints the panic as before; on the UI
/// thread the crash log is written and the saves handed to the writer thread just before reach
/// the disk (a few seconds at most).
pub fn install_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if std::thread::current().name() != Some("main") {
            return;
        }
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()));
        let message = panic_message(info.payload(), location);
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let context = lock(&CONTEXT).clone();
        let mut log = None;
        if let Some(c) = context {
            let text = report_head(&message, "main (UI)", &backtrace);
            match write_log(&c.root, &text) {
                Ok(path) => {
                    eprintln!("crash log: {}", path.display());
                    log = Some(path);
                }
                Err(e) => eprintln!("crash log not written: {e}"),
            }
            if let Some(st) = &c.session {
                let rel = log
                    .as_deref()
                    .map(|p| crate::scene_files::relative(&c.root, p));
                let _ = session::mark_crashed(&c.root, st, rel);
            }
        }
        *lock(&LAST) = Some((message, log));
        misty_engine::asset_db::saver::flush_within(Duration::from_secs(5));
    }));
}

/// What [`after_panic`] did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Backup files written.
    pub backups: Vec<PathBuf>,
    /// Documents that couldn't be backed up (their snapshot panicked too).
    pub failed: Vec<String>,
    pub log: Option<PathBuf>,
}

/// After the UI thread panicked (`payload`) and unwound to the editor frame: an emergency
/// backup of every document with unsaved changes, the Console's last lines in the crash log.
/// The editor exits after this.
pub fn after_panic(s: &mut EditorState, payload: &(dyn Any + Send)) -> Outcome {
    let mut out = Outcome::default();
    let (message, hook_log) = lock(&LAST)
        .take()
        .unwrap_or_else(|| (panic_message(payload, None), None));
    // Each document on its own: one that can't be serialised doesn't stop the others.
    if s.autosave.session.is_some() {
        let docs =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::docs::dirty_docs(s)))
                .unwrap_or_default();
        for doc in docs {
            let snap = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::docs::snapshot(s, &doc)
            }));
            match snap {
                Ok(Some(mut snap)) => {
                    snap.emergency = true;
                    match super::write_backup(s, &snap, true) {
                        Some((_, path)) => out.backups.push(path),
                        None => out.failed.push(snap.name.clone()),
                    }
                }
                Ok(None) => {}
                Err(_) => out.failed.push(format!("{doc:?}")),
            }
        }
    }
    // The log: the hook's, with the Console lines and the backups added (or a new one).
    let mut tail = String::new();
    let _ = writeln!(tail);
    let _ = writeln!(tail, "Emergency backup:");
    if out.backups.is_empty() && out.failed.is_empty() {
        let _ = writeln!(tail, "  nothing had unsaved changes");
    }
    for b in &out.backups {
        let _ = writeln!(tail, "  wrote {}", b.display());
    }
    for f in &out.failed {
        let _ = writeln!(tail, "  could not back up {f}");
    }
    let _ = writeln!(tail);
    let _ = writeln!(tail, "Recent Console lines:");
    let lines: Vec<String> = s
        .console
        .since(0)
        .map(|(_, e)| format!("  [{:?}] {}", e.level, e.message))
        .collect();
    let skip = lines.len().saturating_sub(CONSOLE_LINES);
    for l in &lines[skip..] {
        let _ = writeln!(tail, "{l}");
    }
    let root = s.project.root.clone();
    out.log = match hook_log {
        Some(path) => {
            let mut text = std::fs::read_to_string(&path).unwrap_or_default();
            text.push_str(&tail);
            misty_engine::asset_db::write_atomic(&path, text)
                .ok()
                .map(|_| path)
        }
        None => {
            let backtrace = "(not captured: the panic hook was not installed)";
            let text = report_head(&message, "main (UI)", backtrace) + &tail;
            write_log(&root, &text).ok()
        }
    };
    if let Some(st) = s.autosave.session.clone() {
        let rel = out
            .log
            .as_deref()
            .map(|p| crate::scene_files::relative(&root, p));
        let _ = session::mark_crashed(&root, &st, rel);
    }
    misty_engine::asset_db::saver::flush_within(Duration::from_secs(5));
    out
}
