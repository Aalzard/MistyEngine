//! Assets the editor keeps in memory, re-read when their files change on disk: materials,
//! prefabs (their instances in open scenes refresh), terrain data (heights, paint, grass and
//! detail settings), timelines, animation clips, Animator Controllers and Audio Mixers.
//!
//! Textures, models, shaders, Shader Graphs and effect graphs are cached by the engine and
//! reloaded by `Renderer::reload_changed_assets`; scripts by `AssetDatabase::poll_scripts`.
//! The editor's own saves rewrite files with what it already holds, so they compare equal and
//! count as no change.
//!
//! In Play mode only Animator Controllers and animation clips are checked (the ones the editor
//! holds and the ones the playing scene's Animators use): a change made outside the editor (git,
//! another tool) reaches the running Animators like an edit in the Animator window (see
//! `play::sync_controllers`). The other kinds wait for Edit mode.

use std::collections::{HashMap, HashSet};
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
    /// Controllers of playing Animators already held, with their clips (`poll_play`).
    held_controllers: HashSet<String>,
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
    let list = watched(s);
    let mut reloaded = check(s, list);
    // Terrain data lives in the engine's store (shared with the renderer and physics).
    let root = s.project.root.clone();
    misty_engine::profile_scope!("LiveReload.Terrains");
    for path in misty_engine::terrain::store::poll_changes() {
        reloaded.push(crate::scene_files::relative(&root, &path));
    }
    reloaded
}

/// Play mode: check the Animator Controllers and clips the editor holds, after holding the
/// ones the playing scene's Animators use. Changed ones reach the game through the stores'
/// generation counters. Returns the paths reloaded.
pub fn poll_play(s: &mut EditorState) -> Vec<String> {
    if !s.play.in_play_mode() {
        return Vec::new();
    }
    hold_animator_assets(s);
    let list = watched(s)
        .into_iter()
        .filter(|(k, _)| matches!(k, Kind::Clip | Kind::Controller))
        .collect();
    check(s, list)
}

/// Hold (so they are watched) the controllers of the playing scene's Animators and the clips
/// those controllers play. A controller is looked into once per editor session.
fn hold_animator_assets(s: &mut EditorState) {
    let mut paths: Vec<String> = s
        .scene
        .world
        .query::<&misty_engine::scene::Animator>()
        .iter()
        .filter_map(|a| a.controller.clone())
        .collect();
    paths.sort();
    paths.dedup();
    for path in paths {
        if !s.live.assets.held_controllers.insert(path.clone()) {
            continue;
        }
        let Ok(c) = s.anim_clips.controllers.get(&path) else {
            continue;
        };
        let clips: Vec<String> = c
            .clone()
            .asset_paths_mut()
            .into_iter()
            .filter(|p| misty_engine::animation::is_clip(p))
            .map(std::mem::take)
            .collect();
        for clip in clips {
            let _ = s.anim_clips.get(&clip);
        }
    }
}

/// Look at `list` and reload what changed on disk.
fn check(s: &mut EditorState, list: Vec<(Kind, String)>) -> Vec<String> {
    let mut reloaded = Vec::new();
    for (kind, rel) in list {
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
