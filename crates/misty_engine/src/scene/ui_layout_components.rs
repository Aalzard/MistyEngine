//! Auto-layout components: Horizontal / Vertical / Grid Layout Group, Content Size Fitter and
//! Layout Element. The maths is in `crate::ui::auto_layout`.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::registry::SceneComponent;
use super::ui_components::TextAnchor;

/// Space kept free inside a layout group's rect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Padding {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Padding {
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// Settings shared by the Horizontal and Vertical Layout Groups.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinearLayout {
    pub enabled: bool,
    pub padding: Padding,
    pub spacing: f32,
    pub child_alignment: TextAnchor,
    pub reverse_arrangement: bool,
    /// Set the children's width / height (else only their positions).
    pub control_child_width: bool,
    pub control_child_height: bool,
    /// Give children spare space even without a flexible size.
    pub child_force_expand_width: bool,
    pub child_force_expand_height: bool,
}

impl Default for LinearLayout {
    fn default() -> Self {
        Self {
            enabled: true,
            padding: Padding::default(),
            spacing: 0.0,
            child_alignment: TextAnchor::UpperLeft,
            reverse_arrangement: false,
            control_child_width: true,
            control_child_height: true,
            child_force_expand_width: true,
            child_force_expand_height: true,
        }
    }
}

/// Places children side by side, left to right.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HorizontalLayoutGroup(pub LinearLayout);

/// Places children one below the other, top to bottom.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VerticalLayoutGroup(pub LinearLayout);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridCorner {
    #[default]
    UpperLeft,
    UpperRight,
    LowerLeft,
    LowerRight,
}

impl GridCorner {
    pub const ALL: [Self; 4] = [
        Self::UpperLeft,
        Self::UpperRight,
        Self::LowerLeft,
        Self::LowerRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::UpperLeft => "Upper Left",
            Self::UpperRight => "Upper Right",
            Self::LowerLeft => "Lower Left",
            Self::LowerRight => "Lower Right",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridAxis {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridConstraint {
    /// As many columns as fit.
    #[default]
    Flexible,
    FixedColumnCount,
    FixedRowCount,
}

impl GridConstraint {
    pub const ALL: [Self; 3] = [Self::Flexible, Self::FixedColumnCount, Self::FixedRowCount];

    pub fn label(self) -> &'static str {
        match self {
            Self::Flexible => "Flexible",
            Self::FixedColumnCount => "Fixed Column Count",
            Self::FixedRowCount => "Fixed Row Count",
        }
    }
}

/// Places children in equally sized cells.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridLayoutGroup {
    pub enabled: bool,
    pub padding: Padding,
    pub cell_size: Vec2,
    pub spacing: Vec2,
    pub start_corner: GridCorner,
    pub start_axis: GridAxis,
    pub child_alignment: TextAnchor,
    pub constraint: GridConstraint,
    pub constraint_count: u32,
}

impl Default for GridLayoutGroup {
    fn default() -> Self {
        Self {
            enabled: true,
            padding: Padding::default(),
            cell_size: Vec2::splat(100.0),
            spacing: Vec2::ZERO,
            start_corner: GridCorner::UpperLeft,
            start_axis: GridAxis::Horizontal,
            child_alignment: TextAnchor::UpperLeft,
            constraint: GridConstraint::Flexible,
            constraint_count: 2,
        }
    }
}

/// How a Content Size Fitter sizes one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FitMode {
    #[default]
    Unconstrained,
    MinSize,
    PreferredSize,
}

impl FitMode {
    pub const ALL: [Self; 3] = [Self::Unconstrained, Self::MinSize, Self::PreferredSize];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unconstrained => "Unconstrained",
            Self::MinSize => "Min Size",
            Self::PreferredSize => "Preferred Size",
        }
    }
}

/// Resizes its own rect to the layout size of its content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContentSizeFitter {
    pub enabled: bool,
    pub horizontal_fit: FitMode,
    pub vertical_fit: FitMode,
}

impl Default for ContentSizeFitter {
    fn default() -> Self {
        Self {
            enabled: true,
            horizontal_fit: FitMode::Unconstrained,
            vertical_fit: FitMode::Unconstrained,
        }
    }
}

/// Overrides the min / preferred / flexible sizes a layout group sees (`None` = not set).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutElement {
    pub enabled: bool,
    /// Layout groups skip this object.
    pub ignore_layout: bool,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub preferred_width: Option<f32>,
    pub preferred_height: Option<f32>,
    pub flexible_width: Option<f32>,
    pub flexible_height: Option<f32>,
}

impl Default for LayoutElement {
    fn default() -> Self {
        Self {
            enabled: true,
            ignore_layout: false,
            min_width: None,
            min_height: None,
            preferred_width: None,
            preferred_height: None,
            flexible_width: None,
            flexible_height: None,
        }
    }
}

impl SceneComponent for HorizontalLayoutGroup {
    const NAME: &'static str = "Horizontal Layout Group";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for VerticalLayoutGroup {
    const NAME: &'static str = "Vertical Layout Group";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for GridLayoutGroup {
    const NAME: &'static str = "Grid Layout Group";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for ContentSizeFitter {
    const NAME: &'static str = "Content Size Fitter";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for LayoutElement {
    const NAME: &'static str = "Layout Element";
    const CATEGORY: &'static str = "Layout";
}
