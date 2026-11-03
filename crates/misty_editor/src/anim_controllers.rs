//! Animator Controllers open in the editor: loaded on first use, edited in memory and written to
//! their `.controller` file right after every edit. Edits made during one mouse drag or one
//! text-field session share a single undo step.

use std::collections::BTreeMap;
use std::path::PathBuf;

use misty_engine::animation::AnimatorController;

use crate::anim_clips::ClipSnapshot;
use crate::state::EditorState;

#[derive(Default)]
pub struct ControllerStore {
    root: PathBuf,
    controllers: BTreeMap<String, AnimatorController>,
    /// Undo label of the edit grouped with the current interaction.
    grouping: Option<String>,
}

impl ControllerStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Default::default()
        }
    }

    /// The controller at a project-relative path, read from disk the first time.
    pub fn get(&mut self, path: &str) -> Result<&AnimatorController, String> {
        if !self.controllers.contains_key(path) {
            let c = AnimatorController::load(&self.root.join(path))?;
            self.controllers.insert(path.to_owned(), c);
        }
        Ok(&self.controllers[path])
    }

    pub fn snapshot(&mut self, path: &str) -> Option<ClipSnapshot> {
        self.get(path)
            .ok()
            .map(|c| ClipSnapshot::Controller(path.to_owned(), c.clone()))
    }

    /// Put an undo snapshot back (and save it); returns the controller it replaced.
    pub fn restore(&mut self, path: String, c: AnimatorController) -> ClipSnapshot {
        let _ = c.save(&self.root.join(&path));
        let old = self.controllers.insert(path.clone(), c).unwrap_or_default();
        ClipSnapshot::Controller(path, old)
    }

    /// Write a new controller file and keep it open.
    pub fn create(&mut self, path: &str, c: AnimatorController) -> Result<(), String> {
        c.save(&self.root.join(path))?;
        self.controllers.insert(path.to_owned(), c);
        Ok(())
    }

    /// Follow an asset rename/move of `old` (a controller or a folder) to `new`; clip paths
    /// inside open controllers follow too.
    pub fn rename(&mut self, old: &str, new: &str) {
        let keys: Vec<String> = self.controllers.keys().cloned().collect();
        for key in keys {
            if let Some(to) = crate::asset_ops::remap(&key, old, new) {
                let c = self.controllers.remove(&key).expect("listed key");
                self.controllers.insert(to, c);
            }
        }
        for c in self.controllers.values_mut() {
            for s in &mut c.states {
                let clips = s.motion.iter_mut().chain(
                    s.blend_tree
                        .iter_mut()
                        .flat_map(|b| b.children.iter_mut().filter_map(|c| c.motion.as_mut())),
                );
                for m in clips {
                    if let Some(to) = crate::asset_ops::remap(m, old, new) {
                        *m = to;
                    }
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

/// The user is mid-interaction (dragging, or typing in a field).
pub fn interacting(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()
}

/// Change a controller with undo and save it. Several edits during one interaction (a drag or
/// typing) become one undo step. Returns false if the controller does not load.
pub fn edit(
    s: &mut EditorState,
    path: &str,
    label: &str,
    interacting: bool,
    f: impl FnOnce(&mut AnimatorController),
) -> bool {
    let store = &mut s.anim_clips.controllers;
    let Some(snap) = store.snapshot(path) else {
        return false;
    };
    let grouped = interacting && store.grouping.as_deref() == Some(label);
    if !grouped {
        s.undo
            .push_with_clip(label, &s.scene, &s.selection, Some(snap));
        s.anim_clips.controllers.grouping = interacting.then(|| label.to_owned());
    }
    let store = &mut s.anim_clips.controllers;
    let Some(c) = store.controllers.get_mut(path) else {
        return false;
    };
    f(c);
    if let Err(e) = c.save(&store.root.join(path)) {
        s.console.warn(format!("Could not save {path}: {e}"));
    }
    true
}
