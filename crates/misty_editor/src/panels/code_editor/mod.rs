//! Code Editor window (Window ▸ General ▸ Code Editor): a built-in Lua editor in the style of the
//! familiar IDEs, so scripts never need an external tool.
//!
//! The text model and commands are pure and unit-tested (`text`, `editor`, `commands`, `find`,
//! `lexer`, `completion`, `diagnostics`); `view` draws one file and turns input into commands,
//! `ui` draws the tab (file tabs, find bar, Find in Files, dialogs).

mod api;
mod commands;
mod completion;
mod diagnostics;
mod editor;
mod find;
mod history;
mod input;
mod keys;
mod lexer;
pub mod prefs;
mod text;
mod ui;
mod view;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use crate::actions::Action;
use crate::layout::Tab;
use crate::state::EditorState;

pub use ui::{global_shortcuts, show, show_windows};

use editor::TextEditor;
use lexer::{Structure, Token};
use prefs::{ScriptEditor, Settings};
use text::Pos;

pub const ICON: &str = "📝";

/// Debounce between the last edit and the syntax check.
const CHECK_DELAY: Duration = Duration::from_millis(400);
/// How often open files are compared with the disk.
const POLL_EVERY: Duration = Duration::from_millis(500);

/// Tokens and block structure of a document, rebuilt when its text changes.
pub struct Analysis {
    version: u64,
    pub tokens: Vec<Vec<Token>>,
    pub structure: Structure,
}

/// One open script.
pub struct Document {
    /// Project-relative path ("Assets/Scripts/Rotator.lua").
    pub path: String,
    pub editor: TextEditor,
    saved_version: u64,
    disk_mtime: Option<SystemTime>,
    /// Changed on disk while it has unsaved edits: ask before reloading.
    pub disk_conflict: bool,
    /// The file was deleted or moved outside the editor.
    pub missing: bool,
    pub diagnostic: Option<diagnostics::Diagnostic>,
    checked_version: Option<u64>,
    last_edit: Instant,
    analysis: Option<Rc<Analysis>>,
    pub view: view::ViewState,
}

impl Document {
    fn new(path: String, text: &str, mtime: Option<SystemTime>, tab_size: usize) -> Self {
        let mut editor = TextEditor::new(text);
        editor.tab_size = tab_size;
        Self {
            path,
            saved_version: editor.version,
            editor,
            disk_mtime: mtime,
            disk_conflict: false,
            missing: false,
            diagnostic: None,
            checked_version: None,
            last_edit: Instant::now(),
            analysis: None,
            view: Default::default(),
        }
    }

    pub fn dirty(&self) -> bool {
        self.editor.version != self.saved_version
    }

    /// File name ("Rotator.lua").
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Up-to-date tokens and structure.
    pub fn analysis(&mut self) -> Rc<Analysis> {
        let v = self.editor.version;
        if self.analysis.as_ref().is_none_or(|a| a.version != v) {
            let lines = self.editor.buf.lines();
            let tokens = lexer::tokenize(lines);
            let structure = lexer::structure(lines, &tokens);
            self.analysis = Some(Rc::new(Analysis {
                version: v,
                tokens,
                structure,
            }));
        }
        self.analysis.clone().expect("just built")
    }

    /// Fold ranges (first, last line) currently collapsed. Folds are kept as markers at column 0
    /// of their first line, which move with edits.
    pub fn folded_ranges(&mut self) -> Vec<(usize, usize)> {
        let folds: Vec<usize> = self.editor.markers.iter().map(|p| p.line).collect();
        let regions = self.analysis().structure.folds.clone();
        folds
            .into_iter()
            .filter_map(|l| regions.iter().find(|r| r.0 == l).copied())
            .collect()
    }

    /// Lines not hidden inside a collapsed block, in order.
    pub fn visible_lines(&mut self) -> Vec<usize> {
        let n = self.editor.buf.line_count();
        let mut hidden = vec![false; n];
        for (s, e) in self.folded_ranges() {
            for h in hidden.iter_mut().take(e + 1).skip(s + 1) {
                *h = true;
            }
        }
        (0..n).filter(|&l| !hidden[l]).collect()
    }

