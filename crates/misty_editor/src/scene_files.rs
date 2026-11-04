//! Scene files in the editor: New / Open / Save / Save As, recent scenes, the
//! "Scene Has Been Modified" prompt, and per-project editor settings.
//! Several scenes can be open at once; see `open_scenes` for additive loading and the open set.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use misty_engine::scene::file::EXTENSION;
use misty_engine::scene::{Scene, SceneId};
use serde::{Deserialize, Serialize};

use crate::state::EditorState;

/// Per-user editor state (not meant for version control, like a Library folder).
const SETTINGS_FILE: &str = "Library/EditorSettings.json";
const MAX_RECENT: usize = 8;

/// One scene of the open set remembered between sessions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenSceneRecord {
    pub path: String,
    pub loaded: bool,
}

/// Editor settings stored with the project.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Project-relative scene paths, most recent first.
    pub recent_scenes: Vec<String>,
    /// Output folder of the last build (File ▸ Build And Run builds there again).
    pub last_build_folder: Option<PathBuf>,
    /// Scenes open in the Hierarchy (saved ones), restored on the next launch.
    pub open_scenes: Vec<OpenSceneRecord>,
    /// Path of the active scene among `open_scenes`.
    pub active_scene: Option<String>,
    /// Where the Scene view camera was when the project closed.
    pub scene_view: Option<SavedView>,
}

/// A Scene view eye position and rotation (quaternion x, y, z, w).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// Open the Scene view where it was when the project closed; a project that never saved one
/// (a new project) starts looking through its Main Camera.
pub fn restore_scene_view(s: &mut EditorState) {
    let (position, rotation) = match EditorSettings::load(&s.project.root).scene_view {
        Some(v) => (
            glam::Vec3::from(v.position),
            glam::Quat::from_array(v.rotation).normalize(),
        ),
        None => {
            let Some(cam) = s.scene.find_by_name("Main Camera") else {
                return;
            };
            let (_, r, p) = s.scene.world_matrix(cam).to_scale_rotation_translation();
            (p, r)
        }
    };
    if position.is_finite() && rotation.is_finite() {
        s.scene_view.camera.set_view(position, rotation);
    }
}

/// Remember the Scene view camera for the next launch (3D views only).
pub fn remember_scene_view(s: &EditorState) {
    let cam = &s.scene_view.camera;
    if cam.is_2d() {
        return;
    }
    let mut settings = EditorSettings::load(&s.project.root);
    settings.scene_view = Some(SavedView {
        position: cam.position().to_array(),
        rotation: cam.rotation().to_array(),
    });
    settings.save(&s.project.root);
}

