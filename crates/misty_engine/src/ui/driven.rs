//! Values widgets drive on their parts: Slider Fill / Handle and Scrollbar Handle anchors, the
//! Toggle checkmark, Input Field text and placeholder, the Dropdown caption and Mask graphics.
//! Computed from the widget values every frame (nothing is written into the parts).

use std::collections::{HashMap, HashSet};

use glam::Vec2;
use hecs::Entity;

use crate::scene::{
    InputContentType, Scene, SliderDirection, UiDropdown, UiInputField, UiMask, UiScrollbar,
    UiSlider, UiToggle,
};

/// Slider value clamped to its range (rounded with Whole Numbers).
pub fn clamp_slider(s: &UiSlider, v: f32) -> f32 {
    let (lo, hi) = if s.min_value <= s.max_value {
        (s.min_value, s.max_value)
    } else {
        (s.max_value, s.min_value)
    };
    let v = if v.is_finite() { v.clamp(lo, hi) } else { lo };
    if s.whole_numbers { v.round() } else { v }
}

/// 0..1 position of the slider's value in its range.
pub fn slider_normalized(s: &UiSlider) -> f32 {
    let range = s.max_value - s.min_value;
    if range.abs() < 1e-6 {
        return 0.0;
    }
    ((clamp_slider(s, s.value) - s.min_value) / range).clamp(0.0, 1.0)
}

/// Value for a 0..1 position.
pub fn slider_value_at(s: &UiSlider, normalized: f32) -> f32 {
    clamp_slider(
        s,
        s.min_value + (s.max_value - s.min_value) * normalized.clamp(0.0, 1.0),
    )
}

/// Scrollbar value clamped to 0..1 and snapped to its steps.
pub fn clamp_scrollbar(sb: &UiScrollbar, v: f32) -> f32 {
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if sb.number_of_steps > 1 {
        let n = (sb.number_of_steps - 1) as f32;
        (v * n).round() / n
    } else {
        v
    }
}

/// Anchors (min, max) of a part along a direction, the other axis stretched.
fn along(dir: SliderDirection, lo: f32, hi: f32) -> (Vec2, Vec2) {
    let (mut min, mut max) = (Vec2::ZERO, Vec2::ONE);
    let axis = dir.axis();
    if dir.reversed() {
        min[axis] = 1.0 - hi;
        max[axis] = 1.0 - lo;
    } else {
        min[axis] = lo;
        max[axis] = hi;
    }
    (min, max)
}

/// Anchors widgets set on their parts.
pub fn driven_anchors(scene: &Scene) -> HashMap<Entity, (Vec2, Vec2)> {
    let mut out = HashMap::new();
    for s in scene.world.query::<&UiSlider>().iter() {
        let n = slider_normalized(s);
        if let Some(fill) = s.fill_rect {
            out.insert(fill, along(s.direction, 0.0, n));
        }
        if let Some(handle) = s.handle_rect {
            out.insert(handle, along(s.direction, n, n));
        }
    }
    for sb in scene.world.query::<&UiScrollbar>().iter() {
        if let Some(handle) = sb.handle_rect {
            let size = sb.size.clamp(0.0, 1.0);
            let start = clamp_scrollbar(sb, sb.value) * (1.0 - size);
            out.insert(handle, along(sb.direction, start, start + size));
        }
    }
    out
}

/// What gets drawn differently because of a widget.
#[derive(Clone, Debug, Default)]
pub struct Visuals {
    /// Graphics not drawn (Toggle checkmark while off, Placeholder over text, …).
    pub hidden: HashSet<Entity>,
    /// Text shown instead of a Text component's own text.
    pub text: HashMap<Entity, String>,
}

/// What an Input Field shows for `text` (Password masks every character).
pub fn display_text(field: &UiInputField, text: &str) -> String {
    match field.content_type {
        InputContentType::Password => "*".repeat(text.chars().count()),
        _ => text.to_owned(),
    }
}

pub fn visuals(scene: &Scene) -> Visuals {
    let mut v = Visuals::default();
    let w = &scene.world;
    for t in w.query::<&UiToggle>().iter() {
        if let Some(g) = t.graphic.filter(|_| !t.is_on) {
            v.hidden.insert(g);
        }
    }
    for f in w.query::<&UiInputField>().iter() {
        if let Some(t) = f.text_component {
            v.text.insert(t, display_text(f, &f.text));
        }
        if let Some(p) = f.placeholder.filter(|_| !f.text.is_empty()) {
            v.hidden.insert(p);
        }
    }
    for d in w.query::<&UiDropdown>().iter() {
        if let Some(c) = d.caption_text {
            let caption = usize::try_from(d.value)
                .ok()
                .and_then(|i| d.options.get(i))
                .cloned()
                .unwrap_or_default();
            v.text.insert(c, caption);
        }
    }
    for (e, m) in w.query::<(Entity, &UiMask)>().iter() {
        if m.enabled && !m.show_mask_graphic {
            v.hidden.insert(e);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_values_clamp_round_and_normalise() {
        let mut s = UiSlider {
            min_value: 10.0,
            max_value: 20.0,
            value: 15.0,
            ..Default::default()
        };
        assert_eq!(slider_normalized(&s), 0.5);
        assert_eq!(clamp_slider(&s, 25.0), 20.0);
        s.whole_numbers = true;
        assert_eq!(slider_value_at(&s, 0.33), 13.0);
        assert_eq!(clamp_slider(&s, f32::NAN), 10.0);
    }

    #[test]
    fn parts_follow_the_value_and_direction() {
        let mut scene = Scene::new("UI");
        let slider = scene.spawn("Slider", None);
        let fill = scene.spawn("Fill", Some(slider));
        let handle = scene.spawn("Handle", Some(slider));
        scene.add(
            slider,
            UiSlider {
                fill_rect: Some(fill),
                handle_rect: Some(handle),
                value: 0.25,
                ..Default::default()
            },
        );
        let a = driven_anchors(&scene);
        assert_eq!(a[&fill], (Vec2::ZERO, Vec2::new(0.25, 1.0)));
        assert_eq!(a[&handle], (Vec2::new(0.25, 0.0), Vec2::new(0.25, 1.0)));
        scene.world.get::<&mut UiSlider>(slider).unwrap().direction = SliderDirection::TopToBottom;
        let a = driven_anchors(&scene);
        assert_eq!(a[&fill], (Vec2::new(0.0, 0.75), Vec2::ONE));

        let bar = scene.spawn("Scrollbar", None);
        let h = scene.spawn("Handle", Some(bar));
        scene.add(
            bar,
            UiScrollbar {
                handle_rect: Some(h),
                value: 1.0,
                size: 0.25,
                ..Default::default()
            },
        );
        assert_eq!(
            driven_anchors(&scene)[&h],
            (Vec2::new(0.75, 0.0), Vec2::ONE)
        );
    }
}
