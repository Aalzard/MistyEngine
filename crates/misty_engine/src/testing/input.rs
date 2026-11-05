//! `Input.simulate*`: keys and mouse buttons a PlayMode test presses for the game.

use std::collections::HashSet;

use glam::Vec2;
use mlua::{Lua, Result, Table};

use crate::scripting::Input;

/// Simulated keyboard/mouse state (Lua app data of a test file's state); the runner turns it
/// into each game frame's [`Input`].
#[derive(Clone, Debug, Default)]
pub struct SimInput {
    held: HashSet<String>,
    pressed: HashSet<String>,
    released: HashSet<String>,
    /// `simulateKeyPress`: held for one frame, released on the next.
    taps: HashSet<String>,
    mouse_held: [bool; 3],
    mouse_pressed: [bool; 3],
    mouse_released: [bool; 3],
    mouse_position: Option<Vec2>,
}

impl SimInput {
    pub fn key_down(&mut self, key: &str) {
        let key = key.to_lowercase();
        if self.held.insert(key.clone()) {
            self.pressed.insert(key);
        }
    }

    pub fn key_up(&mut self, key: &str) {
        let key = key.to_lowercase();
        if self.held.remove(&key) || self.taps.remove(&key) {
            self.released.insert(key);
        }
    }

    pub fn key_press(&mut self, key: &str) {
        let key = key.to_lowercase();
        self.pressed.insert(key.clone());
        self.taps.insert(key);
    }

    pub fn mouse_down(&mut self, button: usize) {
        if let Some(h) = self.mouse_held.get_mut(button) {
            if !*h {
                self.mouse_pressed[button] = true;
            }
            *h = true;
        }
    }

    pub fn mouse_up(&mut self, button: usize) {
        if let Some(h) = self.mouse_held.get_mut(button) {
            if *h {
                self.mouse_released[button] = true;
            }
            *h = false;
        }
    }

    /// Release everything (between tests the state starts empty anyway).
    pub fn reset(&mut self) {
        let held: Vec<String> = self.held.drain().chain(self.taps.drain()).collect();
        self.released.extend(held);
        for b in 0..3 {
            self.mouse_up(b);
        }
    }

    /// The next game frame's input; one-frame presses and releases are used up.
    pub fn frame(&mut self, screen: Vec2) -> Input {
        let mut held = self.held.clone();
        held.extend(self.taps.iter().cloned());
        let input = Input {
            held,
            pressed: std::mem::take(&mut self.pressed),
            released: std::mem::take(&mut self.released),
            mouse_held: self.mouse_held,
            mouse_pressed: std::mem::take(&mut self.mouse_pressed),
            mouse_released: std::mem::take(&mut self.mouse_released),
            mouse_position: self.mouse_position,
            screen_size: screen,
            ..Default::default()
        };
        // Taps come up on the next frame.
        self.released.extend(self.taps.drain());
        input
    }
}

fn with_sim(lua: &Lua, f: impl FnOnce(&mut SimInput)) {
    if let Some(mut sim) = lua.app_data_mut::<SimInput>() {
        f(&mut sim);
    }
}

/// Add `Input.simulateKeyDown/Up/Press`, `simulateMouseDown/Up`, `simulateMousePosition` and
/// `simulateReset` (and the [`SimInput`] they write to).
pub fn register(lua: &Lua) -> Result<()> {
    lua.set_app_data(SimInput::default());
    let input: Table = lua.globals().get("Input")?;
    input.set(
        "simulateKeyDown",
        lua.create_function(|lua, key: String| {
            with_sim(lua, |s| s.key_down(&key));
            Ok(())
        })?,
    )?;
    input.set(
        "simulateKeyUp",
        lua.create_function(|lua, key: String| {
            with_sim(lua, |s| s.key_up(&key));
            Ok(())
        })?,
    )?;
    input.set(
        "simulateKeyPress",
        lua.create_function(|lua, key: String| {
            with_sim(lua, |s| s.key_press(&key));
            Ok(())
        })?,
    )?;
    input.set(
        "simulateMouseDown",
        lua.create_function(|lua, button: Option<usize>| {
            with_sim(lua, |s| s.mouse_down(button.unwrap_or(0)));
            Ok(())
        })?,
    )?;
    input.set(
        "simulateMouseUp",
        lua.create_function(|lua, button: Option<usize>| {
            with_sim(lua, |s| s.mouse_up(button.unwrap_or(0)));
            Ok(())
        })?,
    )?;
    input.set(
        "simulateMousePosition",
        lua.create_function(|lua, (x, y): (f32, f32)| {
            with_sim(lua, |s| s.mouse_position = Some(Vec2::new(x, y)));
            Ok(())
        })?,
    )?;
    input.set(
        "simulateReset",
        lua.create_function(|lua, ()| {
            with_sim(lua, SimInput::reset);
            Ok(())
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_last_one_frame_and_taps_release_next_frame() {
        let mut s = SimInput::default();
        let screen = Vec2::new(1280.0, 720.0);
        s.key_down("D");
        let f = s.frame(screen);
        assert!(f.key("d") && f.key_down("d"));
        let f = s.frame(screen);
        assert!(
            f.key("d") && !f.key_down("d"),
            "still held, pressed only once"
        );
        s.key_up("d");
        let f = s.frame(screen);
        assert!(!f.key("d") && f.key_up("d"));

        s.key_press("space");
        let f = s.frame(screen);
        assert!(f.key("space") && f.key_down("space"));
        let f = s.frame(screen);
        assert!(!f.key("space") && f.key_up("space"));
        assert!(!s.frame(screen).key_up("space"));

        s.mouse_down(0);
        let f = s.frame(screen);
        assert!(f.mouse(0) && f.mouse_down(0));
        assert_eq!(f.screen_size, screen);
    }
}
