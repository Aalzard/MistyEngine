//! Animation clips open in the editor: loaded on first use, edited in memory, written back to
//! their `.anim` files once an edit is finished. Animator Controllers live alongside them
//! (`controllers`) so either kind of asset can ride along with an undo snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use misty_engine::animation::AnimationClip;

/// A clip (or Animator Controller) as it was before an edit, kept by undo.
#[derive(Clone)]
pub enum ClipSnapshot {
    Clip(String, AnimationClip),
    Controller(String, misty_engine::animation::AnimatorController),
    Mixer(String, misty_engine::audio::AudioMixer),
}

#[derive(Default)]
pub struct ClipStore {
    root: PathBuf,
    clips: BTreeMap<String, AnimationClip>,
    /// Clips changed since they were last written.
    dirty: BTreeSet<String>,
    /// Animator Controllers being edited.
    pub controllers: crate::anim_controllers::ControllerStore,
    /// Audio Mixers being edited (here so undo can restore them too).
    pub mixers: crate::audio_mixers::MixerStore,
}

impl ClipStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            controllers: crate::anim_controllers::ControllerStore::new(root.clone()),
            mixers: crate::audio_mixers::MixerStore::new(root.clone()),
            root,
            ..Default::default()
        }
    }

    /// The clip at a project-relative path, read from disk the first time.
    pub fn get(&mut self, path: &str) -> Result<&AnimationClip, String> {
        if !self.clips.contains_key(path) {
            let clip = AnimationClip::load(&self.root.join(path))?;
            self.clips.insert(path.to_owned(), clip);
        }
        Ok(&self.clips[path])
    }

    /// Mutable access; the clip is saved at the next [`ClipStore::save_dirty`].
    pub fn get_mut(&mut self, path: &str) -> Option<&mut AnimationClip> {
        self.get(path).ok()?;
        self.dirty.insert(path.to_owned());
        self.clips.get_mut(path)
    }

    pub fn snapshot(&mut self, path: &str) -> Option<ClipSnapshot> {
        self.get(path)
            .ok()
            .map(|c| ClipSnapshot::Clip(path.to_owned(), c.clone()))
    }

    /// Put an undo snapshot back; returns the clip it replaced (for redo).
    pub fn restore(&mut self, snap: ClipSnapshot) -> ClipSnapshot {
        let (path, clip) = match snap {
            ClipSnapshot::Clip(path, clip) => (path, clip),
            ClipSnapshot::Controller(path, c) => return self.controllers.restore(path, c),
            ClipSnapshot::Mixer(path, m) => return self.mixers.restore(path, m),
        };
        let old = self.clips.insert(path.clone(), clip).unwrap_or_default();
        self.dirty.insert(path.clone());
        ClipSnapshot::Clip(path, old)
    }

    /// Write a new clip file and keep it open.
    pub fn create(&mut self, path: &str, clip: AnimationClip) -> Result<(), String> {
        clip.save(&self.root.join(path))?;
        self.clips.insert(path.to_owned(), clip);
        Ok(())
    }

    /// Follow an asset rename/move of `old` (a clip or a folder of clips) to `new`.
    pub fn rename(&mut self, old: &str, new: &str) {
        self.controllers.rename(old, new);
        self.mixers.rename(old, new);
        let keys: Vec<String> = self.clips.keys().cloned().collect();
        for key in keys {
            if let Some(to) = crate::asset_ops::remap(&key, old, new) {
                let clip = self.clips.remove(&key).expect("listed key");
                self.clips.insert(to.clone(), clip);
                if self.dirty.remove(&key) {
                    self.dirty.insert(to);
                }
            }
        }
    }

    /// Write every changed clip; returns error messages.
    pub fn save_dirty(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        for path in std::mem::take(&mut self.dirty) {
            if let Some(c) = self.clips.get(&path)
                && let Err(e) = c.save(&self.root.join(&path))
            {
                errors.push(format!("Could not save {path}: {e}"));
            }
        }
        errors
    }
}
