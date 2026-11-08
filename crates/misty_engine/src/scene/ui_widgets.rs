//! Interactive UI widgets: Toggle (+ Toggle Group), Slider, Scrollbar, Input Field, Dropdown,
//! Scroll Rect, Mask / Rect Mask 2D, Raw Image and the Event System object.
//! Behaviour lives in `crate::ui` (events, driven rects, drawing).

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::components::entity_bits;
use super::registry::SceneComponent;
use super::ui_components::ButtonCall;

/// Colour Tint transition colours of a selectable widget (sRGB + alpha).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorBlock {
    pub normal: [f32; 4],
    pub highlighted: [f32; 4],
    pub pressed: [f32; 4],
    pub selected: [f32; 4],
    pub disabled: [f32; 4],
    pub multiplier: f32,
}

impl Default for ColorBlock {
    fn default() -> Self {
        let g = |v: f32| v / 255.0;
        Self {
            normal: [1.0; 4],
            highlighted: [g(245.0), g(245.0), g(245.0), 1.0],
            pressed: [g(200.0), g(200.0), g(200.0), 1.0],
            selected: [g(245.0), g(245.0), g(245.0), 1.0],
            disabled: [g(200.0), g(200.0), g(200.0), 0.5],
            multiplier: 1.0,
        }
    }
}

/// How keyboard / gamepad navigation leaves a selectable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationMode {
    /// Never selected by navigation.
    None,
    /// Only left / right move on (to the best selectable in that direction).
    Horizontal,
    /// Only up / down move on.
    Vertical,
    /// The best selectable in each direction.
    #[default]
    Automatic,
    /// The selectables named in the Select On fields.
    Explicit,
}

impl NavigationMode {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Horizontal,
        Self::Vertical,
        Self::Automatic,
        Self::Explicit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Automatic => "Automatic",
            Self::Explicit => "Explicit",
        }
    }
}

fn is_automatic(m: &NavigationMode) -> bool {
    *m == NavigationMode::Automatic
}

/// Navigation settings of a selectable, stored flat in it and left out of files at their
/// defaults (Automatic, no explicit targets).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Navigation {
    #[serde(rename = "navigation", skip_serializing_if = "is_automatic")]
    pub mode: NavigationMode,
    /// Horizontal / Vertical: moving past the last selectable comes back at the first.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub wrap_around: bool,
    #[serde(with = "entity_bits::option", skip_serializing_if = "Option::is_none")]
    pub select_on_up: Option<hecs::Entity>,
    #[serde(with = "entity_bits::option", skip_serializing_if = "Option::is_none")]
    pub select_on_down: Option<hecs::Entity>,
    #[serde(with = "entity_bits::option", skip_serializing_if = "Option::is_none")]
    pub select_on_left: Option<hecs::Entity>,
    #[serde(with = "entity_bits::option", skip_serializing_if = "Option::is_none")]
    pub select_on_right: Option<hecs::Entity>,
}

/// What every selectable widget has: Interactable, the graphic it tints, its colours and its
/// navigation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Selectable {
    pub interactable: bool,
    /// Image (or Text) tinted by the Colour Tint transition.
    #[serde(with = "entity_bits::option")]
    pub target_graphic: Option<hecs::Entity>,
    pub colors: ColorBlock,
    #[serde(flatten)]
    pub navigation: Navigation,
}

impl Default for Selectable {
    fn default() -> Self {
        Self {
            interactable: true,
            target_graphic: None,
            colors: ColorBlock::default(),
            navigation: Navigation::default(),
        }
    }
}

/// On/off switch; `graphic` (the checkmark) is shown while on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiToggle {
    pub selectable: Selectable,
    pub is_on: bool,
    #[serde(with = "entity_bits::option")]
    pub graphic: Option<hecs::Entity>,
    /// Object with a Toggle Group: at most one toggle of a group is on.
    #[serde(with = "entity_bits::option")]
    pub group: Option<hecs::Entity>,
    /// Methods called with the new `is_on` value.
    pub on_value_changed: Vec<ButtonCall>,
}

