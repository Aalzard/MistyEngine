//! The Weather component (a weather volume) and its timeline, surface, effect and audio
//! settings.

use serde::{Deserialize, Serialize};

use super::{WeatherParams, WeatherPreset};
use crate::scene::SceneComponent;

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// Where a volume's weather applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coverage {
    /// Everywhere: the precipitation is spawned around the camera wherever it goes.
    #[default]
    Global,
    /// Inside a box (the object's position, rotation and scale × Size), fading over the
    /// Blend Distance at its faces.
    Box,
    /// Inside a sphere (Radius × the object's largest scale), fading over the Blend Distance.
    Sphere,
}

impl Coverage {
    pub const ALL: [Self; 3] = [Self::Global, Self::Box, Self::Sphere];

    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global (follows the camera)",
            Self::Box => "Area: Box",
            Self::Sphere => "Area: Sphere",
        }
    }
}

/// How a timeline moves between its entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineMode {
    /// No timeline: the volume shows its own values.
    #[default]
    Off,
    /// The entries in order (looping or stopping at the last).
    Sequence,
    /// Weighted random entries, each held for a random time between its min and max.
    Random,
}

impl TimelineMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::Sequence, Self::Random];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Sequence => "Sequence",
            Self::Random => "Random (weighted)",
        }
    }
}

/// One state of a weather timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineEntry {
    pub preset: WeatherPreset,
    /// Free values instead of the preset's (None = the preset).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<WeatherParams>,
    /// Seconds the state holds once reached (Random: the shortest hold).
    pub duration: f32,
    /// Random: the longest hold (seconds).
    pub max_duration: f32,
    /// Seconds to blend into this state from the one before.
    pub transition: f32,
    /// Random: how likely this entry is picked next.
    pub weight: f32,
}

impl Default for TimelineEntry {
    fn default() -> Self {
        Self {
            preset: WeatherPreset::Rain,
            custom: None,
            duration: 60.0,
            max_duration: 120.0,
            transition: 15.0,
            weight: 1.0,
        }
    }
}

impl TimelineEntry {
    pub fn new(preset: WeatherPreset, duration: f32, transition: f32) -> Self {
        Self {
            preset,
            duration,
            max_duration: duration * 2.0,
            transition,
            ..Default::default()
        }
    }

    pub fn params(&self) -> WeatherParams {
        self.custom.unwrap_or_else(|| self.preset.params())
    }

    /// Display name ("Heavy Rain", or "Custom").
    pub fn label(&self) -> &'static str {
        match self.custom {
            Some(_) => "Custom",
            None => self.preset.label(),
        }
    }
}

/// A list of weather states the volume moves through over time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherTimeline {
    pub mode: TimelineMode,
    pub entries: Vec<TimelineEntry>,
    /// Sequence: start again after the last entry (else stay on it).
    pub looping: bool,
    /// × the speed time passes on the timeline (1 = real seconds; 60 = a minute per second).
    #[serde(skip_serializing_if = "is_one")]
    pub time_scale: f32,
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

impl Default for WeatherTimeline {
    /// Off, no states, real time (a Time Scale of 1 is left out of scene files, so it must
    /// be the default they load with).
    fn default() -> Self {
        Self {
            mode: TimelineMode::Off,
            entries: Vec::new(),
            looping: false,
            time_scale: 1.0,
        }
    }
}

impl WeatherTimeline {
    pub fn is_on(&self) -> bool {
        self.mode != TimelineMode::Off && !self.entries.is_empty()
    }

    /// The looping clear → light rain → thunderstorm → snow → clear demo cycle.
    pub fn demo_cycle() -> Self {
        Self {
            mode: TimelineMode::Sequence,
            entries: vec![
                TimelineEntry::new(WeatherPreset::Clear, 20.0, 10.0),
                TimelineEntry::new(WeatherPreset::LightRain, 25.0, 12.0),
                TimelineEntry::new(WeatherPreset::Thunderstorm, 35.0, 15.0),
                TimelineEntry::new(WeatherPreset::Snow, 35.0, 20.0),
            ],
            looping: true,
            time_scale: 1.0,
        }
    }
}

/// How surfaces react to the weather.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceSettings {
    /// Surfaces get wet (darker, glossier) while it rains.
    pub wetness: bool,
    /// Seconds of hard rain until surfaces are fully wet (light rain takes longer).
    pub wet_time: f32,
    /// Seconds until wet surfaces have dried after the rain stops.
    pub dry_time: f32,
    /// 0..1: how much of the flat ground puddles cover when fully wet.
    pub puddles: f32,
    /// Size of the puddle pattern (metres).
    pub puddle_scale: f32,
    /// Raindrop ripples on puddles and water while it rains.
    pub ripples: bool,
    /// Snow builds up on upward-facing surfaces while it snows.
    pub snow_cover: bool,
    /// Seconds of heavy snow until the ground is fully white (light snow takes longer).
    pub snow_time: f32,
    /// Seconds for full snow cover to melt after the snow stops (rain melts it faster).
    pub melt_time: f32,
    /// 0..1: the most snow cover allowed.
    pub max_snow: f32,
    /// Snow only settles above this height (metres; with Snow Line on).
    #[serde(skip_serializing_if = "is_default")]
    pub snow_line: bool,
    pub snow_line_height: f32,
    /// sRGB colour of settled snow.
    pub snow_color: [f32; 3],
}

