//! Dedicated server: the game without a window, audio device or GPU. A Dedicated Server build
//! (or `--headless`) runs here: the build scene loads, the network server starts on the scene's
//! Network Manager (`--port` overrides its port) and the game updates at a fixed tick until
//! `Application.Quit()` or the process is stopped.
//!
//! The log goes to stderr and `Player.log`. The line `Server listening on port <n>` tells
//! scripts and tests where to connect.

use std::path::Path;
use std::time::{Duration, Instant};

use misty_engine::network::AutoStart;
use misty_engine::scripting::{Input, LogKind};

use crate::Log;
use crate::game::Game;
use crate::startup::{Args, Launch};

/// Updates per second when Project Settings ▸ Player sets no Target Frame Rate.
pub const DEFAULT_TICK_RATE: f64 = 60.0;

/// Run the headless game; returns the exit code.
pub fn run(root: &Path, launch: &Launch, args: &Args) -> i32 {
    crate::startup::server_console();
    let mut options = args.launch.clone();
    options.headless = true;
    // A server build serves unless the command line asks for something else.
    if options.network.is_none() && launch.info.server {
        options.network = Some(AutoStart::Server);
    }
    let game = Game::open_with(root, &options);
    let player = game
        .as_ref()
        .map(|g| g.player_settings())
        .unwrap_or_default();
    let product = player.product_name_or(root);
    let prefs_file = misty_engine::prefs::default_path(&player.company_name, &product);
    let log_dir = match (&prefs_file, launch.info.development || !launch.packed) {
        (Some(p), false) => p.parent().map(Path::to_path_buf),
        _ => None,
    };
    let mut log = Log::open(log_dir);
    let mut game = match game {
        Ok(g) => g,
        Err(e) => {
            log.write(LogKind::Error, &format!("{}: {e}", root.display()));
            return 1;
        }
    };
    crate::open_user_data(&player, &product, launch);
    let source = if launch.packed { "pak" } else { "folder" };
    log.write(
        LogKind::Info,
        &format!(
            "Dedicated server: {} ({source}) {} · no window, audio or GPU",
            root.display(),
            player.version
        ),
    );
    for (kind, text) in game.take_logs() {
        log.write(kind, &text);
    }
    match game.network_port() {
        Some(port) => {
            let mode = game.network_stats().map(|n| n.mode);
            if mode == Some(misty_engine::network::NetMode::Client) {
                log.write(
                    LogKind::Info,
                    &format!("Client connecting from port {port}"),
                );
            } else {
                log.write(LogKind::Info, &format!("Server listening on port {port}"));
            }
        }
        None if options.network.is_some() => log.write(
            LogKind::Warning,
            "the network didn't start: the scene needs a Network Manager",
        ),
        None => {}
    }
    let interval = player
        .frame_interval()
        .unwrap_or(Duration::from_secs_f64(1.0 / DEFAULT_TICK_RATE));
    let mut last = Instant::now();
    let mut next = last + interval;
    while !game.quit {
        let now = Instant::now();
        if now < next {
            std::thread::sleep(next - now);
        }
        let now = Instant::now();
        // Catch up at most one tick after a hitch.
        next = next.max(now - interval) + interval;
        let dt = now.duration_since(last).as_secs_f32();
        last = now;
        game.frame(dt, Input::default());
        for (kind, text) in game.take_logs() {
            log.write(kind, &text);
        }
        misty_engine::profiler::end_frame();
    }
    log.write(LogKind::Info, "Server stopped (Application.Quit)");
    if let Err(e) = misty_engine::prefs::save() {
        log.write(LogKind::Warning, &format!("PlayerPrefs: {e}"));
    }
    0
}
