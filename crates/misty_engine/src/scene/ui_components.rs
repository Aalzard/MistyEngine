//! UI components: Canvas, Canvas Scaler, Rect Transform, Image, Text and Button.
//! Layout and drawing live in `crate::ui`.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::components::entity_bits;
use super::registry::SceneComponent;
use super::ui_image::{ImageFill, ImageType};

/// How a Canvas is placed: on top of the image, on a plane in front of a camera, or in the world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanvasRenderMode {
    /// Drawn on top of the camera image, sized to the screen.
    #[default]
    ScreenSpaceOverlay,
    /// Sized to the screen but placed `plane_distance` in front of the Render Camera, so scene
    /// objects nearer than the plane draw over it. Unlit, sorted with the scene. Without a
    /// camera it behaves like an overlay.
    ScreenSpaceCamera,
    /// A rectangle in the world (its Transform places it, its Rect Transform sizes it), drawn
    /// with depth and clicked through the camera.
    WorldSpace,
}

impl CanvasRenderMode {
    pub const ALL: [Self; 3] = [
        Self::ScreenSpaceOverlay,
        Self::ScreenSpaceCamera,
        Self::WorldSpace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ScreenSpaceOverlay => "Screen Space - Overlay",
            Self::ScreenSpaceCamera => "Screen Space - Camera",
            Self::WorldSpace => "World Space",
        }
    }
}

fn is_plane_distance(v: &f32) -> bool {
    *v == Canvas::PLANE_DISTANCE
}

/// Root of a UI: everything below it with a Rect Transform is laid out on screen (or on the
/// canvas' plane). The camera settings are left out of files while at their defaults, so older
/// scenes re-save unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Canvas {
    pub enabled: bool,
    pub render_mode: CanvasRenderMode,
    /// Canvases with a higher sort order draw on top (and get clicks first).
    pub sort_order: i32,
    /// Screen Space - Camera: the Render Camera the canvas sits in front of.
    #[serde(with = "entity_bits::option", skip_serializing_if = "Option::is_none")]
    pub world_camera: Option<hecs::Entity>,
    /// Screen Space - Camera: distance of the canvas plane from the camera.
    #[serde(skip_serializing_if = "is_plane_distance")]
    pub plane_distance: f32,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            enabled: true,
            render_mode: CanvasRenderMode::ScreenSpaceOverlay,
            sort_order: 0,
            world_camera: None,
            plane_distance: Self::PLANE_DISTANCE,
        }
    }
}

impl Canvas {
    pub const PLANE_DISTANCE: f32 = 100.0;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiScaleMode {
    /// One canvas unit = `scale_factor` screen pixels.
    ConstantPixelSize,
    /// Scale so the reference resolution fills the screen (see [`ScreenMatchMode`]).
    #[default]
    ScaleWithScreenSize,
    /// Canvas units are a physical length (centimetres, points, …) at the screen's DPI.
    ConstantPhysicalSize,
}

impl UiScaleMode {
    pub const ALL: [Self; 3] = [
        Self::ConstantPixelSize,
        Self::ScaleWithScreenSize,
        Self::ConstantPhysicalSize,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ConstantPixelSize => "Constant Pixel Size",
            Self::ScaleWithScreenSize => "Scale With Screen Size",
            Self::ConstantPhysicalSize => "Constant Physical Size",
        }
    }
}

/// How Scale With Screen Size fits the reference resolution to a screen of another shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenMatchMode {
    /// Blend between matching the width and the height (`match_width_or_height`).
    #[default]
    MatchWidthOrHeight,
    /// The canvas is never smaller than the reference (scale by the smaller ratio).
    Expand,
    /// The canvas is never larger than the reference (scale by the larger ratio).
    Shrink,
}

impl ScreenMatchMode {
    pub const ALL: [Self; 3] = [Self::MatchWidthOrHeight, Self::Expand, Self::Shrink];