impl Default for UiToggle {
    fn default() -> Self {
        Self {
            selectable: Selectable::default(),
            is_on: true,
            graphic: None,
            group: None,
            on_value_changed: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiToggleGroup {
    /// Allow every toggle of the group to be off.
    pub allow_switch_off: bool,
}

/// Which way a slider or scrollbar grows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SliderDirection {
    #[default]
    LeftToRight,
    RightToLeft,
    BottomToTop,
    TopToBottom,
}

impl SliderDirection {
    pub const ALL: [Self; 4] = [
        Self::LeftToRight,
        Self::RightToLeft,
        Self::BottomToTop,
        Self::TopToBottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::LeftToRight => "Left To Right",
            Self::RightToLeft => "Right To Left",
            Self::BottomToTop => "Bottom To Top",
            Self::TopToBottom => "Top To Bottom",
        }
    }

    /// 0 = horizontal, 1 = vertical.
    pub fn axis(self) -> usize {
        match self {
            Self::LeftToRight | Self::RightToLeft => 0,
            _ => 1,
        }
    }

    pub fn reversed(self) -> bool {
        matches!(self, Self::RightToLeft | Self::TopToBottom)
    }
}

/// Value between min and max picked by dragging a handle; drives the Fill and Handle rects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSlider {
    pub selectable: Selectable,
    #[serde(with = "entity_bits::option")]
    pub fill_rect: Option<hecs::Entity>,
    #[serde(with = "entity_bits::option")]
    pub handle_rect: Option<hecs::Entity>,
    pub direction: SliderDirection,
    pub min_value: f32,
    pub max_value: f32,
    pub whole_numbers: bool,
    pub value: f32,
    pub on_value_changed: Vec<ButtonCall>,
}

impl Default for UiSlider {
    fn default() -> Self {
        Self {
            selectable: Selectable::default(),
            fill_rect: None,
            handle_rect: None,
            direction: SliderDirection::LeftToRight,
            min_value: 0.0,
            max_value: 1.0,
            whole_numbers: false,
            value: 0.0,
            on_value_changed: Vec::new(),
        }
    }
}

/// Bar with a handle whose size shows how much of the content is visible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiScrollbar {
    pub selectable: Selectable,
    #[serde(with = "entity_bits::option")]
    pub handle_rect: Option<hecs::Entity>,
    pub direction: SliderDirection,
    /// 0..1 position of the handle.
    pub value: f32,
    /// 0..1 size of the handle.
    pub size: f32,
    /// 0 = continuous, else snap to this many positions.
    pub number_of_steps: u32,
    pub on_value_changed: Vec<ButtonCall>,
}

impl Default for UiScrollbar {
    fn default() -> Self {
        Self {
            selectable: Selectable::default(),
            handle_rect: None,
            direction: SliderDirection::LeftToRight,
            value: 0.0,
            size: 0.2,
            number_of_steps: 0,
            on_value_changed: Vec::new(),
        }
    }
}

/// Characters an Input Field accepts and how it shows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputContentType {
    #[default]
    Standard,
    IntegerNumber,
    DecimalNumber,
    Password,
}

impl InputContentType {
    pub const ALL: [Self; 4] = [
        Self::Standard,
        Self::IntegerNumber,
        Self::DecimalNumber,
        Self::Password,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::IntegerNumber => "Integer Number",
            Self::DecimalNumber => "Decimal Number",
            Self::Password => "Password",
        }
    }
}

/// Single-line editable text shown through a child Text (and a Placeholder while empty).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiInputField {
    pub selectable: Selectable,
    #[serde(with = "entity_bits::option")]
    pub text_component: Option<hecs::Entity>,
    #[serde(with = "entity_bits::option")]
    pub placeholder: Option<hecs::Entity>,
    pub text: String,
    /// 0 = unlimited.
    pub character_limit: u32,
    pub content_type: InputContentType,
    /// Caret blinks per second.
    pub caret_blink_rate: f32,
    pub caret_width: f32,
    pub selection_color: [f32; 4],
    pub read_only: bool,
    /// Called with the text after every edit.
    pub on_value_changed: Vec<ButtonCall>,
    /// Called with the text when editing ends (Return or clicking elsewhere).
    pub on_end_edit: Vec<ButtonCall>,
}

impl Default for UiInputField {
    fn default() -> Self {
        Self {
            selectable: Selectable::default(),
            text_component: None,
            placeholder: None,
            text: String::new(),
            character_limit: 0,
            content_type: InputContentType::Standard,
            caret_blink_rate: 0.85,
            caret_width: 1.0,
            selection_color: [168.0 / 255.0, 206.0 / 255.0, 1.0, 192.0 / 255.0],
            read_only: false,
            on_value_changed: Vec::new(),
            on_end_edit: Vec::new(),
        }
    }
}

/// Button that opens a list of options built from its (inactive) Template child.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiDropdown {
    pub selectable: Selectable,
    #[serde(with = "entity_bits::option")]
    pub template: Option<hecs::Entity>,
    /// Text showing the chosen option.
    #[serde(with = "entity_bits::option")]
    pub caption_text: Option<hecs::Entity>,
    /// Text of the template's item (copied once per option).
    #[serde(with = "entity_bits::option")]
    pub item_text: Option<hecs::Entity>,
    pub options: Vec<String>,
    /// Index of the chosen option.
    pub value: i32,
    pub on_value_changed: Vec<ButtonCall>,
}

