//! Misty Engine standalone player: opens a window and plays a project's build scenes.
//!
//! `misty_player [ProjectFolder]`. Without an argument it plays the build it belongs to
//! (`<Product>_Data/data.pak` next to the executable), else the nearest `SampleProject`.
//! Project Settings ▸ Player (window, VSync, frame rate, icon, splash) and the chosen quality
//! level and resolution (kept in the per-user prefs file) are applied at startup.
//! `MISTY_SCREENSHOT=out.png misty_player` saves the game image after warm-up and exits
//! (`MISTY_UPSCALING=Performance` etc. overrides the quality levels' Upscaling).
//! `MISTY_PROFILE=capture.jsonl misty_player` records the profiler into a capture file
//! (one JSON object per frame) until the player quits.
//! Multiplayer: `--net-host`, `--net-server` or `--net-client host[:port]` start networking
//! once the scene runs; `--scene Assets/…scene [--scene-file copy.scene]` plays that scene;
//! `--title`, `--size 800x450` and `--pos 40,40` place the window (the editor's Multiplayer
//! Play Mode launches its player windows this way).

// Release builds are windowed apps with no console; output goes to Player.log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod game;
mod gfx;
mod input;
mod screen;
mod startup;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use misty_engine::glam::Vec2;
use misty_engine::player_settings::{PlayerSettings, ScreenMode};
use misty_engine::prefs::{self, PrefValue, keys};
use misty_engine::scripting::LogKind;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window, WindowId};

use game::Game;
use gfx::Gfx;
use input::InputState;
use misty_engine::input::Gamepads;

/// Frames rendered before a `MISTY_SCREENSHOT` capture.
const SCREENSHOT_FRAME: u64 = 60;

fn main() {
    // Players read compressed textures from the build (or the project's Library); they never
    // encode.
    misty_engine::sprites::compress::set_mode(misty_engine::sprites::compress::Mode::CacheOnly);
    let args = match startup::parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            Log::open(None).write(LogKind::Error, &e);
            std::process::exit(2);
        }
    };
    let launch = match startup::locate(args.project.clone()) {
        Ok(l) => l,
        Err(e) => {
            Log::open(None).write(LogKind::Error, &e);
            std::process::exit(1);
        }
    };
    let root = launch.root.clone();
    if launch.info.development && launch.packed {
        startup::open_console();
    }
    let game = Game::open_with(&root, &args.launch);
    let player = game
        .as_ref()
        .map(|g| g.player_settings())
        .unwrap_or_default();
    let product = player.product_name_or(&root);
    let prefs_file = prefs::default_path(&player.company_name, &product);
    // Development builds log next to the game; releases in the user's folder (the game's folder
    // may be read-only, e.g. under Program Files or Steam).
    let log_dir = match (&prefs_file, launch.info.development) {
        (Some(p), false) => p.parent().map(Path::to_path_buf),
        _ => None,
    };
    let mut log = Log::open(log_dir);
    let game = match game {
        Ok(g) => g,
        Err(e) => {
            log.write(LogKind::Error, &format!("{}: {e}", root.display()));
            std::process::exit(1);
        }
    };
    if let Some(p) = &prefs_file {
        prefs::open(p);
    }
    // Save games go to the per-user folder next to PlayerPrefs.
    misty_engine::save::set_folder_override(misty_engine::save::player_folder(
        &player.company_name,
        &product,
    ));
    // The startup locale may come from PlayerPrefs, which are only open now.
    misty_engine::localization::reset_session();
    game.apply_saved_quality();
    let source = if launch.packed { "pak" } else { "folder" };
    log.write(
        LogKind::Info,
        &format!("Playing {} ({source}) {}", root.display(), player.version),
    );
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            log.write(LogKind::Error, &format!("could not create a window: {e}"));
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    if let Some(path) = std::env::var_os("MISTY_PROFILE")
        && let Err(e) = misty_engine::profiler::capture::start(Path::new(&path))
    {
        log.write(LogKind::Warning, &format!("profiler capture: {e}"));
    }
    let screenshot = std::env::var_os("MISTY_SCREENSHOT").map(PathBuf::from);
    let mut app = Player {
        game,
        log,
        window: None,
        gfx: None,
        input: InputState::default(),
        gamepads: Gamepads::default(),
        last: Instant::now(),
        frames: 0,
        // Automated captures skip the splash screen.
        splash_until: None,
        paused: false,
        next_frame: None,
        quality: u64::MAX,
        mode: player.screen_mode,
        screenshot,
        cursor: Default::default(),
        player,
        window_options: args.window,
        title_updated: Instant::now(),
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        app.log.write(LogKind::Error, &e.to_string());
    }
    if let Err(e) = prefs::save() {
        app.log
            .write(LogKind::Warning, &format!("PlayerPrefs: {e}"));
    }
    if let Err(e) = misty_engine::profiler::capture::finish() {
        app.log
            .write(LogKind::Warning, &format!("profiler capture: {e}"));
    }
}

