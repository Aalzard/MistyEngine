//! Undo/Redo with whole-scene snapshots and a named, capped history.
//!
//! Structural commands call [`UndoStack::push`] before changing the scene. Continuous edits
//! (dragging a value, typing a name) are grouped per interaction: a snapshot is taken when the
//! mouse goes down or typing starts, and committed when the interaction ends if anything changed
//! ([`UndoStack::mark_edited`] names the step). A snapshot can also hold one animation clip, the
//! terrain tiles an edit changed, and a material asset, so those undo in the same history.
//! Every snapshot restores the selection it was taken with; selection alone is never a step.
//!
//! The history is a list of steps: `past` are applied (newest last), `future` can be redone
//! (next first at the end). [`UndoStack::jump`] moves to any point, like Edit ▸ Undo History.

use misty_engine::assets::Material;
use misty_engine::scene::Scene;

use crate::anim_clips::{ClipSnapshot, ClipStore};
use crate::assets::AssetDatabase;
use crate::selection::Selection;
use crate::undo_terrain::TerrainUndo;

/// A material asset as it was before an Inspector edit.
pub struct MaterialUndo {
    pub rel: String,
    pub material: Material,
}

impl MaterialUndo {
    /// Put this material back (and save it); returns the one it replaced.
    fn restore(self, assets: &mut AssetDatabase) -> MaterialUndo {
        let current = assets
            .materials
            .get(&self.rel)
            .cloned()
            .unwrap_or_else(|| self.material.clone());
        let _ = assets.update_material(&self.rel, self.material);
        MaterialUndo {
            rel: self.rel,
            material: current,
        }
    }
}

/// Model import settings (Virtual Geometry) and Quality levels as they were before a
/// one-click conversion (see `vgeo_convert`).
pub struct ImportUndo {
    /// (model path, Virtual Geometry import setting).
    pub models: Vec<(String, bool)>,
    /// Every Quality level, when the conversion turned some on.
    pub quality: Option<misty_engine::quality::QualitySettings>,
}

impl ImportUndo {
    /// Put the settings back (saving the `.meta` files and Project Settings); returns the
    /// ones they replaced.
    fn restore(self, to: &mut Restore) -> ImportUndo {
        let mut current = ImportUndo {
            models: Vec::new(),
            quality: None,
        };
        for (rel, on) in &self.models {
            let abs = to.assets.absolute(rel);
            let mut settings = misty_engine::models::ModelImporter::load(&abs);
            current
                .models
                .push((rel.clone(), settings.virtual_geometry));
            settings.virtual_geometry = *on;
            let _ = settings.save(&abs);
        }
        let rels: Vec<String> = self.models.into_iter().map(|(rel, _)| rel).collect();
        misty_engine::models::forget(&rels);
        if let Some(q) = self.quality {
            current.quality = Some(std::mem::replace(&mut to.project_settings.quality, q));
            let settings = &to.project_settings;
            let _ = settings.save(&to.assets.absolute(""));
            misty_engine::quality::install(&settings.quality, misty_engine::quality::level());
        }
        current
    }
}

pub struct Snapshot {
    pub label: String,
    pub scene: Scene,
    pub selection: Selection,
    pub clip: Option<ClipSnapshot>,
    pub terrain: Option<TerrainUndo>,
    pub material: Option<MaterialUndo>,
    pub imports: Option<ImportUndo>,
    /// Approximate memory held (for the history cap).
    bytes: usize,
}

/// Rough size of one scene copy: objects dominate (components are small structs).
const BYTES_PER_OBJECT: usize = 1024;

impl Snapshot {
    fn new(label: &str, scene: &Scene, selection: &Selection) -> Self {
        Self {
            label: label.to_owned(),
            scene: scene.clone(),
            selection: selection.clone(),
            clip: None,
            terrain: None,
            material: None,
            imports: None,
            bytes: 0,
        }
    }

    fn measure(&mut self) {
        let terrain = self.terrain.as_ref().map_or(0, TerrainUndo::bytes);
        let clip = self.clip.as_ref().map_or(0, |_| 16 * 1024);
        self.bytes = 4096 + self.scene.len() * BYTES_PER_OBJECT + terrain + clip;
    }
}

/// What undo and redo write back into.
pub struct Restore<'a> {
    pub scene: &'a mut Scene,
    pub selection: &'a mut Selection,
    pub clips: &'a mut ClipStore,
    pub assets: &'a mut AssetDatabase,
    pub project_settings: &'a mut misty_engine::settings::ProjectSettings,
}

