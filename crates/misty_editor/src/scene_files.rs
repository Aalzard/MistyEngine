//! Scene files in the editor: New / Open / Save / Save As, recent scenes, the
//! "Scene Has Been Modified" prompt, and per-project editor settings.
//! Several scenes can be open at once; see `open_scenes` for additive loading and the open set.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use misty_engine::scene::file::EXTENSION;
use misty_engine::scene::{Scene, SceneId};
use serde::{Deserialize, Serialize};

use crate::i18n::tr;
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
    /// The Scene view was in 2D mode, showing this part of the XY plane (`scene_view` is then
    /// the 3D view that turning 2D off returns to).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_view_2d: Option<SavedView2D>,
    /// Scene view icon gizmo size and tint.
    pub scene_icons: crate::scene_icons::IconSettings,
    /// Scene camera settings (field of view, clipping, speed, easing).
    pub scene_camera: crate::scene_camera::CameraSettings,
}

/// A Scene view eye position and rotation (quaternion x, y, z, w).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// A Scene view in 2D mode: the point in the middle (x, y) and half the view's height in world
/// units (like an orthographic camera's size).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedView2D {
    pub center: [f32; 2],
    pub size: f32,
}

/// Open the Scene view where it was when the project closed (in 2D mode if it was; a 2D
/// template starts that way); a project that never saved one (a new 3D project) starts
/// looking through its Main Camera.
pub fn restore_scene_view(s: &mut EditorState) {
    let settings = EditorSettings::load(&s.project.root);
    s.scene_view.icons = settings.scene_icons;
    s.scene_view.camera.settings = settings.scene_camera;
    let view = match settings.scene_view {
        Some(v) => Some((
            glam::Vec3::from(v.position),
            glam::Quat::from_array(v.rotation).normalize(),
        )),
        None => s.scene.find_by_name("Main Camera").map(|cam| {
            let (_, r, p) = s.scene.world_matrix(cam).to_scale_rotation_translation();
            (p, r)
        }),
    };
    if let Some((position, rotation)) = view
        && position.is_finite()
        && rotation.is_finite()
    {
        s.scene_view.camera.set_view(position, rotation);
    }
    if let Some(v) = settings.scene_view_2d {
        let [x, y] = v.center;
        s.scene_view
            .camera
            .set_2d_view(glam::Vec3::new(x, y, 0.0), v.size);
        s.scene_view.is_2d = s.scene_view.camera.is_2d();
    }
    keep_view_above_ground(s);
}

/// How far above a terrain the Scene view eye must be to count as "above ground".
const GROUND_CLEARANCE: f32 = 0.3;

/// Terrain surface height (world y) under a world point, if the point is over a terrain.
fn ground_under(scene: &Scene, p: glam::Vec3) -> Option<f32> {
    use misty_engine::terrain;
    terrain::terrains(scene)
        .into_iter()
        .filter_map(|(e, _)| {
            let d = terrain::data_of(scene, e)?;
            let origin = scene.world_position(e);
            let (lo, hi) = d.bounds();
            let local = p - origin;
            let inside = local.x >= lo.x && local.x <= hi.x && local.z >= lo.z && local.z <= hi.z;
            inside.then(|| origin.y + d.sample_height(local.x, local.z))
        })
        .reduce(f32::max)
}

/// The camera a scene is seen through: an active, enabled Camera tagged MainCamera, else the
/// first active enabled Camera, else the object named "Main Camera".
fn scene_camera_view(scene: &Scene) -> Option<(glam::Vec3, glam::Quat)> {
    use misty_engine::scene::components::Camera;
    let mut tagged = None;
    let mut first = None;
    for (e, _) in scene.walk() {
        let Ok(cam) = scene.world.get::<&Camera>(e) else {
            continue;
        };
        if !cam.enabled || !scene.active_in_hierarchy(e) {
            continue;
        }
        if tagged.is_none() && scene.game_object(e).tag == "MainCamera" {
            tagged = Some(e);
        }
        first.get_or_insert(e);
    }
    let e = tagged
        .or(first)
        .or_else(|| scene.find_by_name("Main Camera"))?;
    let (_, r, p) = scene.world_matrix(e).to_scale_rotation_translation();
    (p.is_finite() && r.is_finite()).then_some((p, r))
}

/// After opening a scene (or the project), a Scene view eye left under a terrain (the
/// previous scene's view, or a new project's first view) moves to where the scene is seen
/// from: its camera, or straight up above the ground keeping the direction.
pub(crate) fn keep_view_above_ground(s: &mut EditorState) {
    if s.scene_view.camera.is_2d() {
        return;
    }
    let eye = s.scene_view.camera.position();
    let Some(ground) = ground_under(&s.scene, eye) else {
        return;
    };
    if eye.y >= ground + GROUND_CLEARANCE {
        return;
    }
    if let Some((p, r)) = scene_camera_view(&s.scene) {
        let clear = ground_under(&s.scene, p).is_none_or(|g| p.y >= g + GROUND_CLEARANCE);
        if clear {
            s.scene_view.camera.set_view(p, r);
            return;
        }
    }
    let rotation = s.scene_view.camera.rotation();
    s.scene_view
        .camera
        .set_view(glam::Vec3::new(eye.x, ground + 2.0, eye.z), rotation);
}

