//! Scenes added to the world a slice at a time (World Partition cells).
//!
//! [`parse`] does everything that doesn't touch the world (JSON parsing, the asset references a
//! cell needs) and runs on a background thread; [`Scene::begin_streamed`] makes the scene entry
//! and reserves an entity for every object, and [`Scene::continue_streamed`] then creates whole
//! top-level subtrees until a deadline, so one frame never does more than its budget (a subtree
//! is never left half made: scripts and physics only ever see complete objects). References
//! between objects work across slices because every entity id is reserved up front.

use std::collections::BTreeSet;
use std::time::Instant;

use hecs::Entity;

use super::file::{LoadReport, ObjectEntry, SceneParts, read_scene_parts, remap_refs};
use super::multi_scene::{SceneEntry, SceneId, name_from_path};
use super::{Node, Scene, SceneComponent, registry};

/// A scene file parsed off the main thread, ready for [`Scene::begin_streamed`].
pub struct ParsedScene {
    parts: SceneParts,
    pub report: LoadReport,
    /// Size of the file text (memory estimates).
    pub bytes: usize,
}

impl ParsedScene {
    /// Objects in the file (every level of the hierarchy).
    pub fn object_count(&self) -> usize {
        self.parts.objects.len()
    }

    /// Project-relative asset paths the objects name (`Assets/…`, model mesh indices removed),
    /// sorted: what a loader can read ahead of time.
    pub fn asset_refs(&self) -> Vec<String> {
        fn walk(v: &serde_json::Value, out: &mut BTreeSet<String>) {
            match v {
                serde_json::Value::String(s) => {
                    let path = s.rsplit_once('#').map_or(s.as_str(), |(p, _)| p);
                    // Project paths, or absolute paths of asset files (tests, rebased cells).
                    let absolute = std::path::Path::new(path).is_absolute()
                        && path.rsplit_once('.').is_some_and(|(_, e)| e.len() <= 8);
                    if path.starts_with("Assets/") || absolute {
                        out.insert(path.to_owned());
                    }
                }
                serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
                serde_json::Value::Object(map) => map.values().for_each(|i| walk(i, out)),
                _ => {}
            }
        }
        let mut out = BTreeSet::new();
        for o in &self.parts.objects {
            for v in o.components.values() {
                walk(v, &mut out);
            }
        }
        out.into_iter().collect()
    }

    /// Point every `Assets/…` path at `root` (absolute paths), so the cell loads the same
    /// whatever the process-wide project root is (tests running beside others).
    pub fn rebase(&mut self, root: &std::path::Path) {
        fn walk(v: &mut serde_json::Value, root: &std::path::Path) {
            match v {
                serde_json::Value::String(s) if s.starts_with("Assets/") => {
                    *s = root.join(s.as_str()).to_string_lossy().replace('\\', "/");
                }
                serde_json::Value::Array(items) => items.iter_mut().for_each(|i| walk(i, root)),
                serde_json::Value::Object(map) => map.values_mut().for_each(|i| walk(i, root)),
                _ => {}
            }
        }
        for o in &mut self.parts.objects {
            for v in o.components.values_mut() {
                walk(v, root);
            }
        }
    }

    /// World-space positions of the top-level objects (their Transform position).
    pub fn root_positions(&self) -> Vec<glam::Vec3> {
        self.parts
            .objects
            .iter()
            .filter(|o| o.parent.is_none())
            .map(|o| {
                o.components
                    .get("Transform")
                    .and_then(|t| t.get("position"))
                    .and_then(|p| serde_json::from_value::<[f32; 3]>(p.clone()).ok())
                    .map_or(glam::Vec3::ZERO, glam::Vec3::from)
            })
            .collect()
    }
}

/// Parse `.scene` text (safe on any thread).
pub fn parse(text: &str) -> Result<ParsedScene, String> {
    let (parts, report) = read_scene_parts(text)?;
    Ok(ParsedScene {
        parts,
        report,
        bytes: text.len(),
    })
}

/// A scene being added to the world over several frames.
pub struct StreamedLoad {
    pub id: SceneId,
    objects: Vec<Option<ObjectEntry>>,
    /// Reserved entity per object.
    entities: Vec<Entity>,
    /// Objects created so far (the next one to create).
    next: usize,
    pub report: LoadReport,
}

impl StreamedLoad {
    pub fn is_done(&self) -> bool {
        self.next >= self.objects.len()
    }

    /// Objects created / in the file.
    pub fn progress(&self) -> (usize, usize) {
        (self.next, self.objects.len())
    }

    /// Entities reserved for objects not created yet (to free when the load is abandoned).
    pub fn unspawned(&self) -> &[Entity] {
        &self.entities[self.next.min(self.entities.len())..]
    }
}

