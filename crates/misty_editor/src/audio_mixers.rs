//! Audio Mixers open in the editor: loaded on first use, edited in memory and written to their
//! `.mixer` file right after every edit. Edits during one drag or text-field session share an
//! undo step. A generation counter tells the Mixer window when to push edits to a running game.

use std::collections::BTreeMap;
use std::path::PathBuf;

use misty_engine::audio::AudioMixer;

use crate::anim_clips::ClipSnapshot;
use crate::state::EditorState;

#[derive(Default)]
pub struct MixerStore {
    root: PathBuf,
    mixers: BTreeMap<String, AudioMixer>,
    /// Undo label of the edit grouped with the current interaction.
    grouping: Option<String>,
    /// Bumped by every change (edits, undo, redo).
    pub generation: u64,
}

impl MixerStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Default::default()
        }
    }

    /// The mixer at a project-relative path, read from disk the first time.
    pub fn get(&mut self, path: &str) -> Result<&AudioMixer, String> {
        if !self.mixers.contains_key(path) {
            let m = AudioMixer::load(&self.root.join(path))?;
            self.mixers.insert(path.to_owned(), m);
        }
        Ok(&self.mixers[path])
    }

    pub fn snapshot(&mut self, path: &str) -> Option<ClipSnapshot> {
        self.get(path)
            .ok()
            .map(|m| ClipSnapshot::Mixer(path.to_owned(), m.clone()))
    }

    /// Put an undo snapshot back (and save it); returns the mixer it replaced.
    pub fn restore(&mut self, path: String, m: AudioMixer) -> ClipSnapshot {
        let _ = m.save(&self.root.join(&path));
        self.generation += 1;
        let old = self.mixers.insert(path.clone(), m).unwrap_or_default();
        ClipSnapshot::Mixer(path, old)
    }

    /// Write a new mixer file and keep it open.
    pub fn create(&mut self, path: &str, m: AudioMixer) -> Result<(), String> {
        m.save(&self.root.join(path))?;
        self.mixers.insert(path.to_owned(), m);
        Ok(())
    }

    /// Follow an asset rename/move of `old` (a mixer or a folder) to `new`.
    pub fn rename(&mut self, old: &str, new: &str) {
        let keys: Vec<String> = self.mixers.keys().cloned().collect();
        for key in keys {
            if let Some(to) = crate::asset_ops::remap(&key, old, new) {
                let m = self.mixers.remove(&key).expect("listed key");
                self.mixers.insert(to, m);
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

/// Change a mixer with undo and save it. Several edits during one interaction (a fader drag,
/// typing a name) become one undo step. Returns false if the mixer does not load.
pub fn edit(
    s: &mut EditorState,
    path: &str,
    label: &str,
    interacting: bool,
    f: impl FnOnce(&mut AudioMixer),
) -> bool {
    let store = &mut s.anim_clips.mixers;
    let Some(snap) = store.snapshot(path) else {
        return false;
    };
    let grouped = interacting && store.grouping.as_deref() == Some(label);
    if !grouped {
        s.undo
            .push_with_clip(label, &s.scene, &s.selection, Some(snap));
        s.anim_clips.mixers.grouping = interacting.then(|| label.to_owned());
    }
    let store = &mut s.anim_clips.mixers;
    let Some(m) = store.mixers.get_mut(path) else {
        return false;
    };
    f(m);
    store.generation += 1;
    if let Err(e) = m.save(&store.root.join(path)) {
        s.console.warn(format!("Could not save {path}: {e}"));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;
    use misty_engine::audio::mixer::MASTER;

    #[test]
    fn edits_save_and_undo() {
        let root = std::env::temp_dir().join(format!("misty_mixer_store_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let path = "Assets/Main.mixer";
        s.anim_clips
            .mixers
            .create(path, AudioMixer::default())
            .unwrap();
        assert!(edit(&mut s, path, "Add Group", false, |m| {
            m.add_group(MASTER, "Music");
        }));
        let on_disk = AudioMixer::load(&root.join(path)).unwrap();
        assert!(on_disk.group_by_name("Music").is_some(), "saved right away");
        let before = s.anim_clips.mixers.generation;
        crate::commands::apply(&mut s, crate::actions::Action::Undo);
        let m = s.anim_clips.mixers.get(path).unwrap();
        assert!(m.group_by_name("Music").is_none(), "undone");
        assert!(s.anim_clips.mixers.generation > before);
        std::fs::remove_dir_all(root).ok();
    }
}