/// History cap: the oldest steps are dropped past either limit (the newest step always stays).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_steps: usize,
    pub max_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_steps: 200,
            max_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Default)]
pub struct UndoStack {
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    candidate: Option<Snapshot>,
    /// Name of the grouped edit in progress (first one wins).
    edited: Option<String>,
    /// Material asset before the edit in progress.
    material: Option<MaterialUndo>,
    pub limits: Limits,
}

impl UndoStack {
    pub fn push(&mut self, label: &str, scene: &Scene, selection: &Selection) {
        self.push_with_clip(label, scene, selection, None);
    }

    /// Like [`UndoStack::push`], also restoring `clip` on undo.
    pub fn push_with_clip(
        &mut self,
        label: &str,
        scene: &Scene,
        selection: &Selection,
        clip: Option<ClipSnapshot>,
    ) {
        let mut snap = Snapshot::new(label, scene, selection);
        snap.clip = clip;
        self.commit(snap);
        // A structural command ends any pending continuous edit.
        self.reset_interaction();
    }

    /// A one-click edit of material asset `rel` (a preset applied from the picker): one step
    /// that puts `before` back on undo. The scene is unchanged by it.
    pub fn push_material(
        &mut self,
        label: &str,
        scene: &Scene,
        selection: &Selection,
        rel: &str,
        before: &Material,
    ) {
        let mut snap = Snapshot::new(label, scene, selection);
        snap.material = Some(MaterialUndo {
            rel: rel.to_owned(),
            material: before.clone(),
        });
        self.commit(snap);
        self.reset_interaction();
    }

    /// A one-click change of model import settings (and Quality levels): one step that puts
    /// `before` back on undo. The scene is unchanged by it.
    pub fn push_imports(
        &mut self,
        label: &str,
        scene: &Scene,
        selection: &Selection,
        before: ImportUndo,
    ) {
        let mut snap = Snapshot::new(label, scene, selection);
        snap.imports = Some(before);
        self.commit(snap);
        self.reset_interaction();
    }

    /// Like [`UndoStack::push`], also restoring terrain data `before` on undo. Call after the
    /// edit, with its result in the terrain store (only the changed tiles are kept). Scene edits
    /// made during the same interaction join this step.
    pub fn push_terrain(
        &mut self,
        label: &str,
        scene: &Scene,
        selection: &Selection,
        before: crate::terrain_tools::TerrainSnapshot,
    ) {
        let mut snap = match (self.candidate.take(), &self.edited) {
            (Some(c), Some(_)) => c,
            _ => Snapshot::new("", scene, selection),
        };
        snap.label = label.to_owned();
        snap.terrain = Some(TerrainUndo::new(before));
        snap.material = self.material.take();
        self.commit(snap);
        self.reset_interaction();
    }

    /// Name the current interaction's edit, e.g. "Change Light Intensity" (first name wins).
    pub fn mark_edited(&mut self, label: &str) {
        if self.edited.is_none() {
            self.edited = Some(label.to_owned());
        }
    }

    /// The current interaction is editing material asset `rel`, which was `before`.
    pub fn note_material(&mut self, rel: &str, before: &Material) {
        if self.candidate.is_some() && self.material.is_none() {
            self.material = Some(MaterialUndo {
                rel: rel.to_owned(),
                material: before.clone(),
            });
        }
    }

    /// Call at the start of every frame with whether the user is mid-interaction.
    /// `clip` gives the clip an interaction may edit (only called when an interaction starts).
    pub fn begin_frame(
        &mut self,
        interacting: bool,
        scene: &Scene,
        selection: &Selection,
        clip: impl FnOnce() -> Option<ClipSnapshot>,
    ) {
        if interacting && self.candidate.is_none() {
            let mut snap = Snapshot::new("", scene, selection);
            snap.clip = clip();
            self.candidate = Some(snap);
        }
    }

    /// Call at the end of every frame; commits the grouped edit once the interaction is over.
    pub fn end_frame(&mut self, interacting: bool) {
        if interacting {
            return;
        }
        if let (Some(mut snap), Some(label)) = (self.candidate.take(), self.edited.take()) {
            snap.label = label;
            snap.material = self.material.take();
            self.commit(snap);
        }
        self.reset_interaction();
    }

    fn reset_interaction(&mut self) {
        self.candidate = None;
        self.edited = None;
        self.material = None;
    }

