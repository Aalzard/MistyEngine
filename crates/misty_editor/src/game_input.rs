//! Keyboard/mouse for scripts while playing, using the familiar key names
//! ("w", "space", "left shift", "up", "return", "escape", "1", "f1").

use egui::{Event, Key, PointerButton};
use misty_engine::scripting::Input;

/// Script-facing name of an egui key.
pub fn key_name(key: Key) -> String {
    match key {
        Key::Space => "space".into(),
        Key::Enter => "return".into(),
        Key::Escape => "escape".into(),
        Key::Tab => "tab".into(),
        Key::Backspace => "backspace".into(),
        Key::Delete => "delete".into(),
        Key::ArrowUp => "up".into(),
        Key::ArrowDown => "down".into(),
        Key::ArrowLeft => "left".into(),
        Key::ArrowRight => "right".into(),
        Key::Num0 => "0".into(),
        Key::Num1 => "1".into(),
        Key::Num2 => "2".into(),
        Key::Num3 => "3".into(),
        Key::Num4 => "4".into(),
        Key::Num5 => "5".into(),
        Key::Num6 => "6".into(),
        Key::Num7 => "7".into(),
        Key::Num8 => "8".into(),
        Key::Num9 => "9".into(),
        other => other.name().to_lowercase(),
    }
}

/// This frame's input as scripts see it; `view` = the game image's screen rect (UI clicks
/// use its pixels).
pub fn capture(ctx: &egui::Context, view: egui::Rect) -> Input {
    ctx.input(|i| {
        let mut input = Input::default();
        let ppp = i.pixels_per_point;
        // Same pixel size the Game view renders at.
        input.screen_size = (glam::Vec2::new(view.width(), view.height()) * ppp).floor();
        input.mouse_position = i
            .pointer
            .hover_pos()
            .filter(|p| view.contains(*p))
            .map(|p| glam::Vec2::new(p.x - view.min.x, p.y - view.min.y) * ppp);
        for k in &i.keys_down {
            input.held.insert(key_name(*k));
        }
        // Modifiers are not keys in egui; expose them like the usual shift/ctrl/alt keys.
        let m = i.modifiers;
        for (on, name) in [
            (m.shift, "left shift"),
            (m.ctrl, "left ctrl"),
            (m.alt, "left alt"),
        ] {
            if on {
                input.held.insert(name.into());
            }
        }
        for e in &i.events {
            if let Event::Key {
                key,
                pressed,
                repeat: false,
                ..
            } = e
            {
                let set = if *pressed {
                    &mut input.pressed
                } else {
                    &mut input.released
                };
                set.insert(key_name(*key));
            }
        }
        for (b, idx) in [
            (PointerButton::Primary, 0),
            (PointerButton::Secondary, 1),
            (PointerButton::Middle, 2),
        ] {
            input.mouse_held[idx] = i.pointer.button_down(b);
            input.mouse_pressed[idx] = i.pointer.button_pressed(b);
            input.mouse_released[idx] = i.pointer.button_released(b);
        }
        let d = i.pointer.delta();
        input.mouse_delta = glam::Vec2::new(d.x, d.y);
        input
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn familiar_key_names() {
        assert_eq!(key_name(Key::W), "w");
        assert_eq!(key_name(Key::Space), "space");
        assert_eq!(key_name(Key::ArrowLeft), "left");
        assert_eq!(key_name(Key::Num3), "3");
        assert_eq!(key_name(Key::F1), "f1");
    }
}
