//! The restore file a restart leaves for the new editor process: project, open scenes and the
//! active one, Scene view camera, selection, expanded Hierarchy rows, dock layout and the open
//! tabs (Code Editor scripts, the Project window folder, the Inspector's asset). Written to
//! `<project>/Library/RestoreSession.json`; `misty --restore <file>` opens the project and puts
//! it all back, then deletes the file.
//!
//! Unsaved scenes are saved or discarded first (the usual prompt), so the scenes come back from
//! their files; objects are matched by place (see `paths`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use egui_dock::DockState;
use serde::{Deserialize, Serialize};

use super::paths::{self, ObjectRef};
use crate::layout::Tab;
use crate::scene_camera::CameraState;
use crate::scene_files::OpenSceneRecord;
use crate::state::EditorState;

/// Where the file goes, inside the project.
pub const FILE: &str = "Library/RestoreSession.json";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RestoreFile {
    pub project: PathBuf,
    pub open_scenes: Vec<OpenSceneRecord>,
    pub active_scene: Option<String>,
    pub camera: Option<CameraState>,
    /// The Scene view's 2D toggle.
    pub scene_view_2d: bool,
    pub selection: Vec<ObjectRef>,
    pub active_object: Option<ObjectRef>,
    pub expanded: Vec<ObjectRef>,
    /// Scene headers folded closed in the Hierarchy (by scene file).
    pub collapsed_scenes: Vec<String>,
    /// The dock arrangement (egui_dock's JSON) and the layout's name.
    pub dock: Option<serde_json::Value>,
    pub layout_name: String,
    /// Scripts open in the Code Editor and the one in front.
    pub scripts: Vec<String>,
    pub active_script: usize,
    /// Project window folder and the asset shown in the Inspector.
    pub project_folder: Option<String>,
    pub inspector_asset: Option<String>,
}

/// Everything to put back, from the running editor.
pub fn capture(s: &EditorState, dock: &DockState<Tab>) -> RestoreFile {
    let scene = &s.scene;
    let refs = |list: &mut dyn Iterator<Item = &misty_engine::hecs::Entity>| -> Vec<ObjectRef> {
        list.filter(|e| scene.contains(**e))
            .map(|e| paths::object_ref(scene, *e))
            .collect()
    };
    let mut expanded = refs(&mut s.hierarchy.expanded.iter());
    expanded.sort_by(|a, b| a.path.cmp(&b.path));
    RestoreFile {
        project: s.project.root.clone(),
        open_scenes: scene
            .scenes()
            .iter()
            .filter_map(|e| {
                Some(OpenSceneRecord {
                    path: e.path.clone()?,
                    loaded: e.loaded,
                })
            })
            .collect(),
        active_scene: scene.active_path(),
        camera: Some(s.scene_view.camera.state()),
        scene_view_2d: s.scene_view.is_2d,
        selection: refs(&mut s.selection.list().iter()),
        active_object: s
            .selection
            .active()
            .filter(|e| scene.contains(*e))
            .map(|e| paths::object_ref(scene, e)),
        expanded,
        collapsed_scenes: s
            .hierarchy
            .collapsed_scenes
            .iter()
            .filter_map(|id| scene.scene_path(*id))
            .collect(),
        dock: serde_json::to_value(crate::layouts::normalized(dock)).ok(),
        layout_name: s.layouts.current_name.clone(),
        scripts: s.code_editor.docs.iter().map(|d| d.path.clone()).collect(),
        active_script: s.code_editor.active,
        project_folder: Some(crate::scene_files::relative(
            &s.project.root,
            &s.project.current,
        )),
        inspector_asset: s.inspector_asset.clone(),
    }
}

/// Write the restore file for the running editor; returns its path.
pub fn write(s: &EditorState, dock: &DockState<Tab>) -> Result<PathBuf, String> {
    let path = s.project.root.join(FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&capture(s, dock)).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn read(path: &Path) -> Result<RestoreFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// `--restore <file>` from the command line, until the editor has opened.
static PENDING: Mutex<Option<(PathBuf, RestoreFile)>> = Mutex::new(None);

/// `misty --restore <file>`: read it and return the project to open.
pub fn set_pending(path: &Path) -> Result<PathBuf, String> {
    let file = read(path)?;
    let project = file.project.clone();
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((path.to_path_buf(), file));
    Ok(project)
}

/// The restore file waiting for `project` (taken once; the file is deleted).
pub fn take_pending(project: &Path) -> Option<RestoreFile> {
    let (path, file) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    let _ = std::fs::remove_file(&path);
    (file.project == project).then_some(file)
}

/// Put a restore file back into a freshly opened editor.
pub fn apply(s: &mut EditorState, dock: &mut DockState<Tab>, r: RestoreFile) {
    // Scenes: normally the remembered open set already matches (it is kept up to date in the
    // project's editor settings); open the restore file's set when it doesn't.
    let current: Vec<OpenSceneRecord> = s
        .scene
        .scenes()
        .iter()
        .filter_map(|e| {
            Some(OpenSceneRecord {
                path: e.path.clone()?,
                loaded: e.loaded,
            })
        })
        .collect();
    if !r.open_scenes.is_empty() && current != r.open_scenes {
        s.files.settings.open_scenes = r.open_scenes.clone();
        s.files.settings.active_scene = r.active_scene.clone();
        crate::open_scenes::restore_open_set(s);
    }
    if let Some(id) = r
        .active_scene
        .as_deref()
        .and_then(|a| s.scene.find_loaded(a))
    {
        crate::open_scenes::set_active(s, id);
    }
    if let Some(cam) = &r.camera {
        s.scene_view.camera.set_state(cam);
    }
    s.scene_view.is_2d = r.scene_view_2d;
    let mut list: Vec<_> = r
        .selection
        .iter()
        .filter_map(|o| paths::find(&s.scene, o))
        .collect();
    if let Some(a) = r
        .active_object
        .as_ref()
        .and_then(|o| paths::find(&s.scene, o))
    {
        list.retain(|e| *e != a);
        list.push(a);
    }
    s.selection.set_many(list);
    s.hierarchy.expanded = r
        .expanded
        .iter()
        .filter_map(|o| paths::find(&s.scene, o))
        .collect();
    s.hierarchy.collapsed_scenes = r
        .collapsed_scenes
        .iter()
        .filter_map(|p| s.scene.find_by_path(p))
        .collect();
    if let Some(d) = r
        .dock
        .and_then(|v| serde_json::from_value::<DockState<Tab>>(v).ok())
        .filter(|d| d.iter_all_tabs().next().is_some())
    {
        *dock = d;
    }
    if !r.layout_name.is_empty() {
        s.layouts.current_name = r.layout_name;
    }
    for script in &r.scripts {
        let _ = s.code_editor.open(script, None);
    }
    if r.active_script < s.code_editor.docs.len() {
        s.code_editor.active = r.active_script;
    }
    if let Some(folder) = r.project_folder {
        let dir = s.project.root.join(folder);
        if dir.is_dir() {
            s.project.open_folder(dir);
        }
    }
    if let Some(asset) = r.inspector_asset
        && s.project.root.join(&asset).exists()
    {
        s.inspector_asset = Some(asset);
    }
    s.console
        .info("Restored the session from before the restart.");
}
