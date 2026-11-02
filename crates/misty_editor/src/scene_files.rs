//! Scene files in the editor: New / Open / Save / Save As, recent scenes, the
//! "Scene Has Been Modified" prompt, and per-project editor settings.

use std::path::{Path, PathBuf};

use misty_engine::scene::Scene;
use misty_engine::scene::file::EXTENSION;
use serde::{Deserialize, Serialize};

use crate::state::EditorState;

/// Per-user editor state (not meant for version control, like a Library folder).
const SETTINGS_FILE: &str = "Library/EditorSettings.json";
const MAX_RECENT: usize = 8;

/// Editor settings stored with the project.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Project-relative scene paths, most recent first.
    pub recent_scenes: Vec<String>,
}

impl EditorSettings {
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(SETTINGS_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self, root: &Path) {
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
}

/// Scene file state held by the editor.
#[derive(Default)]
pub struct SceneFiles {
    /// Project-relative path of the open scene; `None` = never saved.
    pub path: Option<String>,
    pub settings: EditorSettings,
    /// Waiting for Save / Don't Save / Cancel.
    pub prompt: Option<AfterPrompt>,
    /// The user confirmed quitting: let the window close.
    pub quit_confirmed: bool,
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn remember(s: &mut EditorState, rel: &str) {
    let recent = &mut s.files.settings.recent_scenes;
    recent.retain(|r| r != rel);
    recent.insert(0, rel.to_owned());
    recent.truncate(MAX_RECENT);
    s.files.settings.save(&s.project.root);
}

/// On startup: reopen the last scene, else the sample scene, else keep the default one.
pub fn open_initial(s: &mut EditorState) {
    s.files.settings = EditorSettings::load(&s.project.root);
    let candidates: Vec<String> = s
        .files
        .settings
        .recent_scenes
        .iter()
        .cloned()
        .chain(std::iter::once(
            "Assets/Scenes/SampleScene.scene".to_owned(),
        ))
        .collect();
    for rel in candidates {
        if s.project.root.join(&rel).is_file() && load(s, &rel) {
            return;
        }
    }
}

/// Run `then` now, or ask first if the scene has unsaved changes.
pub fn guard(s: &mut EditorState, then: AfterPrompt) -> Option<AfterPrompt> {
    if s.play.in_play_mode() {
        s.console.warn("Stop Play mode first.");
        return None;
    }
    if s.scene_dirty {
        s.files.prompt = Some(then);
        None
    } else {
        Some(then)
    }
}

/// Carry out a confirmed action. Returns true if the editor should quit.
pub fn perform(s: &mut EditorState, action: AfterPrompt) -> bool {
    match action {
        AfterPrompt::NewScene => {
            s.scene = Scene::default_3d("Untitled");
            s.files.path = None;
            reset_after_load(s);
            s.console.info("Created new scene.");
            false
        }
        AfterPrompt::Open(rel) => {
            load(s, &rel);
            false
        }
        AfterPrompt::Quit => {
            s.files.quit_confirmed = true;
            true
        }
    }
}

fn reset_after_load(s: &mut EditorState) {
    s.selection.clear();
    s.undo = Default::default();
    s.scene_dirty = false;
    s.hierarchy.expanded.clear();
    s.inspector_targets.clear();
}

/// Load a scene file into the editor. Returns false (and logs) on failure.
pub fn load(s: &mut EditorState, rel: &str) -> bool {
    let path = s.project.root.join(rel);
    match Scene::load(&path) {
        Ok((scene, report)) => {
            s.scene = scene;
            s.files.path = Some(rel.to_owned());
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

/// Save to the current path, or ask where (Save As) the first time. Returns true when saved.
pub fn save(s: &mut EditorState) -> bool {
    if s.play.in_play_mode() {
        s.console.warn("Scenes can't be saved in Play mode.");
        return false;
    }
    match s.files.path.clone() {
        Some(rel) => write(s, &rel),
        None => save_as(s),
    }
}

/// Ask for a file name under Assets and save there.
pub fn save_as(s: &mut EditorState) -> bool {
    if s.play.in_play_mode() {
        s.console.warn("Scenes can't be saved in Play mode.");
        return false;
    }
    let scenes_dir = s.project.root.join("Assets").join("Scenes");
    let _ = std::fs::create_dir_all(&scenes_dir);
    let picked = rfd::FileDialog::new()
        .set_title("Save Scene")
        .set_directory(&scenes_dir)
        .set_file_name(format!("{}.{EXTENSION}", s.scene.name))
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
    write(s, &rel)
}

fn with_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none_or(|e| e != EXTENSION) {
        path.set_extension(EXTENSION);
    }
    path
}

fn write(s: &mut EditorState, rel: &str) -> bool {
    let path = s.project.root.join(rel);
    if let Some(stem) = path.file_stem() {
        s.scene.name = stem.to_string_lossy().into_owned();
    }
    match s.scene.save(&path) {
        Ok(()) => {
            s.files.path = Some(rel.to_owned());
            s.scene_dirty = false;
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

/// Window title: "Project - Scene* - Misty Engine".
pub fn window_title(s: &EditorState) -> String {
    let project = s
        .project
        .root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dirty = if s.scene_dirty { "*" } else { "" };
    format!("{project} - {}{dirty} - Misty Engine", s.scene.name)
}

/// The modal "Scene Has Been Modified" prompt. Returns the action to carry out, if any.
pub fn prompt_ui(ctx: &egui::Context, s: &mut EditorState) -> Option<AfterPrompt> {
    let pending = s.files.prompt.clone()?;
    let mut result = None;
    let mut close = false;
    egui::Modal::new(egui::Id::new("scene_modified_prompt")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading("Scene Has Been Modified");
        ui.add_space(6.0);
        ui.label(format!(
            "Do you want to save the changes you made in the scene \"{}\"?",
            s.scene.name
        ));
        ui.label(egui::RichText::new("Your changes will be lost if you don't save them.").weak());
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                if save(s) {
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
        assert!(write(&mut s, "Assets/Scenes/Level.scene"));
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
            again.files.path.as_deref(),
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

    #[test]
    fn new_scene_asset_and_title() {
        let mut s = state("asset");
        let folder = s.project.root.join("Assets");
        let rel = create_scene_asset(&mut s, &folder).unwrap();
        assert_eq!(rel, "Assets/New Scene.scene");
        assert!(load(&mut s, &rel));
        s.scene_dirty = true;
        assert!(window_title(&s).contains("New Scene* - Misty Engine"));
    }
}
