//! Pointer raycasts against UI graphics and the selectable widgets that handle them
//! (Button, Toggle, Slider, Scrollbar, Input Field, Dropdown), with their Colour Tint states.

use glam::Vec2;
use hecs::Entity;

use super::tree::UiLayout;
use crate::scene::{
    ColorBlock, Scene, Selectable, UiButton, UiDropdown, UiImage, UiInputField, UiRawImage,
    UiScrollbar, UiSlider, UiText, UiToggle,
};

pub use super::events::UiInteraction;

/// Colour Tint state of a selectable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    Normal,
    Highlighted,
    Pressed,
    Selected,
    Disabled,
}

/// Graphics that receive clicks: enabled Image/Raw Image/Text with Raycast Target on.
fn is_raycast_target(scene: &Scene, e: Entity) -> bool {
    let w = &scene.world;
    w.get::<&UiImage>(e)
        .is_ok_and(|i| i.enabled && i.raycast_target)
        || w.get::<&UiRawImage>(e)
            .is_ok_and(|i| i.enabled && i.raycast_target)
        || w.get::<&UiText>(e)
            .is_ok_and(|t| t.enabled && t.raycast_target)
}

/// Topmost raycast-target graphic under `p` (screen pixels, y down). Masks clip hits too.
pub fn raycast(layout: &UiLayout, scene: &Scene, p: Vec2) -> Option<Entity> {
    layout
        .nodes
        .iter()
        .rev()
        .find(|n| {
            n.screen.contains(p)
                && n.clip.is_none_or(|c| c.contains(p))
                && is_raycast_target(scene, n.entity)
        })
        .map(|n| n.entity)
}

/// `e` itself or its nearest ancestor matching `f`.
pub fn ancestor(scene: &Scene, e: Entity, f: impl Fn(&Scene, Entity) -> bool) -> Option<Entity> {
    let mut cur = Some(e);
    while let Some(x) = cur {
        if f(scene, x) {
            return Some(x);
        }
        cur = scene.parent(x);
    }
    None
}

/// Selectable settings of a widget (a Button's colours become a block targeting itself).
pub fn selectable(scene: &Scene, e: Entity) -> Option<Selectable> {
    let w = &scene.world;
    if let Ok(b) = w.get::<&UiButton>(e) {
        return Some(Selectable {
            interactable: b.interactable,
            target_graphic: Some(e),
            colors: ColorBlock {
                normal: b.normal_color,
                highlighted: b.highlighted_color,
                pressed: b.pressed_color,
                selected: b.selected_color,
                disabled: b.disabled_color,
                multiplier: b.color_multiplier,
            },
        });
    }
    w.get::<&UiToggle>(e)
        .map(|c| c.selectable)
        .or_else(|_| w.get::<&UiSlider>(e).map(|c| c.selectable))
        .or_else(|_| w.get::<&UiScrollbar>(e).map(|c| c.selectable))
        .or_else(|_| w.get::<&UiInputField>(e).map(|c| c.selectable))
        .or_else(|_| w.get::<&UiDropdown>(e).map(|c| c.selectable))
        .ok()
}

pub fn is_selectable(scene: &Scene, e: Entity) -> bool {
    selectable(scene, e).is_some()
}

pub fn interactable(scene: &Scene, e: Entity) -> bool {
    selectable(scene, e).is_some_and(|s| s.interactable)
}

/// Tint colour for a state (Colour Tint transition), multiplier applied.
pub fn tint_color(c: &ColorBlock, state: ButtonState) -> [f32; 4] {
    let col = match state {
        ButtonState::Normal => c.normal,
        ButtonState::Highlighted => c.highlighted,
        ButtonState::Pressed => c.pressed,
        ButtonState::Selected => c.selected,
        ButtonState::Disabled => c.disabled,
    };
    col.map(|v| (v * c.multiplier).clamp(0.0, 1.0))
}

