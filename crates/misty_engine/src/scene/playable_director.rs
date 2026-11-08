//! The Playable Director component: plays a `.timeline` asset in Play mode, with the scene
//! objects each track drives (evaluated by `timeline::DirectorPlayer`).

use hecs::Entity;
use serde::{Deserialize, Serialize};

/// Which clock moves the director.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirectorUpdateMode {
    /// Scaled game time (pauses with Time Scale 0).
    #[default]
    GameTime,
    UnscaledGameTime,
    /// Only moves when a script sets `time` and calls `Evaluate()`.
    Manual,
}

impl DirectorUpdateMode {
    pub const ALL: [DirectorUpdateMode; 3] = [Self::GameTime, Self::UnscaledGameTime, Self::Manual];

    pub fn label(self) -> &'static str {
        match self {
            Self::GameTime => "Game Time",
            Self::UnscaledGameTime => "Unscaled Game Time",
            Self::Manual => "Manual",
        }
    }
}

/// What happens when playback reaches the end of the timeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirectorWrapMode {
    /// Stay on the last frame (still playing).
    Hold,
    /// Start over.
    Loop,
    /// Stop.
    #[default]
    None,
}

impl DirectorWrapMode {
    pub const ALL: [DirectorWrapMode; 3] = [Self::Hold, Self::Loop, Self::None];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hold => "Hold",
            Self::Loop => "Loop",
            Self::None => "None",
        }
    }
}

/// A track and the scene object it drives.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackBinding {
    /// The track's id in the timeline asset.
    pub track: String,
    #[serde(with = "super::components::entity_bits::option")]
    pub object: Option<Entity>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayableDirector {
    pub enabled: bool,
    /// Timeline asset path ("Assets/Timelines/Intro.timeline").
    pub playable: Option<String>,
    pub update_method: DirectorUpdateMode,
    pub play_on_awake: bool,
    pub wrap_mode: DirectorWrapMode,
    /// Seconds playback starts at.
    pub initial_time: f32,
    pub bindings: Vec<TrackBinding>,
    /// Scene objects the timeline's clips point at (a Camera clip's virtual camera, a Control
    /// clip's source object), by the clip's reference name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ExposedReference>,
}

/// A clip's scene object, kept on the director because assets can't hold scene objects.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExposedReference {
    /// The clip's `reference` name in the timeline ("r1", "r2", …).
    pub name: String,
    #[serde(with = "super::components::entity_bits::option")]
    pub object: Option<Entity>,
}

impl Default for PlayableDirector {
    fn default() -> Self {
        Self {
            enabled: true,
            playable: None,
            update_method: DirectorUpdateMode::GameTime,
            play_on_awake: true,
            wrap_mode: DirectorWrapMode::None,
            initial_time: 0.0,
            bindings: Vec::new(),
            references: Vec::new(),
        }
    }
}

impl PlayableDirector {
    /// The object bound to track `id`.
    pub fn binding(&self, id: &str) -> Option<Entity> {
        self.bindings
            .iter()
            .find(|b| b.track == id)
            .and_then(|b| b.object)
    }

    /// Bind track `id` (`None` clears it).
    pub fn set_binding(&mut self, id: &str, object: Option<Entity>) {
        match self.bindings.iter_mut().find(|b| b.track == id) {
            Some(b) => b.object = object,
            None => self.bindings.push(TrackBinding {
                track: id.to_owned(),
                object,
            }),
        }
    }

    /// The object a clip reference `name` points at.
    pub fn reference(&self, name: &str) -> Option<Entity> {
        self.references
            .iter()
            .find(|r| r.name == name)
            .and_then(|r| r.object)
    }

    /// Point clip reference `name` at `object` (`None` clears it).
    pub fn set_reference(&mut self, name: &str, object: Option<Entity>) {
        match self.references.iter_mut().find(|r| r.name == name) {
            Some(r) => r.object = object,
            None => self.references.push(ExposedReference {
                name: name.to_owned(),
                object,
            }),
        }
    }
}
