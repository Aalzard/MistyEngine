//! Recover Unsaved Work: the backups an earlier session left behind, and the dialog that offers
//! them back.
//!
//! A clean exit deletes its session's backups, so any session folder left in
//! `Library/Autosave` (other than a running editor's) belongs to a session that crashed, lost
//! power, or whose recovery was put off ("Decide later"). For each document the newest backup
//! is offered when it is newer than the document's file (or the file is gone, or the scene was
//! never saved); older backups are stale (the file was saved after them) and are deleted.
//!
//! Restoring opens the backup as a *modified* document: the scene, prefab, graph or script
//! shows its changes and the user saves it. The file itself is never written here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::i18n::tr;
use crate::state::EditorState;

use super::docs::{self, DocKind, Header};
use super::session::{self, Started};

/// One document that can be recovered.
#[derive(Clone, Debug)]
pub struct Leftover {
    pub header: Header,
    /// The newest backup file.
    pub backup: PathBuf,
    /// When that backup was taken (Unix ms).
    pub backup_ms: u64,
    /// The document's file and when it was last written (`None`: missing or never saved).
    pub file_ms: Option<u64>,
    /// Every backup folder holding this document (several when sessions crashed in a row).
    pub dirs: Vec<PathBuf>,
    /// Ticked in the dialog.
    pub selected: bool,
    /// Why the last restore failed.
    pub error: Option<String>,
}

impl Leftover {
    /// Identity of the document (kind and file, or the name of a never-saved scene).
    pub fn key(&self) -> String {
        match &self.header.path {
            Some(p) => format!("{}:{p}", self.header.kind.id()),
            None => format!("{}:untitled:{}", self.header.kind.id(), self.header.name),
        }
    }
}

/// What a scan of `Library/Autosave` found.
#[derive(Debug, Default)]
pub struct Scan {
    /// Documents to offer, newest first.
    pub leftovers: Vec<Leftover>,
    /// Document folders holding only stale backups (deleted).
    pub stale: Vec<PathBuf>,
}

/// Read the backups in `Library/Autosave` of sessions other than `current` and other than a
/// running editor's.
pub fn scan(root: &Path, current: Option<&str>) -> Scan {
    let base = root.join(super::AUTOSAVE_DIR);
    let mut by_doc: BTreeMap<String, Leftover> = BTreeMap::new();
    let mut all_dirs: Vec<PathBuf> = Vec::new();
    let Ok(sessions) = std::fs::read_dir(&base) else {
        return Scan::default();
    };
    for session_dir in sessions.flatten() {
        let name = session_dir.file_name().to_string_lossy().into_owned();
        if Some(name.as_str()) == current || !session_dir.path().is_dir() {
            continue;
        }
        // A running editor's backups (another window on this project) are left alone.
        if let Some((started, pid)) = session::parse_session_id(&name)
            && session::session_alive(pid, started)
        {
            continue;
        }
        let Ok(doc_dirs) = std::fs::read_dir(session_dir.path()) else {
            continue;
        };
        for doc_dir in doc_dirs.flatten() {
            let dir = doc_dir.path();
            if !dir.is_dir() {
                continue;
            }
            all_dirs.push(dir.clone());
            let Some((header, backup)) = newest_backup(&dir) else {
                continue;
            };
            let file_ms = header
                .path
                .as_deref()
                .and_then(|p| session::mtime_ms(&root.join(p)));
            let item = Leftover {
                backup_ms: header.time_ms,
                header,
                backup,
                file_ms,
                dirs: vec![dir.clone()],
                selected: true,
                error: None,
            };
            let key = item.key();
            match by_doc.get_mut(&key) {
                Some(have) => {
                    have.dirs.push(dir);
                    if item.backup_ms > have.backup_ms {
                        let dirs = std::mem::take(&mut have.dirs);
                        *have = Leftover { dirs, ..item };
                    }
                }
                None => {
                    by_doc.insert(key, item);
                }
            }
        }
    }
    let mut out = Scan::default();
    for (_, item) in by_doc {
        // Saved after its newest backup: nothing to recover.
        if item.file_ms.is_some_and(|f| f > item.backup_ms) {
            out.stale.extend(item.dirs);
        } else {
            out.leftovers.push(item);
        }
    }
    // Folders with no readable backup at all (a write cut short before the first one landed).
    for d in all_dirs {
        if !out.stale.contains(&d) && !out.leftovers.iter().any(|l| l.dirs.contains(&d)) {
            out.stale.push(d);
        }
    }
    out.leftovers
        .sort_by(|a, b| b.backup_ms.cmp(&a.backup_ms).then(a.key().cmp(&b.key())));
    out
}