impl Default for UiDropdown {
    fn default() -> Self {
        Self {
            selectable: Selectable::default(),
            template: None,
            caption_text: None,
            item_text: None,
            options: vec!["Option A".into(), "Option B".into(), "Option C".into()],
            value: 0,
            on_value_changed: Vec::new(),
        }
    }
}

/// How a Scroll Rect treats content dragged past its edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovementType {
    Unrestricted,
    /// Springs back to the edge.
    #[default]
    Elastic,
    Clamped,
}

impl MovementType {
    pub const ALL: [Self; 3] = [Self::Unrestricted, Self::Elastic, Self::Clamped];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unrestricted => "Unrestricted",
            Self::Elastic => "Elastic",
            Self::Clamped => "Clamped",
        }
    }
}

/// Drag or wheel-scroll a larger Content rect inside a Viewport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiScrollRect {
    pub enabled: bool,
    #[serde(with = "entity_bits::option")]
    pub content: Option<hecs::Entity>,
    pub horizontal: bool,
    pub vertical: bool,
    pub movement_type: MovementType,
    /// Seconds the elastic spring takes to settle (roughly).
    pub elasticity: f32,
    pub inertia: bool,
    /// Fraction of the velocity kept after one second.
    pub deceleration_rate: f32,
    pub scroll_sensitivity: f32,
    /// `None` = this object's rect.
    #[serde(with = "entity_bits::option")]
    pub viewport: Option<hecs::Entity>,
    #[serde(with = "entity_bits::option")]
    pub horizontal_scrollbar: Option<hecs::Entity>,
    #[serde(with = "entity_bits::option")]
    pub vertical_scrollbar: Option<hecs::Entity>,
}

impl Default for UiScrollRect {
    fn default() -> Self {
        Self {
            enabled: true,
            content: None,
            horizontal: true,
            vertical: true,
            movement_type: MovementType::Elastic,
            elasticity: 0.1,
            inertia: true,
            deceleration_rate: 0.135,
            scroll_sensitivity: 1.0,
            viewport: None,
            horizontal_scrollbar: None,
            vertical_scrollbar: None,
        }
    }
}

/// Clips children to this object's rect (its Image is the mask shape).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiMask {
    pub enabled: bool,
    /// Draw the mask's own Image.
    pub show_mask_graphic: bool,
}

impl Default for UiMask {
    fn default() -> Self {
        Self {
            enabled: true,
            show_mask_graphic: true,
        }
    }
}

/// Clips children to this object's rect, shrunk by `padding` (left, bottom, right, top).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiRectMask2D {
    pub enabled: bool,
    pub padding: [f32; 4],
}

impl Default for UiRectMask2D {
    fn default() -> Self {
        Self {
            enabled: true,
            padding: [0.0; 4],
        }
    }
}

/// A texture drawn with a UV rectangle (no sprite).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiRawImage {
    pub enabled: bool,
    pub texture: Option<String>,
    pub color: [f32; 4],
    /// x, y, width, height in UV space.
    pub uv_rect: [f32; 4],
    pub raycast_target: bool,
}

impl Default for UiRawImage {
    fn default() -> Self {
        Self {
            enabled: true,
            texture: None,
            color: [1.0; 4],
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            raycast_target: true,
        }
    }
}

/// Settings of the scene's UI input handling (one per scene; created with the first Canvas).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventSystem {
    pub enabled: bool,
    /// Selected when Play starts.
    #[serde(with = "entity_bits::option")]
    pub first_selected: Option<hecs::Entity>,
    /// Arrow keys move the selection, Return/Space submit.
    pub send_navigation_events: bool,
    /// Pixels the pointer moves before a press becomes a drag.
    pub drag_threshold: f32,
    /// Input Actions asset whose "UI" map (Navigate, Submit, Cancel) drives navigation; `None`
    /// = the default UI actions (arrows, Return, Esc, gamepad stick / d-pad, South / East).
    /// With Active Input Handling set to the Input Manager, the keys and joystick are read
    /// directly instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions_asset: Option<String>,
    /// Seconds a held direction waits before it repeats.
    #[serde(skip_serializing_if = "is_repeat_delay")]
    pub move_repeat_delay: f32,
    /// Seconds between repeats of a held direction.
    #[serde(skip_serializing_if = "is_repeat_rate")]
    pub move_repeat_rate: f32,
}

fn is_repeat_delay(v: &f32) -> bool {
    *v == EventSystem::REPEAT_DELAY
}

fn is_repeat_rate(v: &f32) -> bool {
    *v == EventSystem::REPEAT_RATE
}

