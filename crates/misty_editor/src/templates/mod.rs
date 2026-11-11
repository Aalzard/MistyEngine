//! Project templates for the Hub's New Project page. Each one writes a complete project folder:
//! `Assets/`, `ProjectSettings/` (with Build Settings listing the starting scene), the starting
//! scene itself and `Library/EditorSettings.json` so it opens first.
//!
//! - MeadowDemo is copied from the sample project (see [`meadow`]).
//! - First Person builds its scene in code around the sample's First Person Player prefab
//!   (see [`first_person`]).
//! - Racing (see [`racing`]), 2D Platformer (see [`platformer_2d`]) and Floating Platform (see
//!   [`scenes`]) are generated in code.
//!
//! Every 3D template starts with a global post-processing volume: tonemapping, bloom,
//! volumetric clouds and sun shafts ([`scenes::sky_volume`]; MeadowDemo has its own). The 2D
//! Platformer has bloom for its glowing sun, torches and coins.

pub mod first_person;
pub mod meadow;
pub mod platformer_2d;
pub mod racing;
pub mod samples;
mod scenes;
#[cfg(test)]
mod tests;
mod textures;

use std::path::Path;

use misty_engine::asset_db::{GuidDatabase, refs};
use misty_engine::build::{BuildScene, BuildSettings};
use misty_engine::player_settings::PlayerSettings;
use misty_engine::scene::Scene;
use misty_engine::settings::ProjectSettings;
use serde::{Deserialize, Serialize};

use crate::scene_files::{SavedView, SavedView2D};

/// The starting scene of the generated templates (listed in Build Settings); MeadowDemo
/// starts on [`meadow::SCENE`] (see [`Template::scene`]).
pub const SCENE: &str = "Assets/Scenes/Main.scene";
/// Preview the Hub shows for projects it created (a copy of the template's thumbnail).
pub const PROJECT_THUMBNAIL: &str = "Library/HubThumbnail.png";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Template {
    /// The sample project's walkable meadow with its first-person player.
    MeadowDemo,
    /// A first-person player on an endless flat white ground.
    FirstPerson,
    /// An arcade racer: a circuit, a car with three bodies, lap timer and live settings.
    Racing,
    /// A 2D platformer: a tilemap level, a player that runs and jumps, coins, 2D lights.
    Platformer2D,
    /// A platform floating in the sky: start from scratch.
    #[default]
    #[serde(alias = "Standard")]
    FloatingPlatform,
}