/// The newest readable backup in a document folder.
fn newest_backup(dir: &Path) -> Option<(Header, PathBuf)> {
    let mut best: Option<(Header, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != super::BACKUP_EXT) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some((header, _)) = docs::parse_backup(&bytes) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(b, _)| header.time_ms > b.time_ms)
        {
            best = Some((header, path));
        }
    }
    best
}

/// Delete a document's backup folder, and its session folder once that is empty.
pub fn remove_doc_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    if let Some(session) = dir.parent()
        && std::fs::read_dir(session).is_ok_and(|mut d| d.next().is_none())
    {
        let _ = std::fs::remove_dir(session);
    }
}

// ---- restoring ------------------------------------------------------------------------------

/// Open `item`'s backup as a modified document. The document's file is not touched.
pub fn restore(s: &mut EditorState, item: &Leftover) -> Result<(), String> {
    let bytes = std::fs::read(&item.backup).map_err(|e| e.to_string())?;
    let (header, body) = docs::parse_backup(&bytes).ok_or("the backup can't be read")?;
    if s.play.in_play_mode() {
        return Err(tr!("Stop Play mode first."));
    }
    let in_prefab_mode = crate::prefab_mode::active(s);
    match header.kind {
        DocKind::Scene if in_prefab_mode => Err(tr!("Leave Prefab Mode first.")),
        DocKind::Scene => restore_scene(s, &header, body),
        DocKind::Prefab => {
            let rel = header.path.as_deref().ok_or("no prefab path")?;
            let (prefab, _) = misty_engine::scene::prefab::Prefab::from_json(body)?;
            if crate::prefab_mode::open_restored(s, rel, prefab) {
                Ok(())
            } else {
                Err(tr!("The prefab could not be opened in Prefab Mode."))
            }
        }
        DocKind::ScriptGraph => {
            let rel = header.path.as_deref().ok_or("no path")?;
            let graph = misty_engine::visual_script::ScriptGraph::from_json(body)?;
            busy_window(
                s.script_graph.dirty(),
                s.script_graph.path.as_deref(),
                rel,
                "Script Graph",
            )?;
            let file = s.assets.absolute(rel);
            let w = &mut s.script_graph;
            w.open(&file, rel);
            w.edit("Restore Autosave", false, |g| *g = graph);
            s.push(crate::actions::Action::FocusTab(
                crate::layout::Tab::ScriptGraph,
            ));
            Ok(())
        }
        DocKind::ShaderGraph => {
            let rel = header.path.as_deref().ok_or("no path")?;
            let graph = misty_engine::shader_graph::ShaderGraph::from_json(body)?;
            busy_window(
                s.shader_graph.dirty(),
                s.shader_graph.path.as_deref(),
                rel,
                "Shader Graph",
            )?;
            let file = s.assets.absolute(rel);
            let w = &mut s.shader_graph;
            w.open(&file, rel);
            w.edit("Restore Autosave", false, |g| *g = graph);
            s.push(crate::actions::Action::FocusTab(
                crate::layout::Tab::ShaderGraph,
            ));
            Ok(())
        }
        DocKind::EffectGraph => {
            let rel = header.path.as_deref().ok_or("no path")?;
            let graph = misty_engine::vfx::EffectGraph::from_json(body)?;
            busy_window(
                s.effect_graph.dirty(),
                s.effect_graph.path.as_deref(),
                rel,
                "Effect Graph",
            )?;
            let file = s.assets.absolute(rel);
            let w = &mut s.effect_graph;
            w.open(&file, rel);
            w.edit("Restore Autosave", false, |g| *g = graph);
            s.push(crate::actions::Action::FocusTab(
                crate::layout::Tab::EffectGraph,
            ));
            Ok(())
        }
        DocKind::BehaviourTree => {
            let rel = header.path.as_deref().ok_or("no path")?;
            busy_window(
                s.behaviour_tree.dirty(),
                s.behaviour_tree.path.as_deref(),
                rel,
                "Behaviour Tree",
            )?;
            let tree = misty_engine::behaviour_tree::BehaviourTree::from_json(body)?;
            let file = s.assets.absolute(rel);
            let w = &mut s.behaviour_tree;
            w.open(&file, rel);
            w.edit("Restore Autosave", false, |t| *t = tree);
            s.push(crate::actions::Action::FocusTab(
                crate::layout::Tab::BehaviourTree,
            ));
            Ok(())
        }
        DocKind::Script => {
            let rel = header.path.as_deref().ok_or("no path")?;
            let ce = &mut s.code_editor;
            if ce.open(rel, None).is_err() {
                // The script's file is gone: open the backup as a new script there (unsaved).
                return Err(tr!(
                    "The script's file is missing: save the backup by hand."
                ));
            }
            let i = ce.active;
            let doc = &mut ce.docs[i];
            doc.editor.reload(body);
            s.push(crate::actions::Action::FocusTab(
                crate::layout::Tab::CodeEditor,
            ));
            Ok(())
        }
    }
}

