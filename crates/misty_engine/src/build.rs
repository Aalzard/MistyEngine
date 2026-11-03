//! Build Settings: the scenes a built game contains (in order) and how its window opens.
//!
//! Stored in `<Project>/ProjectSettings/BuildSettings.json` next to the Project Settings.
//! Scene 0 of the enabled list is the one the player starts with.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Project-relative location of the build settings file.
pub const BUILD_SETTINGS_FILE: &str = "ProjectSettings/BuildSettings.json";

/// One row of the "Scenes In Build" list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildScene {
    /// Project-relative path ("Assets/Scenes/Level1.scene").
    pub path: String,
    /// Unticked scenes stay in the list but aren't built and get no index.
    pub enabled: bool,
}

impl Default for BuildScene {
    fn default() -> Self {
        Self {
            path: String::new(),
            enabled: true,
        }
    }
}

impl BuildScene {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            enabled: true,
        }
    }

    /// Scene name: the file name without extension ("Level1").
    pub fn name(&self) -> &str {
        let file = self.path.rsplit('/').next().unwrap_or(&self.path);
        file.rsplit_once('.').map_or(file, |(stem, _)| stem)
    }
}

/// A scene asked for by a script: `SceneManager.LoadScene("Level1")` or `LoadScene(1)`.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneRef {
    /// Scene name or project-relative path.
    Name(String),
    /// Build index (position among the enabled scenes).
    Index(i64),
}

impl std::fmt::Display for SceneRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneRef::Name(n) => write!(f, "'{n}'"),
            SceneRef::Index(i) => write!(f, "build index {i}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildSettings {
    pub scenes: Vec<BuildScene>,
    /// Window title and executable name; empty = the project folder's name.
    pub product_name: String,
    /// Windowed size in pixels.
    pub width: u32,
    pub height: u32,
    /// Start in borderless fullscreen.
    pub fullscreen: bool,
}

impl Default for BuildSettings {
    fn default() -> Self {
        Self {
            scenes: Vec::new(),
            product_name: String::new(),
            width: 1280,
            height: 720,
            fullscreen: false,
        }
    }
}

impl BuildSettings {
    pub fn path(root: &Path) -> PathBuf {
        root.join(BUILD_SETTINGS_FILE)
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut s: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        s.sanitize();
        Ok(s)
    }

    /// Load a project's build settings. No file = defaults; an unreadable file is an error.
    pub fn load(root: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(Self::path(root)) {
            Ok(text) => Self::from_json(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = Self::path(root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())
    }

    /// Keep the window size usable.
    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(64, 16384);
        self.height = self.height.clamp(64, 16384);
    }

    /// Product name, or the project folder's name when none is set.
    pub fn product_name_or(&self, root: &Path) -> String {
        let name = self.product_name.trim();
        if !name.is_empty() {
            return name.to_owned();
        }
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Misty Game".into())
    }

    /// Scenes that are built, in build-index order.
    pub fn enabled(&self) -> impl Iterator<Item = &BuildScene> {
        self.scenes.iter().filter(|s| s.enabled)
    }

    /// Whether `path` is in the list (enabled or not).
    pub fn contains(&self, path: &str) -> bool {
        self.scenes.iter().any(|s| s.path == path)
    }

    /// Add a scene at the end unless it is already listed.
    pub fn add(&mut self, path: &str) -> bool {
        if self.contains(path) {
            return false;
        }
        self.scenes.push(BuildScene::new(path));
        true
    }

    /// Build index of `path` (position among the enabled scenes).
    pub fn index_of(&self, path: &str) -> Option<usize> {
        self.enabled().position(|s| s.path == path)
    }

    /// Project-relative path and build index of the scene a script asked for.
    /// Names match the file name ("Level1") or the full path, like the familiar API.
    pub fn resolve(&self, scene: &SceneRef) -> Result<(String, usize), String> {
        let found = match scene {
            SceneRef::Index(i) => usize::try_from(*i)
                .ok()
                .and_then(|i| self.enabled().nth(i).map(|s| (s, i))),
            SceneRef::Name(n) => {
                let n = n.trim_end_matches(".scene");
                self.enabled()
                    .enumerate()
                    .find(|(_, s)| s.name() == n || s.path.trim_end_matches(".scene") == n)
                    .map(|(i, s)| (s, i))
            }
        };
        found.map(|(s, i)| (s.path.clone(), i)).ok_or_else(|| {
            format!(
                "Scene {scene} couldn't be loaded because it has not been added to the build \
                 settings (File ▸ Build Settings…)"
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> BuildSettings {
        BuildSettings {
            scenes: vec![
                BuildScene::new("Assets/Scenes/Menu.scene"),
                BuildScene {
                    path: "Assets/Scenes/Unused.scene".into(),
                    enabled: false,
                },
                BuildScene::new("Assets/Levels/Level1.scene"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn round_trips_through_a_project_folder() {
        let root = std::env::temp_dir().join("misty_build_settings_round_trip");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            BuildSettings::load(&root).unwrap(),
            BuildSettings::default()
        );
        let mut s = settings();
        s.product_name = "Rolling Ball".into();
        s.width = 800;
        s.height = 600;
        s.fullscreen = true;
        s.save(&root).unwrap();
        assert!(root.join("ProjectSettings/BuildSettings.json").is_file());
        assert_eq!(BuildSettings::load(&root).unwrap(), s);
    }

    #[test]
    fn missing_fields_use_defaults_and_sizes_are_clamped() {
        let s = BuildSettings::from_json(r#"{ "width": 0, "scenes": [{ "path": "a.scene" }] }"#)
            .unwrap();
        assert_eq!(s.width, 64);
        assert_eq!(s.height, 720);
        assert!(s.scenes[0].enabled);
        assert!(BuildSettings::from_json("nope").is_err());
    }

    #[test]
    fn scenes_resolve_by_name_path_and_index() {
        let s = settings();
        let level = ("Assets/Levels/Level1.scene".to_owned(), 1);
        assert_eq!(
            s.resolve(&SceneRef::Name("Level1".into())),
            Ok(level.clone())
        );
        assert_eq!(
            s.resolve(&SceneRef::Name("Assets/Levels/Level1".into())),
            Ok(level.clone())
        );
        assert_eq!(s.resolve(&SceneRef::Index(1)), Ok(level));
        assert_eq!(
            s.resolve(&SceneRef::Index(0)).unwrap().0,
            "Assets/Scenes/Menu.scene"
        );
        // Disabled scenes have no index and can't be loaded.
        assert!(s.resolve(&SceneRef::Name("Unused".into())).is_err());
        assert!(s.resolve(&SceneRef::Index(2)).is_err());
        assert!(s.resolve(&SceneRef::Index(-1)).is_err());
        assert_eq!(s.index_of("Assets/Levels/Level1.scene"), Some(1));
    }

    #[test]
    fn product_name_defaults_to_the_project_folder() {
        let mut s = BuildSettings::default();
        assert_eq!(s.product_name_or(Path::new("/x/MyGame")), "MyGame");
        s.product_name = " Rolling Ball ".into();
        assert_eq!(s.product_name_or(Path::new("/x/MyGame")), "Rolling Ball");
    }

    #[test]
    fn add_skips_duplicates() {
        let mut s = BuildSettings::default();
        assert!(s.add("Assets/a.scene"));
        assert!(!s.add("Assets/a.scene"));
        assert_eq!(s.scenes.len(), 1);
        assert_eq!(s.scenes[0].name(), "a");
    }
}