    pub fn label(self) -> &'static str {
        match self {
            Self::MatchWidthOrHeight => "Match Width Or Height",
            Self::Expand => "Expand",
            Self::Shrink => "Shrink",
        }
    }
}

/// Length unit of Constant Physical Size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicalUnit {
    Centimeters,
    Millimeters,
    Inches,
    #[default]
    Points,
    Picas,
}

impl PhysicalUnit {
    pub const ALL: [Self; 5] = [
        Self::Centimeters,
        Self::Millimeters,
        Self::Inches,
        Self::Points,
        Self::Picas,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Centimeters => "Centimeters",
            Self::Millimeters => "Millimeters",
            Self::Inches => "Inches",
            Self::Points => "Points",
            Self::Picas => "Picas",
        }
    }

    /// How many of this unit make an inch.
    pub fn per_inch(self) -> f32 {
        match self {
            Self::Centimeters => 2.54,
            Self::Millimeters => 25.4,
            Self::Inches => 1.0,
            Self::Points => 72.0,
            Self::Picas => 6.0,
        }
    }
}

fn is_match_mode(m: &ScreenMatchMode) -> bool {
    *m == ScreenMatchMode::MatchWidthOrHeight
}

fn is_points(u: &PhysicalUnit) -> bool {
    *u == PhysicalUnit::Points
}

fn is_96(v: &f32) -> bool {
    *v == 96.0
}

fn is_100(v: &f32) -> bool {
    *v == 100.0
}

fn is_unit_f32(v: &f32) -> bool {
    *v == 1.0
}

/// Canvas Scaler: how canvas units map to screen pixels. Settings added with the later modes are
/// left out of files while at their defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasScaler {
    pub enabled: bool,
    pub mode: UiScaleMode,
    /// Constant Pixel Size factor.
    pub scale_factor: f32,
    pub reference_resolution: Vec2,
    /// 0 = match width, 1 = match height.
    pub match_width_or_height: f32,
    #[serde(skip_serializing_if = "is_match_mode")]
    pub screen_match_mode: ScreenMatchMode,
    #[serde(skip_serializing_if = "is_points")]
    pub physical_unit: PhysicalUnit,
    /// Constant Physical Size: DPI assumed when the screen's is unknown.
    #[serde(skip_serializing_if = "is_96")]
    pub fallback_screen_dpi: f32,
    /// Constant Physical Size: DPI sprites are authored at.
    #[serde(skip_serializing_if = "is_96")]
    pub default_sprite_dpi: f32,
    /// Sprite pixels per canvas unit for a sprite of 100 Pixels Per Unit (sliced borders, tiles).
    #[serde(skip_serializing_if = "is_100")]
    pub reference_pixels_per_unit: f32,
    /// World Space canvases: pixels per canvas unit that text and masks are made at.
    #[serde(skip_serializing_if = "is_unit_f32")]
    pub dynamic_pixels_per_unit: f32,
}

impl Default for CanvasScaler {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: UiScaleMode::ScaleWithScreenSize,
            scale_factor: 1.0,
            reference_resolution: Vec2::new(800.0, 600.0),
            match_width_or_height: 0.0,
            screen_match_mode: ScreenMatchMode::MatchWidthOrHeight,
            physical_unit: PhysicalUnit::Points,
            fallback_screen_dpi: 96.0,
            default_sprite_dpi: 96.0,
            reference_pixels_per_unit: 100.0,
            dynamic_pixels_per_unit: 1.0,
        }
    }
}

/// 2D layout rectangle of a UI object, relative to its parent's rectangle (y up, like the
/// familiar Rect Transform). Lives beside the Transform, which UI layout ignores.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RectTransform {
    /// Normalised parent position of the lower-left corner's anchor.
    pub anchor_min: Vec2,
    /// Normalised parent position of the upper-right corner's anchor.
    pub anchor_max: Vec2,
    /// Normalised point of this rect that `anchored_position` places.
    pub pivot: Vec2,
    /// Pivot offset from the anchor reference point.
    pub anchored_position: Vec2,
    /// Size added to the distance between the anchors.
    pub size_delta: Vec2,
}

