//! Window keyboard/mouse events → the per-frame `Input` scripts read, with the familiar key names
//! ("w", "space", "left shift", "up", "return", "escape", "1", "f1").

use std::collections::HashSet;

use misty_engine::glam::Vec2;
use misty_engine::scripting::Input;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

/// Script-facing name of a physical key (layout-independent, like key codes).
pub fn key_name(key: KeyCode) -> Option<String> {
    use KeyCode::*;
    let name = match key {
        Space => "space",
        Enter | NumpadEnter => "return",
        Escape => "escape",
        Tab => "tab",
        Backspace => "backspace",
        Delete => "delete",
        Insert => "insert",
        Home => "home",
        End => "end",
        PageUp => "page up",
        PageDown => "page down",
        ArrowUp => "up",
        ArrowDown => "down",
        ArrowLeft => "left",
        ArrowRight => "right",
        ShiftLeft => "left shift",
        ShiftRight => "right shift",
        ControlLeft => "left ctrl",
        ControlRight => "right ctrl",
        AltLeft => "left alt",
        AltRight => "right alt",
        Minus => "-",
        Equal => "=",
        Comma => ",",
        Period => ".",
        Slash => "/",
        Backslash => "\\",
        Semicolon => ";",
        Quote => "'",
        BracketLeft => "[",
        BracketRight => "]",
        Backquote => "`",
        other => {
            // KeyA → "a", Digit1 → "1", F5 → "f5".
            let debug = format!("{other:?}");
            let short = debug
                .strip_prefix("Key")
                .or_else(|| debug.strip_prefix("Digit"))
                .unwrap_or(&debug);
            let is_f = short.len() > 1
                && short.starts_with('F')
                && short[1..].chars().all(|c| c.is_ascii_digit());
            return (short.len() == 1 || is_f).then(|| short.to_lowercase());
        }
    };
    Some(name.into())
}

/// Accumulates window events between game frames.
#[derive(Default)]
pub struct InputState {
    held: HashSet<String>,
    pressed: HashSet<String>,
    released: HashSet<String>,
    mouse_held: [bool; 3],
    mouse_pressed: [bool; 3],
    mouse_released: [bool; 3],
    cursor: Option<Vec2>,
    mouse_delta: Vec2,
    /// Cursor in physical pixels (what the UI is laid out in); None = outside the window.
    cursor_px: Option<Vec2>,
    /// Window size in physical pixels.
    screen: Vec2,
    /// Characters typed and key presses with repeats (UI text editing).
    text: String,
    typed: Vec<String>,
    /// Wheel movement in pixels (y > 0 = away from the user).
    scroll: Vec2,
}

impl InputState {
    pub fn key(&mut self, key: KeyCode, state: ElementState, repeat: bool) {
        let Some(name) = key_name(key) else { return };
        match state {
            ElementState::Pressed => {
                self.typed.push(name.clone());
                if !repeat {
                    self.pressed.insert(name.clone());
                }
                self.held.insert(name);
            }
            ElementState::Released => {
                self.held.remove(&name);
                self.released.insert(name);
            }
        }
    }

    pub fn mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let i = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            _ => return,
        };
        let down = state == ElementState::Pressed;
        self.mouse_held[i] = down;
        if down {
            self.mouse_pressed[i] = true;
        } else {
            self.mouse_released[i] = true;
        }
    }

    /// Cursor position in logical pixels (same units as the editor's Game view).
    pub fn cursor_moved(&mut self, pos: Vec2) {
        if let Some(last) = self.cursor {
            self.mouse_delta += pos - last;
        }
        self.cursor = Some(pos);
    }

    /// Cursor position in physical pixels, for UI clicks (`None` = it left the window).
    pub fn cursor_pixels(&mut self, pos: Option<Vec2>) {
        self.cursor_px = pos;
    }

    /// Window size in physical pixels (UI layout).
    pub fn screen_resized(&mut self, width: u32, height: u32) {
        self.screen = Vec2::new(width as f32, height as f32);
    }

    /// Text a key press produced (control characters are dropped).
    pub fn typed_text(&mut self, text: &str) {
        self.text.extend(text.chars().filter(|c| !c.is_control()));
    }

    /// Mouse wheel movement in pixels.
    pub fn wheel(&mut self, delta: Vec2) {
        self.scroll += delta;
    }

    /// Focus lost: release everything so keys don't stick.
    pub fn clear_held(&mut self) {
        self.released.extend(self.held.drain());
        self.mouse_held = [false; 3];
    }

    /// This frame's input; per-frame events start over.
    pub fn frame(&mut self) -> Input {
        Input {
            held: self.held.clone(),
            pressed: std::mem::take(&mut self.pressed),
            released: std::mem::take(&mut self.released),
            mouse_held: self.mouse_held,
            mouse_pressed: std::mem::take(&mut self.mouse_pressed),
            mouse_released: std::mem::take(&mut self.mouse_released),
            mouse_delta: std::mem::take(&mut self.mouse_delta),
            mouse_position: self.cursor_px,
            screen_size: self.screen,
            text: std::mem::take(&mut self.text),
            typed: std::mem::take(&mut self.typed),
            scroll_delta: std::mem::take(&mut self.scroll),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn familiar_key_names() {
        let name = |k| key_name(k).unwrap();
        assert_eq!(name(KeyCode::KeyW), "w");
        assert_eq!(name(KeyCode::Space), "space");
        assert_eq!(name(KeyCode::ShiftLeft), "left shift");
        assert_eq!(name(KeyCode::ArrowUp), "up");
        assert_eq!(name(KeyCode::Digit3), "3");
        assert_eq!(name(KeyCode::F1), "f1");
        assert_eq!(name(KeyCode::F12), "f12");
        assert_eq!(name(KeyCode::Enter), "return");
        assert_eq!(key_name(KeyCode::Fn), None);
    }

    #[test]
    fn presses_last_one_frame_and_held_keys_stay() {
        let mut s = InputState::default();
        s.key(KeyCode::KeyW, ElementState::Pressed, false);
        s.mouse_button(MouseButton::Left, ElementState::Pressed);
        s.cursor_moved(Vec2::new(10.0, 10.0));
        s.cursor_moved(Vec2::new(14.0, 7.0));
        let a = s.frame();
        assert!(a.key("w") && a.key_down("w") && a.mouse_down(0));
        assert_eq!(a.mouse_delta, Vec2::new(4.0, -3.0));
        let b = s.frame();
        assert!(b.key("w") && !b.key_down("w") && b.mouse(0) && !b.mouse_down(0));
        assert_eq!(b.mouse_delta, Vec2::ZERO);
        s.key(KeyCode::KeyW, ElementState::Pressed, true);
        assert!(!s.frame().key_down("w"), "auto-repeat isn't a new press");
        s.clear_held();
        let c = s.frame();
        assert!(!c.key("w") && c.key_up("w") && !c.mouse(0));
    }
}
