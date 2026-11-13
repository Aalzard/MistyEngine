//! The documents autosave backs up, their snapshots and the backup file format.
//!
//! A backup file is one line of JSON ([`Header`]: what the document is, where its file is, when
//! the backup was taken) followed by the document exactly as its own file would hold it (scene,
//! prefab and graph JSON, script text). One file says everything about itself, so a backup cut
//! short by a power loss can't be paired with the wrong document.

use std::hash::{Hash, Hasher};

use misty_engine::hecs::Entity;
use misty_engine::scene::{Scene, SceneId};
use serde::{Deserialize, Serialize};

use crate::state::EditorState;

/// The kind of document a backup holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    Scene,
    Prefab,
    ScriptGraph,
    ShaderGraph,
    EffectGraph,
    Script,
}

impl DocKind {
    /// Short name (backup folder names, document keys).
    pub fn id(self) -> &'static str {
        match self {
            DocKind::Scene => "scene",
            DocKind::Prefab => "prefab",
            DocKind::ScriptGraph => "script_graph",
            DocKind::ShaderGraph => "shader_graph",
            DocKind::EffectGraph => "effect_graph",
            DocKind::Script => "script",
        }
    }

    /// What the recovery dialog calls it (English; translated where shown).
    pub fn label(self) -> &'static str {
        match self {
            DocKind::Scene => "Scene",
            DocKind::Prefab => "Prefab",
            DocKind::ScriptGraph => "Script Graph",
            DocKind::ShaderGraph => "Shader Graph",
            DocKind::EffectGraph => "Effect Graph",
            DocKind::Script => "Script",
        }
    }

    /// Restoring order: scenes first (a restored prefab opens Prefab Mode over them).
    pub fn order(self) -> u8 {
        match self {
            DocKind::Scene => 0,
            DocKind::ScriptGraph | DocKind::ShaderGraph | DocKind::EffectGraph => 1,
            DocKind::Script => 2,
            DocKind::Prefab => 3,
        }
    }
}

/// The first line of a backup file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// Format version (1).
    pub misty_autosave: u32,
    pub kind: DocKind,
    /// Project-relative path of the document's file; `None` for a scene never saved.
    pub path: Option<String>,
    /// Name shown in the recovery dialog.
    pub name: String,
    /// When the snapshot was taken (Unix ms).
    pub time_ms: u64,
    /// Written by the panic handler (the editor was crashing).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub emergency: bool,
}

pub const FORMAT: u32 = 1;

/// A document with unsaved changes, to snapshot (resolved when its turn comes, so a run spread
/// over a few frames snapshots what the document holds then).
#[derive(Clone, Debug, PartialEq)]
pub enum DocRef {
    /// Scene `id` of backup layer `layer` (see [`layers`]).
    Scene {
        layer: usize,
        id: SceneId,
    },
    /// The prefab of backup layer `layer`.
    Prefab {
        layer: usize,
    },
    ScriptGraph,
    ShaderGraph,
    EffectGraph,
    /// A script open in the Code Editor (project-relative path).
    Script(String),
}

/// One document's contents, ready to write.
#[derive(Clone, Debug)]
pub struct DocSnapshot {
    /// Identity of the document across autosaves ("scene:Assets/Scenes/Level.scene").
    pub key: String,
    pub kind: DocKind,
    pub path: Option<String>,
    pub name: String,
    pub body: String,
    pub emergency: bool,
}

fn hash_of(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl DocSnapshot {
    pub fn new(kind: DocKind, path: Option<String>, name: String, body: String) -> Self {
        let key = match &path {
            Some(p) => format!("{}:{p}", kind.id()),
            None => format!("{}:untitled:{name}", kind.id()),
        };
        Self {
            key,
            kind,
            path,
            name,
            body,
            emergency: false,
        }
    }

    /// The document's backup folder: kind, a readable name and a hash of the key
    /// ("scene-Level-1a2b3c4d").
    pub fn folder_name(&self) -> String {
        folder_name(self.kind, &self.name, &self.key)
    }

    pub fn content_hash(&self) -> u64 {
        hash_of(&self.body)
    }

    /// The backup file: the header line, then the document.
    pub fn file_bytes(&self, time_ms: u64) -> Vec<u8> {
        let header = Header {
            misty_autosave: FORMAT,
            kind: self.kind,
            path: self.path.clone(),
            name: self.name.clone(),
            time_ms,
            emergency: self.emergency,
        };
        let mut out = serde_json::to_string(&header)
            .expect("header serialises")
            .into_bytes();
        out.push(b'\n');
        out.extend_from_slice(self.body.as_bytes());
        out
    }
}

/// See [`DocSnapshot::folder_name`].
pub fn folder_name(kind: DocKind, name: &str, key: &str) -> String {
    let readable: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    format!("{}-{readable}-{:08x}", kind.id(), hash_of(key) as u32)
}

/// Split a backup file into its header and the document. `None`: not a backup, or cut short
/// before the header ended.
pub fn parse_backup(bytes: &[u8]) -> Option<(Header, &str)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let (first, body) = text.split_once('\n')?;
    let header: Header = serde_json::from_str(first).ok()?;
    (header.misty_autosave <= FORMAT).then_some((header, body))
}