/// A window that holds one document (the graph windows) already has another one with unsaved
/// changes, a restored one among them: opening the backup there would drop them. The backup
/// stays in the list until that one is saved.
fn busy_window(dirty: bool, open: Option<&str>, rel: &str, window: &str) -> Result<(), String> {
    match open {
        Some(other) if dirty && other != rel => Err(tr!(
            "The {window} window has unsaved changes to {other}: save them, then restore this one.",
            window = window,
            other = other
        )),
        _ => Ok(()),
    }
}

/// A scene backup: in place of the open scene with that file, or added to the open scenes.
/// The scene counts as changed (it differs from its file) until it is saved.
fn restore_scene(s: &mut EditorState, header: &Header, body: &str) -> Result<(), String> {
    use misty_engine::scene::Scene;
    let id = match header.path.as_deref() {
        Some(rel) => match s.scene.find_by_path(rel) {
            Some(id) if s.scene.entry(id).is_some_and(|e| e.loaded) => {
                let (_, report) = s.scene.reload_scene(id, body)?;
                warn_all(s, rel, report.warnings);
                id
            }
            _ => {
                let (id, _, report) = s.scene.load_additive(rel, body)?;
                warn_all(s, rel, report.warnings);
                crate::scene_files::remember(s, rel);
                id
            }
        },
        None => {
            // A scene never saved: in place of the open scenes when none of them has changes.
            if !crate::scene_files::dirty_now(s).is_empty() {
                return Err(tr!(
                    "Save or discard the open scene's changes first: an untitled scene replaces the open scenes."
                ));
            }
            let (scene, report) = Scene::from_json(&header.name, body)?;
            s.scene = scene;
            crate::scene_files::reset_after_load(s);
            warn_all(s, &header.name, report.warnings);
            s.scene.active_scene()
        }
    };
    // Instances follow their prefabs as when the file is opened.
    crate::prefabs::catch_up(s, Some(id));
    // Changed: what the file holds is something else (never matches a real hash).
    s.files.saved.insert(id, crate::scene_files::SavedHash::ready(0));
    s.scene_dirty = true;
    s.selection.retain_existing(&s.scene);
    let scene = &s.scene;
    s.inspector_targets.retain(|e| scene.contains(*e));
    s.undo.clear();
    s.files.dirty = crate::scene_files::dirty_now(s);
    crate::open_scenes::remember_open_set(s);
    Ok(())
}

fn warn_all(s: &mut EditorState, what: &str, warnings: Vec<String>) {
    for w in warnings {
        s.console.warn(format!("{what}: {w}"));
    }
}

// ---- the dialog -----------------------------------------------------------------------------

/// The Recover Unsaved Work dialog (and the crash notice).
#[derive(Debug, Default)]
pub struct RecoveryDialog {
    pub items: Vec<Leftover>,
    /// The last session ended with a crash or power loss (not only put off).
    pub unclean: bool,
    /// Crash logs the last session wrote (`Library/Logs/crash-*.txt`).
    pub crash_logs: Vec<PathBuf>,
    /// "Discard all backups?" is being asked.
    pub confirm_discard: bool,
}