impl Template {
    pub const ALL: [Self; 5] = [
        Self::MeadowDemo,
        Self::FirstPerson,
        Self::Racing,
        Self::Platformer2D,
        Self::FloatingPlatform,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MeadowDemo => "MeadowDemo",
            Self::FirstPerson => "First Person",
            Self::Racing => "Racing",
            Self::Platformer2D => "2D Platformer",
            Self::FloatingPlatform => "Floating Platform",
        }
    }

    /// Case and spaces don't matter: `--new firstperson` works too. "Standard" (the Floating
    /// Platform's old name) still works, and "2D" or "Platformer2D" (the sample scene's name)
    /// pick the 2D Platformer.
    pub fn from_name(name: &str) -> Option<Self> {
        let key = |n: &str| n.replace(' ', "").to_ascii_lowercase();
        match key(name).as_str() {
            "standard" => return Some(Self::FloatingPlatform),
            "2d" | "platformer2d" => return Some(Self::Platformer2D),
            _ => {}
        }
        Self::ALL.into_iter().find(|t| key(t.name()) == key(name))
    }

    /// The thumbnail file name in `assets/templates/` (for `write_template_thumbnails`).
    #[cfg(test)]
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::MeadowDemo => "meadow_demo",
            Self::FirstPerson => "first_person",
            Self::Racing => "racing",
            Self::Platformer2D => "platformer_2d",
            Self::FloatingPlatform => "floating_platform",
        }
    }

    /// One line for the template card.
    pub fn description(self) -> &'static str {
        match self {
            Self::MeadowDemo => {
                "A walkable meadow with a first-person player: thin grass, trees, Misty GI. \
                 Free assets for your game."
            }
            Self::FirstPerson => {
                "A first-person player on an endless white plain, ready to press Play."
            }
            Self::Racing => {
                "An arcade racer: a circuit, three cars, boost, a lap timer and live settings, \
                 all in Lua."
            }
            Self::Platformer2D => {
                "A 2D platformer: run and jump through a tilemap level, collect coins, 2D lights \
                 and glow. Paint more with the Tile Palette."
            }
            Self::FloatingPlatform => {
                "A platform floating in the sky: camera, sun, clouds and glow. Start from scratch."
            }
        }
    }

    /// The scene the project opens on (and lists first in Build Settings).
    pub fn scene(self) -> &'static str {
        match self {
            Self::MeadowDemo => meadow::SCENE,
            Self::FirstPerson | Self::Racing | Self::Platformer2D | Self::FloatingPlatform => SCENE,
        }
    }

    /// Whether this install can make the template (MeadowDemo and First Person copy from the
    /// sample project).
    pub fn available(self) -> bool {
        match self {
            Self::MeadowDemo | Self::FirstPerson => samples::source().is_some(),
            Self::Racing | Self::Platformer2D | Self::FloatingPlatform => true,
        }
    }

    /// The thumbnail PNG (rendered headlessly by `write_template_thumbnails`).
    pub fn thumbnail_png(self) -> &'static [u8] {
        match self {
            Self::MeadowDemo => include_bytes!("../../assets/templates/meadow_demo.png"),
            Self::FirstPerson => include_bytes!("../../assets/templates/first_person.png"),
            Self::Racing => include_bytes!("../../assets/templates/racing.png"),
            Self::Platformer2D => include_bytes!("../../assets/templates/platformer_2d.png"),
            Self::FloatingPlatform => {
                include_bytes!("../../assets/templates/floating_platform.png")
            }
        }
    }

    /// Whether the template is a 2D game (its Scene view starts in 2D mode).
    pub fn is_2d(self) -> bool {
        self == Self::Platformer2D
    }

    /// Write the template's assets and starting scene into `root` (an existing project folder
    /// with its settings). Returns the Scene view to start with.
    fn build(self, root: &Path) -> Result<StartView, String> {
        match self {
            Self::MeadowDemo => meadow::create(root).map(StartView::Eye),
            Self::FirstPerson => first_person::create(root).map(StartView::Eye),
            Self::Racing => racing::create(root).map(|()| StartView::MainCamera),
            Self::Platformer2D => platformer_2d::create(root).map(StartView::TwoD),
            Self::FloatingPlatform => {
                scenes::write_materials(root)?;
                save_scene(root, SCENE, &scenes::floating_platform())
                    .map(|()| StartView::MainCamera)
            }
        }
    }
}

/// Where a new project's Scene view starts.
enum StartView {
    /// Looking through the scene's Main Camera.
    MainCamera,
    /// At this eye (3D).
    Eye(SavedView),
    /// In 2D mode, showing this part of the XY plane.
    TwoD(SavedView2D),
}

/// Save a template's scene at `root/rel`, its GUID table from the new project's own meta
/// files (not the project the editor has open, if any).
fn save_scene(root: &Path, rel: &str, scene: &Scene) -> Result<(), String> {
    let path = root.join(rel);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_error("Could not create the folder", dir))?;
    }
    let text = refs::embed(&GuidDatabase::read(root), &scene.to_json());
    std::fs::write(&path, text).map_err(io_error("Could not write", &path))
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
        // MeadowDemo and the 2D Platformer use no materials: no empty folder.
        let no_materials = template == Template::MeadowDemo || template.is_2d();
        if no_materials && dir == "Assets/Materials" {
            continue;
        }
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
            path: template.scene().into(),
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

    let start = template.build(root)?;
    // The editor opens the most recent scene first.
    let (scene_view, scene_view_2d) = match start {
        StartView::MainCamera => (None, None),
        StartView::Eye(v) => (Some(v), None),
        StartView::TwoD(v) => (None, Some(v)),
    };
    crate::scene_files::EditorSettings {
        recent_scenes: vec![template.scene().into()],
        scene_view,
        scene_view_2d,
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
