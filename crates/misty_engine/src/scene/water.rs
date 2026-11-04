//! Water components: Water Surface (ocean, lake, river) and Floater (buoyancy for Rigidbodies).

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::water::{WaterPreset, WaterSettings};

/// Shape of a water surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WaterKind {
    /// Endless sea following the camera to the horizon (only the object's height matters).
    #[default]
    Ocean,
    /// A rectangle `size` wide, centred on the object.
    Lake,
    /// A rectangle flowing along the object's forward (+Z) axis.
    River,
}

impl WaterKind {
    pub const ALL: [Self; 3] = [Self::Ocean, Self::Lake, Self::River];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ocean => "Ocean",
            Self::Lake => "Lake",
            Self::River => "River",
        }
    }

    /// The preset new surfaces of this kind start from.
    pub fn preset(self) -> WaterPreset {
        match self {
            Self::Ocean => WaterPreset::Ocean,
            Self::Lake => WaterPreset::Lake,
            Self::River => WaterPreset::MurkyRiver,
        }
    }
}

/// A water surface at the object's height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterSurface {
    pub enabled: bool,
    pub kind: WaterKind,
    /// Lake / River extent in metres along the object's X and Z (scaled by the object).
    pub size: [f32; 2],
    pub settings: WaterSettings,
}

impl Default for WaterSurface {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: WaterKind::Ocean,
            size: [40.0, 40.0],
            settings: WaterSettings::default(),
        }
    }
}

impl WaterSurface {
    /// A surface of `kind` with its default preset.
    pub fn new(kind: WaterKind) -> Self {
        Self {
            kind,
            size: match kind {
                WaterKind::River => [8.0, 60.0],
                _ => [40.0, 40.0],
            },
            settings: kind.preset().settings(),
            ..Default::default()
        }
    }
}

/// Makes the object's Rigidbody float on water: probes at the corners of a box push up by how
/// deep they are, and water drag slows it down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Floater {
    pub enabled: bool,
    /// Upward push when fully under water, in multiples of the object's weight
    /// (above 1 floats; 2 floats about half submerged).
    pub buoyancy: f32,
    /// Probe box size in local metres (probes sit at its eight corners).
    pub size: Vec3,
    /// Probe box centre in local space.
    pub center: Vec3,
    /// Linear drag while in water (per second).
    pub water_drag: f32,
    /// Angular drag while in water (per second).
    pub water_angular_drag: f32,
}

impl Default for Floater {
    fn default() -> Self {
        Self {
            enabled: true,
            buoyancy: 2.0,
            size: Vec3::ONE,
            center: Vec3::ZERO,
            water_drag: 1.5,
            water_angular_drag: 1.0,
        }
    }
}