impl Default for RectTransform {
    fn default() -> Self {
        Self {
            anchor_min: Vec2::splat(0.5),
            anchor_max: Vec2::splat(0.5),
            pivot: Vec2::splat(0.5),
            anchored_position: Vec2::ZERO,
            size_delta: Vec2::new(100.0, 100.0),
        }
    }
}

impl RectTransform {
    /// Fixed-size rect centred on its anchor point.
    pub fn sized(width: f32, height: f32) -> Self {
        Self {
            size_delta: Vec2::new(width, height),
            ..Default::default()
        }
    }

    /// Rect filling its parent.
    pub fn stretched() -> Self {
        Self {
            anchor_min: Vec2::ZERO,
            anchor_max: Vec2::ONE,
            size_delta: Vec2::ZERO,
            ..Default::default()
        }
    }
}

/// Where text sits inside its rect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAnchor {
    #[default]
    UpperLeft,
    UpperCenter,
    UpperRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    LowerLeft,
    LowerCenter,
    LowerRight,
}

impl TextAnchor {
    pub const ALL: [Self; 9] = [
        Self::UpperLeft,
        Self::UpperCenter,
        Self::UpperRight,
        Self::MiddleLeft,
        Self::MiddleCenter,
        Self::MiddleRight,
        Self::LowerLeft,
        Self::LowerCenter,
        Self::LowerRight,
    ];

    /// Column (0 left, 1 centre, 2 right) and row (0 upper, 1 middle, 2 lower).
    pub fn cell(self) -> (usize, usize) {
        let i = Self::ALL.iter().position(|a| *a == self).unwrap_or(0);
        (i % 3, i / 3)
    }

    pub fn from_cell(column: usize, row: usize) -> Self {
        Self::ALL[row.min(2) * 3 + column.min(2)]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::UpperLeft => "Upper Left",
            Self::UpperCenter => "Upper Center",
            Self::UpperRight => "Upper Right",
            Self::MiddleLeft => "Middle Left",
            Self::MiddleCenter => "Middle Center",
            Self::MiddleRight => "Middle Right",
            Self::LowerLeft => "Lower Left",
            Self::LowerCenter => "Lower Center",
            Self::LowerRight => "Lower Right",
        }
    }
}

/// Coloured (optionally textured) rectangle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiImage {
    pub enabled: bool,
    /// sRGB colour + alpha (multiplies the texture).
    pub color: [f32; 4],
    /// Texture asset path ("Assets/Textures/Panel.png"); `None` = plain colour.
    pub texture: Option<String>,
    /// Blocks and receives clicks.
    pub raycast_target: bool,
    /// Sprite reference (see `crate::sprites`); when set it replaces `texture` and only its rect
    /// is drawn. Left out of files when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<String>,
    /// Simple, Sliced, Tiled or Filled (the settings below are left out of files at their
    /// defaults).
    #[serde(skip_serializing_if = "is_simple")]
    pub image_type: ImageType,
    /// Sliced / Tiled: draw the middle part too.
    #[serde(skip_serializing_if = "is_true")]
    pub fill_center: bool,
    /// Filled: method, origin, amount and direction.
    #[serde(skip_serializing_if = "ImageFill::is_default")]
    pub fill: ImageFill,
    /// Simple / Filled: shrink the drawn rect to the sprite's width / height ratio.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub preserve_aspect: bool,
    /// Sliced / Tiled: multiplies the sprite's pixels per unit (2 = borders and tiles half as
    /// large).
    #[serde(skip_serializing_if = "is_unit_f32")]
    pub pixels_per_unit_multiplier: f32,
}

fn is_simple(t: &ImageType) -> bool {
    *t == ImageType::Simple
}

