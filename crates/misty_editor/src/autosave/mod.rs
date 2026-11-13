//! Autosave and crash recovery.
//!
//! **Autosave.** Every few minutes (Edit ▸ Preferences ▸ Autosave: on by default, every 5
//! minutes) the editor writes a backup of each open document with unsaved changes: the open
//! scenes, the prefab open in Prefab Mode (and the scenes or prefabs it was opened from), the
//! Script, Shader and Effect Graphs and the scripts open in the Code Editor. Backups go to
//! `Library/Autosave/<session>/<document>/<slot>.autosave`, never over the real files. The main
//! thread only takes the snapshot (serialises the document, see [`docs`]); the writes go
//! through the background saver (`asset_db::saver`), each one atomic (temp file, flush to the
//! device, rename). Each document keeps its last [`KEEP`] backups in rotating slots. The status
//! bar says "Autosaved 14:32".
//!
//! Animator controllers, Timelines, Animation clips and terrain edits need no backup: they are
//! written to their files right after each edit (terrain at the end of each brush stroke).
//!
//! **Crash recovery.** `Library/session.lock` (see [`session`]) says whether the last session
//! of the project ended cleanly. A clean exit deletes its own backups (the user answered the
//! save prompts), so backups left in `Library/Autosave` belong to a session that crashed, lost
//! power, or whose recovery was put off. On the next open, the **Recover Unsaved Work** dialog
//! (see [`recovery`]) lists the documents whose backup is newer than their file; restoring opens
//! the backup as a modified document, which the user saves (the file is never overwritten).
//!
//! **Panics.** A panic of the UI thread writes a crash log (`Library/Logs/crash-<time>.txt`)
//! and an emergency backup of every document with unsaved changes, then the editor exits (see
//! [`crash`]). Nothing is ever sent anywhere.

pub mod crash;
pub mod docs;
pub mod recovery;
pub mod session;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::i18n::tr;
use crate::state::EditorState;
use docs::{DocRef, DocSnapshot};

/// Backups kept per document (rotating slots).
pub const KEEP: usize = 3;
/// Folder of the backups, under the project.
pub const AUTOSAVE_DIR: &str = "Library/Autosave";
/// File name extension of a backup.
pub const BACKUP_EXT: &str = "autosave";
/// Main-thread time one frame spends on snapshots before the rest waits for the next frame.
pub const FRAME_BUDGET: Duration = Duration::from_millis(6);
/// A snapshot that took longer than this marks the documents as big: the next autosave waits
/// until the editor is idle for [`IDLE_FOR`] (at most one more interval).
pub const BIG_SNAPSHOT: Duration = Duration::from_millis(20);
/// How long without input counts as idle (for big snapshots).
pub const IDLE_FOR: Duration = Duration::from_secs(2);
/// A due autosave waits while a mouse button is down or a text field has focus, at most this
/// long.
pub const MAX_WAIT_FOR_INPUT: Duration = Duration::from_secs(60);

// ---- preferences ----------------------------------------------------------------------------

/// Edit ▸ Preferences ▸ Autosave (per user, `autosave.json` next to `hub.json`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub enabled: bool,
    /// Minutes between autosaves.
    pub interval_minutes: u32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 5,
        }
    }
}

/// The intervals Preferences offers (minutes).
pub const INTERVALS: [u32; 7] = [1, 2, 5, 10, 15, 30, 60];

impl Prefs {
    pub fn interval(&self) -> Duration {
        Duration::from_secs(60 * u64::from(self.interval_minutes.clamp(1, 24 * 60)))
    }

    fn file() -> Option<PathBuf> {
        Some(crate::hub::config::config_dir()?.join("autosave.json"))
    }

    /// The saved preferences (defaults when there are none).
    pub fn load() -> Self {
        Self::file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::file().ok_or("no config folder")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        misty_engine::asset_db::write_atomic(&path, text + "\n").map_err(|e| e.to_string())
    }
}

// ---- timing ---------------------------------------------------------------------------------

/// When the next autosave is due.
#[derive(Clone, Debug)]
pub struct Timer {
    /// The last autosave (or when the session started).
    pub last: Instant,
    /// When the autosave became due while the user was busy (it waits for them).
    pub due_since: Option<Instant>,
    /// Run at the next chance, whatever the interval (after a restore).
    pub now: bool,
}

impl Timer {
    pub fn new(now: Instant) -> Self {
        Self {
            last: now,
            due_since: None,
            now: false,
        }
    }

