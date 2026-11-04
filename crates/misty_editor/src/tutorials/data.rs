//! Tutorial data: one JSON file per tutorial in `crates/misty_editor/assets/tutorials/`,
//! built into the editor. To add a tutorial, write a new file and list it in [`FILES`].
//!
//! ```json
//! { "id": "my-tutorial", "title": "…", "description": "…", "minutes": 5,
//!   "scene": "basic",
//!   "steps": [ { "title": "…", "text": "…", "targets": ["hierarchy.Main Camera"],
//!                "done_when": "selected:Main Camera", "demo": "select:Main Camera" } ] }
//! ```
//! `scene` names a starting scene in `scenes.rs`; `targets` are UI IDs (see `targets.rs`),
//! the first one drawn is highlighted; `done_when` (see `conditions.rs`) auto-advances the
//! step; `demo` (see `demos.rs`) does the step for screenshots and tests.

use serde::Deserialize;

/// The built-in tutorials, in the order the Tutorials window lists them.
const FILES: [&str; 4] = [
    include_str!("../../assets/tutorials/post_processing.json"),
    include_str!("../../assets/tutorials/god_rays.json"),
    include_str!("../../assets/tutorials/terrain.json"),
    include_str!("../../assets/tutorials/world_builder.json"),
];

#[derive(Clone, Debug, Deserialize)]
pub struct Tutorial {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Estimated time in minutes.
    pub minutes: u32,
    /// Starting scene (see `scenes::setup`).
    #[serde(default)]
    pub scene: String,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Step {
    pub title: String,
    /// One to three short sentences.
    pub text: String,
    /// UI IDs to highlight, most specific first.
    pub targets: Vec<String>,
    /// Condition that finishes the step on its own.
    pub done_when: Option<String>,
    /// What the step asks for, done in code (screenshots, tests).
    pub demo: Option<String>,
}

/// Parse every built-in tutorial (panics on a broken file: they are part of the build and
/// covered by tests).
pub fn all() -> &'static [Tutorial] {
    static ALL: std::sync::OnceLock<Vec<Tutorial>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        FILES
            .iter()
            .map(|text| serde_json::from_str(text).expect("built-in tutorial JSON"))
            .collect()
    })
}

pub fn find(id: &str) -> Option<&'static Tutorial> {
    all().iter().find(|t| t.id == id)
}
