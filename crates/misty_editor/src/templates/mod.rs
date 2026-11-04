//! Project templates for the Hub's New Project page. Each one writes a complete project folder
//! generated in code: `Assets/`, `ProjectSettings/` (with Build Settings listing the starting
//! scene), the starting scene itself and `Library/EditorSettings.json` so it opens first.

mod scenes;
#[cfg(test)]
mod tests;

use std::path::Path;

use misty_engine::build::{BuildScene, BuildSettings};
use misty_engine::player_settings::PlayerSettings;
use misty_engine::settings::ProjectSettings;
use serde::{Deserialize, Serialize};

/// The starting scene every template writes (and lists in Build Settings).
pub const SCENE: &str = "Assets/Scenes/Main.scene";
/// Preview the Hub shows for projects it created (a copy of the template's thumbnail).
pub const PROJECT_THUMBNAIL: &str = "Library/HubThumbnail.png";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Template {
    #[default]
    Standard,
    /// Post-processing ready: the Standard platform at sunset over the ocean.
    PpReady,
}

impl Template {
    pub const ALL: [Self; 2] = [Self::Standard, Self::PpReady];

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::PpReady => "PP Ready",
        }
    }

    /// Case and spaces don't matter: `--new ppready` works too.
    pub fn from_name(name: &str) -> Option<Self> {
        let key = |n: &str| n.replace(' ', "").to_ascii_lowercase();
        Self::ALL.into_iter().find(|t| key(t.name()) == key(name))
    }

    /// The thumbnail file name in `assets/templates/` (for `write_template_thumbnails`).
    #[cfg(test)]
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::PpReady => "pp_ready",
        }
    }

    /// One line for the template card.
    pub fn description(self) -> &'static str {
        match self {
            Self::Standard => "A floating platform under a sunny sky, ready to build on.",
            Self::PpReady => {
                "The platform on the ocean at sunset: clouds, god rays, light fog, all effects on."
            }
        }
    }

    /// The thumbnail PNG (rendered headlessly by `write_template_thumbnails`).
    pub fn thumbnail_png(self) -> &'static [u8] {
        match self {
            Self::Standard => include_bytes!("../../assets/templates/standard.png"),
            Self::PpReady => include_bytes!("../../assets/templates/pp_ready.png"),
        }
    }

    /// Write the template's assets into `root` (an existing project folder) and return its scene.
    fn build(self, root: &Path) -> Result<misty_engine::scene::Scene, String> {
        scenes::write_materials(root)?;
        Ok(match self {
            Self::Standard => scenes::standard(),
            Self::PpReady => scenes::pp_ready(),
        })
    }
}

/// Create a new project from `template` at `root`, which must not exist yet (or be empty).
/// The folder name is the product name.
pub fn create(root: &Path, template: Template) -> Result<(), String> {
    if root.exists()
        && std::fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err(format!(
            "{} already exists and is not empty.",
            root.display()
        ));
    }
    for dir in [
        "Assets/Scenes",
        "Assets/Materials",
        "Assets/Scripts",
        "Library",
    ] {
        std::fs::create_dir_all(root.join(dir)).map_err(|e| e.to_string())?;
    }
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "New Project".into());
    let settings = ProjectSettings {
        player: Some(PlayerSettings {
            product_name: name.clone(),
            ..Default::default()
        }),
        ..Default::default()
    };
    settings.save(root)?;
    BuildSettings {
        scenes: vec![BuildScene {
            path: SCENE.into(),
            enabled: true,
        }],
        product_name: name,
        ..Default::default()
    }
    .save(root)?;

    let scene = template.build(root)?;
    scene.save(&root.join(SCENE))?;
    // The editor opens the most recent scene first.
    crate::scene_files::EditorSettings {
        recent_scenes: vec![SCENE.into()],
        ..Default::default()
    }
    .save(root);
    std::fs::write(root.join(PROJECT_THUMBNAIL), template.thumbnail_png())
        .map_err(|e| e.to_string())?;
    std::fs::write(root.join(".gitignore"), "Library/\nBuilds/\n").map_err(|e| e.to_string())
}