    /// Whether to start an autosave at `now`. `busy`: a mouse button is down or a text field
    /// has focus; `idle_for`: time since the last input; `big`: the last snapshot was slow.
    pub fn should_start(
        &mut self,
        prefs: &Prefs,
        now: Instant,
        busy: bool,
        idle_for: Duration,
        big: bool,
    ) -> bool {
        if !prefs.enabled && !self.now {
            self.due_since = None;
            return false;
        }
        let interval = prefs.interval();
        if !self.now && now.saturating_duration_since(self.last) < interval {
            return false;
        }
        let since = *self.due_since.get_or_insert(now);
        let waited = now.saturating_duration_since(since);
        // A slow snapshot waits for a pause in the work (at most one more interval); a quick
        // one only for the mouse button or the text field (at most a minute).
        let ready = if big {
            (!busy && idle_for >= IDLE_FOR) || waited >= interval
        } else {
            !busy || waited >= MAX_WAIT_FOR_INPUT
        };
        if ready {
            self.due_since = None;
            self.now = false;
        }
        ready
    }

    /// An autosave finished (or found nothing to do) at `now`.
    pub fn done(&mut self, now: Instant) {
        self.last = now;
    }
}

// ---- state ----------------------------------------------------------------------------------

/// The last autosave, for the status bar.
#[derive(Clone, Debug, Default)]
pub struct LastRun {
    /// Wall-clock time (Unix ms) of the last autosave that wrote something.
    pub saved_at_ms: Option<u64>,
    /// Documents it wrote.
    pub documents: usize,
    /// Main-thread time of the last autosave's snapshots, in total and the slowest one.
    pub cost: Duration,
    pub slowest: Duration,
    /// Bytes handed to the saver.
    pub bytes: usize,
}

/// An autosave in progress: the documents still to snapshot (a few per frame).
struct Run {
    /// What the editor looked like when it started (Prefab Mode levels, Play mode): a change
    /// stops the run, and the next frame starts again.
    shape: (usize, bool),
    todo: Vec<DocRef>,
    /// Scene copies being serialised on worker threads.
    workers: Vec<std::sync::mpsc::Receiver<DocSnapshot>>,
    written: usize,
    bytes: usize,
    cost: Duration,
    slowest: Duration,
}

impl Run {
    fn write(&mut self, s: &mut EditorState, snap: &DocSnapshot) {
        if let Some((bytes, _)) = write_backup(s, snap, false) {
            self.written += 1;
            self.bytes += bytes;
        }
    }
}

/// Autosave and recovery state of the open project.
#[derive(Default)]
pub struct Autosave {
    /// Autosave runs for this project (off: no project, a screenshot run, tests).
    pub active: bool,
    pub prefs: Prefs,
    pub timer: Option<Timer>,
    /// This session (`<start ms>-<pid>`) and its backup folder.
    pub session: Option<session::Session>,
    pub last: LastRun,
    /// The last snapshot was slow: wait for the user to pause.
    pub big: bool,
    run: Option<Run>,
    /// Hash of what each document's last backup holds (unchanged documents aren't written
    /// again) and the slot it went to.
    backed_up: HashMap<String, (u64, usize)>,
    /// Document folders made this session.
    made_dirs: HashSet<PathBuf>,
    /// The Recover Unsaved Work dialog (or the crash notice) waiting to be answered.
    pub recovery: Option<recovery::RecoveryDialog>,
    /// Backup folders of earlier sessions whose documents were restored: deleted on a clean
    /// exit (after this session backed the restored documents up itself).
    pub restored_dirs: Vec<PathBuf>,
    /// Last input seen (for the idle wait).
    last_input: Option<Instant>,
}

impl Autosave {
    /// The backup folder of this session.
    pub fn session_dir(&self) -> Option<&Path> {
        self.session.as_ref().map(|s| s.dir.as_path())
    }
}

/// Start autosave and the session marker for the project `s` has open, and look for work left
/// by an earlier session (the Recover Unsaved Work dialog shows it).
pub fn start(s: &mut EditorState) {
    let started = session::start(&s.project.root);
    begin(s, Prefs::load(), started);
}

/// [`start`] with the preferences and the session given (tests simulate sessions).
pub fn begin(s: &mut EditorState, prefs: Prefs, started: Result<session::Started, String>) {
    let root = s.project.root.clone();
    let a = &mut s.autosave;
    a.prefs = prefs;
    a.timer = Some(Timer::new(Instant::now()));
    match started {
        Ok(st) => {
            if st.other_editor {
                s.console.warn(
                    "Another Misty Engine editor has this project open: autosave and crash \
                     recovery are off in this one.",
                );
                return;
            }
            let previous = st.previous.clone();
            let dialog = recovery::RecoveryDialog::open(&root, &st);
            a.session = Some(st.session);
            a.active = true;
            a.recovery = dialog;
            crash::set_context(crash::Context {
                root: root.clone(),
                session: a.session.clone(),
            });
            if let Some(p) = previous
                && !p.clean_exit
            {
                s.console.warn(
                    "Misty Engine did not close properly last time (a crash or a power cut).",
                );
            }
        }
        Err(e) => s.console.warn(format!(
            "Autosave is off: could not write the session marker: {e}"
        )),
    }
}