// ---- what to back up ------------------------------------------------------------------------

/// Scenes (and maybe a prefab) autosave looks at, outermost first. Editing scenes: the open
/// scenes. Playing: the scenes as they were before Play (Play-mode changes are thrown away on
/// Stop). In Prefab Mode: the scenes underneath, each outer prefab, then the open prefab.
pub struct Layer<'a> {
    pub scene: &'a Scene,
    /// The editor's "changed" flag for this layer.
    pub dirty: bool,
    /// Scenes with changes, as last worked out (several scenes open).
    pub dirty_list: &'a [SceneId],
    /// The prefab this layer edits (path, root object); `None`: scenes.
    pub prefab: Option<(&'a str, Entity)>,
}

pub fn layers(s: &EditorState) -> Vec<Layer<'_>> {
    if crate::prefab_mode::active(s) {
        return crate::prefab_mode::backup_layers(s);
    }
    if let Some((scene, dirty)) = crate::play::edit_scene(s) {
        return vec![Layer {
            scene,
            dirty,
            dirty_list: &s.files.dirty,
            prefab: None,
        }];
    }
    vec![Layer {
        scene: &s.scene,
        dirty: s.scene_dirty,
        dirty_list: &s.files.dirty,
        prefab: None,
    }]
}

/// Prefab Mode levels and Play mode: a run started under one shape stops when it changes.
pub fn shape(s: &EditorState) -> (usize, bool) {
    (s.prefab_stages.len(), s.play.in_play_mode())
}

/// Loaded scenes of a layer with unsaved changes (like `scene_files::dirty_now`, without
/// serialising every scene: the list the editor keeps fresh is used).
pub fn layer_scenes(l: &Layer) -> Vec<SceneId> {
    if !l.dirty {
        return Vec::new();
    }
    let loaded = l.scene.loaded_scenes();
    if let [only] = loaded[..] {
        return vec![only];
    }
    loaded
        .into_iter()
        .filter(|id| l.dirty_list.contains(id))
        .collect()
}

/// Every open document with unsaved changes. Cheap: nothing is serialised here.
pub fn dirty_docs(s: &EditorState) -> Vec<DocRef> {
    let mut out = Vec::new();
    for (i, l) in layers(s).iter().enumerate() {
        match l.prefab {
            Some(_) if l.dirty => out.push(DocRef::Prefab { layer: i }),
            Some(_) => {}
            None => out.extend(
                layer_scenes(l)
                    .into_iter()
                    .map(|id| DocRef::Scene { layer: i, id }),
            ),
        }
    }
    if s.script_graph.dirty() {
        out.push(DocRef::ScriptGraph);
    }
    if s.shader_graph.dirty() {
        out.push(DocRef::ShaderGraph);
    }
    if s.effect_graph.dirty() {
        out.push(DocRef::EffectGraph);
    }
    for d in &s.code_editor.docs {
        if d.dirty() {
            out.push(DocRef::Script(d.path.clone()));
        }
    }
    // Runs pop from the end: the open scenes first.
    out.reverse();
    out
}

/// A snapshot under way: done now, or a scene copy being serialised on a worker thread.
pub enum Started {
    Done(Option<DocSnapshot>),
    Worker(std::sync::mpsc::Receiver<DocSnapshot>),
}

