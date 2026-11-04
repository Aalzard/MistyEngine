//! Self-screenshot for automated checks: `MISTY_SCREENSHOT=out.png misty` saves the window and exits.
//! See [`apply_env`] for `MISTY_SCENE` / `MISTY_SELECT` / `MISTY_ASSET` / `MISTY_SCRIPT` / `MISTY_TAB` /
//! `MISTY_PLAY` / `MISTY_STATS`.

use std::path::PathBuf;

pub struct Capture {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

impl Capture {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("MISTY_SCREENSHOT").map(|p| Self {
            path: p.into(),
            frames: 0,
            requested: false,
        })
    }

    /// Call once per frame; requests a screenshot after warm-up and quits once saved.
    pub fn update(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        ctx.request_repaint();
        if self.frames == 45 && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            match save_png(&self.path, &image) {
                Ok(()) => eprintln!("screenshot saved to {}", self.path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Screenshot setup from the environment: `MISTY_SELECT=<object name>` selects an object,
/// `MISTY_TAB=<tab title>` brings a tab to the front (e.g. the Animation window) and
/// `MISTY_PLAY=1` enters Play mode.
pub fn apply_env(
    s: &mut crate::state::EditorState,
    dock: &mut egui_dock::DockState<crate::layout::Tab>,
) {
    // `MISTY_SCENE=Assets/Scenes/AudioDemo.scene` opens a scene first.
    if let Some(scene) = std::env::var_os("MISTY_SCENE") {
        crate::scene_files::load(s, &scene.to_string_lossy());
    }
    if let Some(name) = std::env::var_os("MISTY_SELECT")
        && let Some(e) = s.scene.find_by_name(&name.to_string_lossy())
    {
        s.select(e);
    }
    // `MISTY_VIEW=<object name>` puts the Scene view camera where that object is (e.g. Main Camera).
    if let Some(name) = std::env::var_os("MISTY_VIEW")
        && let Some(e) = s.scene.find_by_name(&name.to_string_lossy())
    {
        let (_, rot, pos) = s.scene.world_matrix(e).to_scale_rotation_translation();
        s.scene_view.camera.align_to(pos, rot);
    }
    // `MISTY_ASSET=Assets/…` selects a Project asset (its folder is opened, the Inspector shows it).
    if let Some(rel) = std::env::var_os("MISTY_ASSET") {
        let rel = rel.to_string_lossy().into_owned();
        let abs = s.assets.absolute(&rel);
        if let Some(dir) = abs.parent() {
            s.project.current = dir.to_path_buf();
            s.project.refresh();
        }
        s.project.selected = Some(abs);
        s.selection.clear();
        s.inspector_asset = Some(rel);
    }
    // `MISTY_SCRIPT=Assets/Scripts/Rotator.lua` (optionally `:line`) opens it in the Code Editor.
    if let Some(script) = std::env::var_os("MISTY_SCRIPT") {
        let script = script.to_string_lossy().into_owned();
        let (rel, line) = match script.rsplit_once(':') {
            Some((r, l)) if l.parse::<usize>().is_ok() => (r.to_owned(), l.parse().ok()),
            _ => (script, None),
        };
        if s.code_editor.open(&rel, line).is_ok() {
            crate::layout::focus_or_open(dock, crate::layout::Tab::CodeEditor);
        }
    }
    // Several tabs may be listed: `MISTY_TAB=Game,Profiler`.
    if let Some(titles) = std::env::var_os("MISTY_TAB") {
        for title in titles.to_string_lossy().split(',') {
            if let Some(tab) = crate::layout::Tab::every().find(|t| t.title() == title.trim()) {
                crate::layout::focus_or_open(dock, tab);
            }
        }
    }
    // `MISTY_STATS=1` shows the Game view's Stats box.
    if std::env::var_os("MISTY_STATS").is_some() {
        s.game_view.stats = true;
    }
    // `MISTY_SETTINGS=Input Manager` opens Project Settings on that page.
    if let Some(page) = std::env::var_os("MISTY_SETTINGS")
        && let Some(c) = crate::project_settings::Category::ALL
            .into_iter()
            .find(|c| c.title() == page.to_string_lossy())
    {
        crate::project_settings::open_at(s, c);
    }
    // `MISTY_2D=1` switches the Scene view to 2D (looking down +Z at the XY plane).
    if std::env::var_os("MISTY_2D").is_some() {
        s.scene_view.is_2d = true;
    }
    // `MISTY_PLAY=1` enters Play mode (live views such as the Audio Mixer meters).
    if std::env::var_os("MISTY_PLAY").is_some() {
        crate::play::start(s);
    }
}

fn save_png(path: &PathBuf, image: &egui::ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    encoder.write_header()?.write_image_data(&bytes)?;
    Ok(())
}
