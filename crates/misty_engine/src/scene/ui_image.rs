//! Image Type settings of the UI Image (Simple, Sliced, Tiled, Filled) and the extra UI
//! components of round 19: Canvas Group and Aspect Ratio Fitter. The geometry is made in
//! `crate::ui::image_mesh`, the group and fitter maths in `crate::ui::tree`.

use serde::{Deserialize, Serialize};

use super::registry::SceneComponent;

/// How an Image fills its rect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageType {
    /// The sprite stretched over the rect.
    #[default]
    Simple,
    /// Corners kept at their size, edges stretched along one axis, centre in both (the sprite's
    /// borders from its import settings).
    Sliced,
    /// The sprite repeated at its size (the borders, if any, stay at the edges).
    Tiled,
    /// Part of the sprite shown, cut horizontally, vertically or by an angle (`fill_amount`).
    Filled,
}

impl ImageType {
    pub const ALL: [Self; 4] = [Self::Simple, Self::Sliced, Self::Tiled, Self::Filled];

    pub fn label(self) -> &'static str {
        match self {
            Self::Simple => "Simple",
            Self::Sliced => "Sliced",
            Self::Tiled => "Tiled",
            Self::Filled => "Filled",
        }
    }
}

/// The cut of a Filled image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillMethod {
    Horizontal,
    Vertical,
    /// A quarter turn around a corner.
    Radial90,
    /// A half turn around the middle of an edge.
    Radial180,
    /// A full turn around the centre.
    #[default]
    Radial360,
}

impl FillMethod {
    pub const ALL: [Self; 5] = [
        Self::Horizontal,
        Self::Vertical,
        Self::Radial90,
        Self::Radial180,
        Self::Radial360,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Radial90 => "Radial 90",
            Self::Radial180 => "Radial 180",
            Self::Radial360 => "Radial 360",
        }
    }

    /// Names of the fill origins this method offers (`fill_origin` indexes them).
    pub fn origins(self) -> &'static [&'static str] {
        match self {
            Self::Horizontal => &["Left", "Right"],
            Self::Vertical => &["Bottom", "Top"],
            Self::Radial90 => &["Bottom Left", "Top Left", "Top Right", "Bottom Right"],
            Self::Radial180 | Self::Radial360 => &["Bottom", "Left", "Top", "Right"],
        }
    }

    /// Radial methods have a direction (clockwise or not).
    pub fn is_radial(self) -> bool {
        matches!(self, Self::Radial90 | Self::Radial180 | Self::Radial360)
    }
}

/// Settings of the non-simple Image types; left out of files while the Image is Simple with
/// default values, so older scenes re-save unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageFill {
    pub method: FillMethod,
    /// Index into [`FillMethod::origins`].
    pub origin: u32,
    /// 0 (nothing) to 1 (the whole image).
    pub amount: f32,
    pub clockwise: bool,
}

impl Default for ImageFill {
    fn default() -> Self {
        Self {
            method: FillMethod::Radial360,
            origin: 0,
            amount: 1.0,
            clockwise: true,
        }
    }
}

impl ImageFill {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Fades, disables and lets clicks through a whole branch of UI: the alpha multiplies every
/// graphic below it, Interactable off disables its selectables, Blocks Raycasts off makes its
/// graphics ignore the pointer. Nested groups combine unless one ignores its parents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasGroup {
    pub enabled: bool,
    pub alpha: f32,
    pub interactable: bool,
    pub blocks_raycasts: bool,
    pub ignore_parent_groups: bool,
}

impl Default for CanvasGroup {
    fn default() -> Self {
        Self {
            enabled: true,
            alpha: 1.0,
            interactable: true,
            blocks_raycasts: true,
            ignore_parent_groups: false,
        }
    }
}

/// What an Aspect Ratio Fitter changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectMode {
    #[default]
    None,
    /// The width sets the height.
    WidthControlsHeight,
    /// The height sets the width.
    HeightControlsWidth,
    /// As large as fits inside the parent.
    FitInParent,
    /// As small as covers the parent.
    EnvelopeParent,
}

impl AspectMode {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::WidthControlsHeight,
        Self::HeightControlsWidth,
        Self::FitInParent,
        Self::EnvelopeParent,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::WidthControlsHeight => "Width Controls Height",
            Self::HeightControlsWidth => "Height Controls Width",
            Self::FitInParent => "Fit In Parent",
            Self::EnvelopeParent => "Envelope Parent",
        }
    }
}

/// Keeps a rect at a width / height ratio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AspectRatioFitter {
    pub enabled: bool,
    pub aspect_mode: AspectMode,
    /// Width divided by height.
    pub aspect_ratio: f32,
}

impl Default for AspectRatioFitter {
    fn default() -> Self {
        Self {
            enabled: true,
            aspect_mode: AspectMode::None,
            aspect_ratio: 1.0,
        }
    }
}

impl SceneComponent for CanvasGroup {
    const NAME: &'static str = "Canvas Group";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for AspectRatioFitter {
    const NAME: &'static str = "Aspect Ratio Fitter";
    const CATEGORY: &'static str = "Layout";
}