impl Default for UiImage {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [1.0; 4],
            texture: None,
            raycast_target: true,
            sprite: None,
            image_type: ImageType::Simple,
            fill_center: true,
            fill: ImageFill::default(),
            preserve_aspect: false,
            pixels_per_unit_multiplier: 1.0,
        }
    }
}

/// What happens to text that doesn't fit its rect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextOverflow {
    /// Draw past the rect.
    #[default]
    Overflow,
    /// Cut the last line that fits with "…".
    Ellipsis,
    /// Hide lines that don't fit.
    Truncate,
    /// Show one rect-full at a time (`page` picks which).
    Page,
}

impl TextOverflow {
    pub const ALL: [TextOverflow; 4] = [
        TextOverflow::Overflow,
        TextOverflow::Ellipsis,
        TextOverflow::Truncate,
        TextOverflow::Page,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextOverflow::Overflow => "Overflow",
            TextOverflow::Ellipsis => "Ellipsis",
            TextOverflow::Truncate => "Truncate",
            TextOverflow::Page => "Page",
        }
    }
}

/// Outline, drop shadow and gradient of a text (drawn from its distance field glyphs).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextEffects {
    /// Outline thickness in canvas units (0 = none).
    pub outline_width: f32,
    pub outline_color: [f32; 4],
    pub shadow: bool,
    /// Shadow offset in canvas units (x right, y up).
    pub shadow_offset: [f32; 2],
    pub shadow_color: [f32; 4],
    /// Shadow blur, 0 (sharp) to 1 (very soft).
    pub shadow_softness: f32,
    /// Colour each glyph with a gradient (multiplied with the text colour).
    pub gradient: bool,
    /// Top-left, top-right, bottom-left, bottom-right colours.
    pub gradient_colors: [[f32; 4]; 4],
}

impl Default for TextEffects {
    fn default() -> Self {
        Self {
            outline_width: 0.0,
            outline_color: [0.0, 0.0, 0.0, 1.0],
            shadow: false,
            shadow_offset: [1.5, -1.5],
            shadow_color: [0.0, 0.0, 0.0, 0.5],
            shadow_softness: 0.2,
            gradient: false,
            gradient_colors: [
                [1.0; 4],
                [1.0; 4],
                [0.6, 0.6, 0.6, 1.0],
                [0.6, 0.6, 0.6, 1.0],
            ],
        }
    }
}

impl TextEffects {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Any effect switched on?
    pub fn any(&self) -> bool {
        self.outline_width > 0.0 || self.shadow || self.gradient
    }
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

fn is_default_overflow(o: &TextOverflow) -> bool {
    *o == TextOverflow::Overflow
}

fn is_min_size(v: &f32) -> bool {
    *v == UiText::AUTO_MIN
}

fn is_max_size(v: &f32) -> bool {
    *v == UiText::AUTO_MAX
}

/// Text: the built-in font or a font asset, with rich text markup, effects and overflow modes.
/// The rich text settings are left out of files while at their defaults, so older scenes load
/// and save unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiText {
    pub enabled: bool,
    pub text: String,
    /// Em size in canvas units.
    pub font_size: f32,
    pub line_spacing: f32,
    /// sRGB colour + alpha.
    pub color: [f32; 4],
    pub alignment: TextAnchor,
    /// Wrap lines at the rect's width (else they overflow it).
    pub wrap: bool,
    pub raycast_target: bool,
    /// Interpret `<b>`, `<color=…>`, `<link=…>` and the other tags.
    #[serde(skip_serializing_if = "is_true")]
    pub rich_text: bool,
    /// Font asset (`.ttf` / `.otf`); None = the built-in font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Texture whose sliced sprites `<sprite name=…>` shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_asset: Option<String>,
    /// Extra space after each character, hundredths of an em.
    #[serde(skip_serializing_if = "is_zero")]
    pub character_spacing: f32,
    /// Extra space added to spaces, hundredths of an em.
    #[serde(skip_serializing_if = "is_zero")]
    pub word_spacing: f32,
    #[serde(skip_serializing_if = "is_default_overflow")]
    pub overflow: TextOverflow,
    /// Page shown in Page overflow mode (1 = first).
    #[serde(skip_serializing_if = "is_one")]
    pub page: u32,
    /// Pick the largest size between the limits that fits the rect.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_size: bool,
    #[serde(skip_serializing_if = "is_min_size")]
    pub font_size_min: f32,
    #[serde(skip_serializing_if = "is_max_size")]
    pub font_size_max: f32,
    #[serde(skip_serializing_if = "TextEffects::is_default")]
    pub effects: TextEffects,
}

