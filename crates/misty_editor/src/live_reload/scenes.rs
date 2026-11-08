//! Open scenes follow their files: a scene file changed on disk reloads in place when the scene
//! has no unsaved changes, else a bar asks "Reload / Keep mine". Additive scenes too.
//!
//! The editor remembers a hash of each scene file as it last read or wrote it ([`note_disk`]),
//! so its own saves are never mistaken for outside changes, whatever the file times say.

use std::collections::HashMap;

use misty_engine::hecs::Entity;
use misty_engine::scene::SceneId;

use super::paths::{self, Step};
use super::{Settle, Stamp, stamp};
use crate::console::LogLevel;
use crate::scene_files;
use crate::state::EditorState;

#[derive(Default)]
pub struct SceneWatch {
    /// Each scene file as the editor last read or wrote it: its stamp and content hash.
    known: HashMap<String, (Option<Stamp>, u64)>,
    settle: Settle,
    /// Scenes whose file changed while they had unsaved changes (the bar asks about them).
    pub conflicts: Vec<String>,
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// The editor just read or wrote the scene file `rel`: what is on disk now is its own.
pub fn note_disk(s: &mut EditorState, rel: &str) {
    let abs = s.project.root.join(rel);
    let Ok(bytes) = std::fs::read(&abs) else {
        return;
    };
    let w = &mut s.live.scenes;
    w.known.insert(rel.to_owned(), (stamp(&abs), hash(&bytes)));
    w.settle.forget(rel);
    w.conflicts.retain(|c| c != rel);
}

/// Check the loaded scenes' files (Edit mode only). Returns the scenes reloaded.
pub fn poll(s: &mut EditorState) -> Vec<String> {
    if s.play.in_play_mode() {
        return Vec::new();
    }
    let open: Vec<(SceneId, String)> = s
        .scene
        .scenes()
        .iter()
        .filter(|e| e.loaded)
        .filter_map(|e| Some((e.id, e.path.clone()?)))
        .collect();
    let mut reloaded = Vec::new();
    for (id, rel) in open {
        let abs = s.project.root.join(&rel);
        let now = stamp(&abs);
        let Some(&(seen, known_hash)) = s.live.scenes.known.get(&rel) else {
            // Opened some other way (tests, older code paths): start watching from here.
            note_disk(s, &rel);
            continue;
        };
        if now.is_none() || now == seen {
            continue;
        }
        let settle = s.live.settle;
        if !s.live.scenes.settle.ready(&rel, now, settle) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&abs) else {
            continue;
        };
        let h = hash(&bytes);
        let w = &mut s.live.scenes;
        w.known.insert(rel.clone(), (now, h));
        if h == known_hash || w.conflicts.contains(&rel) {
            // Touched but unchanged, or already asking about it.
            continue;
        }
        if scene_files::dirty_now(s).contains(&id) {
            s.console.warn(format!(
                "{} changed on disk, but it has unsaved changes here.",
                file_name(&rel)
            ));
            s.live.scenes.conflicts.push(rel);
            continue;
        }
        match reload_in_place(s, id) {
            Ok(()) => reloaded.push(rel),
            Err(e) => s.console.log(
                LogLevel::Error,
                format!("Could not reload {}: {e}", file_name(&rel)),
            ),
        }
    }
    reloaded
}

pub fn file_name(rel: &str) -> &str {
    rel.rsplit(['/', '\\']).next().unwrap_or(rel)
}

/// Selection, Inspector targets and expanded Hierarchy rows inside one scene, by place.
struct Kept {
    selection: Vec<Vec<Step>>,
    active: Option<Vec<Step>>,
    expanded: Vec<Vec<Step>>,
    inspector: Vec<Vec<Step>>,
}

impl Kept {
    fn capture(s: &EditorState, id: SceneId) -> Self {
        let scene = &s.scene;
        let ours = |e: &Entity| scene.contains(*e) && scene.scene_of(*e) == id;
        let many = |list: &mut dyn Iterator<Item = &Entity>| -> Vec<Vec<Step>> {
            list.filter(|e| ours(e))
                .map(|e| paths::steps(scene, *e))
                .collect()
        };
        Self {
            selection: many(&mut s.selection.list().iter()),
            active: s
                .selection
                .active()
                .filter(|e| ours(e))
                .map(|e| paths::steps(scene, e)),
            expanded: many(&mut s.hierarchy.expanded.iter()),
            inspector: many(&mut s.inspector_targets.iter()),
        }
    }

