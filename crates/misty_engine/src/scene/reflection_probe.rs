//! Reflection Probe component (GameObject ▸ Light ▸ Reflection Probe): captures the scene around
//! a point into a cubemap that nearby surfaces reflect, plus diffuse ambient (SH9) from it.
//! The influence box is axis-aligned in world space: `center` offsets it from the object and
//! rotation/scale are ignored. Baked data lives in `baked` (see `render::probes`).
//! Reflection Probe Group marks the parent of generated probes and keeps the generator settings.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Baked = captured by Bake Reflection Probes and saved; Realtime = re-captured while running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeMode {
    #[default]
    Baked,
    Realtime,
}

impl ProbeMode {
    pub const ALL: [Self; 2] = [Self::Baked, Self::Realtime];

    pub fn label(self) -> &'static str {
        match self {
            Self::Baked => "Baked",
            Self::Realtime => "Realtime",
        }
    }
}

/// What the capture shows where nothing was drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeClearFlags {
    #[default]
    Skybox,
    SolidColor,
}

impl ProbeClearFlags {
    pub const ALL: [Self; 2] = [Self::Skybox, Self::SolidColor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Skybox => "Skybox",
            Self::SolidColor => "Solid Color",
        }
    }
}

/// Cubemap face sizes offered in the Inspector.
pub const RESOLUTIONS: [u32; 4] = [64, 128, 256, 512];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReflectionProbe {
    pub enabled: bool,
    pub mode: ProbeMode,
    /// Realtime: re-capture every this many frames (0 = only on demand), one face per frame
    /// when time slicing.
    pub refresh_interval: u32,
    pub time_slicing: bool,
    /// Higher wins where boxes overlap; ties go to the smaller box.
    pub importance: i32,
    pub intensity: f32,
    /// Parallax-correct reflections against the box (walls line up).
    pub box_projection: bool,
    /// Fade-in distance inside the box faces (metres).
    pub blend_distance: f32,
    /// Box size and its offset from the object (world axes).
    pub size: Vec3,
    pub center: Vec3,
    /// Cubemap face size in pixels (64–512).
    pub resolution: u32,
    pub near_clip: f32,
    pub far_clip: f32,
    pub clear_flags: ProbeClearFlags,
    /// sRGB background for Solid Color.
    pub background: [f32; 3],
    /// Project path of the baked cubemap (None = not baked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baked: Option<String>,
}

impl Default for ReflectionProbe {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: ProbeMode::Baked,
            refresh_interval: 1,
            time_slicing: true,
            importance: 1,
            intensity: 1.0,
            box_projection: false,
            blend_distance: 1.0,
            size: Vec3::splat(10.0),
            center: Vec3::ZERO,
            resolution: 128,
            near_clip: 0.3,
            far_clip: 1000.0,
            clear_flags: ProbeClearFlags::Skybox,
            background: [0.19, 0.3, 0.47],
            baked: None,
        }
    }
}

impl ReflectionProbe {
    /// World influence box (min, max) of a probe at `position`.
    pub fn world_box(&self, position: Vec3) -> (Vec3, Vec3) {
        let c = position + self.center;
        let h = self.size.abs() * 0.5;
        (c - h, c + h)
    }

    /// Face size clamped to the supported range and rounded to a power of two.
    pub fn face_size(&self) -> u32 {
        self.resolution.clamp(64, 512).next_power_of_two().min(512)
    }
}

/// Parent of the probes made by Generate Probes, with the settings used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReflectionProbeGroup {
    /// Grid spacing between candidate probes (metres).
    pub spacing: f32,
    /// Probe height above the floor below it.
    pub height: f32,
    /// Merge neighbours whose boxes are nearly the same (large open areas, small rooms).
    pub merge: bool,
    pub resolution: u32,
    /// Second bake pass in which the probes see each other's reflections.
    pub second_bounce: bool,
}

impl Default for ReflectionProbeGroup {
    fn default() -> Self {
        Self {
            spacing: 4.0,
            height: 1.5,
            merge: true,
            resolution: 128,
            second_bounce: true,
        }
    }
}
