//! Timeline assets open in the editor: loaded on first use, edited in memory and written to
//! their `.timeline` file right after every edit. Edits made during one mouse drag share a
//! single undo step.

use std::collections::BTreeMap;
use std::path::PathBuf;

use misty_engine::animation::AnimationClip;
use misty_engine::timeline::{ClipSource, TimelineAsset};

use crate::anim_clips::{ClipSnapshot, ClipStore};
use crate::state::EditorState;

#[derive(Default)]
pub struct TimelineStore {
    root: PathBuf,
    timelines: BTreeMap<String, TimelineAsset>,
    /// Undo label of the edit grouped with the current interaction.
    grouping: Option<String>,
}

impl TimelineStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Default::default()
        }
    }

    /// The timeline at a project-relative path, read from disk the first time.
    pub fn get(&mut self, path: &str) -> Result<&TimelineAsset, String> {
        if !self.timelines.contains_key(path) {
            let t = TimelineAsset::load(&self.root.join(path))?;
            self.timelines.insert(path.to_owned(), t);
        }
        Ok(&self.timelines[path])
    }

    pub fn snapshot(&mut self, path: &str) -> Option<ClipSnapshot> {
        self.get(path)
            .ok()
            .map(|t| ClipSnapshot::Timeline(path.to_owned(), t.clone()))
    }

    /// Put an undo snapshot back (and save it); returns the timeline it replaced.
    pub fn restore(&mut self, path: String, t: TimelineAsset) -> ClipSnapshot {
        let _ = t.save(&self.root.join(&path));
        let old = self.timelines.insert(path.clone(), t).unwrap_or_default();
        ClipSnapshot::Timeline(path, old)
    }

    /// Write a new timeline file and keep it open.
    pub fn create(&mut self, path: &str, t: TimelineAsset) -> Result<(), String> {
        t.save(&self.root.join(path))?;
        self.timelines.insert(path.to_owned(), t);
        Ok(())
    }

    /// Follow an asset rename/move of `old` (a timeline or a folder) to `new`; clip paths
    /// inside open timelines follow too.
    pub fn rename(&mut self, old: &str, new: &str) {
        let keys: Vec<String> = self.timelines.keys().cloned().collect();
        for key in keys {
            if let Some(to) = crate::asset_ops::remap(&key, old, new) {
                let t = self.timelines.remove(&key).expect("listed key");
                self.timelines.insert(to, t);
            }
        }
        for t in self.timelines.values_mut() {
            for c in t.tracks.iter_mut().flat_map(|tr| tr.clips.iter_mut()) {
                if let Some(a) = c.asset.as_mut()
                    && let Some(to) = crate::asset_ops::remap(a, old, new)
                {
                    *a = to;
                }
            }
        }
    }

    /// Forget the grouped edit once the mouse is up and no text field has focus.
    pub fn end_interaction(&mut self, interacting: bool) {
        if !interacting {
            self.grouping = None;
        }
    }
}

/// Change a timeline with undo and save it. Several edits during one interaction (a drag or
/// typing) become one undo step. Returns false if the timeline does not load.
pub fn edit(
    s: &mut EditorState,
    path: &str,
    label: &str,
    interacting: bool,
    f: impl FnOnce(&mut TimelineAsset),
) -> bool {
    let store = &mut s.anim_clips.timelines;
    let Some(snap) = store.snapshot(path) else {
        return false;
    };
    let grouped = interacting && store.grouping.as_deref() == Some(label);
    if !grouped {
        s.undo
            .push_with_clip(label, &s.scene, &s.selection, Some(snap));
        s.anim_clips.timelines.grouping = interacting.then(|| label.to_owned());
    }
    let store = &mut s.anim_clips.timelines;
    let Some(t) = store.timelines.get_mut(path) else {
        return false;
    };
    f(t);
    if let Err(e) = t.save(&store.root.join(path)) {
        s.console.warn(format!("Could not save {path}: {e}"));
    }
    true
}

/// Animation clips as the editor has them (including unsaved edits), for previews.
impl ClipSource for ClipStore {
    fn clip(&self, path: &str) -> Option<&AnimationClip> {
        self.loaded(path)
    }
}