impl EditorSettings {
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(SETTINGS_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) {
        let path = root.join(SETTINGS_FILE);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// What to do once the user has answered the "save changes?" prompt.
#[derive(Clone, Debug, PartialEq)]
pub enum AfterPrompt {
    NewScene,
    Open(String),
    Quit,
    /// Close the project and return to the Misty Hub (its New Project page when true).
    OpenHub(bool),
    /// Unload / remove one scene of the open set.
    Unload(SceneId),
    Remove(SceneId),
    /// Remove every scene but this one.
    CloseOthers(SceneId),
    /// Install the available update (the editor restarts).
    InstallUpdate,
}

/// Scene file state held by the editor.
#[derive(Default)]
pub struct SceneFiles {
    pub settings: EditorSettings,
    /// Waiting for Save / Don't Save / Cancel.
    pub prompt: Option<AfterPrompt>,
    /// The user confirmed quitting: let the window close.
    pub quit_confirmed: bool,
    /// Confirmed File ▸ Open Misty Hub: the launcher swaps the editor for the Hub (`Some(true)`
    /// = on the New Project page).
    pub open_hub: Option<bool>,
    /// Hash of each loaded scene's JSON when it was last loaded or saved.
    pub saved: HashMap<SceneId, u64>,
    /// Scenes with unsaved changes (refreshed each frame by [`refresh_dirty`]).
    pub dirty: Vec<SceneId>,
    /// When `dirty` was last worked out by comparing scene contents.
    dirty_checked: Option<std::time::Instant>,
}

pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn remember(s: &mut EditorState, rel: &str) {
    let recent = &mut s.files.settings.recent_scenes;
    recent.retain(|r| r != rel);
    recent.insert(0, rel.to_owned());
    recent.truncate(MAX_RECENT);
    crate::open_scenes::remember_open_set(s);
}

// ---- unsaved changes ---------------------------------------------------------------------------

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Remember `id`'s current contents as saved.
pub(crate) fn mark_saved(s: &mut EditorState, id: SceneId) {
    let h = hash(&s.scene.scene_to_json(id));
    s.files.saved.insert(id, h);
    s.files.dirty.retain(|d| *d != id);
}

/// Loaded scenes with unsaved changes. With one scene this is the editor's dirty flag;
/// with several, each scene's contents are compared with what was loaded or saved.
pub fn dirty_now(s: &EditorState) -> Vec<SceneId> {
    if !s.scene_dirty {
        return Vec::new();
    }
    let loaded = s.scene.loaded_scenes();
    if let [only] = loaded[..] {
        return vec![only];
    }
    loaded
        .into_iter()
        .filter(|id| s.files.saved.get(id) != Some(&hash(&s.scene.scene_to_json(*id))))
        .collect()
}

/// Update the dirty list (at most a few times a second with several scenes open).
pub fn refresh_dirty(s: &mut EditorState) {
    // Play mode changes are thrown away on Stop: keep showing the Edit-mode state.
    if s.play.in_play_mode() {
        return;
    }
    let several = s.scene.loaded_scenes().len() > 1;
    let fresh = s
        .files
        .dirty_checked
        .is_some_and(|t| t.elapsed().as_secs_f32() < 0.3);
    if several && s.scene_dirty && fresh {
        return;
    }
    s.files.dirty = dirty_now(s);
    s.files.dirty_checked = Some(std::time::Instant::now());
}

/// Scenes whose changes `action` would throw away.
fn at_risk(s: &EditorState, action: &AfterPrompt) -> Vec<SceneId> {
    let dirty = dirty_now(s);
    match action {
        AfterPrompt::NewScene
        | AfterPrompt::Open(_)
        | AfterPrompt::Quit
        | AfterPrompt::OpenHub(_)
        | AfterPrompt::InstallUpdate => dirty,
        AfterPrompt::Unload(id) | AfterPrompt::Remove(id) => {
            dirty.into_iter().filter(|d| d == id).collect()
        }
        AfterPrompt::CloseOthers(id) => dirty.into_iter().filter(|d| d != id).collect(),
    }
}

// ---- opening ----------------------------------------------------------------------------------

/// On startup: reopen the last open scenes, else the last scene, else the first Build Settings
/// scene, else the sample scene, else keep the default one.
pub fn open_initial(s: &mut EditorState) {
    s.files.settings = EditorSettings::load(&s.project.root);
    if crate::open_scenes::restore_open_set(s) {
        return;
    }
    let candidates: Vec<String> = s
        .files
        .settings
        .recent_scenes
        .iter()
        .cloned()
        // Then the first scene in Build Settings (a fresh project), then the sample scene.
        .chain(
            misty_engine::build::BuildSettings::load(&s.project.root)
                .ok()
                .and_then(|b| b.scenes.into_iter().find(|sc| sc.enabled))
                .map(|sc| sc.path),
        )
        .chain(std::iter::once(
            "Assets/Scenes/SampleScene.scene".to_owned(),
        ))
        .collect();
    for rel in candidates {
        if s.project.root.join(&rel).is_file() && load(s, &rel) {
            return;
        }
    }
    mark_saved(s, 0);
}

/// Run `then` now, or ask first if it would lose unsaved changes.
pub fn guard(s: &mut EditorState, then: AfterPrompt) -> Option<AfterPrompt> {
    if s.play.in_play_mode() {
        s.console.warn("Stop Play mode first.");
        return None;
    }
    if at_risk(s, &then).is_empty() {
        Some(then)
    } else {
        s.files.prompt = Some(then);
        None
    }
}

/// Carry out a confirmed action. Returns true if the editor should quit.
pub fn perform(s: &mut EditorState, action: AfterPrompt) -> bool {
    match action {
        AfterPrompt::NewScene => {
            s.scene = Scene::default_3d("Untitled");
            reset_after_load(s);
            crate::open_scenes::remember_open_set(s);
            s.console.info("Created new scene.");
        }
        AfterPrompt::Open(rel) => {
            load(s, &rel);
        }
        AfterPrompt::Quit => {
            s.files.quit_confirmed = true;
            return true;
        }
        AfterPrompt::OpenHub(new_project) => s.files.open_hub = Some(new_project),
        AfterPrompt::Unload(id) => crate::open_scenes::unload(s, id),
        AfterPrompt::Remove(id) => crate::open_scenes::remove(s, id),
        AfterPrompt::CloseOthers(id) => crate::open_scenes::close_others(s, id),
        AfterPrompt::InstallUpdate => s.updater.install(),
    }
    false
}

pub(crate) fn reset_after_load(s: &mut EditorState) {
    s.selection.clear();
    s.undo.clear();
    s.scene_dirty = false;
    s.hierarchy.expanded.clear();
    s.hierarchy.collapsed_scenes.clear();
    s.inspector_targets.clear();
    s.files.saved.clear();
    s.files.dirty.clear();
    for id in s.scene.loaded_scenes() {
        mark_saved(s, id);
    }
}

/// Load a scene file into the editor, replacing every open scene. Returns false (and logs)
/// on failure.
pub fn load(s: &mut EditorState, rel: &str) -> bool {
    let path = s.project.root.join(rel);
    match Scene::load(&path) {
        Ok((mut scene, report)) => {
            scene.set_scene_path(0, rel);
            s.scene = scene;
            reset_after_load(s);
            for w in report.warnings {
                s.console.warn(format!("{rel}: {w}"));
            }
            remember(s, rel);
            s.console.info(format!("Opened {rel}"));
            true
        }
        Err(e) => {
            s.console.log(
                crate::console::LogLevel::Error,
                format!("Could not open scene: {e}"),
            );
            false
        }
    }
}

// ---- saving ------------------------------------------------------------------------------------

/// File ▸ Save: save every scene with changes (the active one when nothing changed).
/// Untitled scenes ask where to go. Returns true when everything was saved.
pub fn save(s: &mut EditorState) -> bool {
    if s.play.in_play_mode() {
        s.console.warn("Scenes can't be saved in Play mode.");
        return false;
    }
    let mut list = dirty_now(s);
    if list.is_empty() {
        list.push(s.scene.active_scene());
    }
    list.into_iter().all(|id| save_scene(s, id))
}

/// Save one scene to its file, or ask where (Save As) the first time.
pub fn save_scene(s: &mut EditorState, id: SceneId) -> bool {
    if s.play.in_play_mode() {
        s.console.warn("Scenes can't be saved in Play mode.");
        return false;
    }
    match s.scene.scene_path(id) {
        Some(rel) => write(s, id, &rel),
        None => save_scene_as(s, id),
    }
}

/// File ▸ Save As: the active scene.
pub fn save_as(s: &mut EditorState) -> bool {
    save_scene_as(s, s.scene.active_scene())
}

/// Ask for a file name under Assets and save scene `id` there.
pub fn save_scene_as(s: &mut EditorState, id: SceneId) -> bool {
    if s.play.in_play_mode() {
        s.console.warn("Scenes can't be saved in Play mode.");
        return false;
    }
    let scenes_dir = s.project.root.join("Assets").join("Scenes");
    let _ = std::fs::create_dir_all(&scenes_dir);
    let picked = rfd::FileDialog::new()
        .set_title("Save Scene")
        .set_directory(&scenes_dir)
        .set_file_name(format!("{}.{EXTENSION}", s.scene.scene_name(id)))
        .add_filter("Scene", &[EXTENSION])
        .save_file();
    let Some(path) = picked else { return false };
    let path = with_extension(path);
    if !path.starts_with(s.project.root.join("Assets")) {
        s.console
            .warn("Scenes must be saved inside the project's Assets folder.");
        return false;
    }
    let rel = relative(&s.project.root, &path);
    write(s, id, &rel)
}

fn with_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none_or(|e| e != EXTENSION) {
        path.set_extension(EXTENSION);
    }
    path
}

/// Write scene `id` to `rel` (it becomes the scene's file).
pub(crate) fn write(s: &mut EditorState, id: SceneId, rel: &str) -> bool {
    if s.scene.find_by_path(rel).is_some_and(|other| other != id) {
        s.console
            .warn(format!("{rel} is already open as another scene."));
        return false;
    }
    let path = s.project.root.join(rel);
    s.scene.set_scene_path(id, rel);
    // References into other scenes can't be stored (like the familiar editor, they are lost).
    for w in s.scene.cross_scene_references(id) {
        s.console
            .warn(format!("Cross-scene reference not saved: {w}"));
    }
    match s.scene.save_scene(id, &path) {
        Ok(()) => {
            mark_saved(s, id);
            // Clean once no other loaded scene has changes left.
            let one = s.scene.loaded_scenes().len() == 1;
            if one || dirty_now(s).is_empty() {
                s.scene_dirty = false;
            }
            remember(s, rel);
            s.project.refresh();
            s.console.info(format!("Saved {rel}"));
            true
        }
        Err(e) => {
            s.console.log(
                crate::console::LogLevel::Error,
                format!("Could not save scene: {e}"),
            );
            false
        }
    }
}

/// File ▸ Open Scene: pick a `.scene` file inside the project.
pub fn pick_scene(s: &mut EditorState) -> Option<String> {
    let path = rfd::FileDialog::new()
        .set_title("Open Scene")
        .set_directory(s.project.root.join("Assets"))
        .add_filter("Scene", &[EXTENSION])
        .pick_file()?;
    if !path.starts_with(&s.project.root) {
        s.console.warn("Open scenes from inside the project.");
        return None;
    }
    Some(relative(&s.project.root, &path))
}

/// Assets ▸ Create ▸ Scene: a new default scene file in `folder`.
pub fn create_scene_asset(s: &mut EditorState, folder: &Path) -> Result<String, String> {
    let path = (0..)
        .map(|n| {
            folder.join(if n == 0 {
                format!("New Scene.{EXTENSION}")
            } else {
                format!("New Scene {n}.{EXTENSION}")
            })
        })
        .find(|p| !p.exists())
        .expect("a free name");
    let name = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Scene::default_3d(&name).save(&path)?;
    s.project.refresh();
    Ok(relative(&s.project.root, &path))
}

/// Window title: "Project - ActiveScene* - Misty Engine" (* = an open scene has changes).
pub fn window_title(s: &EditorState) -> String {
    let project = s
        .project
        .root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dirty = if s.files.dirty.is_empty() { "" } else { "*" };
    format!("{project} - {}{dirty} - Misty Engine", s.scene.name)
}

/// The modal "Scene Has Been Modified" prompt. Returns the action to carry out, if any.
pub fn prompt_ui(ctx: &egui::Context, s: &mut EditorState) -> Option<AfterPrompt> {
    let pending = s.files.prompt.clone()?;
    let scenes = at_risk(s, &pending);
    let names: Vec<String> = scenes
        .iter()
        .map(|id| format!("\"{}\"", s.scene.scene_name(*id)))
        .collect();
    let several = scenes.len() > 1;
    let mut result = None;
    let mut close = false;
    egui::Modal::new(egui::Id::new("scene_modified_prompt")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading(if several {
            "Scenes Have Been Modified"
        } else {
            "Scene Has Been Modified"
        });
        ui.add_space(6.0);
        let which = if several { "scenes" } else { "scene" };
        ui.label(format!(
            "Do you want to save the changes you made in the {which} {}?",
            names.join(", ")
        ));
        ui.label(egui::RichText::new("Your changes will be lost if you don't save them.").weak());
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                if scenes.iter().all(|id| save_scene(s, *id)) {
                    result = Some(pending.clone());
                }
                close = true;
            }
            if ui.button("Don't Save").clicked() {
                result = Some(pending.clone());
                close = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if close {
        s.files.prompt = None;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;

    /// Each test gets its own project folder (tests run in parallel).
    fn state(name: &str) -> EditorState {
        let root =
            std::env::temp_dir().join(format!("misty_scene_files_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        EditorState::new(ProjectBrowser::open(root))
    }

    #[test]
    fn save_then_reopen_restores_the_scene_and_recent_list() {
        let mut s = state("reopen");
        s.scene.spawn("Extra", None);
        s.scene_dirty = true;
        assert!(write(&mut s, 0, "Assets/Scenes/Level.scene"));
        assert!(!s.scene_dirty);
        assert_eq!(s.scene.name, "Level");

        // Change, then load the saved file back.
        s.scene.spawn("Unsaved", None);
        assert!(load(&mut s, "Assets/Scenes/Level.scene"));
        assert!(s.scene.find_by_name("Extra").is_some());
        assert!(s.scene.find_by_name("Unsaved").is_none());
        assert_eq!(
            s.files.settings.recent_scenes,
            ["Assets/Scenes/Level.scene"]
        );

        // A fresh editor on the same project reopens it.
        let mut again = EditorState::new(ProjectBrowser::open(s.project.root.clone()));
        open_initial(&mut again);
        assert_eq!(
            again.scene.active_path().as_deref(),
            Some("Assets/Scenes/Level.scene")
        );
    }

    #[test]
    fn dirty_scene_asks_before_replacing() {
        let mut s = state("dirty");
        assert_eq!(
            guard(&mut s, AfterPrompt::NewScene),
            Some(AfterPrompt::NewScene)
        );
        s.scene_dirty = true;
        assert_eq!(guard(&mut s, AfterPrompt::NewScene), None);
        assert_eq!(s.files.prompt, Some(AfterPrompt::NewScene));
    }

    /// File ▸ Open Misty Hub: straight away when nothing changed, else after the save prompt;
    /// the launcher picks the request up once.
    #[test]
    fn open_hub_asks_to_save_first() {
        let mut s = state("open_hub");
        let a = guard(&mut s, AfterPrompt::OpenHub(false)).expect("nothing to save");
        assert!(!perform(&mut s, a), "the window stays open");
        assert_eq!(s.files.open_hub.take(), Some(false));
        s.scene.spawn("Unsaved", None);
        s.scene_dirty = true;
        assert_eq!(guard(&mut s, AfterPrompt::OpenHub(true)), None);
        assert_eq!(s.files.prompt, Some(AfterPrompt::OpenHub(true)));
        assert_eq!(s.files.open_hub, None, "not before the prompt is answered");
        perform(&mut s, AfterPrompt::OpenHub(true));
        assert_eq!(s.files.open_hub, Some(true));
    }

    #[test]
    fn new_scene_asset_and_title() {
        let mut s = state("asset");
        let folder = s.project.root.join("Assets");
        let rel = create_scene_asset(&mut s, &folder).unwrap();
        assert_eq!(rel, "Assets/New Scene.scene");
        assert!(load(&mut s, &rel));
        s.scene_dirty = true;
        refresh_dirty(&mut s);
        assert!(window_title(&s).contains("New Scene* - Misty Engine"));
    }
}