/// Remember the Scene view camera (a 3D view, or the 2D view when in 2D mode), its settings
/// and the icon settings for the next launch.
pub fn remember_scene_view(s: &EditorState) {
    let cam = &s.scene_view.camera;
    let mut settings = EditorSettings::load(&s.project.root);
    settings.scene_icons = s.scene_view.icons;
    settings.scene_camera = cam.settings;
    if cam.is_2d() {
        let (center, size) = cam.view_2d();
        settings.scene_view_2d = Some(SavedView2D {
            center: [center.x, center.y],
            size,
        });
    } else {
        settings.scene_view_2d = None;
        settings.scene_view = Some(SavedView {
            position: cam.position().to_array(),
            rotation: cam.rotation().to_array(),
        });
    }
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
    /// File ▸ Reload Scene: load every open scene from disk again.
    ReloadScenes,
    /// Restart into a newer build of the editor (dev builds, see `live_reload::dev_build`).
    Restart,
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

/// After one scene was reloaded or saved: the editor is clean again unless another loaded scene
/// still differs from its file.
pub(crate) fn recheck_dirty(s: &mut EditorState) {
    let scene = &s.scene;
    let saved = &s.files.saved;
    let changed = scene
        .loaded_scenes()
        .into_iter()
        .any(|id| saved.get(&id) != Some(&hash(&scene.scene_to_json(id))));
    s.scene_dirty = s.scene_dirty && changed;
    s.files.dirty = dirty_now(s);
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
        | AfterPrompt::InstallUpdate
        | AfterPrompt::ReloadScenes
        | AfterPrompt::Restart => dirty,
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
        AfterPrompt::ReloadScenes => crate::live_reload::scenes::reload_all(s),
        AfterPrompt::Restart => s.live.restart_now = true,
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
    // World Partition cells of the old scene are forgotten.
    s.world_partition.streamer.reset();
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
            crate::prefabs::catch_up(s, None);
            keep_view_above_ground(s);
            for w in report.warnings {
                s.console.warn(format!("{rel}: {w}"));
            }
            remember(s, rel);
            crate::live_reload::scenes::note_disk(s, rel);
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
    // Each prefab instance's overrides go into the file, so it can follow its prefab later.
    crate::prefabs::record_overrides(&mut s.scene, &mut s.assets, Some(id));
    match s.scene.save_scene(id, &path) {
        Ok(()) => {
            mark_saved(s, id);
            // Clean once no other loaded scene has changes left.
            let one = s.scene.loaded_scenes().len() == 1;
            if one || dirty_now(s).is_empty() {
                s.scene_dirty = false;
            }
            remember(s, rel);
            crate::live_reload::scenes::note_disk(s, rel);
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
    let (project, scene) = title_parts(s);
    format!("{project} - {scene} - Misty Engine")
}

/// The project folder's name and the scene name (with `*` while anything is unsaved), for the
/// taskbar title and the title bar.
pub fn title_parts(s: &EditorState) -> (String, String) {
    // A root like `SampleProject\.` or `.` has no file name of its own: use the real folder's.
    let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    let root = &s.project.root;
    let project = name(root)
        .filter(|n| !n.is_empty() && n != ".")
        .or_else(|| root.canonicalize().ok().and_then(|p| name(&p)))
        .unwrap_or_default();
    let dirty = if s.files.dirty.is_empty() { "" } else { "*" };
    (project, format!("{}{dirty}", s.scene.name))
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
            tr!("Scenes Have Been Modified")
        } else {
            tr!("Scene Has Been Modified")
        });
        ui.add_space(6.0);
        let names = names.join(", ");
        ui.label(if several {
            tr!(
                "Do you want to save the changes you made in the scenes {names}?",
                names = names
            )
        } else {
            tr!(
                "Do you want to save the changes you made in the scene {names}?",
                names = names
            )
        });
        ui.label(
            egui::RichText::new(tr!("Your changes will be lost if you don't save them.")).weak(),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(tr!("Save")).clicked() {
                if scenes.iter().all(|id| save_scene(s, *id)) {
                    result = Some(pending.clone());
                }
                close = true;
            }
            if ui.button(tr!("Don't Save")).clicked() {
                result = Some(pending.clone());
                close = true;
            }
            if ui.button(tr!("Cancel")).clicked() {
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

    /// Opening MeadowDemo with the Scene view left under its terrain (a new project's first
    /// view, or the last scene's) moves the eye above the ground.
    #[test]
    fn opening_a_scene_lifts_a_view_left_under_the_terrain() {
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let mut s = EditorState::new(ProjectBrowser::open(sample.clone()));
        // The terrain is found through the process-wide project root, which tests running
        // alongside move: try again (a few times) only when that happened meanwhile.
        let mut tries = 0;
        let (eye, ground) = loop {
            tries += 1;
            misty_engine::assets::set_project_root(&sample);
            let generation = misty_engine::assets::root_generation();
            s.scene_view
                .camera
                .set_view(glam::Vec3::new(0.0, -50.0, 0.0), glam::Quat::IDENTITY);
            assert!(load(&mut s, "Assets/Scenes/MeadowDemo.scene"));
            let eye = s.scene_view.camera.position();
            let ground = ground_under(&s.scene, eye);
            let moved = misty_engine::assets::root_generation() != generation;
            let lifted = ground.is_some_and(|g| eye.y >= g + GROUND_CLEARANCE);
            if !lifted && moved && tries < 20 {
                continue;
            }
            break (eye, ground.expect("the eye is over the meadow terrain"));
        };
        assert!(
            eye.y >= ground + GROUND_CLEARANCE,
            "eye {eye} ground {ground}"
        );
        // A view already above ground is left alone.
        let above = glam::Vec3::new(eye.x, ground + 25.0, eye.z);
        s.scene_view.camera.set_view(above, glam::Quat::IDENTITY);
        keep_view_above_ground(&mut s);
        assert!((s.scene_view.camera.position() - above).length() < 1e-3);
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