impl Default for SurfaceSettings {
    fn default() -> Self {
        Self {
            wetness: true,
            wet_time: 40.0,
            dry_time: 150.0,
            puddles: 0.45,
            puddle_scale: 3.0,
            ripples: true,
            snow_cover: true,
            snow_time: 150.0,
            melt_time: 240.0,
            max_snow: 1.0,
            snow_line: false,
            snow_line_height: 0.0,
            snow_color: [0.93, 0.95, 0.98],
        }
    }
}

/// Which parts of the weather are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherEffects {
    /// Splash particles where drops hit (from the depth buffer, not under roofs).
    pub splashes: bool,
    /// Ground spray in heavy rain.
    pub mist: bool,
    /// Soft distant rain / snow sheets so density reads far away.
    pub far_sheets: bool,
    /// Haze that lowers visibility (blends with the fog).
    pub haze: bool,
    /// No rain or snow under roofs, trees and other shadow casters (a top-down depth map).
    pub occlusion: bool,
    /// Lightning bolts drawn in the sky (flashes light the scene either way).
    pub bolts: bool,
    /// Drops on the camera lens while rain falls on it.
    pub lens_drops: bool,
    /// Moves the scene's volumetric clouds towards the weather's cover and density, and dims
    /// the sun and sky by the weather's dimming.
    pub drive_clouds: bool,
    /// Uses the scene's Wind Zone for the wind direction (strength × Main).
    pub use_wind_zone: bool,
}

impl Default for WeatherEffects {
    fn default() -> Self {
        Self {
            splashes: true,
            mist: true,
            far_sheets: true,
            haze: true,
            occlusion: true,
            bolts: true,
            lens_drops: false,
            drive_clouds: true,
            use_wind_zone: false,
        }
    }
}

/// Weather sounds: the bundled steady rain and storm recordings crossfading with the rain
/// intensity (quieter under cover), a generated wind loop, and thunder (generated unless
/// Thunder Clips are set) delayed by the strike's distance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherAudio {
    pub enabled: bool,
    /// 0..1.
    pub volume: f32,
    /// Rain loop for light and steady rain (empty = the bundled steady rain recording).
    pub rain_clip: String,
    /// Wind loop for strong wind and blizzards (empty = generated).
    pub wind_clip: String,
    /// Thunder clips picked at random (empty = generated near cracks and far rumbles).
    pub thunder_clips: Vec<String>,
    /// Audio Mixer group the sounds play through ("Assets/Audio/Main.mixer#SFX"; empty =
    /// straight to the output).
    pub output: String,
    // Newer fields are left out of files at their defaults (older scenes re-save unchanged).
    /// Loop the rain crossfades into as it gets heavy (empty = the bundled storm recording).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub storm_clip: String,
    /// 0..1: share of the rain and wind volume left under a roof (cover overhead, from rays
    /// cast up from the Audio Listener; Sheltered Areas off = always in the open).
    #[serde(skip_serializing_if = "is_under_cover_default")]
    pub under_cover: f32,
}

/// Default [`WeatherAudio::under_cover`].
/// Rain on a roof (or a car) is still loud, a drumming close overhead: a roof takes only a
/// little of the volume away.
pub const UNDER_COVER: f32 = 0.8;

fn is_under_cover_default(v: &f32) -> bool {
    *v == UNDER_COVER
}

impl Default for WeatherAudio {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.8,
            rain_clip: String::new(),
            wind_clip: String::new(),
            thunder_clips: Vec::new(),
            output: String::new(),
            storm_clip: String::new(),
            under_cover: UNDER_COVER,
        }
    }
}

/// A weather volume (GameObject ▸ Rendering ▸ Weather).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weather {
    pub enabled: bool,
    pub coverage: Coverage,
    /// Box size in local units (× the object's scale).
    pub size: [f32; 3],
    /// Sphere radius (× the object's largest scale).
    pub radius: f32,
    /// Metres over which an area fades out at its edge.
    pub blend_distance: f32,
    /// Higher priority volumes blend over lower ones.
    pub priority: f32,
    /// 0..1: how much of this volume's weather applies.
    pub weight: f32,
    /// The weather this volume shows (when its timeline is off).
    pub state: WeatherParams,
    pub timeline: WeatherTimeline,
    pub surfaces: SurfaceSettings,
    pub effects: WeatherEffects,
    pub audio: WeatherAudio,
    /// Show the weather in the Scene and Game views without entering Play.
    pub preview_in_edit_mode: bool,
    /// Seed of the lightning and random timeline (same seed = same storm).
    pub seed: u32,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            enabled: true,
            coverage: Coverage::Global,
            size: [40.0, 30.0, 40.0],
            radius: 25.0,
            blend_distance: 8.0,
            priority: 0.0,
            weight: 1.0,
            state: WeatherPreset::Rain.params(),
            timeline: WeatherTimeline::default(),
            surfaces: SurfaceSettings::default(),
            effects: WeatherEffects::default(),
            audio: WeatherAudio::default(),
            preview_in_edit_mode: true,
            seed: 1,
        }
    }
}

impl SceneComponent for Weather {
    const NAME: &'static str = "Weather";
    const CATEGORY: &'static str = "Rendering";
}

impl Weather {
    /// A global volume showing `preset`.
    pub fn with_preset(preset: WeatherPreset) -> Self {
        Self {
            state: preset.params(),
            ..Default::default()
        }
    }

    pub fn is_area(&self) -> bool {
        self.coverage != Coverage::Global
    }
}