/// Once a frame (after the panels): the recovery dialog, and the autosave when it is due.
pub fn frame(ctx: &egui::Context, s: &mut EditorState) {
    if s.autosave.recovery.is_some() {
        recovery::dialog_ui(ctx, s);
    }
    if !s.autosave.active {
        return;
    }
    let now = Instant::now();
    let (busy, had_input) = ctx.input(|i| {
        (
            i.pointer.any_down(),
            !i.events.is_empty() || i.pointer.is_moving(),
        )
    });
    let busy = busy || ctx.egui_wants_keyboard_input();
    if had_input || s.autosave.last_input.is_none() {
        s.autosave.last_input = Some(now);
    }
    let idle_for = s
        .autosave
        .last_input
        .map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
    tick(s, now, busy, idle_for);
    // Wake up for the next check while the editor sits idle.
    if s.autosave.run.is_some() {
        ctx.request_repaint();
    } else if s.autosave.prefs.enabled
        && let Some(t) = &s.autosave.timer
    {
        // When the next one is due (then every few seconds while it waits for the user).
        let left = s
            .autosave
            .prefs
            .interval()
            .saturating_sub(now.saturating_duration_since(t.last));
        ctx.request_repaint_after(left.max(Duration::from_secs(2)));
    }
}

/// The autosave step of one frame (`frame` without the window; tests drive it directly).
pub fn tick(s: &mut EditorState, now: Instant, busy: bool, idle_for: Duration) {
    if !s.autosave.active {
        return;
    }
    if s.autosave.run.is_none() {
        let prefs = s.autosave.prefs;
        let big = s.autosave.big;
        let Some(timer) = s.autosave.timer.as_mut() else {
            return;
        };
        if !timer.should_start(&prefs, now, busy, idle_for, big) {
            return;
        }
        let todo = docs::dirty_docs(s);
        if todo.is_empty() {
            if let Some(t) = s.autosave.timer.as_mut() {
                t.done(now);
            }
            return;
        }
        s.autosave.run = Some(Run {
            shape: docs::shape(s),
            todo,
            workers: Vec::new(),
            written: 0,
            bytes: 0,
            cost: Duration::ZERO,
            slowest: Duration::ZERO,
        });
    }
    step(s, now);
}

/// Snapshot documents of the run in progress until this frame's budget is used up.
fn step(s: &mut EditorState, now: Instant) {
    let Some(mut run) = s.autosave.run.take() else {
        return;
    };
    if run.shape != docs::shape(s) {
        // Prefab Mode or Play mode changed under the run: start again next frame.
        if let Some(t) = s.autosave.timer.as_mut() {
            t.now = true;
        }
        return;
    }
    let frame_start = Instant::now();
    while let Some(doc) = run.todo.pop() {
        let t0 = Instant::now();
        let started = docs::start_snapshot(s, &doc);
        let took = t0.elapsed();
        run.cost += took;
        run.slowest = run.slowest.max(took);
        match started {
            docs::Started::Done(Some(snap)) => run.write(s, &snap),
            docs::Started::Done(None) => {}
            docs::Started::Worker(rx) => run.workers.push(rx),
        }
        if frame_start.elapsed() >= FRAME_BUDGET {
            break;
        }
    }
    // Scenes the workers finished (one that panicked is dropped).
    let mut waiting = Vec::new();
    for rx in std::mem::take(&mut run.workers) {
        match rx.try_recv() {
            Ok(snap) => run.write(s, &snap),
            Err(std::sync::mpsc::TryRecvError::Empty) => waiting.push(rx),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
        }
    }
    run.workers = waiting;
    if !run.todo.is_empty() || !run.workers.is_empty() {
        s.autosave.run = Some(run);
        return;
    }
    let a = &mut s.autosave;
    a.big = run.slowest >= BIG_SNAPSHOT;
    a.last.cost = run.cost;
    a.last.slowest = run.slowest;
    if run.written > 0 {
        a.last.saved_at_ms = Some(session::now_ms());
        a.last.documents = run.written;
        a.last.bytes = run.bytes;
    }
    if let Some(t) = a.timer.as_mut() {
        t.done(now);
    }
}

