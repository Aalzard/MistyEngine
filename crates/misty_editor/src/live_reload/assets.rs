//! Assets the editor keeps in memory, re-read when their files change on disk: materials,
//! prefabs (their instances in open scenes refresh), terrain data (heights, paint, grass and
//! detail settings), timelines, animation clips, Animator Controllers and Audio Mixers.
//!
//! Textures, models, shaders, Shader Graphs and effect graphs are cached by the engine and
//! reloaded by `Renderer::reload_changed_assets`; scripts by `AssetDatabase::poll_scripts`.
//! The editor's own saves rewrite files with what it already holds, so they compare equal and
//! count as no change.

use std::collections::HashMap;
use std::rc::Rc;

use misty_engine::scene::prefab::Prefab;

use super::{Settle, Stamp, stamp};
use crate::state::EditorState;

#[derive(Default)]
pub struct AssetWatch {
    /// Stamp of each watched file when last looked at.
    known: HashMap<String, Option<Stamp>>,
    settle: Settle,
    /// Each prefab used by an open scene as it was last read: instances that matched it had
    /// no overrides, so they take the new version.
    prefabs: HashMap<String, Rc<Prefab>>,
}

/// Kinds of editor-held assets.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Material,
    Prefab,
    Clip,
    Controller,
    Mixer,
    Timeline,
}

/// Files the editor currently holds in memory.
fn watched(s: &EditorState) -> Vec<(Kind, String)> {
    let mut out: Vec<(Kind, String)> = s
        .assets
        .materials
        .keys()
        .map(|k| (Kind::Material, k.clone()))
        .collect();
    let mut prefabs: Vec<String> = s
        .scene
        .walk()
        .into_iter()
        .filter_map(|(e, _)| s.scene.prefab_link(e))
        .collect();
    prefabs.sort();
    prefabs.dedup();
    out.extend(prefabs.into_iter().map(|p| (Kind::Prefab, p)));
    let clips = &s.anim_clips;
    out.extend(clips.loaded_paths().into_iter().map(|p| (Kind::Clip, p)));
    let controllers = clips.controllers.loaded_paths();
    out.extend(controllers.into_iter().map(|p| (Kind::Controller, p)));
    let mixers = clips.mixers.loaded_paths();
    out.extend(mixers.into_iter().map(|p| (Kind::Mixer, p)));
    let timelines = clips.timelines.loaded_paths();
    out.extend(timelines.into_iter().map(|p| (Kind::Timeline, p)));
    out
}

/// Check the editor-held assets (Edit mode only). Returns the paths reloaded.
pub fn poll(s: &mut EditorState) -> Vec<String> {
    if s.play.in_play_mode() {
        return Vec::new();
    }
    let mut reloaded = Vec::new();
    for (kind, rel) in watched(s) {
        let abs = s.project.root.join(&rel);
        let w = &mut s.live.assets;
        let Some(seen) = w.known.get(&rel).copied() else {
            let now = stamp(&abs);
            w.known.insert(rel.clone(), now);
            if kind == Kind::Prefab
                && let Ok(p) = s.assets.prefab(&rel)
            {
                s.live.assets.prefabs.insert(rel, p);
            }
            continue;
        };
        let Some(now) = super::changed_stamp(&abs, seen) else {
            continue;
        };
        if now.is_none() {
            continue;
        }
        let settle = s.live.settle;
        if !w.settle.ready(&rel, now, settle) {
            continue;
        }
        w.known.insert(rel.clone(), now);
        match reload(s, kind, &rel) {
            Ok(true) => reloaded.push(rel),
            Ok(false) => {}
            Err(e) => s.console.warn(format!("Could not reload {rel}: {e}")),
        }
    }
    // Terrain data lives in the engine's store (shared with the renderer and physics).
    let root = s.project.root.clone();
    for path in misty_engine::terrain::store::poll_changes() {
        reloaded.push(crate::scene_files::relative(&root, &path));
    }
    reloaded
}

/// Re-read one asset. Ok(true) when what the editor holds changed.
fn reload(s: &mut EditorState, kind: Kind, rel: &str) -> Result<bool, String> {
    let clips = &mut s.anim_clips;
    match kind {
        Kind::Material => s.assets.reload_material(rel),
        Kind::Clip => clips.reload_from_disk(rel),
        Kind::Controller => clips.controllers.reload_from_disk(rel),
        Kind::Mixer => clips.mixers.reload_from_disk(rel),
        Kind::Timeline => clips.timelines.reload_from_disk(rel),
        Kind::Prefab => {
            let new = s.assets.prefab(rel)?;
            let old = s.live.assets.prefabs.insert(rel.to_owned(), new.clone());
            Ok(match old {
                Some(old) if *old != *new => refresh_instances(s, rel, &old, &new) > 0,
                _ => false,
            })
        }
    }
}

/// The editor just wrote the prefab at `rel` (Apply, Prefab Mode, an update from a prefab it
/// uses): remember it as read, so the write isn't taken for an outside change.
pub fn note_prefab(s: &mut EditorState, rel: &str) {
    let now = stamp(&s.project.root.join(rel));
    let w = &mut s.live.assets;
    w.known.insert(rel.to_owned(), now);
    w.settle.forget(rel);
    if let Ok(p) = s.assets.prefab(rel) {
        s.live.assets.prefabs.insert(rel.to_owned(), p);
    }
}

/// The prefab at `rel` changed on disk from `old` to `new`: instances follow it in place and
/// keep their overrides, and prefabs that use it are updated (see `prefabs::propagate`). One
/// undo step. Returns how many instances changed.
pub fn refresh_instances(s: &mut EditorState, rel: &str, old: &Prefab, new: &Prefab) -> usize {
    let before = s.scene.clone();
    let changed = crate::prefabs::propagate(s, rel, old, new);
    if changed > 0 {
        s.undo
            .push("Refresh Prefab Instances", &before, &s.selection);
        s.scene_dirty = true;
        let scene = &s.scene;
        s.inspector_targets.retain(|e| scene.contains(*e));
        s.hierarchy.expanded.retain(|e| scene.contains(*e));
    }
    changed
}