    /// Add a new step: the redo branch is dropped, then the oldest steps past the cap.
    fn commit(&mut self, mut snap: Snapshot) {
        snap.measure();
        self.past.push(snap);
        self.future.clear();
        self.enforce_limits();
    }

    fn enforce_limits(&mut self) {
        let over_count = self.past.len().saturating_sub(self.limits.max_steps);
        if over_count > 0 {
            self.past.drain(..over_count);
        }
        while self.past.len() > 1 && self.bytes() > self.limits.max_bytes {
            self.past.remove(0);
        }
    }

    /// Approximate memory held by the whole history.
    pub fn bytes(&self) -> usize {
        self.past.iter().chain(&self.future).map(|s| s.bytes).sum()
    }

    /// Every scene held by the history (asset renames rewrite their paths).
    pub fn scenes_mut(&mut self) -> impl Iterator<Item = &mut Scene> {
        self.past
            .iter_mut()
            .chain(self.future.iter_mut())
            .chain(self.candidate.iter_mut())
            .map(|s| &mut s.scene)
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

    /// Every step name, oldest first: applied ones, then the redoable ones.
    pub fn steps(&self) -> Vec<&str> {
        self.past
            .iter()
            .chain(self.future.iter().rev())
            .map(|s| s.label.as_str())
            .collect()
    }

    /// How many of [`UndoStack::steps`] are applied (0 = the state before the first step).
    pub fn position(&self) -> usize {
        self.past.len()
    }

    /// Forget every step (Undo History ▸ Clear History).
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.reset_interaction();
    }

    /// Restore the previous snapshot; returns its label.
    pub fn undo(&mut self, to: &mut Restore) -> Option<String> {
        let snap = self.past.pop()?;
        let (label, current) = Self::swap(snap, to);
        self.future.push(current);
        self.reset_interaction();
        Some(label)
    }

    pub fn redo(&mut self, to: &mut Restore) -> Option<String> {
        let snap = self.future.pop()?;
        let (label, current) = Self::swap(snap, to);
        self.past.push(current);
        self.reset_interaction();
        Some(label)
    }

    /// Undo or redo until `position` steps are applied; returns how many steps moved.
    pub fn jump(&mut self, position: usize, to: &mut Restore) -> usize {
        let mut moved = 0;
        while self.past.len() > position && self.undo(to).is_some() {
            moved += 1;
        }
        while self.past.len() < position && self.redo(to).is_some() {
            moved += 1;
        }
        moved
    }