impl RecoveryDialog {
    /// What to show after opening the project, if anything (stale backups are deleted).
    pub fn open(root: &Path, started: &Started) -> Option<Self> {
        let scan = scan(root, Some(&started.session.id));
        for d in &scan.stale {
            remove_doc_dir(d);
        }
        let previous = started.previous.as_ref();
        let unclean = previous.is_some_and(|m| !m.clean_exit);
        let crash_logs = match previous {
            Some(m) if !m.clean_exit => crash_logs_since(root, m.started_ms),
            _ => Vec::new(),
        };
        (!scan.leftovers.is_empty() || !crash_logs.is_empty()).then_some(Self {
            items: scan.leftovers,
            unclean,
            crash_logs,
            confirm_discard: false,
        })
    }
}

/// Crash logs written since `since_ms`.
pub fn crash_logs_since(root: &Path, since_ms: u64) -> Vec<PathBuf> {
    let dir = root.join(super::crash::LOG_DIR);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
                && session::mtime_ms(p).is_some_and(|t| t >= since_ms)
        })
        .collect();
    out.sort();
    out
}

/// Restore the ticked documents (all of them with `all`); those restored leave the list.
pub fn restore_items(s: &mut EditorState, all: bool) {
    let Some(mut dialog) = s.autosave.recovery.take() else {
        return;
    };
    dialog.items.sort_by_key(|i| i.header.kind.order());
    let mut kept = Vec::new();
    let mut restored = 0;
    for mut item in std::mem::take(&mut dialog.items) {
        if !all && !item.selected {
            kept.push(item);
            continue;
        }
        match restore(s, &item) {
            Ok(()) => {
                restored += 1;
                s.console.info(format!(
                    "Recovered {} from its backup of {} (not saved yet).",
                    item.header.path.as_deref().unwrap_or(&item.header.name),
                    session::local_date_time(item.backup_ms)
                ));
                // Deleted once this session has backed the document up itself (clean exit).
                s.autosave.restored_dirs.extend(item.dirs.iter().cloned());
            }
            Err(e) => {
                s.console.warn(format!(
                    "Could not recover {}: {e}",
                    item.header.path.as_deref().unwrap_or(&item.header.name)
                ));
                item.error = Some(e);
                kept.push(item);
            }
        }
    }
    if restored > 0 {
        // Back the restored documents up in this session right away.
        super::request_now(s);
    }
    dialog.items = kept;
    dialog.crash_logs.clear();
    if !dialog.items.is_empty() {
        s.autosave.recovery = Some(dialog);
    }
}

/// Delete the backups of every listed document.
pub fn discard_items(s: &mut EditorState) {
    let Some(dialog) = s.autosave.recovery.take() else {
        return;
    };
    for item in &dialog.items {
        for d in &item.dirs {
            remove_doc_dir(d);
        }
    }
    if !dialog.items.is_empty() {
        s.console.info(format!(
            "Discarded the backups of {} documents.",
            dialog.items.len()
        ));
    }
}

/// Close the dialog; the backups stay and are offered again at the next start.
pub fn decide_later(s: &mut EditorState) {
    s.autosave.recovery = None;
}

/// Open `Library/Logs` in the file manager.
pub fn open_log_folder(s: &mut EditorState) {
    let dir = s.project.root.join(super::crash::LOG_DIR);
    if let Err(e) = crate::assets::open_external(&dir) {
        s.console
            .warn(format!("Could not open {}: {e}", dir.display()));
    }
}