struct Player {
    game: Game,
    log: Log,
    player: PlayerSettings,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    input: InputState,
    gamepads: Gamepads,
    last: Instant,
    frames: u64,
    /// The splash screen shows until then.
    splash_until: Option<Instant>,
    /// Unfocused and not Run In Background.
    paused: bool,
    /// Target Frame Rate: when the next frame may start.
    next_frame: Option<Instant>,
    /// [`misty_engine::quality::generation`] the renderer was set up for.
    quality: u64,
    /// Current screen mode.
    mode: ScreenMode,
    screenshot: Option<PathBuf>,
    /// `Cursor.lockState` / `Cursor.visible` last applied to the window.
    cursor: misty_engine::scripting::CursorState,
    /// Command-line title, size and position.
    window_options: startup::WindowOptions,
    /// The window title shows network stats, refreshed about once a second.
    title_updated: Instant,
}

impl Player {
    /// One game frame: update, render, present.
    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(until) = self.splash_until {
            if Instant::now() < until {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.draw_splash();
                }
                return;
            }
            self.splash_until = None;
            self.last = Instant::now();
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_splash(None, [0.0; 3]);
            }
        }
        if self.paused {
            return;
        }
        self.sync_quality();
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        let mut input = self.input.frame();
        input.joysticks = self.gamepads.poll();
        self.game.frame(dt, input);
        // Gamepad rumble scripts asked for.
        if let Some(motors) = self.game.take_rumble() {
            for (pad, low, high) in motors {
                self.gamepads.set_motor_speeds(pad, low, high);
            }
        }
        for (kind, text) in self.game.take_logs() {
            self.log.write(kind, &text);
        }
        if self.game.quit {
            event_loop.exit();
            return;
        }
        self.apply_screen_requests();
        self.apply_cursor();
        self.show_network_in_title();
        let Some(gfx) = self.gfx.as_mut() else { return };
        gfx.update_probes(&self.game.scene, &self.game.assets);
        let view = self.game.view();
        let frame = view.as_ref().map(|v| v.frame());
        let effects = view.as_ref().map_or(&[][..], |v| &v.effects[..]);
        gfx.draw(
            frame.as_ref(),
            effects,
            &self.game.scene,
            self.game.ui_interaction(),
        );
        for w in gfx.take_warnings() {
            self.log.write(LogKind::Warning, &w);
        }
        if misty_engine::profiler::enabled() {
            misty_engine::profiler::set_memory(misty_engine::profiler::MemoryStats {
                entities: self.game.scene.world.len() as usize,
                render: gfx.memory(),
            });
        }
        misty_engine::profiler::end_frame();
        // SaveSystem.Save asked for a thumbnail of this frame.
        if let Some(path) = misty_engine::save::take_thumbnail_request() {
            let (w, h, pixels) = gfx.capture();
            if let Err(e) = misty_engine::save::write_thumbnail(&path, w, h, pixels) {
                self.log
                    .write(LogKind::Warning, &format!("save thumbnail: {e}"));
            }
        }
        self.frames += 1;
        if self.frames == SCREENSHOT_FRAME
            && let Some(path) = self.screenshot.take()
        {
            let (w, h, pixels) = gfx.capture();
            match save_png(&path, w, h, &pixels) {
                Ok(()) => eprintln!("screenshot saved to {}", path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            event_loop.exit();
        }
    }

    /// "Net Demo · Player 2 · Client · RTT 3 ms" while networking runs.
    fn show_network_in_title(&mut self) {
        if self.title_updated.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.title_updated = Instant::now();
        let Some(window) = &self.window else { return };
        let base = self
            .window_options
            .title
            .clone()
            .unwrap_or_else(|| self.game.product_name());
        let title = match self.game.network_stats() {
            Some(s) => format!(
                "{base} · {} · RTT {:.0} ms · {} objects",
                s.mode.label(),
                s.rtt_ms,
                s.objects
            ),
            None => base,
        };
        if window.title() != title {
            window.set_title(&title);
        }
    }

    /// A new quality level (startup or `QualitySettings.SetQualityLevel`) reaches the renderer.
    fn sync_quality(&mut self) {
        let generation = misty_engine::quality::generation();
        if generation == self.quality {
            return;
        }
        self.quality = generation;
        let level = misty_engine::quality::active();
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.set_quality(&level);
            let [w, h] = gfx.target_size();
            self.input.screen_resized(w, h);
        }
        self.log.write(
            LogKind::Info,
            &format!(
                "Quality: {} (render scale {})",
                level.name, level.render_scale
            ),
        );
    }

    /// `Screen.SetResolution` / `Screen.fullScreen` from scripts (the last request wins).
    fn apply_screen_requests(&mut self) {
        let Some((w, h, mode)) = misty_engine::screen::take_requests().pop() else {
            return;
        };
        if let Some(window) = &self.window {
            screen::apply(window, mode, w, h);
            self.mode = mode;
            prefs::set(keys::WIDTH, PrefValue::Int(w.into()));
            prefs::set(keys::HEIGHT, PrefValue::Int(h.into()));
            prefs::set(
                keys::SCREEN_MODE,
                PrefValue::String(screen::mode_key(mode).into()),
            );
        }
    }

    /// `Cursor.lockState` / `Cursor.visible` from scripts reach the window when they change.
    /// Platforms without a locked mode confine the cursor instead; scripts read raw motion
    /// either way.
    fn apply_cursor(&mut self) {
        use misty_engine::scripting::CursorLockMode;
        let wanted = self.game.cursor();
        if wanted == self.cursor {
            return;
        }
        self.cursor = wanted;
        self.input.locked = wanted.lock == CursorLockMode::Locked;
        let Some(window) = &self.window else { return };
        let result = match wanted.lock {
            CursorLockMode::None => window.set_cursor_grab(CursorGrabMode::None),
            CursorLockMode::Confined => window.set_cursor_grab(CursorGrabMode::Confined),
            CursorLockMode::Locked => window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined)),
        };
        if let Err(e) = result {
            self.log
                .write(LogKind::Warning, &format!("Cursor.lockState: {e}"));
        }
        window.set_cursor_visible(wanted.visible);
    }

    fn report_size(&mut self, width: u32, height: u32) {
        misty_engine::screen::set_size(width, height, self.mode);
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.resize(width, height);
            let [w, h] = gfx.target_size();
            self.input.screen_resized(w, h);
        } else {
            self.input.screen_resized(width, height);
        }
    }
}