impl Default for UiText {
    fn default() -> Self {
        let grey = 50.0 / 255.0;
        Self {
            enabled: true,
            text: "New Text".into(),
            font_size: 14.0,
            line_spacing: 1.0,
            color: [grey, grey, grey, 1.0],
            alignment: TextAnchor::UpperLeft,
            wrap: true,
            raycast_target: true,
            rich_text: true,
            font: None,
            sprite_asset: None,
            character_spacing: 0.0,
            word_spacing: 0.0,
            overflow: TextOverflow::Overflow,
            page: 1,
            auto_size: false,
            font_size_min: Self::AUTO_MIN,
            font_size_max: Self::AUTO_MAX,
            effects: TextEffects::default(),
        }
    }
}

impl UiText {
    pub const AUTO_MIN: f32 = 10.0;
    pub const AUTO_MAX: f32 = 72.0;
}

/// One On Click entry: call `method` on the scripts of `target`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ButtonCall {
    /// Scene files store the target's object index (see `scene::file`).
    #[serde(with = "entity_bits::option")]
    pub target: Option<hecs::Entity>,
    /// Lua method name on the target's scripts.
    pub method: String,
}

/// Clickable button: tints the Image on the same object and runs its On Click calls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiButton {
    pub interactable: bool,
    /// Colour Tint transition colours (sRGB + alpha, multiplied with the Image colour).
    pub normal_color: [f32; 4],
    pub highlighted_color: [f32; 4],
    pub pressed_color: [f32; 4],
    pub disabled_color: [f32; 4],
    /// Colour while selected (last clicked); left out of files while at its default so older
    /// scenes re-save unchanged.
    #[serde(skip_serializing_if = "is_default_selected")]
    pub selected_color: [f32; 4],
    pub color_multiplier: f32,
    pub on_click: Vec<ButtonCall>,
    /// Keyboard / gamepad navigation (left out of files at its defaults).
    #[serde(flatten)]
    pub navigation: super::ui_widgets::Navigation,
}

const DEFAULT_SELECTED: [f32; 4] = [245.0 / 255.0, 245.0 / 255.0, 245.0 / 255.0, 1.0];

fn is_default_selected(c: &[f32; 4]) -> bool {
    *c == DEFAULT_SELECTED
}

impl Default for UiButton {
    fn default() -> Self {
        let g = |v: f32| v / 255.0;
        Self {
            interactable: true,
            normal_color: [1.0; 4],
            highlighted_color: [g(245.0), g(245.0), g(245.0), 1.0],
            pressed_color: [g(200.0), g(200.0), g(200.0), 1.0],
            disabled_color: [g(200.0), g(200.0), g(200.0), 0.5],
            selected_color: DEFAULT_SELECTED,
            color_multiplier: 1.0,
            on_click: Vec::new(),
            navigation: Default::default(),
        }
    }
}

// ---- registry --------------------------------------------------------------------------------

