//! Project templates for the Hub's New Project page. Each one writes a complete project folder
//! generated in code: `Assets/`, `ProjectSettings/` (with Build Settings listing the starting
//! scene), the starting scene itself and `Library/EditorSettings.json` so it opens first.

mod landscape;
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
    Empty,
    #[default]
    Standard,
    Ocean,
    Landscape,
}

impl Template {
    pub const ALL: [Self; 4] = [Self::Empty, Self::Standard, Self::Ocean, Self::Landscape];

    pub fn name(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Standard => "Standard",
            Self::Ocean => "Ocean",
            Self::Landscape => "Landscape",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(name.trim()))
    }

    /// One line for the template card.
    pub fn description(self) -> &'static str {
        match self {
            Self::Empty => "A camera and a light. Start from nothing.",
            Self::Standard => "A floating platform under a sunny sky, ready to build on.",
            Self::Ocean => "The platform in an open ocean under a few clouds, post-processing on.",
            Self::Landscape => {
                "Generated terrain under volumetric clouds, fog and sun shafts: everything on."
            }
        }
    }

    /// The thumbnail PNG (rendered headlessly by `write_template_thumbnails`).
    pub fn thumbnail_png(self) -> &'static [u8] {
        match self {
            Self::Empty => include_bytes!("../../assets/templates/empty.png"),
            Self::Standard => include_bytes!("../../assets/templates/standard.png"),
            Self::Ocean => include_bytes!("../../assets/templates/ocean.png"),
            Self::Landscape => include_bytes!("../../assets/templates/landscape.png"),
        }
    }

    /// Write the template's assets into `root` (an existing project folder) and return its scene.
    fn build(self, root: &Path) -> Result<misty_engine::scene::Scene, String> {
        Ok(match self {
            Self::Empty => scenes::empty(),
            Self::Standard => {
                scenes::write_materials(root)?;
                scenes::standard()
            }
            Self::Ocean => {
                scenes::write_materials(root)?;
                scenes::ocean()
            }
            Self::Landscape => landscape::build(root)?,
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
