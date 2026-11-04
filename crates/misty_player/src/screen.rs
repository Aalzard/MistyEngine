//! The game window: how it opens (Player settings, or the resolution the player chose last
//! time), screen mode changes from scripts, and the icon.

use misty_engine::player_settings::{PlayerSettings, ScreenMode};
use misty_engine::prefs::{self, keys};
use misty_engine::screen::Resolution;
use winit::dpi::PhysicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::monitor::{MonitorHandle, VideoModeHandle};
use winit::window::{Fullscreen, Icon, Window, WindowAttributes};

use crate::game::Game;

/// Prefs value of a screen mode.
pub fn mode_key(mode: ScreenMode) -> &'static str {
    match mode {
        ScreenMode::FullscreenWindow => "FullscreenWindow",
        ScreenMode::ExclusiveFullscreen => "ExclusiveFullscreen",
        ScreenMode::Windowed => "Windowed",
        ScreenMode::Maximized => "Maximized",
    }
}

fn mode_from_key(key: &str) -> Option<ScreenMode> {
    ScreenMode::ALL.into_iter().find(|m| mode_key(*m) == key)
}

/// Size and mode to open with: the player's last choice, else the Player settings.
pub fn startup_mode(p: &PlayerSettings) -> (u32, u32, ScreenMode) {
    let int = |k, d: u32| u32::try_from(prefs::get_int(k, d.into())).unwrap_or(d);
    let w = int(keys::WIDTH, p.width).clamp(64, 16384);
    let h = int(keys::HEIGHT, p.height).clamp(64, 16384);
    let mode = mode_from_key(&prefs::get_string(keys::SCREEN_MODE, "")).unwrap_or(p.screen_mode);
    (w, h, mode)
}

/// Window attributes from the Player settings.
pub fn attributes(
    p: &PlayerSettings,
    game: &Game,
    event_loop: &ActiveEventLoop,
) -> (WindowAttributes, ScreenMode) {
    let (w, h, mode) = startup_mode(p);
    let mut attrs = Window::default_attributes()
        .with_title(game.product_name())
        .with_inner_size(PhysicalSize::new(w, h))
        .with_resizable(p.resizable)
        .with_window_icon(icon(p));
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next());
    attrs = match mode {
        ScreenMode::FullscreenWindow => attrs.with_fullscreen(Some(Fullscreen::Borderless(None))),
        ScreenMode::ExclusiveFullscreen => attrs.with_fullscreen(Some(
            monitor
                .as_ref()
                .and_then(|m| video_mode(m, w, h))
                .map_or(Fullscreen::Borderless(None), Fullscreen::Exclusive),
        )),
        ScreenMode::Maximized => attrs.with_maximized(true),
        ScreenMode::Windowed => attrs,
    };
    (attrs, mode)
}

/// Switch mode / size while running.
pub fn apply(window: &Window, mode: ScreenMode, w: u32, h: u32) {
    match mode {
        ScreenMode::FullscreenWindow => window.set_fullscreen(Some(Fullscreen::Borderless(None))),
        ScreenMode::ExclusiveFullscreen => {
            let vm = window.current_monitor().and_then(|m| video_mode(&m, w, h));
            window.set_fullscreen(Some(
                vm.map_or(Fullscreen::Borderless(None), Fullscreen::Exclusive),
            ));
        }
        ScreenMode::Windowed | ScreenMode::Maximized => {
            window.set_fullscreen(None);
            window.set_maximized(mode == ScreenMode::Maximized);
            if mode == ScreenMode::Windowed {
                let _ = window.request_inner_size(PhysicalSize::new(w, h));
            }
        }
    }
}

/// The monitor's mode closest to `w`×`h` (highest refresh rate among equals).
fn video_mode(m: &MonitorHandle, w: u32, h: u32) -> Option<VideoModeHandle> {
    m.video_modes().min_by_key(|v| {
        let s = v.size();
        let d =
            (i64::from(s.width) - i64::from(w)).abs() + (i64::from(s.height) - i64::from(h)).abs();
        (d, u32::MAX - v.refresh_rate_millihertz())
    })
}

/// `Screen.resolutions`: the current monitor's video modes.
pub fn report_resolutions(window: &Window) {
    let list = window
        .current_monitor()
        .map(|m| {
            m.video_modes()
                .map(|v| Resolution {
                    width: v.size().width,
                    height: v.size().height,
                    refresh: v.refresh_rate_millihertz().div_ceil(1000),
                })
                .collect()
        })
        .unwrap_or_default();
    misty_engine::screen::set_resolutions(list);
}

/// The window icon from Player ▸ Icon (read through the pak in builds).
fn icon(p: &PlayerSettings) -> Option<Icon> {
    let img = misty_engine::sprites::load_image(p.icon.as_deref()?).ok()?;
    let img = misty_engine::render::texture::fit(img, 256);
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}
