//! Avatar Masks open in the editor: loaded on first use, edited in memory and written to their
//! `.mask` file right after every edit, with undo (one step per interaction).

use std::collections::BTreeMap;
use std::path::PathBuf;

use misty_engine::animation::AvatarMask;

use crate::anim_clips::ClipSnapshot;
use crate::state::EditorState;

#[derive(Default)]
pub struct MaskStore {
    root: PathBuf,
    masks: BTreeMap<String, AvatarMask>,
    /// Undo label of the edit grouped with the current interaction.
    grouping: Option<String>,
    /// Bumped by every change to a held mask (Play mode hands changed masks to the game, see
    /// `play::sync_controllers`).
    generation: u64,
    /// The generation of each mask's last change.
    versions: BTreeMap<String, u64>,
}

impl MaskStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Default::default()
        }
    }

    /// The mask at a project-relative path, read from disk the first time.
    pub fn get(&mut self, path: &str) -> Result<&AvatarMask, String> {
        if !self.masks.contains_key(path) {
            let m = AvatarMask::load(&self.root.join(path))?;
            self.masks.insert(path.to_owned(), m);
        }
        Ok(&self.masks[path])
    }

    /// Bumped by every change to a mask the editor holds.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn touch(&mut self, path: &str) {
        self.generation += 1;
        self.versions.insert(path.to_owned(), self.generation);
    }

    /// Masks changed after generation `since`, as their saved files read back.
    pub fn changed_since(&self, since: u64) -> Vec<(String, AvatarMask)> {
        self.versions
            .iter()
            .filter(|(_, v)| **v > since)
            .filter_map(|(p, _)| {
                let m = self.masks.get(p)?;
                let saved = AvatarMask::from_json(&m.to_json()).ok()?;
                Some((p.clone(), saved))
            })
            .collect()
    }

    pub fn snapshot(&mut self, path: &str) -> Option<ClipSnapshot> {
        self.get(path)
            .ok()
            .map(|m| ClipSnapshot::Mask(path.to_owned(), m.clone()))
    }

    /// Put an undo snapshot back (and save it); returns the mask it replaced.
    pub fn restore(&mut self, path: String, m: AvatarMask) -> ClipSnapshot {
        let _ = m.save(&self.root.join(&path));
        let old = self.masks.insert(path.clone(), m).unwrap_or_default();
        self.touch(&path);
        ClipSnapshot::Mask(path, old)
    }

    /// Follow an asset rename/move of `old` (a mask or a folder) to `new`.
    pub fn rename(&mut self, old: &str, new: &str) {
        let keys: Vec<String> = self.masks.keys().cloned().collect();
        for key in keys {
            if let Some(to) = crate::asset_ops::remap(&key, old, new) {
                let m = self.masks.remove(&key).expect("listed key");
                self.versions.remove(&key);
                self.masks.insert(to.clone(), m);
                self.touch(&to);
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

/// Change a mask with undo and save it. Returns false if the mask does not load.
pub fn edit(
    s: &mut EditorState,
    path: &str,
    label: &str,
    interacting: bool,
    f: impl FnOnce(&mut AvatarMask),
) -> bool {
    let store = &mut s.anim_clips.masks;
    let Some(snap) = store.snapshot(path) else {
        return false;
    };
    let grouped = interacting && store.grouping.as_deref() == Some(label);
    if !grouped {
        s.undo.push_asset(label, &s.selection, snap);
        s.anim_clips.masks.grouping = interacting.then(|| label.to_owned());
    }
    let store = &mut s.anim_clips.masks;
    let Some(m) = store.masks.get_mut(path) else {
        return false;
    };
    f(m);
    if let Err(e) = m.save(&store.root.join(path)) {
        s.console.warn(format!("Could not save {path}: {e}"));
    }
    store.touch(path);
    true
}