impl EventSystem {
    pub const REPEAT_DELAY: f32 = 0.5;
    pub const REPEAT_RATE: f32 = 0.1;
}

impl Default for EventSystem {
    fn default() -> Self {
        Self {
            enabled: true,
            first_selected: None,
            send_navigation_events: true,
            drag_threshold: 10.0,
            actions_asset: None,
            move_repeat_delay: Self::REPEAT_DELAY,
            move_repeat_rate: Self::REPEAT_RATE,
        }
    }
}

impl UiRawImage {
    pub fn uv_min(&self) -> Vec2 {
        Vec2::new(self.uv_rect[0], self.uv_rect[1])
    }
    pub fn uv_max(&self) -> Vec2 {
        Vec2::new(
            self.uv_rect[0] + self.uv_rect[2],
            self.uv_rect[1] + self.uv_rect[3],
        )
    }
}

// ---- registry --------------------------------------------------------------------------------

macro_rules! scene_component {
    ($($t:ty => $name:literal, $cat:literal;)*) => {
        $(impl SceneComponent for $t {
            const NAME: &'static str = $name;
            const CATEGORY: &'static str = $cat;
        })*
    };
}

scene_component! {
    UiToggle => "Toggle", "UI";
    UiToggleGroup => "Toggle Group", "UI";
    UiSlider => "Slider", "UI";
    UiScrollbar => "Scrollbar", "UI";
    UiInputField => "Input Field", "UI";
    UiDropdown => "Dropdown", "UI";
    UiScrollRect => "Scroll Rect", "UI";
    UiMask => "Mask", "UI";
    UiRectMask2D => "Rect Mask 2D", "UI";
    UiRawImage => "Raw Image", "UI";
    EventSystem => "Event System", "Event";
}

/// Object-reference fields for scene files and duplicates: (component, list, field).
pub const WIDGET_REFS: &[(&str, &str, &str)] = &[
    ("Toggle", "", "graphic"),
    ("Toggle", "", "group"),
    ("Toggle", "on_value_changed", "target"),
    ("Slider", "", "fill_rect"),
    ("Slider", "", "handle_rect"),
    ("Slider", "on_value_changed", "target"),
    ("Scrollbar", "", "handle_rect"),
    ("Scrollbar", "on_value_changed", "target"),
    ("Input Field", "", "text_component"),
    ("Input Field", "", "placeholder"),
    ("Input Field", "on_value_changed", "target"),
    ("Input Field", "on_end_edit", "target"),
    ("Dropdown", "", "template"),
    ("Dropdown", "", "caption_text"),
    ("Dropdown", "", "item_text"),
    ("Dropdown", "on_value_changed", "target"),
    ("Scroll Rect", "", "content"),
    ("Scroll Rect", "", "viewport"),
    ("Scroll Rect", "", "horizontal_scrollbar"),
    ("Scroll Rect", "", "vertical_scrollbar"),
    ("Event System", "", "first_selected"),
    ("Canvas", "", "world_camera"),
    ("Button", "", "select_on_up"),
    ("Button", "", "select_on_down"),
    ("Button", "", "select_on_left"),
    ("Button", "", "select_on_right"),
];

/// The nested references of each selectable widget: Target Graphic and the explicit
/// navigation targets.
pub const SELECTABLE_REFS: &[(&str, &str, &str)] = &[
    ("Toggle", "selectable", "target_graphic"),
    ("Slider", "selectable", "target_graphic"),
    ("Scrollbar", "selectable", "target_graphic"),
    ("Input Field", "selectable", "target_graphic"),
    ("Dropdown", "selectable", "target_graphic"),
    ("Toggle", "selectable", "select_on_up"),
    ("Toggle", "selectable", "select_on_down"),
    ("Toggle", "selectable", "select_on_left"),
    ("Toggle", "selectable", "select_on_right"),
    ("Slider", "selectable", "select_on_up"),
    ("Slider", "selectable", "select_on_down"),
    ("Slider", "selectable", "select_on_left"),
    ("Slider", "selectable", "select_on_right"),
    ("Scrollbar", "selectable", "select_on_up"),
    ("Scrollbar", "selectable", "select_on_down"),
    ("Scrollbar", "selectable", "select_on_left"),
    ("Scrollbar", "selectable", "select_on_right"),
    ("Input Field", "selectable", "select_on_up"),
    ("Input Field", "selectable", "select_on_down"),
    ("Input Field", "selectable", "select_on_left"),
    ("Input Field", "selectable", "select_on_right"),
    ("Dropdown", "selectable", "select_on_up"),
    ("Dropdown", "selectable", "select_on_down"),
    ("Dropdown", "selectable", "select_on_left"),
    ("Dropdown", "selectable", "select_on_right"),
];
