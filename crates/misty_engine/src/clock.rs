//! Animation clock shared by shaders (`globals.time`), water waves and buoyancy, so what the GPU
//! draws and what the CPU queries agree. While playing it follows the game's `Time.time`; in Edit
//! mode it is wall-clock time since the process started.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct State {
    start: Instant,
    /// Game time and when it was last set (Play mode).
    play: Option<(f64, Instant)>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(State {
            start: Instant::now(),
            play: None,
        })
    })
}

/// How long a game time stays current without being set again (Play mode ended or paused long).
const PLAY_TIMEOUT: f64 = 0.5;

/// Seconds of animation time now.
pub fn now() -> f64 {
    let s = state().lock().unwrap_or_else(|e| e.into_inner());
    match s.play {
        Some((t, at)) if at.elapsed().as_secs_f64() < PLAY_TIMEOUT => t,
        _ => s.start.elapsed().as_secs_f64(),
    }
}

/// Follow the game clock (called every Play-mode frame).
pub fn set_play_time(t: f64) {
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    s.play = Some((t, Instant::now()));
}
