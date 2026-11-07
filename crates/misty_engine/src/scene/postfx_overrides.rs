//! Post-process Volume overrides of the camera-like effects: Depth of Field, Motion Blur and
//! Auto Exposure (eye adaptation). Like Unity's Volume overrides each one only applies while
//! ticked, and all of them are off by default, so scenes saved before they existed render as
//! they did. The renderer side lives in `render::postfx`.

use serde::{Deserialize, Serialize};

/// How Depth of Field blurs (Unity URP's Depth of Field ▸ Mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DofMode {
    /// Cheap: a Gaussian blur of what is past a distance range (far field only).
    #[default]
    Gaussian,
    /// Physically based: a lens with a focus distance, aperture and focal length, and a
    /// polygonal aperture (blade count) shaping the bokeh. Blurs near and far fields.
    Bokeh,
}

impl DofMode {
    pub const ALL: [DofMode; 2] = [Self::Gaussian, Self::Bokeh];

    pub fn label(self) -> &'static str {
        match self {
            Self::Gaussian => "Gaussian",
            Self::Bokeh => "Bokeh",
        }
    }
}

/// Sample budget of a screen effect (more samples = smoother, slower).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostFxQuality {
    Low,
    #[default]
    Medium,
    High,
}

impl PostFxQuality {
    pub const ALL: [PostFxQuality; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DepthOfFieldOverride {
    pub active: bool,
    pub mode: DofMode,
    /// Gaussian: camera distance (m) where the blur starts.
    pub gaussian_start: f32,
    /// Gaussian: camera distance (m) where the blur reaches Max Radius.
    pub gaussian_end: f32,
    /// Gaussian: 0.5..1.5, the largest blur (1 = 10 px at 1080p).
    pub gaussian_max_radius: f32,
    /// Gaussian: more taps (smoother, a little slower).
    pub high_quality_sampling: bool,
    /// Bokeh: distance (m) of the plane in perfect focus.
    pub focus_distance: f32,
    /// Bokeh: focal length of the lens in millimetres (on a 24 mm tall sensor).
    pub focal_length: f32,
    /// Bokeh: aperture as an f-number (smaller = shallower focus).
    pub aperture: f32,
    /// Bokeh: aperture blades (3..9): the shape of the out-of-focus highlights.
    pub blade_count: u32,
    /// Bokeh: 0 = straight blades (a polygon), 1 = round.
    pub blade_curvature: f32,
    /// Bokeh: rotation of the aperture shape in degrees.
    pub blade_rotation: f32,
    /// Bokeh: gather samples (Low 25, Medium 49, High 81) and largest blur radius.
    pub quality: PostFxQuality,
}

impl Default for DepthOfFieldOverride {
    fn default() -> Self {
        Self {
            active: false,
            mode: DofMode::Gaussian,
            gaussian_start: 10.0,
            gaussian_end: 30.0,
            gaussian_max_radius: 1.0,
            high_quality_sampling: false,
            focus_distance: 10.0,
            focal_length: 50.0,
            aperture: 5.6,
            blade_count: 5,
            blade_curvature: 1.0,
            blade_rotation: 0.0,
            quality: PostFxQuality::Medium,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionBlurOverride {
    pub active: bool,
    /// 0..1: share of the frame the shutter is open (0.5 = a 180° shutter).
    pub intensity: f32,
    /// Longest blur as a share of the screen height (0..0.2).
    pub maximum_velocity: f32,
    /// Samples along the motion (4..32).
    pub sample_count: u32,
    /// Blur from the camera's own movement and rotation (off = moving objects only).
    pub camera_motion_blur: bool,
    /// Longest camera-motion blur as a share of the screen height (0..0.2): keeps fast
    /// camera turns readable while objects still blur fully.
    pub camera_clamp: f32,
}

impl Default for MotionBlurOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 0.5,
            maximum_velocity: 0.05,
            sample_count: 8,
            camera_motion_blur: true,
            camera_clamp: 0.03,
        }
    }
}

/// Where Auto Exposure measures the scene's brightness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeteringMode {
    /// The whole screen counts the same.
    Average,
    /// Only the middle of the screen (a circle about a fifth of its height).
    Spot,
    /// The whole screen, the middle counting most.
    #[default]
    CenterWeighted,
}

impl MeteringMode {
    pub const ALL: [MeteringMode; 3] = [Self::Average, Self::Spot, Self::CenterWeighted];

    pub fn label(self) -> &'static str {
        match self {
            Self::Average => "Average",
            Self::Spot => "Spot",
            Self::CenterWeighted => "Center Weighted",
        }
    }
}

/// How fast Auto Exposure follows a change of brightness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdaptationMode {
    /// Jumps to the new exposure at once.
    Fixed,
    /// Eases towards it (Speed Dark to Light / Light to Dark).
    #[default]
    Progressive,
}

impl AdaptationMode {
    pub const ALL: [AdaptationMode; 2] = [Self::Fixed, Self::Progressive];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fixed => "Fixed",
            Self::Progressive => "Progressive",
        }
    }
}

/// Eye adaptation from a luminance histogram of the image. EV values are relative to the
/// engine's fixed exposure: EV 0 means "as without Auto Exposure", +1 a scene twice as bright
/// as mid grey (which is then darkened a stop).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoExposureOverride {
    pub active: bool,
    pub metering: MeteringMode,
    /// Darkest exposure the adaptation goes to (EV, -10..10).
    pub limit_min: f32,
    /// Brightest exposure the adaptation goes to (EV, -10..10).
    pub limit_max: f32,
    /// EV added after adaptation (+1 = a stop brighter).
    pub compensation: f32,
    pub adaptation: AdaptationMode,
    /// Progressive: adaptation speed from a dark scene to a bright one.
    pub speed_dark_to_light: f32,
    /// Progressive: adaptation speed from a bright scene to a dark one.
    pub speed_light_to_dark: f32,
    /// Share of the darkest pixels left out of the average (percent).
    pub histogram_low: f32,
    /// Share up to which brighter pixels count (percent; the rest are left out).
    pub histogram_high: f32,
}

impl Default for AutoExposureOverride {
    fn default() -> Self {
        Self {
            active: false,
            metering: MeteringMode::CenterWeighted,
            limit_min: -3.0,
            limit_max: 3.0,
            compensation: 0.0,
            adaptation: AdaptationMode::Progressive,
            speed_dark_to_light: 3.0,
            speed_light_to_dark: 1.0,
            histogram_low: 40.0,
            histogram_high: 90.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::PostProcessVolume;

    #[test]
    fn new_overrides_stay_out_of_old_files() {
        // A volume saved before these existed keeps its exact text, and loads with them off.
        let text = serde_json::to_string(&PostProcessVolume::default()).unwrap();
        for key in ["depth_of_field", "motion_blur", "auto_exposure"] {
            assert!(!text.contains(key), "{key} in {text}");
        }
        let old: PostProcessVolume = serde_json::from_str("{\"priority\":1.0}").unwrap();
        assert!(!old.depth_of_field.active && !old.motion_blur.active);
        assert!(!old.auto_exposure.active);
        // A ticked override round-trips.
        let mut v = PostProcessVolume::default();
        v.depth_of_field.active = true;
        v.depth_of_field.mode = DofMode::Bokeh;
        v.motion_blur.active = true;
        v.auto_exposure.active = true;
        v.auto_exposure.metering = MeteringMode::Spot;
        let back: PostProcessVolume =
            serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(back, v);
    }
}
