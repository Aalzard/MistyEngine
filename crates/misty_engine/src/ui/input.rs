//! Pointer events for UI: raycast against graphics, hover/press state of buttons and clicks.

use glam::Vec2;
use hecs::Entity;

use super::tree::UiLayout;
use crate::scene::{Scene, UiButton, UiImage, UiText};

/// Colour Tint state of a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    Normal,
    Highlighted,
    Pressed,
    Disabled,
}

/// Which button the pointer is over / pressed on (lives while playing).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiInteraction {
    pub hovered: Option<Entity>,
    pub pressed: Option<Entity>,
}

/// Graphics that receive clicks: enabled Image/Text with Raycast Target on.
fn is_raycast_target(scene: &Scene, e: Entity) -> bool {
    let w = &scene.world;
    w.get::<&UiImage>(e)
        .is_ok_and(|i| i.enabled && i.raycast_target)
        || w.get::<&UiText>(e)
            .is_ok_and(|t| t.enabled && t.raycast_target)
}

/// Topmost raycast-target graphic under `p` (screen pixels, y down).
pub fn raycast(layout: &UiLayout, scene: &Scene, p: Vec2) -> Option<Entity> {
    layout
        .nodes
        .iter()
        .rev()
        .find(|n| n.screen.contains(p) && is_raycast_target(scene, n.entity))
        .map(|n| n.entity)
}

/// The button handling events for `hit`: itself or its nearest ancestor with a Button.
fn button_for(scene: &Scene, hit: Entity) -> Option<Entity> {
    let mut cur = Some(hit);
    while let Some(e) = cur {
        if scene.world.get::<&UiButton>(e).is_ok() {
            return Some(e);
        }
        cur = scene.parent(e);
    }
    None
}

impl UiInteraction {
    /// Feed one frame of pointer input; returns the buttons clicked this frame.
    /// A click = press and release over the same interactable button.
    pub fn update(
        &mut self,
        layout: &UiLayout,
        scene: &Scene,
        pointer: Option<Vec2>,
        down: bool,
        up: bool,
    ) -> Vec<Entity> {
        self.hovered = pointer
            .and_then(|p| raycast(layout, scene, p))
            .and_then(|hit| button_for(scene, hit));
        let mut clicked = Vec::new();
        if down {
            self.pressed = self.hovered;
        }
        // Any release ends the press; it clicks only over the same button.
        if up
            && let Some(b) = self.pressed.take()
            && self.hovered == Some(b)
            && interactable(scene, b)
        {
            clicked.push(b);
        }
        // Destroyed or deactivated buttons forget their state.
        if self.pressed.is_some_and(|b| layout.node(b).is_none()) {
            self.pressed = None;
        }
        clicked
    }

    /// Colour Tint state of `button` (`None` interaction = edit mode: normal or disabled).
    pub fn state(this: Option<&Self>, scene: &Scene, button: Entity) -> ButtonState {
        if !interactable(scene, button) {
            return ButtonState::Disabled;
        }
        let Some(i) = this else {
            return ButtonState::Normal;
        };
        match (i.pressed == Some(button), i.hovered == Some(button)) {
            (true, true) => ButtonState::Pressed,
            (false, true) => ButtonState::Highlighted,
            _ => ButtonState::Normal,
        }
    }
}

fn interactable(scene: &Scene, b: Entity) -> bool {
    scene
        .world
        .get::<&UiButton>(b)
        .is_ok_and(|b| b.interactable)
}

/// Tint colour for a button state (Colour Tint transition), multiplier applied.
pub fn tint(button: &UiButton, state: ButtonState) -> [f32; 4] {
    let c = match state {
        ButtonState::Normal => button.normal_color,
        ButtonState::Highlighted => button.highlighted_color,
        ButtonState::Pressed => button.pressed_color,
        ButtonState::Disabled => button.disabled_color,
    };
    c.map(|v| (v * button.color_multiplier).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Canvas, CanvasScaler, RectTransform, UiScaleMode};
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

    #[test]
    fn press_and_release_over_a_button_clicks_it() {
        let (s, button, label) = scene();
        let l = layout(&s, Vec2::new(800.0, 600.0));
        let centre = Vec2::new(400.0, 300.0);
        assert_eq!(raycast(&l, &s, centre), Some(label), "text is on top");
        let mut ui = UiInteraction::default();
        assert!(ui.update(&l, &s, Some(centre), false, false).is_empty());
        assert_eq!(
            UiInteraction::state(Some(&ui), &s, button),
            ButtonState::Highlighted
        );
        ui.update(&l, &s, Some(centre), true, false);
        assert_eq!(
            UiInteraction::state(Some(&ui), &s, button),
            ButtonState::Pressed
        );
        assert_eq!(ui.update(&l, &s, Some(centre), false, true), [button]);
        assert_eq!(ui.pressed, None);
        assert_eq!(UiInteraction::state(None, &s, button), ButtonState::Normal);
    }

    #[test]
    fn releasing_elsewhere_or_disabled_does_not_click() {
        let (s, button, _) = scene();
        let l = layout(&s, Vec2::new(800.0, 600.0));
        let mut ui = UiInteraction::default();
        ui.update(&l, &s, Some(Vec2::new(400.0, 300.0)), true, false);
        let outside = Some(Vec2::new(10.0, 10.0));
        assert!(ui.update(&l, &s, outside, false, true).is_empty());
        assert_eq!(ui.hovered, None);

        s.world.get::<&mut UiButton>(button).unwrap().interactable = false;
        ui.update(&l, &s, Some(Vec2::new(400.0, 300.0)), true, false);
        assert!(
            ui.update(&l, &s, Some(Vec2::new(400.0, 300.0)), false, true)
                .is_empty()
        );
        assert_eq!(
            UiInteraction::state(Some(&ui), &s, button),
            ButtonState::Disabled
        );
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