/// Start one document's snapshot. A scene is copied on the main thread (a few times cheaper
/// than writing its JSON: 1.2 ms against 9.5 ms for NoonValley) and serialised on a worker;
/// the rest is serialised here (small documents).
pub fn start_snapshot(s: &mut EditorState, doc: &DocRef) -> Started {
    let DocRef::Scene { layer, id } = doc else {
        return Started::Done(snapshot(s, doc));
    };
    let live = *layer == 0 && shape(s) == (0, false);
    if live && s.scene.entry(*id).is_some_and(|e| e.loaded) {
        crate::prefabs::record_overrides(&mut s.scene, &mut s.assets, Some(*id));
    }
    let copy = {
        let ls = layers(s);
        match ls.get(*layer) {
            Some(l) if l.prefab.is_none() && l.scene.entry(*id).is_some_and(|e| e.loaded) => {
                l.scene.clone()
            }
            _ => return Started::Done(None),
        }
    };
    let id = *id;
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("misty autosave snapshot".into())
        .spawn(move || {
            let _ = tx.send(scene_snapshot(&copy, id));
        });
    match spawned {
        Ok(_) => Started::Worker(rx),
        // No thread: serialise here.
        Err(_) => Started::Done(snapshot(s, doc)),
    }
}

/// Serialise one document on the main thread (the panic path; small documents). `None` when it
/// is gone.
pub fn snapshot(s: &mut EditorState, doc: &DocRef) -> Option<DocSnapshot> {
    match doc {
        DocRef::Scene { layer, id } => {
            // Editing scenes: each prefab instance's override list goes in, as File ▸ Save
            // does (without it, a restored instance would lose its overrides to the prefab).
            let live = *layer == 0 && shape(s) == (0, false);
            if live && s.scene.entry(*id).is_some_and(|e| e.loaded) {
                crate::prefabs::record_overrides(&mut s.scene, &mut s.assets, Some(*id));
            }
            let ls = layers(s);
            let l = ls.get(*layer)?;
            if l.prefab.is_some() || !l.scene.entry(*id).is_some_and(|e| e.loaded) {
                return None;
            }
            Some(scene_snapshot(l.scene, *id))
        }
        DocRef::Prefab { layer } => {
            let ls = layers(s);
            let l = ls.get(*layer)?;
            let (path, root) = l.prefab?;
            l.scene
                .contains(root)
                .then(|| prefab_snapshot(l.scene, path, root))
        }
        DocRef::ScriptGraph => {
            let w = &s.script_graph;
            let path = w.path.clone()?;
            Some(DocSnapshot::new(
                DocKind::ScriptGraph,
                Some(path.clone()),
                file_name(&path),
                w.graph.to_json(),
            ))
        }
        DocRef::ShaderGraph => {
            let w = &s.shader_graph;
            let path = w.path.clone()?;
            Some(DocSnapshot::new(
                DocKind::ShaderGraph,
                Some(path.clone()),
                file_name(&path),
                w.graph.to_json(),
            ))
        }
        DocRef::EffectGraph => {
            let w = &s.effect_graph;
            let path = w.path.clone()?;
            Some(DocSnapshot::new(
                DocKind::EffectGraph,
                Some(path.clone()),
                file_name(&path),
                w.graph.to_json(),
            ))
        }
        DocRef::Script(path) => {
            let d = s.code_editor.docs.iter().find(|d| &d.path == path)?;
            Some(DocSnapshot::new(
                DocKind::Script,
                Some(path.clone()),
                file_name(path),
                d.editor.text(),
            ))
        }
    }
}

/// Scene `id` as its `.scene` file would hold it.
pub fn scene_snapshot(scene: &Scene, id: SceneId) -> DocSnapshot {
    let body = misty_engine::asset_db::embed_json(&scene.scene_to_json(id));
    DocSnapshot::new(
        DocKind::Scene,
        scene.scene_path(id),
        scene.scene_name(id),
        body,
    )
}

/// The prefab at `root` of a Prefab Mode stage, as its `.prefab` file would hold it.
pub fn prefab_snapshot(scene: &Scene, path: &str, root: Entity) -> DocSnapshot {
    let prefab = misty_engine::scene::prefab::Prefab::from_scene_keep_base(scene, root).with_ids();
    let body = misty_engine::asset_db::embed_json(&prefab.to_json());
    DocSnapshot::new(
        DocKind::Prefab,
        Some(path.to_owned()),
        crate::prefabs::display_name(path),
        body,
    )
}

/// "Assets/Graphs/Door.scriptgraph" → "Door.scriptgraph".
pub fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}
