//! Keyboard/mouse for scripts while playing, using the familiar key names
//! ("w", "space", "left shift", "up", "return", "escape", "1", "f1"), and gamepads.

use std::cell::RefCell;

use egui::{Event, Key, PointerButton};
use misty_engine::input::{Gamepads, Joystick};
use misty_engine::scripting::Input;

thread_local! {
    /// Opened on the first Play frame (the UI thread owns it). Leaked on purpose: gilrs joins
    /// its worker thread on drop, which panics in a thread-local destructor at process exit.
    static GAMEPADS: &'static RefCell<Gamepads> = Box::leak(Box::default());
}

/// Connected gamepads this frame. They reach the game even without Game view focus, since
/// the editor UI doesn't use them.
pub fn gamepads() -> Vec<Joystick> {
    GAMEPADS.with(|g| g.borrow_mut().poll())
}

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
/// use its pixels). With the cursor `locked` (`Cursor.lockState`), the mouse delta is the raw
/// device motion, since the pointer itself no longer moves.
pub fn capture(ctx: &egui::Context, view: egui::Rect, locked: bool) -> Input {
    ctx.input(|i| {
        let mut input = Input::default();
        let ppp = i.pixels_per_point;
        // Same pixel size the Game view renders at (Quality ▸ Render Scale included).
        let px = ppp * misty_engine::quality::active().render_scale;
        input.screen_size = (glam::Vec2::new(view.width(), view.height()) * px).floor();
        input.mouse_position = i
            .pointer
            .hover_pos()
            .filter(|p| view.contains(*p))
            .map(|p| glam::Vec2::new(p.x - view.min.x, p.y - view.min.y) * px);
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
            match e {
                Event::Key {
                    key,
                    pressed,
                    repeat,
                    ..
                } => {
                    // Auto-repeats only reach UI text editing.
                    if *pressed {
                        input.typed.push(key_name(*key));
                    }
                    if !*repeat {
                        let set = if *pressed {
                            &mut input.pressed
                        } else {
                            &mut input.released
                        };
                        set.insert(key_name(*key));
                    }
                }
                Event::Text(t) | Event::Paste(t) => input.text.push_str(t),
                _ => {}
            }
        }
        let wheel = i.smooth_scroll_delta * ppp;
        input.scroll_delta = glam::Vec2::new(wheel.x, wheel.y);
        for (b, idx) in [
            (PointerButton::Primary, 0),
            (PointerButton::Secondary, 1),
            (PointerButton::Middle, 2),
        ] {
            input.mouse_held[idx] = i.pointer.button_down(b);
            input.mouse_pressed[idx] = i.pointer.button_pressed(b);
            input.mouse_released[idx] = i.pointer.button_released(b);
        }
        input.mouse_delta = if locked {
            let raw = i.events.iter().fold(egui::Vec2::ZERO, |acc, e| match e {
                Event::MouseMoved(d) => acc + *d,
                _ => acc,
            });
            glam::Vec2::new(raw.x, raw.y)
        } else {
            let d = i.pointer.delta();
            glam::Vec2::new(d.x, d.y)
        };
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
