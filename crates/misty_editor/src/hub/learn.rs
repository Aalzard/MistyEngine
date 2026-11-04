//! The Learn page's tutorial registry. The tutorials themselves come later: add entries to
//! [`TUTORIALS`] and give them an `open` action (the page then shows a Start button).

pub struct Tutorial {
    pub title: &'static str,
    pub summary: &'static str,
    /// Rough length in minutes.
    pub minutes: u32,
    /// Starts the tutorial; `None` = listed as "Coming soon".
    pub open: Option<fn() -> Result<std::path::PathBuf, String>>,
}

/// Every tutorial the Learn page lists, in order.
pub const TUTORIALS: &[Tutorial] = &[
    Tutorial {
        title: "Your first scene",
        summary: "Move around the Scene view, add objects and change them in the Inspector.",
        minutes: 10,
        open: None,
    },
    Tutorial {
        title: "Roll a ball",
        summary: "Physics, a Lua script and input: a small game from start to finish.",
        minutes: 25,
        open: None,
    },
    Tutorial {
        title: "Light and atmosphere",
        summary: "Sun, sky, shadows and a Post-process Volume with bloom and fog.",
        minutes: 15,
        open: None,
    },
    Tutorial {
        title: "Build a landscape",
        summary: "Shape terrain with the World Builder, paint biomes and add water.",
        minutes: 20,
        open: None,
    },
];