    /// Collapse or expand the block starting on `line`.
    pub fn toggle_fold(&mut self, line: usize) {
        if let Some(i) = self.editor.markers.iter().position(|p| p.line == line) {
            self.editor.markers.remove(i);
        } else if self.analysis().structure.folds.iter().any(|r| r.0 == line) {
            self.editor.markers.push(Pos::new(line, 0));
        }
    }

    /// Ctrl+M,O: collapse every outermost block (functions).
    pub fn collapse_all(&mut self) {
        let regions = self.analysis().structure.folds.clone();
        self.editor.markers = regions
            .iter()
            .filter(|r| !regions.iter().any(|o| o != *r && o.0 <= r.0 && r.1 <= o.1))
            .map(|r| Pos::new(r.0, 0))
            .collect();
    }

    /// Keep folds valid: drop ones that no longer start a block or that hide a caret.
    fn tidy_folds(&mut self) {
        if self.editor.markers.is_empty() {
            return;
        }
        let regions = self.analysis().structure.folds.clone();
        let carets: Vec<usize> = self.editor.sels.iter().map(|s| s.head.line).collect();
        let mut folds = std::mem::take(&mut self.editor.markers);
        folds.retain(|p| {
            regions
                .iter()
                .find(|r| r.0 == p.line)
                .is_some_and(|r| !carets.iter().any(|&c| c > r.0 && c <= r.1))
        });
        folds.sort();
        folds.dedup_by_key(|p| p.line);
        self.editor.markers = folds;
    }

    /// Re-check the syntax once typing pauses.
    fn check_syntax(&mut self, now: Instant) -> bool {
        let v = self.editor.version;
        if self.checked_version == Some(v) || now.duration_since(self.last_edit) < CHECK_DELAY {
            return self.checked_version != Some(v);
        }
        self.checked_version = Some(v);
        let source = self.editor.text();
        self.diagnostic = diagnostics::check(&source, &self.path, self.editor.buf.lines());
        false
    }
}

/// The Code Editor window state, kept while the tab is closed.
pub struct CodeEditor {
    root: PathBuf,
    pub docs: Vec<Document>,
    pub active: usize,
    pub settings: Settings,
    pub ui: ui::UiState,
    last_poll: Instant,
}