/// Tint colour of a Button in a state.
pub fn tint(button: &UiButton, state: ButtonState) -> [f32; 4] {
    let c = ColorBlock {
        normal: button.normal_color,
        highlighted: button.highlighted_color,
        pressed: button.pressed_color,
        selected: button.selected_color,
        disabled: button.disabled_color,
        multiplier: button.color_multiplier,
    };
    tint_color(&c, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Canvas, CanvasScaler, RectTransform, UiScaleMode};
    use crate::ui::events::PointerInput;
    use crate::ui::tree::layout;

    /// 800×600 canvas at scale 1 with a 160×30 button (Image + Text child) in the middle.
    fn scene() -> (Scene, Entity, Entity) {
        let mut s = Scene::new("UI");
        let canvas = s.spawn("Canvas", None);
        s.add(canvas, Canvas::default());
        s.add(
            canvas,
            CanvasScaler {
                mode: UiScaleMode::ConstantPixelSize,
                ..Default::default()
            },
        );
        let button = s.spawn("Button", Some(canvas));
        s.add(button, RectTransform::sized(160.0, 30.0));
        s.add(button, UiImage::default());
        s.add(button, UiButton::default());
        let label = s.spawn("Text", Some(button));
        s.add(label, RectTransform::stretched());
        s.add(label, UiText::default());
        (s, button, label)
    }

    fn at(p: Vec2, down: bool, up: bool) -> PointerInput {
        PointerInput {
            position: Some(p),
            down,
            held: down,
            up,
            screen: Vec2::new(800.0, 600.0),
            ..Default::default()
        }
    }

    #[test]
    fn press_and_release_over_a_button_clicks_it() {
        let (mut s, button, label) = scene();
        let l = layout(&s, Vec2::new(800.0, 600.0));
        let centre = Vec2::new(400.0, 300.0);
        assert_eq!(raycast(&l, &s, centre), Some(label), "text is on top");
        let mut ui = UiInteraction::default();
        assert!(ui.update(&mut s, &at(centre, false, false)).is_empty());
        assert_eq!(ui.state(&s, button), ButtonState::Highlighted);
        ui.update(&mut s, &at(centre, true, false));
        assert_eq!(ui.state(&s, button), ButtonState::Pressed);
        let ev = ui.update(&mut s, &at(centre, false, true));
        assert_eq!(ev, [crate::ui::UiEvent::Click(button)]);
        assert_eq!(ui.pressed, None);
        assert_eq!(
            ui.state(&s, button),
            ButtonState::Selected,
            "clicked = selected"
        );
        ui.update(&mut s, &at(Vec2::new(5.0, 5.0), true, false));
        assert_eq!(ui.selected, None, "clicking empty space deselects");
        assert_eq!(
            UiInteraction::state_of(None, &s, button),
            ButtonState::Normal
        );
    }

    #[test]
    fn releasing_elsewhere_or_disabled_does_not_click() {
        let (mut s, button, _) = scene();
        let mut ui = UiInteraction::default();
        ui.update(&mut s, &at(Vec2::new(400.0, 300.0), true, false));
        let outside = Vec2::new(10.0, 10.0);
        assert!(ui.update(&mut s, &at(outside, false, true)).is_empty());
        assert_eq!(ui.hovered, None);

        s.world.get::<&mut UiButton>(button).unwrap().interactable = false;
        ui.update(&mut s, &at(Vec2::new(400.0, 300.0), true, false));
        assert!(
            ui.update(&mut s, &at(Vec2::new(400.0, 300.0), false, true))
                .is_empty()
        );
        assert_eq!(ui.state(&s, button), ButtonState::Disabled);
    }

    #[test]
    fn graphics_without_raycast_target_let_clicks_through() {
        let (s, button, label) = scene();
        s.world.get::<&mut UiText>(label).unwrap().raycast_target = false;
        let l = layout(&s, Vec2::new(800.0, 600.0));
        assert_eq!(raycast(&l, &s, Vec2::new(400.0, 300.0)), Some(button));
        let b = UiButton::default();
        assert_eq!(tint(&b, ButtonState::Normal), [1.0; 4]);
        assert!(tint(&b, ButtonState::Disabled)[3] < 1.0);
    }
}