impl SceneComponent for RectTransform {
    const NAME: &'static str = "Rect Transform";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for Canvas {
    const NAME: &'static str = "Canvas";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for CanvasScaler {
    const NAME: &'static str = "Canvas Scaler";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for UiImage {
    const NAME: &'static str = "Image";
    const CATEGORY: &'static str = "UI";
}
impl SceneComponent for UiText {
    const NAME: &'static str = "Text";
    const CATEGORY: &'static str = "UI";
}
impl SceneComponent for UiButton {
    const NAME: &'static str = "Button";
    const CATEGORY: &'static str = "UI";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;

    fn ui_scene() -> Scene {
        let mut s = Scene::new("UI");
        let target = s.spawn("Target", None);
        let canvas = s.spawn("Canvas", None);
        s.add(canvas, RectTransform::stretched());
        s.add(
            canvas,
            Canvas {
                sort_order: 3,
                ..Default::default()
            },
        );
        s.add(
            canvas,
            CanvasScaler {
                reference_resolution: Vec2::new(1920.0, 1080.0),
                match_width_or_height: 0.5,
                ..Default::default()
            },
        );
        let button = s.spawn("Button", Some(canvas));
        s.add(button, RectTransform::sized(160.0, 30.0));
        s.add(
            button,
            UiImage {
                color: [0.5, 0.25, 1.0, 0.75],
                texture: Some("Assets/Textures/Checker.png".into()),
                raycast_target: false,
                ..Default::default()
            },
        );
        s.add(
            button,
            UiButton {
                interactable: false,
                on_click: vec![
                    ButtonCall {
                        target: Some(target),
                        method: "Clicked".into(),
                    },
                    ButtonCall::default(),
                ],
                ..Default::default()
            },
        );
        let label = s.spawn("Text", Some(button));
        s.add(label, RectTransform::stretched());
        s.add(
            label,
            UiText {
                text: "Two\nlines".into(),
                alignment: TextAnchor::LowerRight,
                wrap: false,
                font_size: 22.0,
                ..Default::default()
            },
        );
        s
    }

    #[test]
    fn ui_components_round_trip_through_scene_files() {
        let scene = ui_scene();
        let json = scene.to_json();
        assert!(
            json.contains("\"target\": 0"),
            "references saved as indices"
        );
        let (loaded, report) = Scene::from_json("UI", &json).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(loaded.to_json(), json, "byte-identical after a round trip");
        let button = loaded.find_by_name("Button").unwrap();
        let b = loaded.world.get::<&UiButton>(button).unwrap();
        assert_eq!(b.on_click[0].target, loaded.find_by_name("Target"));
        assert_eq!(b.on_click[1].target, None);
        let label = loaded.find_by_name("Text").unwrap();
        assert_eq!(
            loaded.world.get::<&UiText>(label).unwrap().alignment,
            TextAnchor::LowerRight
        );
        assert_eq!(
            *loaded.world.get::<&RectTransform>(label).unwrap(),
            RectTransform::stretched()
        );
    }

    #[test]
    fn missing_fields_use_defaults_and_outside_references_become_none() {
        let (s, _) = Scene::from_json(
            "Old",
            r#"{"version":1,"objects":[{"parent":null,"components":{
                "GameObject":{"name":"B"},"Button":{"on_click":[{"target":7,"method":"X"}]},
                "Text":{"text":"hi"}}}]}"#,
        )
        .unwrap();
        let e = s.find_by_name("B").unwrap();
        let b = s.world.get::<&UiButton>(e).unwrap();
        assert!(b.interactable);
        assert_eq!(b.on_click[0].target, None);
        let t = s.world.get::<&UiText>(e).unwrap();
        assert_eq!((t.text.as_str(), t.font_size), ("hi", 14.0));
    }

    #[test]
    fn duplicates_keep_their_click_target() {
        let mut scene = ui_scene();
        let button = scene.find_by_name("Button").unwrap();
        let copy = scene.duplicate(button);
        let target = scene.find_by_name("Target");
        assert_eq!(
            scene.world.get::<&UiButton>(copy).unwrap().on_click[0].target,
            target
        );
    }
}
