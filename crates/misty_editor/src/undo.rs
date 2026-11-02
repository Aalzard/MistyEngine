//! Undo/Redo with whole-scene snapshots.
//!
//! Structural commands call [`UndoStack::push`] before changing the scene. Continuous edits
//! (dragging a value, typing a name) are grouped per interaction: a snapshot is taken when the
//! mouse goes down or typing starts, and committed when the interaction ends if anything changed.

use misty_engine::scene::Scene;

use crate::selection::Selection;

pub struct Snapshot {
    pub label: String,
    pub scene: Scene,
    pub selection: Selection,
}

#[derive(Default)]
pub struct UndoStack {
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    candidate: Option<Snapshot>,
    edited: Option<String>,
}

const LIMIT: usize = 200;

impl UndoStack {
    pub fn push(&mut self, label: &str, scene: &Scene, selection: &Selection) {
        self.past.push(Snapshot {
            label: label.to_owned(),
            scene: scene.clone(),
            selection: selection.clone(),
        });
        if self.past.len() > LIMIT {
            self.past.remove(0);
        }
        self.future.clear();
        // A structural command ends any pending continuous edit.
        self.candidate = None;
        self.edited = None;
    }

    /// Mark that the current interaction changed something (e.g. "Transform").
    pub fn mark_edited(&mut self, what: &str) {
        if self.edited.is_none() {
            self.edited = Some(what.to_owned());
        }
    }

    /// Call at the start of every frame with whether the user is mid-interaction.
    pub fn begin_frame(&mut self, interacting: bool, scene: &Scene, selection: &Selection) {
        if interacting && self.candidate.is_none() {
            self.candidate = Some(Snapshot {
                label: String::new(),
                scene: scene.clone(),
                selection: selection.clone(),
            });
        }
    }

    /// Call at the end of every frame; commits the grouped edit once the interaction is over.
    pub fn end_frame(&mut self, interacting: bool) {
        if interacting {
            return;
        }
        if let (Some(mut snap), Some(what)) = (self.candidate.take(), self.edited.take()) {
            snap.label = format!("Modify {what}");
            self.past.push(snap);
            self.future.clear();
        }
        self.candidate = None;
        self.edited = None;
    }

    #[cfg(test)]
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|s| s.label.as_str())
    }

    /// Restore the previous snapshot; returns its label.
    pub fn undo(&mut self, scene: &mut Scene, selection: &mut Selection) -> Option<String> {
        let snap = self.past.pop()?;
        Some(self.swap(snap, scene, selection, true))
    }

    pub fn redo(&mut self, scene: &mut Scene, selection: &mut Selection) -> Option<String> {
        let snap = self.future.pop()?;
        Some(self.swap(snap, scene, selection, false))
    }

    fn swap(
        &mut self,
        snap: Snapshot,
        scene: &mut Scene,
        selection: &mut Selection,
        undo: bool,
    ) -> String {
        let label = snap.label.clone();
        let current = Snapshot {
            label: snap.label,
            scene: std::mem::replace(scene, snap.scene),
            selection: std::mem::replace(selection, snap.selection),
        };
        if undo {
            self.future.push(current);
        } else {
            self.past.push(current);
        }
        self.candidate = None;
        self.edited = None;
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_undo_redo_round_trip() {
        let mut scene = Scene::default_3d("t");
        let mut sel = Selection::default();
        let mut undo = UndoStack::default();
        undo.push("Create Cube", &scene, &sel);
        let cube = scene.spawn("Cube", None);
        sel.set(cube);

        assert_eq!(
            undo.undo(&mut scene, &mut sel).as_deref(),
            Some("Create Cube")
        );
        assert_eq!(scene.len(), 2);
        assert!(sel.is_empty());
        assert_eq!(
            undo.redo(&mut scene, &mut sel).as_deref(),
            Some("Create Cube")
        );
        assert_eq!(scene.len(), 3);
        assert_eq!(sel.active(), Some(cube));
    }

    #[test]
    fn continuous_edit_is_one_step() {
        let mut scene = Scene::default_3d("t");
        let sel = Selection::default();
        let mut undo = UndoStack::default();
        let cam = scene.roots()[0];
        for x in 0..10 {
            undo.begin_frame(true, &scene, &sel);
            let mut t = scene.transform(cam);
            t.position.x = x as f32;
            scene.set_transform(cam, t);
            undo.mark_edited("Transform");
            undo.end_frame(true);
        }
        undo.begin_frame(false, &scene, &sel);
        undo.end_frame(false);
        assert_eq!(undo.undo_label(), Some("Modify Transform"));
        let mut sel = sel;
        undo.undo(&mut scene, &mut sel);
        assert_eq!(scene.transform(cam).position.x, 0.0);
        assert!(!undo.can_undo());
    }

    #[test]
    fn interaction_without_change_records_nothing() {
        let scene = Scene::default_3d("t");
        let sel = Selection::default();
        let mut undo = UndoStack::default();
        undo.begin_frame(true, &scene, &sel);
        undo.end_frame(false);
        assert!(!undo.can_undo());
    }
}