impl Scene {
    /// Start adding a parsed scene file as another loaded scene (`path` is project-relative):
    /// its entry is listed at once, marked [`SceneEntry::partial`] until every object exists.
    pub fn begin_streamed(
        &mut self,
        path: &str,
        parsed: ParsedScene,
    ) -> Result<StreamedLoad, String> {
        let existing = self.find_by_path(path);
        if let Some(id) = existing
            && self.entry(id).is_some_and(|s| s.loaded)
        {
            return Err(format!("'{}' is already open", name_from_path(path)));
        }
        let ParsedScene { parts, report, .. } = parsed;
        let id = existing.unwrap_or_else(|| {
            let id = self.next_scene_id;
            self.next_scene_id += 1;
            self.scenes.push(SceneEntry::new(
                id,
                &name_from_path(path),
                Some(path.into()),
            ));
            id
        });
        let entry = self.scenes.iter_mut().find(|s| s.id == id).expect("listed");
        entry.loaded = true;
        entry.partial = true;
        entry.streamed = true;
        entry.set_stash(parts.lighting, parts.navigation);
        let entities: Vec<Entity> = self
            .world
            .reserve_entities(parts.objects.len() as u32)
            .collect();
        Ok(StreamedLoad {
            id,
            objects: parts.objects.into_iter().map(Some).collect(),
            entities,
            next: 0,
            report,
        })
    }

    /// Create objects of `load` until `deadline` (at least one top-level subtree per call, so a
    /// load always progresses). Returns the top-level objects made by this call; the entry stops
    /// being partial once the last object exists.
    pub fn continue_streamed(&mut self, load: &mut StreamedLoad, deadline: Instant) -> Vec<Entity> {
        let mut roots = Vec::new();
        if !self.entry(load.id).is_some_and(|s| s.loaded) {
            return roots;
        }
        let n = load.objects.len();
        while load.next < n {
            if !roots.is_empty() && Instant::now() >= deadline {
                break;
            }
            // One whole top-level subtree: the root and every object after it that isn't the
            // next root (files list subtrees depth-first).
            let start = load.next;
            let mut end = start + 1;
            while end < n
                && load.objects[end]
                    .as_ref()
                    .is_some_and(|o| o.parent.is_some())
            {
                end += 1;
            }
            for i in start..end {
                let obj = load.objects[i].take().expect("created once");
                self.spawn_streamed_object(load, i, obj);
            }
            load.next = end;
            roots.push(load.entities[start]);
        }
        if load.is_done() {
            crate::mesh_builder::sync(self);
            if let Some(e) = self.scenes.iter_mut().find(|s| s.id == load.id) {
                e.partial = false;
            }
        }
        roots
    }

    fn spawn_streamed_object(&mut self, load: &mut StreamedLoad, i: usize, mut obj: ObjectEntry) {
        let e = load.entities[i];
        self.world.spawn_at(
            e,
            (Node {
                scene: load.id,
                ..Default::default()
            },),
        );
        let all = &load.entities;
        remap_refs(&mut obj.components, |index| {
            all.get(index as usize).map(|e| e.to_bits().get())
        });
        let report = &mut load.report;
        for (comp, value) in obj.components {
            match registry::find(&comp) {
                Some(info) if info.name != Node::NAME => {
                    if let Err(err) = (info.from_json)(&mut self.world, e, value) {
                        report.warnings.push(format!("object {i}: {comp}: {err}"));
                    }
                }
                _ => report
                    .warnings
                    .push(format!("object {i}: unknown component '{comp}' skipped")),
            }
        }
        for required in ["GameObject", "Transform"] {
            let info = registry::find(required).expect("built-in component");
            if !(info.has)(&self.world, e) {
                (info.add_default)(&mut self.world, e);
                report
                    .warnings
                    .push(format!("object {i}: missing {required}, added default"));
            }
        }
        // Parents always come earlier in the file, so they exist by now.
        let parent = obj.parent.and_then(|p| (p < i).then(|| load.entities[p]));
        if obj.parent.is_some() && parent.is_none() {
            report.warnings.push(format!(
                "object {i}: parent index out of order, placed at root"
            ));
        }
        self.attach(e, parent, None);
    }

    /// Stop a streamed load part way: the objects made so far are removed with the scene's
    /// entry (see [`Scene::remove_streamed`]) and the reserved entities are freed.
    pub fn abandon_streamed(&mut self, load: StreamedLoad) {
        for e in load.unspawned() {
            let _ = self.world.despawn(*e);
        }
    }

    /// `.scene` JSON of the subtrees under `roots` (references to objects outside them become
    /// null); `compact` = one line (World Partition cells, which are many and generated).
    pub fn subtrees_json(&self, roots: &[Entity], compact: bool) -> String {
        let set: std::collections::HashSet<Entity> = roots.iter().copied().collect();
        let objects = self.entries_where(|e| {
            let mut cur = Some(e);
            while let Some(x) = cur {
                if set.contains(&x) {
                    return true;
                }
                cur = self.parent(x);
            }
            false
        });
        if compact {
            super::file::write_json_compact(objects)
        } else {
            super::file::write_json(objects, None, None)
        }
    }

