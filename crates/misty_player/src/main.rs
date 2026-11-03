//! Misty Engine standalone player: opens a window and plays a project's build scenes.
//!
//! `misty_player [ProjectFolder]`. Without an argument it plays the folder it sits in (a build
//! made by File ▸ Build), else the nearest `SampleProject`.
//! `MISTY_SCREENSHOT=out.png misty_player` saves the game image after warm-up and exits.

// Release builds are windowed apps with no console; output goes to Player.log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod game;
mod gfx;
mod input;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use misty_engine::glam::Vec2;
use misty_engine::scripting::LogKind;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Fullscreen, Window, WindowId};

use game::Game;
use gfx::Gfx;
use input::InputState;

/// Frames rendered before a `MISTY_SCREENSHOT` capture.
const SCREENSHOT_FRAME: u64 = 60;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_project);
    let mut log = Log::open();
    let game = match Game::open(&root) {
        Ok(g) => g,
        Err(e) => {
            log.write(LogKind::Error, &format!("{}: {e}", root.display()));
            std::process::exit(1);
        }
    };
    log.write(LogKind::Info, &format!("Playing {}", root.display()));
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            log.write(LogKind::Error, &format!("could not create a window: {e}"));
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = Player {
        game,
        log,
        window: None,
        gfx: None,
        input: InputState::default(),
        last: Instant::now(),
        frames: 0,
        screenshot: std::env::var_os("MISTY_SCREENSHOT").map(PathBuf::from),
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        app.log.write(LogKind::Error, &e.to_string());
    }
}

struct Player {
    game: Game,
    log: Log,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    input: InputState,
    last: Instant,
    frames: u64,
    screenshot: Option<PathBuf>,
}

impl Player {
    /// One game frame: update, render, present.
    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        self.game.frame(dt, self.input.frame());
        for (kind, text) in self.game.take_logs() {
            self.log.write(kind, &text);
        }
        if self.game.quit {
            event_loop.exit();
            return;
        }
        let Some(gfx) = self.gfx.as_mut() else { return };
        let view = self.game.view();
        let frame = view.as_ref().map(|v| v.frame());
        gfx.draw(frame.as_ref(), &self.game.scene, self.game.ui_interaction());
        for w in gfx.take_warnings() {
            self.log.write(LogKind::Warning, &w);
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
}

impl ApplicationHandler for Player {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let build = &self.game.build;
        let mut attrs = Window::default_attributes()
            .with_title(self.game.product_name())
            .with_inner_size(PhysicalSize::new(build.width, build.height));
        if build.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.log.write(LogKind::Error, &format!("window: {e}"));
                event_loop.exit();
                return;
            }
        };
        match Gfx::new(window.clone()) {
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
        let size = window.inner_size();
        self.input.screen_resized(size.width, size.height);
        self.window = Some(window);
        self.last = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.input.screen_resized(size.width, size.height);
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => self.input.clear_held(),
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
                self.input
                    .cursor_pixels(Some(Vec2::new(position.x as f32, position.y as f32)));
            }
            WindowEvent::CursorLeft { .. } => self.input.cursor_pixels(None),
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

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// Game output: stderr plus `Player.log` next to the executable.
struct Log {
    file: Option<std::fs::File>,
}

impl Log {
    fn open() -> Self {
        let file = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("Player.log")))
            .and_then(|p| std::fs::File::create(p).ok());
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

/// The folder the executable sits in when it is a build, else the nearest `SampleProject`.
fn default_project() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    if let Some(dir) = &exe_dir
        && dir.join("Assets").is_dir()
    {
        return dir.clone();
    }
    [std::env::current_dir().ok(), exe_dir]
        .into_iter()
        .flatten()
        .flat_map(|start| {
            start
                .ancestors()
                .map(|a| a.join("SampleProject"))
                .collect::<Vec<_>>()
        })
        .find(|p| p.join("Assets").is_dir())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}
