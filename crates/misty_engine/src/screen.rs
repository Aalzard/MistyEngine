//! `Screen`: the game window's size and mode as scripts see it, and the resolution changes
//! they ask for. The host (player or editor Game view) reports the size every frame and applies
//! the requests it can.

use std::sync::Mutex;

use crate::player_settings::ScreenMode;

/// A resolution the monitor supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    /// Refresh rate in Hz (0 = unknown).
    pub refresh: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenState {
    pub width: u32,
    pub height: u32,
    pub mode: ScreenMode,
    pub resolutions: Vec<Resolution>,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            mode: ScreenMode::Windowed,
            resolutions: Vec::new(),
        }
    }
}

struct Shared {
    state: ScreenState,
    requests: Vec<(u32, u32, ScreenMode)>,
}

static SCREEN: Mutex<Option<Shared>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    let mut s = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    f(s.get_or_insert_with(|| Shared {
        state: ScreenState::default(),
        requests: Vec::new(),
    }))
}

/// Current size and mode (what `Screen.width` etc. return).
pub fn state() -> ScreenState {
    with(|s| s.state.clone())
}

/// The host reports the window's size and mode.
pub fn set_size(width: u32, height: u32, mode: ScreenMode) {
    with(|s| {
        s.state.width = width;
        s.state.height = height;
        s.state.mode = mode;
    });
}

/// The host reports the monitor's resolutions (largest first is fine; they're sorted here).
pub fn set_resolutions(mut list: Vec<Resolution>) {
    list.sort_by_key(|r| (r.width, r.height, r.refresh));
    list.dedup();
    with(|s| s.state.resolutions = list);
}

/// `Screen.SetResolution(w, h, mode)`: applied by the host at the end of the frame.
pub fn request(width: u32, height: u32, mode: ScreenMode) {
    with(|s| s.requests.push((width.max(64), height.max(64), mode)));
}

/// Requests since the last call (the last one wins).
pub fn take_requests() -> Vec<(u32, u32, ScreenMode)> {
    with(|s| std::mem::take(&mut s.requests))
}