    /// Take a streamed scene out of the open set once its objects are gone (they are removed
    /// by the caller a few at a time, so scripts and physics can see them go).
    pub fn remove_streamed(&mut self, id: SceneId) {
        for r in self.roots_in(id) {
            self.despawn(r);
        }
        if id == self.active {
            return;
        }
        self.scenes.retain(|s| s.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{HingeJoint, Transform};
    use glam::Vec3;
    use std::time::Duration;

    fn cell_text(rocks: usize) -> String {
        let mut s = Scene::new("Cell");
        for i in 0..rocks {
            let r = s.spawn(&format!("Rock {i}"), None);
            s.set_transform(r, Transform::from_position(Vec3::new(i as f32, 0.0, 0.0)));
            let child = s.spawn("Moss", Some(r));
            s.add_by_name(child, "Mesh Filter");
        }
        // A reference to an object created later in the file.
        let door = s.spawn("Door", None);
        s.add_by_name(door, "Rigidbody");
        s.add_by_name(door, "Hinge Joint");
        let frame = s.spawn("Frame", None);
        s.add_by_name(frame, "Rigidbody");
        s.world.get::<&mut HingeJoint>(door).unwrap().connected_body = Some(frame);
        s.to_json()
    }

    #[test]
    fn a_streamed_scene_arrives_in_whole_subtrees_and_matches_a_normal_load() {
        let text = cell_text(20);
        let mut normal = Scene::default_3d("Main");
        let (nid, _, _) = normal.load_additive("Assets/Cell.scene", &text).unwrap();

        let mut s = Scene::default_3d("Main");
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.object_count(), 42);
        assert_eq!(parsed.root_positions().len(), 22);
        let mut load = s.begin_streamed("Assets/Cell.scene", parsed).unwrap();
        assert!(s.entry(load.id).unwrap().partial);
        // An expired deadline still makes one whole subtree per call.
        let past = Instant::now() - Duration::from_millis(1);
        let mut calls = 0;
        while !load.is_done() {
            let roots = s.continue_streamed(&mut load, past);
            assert_eq!(roots.len(), 1);
            for r in &roots {
                assert!(s.parent(*r).is_none());
                // A rock arrives with its child.
                if s.name(*r).starts_with("Rock") {
                    assert_eq!(s.children(*r).len(), 1);
                }
            }
            calls += 1;
        }
        assert_eq!(calls, 22);
        assert!(!s.entry(load.id).unwrap().partial);
        assert!(s.entry(load.id).unwrap().streamed);
        assert!(
            load.report.warnings.is_empty(),
            "{:?}",
            load.report.warnings
        );
        // Same objects, same file.
        assert_eq!(s.scene_to_json(load.id), normal.scene_to_json(nid));
        let door = s.find_by_name("Door").unwrap();
        let frame = s.find_by_name("Frame").unwrap();
        assert_eq!(
            s.world.get::<&HingeJoint>(door).unwrap().connected_body,
            Some(frame),
            "a reference to an object made in a later slice"
        );
    }

    #[test]
    fn a_generous_deadline_loads_everything_in_one_call_and_removal_frees_the_entry() {
        let mut s = Scene::default_3d("Main");
        let before = s.len();
        let mut load = s
            .begin_streamed("Assets/Cell.scene", parse(&cell_text(5)).unwrap())
            .unwrap();
        let roots = s.continue_streamed(&mut load, Instant::now() + Duration::from_secs(5));
        assert_eq!(roots.len(), 7);
        assert!(load.is_done());
        let id = load.id;
        assert!(
            s.begin_streamed("Assets/Cell.scene", parse(&cell_text(1)).unwrap())
                .is_err()
        );
        s.remove_streamed(id);
        assert_eq!(s.len(), before);
        assert!(s.entry(id).is_none());
    }

    #[test]
    fn an_abandoned_load_leaves_nothing_behind() {
        let mut s = Scene::default_3d("Main");
        let before = s.len();
        let mut load = s
            .begin_streamed("Assets/Cell.scene", parse(&cell_text(8)).unwrap())
            .unwrap();
        let past = Instant::now() - Duration::from_millis(1);
        s.continue_streamed(&mut load, past);
        s.continue_streamed(&mut load, past);
        let id = load.id;
        s.abandon_streamed(load);
        s.remove_streamed(id);
        assert_eq!(
            s.len(),
            before,
            "reserved entities and made objects are gone"
        );
        assert!(s.find_by_name("Rock 0").is_none());
    }
}