    /// Put them back onto the reloaded scene's objects (other scenes' entries stay).
    fn restore(self, s: &mut EditorState, id: SceneId) {
        let find = |s: &EditorState, steps: &[Step]| paths::resolve(&s.scene, id, steps);
        let mut list: Vec<Entity> = s
            .selection
            .list()
            .iter()
            .copied()
            .filter(|e| s.scene.contains(*e))
            .collect();
        list.extend(self.selection.iter().filter_map(|p| find(s, p)));
        let active = self
            .active
            .as_deref()
            .and_then(|p| find(s, p))
            .or_else(|| s.selection.active().filter(|e| s.scene.contains(*e)));
        // `set_many` makes the last one active.
        if let Some(a) = active {
            list.retain(|e| *e != a);
            list.push(a);
        }
        s.selection.set_many(list);
        let expanded: Vec<Entity> = self.expanded.iter().filter_map(|p| find(s, p)).collect();
        let scene = &s.scene;
        s.hierarchy.expanded.retain(|e| scene.contains(*e));
        s.hierarchy.expanded.extend(expanded);
        let inspector: Vec<Entity> = self.inspector.iter().filter_map(|p| find(s, p)).collect();
        let scene = &s.scene;
        s.inspector_targets.retain(|e| scene.contains(*e));
        s.inspector_targets.extend(inspector);
    }
}

/// Read scene `id`'s file again and replace its objects, keeping the Scene view camera, the
/// selection and the expanded Hierarchy rows (matched by place). Unsaved changes in that scene
/// are lost; the caller checks first. Not undoable: the history is cleared, as on open.
pub fn reload_in_place(s: &mut EditorState, id: SceneId) -> Result<(), String> {
    if s.play.in_play_mode() {
        return Err("stop Play mode first".into());
    }
    let rel = s
        .scene
        .scene_path(id)
        .ok_or_else(|| "the scene was never saved".to_owned())?;
    let text = misty_engine::vfs::read_to_string(s.project.root.join(&rel))
        .map_err(|e| format!("{rel}: {e}"))?;
    // Previews pose objects; they would write the pose into the fresh copy.
    crate::panels::animation::preview::stop_preview(s);
    crate::panels::timeline::preview::stop_preview(s);
    let kept = Kept::capture(s, id);
    let (_, report) = s.scene.reload_scene(id, &text)?;
    kept.restore(s, id);
    s.hierarchy.renaming = None;
    s.undo.clear();
    scene_files::mark_saved(s, id);
    scene_files::recheck_dirty(s);
    crate::prefabs::catch_up(s, Some(id));
    for w in report.warnings {
        s.console.warn(format!("{rel}: {w}"));
    }
    note_disk(s, &rel);
    s.console
        .info(format!("Reloaded {} (changed on disk)", file_name(&rel)));
    s.live.toast(format!("Reloaded {}", file_name(&rel)));
    Ok(())
}

/// The bar's "Reload": throw away this scene's changes and load the file.
pub fn accept(s: &mut EditorState, rel: &str) {
    s.live.scenes.conflicts.retain(|c| c != rel);
    let Some(id) = s.scene.find_loaded(rel) else {
        return;
    };
    if let Err(e) = reload_in_place(s, id) {
        s.console.log(
            LogLevel::Error,
            format!("Could not reload {}: {e}", file_name(rel)),
        );
    }
}

/// The bar's "Keep mine": keep editing; the next save overwrites the file.
pub fn keep_mine(s: &mut EditorState, rel: &str) {
    s.live.scenes.conflicts.retain(|c| c != rel);
}

/// File ▸ Reload Scene (Ctrl+Shift+R), once unsaved changes are saved or discarded: every
/// loaded scene with a file reloads from disk, and assets are checked straight away.
pub fn reload_all(s: &mut EditorState) {
    let ids: Vec<SceneId> = s
        .scene
        .scenes()
        .iter()
        .filter(|e| e.loaded && e.path.is_some())
        .map(|e| e.id)
        .collect();
    if ids.is_empty() {
        s.console
            .warn("The scene has never been saved: nothing to reload.");
    }
    for id in ids {
        let name = s.scene.scene_name(id);
        if let Err(e) = reload_in_place(s, id) {
            s.console
                .log(LogLevel::Error, format!("Could not reload {name}: {e}"));
        }
    }
    s.live.scenes.conflicts.clear();
    s.refresh_assets();
    super::poll_now(s);
}

/// Background of the "changed on disk" bar.
pub const BAR_FILL: egui::Color32 = egui::Color32::from_rgb(92, 72, 24);

/// The non-modal "X.scene changed on disk — Reload / Keep mine" bar (one row per scene).
pub fn bar(ui: &mut egui::Ui, s: &mut EditorState) {
    let conflicts = s.live.scenes.conflicts.clone();
    for rel in conflicts {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("⚠  {} changed on disk", file_name(&rel)))
                    .color(egui::Color32::from_rgb(255, 222, 150))
                    .strong(),
            );
            ui.label(
                egui::RichText::new("— you have unsaved changes in it.")
                    .color(egui::Color32::from_rgb(230, 210, 170)),
            );
            if ui
                .button("Reload")
                .on_hover_text("Load the file from disk; your unsaved changes in it are lost")
                .clicked()
            {
                accept(s, &rel);
            }
            if ui
                .button("Keep mine")
                .on_hover_text("Keep your version; saving overwrites the file")
                .clicked()
            {
                keep_mine(s, &rel);
            }
        });
    }
}
