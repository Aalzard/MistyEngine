//! UI components: Canvas, Canvas Scaler, Rect Transform, Image, Text and Button.
//! Layout and drawing live in `crate::ui`.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::components::entity_bits;
use super::registry::SceneComponent;

/// How a Canvas is placed on screen (only the overlay mode exists so far).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanvasRenderMode {
    /// Drawn on top of the camera image, sized to the screen.
    #[default]
    ScreenSpaceOverlay,
}

impl CanvasRenderMode {
    pub const ALL: [Self; 1] = [Self::ScreenSpaceOverlay];

    pub fn label(self) -> &'static str {
        match self {
            Self::ScreenSpaceOverlay => "Screen Space - Overlay",
        }
    }
}

/// Root of a UI: everything below it with a Rect Transform is laid out on screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Canvas {
    pub enabled: bool,
    pub render_mode: CanvasRenderMode,
    /// Canvases with a higher sort order draw on top (and get clicks first).
    pub sort_order: i32,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            enabled: true,
            render_mode: CanvasRenderMode::ScreenSpaceOverlay,
            sort_order: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiScaleMode {
    /// One canvas unit = `scale_factor` screen pixels.
    ConstantPixelSize,
    /// Scale so the reference resolution fills the screen (blending width/height by `match`).
    #[default]
    ScaleWithScreenSize,
}

impl UiScaleMode {
    pub const ALL: [Self; 2] = [Self::ConstantPixelSize, Self::ScaleWithScreenSize];

    pub fn label(self) -> &'static str {
        match self {
            Self::ConstantPixelSize => "Constant Pixel Size",
            Self::ScaleWithScreenSize => "Scale With Screen Size",
        }
    }
}

/// Canvas Scaler: how canvas units map to screen pixels.
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
}

impl Default for CanvasScaler {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: UiScaleMode::ScaleWithScreenSize,
            scale_factor: 1.0,
            reference_resolution: Vec2::new(800.0, 600.0),
            match_width_or_height: 0.0,
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
}

impl Default for UiImage {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [1.0; 4],
            texture: None,
            raycast_target: true,
        }
    }
}

/// Text drawn with the built-in font.
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
        }
    }
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
    pub color_multiplier: f32,
    pub on_click: Vec<ButtonCall>,
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
            color_multiplier: 1.0,
            on_click: Vec::new(),
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