    /// Put `snap` in place; returns its label and a snapshot of what it replaced.
    fn swap(snap: Snapshot, to: &mut Restore) -> (String, Snapshot) {
        // The active scene and scene file paths stay as they are now (multi-scene editing).
        let mut scene = snap.scene;
        scene.keep_open_set_from(to.scene);
        let current = Snapshot {
            label: snap.label.clone(),
            scene: std::mem::replace(to.scene, scene),
            selection: std::mem::replace(to.selection, snap.selection),
            clip: snap.clip.map(|c| to.clips.restore(c)),
            terrain: snap.terrain.map(TerrainUndo::restore),
            material: snap.material.map(|m| m.restore(to.assets)),
            imports: snap.imports.map(|i| i.restore(to)),
            bytes: snap.bytes,
        };
        (snap.label, current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    fn state(name: &str) -> EditorState {
        EditorState::new(ProjectBrowser::open(std::env::temp_dir().join(name)))
    }

    #[test]
    fn push_undo_redo_round_trip() {
        let mut s = state("misty_undo_round_trip");
        s.record("Create Cube");
        let cube = s.scene.spawn("Cube", None);
        s.selection.set(cube);

        assert_eq!(s.undo_step().as_deref(), Some("Create Cube"));
        assert_eq!(s.scene.len(), 2);
        assert!(s.selection.is_empty());
        assert_eq!(s.redo_step().as_deref(), Some("Create Cube"));
        assert_eq!(s.scene.len(), 3);
        assert_eq!(s.selection.active(), Some(cube));
    }

    #[test]
    fn continuous_edit_is_one_step() {
        let mut s = state("misty_undo_continuous");
        let cam = s.scene.roots()[0];
        for x in 0..10 {
            s.undo.begin_frame(true, &s.scene, &s.selection, || None);
            let mut t = s.scene.transform(cam);
            t.position.x = x as f32 + 1.0;
            s.scene.set_transform(cam, t);
            s.undo.mark_edited("Move Main Camera");
            s.undo.end_frame(true);
        }
        s.undo.begin_frame(false, &s.scene, &s.selection, || None);
        s.undo.end_frame(false);
        assert_eq!(s.undo.steps(), vec!["Move Main Camera"]);
        s.undo_step();
        assert_eq!(s.scene.transform(cam).position.x, 0.0);
        assert!(!s.undo.can_undo());
    }

    #[test]
    fn interaction_without_change_records_nothing() {
        let scene = Scene::default_3d("t");
        let sel = Selection::default();
        let mut undo = UndoStack::default();
        undo.begin_frame(true, &scene, &sel, || None);
        undo.end_frame(false);
        assert!(!undo.can_undo());
    }

    /// Steps named A..E; jumping back and forth lands on the same scenes as undo/redo.
    #[test]
    fn jump_moves_to_any_step_and_back() {
        let mut s = state("misty_undo_jump");
        let base = s.scene.len();
        for n in ["A", "B", "C", "D", "E"] {
            s.record(&format!("Create {n}"));
            s.scene.spawn(n, None);
        }
        assert_eq!(s.undo.position(), 5);
        assert_eq!(s.undo_jump(2), 3);
        assert_eq!(s.scene.len(), base + 2);
        assert_eq!(s.undo.position(), 2);
        // Later steps stay redoable, in order.
        assert_eq!(s.undo.steps().len(), 5);
        assert_eq!(s.undo.redo_label(), Some("Create C"));
        assert_eq!(s.undo_jump(4), 2);
        assert_eq!(s.scene.len(), base + 4);
        assert_eq!(s.undo_jump(0), 4);
        assert_eq!(s.scene.len(), base);
        assert_eq!(s.undo_jump(99), 5);
        assert_eq!(s.scene.len(), base + 5);
        // A new step after jumping back drops the redo branch.
        s.undo_jump(3);
        s.record("Create F");
        assert_eq!(
            s.undo.steps(),
            vec!["Create A", "Create B", "Create C", "Create F"]
        );
    }

    #[test]
    fn history_is_capped_by_count_and_bytes() {
        let mut s = state("misty_undo_cap");
        s.undo.limits.max_steps = 3;
        for i in 0..6 {
            s.record(&format!("Step {i}"));
        }
        assert_eq!(s.undo.steps(), vec!["Step 3", "Step 4", "Step 5"]);
        // A byte budget smaller than two snapshots keeps only the newest step.
        s.undo.limits.max_bytes = s.undo.bytes() / 3 + 1;
        s.record("Step 6");
        assert_eq!(s.undo.steps(), vec!["Step 6"]);
    }

    #[test]
    fn clear_forgets_everything() {
        let mut s = state("misty_undo_clear");
        s.record("Create Cube");
        s.record("Create Sphere");
        s.undo_step();
        s.undo.clear();
        assert!(s.undo.steps().is_empty());
        assert_eq!(s.undo.undo_label(), None);
        assert_eq!(s.undo.redo_label(), None);
    }

    /// A material edit inside an interaction undoes with that step.
    #[test]
    fn material_edit_undoes() {
        let root = std::env::temp_dir().join("misty_undo_material");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root));
        let rel = "Assets/Test.mat";
        s.assets.update_material(rel, Material::default()).unwrap();
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
        let before = s.assets.materials[rel].clone();
        let mut m = before.clone();
        m.metallic = 0.75;
        s.undo.note_material(rel, &before);
        s.assets.update_material(rel, m).unwrap();
        s.undo.mark_edited("Change Material Metallic");
        s.undo.end_frame(false);
        s.undo_step();
        assert_eq!(s.assets.materials[rel].metallic, before.metallic);
        s.redo_step();
        assert_eq!(s.assets.materials[rel].metallic, 0.75);
    }

    /// A preset picked for a material is its own step.
    #[test]
    fn material_preset_step_undoes() {
        let root = std::env::temp_dir().join("misty_undo_material_preset");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root));
        let rel = "Assets/Test.mat";
        s.assets.update_material(rel, Material::default()).unwrap();
        let before = s.assets.materials[rel].clone();
        let (scene, selection) = (&s.scene, &s.selection);
        s.undo
            .push_material("Apply Preset Shiny", scene, selection, rel, &before);
        let mut m = before.clone();
        m.metallic = 0.9;
        s.assets.update_material(rel, m).unwrap();
        assert_eq!(s.undo_step().as_deref(), Some("Apply Preset Shiny"));
        assert_eq!(s.assets.materials[rel].metallic, before.metallic);
        s.redo_step();
        assert_eq!(s.assets.materials[rel].metallic, 0.9);
    }
}