/// The dialog (modal while it is up).
pub fn dialog_ui(ctx: &egui::Context, s: &mut EditorState) {
    enum Do {
        RestoreAll,
        RestoreSelected,
        Discard,
        Later,
        OpenLogs,
    }
    let mut action = None;
    let Some(dialog) = s.autosave.recovery.as_mut() else {
        return;
    };
    egui::Modal::new(egui::Id::new("recover_unsaved_work")).show(ctx, |ui| {
        ui.set_width(560.0);
        if dialog.items.is_empty() {
            ui.heading(tr!("Misty Engine closed unexpectedly"));
            ui.add_space(4.0);
            ui.label(tr!(
                "A crash report was saved in the project's Library/Logs folder. It stays on this computer; nothing is sent anywhere."
            ));
            ui.label(egui::RichText::new(tr!("No unsaved work was waiting to be recovered.")).weak());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(tr!("Open crash log folder")).clicked() {
                    action = Some(Do::OpenLogs);
                }
                if ui.button(tr!("Close")).clicked() {
                    action = Some(Do::Later);
                }
            });
            return;
        }
        ui.heading(tr!("Recover Unsaved Work"));
        ui.add_space(4.0);
        ui.label(if dialog.unclean {
            tr!(
                "Misty Engine did not close properly last time. Backups of these documents are newer than their files:"
            )
        } else {
            tr!("Backups of these documents are newer than their files:")
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            egui::Grid::new("recover_list")
                .num_columns(4)
                .striped(true)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label("");
                    ui.strong(tr!("Document"));
                    ui.strong(tr!("Backup"));
                    ui.strong(tr!("File"));
                    ui.end_row();
                    for item in &mut dialog.items {
                        ui.checkbox(&mut item.selected, "");
                        let kind = crate::i18n::tr(item.header.kind.label());
                        let name = crate::i18n::display(&item.header.name).into_owned();
                        let r = ui.label(format!("{name}  ({kind})"));
                        if let Some(p) = &item.header.path {
                            r.on_hover_text(p.as_str());
                        }
                        let backup = session::local_date_time(item.backup_ms);
                        if item.header.emergency {
                            ui.label(format!("{backup}  ⚠")).on_hover_text(tr!(
                                "Written while the editor was crashing: check it before saving."
                            ));
                        } else {
                            ui.label(backup);
                        }
                        let file = match (&item.header.path, item.file_ms) {
                            (None, _) => tr!("never saved"),
                            (Some(_), None) => tr!("missing"),
                            (Some(_), Some(t)) => session::local_date_time(t),
                        };
                        ui.label(file);
                        ui.end_row();
                        if let Some(e) = &item.error {
                            ui.label("");
                            ui.colored_label(crate::theme::warn(), e.as_str());
                            ui.end_row();
                        }
                    }
                });
        });
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(tr!(
                "Restored documents open with unsaved changes: save them to keep them. Your files are not changed."
            ))
            .weak(),
        );
        ui.add_space(8.0);
        if dialog.confirm_discard {
            ui.label(
                egui::RichText::new(tr!("Delete these backups? This can't be undone."))
                    .strong(),
            );
            ui.horizontal(|ui| {
                if ui.button(tr!("Discard")).clicked() {
                    action = Some(Do::Discard);
                }
                if ui.button(tr!("Cancel")).clicked() {
                    dialog.confirm_discard = false;
                }
            });
            return;
        }
        ui.horizontal(|ui| {
            if ui.button(tr!("Restore All")).clicked() {
                action = Some(Do::RestoreAll);
            }
            let any = dialog.items.iter().any(|i| i.selected);
            if ui
                .add_enabled(any, egui::Button::new(tr!("Restore Selected")))
                .clicked()
            {
                action = Some(Do::RestoreSelected);
            }
            if ui
                .button(tr!("Discard"))
                .on_hover_text(tr!("Delete the backups of every listed document"))
                .clicked()
            {
                dialog.confirm_discard = true;
            }
            if ui
                .button(tr!("Decide Later"))
                .on_hover_text(tr!("Keep the backups and ask again next time"))
                .clicked()
            {
                action = Some(Do::Later);
            }
            if !dialog.crash_logs.is_empty() && ui.button(tr!("Open crash log folder")).clicked() {
                action = Some(Do::OpenLogs);
            }
        });
    });
    match action {
        Some(Do::RestoreAll) => restore_items(s, true),
        Some(Do::RestoreSelected) => restore_items(s, false),
        Some(Do::Discard) => discard_items(s),
        Some(Do::Later) => decide_later(s),
        Some(Do::OpenLogs) => open_log_folder(s),
        None => {}
    }
}