impl CodeEditor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            docs: Vec::new(),
            active: 0,
            settings: Settings::load(root),
            ui: Default::default(),
            last_poll: Instant::now(),
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.docs.iter().any(Document::dirty)
    }

    fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Open (or switch to) a script; `line` is 1-based.
    pub fn open(&mut self, rel: &str, line: Option<usize>) -> Result<(), String> {
        let i = match self.docs.iter().position(|d| d.path == rel) {
            Some(i) => i,
            None => {
                let abs = self.abs(rel);
                let text = std::fs::read_to_string(&abs).map_err(|e| e.to_string())?;
                let doc = Document::new(rel.to_owned(), &text, mtime(&abs), self.settings.tab_size);
                // New tabs open right of the current one, like the reference IDEs.
                let at = if self.docs.is_empty() {
                    0
                } else {
                    self.active + 1
                };
                self.docs.insert(at, doc);
                at
            }
        };
        self.active = i;
        if let Some(line) = line {
            let doc = &mut self.docs[i];
            doc.editor.go_to_line(line);
            doc.view.scroll_to_caret = true;
            doc.view.center = true;
        }
        self.ui.focus_text = true;
        Ok(())
    }

    /// Write a document to disk.
    pub fn save(&mut self, i: usize) -> Result<(), String> {
        let abs = self.abs(&self.docs[i].path);
        let doc = &mut self.docs[i];
        std::fs::write(&abs, doc.editor.text()).map_err(|e| e.to_string())?;
        doc.saved_version = doc.editor.version;
        doc.disk_mtime = mtime(&abs);
        doc.disk_conflict = false;
        doc.missing = false;
        Ok(())
    }

    /// Close without asking (the caller checked for unsaved changes).
    pub fn close(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        self.docs.remove(i);
        if self.active > i || self.active >= self.docs.len() {
            self.active = self.active.saturating_sub(1);
        }
    }

    /// Reload a document from disk (keeps the carets, undoable).
    pub fn reload(&mut self, i: usize) {
        let abs = self.abs(&self.docs[i].path);
        if let Ok(text) = std::fs::read_to_string(&abs) {
            let doc = &mut self.docs[i];
            doc.editor.reload(&text);
            doc.saved_version = doc.editor.version;
            doc.disk_mtime = mtime(&abs);
            doc.disk_conflict = false;
        }
    }

    /// Follow a renamed or moved asset (`old` may be a folder).
    pub fn rename(&mut self, old: &str, new: &str) {
        for d in &mut self.docs {
            if let Some(p) = crate::asset_ops::remap(&d.path, old, new) {
                d.path = p;
                d.disk_mtime = mtime(&self.root.join(&d.path));
                d.missing = false;
            }
        }
    }

    /// Compare open files with the disk: clean ones reload, dirty ones ask.
    pub fn poll_disk(&mut self, now: Instant) {
        if now.duration_since(self.last_poll) < POLL_EVERY {
            return;
        }
        self.last_poll = now;
        for i in 0..self.docs.len() {
            let abs = self.abs(&self.docs[i].path);
            let m = mtime(&abs);
            let doc = &mut self.docs[i];
            doc.missing = m.is_none();
            if m.is_none() || m == doc.disk_mtime {
                continue;
            }
            if doc.dirty() {
                doc.disk_conflict = true;
                doc.disk_mtime = m;
            } else {
                self.reload(i);
            }
        }
    }

    /// Per-frame upkeep for every open document; true while a syntax check is pending.
    fn tick(&mut self, now: Instant) -> bool {
        self.poll_disk(now);
        let mut pending = false;
        for d in &mut self.docs {
            if d.editor.version != d.view.seen_version {
                d.last_edit = now;
                d.view.seen_version = d.editor.version;
            }
            pending |= d.check_syntax(now);
            d.tidy_folds();
        }
        pending
    }

    /// Apply new preferences: tab size to open files, then save.
    pub fn settings_changed(&mut self) {
        for d in &mut self.docs {
            d.editor.tab_size = self.settings.tab_size;
        }
        let _ = self.settings.save(&self.root);
    }

    /// Quit guard: true (and the save prompt opens) when scripts have unsaved changes.
    pub fn cancel_close(&mut self) -> bool {
        if std::mem::take(&mut self.ui.quit_confirmed) || !self.any_dirty() {
            return false;
        }
        let dirty = self
            .docs
            .iter()
            .filter(|d| d.dirty())
            .map(|d| d.path.clone())
            .collect();
        self.ui.prompt = Some(ui::Prompt::Close {
            docs: dirty,
            quit: true,
        });
        true
    }
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Open a script for editing: in the Code Editor (at `line`, 1-based), or the OS default app
/// when Preferences ▸ External Tools says so.
pub fn open_script(state: &mut EditorState, rel: &str, line: Option<usize>) {
    if state.code_editor.settings.script_editor == ScriptEditor::External {
        let abs = state.assets.absolute(rel);
        if let Err(err) = crate::assets::open_external(&abs) {
            state.console.warn(format!("Could not open {rel}: {err}"));
        }
        return;
    }
    match state.code_editor.open(rel, line) {
        Ok(()) => state.push(Action::FocusTab(Tab::CodeEditor)),
        Err(e) => state.console.warn(format!("Could not open {rel}: {e}")),
    }
}

/// Open the script named in a Console message at its line; false when it names none.
pub fn open_console_location(state: &mut EditorState, message: &str) -> bool {
    match diagnostics::error_location(message) {
        Some((rel, line)) => {
            // Always the built-in editor: the external one can't jump to a line.
            match state.code_editor.open(&rel, Some(line)) {
                Ok(()) => state.push(Action::FocusTab(Tab::CodeEditor)),
                Err(e) => state.console.warn(format!("Could not open {rel}: {e}")),
            }
            true
        }
        None => false,
    }
}

/// `file.lua:12` link text for a Console message that names a script line.
pub fn console_link(message: &str) -> Option<String> {
    let (rel, line) = diagnostics::error_location(message)?;
    let name = rel.rsplit('/').next().unwrap_or(&rel).to_owned();
    Some(format!("{name}:{line}"))
}