/// Hand one snapshot to the saver (or write it now: `direct`, the panic path). Returns the size
/// and the backup file, or `None` when the document hasn't changed since its last backup or the write
/// failed.
pub fn write_backup(
    s: &mut EditorState,
    snap: &DocSnapshot,
    direct: bool,
) -> Option<(usize, PathBuf)> {
    let a = &mut s.autosave;
    let dir = a.session.as_ref()?.dir.join(snap.folder_name());
    let hash = snap.content_hash();
    let slot = match a.backed_up.get(&snap.key) {
        Some((h, _)) if *h == hash && !direct => return None,
        Some((_, slot)) => (slot + 1) % KEEP,
        None => 0,
    };
    if !a.made_dirs.contains(&dir) {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            s.console
                .warn(format!("Autosave: could not make {}: {e}", dir.display()));
            return None;
        }
        a.made_dirs.insert(dir.clone());
    }
    let path = dir.join(format!("{slot}.{BACKUP_EXT}"));
    let bytes = snap.file_bytes(session::now_ms());
    let len = bytes.len();
    let result = if direct {
        misty_engine::asset_db::write_atomic(&path, bytes)
    } else {
        misty_engine::asset_db::saver::save_later(&path, bytes)
    };
    match result {
        Ok(()) => {
            a.backed_up.insert(snap.key.clone(), (hash, slot));
            Some((len, path))
        }
        Err(e) => {
            s.console
                .warn(format!("Autosave: could not write {}: {e}", path.display()));
            None
        }
    }
}

/// Autosave now (the next frames), whatever the interval.
pub fn request_now(s: &mut EditorState) {
    if let Some(t) = s.autosave.timer.as_mut() {
        t.now = true;
    }
}

/// The editor closes normally: the session's backups are no longer needed (the save prompts
/// were answered), nor those of earlier sessions it restored; the marker says "clean exit".
pub fn clean_exit(s: &mut EditorState) {
    if !s.autosave.active {
        return;
    }
    s.autosave.active = false;
    s.autosave.run = None;
    // Backups still on their way to the disk land first, then go.
    misty_engine::asset_db::saver::flush();
    if let Some(dir) = s.autosave.session_dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
    for dir in std::mem::take(&mut s.autosave.restored_dirs) {
        recovery::remove_doc_dir(&dir);
    }
    if let Some(st) = s.autosave.session.take()
        && let Err(e) = session::mark_clean(&s.project.root, &st)
    {
        eprintln!("session marker not updated: {e}");
    }
    crash::clear_context();
}

// ---- UI -------------------------------------------------------------------------------------

/// Status bar: "Autosaved 14:32" (weak), with the details on hover.
pub fn status_item(ui: &mut egui::Ui, s: &EditorState) {
    let a = &s.autosave;
    if !a.active || !a.prefs.enabled {
        return;
    }
    let Some(at) = a.last.saved_at_ms else {
        return;
    };
    let clock = session::local_clock(at);
    let text = tr!("Autosaved {time}", time = clock);
    let hover = tr!(
        "{count} documents with unsaved changes were backed up to Library/Autosave ({ms} ms). Your files are only changed when you save.",
        count = a.last.documents,
        ms = format!("{:.1}", a.last.cost.as_secs_f64() * 1000.0)
    );
    ui.label(egui::RichText::new(text).weak().small())
        .on_hover_text(hover);
}

/// Edit ▸ Preferences ▸ Autosave: on/off and the interval.
pub fn preferences_ui(ui: &mut egui::Ui, a: &mut Autosave, console: &mut crate::console::Console) {
    ui.strong(tr!("Autosave"));
    let before = a.prefs;
    ui.checkbox(&mut a.prefs.enabled, tr!("Autosave unsaved work"))
        .on_hover_text(tr!(
            "Backs up open scenes, prefabs, graphs and scripts with unsaved changes to Library/Autosave. Your files are only changed when you save."
        ));
    ui.add_enabled_ui(a.prefs.enabled, |ui| {
        ui.horizontal(|ui| {
            let label = ui.label(tr!("Every"));
            let minutes = |m: u32| tr!("{count} min", count = m);
            egui::ComboBox::from_id_salt("pref_autosave_interval")
                .selected_text(minutes(a.prefs.interval_minutes))
                .show_ui(ui, |ui| {
                    for m in INTERVALS {
                        ui.selectable_value(&mut a.prefs.interval_minutes, m, minutes(m));
                    }
                })
                .response
                .labelled_by(label.id);
        });
    });
    if a.prefs != before {
        if let Err(e) = a.prefs.save() {
            console.warn(format!("Autosave setting not saved: {e}"));
        }
        // A new interval counts from now.
        if let Some(t) = a.timer.as_mut() {
            t.last = Instant::now();
        }
    }
}
