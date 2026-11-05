//! Self-screenshot for automated checks: `MISTY_SCREENSHOT=out.png misty` saves the window and exits.
//! See [`apply_env`] for `MISTY_SCENE` / `MISTY_ACTIVE_SCENE` / `MISTY_SELECT` / `MISTY_ASSET` / `MISTY_SCRIPT` / `MISTY_TAB` /
//! `MISTY_PLAY` / `MISTY_STATS` / `MISTY_SEARCH` / `MISTY_ABOUT` / `MISTY_EXPAND_ALL` / `MISTY_FOLDER_DEMO`, and `MISTY_THEME=light` (and
//! `MISTY_TUTORIAL`, see `tutorials::from_env`; `MISTY_SPLASH`, see `branding::splash`).

use std::path::PathBuf;

pub struct Capture {
    path: PathBuf,
    frames: u32,
    requested: bool,
    saved: bool,
    /// `MISTY_THEME=light`: egui's light visuals, to check theme-tinted icons (the editor
    /// itself only has the dark theme).
    light: bool,
}

impl Capture {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("MISTY_SCREENSHOT").map(|p| Self {
            path: p.into(),
            frames: 0,
            requested: false,
            saved: false,
            light: std::env::var("MISTY_THEME").is_ok_and(|t| t == "light"),
        })
    }

    /// The screenshot was taken and the window is closing.
    pub fn saved(&self) -> bool {
        self.saved
    }

    /// Call once per frame; requests a screenshot after warm-up and quits once saved.
    pub fn update(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        ctx.request_repaint();
        if self.light && self.frames == 1 {
            ctx.set_visuals(egui::Visuals::light());
        }
        // `MISTY_CAPTURE_FRAME=<n>` waits longer (e.g. to catch a later moment of Play mode).
        let at = std::env::var("MISTY_CAPTURE_FRAME")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(45);
        if self.frames >= at && !self.requested {
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
            self.saved = true;
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
    // `MISTY_SCENE=Assets/Scenes/AudioDemo.scene` opens a scene first; more scenes after
    // commas are opened additively (`MISTY_SCENE=A.scene,B.scene`).
    if let Some(scenes) = std::env::var_os("MISTY_SCENE") {
        let scenes = scenes.to_string_lossy().into_owned();
        let mut list = scenes.split(',').map(str::trim).filter(|p| !p.is_empty());
        if let Some(first) = list.next() {
            crate::scene_files::load(s, first);
        }
        for rel in list {
            crate::open_scenes::open_additive(s, rel);
        }
    }
    // `MISTY_ACTIVE_SCENE=<scene name>` makes one of the open scenes active.
    if let Some(name) = std::env::var_os("MISTY_ACTIVE_SCENE")
        && let Some(id) = s.scene.find_loaded(&name.to_string_lossy())
    {
        crate::open_scenes::set_active(s, id);
    }
    if let Some(name) = std::env::var_os("MISTY_SELECT")
        && let Some(e) = s.scene.find_by_name(&name.to_string_lossy())
    {
        s.select(e);
    }
    // `MISTY_EXPAND_ALL=1` opens every Hierarchy parent and Project folder (tree screenshots).
    if std::env::var_os("MISTY_EXPAND_ALL").is_some() {
        expand_all(s);
    }
    // `MISTY_FOLDER_DEMO=1` groups the scene's objects into coloured Hierarchy folders
    // (`=picker` also opens the colour wheel on the first one).
    if let Some(v) = std::env::var_os("MISTY_FOLDER_DEMO") {
        folder_demo(s, v == "picker");
    }
    // `MISTY_PRESET_PICKER=Light` opens that component's (or asset kind's) preset picker.
    if let Some(kind) = std::env::var_os("MISTY_PRESET_PICKER") {
        s.preset_picker_request = Some(kind.to_string_lossy().into_owned());
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
    // `MISTY_UNDO_DEMO=1` makes a few named edits (for Undo History screenshots).
    if std::env::var_os("MISTY_UNDO_DEMO").is_some() {
        undo_demo(s);
    }
    // `MISTY_STATS=1` shows the Game view's Stats box.
    if std::env::var_os("MISTY_STATS").is_some() {
        s.game_view.stats = true;
    }
    // `MISTY_SETTINGS=Input Manager` opens Project Settings on that page.
    if let Some(page) = std::env::var_os("MISTY_SETTINGS")
        && let Some(c) = crate::project_settings::Category::from_title(&page.to_string_lossy())
    {
        crate::project_settings::open_at(s, c);
    }
    // `MISTY_VIEW=x,y,z,pitch,yaw` places the Scene view camera; `MISTY_VIEW=<object name>`
    // puts it where that object is (e.g. Main Camera).
    if let Some(view) = std::env::var_os("MISTY_VIEW") {
        let view = view.to_string_lossy();
        let v: Vec<f32> = view
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        if let [x, y, z, pitch, yaw] = v[..] {
            let rotation = misty_engine::camera::euler_to_quat(glam::Vec3::new(pitch, yaw, 0.0));
            s.scene_view
                .camera
                .align_to(glam::Vec3::new(x, y, z), rotation);
        } else if let Some(e) = s.scene.find_by_name(&view) {
            let (_, rot, pos) = s.scene.world_matrix(e).to_scale_rotation_translation();
            s.scene_view.camera.align_to(pos, rot);
        }
    }
    // `MISTY_DRAW_MODE=Shadow Cascades` picks a Scene view draw mode.
    if let Some(mode) = std::env::var_os("MISTY_DRAW_MODE")
        && let Some(i) = crate::panels::scene::DRAW_MODES
            .iter()
            .position(|m| *m == mode.to_string_lossy())
    {
        s.scene_view.draw_mode = i;
    }
    // `MISTY_BUILD=1` opens File ▸ Build Settings.
    if std::env::var_os("MISTY_BUILD").is_some() {
        s.build_window.open = true;
    }
    // `MISTY_TERRAIN_TOOL=Paint Texture` picks a Terrain Inspector tool tab.
    if let Some(tool) = std::env::var_os("MISTY_TERRAIN_TOOL")
        && let Some(t) = crate::terrain_tools::TerrainTool::ALL
            .into_iter()
            .find(|t| t.label() == tool.to_string_lossy())
    {
        s.terrain_tools.tool = t;
    }
    // `MISTY_2D=1` switches the Scene view to 2D (looking down +Z at the XY plane).
    if std::env::var_os("MISTY_2D").is_some() {
        s.scene_view.is_2d = true;
    }
    // `MISTY_ALIGN_VIEW=1` puts the Scene view camera where the selected object is (with
    // `MISTY_SELECT=Main Camera`: the Scene view sees what the game camera sees).
    if std::env::var_os("MISTY_ALIGN_VIEW").is_some()
        && let Some(e) = s.selection.active()
    {
        let (_, rot, pos) = s.scene.world_matrix(e).to_scale_rotation_translation();
        s.scene_view.camera.align_to(pos, rot);
    }
    // `MISTY_SEARCH=t:Light` opens the Ctrl+K palette with that text (asset index built now).
    if let Some(text) = std::env::var_os("MISTY_SEARCH") {
        s.search.index.build_now();
        crate::search::open(s, &text.to_string_lossy());
    }
    // `MISTY_GIZMOS_MENU=1` opens the Scene view Gizmos ▾ dropdown; `MISTY_ICON_SIZE=1.5`
    // and `MISTY_ICON_COLOR=255,220,0` set the icon size and tint.
    if std::env::var_os("MISTY_GIZMOS_MENU").is_some() {
        s.scene_view.open_icon_menu = true;
    }
    if let Some(size) = std::env::var("MISTY_ICON_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        s.scene_view.icons.size = size;
    }
    if let Ok(color) = std::env::var("MISTY_ICON_COLOR")
        && let [r, g, b] = color
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect::<Vec<u8>>()[..]
    {
        s.scene_view.icons.color = [r, g, b];
    }
    // `MISTY_QUALITY_MENU=1` opens the Scene view Quality dropdown; `MISTY_RENDER_SCALE=0.25`,
    // `MISTY_UPSCALE=Nearest|Bilinear|Sharpen` and `MISTY_FXAA=1` change the previewed level in
    // memory only (Project Settings are not saved).
    if std::env::var_os("MISTY_QUALITY_MENU").is_some() {
        s.scene_view.open_quality_menu = true;
    }
    quality_env(s);
    // `MISTY_ABOUT=1` opens Help ▸ About Misty Engine.
    if std::env::var_os("MISTY_ABOUT").is_some() {
        s.about_open = true;
    }
    // `MISTY_PLAY=1` enters Play mode (live views such as the Audio Mixer meters).
    if std::env::var_os("MISTY_PLAY").is_some() {
        crate::play::start(s);
    }
}

/// Folders for screenshots: three coloured, one plain; the scene's top-level objects are
/// shared out between them. Never saved.
fn folder_demo(s: &mut crate::state::EditorState, picker: bool) {
    use crate::panels::folder_colors::pastel;
    use misty_engine::scene::HierarchyFolder;
    let roots = s.scene.roots().to_vec();
    let specs = [
        ("Environment", Some(pastel(0.36, 0.4))),
        ("Lighting", Some(pastel(0.08, 0.4))),
        ("Interface", Some(pastel(0.72, 0.4))),
        ("Props", None),
    ];
    let folders: Vec<_> = specs
        .iter()
        .map(|(name, color)| {
            let f = s.scene.spawn(name, None);
            let _ = s
                .scene
                .world
                .insert_one(f, HierarchyFolder { color: *color });
            s.hierarchy.expanded.insert(f);
            f
        })
        .collect();
    for (i, e) in roots.into_iter().enumerate() {
        s.scene.set_parent(e, Some(folders[i % 3]), None, true);
    }
    if picker {
        s.hierarchy.folder_picker = Some((folders[0], egui::pos2(150.0, 170.0)));
    }
    s.scene_dirty = false;
}

fn expand_all(s: &mut crate::state::EditorState) {
    let parents: Vec<_> = s.scene.walk().into_iter().map(|(e, _)| e).collect();
    for e in parents {
        if !s.scene.children(e).is_empty() {
            s.hierarchy.expanded.insert(e);
        }
    }
    fn folders(n: &crate::project::FolderNode, out: &mut Vec<std::path::PathBuf>) {
        out.push(n.path.clone());
        n.children.iter().for_each(|c| folders(c, out));
    }
    let mut all = Vec::new();
    folders(&s.project.tree, &mut all);
    s.project.expanded.extend(all);
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

/// Real edits, each recorded like the editor does: create, drag, Inspector change, rename,
/// duplicate; the last two are then undone so the window shows redoable steps.
fn undo_demo(s: &mut crate::state::EditorState) {
    use crate::actions::{Action, Create};
    use misty_engine::scene::{Light, PrimitiveMesh};
    let create = |what| Action::Create { what, parent: None };
    crate::commands::apply(s, create(Create::Primitive(PrimitiveMesh::Cube)));
    let Some(cube) = s.selection.active() else {
        return;
    };
    // A gizmo drag: one step for the whole interaction.
    s.undo.begin_frame(true, &s.scene, &s.selection, || None);
    for i in 1..=5 {
        let mut t = s.scene.transform(cube);
        t.position.x = i as f32 * 0.4;
        s.scene.set_transform(cube, t);
        s.mark_edited(&format!("Move {}", s.scene.name(cube)));
    }
    s.undo.end_frame(false);
    // A slider drag on a light's Intensity.
    let light = s
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .find(|e| s.scene.world.get::<&Light>(*e).is_ok());
    if let Some(l) = light {
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
        let before = (*s.scene.world.get::<&Light>(l).unwrap()).clone();
        let mut after = before.clone();
        after.intensity *= 1.5;
        let label = crate::undo_names::struct_change("Light", &before, &after);
        let _ = s.scene.world.insert_one(l, after);
        s.mark_edited(&label);
        s.undo.end_frame(false);
    }
    crate::commands::apply(s, create(Create::Primitive(PrimitiveMesh::Sphere)));
    s.select(cube);
    s.record(&format!("Rename {}", s.scene.name(cube)));
    s.scene.game_object_mut(cube).name = "Crate".into();
    crate::commands::apply(s, Action::DuplicateSelected);
    crate::commands::apply(s, Action::DeleteSelected);
    s.undo_step();
    s.undo_step();
    // Demo edits are never saved: closing after the screenshot must not write the scene.
    s.scene_dirty = false;
}

/// `MISTY_RENDER_SCALE` / `MISTY_UPSCALE` / `MISTY_FXAA` on the previewed quality level.
fn quality_env(s: &mut crate::state::EditorState) {
    use misty_engine::quality::{self, UpscaleFilter};
    let scale = std::env::var("MISTY_RENDER_SCALE").ok();
    let filter = std::env::var("MISTY_UPSCALE").ok();
    let fxaa = std::env::var_os("MISTY_FXAA").is_some();
    if scale.is_none() && filter.is_none() && !fxaa {
        return;
    }
    let q = &mut s.project_settings.quality;
    let i = quality::level().min(q.levels.len().saturating_sub(1));
    let Some(level) = q.levels.get_mut(i) else {
        return;
    };
    if let Some(v) = scale.and_then(|v| v.parse().ok()) {
        level.render_scale = v;
    }
    if let Some(f) = filter.and_then(|f| {
        UpscaleFilter::ALL
            .into_iter()
            .find(|u| format!("{u:?}") == f)
    }) {
        level.upscale_filter = f;
    }
    level.fxaa |= fxaa;
    quality::install(q, i);
}
