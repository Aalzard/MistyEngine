//! Project templates for the Hub's New Project page. Each one writes a complete project folder
//! generated in code: `Assets/`, `ProjectSettings/` (with Build Settings listing the starting
//! scene), the starting scene itself and `Library/EditorSettings.json` so it opens first.

pub mod samples;
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
    create_with(root, template, false)
}

/// [`create`], optionally adding every sample scene (New Project ▸ Include sample scenes).
pub fn create_with(root: &Path, template: Template, include_samples: bool) -> Result<(), String> {
    create_template(root, template)?;
    if include_samples {
        let source = samples::source().ok_or("this install has no sample scenes to copy")?;
        samples::add(root, &source)?;
    }
    Ok(())
}

fn create_template(root: &Path, template: Template) -> Result<(), String> {
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
        let path = root.join(dir);
        std::fs::create_dir_all(&path).map_err(io_error("Could not create the folder", &path))?;
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
    settings.save(root).map_err(|e| {
        format!(
            "Could not write the project settings in {}: {e}",
            root.display()
        )
    })?;
    BuildSettings {
        scenes: vec![BuildScene {
            path: SCENE.into(),
            enabled: true,
        }],
        product_name: name,
        ..Default::default()
    }
    .save(root)
    .map_err(|e| {
        format!(
            "Could not write the build settings in {}: {e}",
            root.display()
        )
    })?;

    let scene = template.build(root)?;
    let scene_path = root.join(SCENE);
    scene
        .save(&scene_path)
        .map_err(|e| format!("Could not write {}: {e}", scene_path.display()))?;
    // The editor opens the most recent scene first.
    crate::scene_files::EditorSettings {
        recent_scenes: vec![SCENE.into()],
        ..Default::default()
    }
    .save(root);
    let thumbnail = root.join(PROJECT_THUMBNAIL);
    std::fs::write(&thumbnail, template.thumbnail_png())
        .map_err(io_error("Could not write", &thumbnail))?;
    let gitignore = root.join(".gitignore");
    std::fs::write(&gitignore, "Library/\nBuilds/\n")
        .map_err(io_error("Could not write", &gitignore))
}

/// An I/O error that says what was being done and to which file, so a report from a user's
/// machine points at the cause (a bare "os error 2" doesn't).
pub(crate) fn io_error<'a>(
    action: &'a str,
    path: &'a Path,
) -> impl FnOnce(std::io::Error) -> String + 'a {
    move |e| format!("{action} {}: {e}", path.display())
}