impl ApplicationHandler for Player {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (mut attrs, mode) = screen::attributes(&self.player, &self.game, event_loop);
        self.mode = mode;
        let o = &self.window_options;
        if let Some(t) = &o.title {
            attrs = attrs.with_title(t.clone());
        }
        if let Some((w, h)) = o.size {
            attrs = attrs
                .with_inner_size(winit::dpi::PhysicalSize::new(w, h))
                .with_fullscreen(None)
                .with_maximized(false);
            self.mode = ScreenMode::Windowed;
        }
        if let Some((x, y)) = o.position {
            attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.log.write(LogKind::Error, &format!("window: {e}"));
                event_loop.exit();
                return;
            }
        };
        screen::report_resolutions(&window);
        match Gfx::new(window.clone(), self.player.vsync) {
            Ok(gfx) => {
                self.log
                    .write(LogKind::Info, &format!("Renderer: {}", gfx.adapter_name));
                self.gfx = Some(gfx);
            }
            Err(e) => {
                self.log.write(LogKind::Error, &e);
                event_loop.exit();
                return;
            }
        }
        self.sync_quality();
        let splash = &self.player.splash;
        if splash.enabled && splash.duration > 0.0 && self.screenshot.is_none() {
            let image = splash
                .image
                .as_ref()
                .and_then(|rel| misty_engine::sprites::load_image(rel).ok());
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_splash(image, splash.background);
            }
            self.splash_until = Some(Instant::now() + Duration::from_secs_f32(splash.duration));
        }
        let size = window.inner_size();
        self.window = Some(window);
        self.report_size(size.width, size.height);
        self.last = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.report_size(size.width, size.height),
            WindowEvent::Focused(focused) => {
                if !focused {
                    self.input.clear_held();
                    // Alt-tabbing away must not leave the cursor locked.
                    self.game.release_cursor();
                    self.apply_cursor();
                }
                let pause = !focused && !self.player.run_in_background;
                if self.paused && !pause {
                    // Don't count the paused time as one long frame.
                    self.last = Instant::now();
                }
                self.paused = pause;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.input.key(code, event.state, event.repeat);
                }
                if event.state == ElementState::Pressed
                    && let Some(t) = &event.text
                {
                    self.input.typed_text(t);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => self.input.mouse_button(button, state),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
                let p = position.to_logical::<f32>(scale);
                self.input.cursor_moved(Vec2::new(p.x, p.y));
                // UI clicks are in render-target pixels.
                let rs = self.gfx.as_ref().map_or(1.0, Gfx::render_scale);
                let px = Vec2::new(position.x as f32, position.y as f32) * rs;
                self.input.cursor_pixels(Some(px));
            }
            WindowEvent::CursorLeft { .. } => self.input.cursor_pixels(None),
            WindowEvent::Touch(t) => {
                use misty_engine::input::touch::TouchEvent;
                let rs = self.gfx.as_ref().map_or(1.0, Gfx::render_scale);
                let p = Vec2::new(t.location.x as f32, t.location.y as f32) * rs;
                self.input.touch(match t.phase {
                    winit::event::TouchPhase::Started => TouchEvent::Start(t.id, p),
                    winit::event::TouchPhase::Moved => TouchEvent::Move(t.id, p),
                    winit::event::TouchPhase::Ended => TouchEvent::End(t.id, p),
                    winit::event::TouchPhase::Cancelled => TouchEvent::Cancel(t.id, p),
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Lines → pixels like a browser would (about 50 per notch).
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y) * 50.0,
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32),
                };
                self.input.wheel(d);
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (x, y) } = event {
            self.input.raw_motion(Vec2::new(x as f32, y as f32));
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(w) = &self.window else { return };
        if self.paused {
            // Sleep until the window gets focus back.
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        // Target Frame Rate (VSync off): wait for the next frame's slot.
        match self.player.frame_interval() {
            Some(interval) if self.splash_until.is_none() => {
                let now = Instant::now();
                let next = self.next_frame.unwrap_or(now);
                if now >= next {
                    w.request_redraw();
                    // Catch up at most one frame after a hitch.
                    self.next_frame = Some(next.max(now - interval) + interval);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    self.next_frame.unwrap_or(now + interval),
                ));
            }
            _ => {
                event_loop.set_control_flow(ControlFlow::Poll);
                w.request_redraw();
            }
        }
    }
}

/// Game output: stderr plus `Player.log` (next to the executable, or in `dir`).
struct Log {
    file: Option<std::fs::File>,
}

impl Log {
    fn open(dir: Option<PathBuf>) -> Self {
        let dir = dir.or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
        });
        let file = dir.and_then(|d| {
            let _ = std::fs::create_dir_all(&d);
            std::fs::File::create(d.join("Player.log")).ok()
        });
        Self { file }
    }

    fn write(&mut self, kind: LogKind, text: &str) {
        let line = match kind {
            LogKind::Info => text.to_owned(),
            LogKind::Warning => format!("Warning: {text}"),
            LogKind::Error => format!("Error: {text}"),
        };
        eprintln!("{line}");
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}
