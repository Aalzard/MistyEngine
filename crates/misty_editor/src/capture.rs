//! Self-screenshot for automated checks: `MISTY_SCREENSHOT=out.png misty` saves the window and exits.
//! See [`apply_env`] for `MISTY_SCENE` / `MISTY_ACTIVE_SCENE` / `MISTY_SELECT` / `MISTY_ASSET` / `MISTY_SCRIPT` / `MISTY_TAB` /
//! `MISTY_WORLD_PARTITION` / `MISTY_PLAY` / `MISTY_QUALITY_LEVEL` / `MISTY_RUN_TESTS` / `MISTY_RAGDOLL_WIZARD` / `MISTY_CLOTH_PAINT` / `MISTY_RELOAD_BAR` / `MISTY_RELOAD_TOAST` / `MISTY_STATS` / `MISTY_SEARCH` / `MISTY_ABOUT` / `MISTY_EXPAND_ALL` / `MISTY_FOLDER_DEMO` / `MISTY_VCS` / `MISTY_COLOR_FILTER` / `MISTY_TEXT_SCALE` / `MISTY_PROJECT_SETTINGS` / `MISTY_TERRAIN_TOOL` / `MISTY_TERRAIN_ITEM` / `MISTY_TERRAIN_HOVER` / `MISTY_GUIDE`, and `MISTY_THEME=light` (and
//! `MISTY_TUTORIAL`, see `tutorials::from_env`; `MISTY_SPLASH`, see `branding::splash`).
//! `MISTY_EDITOR_LANGUAGE=tr` (`en`, `tr`, `ar`) runs the editor and the Hub in that language
//! (see `i18n`), `MISTY_OPEN_MENU=File` opens that top menu ([`open_menu_from_env`]) and
//! `MISTY_PREFERENCES=1` opens Edit ▸ Preferences.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

/// When the process started (`main`'s first line): frame times in capture runs are reported
/// against it, so "time to interactive" includes the window, the project and the scene load.
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

pub fn mark_process_start() {
    PROCESS_START.get_or_init(Instant::now);
}

fn since_start_ms(t: Instant) -> f32 {
    let start = *PROCESS_START.get_or_init(Instant::now);
    (t - start).as_secs_f32() * 1000.0
}

/// Frames slower than this count as a freeze in the capture report.
const FREEZE_MS: f32 = 100.0;

/// Whole editor frame times of a capture run (wall clock between two editor frames).
#[derive(Default)]
struct FrameClock {
    last: Option<Instant>,
    /// Editor frames seen.
    frames: u32,
    /// Worst frame: (frame, ms).
    worst: (u32, f32),
    /// End of the last frame slower than [`FREEZE_MS`], ms since the process started.
    last_freeze_end: Option<f32>,
    /// Frames slower than [`FREEZE_MS`].
    freezes: u32,
    first_frame_ms: f32,
    /// Frame Play was pressed on (`MISTY_PLAY_AT`) and the worst frame since: (frame, ms).
    play_at: Option<u32>,
    play_worst: (u32, f32),
    /// Frame the scene was opened on (`MISTY_OPEN_SCENE_AT`) and the worst frame since.
    open_at: Option<u32>,
    open_worst: (u32, f32),
    /// End of the last frame over [`FREEZE_MS`] since the scene was opened, ms after start.
    open_freeze_end: Option<f32>,
    /// Last frame the views were still streaming in (grass chunks, terrain textures, trees):
    /// (frame, ms after start).
    last_loading: Option<(u32, f32)>,
}

impl FrameClock {
    fn tick(&mut self) {
        let now = Instant::now();
        self.frames += 1;
        match self.last.replace(now) {
            None => self.first_frame_ms = since_start_ms(now),
            Some(prev) => {
                let ms = (now - prev).as_secs_f32() * 1000.0;
                if ms > self.worst.1 {
                    self.worst = (self.frames, ms);
                }
                if self.play_at.is_some() && ms > self.play_worst.1 {
                    self.play_worst = (self.frames, ms);
                }
                if self.open_at.is_some() && ms > self.open_worst.1 {
                    self.open_worst = (self.frames, ms);
                }
                if self.open_at.is_some() && ms > FREEZE_MS {
                    self.open_freeze_end = Some(since_start_ms(now));
                }
                if ms > FREEZE_MS {
                    self.freezes += 1;
                    self.last_freeze_end = Some(since_start_ms(now));
                    eprintln!(
                        "slow frame {}: {ms:.0} ms (ends {:.0} ms after start)",
                        self.frames,
                        since_start_ms(now)
                    );
                }
            }
        }
    }

    fn report(&self) {
        eprintln!(
            "first editor frame {:.0} ms after start; worst frame {} at {:.0} ms; {} frames over {FREEZE_MS:.0} ms; interactive {:.0} ms after start",
            self.first_frame_ms,
            self.worst.0,
            self.worst.1,
            self.freezes,
            self.last_freeze_end.unwrap_or(self.first_frame_ms)
        );
        if let Some(at) = self.play_at {
            eprintln!(
                "entering Play at frame {at}: worst frame since {} at {:.0} ms",
                self.play_worst.0, self.play_worst.1
            );
        }
        if let Some((frame, t)) = self.last_loading {
            eprintln!(
                "scene complete (nothing streaming in) after frame {frame}, {t:.0} ms after start"
            );
        }
        if let Some(at) = self.open_at {
            eprintln!(
                "opening the scene at frame {at}: worst frame since {} at {:.0} ms; last frame over {FREEZE_MS:.0} ms ends {}",
                self.open_worst.0,
                self.open_worst.1,
                self.open_freeze_end
                    .map_or("never".to_owned(), |t| format!("{t:.0} ms after start"))
            );
        }
    }
}

pub struct Capture {
    path: PathBuf,
    frames: u32,
    requested: bool,
    /// Something asked to wait before the shot (see [`Capture::hold`]).
    held: bool,
    saved: bool,
    /// `MISTY_THEME=light`: egui's light visuals, to check theme-tinted icons (the editor
    /// itself only has the dark theme).
    light: bool,
    /// Frame times (ms) after the warm-up, printed when the screenshot is saved.
    times: Vec<f32>,
    /// Every editor frame's wall time (slow frames and time to interactive).
    clock: FrameClock,
}

impl Capture {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("MISTY_SCREENSHOT").map(|p| Self {
            path: p.into(),
            frames: 0,
            requested: false,
            held: false,
            saved: false,
            light: std::env::var("MISTY_THEME").is_ok_and(|t| t == "light"),
            times: Vec::new(),
            clock: FrameClock::default(),
        })
    }

    /// The screenshot was taken and the window is closing.
    pub fn saved(&self) -> bool {
        self.saved
    }

    /// `MISTY_PLAY_AT=<frame>`: press Play (as the toolbar button does) on that editor frame,
    /// once the scene has been open a while, to time entering Play on its own.
    pub fn play_due(&mut self) -> bool {
        let due = std::env::var("MISTY_PLAY_AT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .is_some_and(|at| at == self.frames);
        if due {
            self.clock.play_at = Some(self.frames);
        }
        due
    }

    /// `MISTY_OPEN_SCENE_AT=<frame>:<scene path>`: open that scene on that editor frame (as
    /// File ▸ Open Scene does), to time opening a scene in a running editor.
    pub fn scene_due(&mut self) -> Option<String> {
        let (at, rel) = open_scene_at()?;
        if at != self.frames {
            return None;
        }
        self.clock.open_at = Some(self.frames);
        Some(rel)
    }

    /// Keep the shot back while `waiting` (e.g. until Play mode reaches a moment, see
    /// `panels::timeline::capture_waiting`).
    pub fn hold(&mut self, waiting: bool) {
        self.held = waiting;
    }

    /// The views are still streaming in assets this frame (see [`FrameClock::last_loading`]).
    pub fn note_loading(&mut self, loading: bool) {
        if loading && !self.requested {
            self.clock.last_loading = Some((self.frames + 1, since_start_ms(Instant::now())));
        }
    }

    /// Call once per frame; requests a screenshot after warm-up and quits once saved.
    pub fn update(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        self.clock.tick();
        ctx.request_repaint();
        if self.light && self.frames == 1 {
            ctx.set_visuals(egui::Visuals::light());
        }
        // `MISTY_CAPTURE_FRAME=<n>` waits longer (e.g. to catch a later moment of Play mode).
        let at = std::env::var("MISTY_CAPTURE_FRAME")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(45);
        // The frame rate over the last frames before the shot (after a warm-up of half of
        // them): whole editor frames, as the Game view's Stats box counts them.
        if self.frames > at / 2 && !self.requested {
            self.times.push(ctx.input(|i| i.unstable_dt) * 1000.0);
        }
        if self.frames >= at && !self.requested && !self.held {
            let mut t = self.times.clone();
            t.sort_by(f32::total_cmp);
            if let Some(&median) = t.get(t.len() / 2) {
                let mean = t.iter().sum::<f32>() / t.len() as f32;
                let p90 = t[(t.len() * 9 / 10).min(t.len() - 1)];
                let p99 = t[(t.len() * 99 / 100).min(t.len() - 1)];
                eprintln!(
                    "frame time over {} frames: median {median:.2} ms ({:.0} FPS), mean {mean:.2} ms ({:.0} FPS), p90 {p90:.2} ms, p99 {p99:.2} ms",
                    t.len(),
                    1000.0 / median,
                    1000.0 / mean
                );
            }
            self.clock.report();
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

/// `MISTY_OPEN_MENU=<top menu>` (English name: `File`, `Object`…; old names work): that menu of the title
/// bar opens and stays open (screenshots of translated menus).
pub fn open_menu_from_env(ui: &egui::Ui, label: &str, button: &egui::Response) {
    static MENU: OnceLock<Option<String>> = OnceLock::new();
    let wanted = MENU.get_or_init(|| std::env::var("MISTY_OPEN_MENU").ok());
    if wanted
        .as_deref()
        .is_some_and(|w| crate::renames::top_menu(w.trim()).eq_ignore_ascii_case(label))
    {
        let id = egui::Popup::default_response_id(button);
        if !egui::Popup::is_id_open(ui.ctx(), id) {
            egui::Popup::open_id(ui.ctx(), id);
        }
    }
}

/// `MISTY_TERRAIN_HOVER=<x>,<z>`: the terrain brush cursor shown at that terrain-local point
/// (screenshots of the cursor without a mouse).
pub fn terrain_hover() -> Option<[f32; 2]> {
    static AT: OnceLock<Option<[f32; 2]>> = OnceLock::new();
    *AT.get_or_init(|| {
        let v = std::env::var("MISTY_TERRAIN_HOVER").ok()?;
        let (x, z) = v.split_once(',')?;
        Some([x.trim().parse().ok()?, z.trim().parse().ok()?])
    })
}

/// `MISTY_INSPECTOR_SCROLL=<px>`: the object Inspector scrolled down that far (screenshots of
/// long components).
pub fn inspector_scroll() -> Option<f32> {
    static PX: OnceLock<Option<f32>> = OnceLock::new();
    *PX.get_or_init(|| {
        std::env::var("MISTY_INSPECTOR_SCROLL")
            .ok()
            .and_then(|v| v.parse().ok())
    })
}

/// `MISTY_OPEN_SCENE_AT=<frame>:<scene path>`.
fn open_scene_at() -> Option<(u32, String)> {
    let v = std::env::var("MISTY_OPEN_SCENE_AT").ok()?;
    let (at, rel) = v.split_once(':')?;
    Some((at.trim().parse().ok()?, rel.trim().to_owned()))
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
            misty_engine::profile_scope!("Editor.LoadScene");
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
    // `MISTY_TERRAIN_TOOL=details` (raise, set_height, smooth, paint, holes, details, trees,
    // place, settings) opens that Terrain Inspector tool; `MISTY_TERRAIN_ITEM=2` selects that
    // palette item of it (texture layer, detail row, tree).
    if let Some(tool) = std::env::var_os("MISTY_TERRAIN_TOOL") {
        crate::terrain_tools::apply_env(
            s,
            &tool.to_string_lossy(),
            std::env::var("MISTY_TERRAIN_ITEM")
                .ok()
                .and_then(|v| v.parse().ok()),
        );
    }
    // `MISTY_EDIT_LIGHT_PROBES=1` turns on Edit Light Probes for the selected group.
    if std::env::var_os("MISTY_EDIT_LIGHT_PROBES").is_some() {
        s.light_probes.set_editing(true);
        s.light_probes.entity = s.selection.active();
    }
    // `MISTY_MESH_MODE=face` (vertex / edge / face, optionally `face:3,4` to select elements)
    // opens a Mesh Builder mode on the selected Editable Mesh.
    if let Some(mode) = std::env::var_os("MISTY_MESH_MODE") {
        crate::mesh_builder::apply_env(s, &mode.to_string_lossy());
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
    // `MISTY_CREATE=Folder` (or Material): Create ▸ that asset in the open Project folder, so
    // the shot shows the new item renaming (with `MISTY_ASSET` to pick the folder).
    if let Some(kind) = std::env::var_os("MISTY_CREATE") {
        let kind = kind.to_string_lossy();
        if let Some(k) = crate::actions::AssetKind::ALL
            .into_iter()
            .find(|k| k.label().eq_ignore_ascii_case(&kind))
        {
            s.push(crate::actions::Action::CreateAsset(k));
        }
    }
    // `MISTY_PREVIEW_SHAPE=plane` (sphere / cube / plane): the material Inspector preview's shape.
    if let Some(shape) = std::env::var_os("MISTY_PREVIEW_SHAPE") {
        use crate::material_preview::PreviewShape;
        s.project.thumbnails.materials.shape = match shape.to_string_lossy().as_ref() {
            "cube" => PreviewShape::Cube,
            "plane" => PreviewShape::Plane,
            _ => PreviewShape::Sphere,
        };
    }
    // `MISTY_MIXER=Assets/Audio/AudioDemo2.mixer` shows that mixer in the Audio Mixer window;
    // `MISTY_MIXER_SELECT=Music` selects a group (`Music/1`: also its second effect) for the
    // Inspector (combine with `MISTY_TAB=Audio Mixer`).
    if let Some(rel) = std::env::var_os("MISTY_MIXER") {
        let rel = rel.to_string_lossy().into_owned();
        s.audio_mixer.open(Some(rel.clone()));
        if let Some(sel) = std::env::var("MISTY_MIXER_SELECT").ok()
            && let Ok(m) = s.anim_clips.mixers.get(&rel)
        {
            let (name, fx) = sel.split_once('/').unwrap_or((&sel, ""));
            if let Some(g) = m.group_by_name(name) {
                let effect = fx.parse::<usize>().ok().and_then(|i| g.effects.get(i));
                let item = crate::panels::audio_mixer::MixerItem::Group(g.id, effect.map(|e| e.id));
                s.audio_mixer.selected = Some(item);
                s.selection.clear();
                s.inspector_asset = Some(rel);
            }
        }
    }
    // `MISTY_INPUT_ACTIONS=Assets/…/Player.inputactions` opens the Input Actions window;
    // `MISTY_INPUT_SELECT=Player/Move` selects an action, `Player/Move/2` its third binding and
    // `Player/Move/0.1` a composite part.
    if let Some(rel) = std::env::var_os("MISTY_INPUT_ACTIONS") {
        let rel = rel.to_string_lossy().into_owned();
        crate::panels::input_actions::open(s, &rel);
        if let Some(sel) = std::env::var_os("MISTY_INPUT_SELECT") {
            crate::panels::input_actions::select(s, &sel.to_string_lossy());
        }
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
    // `MISTY_SHADERGRAPH=Assets/Shaders/Dissolve.shadergraph` opens it in the Shader Graph window.
    if let Some(rel) = std::env::var_os("MISTY_SHADERGRAPH") {
        let rel = rel.to_string_lossy().into_owned();
        let file = s.assets.absolute(&rel);
        s.shader_graph.open(&file, &rel);
        crate::layout::focus_or_open(dock, crate::layout::Tab::ShaderGraph);
    }
    // `MISTY_EFFECTGRAPH=Assets/Effects/Fireflies.effect` opens it in the Effect Graph window.
    if let Some(rel) = std::env::var_os("MISTY_EFFECTGRAPH") {
        let rel = rel.to_string_lossy().into_owned();
        let file = s.assets.absolute(&rel);
        s.effect_graph.open(&file, &rel);
        crate::layout::focus_or_open(dock, crate::layout::Tab::EffectGraph);
    }
    // `MISTY_SCRIPTGRAPH=Assets/…/Door.vgraph` opens it in the Script Graph window;
    // `MISTY_GRAPH_BREAK=<node id>` puts a breakpoint on a node and `MISTY_GRAPH_SELECT=<ids>`
    // (comma separated) selects nodes.
    if let Some(rel) = std::env::var_os("MISTY_SCRIPTGRAPH") {
        let rel = rel.to_string_lossy().into_owned();
        let file = s.assets.absolute(&rel);
        s.script_graph.open(&file, &rel);
        let ids = |var: &str| -> Vec<u32> {
            std::env::var(var)
                .unwrap_or_default()
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect()
        };
        s.script_graph.breakpoints.extend(ids("MISTY_GRAPH_BREAK"));
        s.script_graph.selected.extend(ids("MISTY_GRAPH_SELECT"));
        // `MISTY_GRAPH_SEARCH=<text>` opens the node search with that text typed (empty = the
        // category tree).
        if let Ok(q) = std::env::var("MISTY_GRAPH_SEARCH") {
            let mut search = crate::panels::script_graph::search::Search::new(
                egui::pos2(760.0, 260.0),
                egui::pos2(0.0, 0.0),
                None,
                Vec::new(),
            );
            search.query = q;
            s.script_graph.search = Some(search);
        }
        s.script_graph.focused = true;
        // `MISTY_GRAPH_FRAME=1` frames the whole graph (as F does).
        if std::env::var_os("MISTY_GRAPH_FRAME").is_some() {
            s.script_graph.canvas.frame_all = true;
        }
        // `MISTY_GRAPH_PANELS=none` hides the Blackboard and the Graph Inspector.
        if std::env::var("MISTY_GRAPH_PANELS").is_ok_and(|v| v == "none") {
            s.script_graph.show_blackboard = false;
            s.script_graph.show_inspector = false;
        }
        crate::layout::focus_or_open(dock, crate::layout::Tab::ScriptGraph);
    }
    // `MISTY_RUN_TESTS=EditMode|PlayMode|1` runs the project's tests (both modes, to the end)
    // and opens the Test Runner on that mode; `MISTY_TEST_SELECT=<test name>` selects a test.
    if let Some(mode) = std::env::var_os("MISTY_RUN_TESTS") {
        let select = std::env::var("MISTY_TEST_SELECT").ok();
        crate::panels::test_runner::run_from_env(s, &mode.to_string_lossy(), select.as_deref());
        crate::layout::focus_or_open(dock, crate::layout::Tab::TestRunner);
    }
    // `MISTY_PREFAB_DEMO=overrides` gives the selected prefab instance a few overrides in memory
    // and opens its Overrides dropdown (`=bold` selects the overridden child instead, `=missing`
    // adds an instance of a prefab that doesn't exist); `MISTY_PREFAB=Assets/….prefab` opens
    // Prefab Mode (in context when the selected object is one of its instances).
    if let Some(demo) = std::env::var_os("MISTY_PREFAB_DEMO") {
        crate::panels::prefab_demo(s, &demo.to_string_lossy());
    }
    if let Some(rel) = std::env::var_os("MISTY_PREFAB") {
        let rel = rel.to_string_lossy().into_owned();
        let from = s
            .selection
            .active()
            .filter(|e| s.scene.prefab_link(*e).as_deref() == Some(rel.as_str()));
        crate::prefab_mode::open(s, &rel, from);
    }
    // `MISTY_PACKAGE=com.misty.example-effects` selects a package in the Package Manager;
    // `MISTY_GROUP_ENTRY=<asset path>` selects an entry in the Asset Groups window.
    if let Some(name) = std::env::var_os("MISTY_PACKAGE") {
        s.packages.selected = Some(name.to_string_lossy().into_owned());
    }
    if let Some(rel) = std::env::var_os("MISTY_GROUP_ENTRY") {
        s.asset_groups.selected_entry = Some(rel.to_string_lossy().into_owned());
    }
    // `MISTY_ANIMATOR=layers,machine:Emotes,select:Bow` sets up the Animator window for the
    // selected object (see `panels::animator::demo`).
    if let Some(spec) = std::env::var_os("MISTY_ANIMATOR") {
        crate::panels::animator::demo(s, &spec.to_string_lossy());
    }
    // Several tabs may be listed: `MISTY_TAB=Game,Profiler`.
    if let Some(titles) = std::env::var_os("MISTY_TAB") {
        for title in titles.to_string_lossy().split(',') {
            if let Some(tab) = crate::layout::Tab::from_title(title) {
                crate::layout::focus_or_open(dock, tab);
            }
        }
    }
    // `MISTY_GUIDE=<chapter>` opens Help ▸ User Guide on that chapter (`05-guide-first-person`).
    if let Ok(chapter) = std::env::var("MISTY_GUIDE") {
        s.guide.open(chapter.trim());
        crate::layout::focus_or_open(dock, crate::layout::Tab::Guide);
    }
    // `MISTY_TIMELINE_TIME=4.5` previews the Timeline window's director at that time.
    if let Some(t) = std::env::var_os("MISTY_TIMELINE_TIME")
        .and_then(|v| v.to_string_lossy().parse::<f32>().ok())
    {
        crate::panels::timeline::sync_target(s);
        s.timeline.time = t.max(0.0);
        s.timeline.preview = true;
    }
    // `MISTY_TIMELINE=record=<track>,clip=<track>:<index>` arms a track for recording and shows
    // a clip in the Inspector (see `panels::timeline::apply_env`).
    if let Some(v) = std::env::var_os("MISTY_TIMELINE") {
        crate::panels::timeline::apply_env(s, &v.to_string_lossy());
    }
    // `MISTY_RELOAD_BAR=1` shows the "changed on disk — Reload / Keep mine" bar for the active
    // scene; `MISTY_RELOAD_TOAST=3` the "Reloaded 3 assets" toast (live reload screenshots;
    // `MISTY_FAKE_NEW_BUILD=1` shows the "New build — Restart" pill, see `live_reload`).
    if std::env::var_os("MISTY_RELOAD_BAR").is_some()
        && let Some(rel) = s.scene.active_path()
    {
        s.live.scenes.conflicts.push(rel);
    }
    if let Some(n) = std::env::var("MISTY_RELOAD_TOAST")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        let text = crate::live_reload::reloaded_text(n);
        s.live.toast(text);
    }
    // `MISTY_UNDO_DEMO=1` makes a few named edits (for Undo History screenshots).
    if std::env::var_os("MISTY_UNDO_DEMO").is_some() {
        undo_demo(s);
    }
    vcs_env(s);
    accessibility_env(s);
    // `MISTY_OCCLUSION_PAGE=Object|Bake|Visualization` picks the Occlusion Culling window's page.
    if let Some(page) = std::env::var_os("MISTY_OCCLUSION_PAGE") {
        use crate::panels::occlusion::Page;
        s.occlusion.page = match page.to_string_lossy().as_ref() {
            "Object" => Page::Object,
            "Visualization" => Page::Visualization,
            _ => Page::Bake,
        };
    }
    // `MISTY_FRAME_DEBUGGER=1` enables the Frame Debugger (whole frame); a number selects that
    // event, other text the first event whose name contains it (e.g. `Draw Mesh Cube`).
    if let Some(v) = std::env::var_os("MISTY_FRAME_DEBUGGER") {
        let v = v.to_string_lossy().into_owned();
        crate::panels::frame_debugger::set_enabled(s, true);
        s.frame_debugger.pending_select = (v != "1").then_some(v);
    }
    // `MISTY_FRAME_DEBUGGER_VIEW=Depth` (Normals, GI, Emission) picks the view;
    // `MISTY_FRAME_DEBUGGER_GPU=1` captures GPU-driven draws.
    if let Ok(v) = std::env::var("MISTY_FRAME_DEBUGGER_VIEW")
        && let Some(view) = misty_engine::render::TargetView::ALL
            .into_iter()
            .find(|t| t.label().eq_ignore_ascii_case(&v))
    {
        s.frame_debugger.view = view;
    }
    if std::env::var_os("MISTY_FRAME_DEBUGGER_GPU").is_some() {
        s.frame_debugger.gpu_driven = true;
    }
    // `MISTY_MEMORY_CAPTURE=2` takes that many Memory Profiler snapshots once the views have
    // drawn; `MISTY_MEMORY_VIEW=Tree Map` picks the view.
    if let Some(n) = std::env::var("MISTY_MEMORY_CAPTURE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        s.memory_profiler.pending_captures = n;
    }
    if let Some(view) = std::env::var_os("MISTY_MEMORY_VIEW") {
        s.memory_profiler.set_view_named(&view.to_string_lossy());
    }
    // `MISTY_PROFILER_MODULE=GPU` selects a Profiler module (CPU, GPU, Rendering, Memory,
    // Physics, Audio, UI); `MISTY_PROFILER_VIEW=Timeline` shows the CPU Timeline;
    // `MISTY_PROFILER_LOAD=<capture.mprof>` shows a capture file instead of the editor.
    if let Some(m) = std::env::var("MISTY_PROFILER_MODULE")
        .ok()
        .and_then(|m| crate::panels::profiler::Module::named(&m))
    {
        s.profiler.module = m;
        s.profiler.reveal_module = true;
    }
    if std::env::var("MISTY_PROFILER_VIEW").is_ok_and(|v| v == "Timeline") {
        s.profiler.cpu_view = crate::panels::profiler::CpuView::Timeline;
    }
    if let Some(path) = std::env::var_os("MISTY_PROFILER_LOAD")
        && let Err(e) = s.profiler.load(path.into())
    {
        eprintln!("profiler capture: {e}");
    }
    // `MISTY_DOCK_SPLIT=0.4`: the first top / bottom split of the layout at that fraction (a
    // taller bottom panel for Profiler screenshots).
    if let Some(f) = std::env::var("MISTY_DOCK_SPLIT")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        && let Some(node) = dock
            .main_surface_mut()
            .iter_mut()
            .find(|n| matches!(n, egui_dock::Node::Vertical(_)))
        && let egui_dock::Node::Vertical(v) = node
    {
        v.fraction = f.clamp(0.1, 0.9);
    }
    // `MISTY_WORLD_PARTITION=around` streams World Partition cells around the Scene view camera
    // (Edit mode); `all` loads every cell; `none` loads none (only the HLOD proxies show).
    if let Some(mode) = std::env::var_os("MISTY_WORLD_PARTITION") {
        let w = &mut s.world_partition;
        match mode.to_string_lossy().as_ref() {
            "around" => w.stream_around_camera = true,
            "all" => {
                if let Some((_, wp)) = misty_engine::world_partition::find(&s.scene) {
                    w.streamer.load_all(&wp);
                }
            }
            _ => {}
        }
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
            .position(|m| *m == crate::renames::current_phrase(&mode.to_string_lossy()))
    {
        s.scene_view.draw_mode = i;
    }
    // `MISTY_GENERATE_LIGHTING=1` starts a lightmap bake (Lighting window progress bar).
    if std::env::var_os("MISTY_GENERATE_LIGHTING").is_some() {
        crate::lightmap_tools::start(s);
    }
    // `MISTY_BUILD=1` opens File ▸ Build Profiles; `MISTY_BUILD_PROFILE=<name>` shows that
    // profile (`Scene List`: the shared list); `MISTY_BUILD_REPORT=<report.json>` shows a
    // saved build report under it.
    if std::env::var_os("MISTY_BUILD").is_some() {
        s.build_window.open = true;
        if let Ok(name) = std::env::var("MISTY_BUILD_PROFILE") {
            s.build_window.page = if name == "Scene List" {
                crate::build_settings::Page::SceneList
            } else {
                s.build_profiles.index_of(&name).map_or(
                    crate::build_settings::Page::Active,
                    crate::build_settings::Page::Profile,
                )
            };
        }
        // `MISTY_BUILD_SCROLL=<px>` scrolls the profile page down (the report at the bottom).
        s.build_window.scroll_to = std::env::var("MISTY_BUILD_SCROLL")
            .ok()
            .and_then(|v| v.trim().parse().ok());
        if let Some(path) = std::env::var_os("MISTY_BUILD_REPORT") {
            s.build_window.report =
                misty_engine::build_report::BuildReport::load(std::path::Path::new(&path)).ok();
        }
    }
    // `MISTY_WORLD_TEMPLATE=Mountains` makes a terrain + World Builder from that template and
    // generates it (GameObject ▸ 3D Object ▸ World); `MISTY_WB_PREVIEW=Height|Relief|Biomes`
    // picks the World Builder's preview mode.
    if let Some(name) = std::env::var_os("MISTY_WORLD_TEMPLATE")
        && let Some(t) = misty_engine::world_gen::templates::Template::ALL
            .into_iter()
            .find(|t| t.label() == name.to_string_lossy())
    {
        crate::panels::world_builder::templates::create_world(s, t, None, true);
        crate::layout::focus_or_open(dock, crate::layout::Tab::WorldBuilder);
        // `MISTY_SELECT` still wins (the new world selects itself).
        if let Some(name) = std::env::var_os("MISTY_SELECT")
            && let Some(e) = s.scene.find_by_name(&name.to_string_lossy())
        {
            s.select(e);
        }
    }
    if let Some(mode) = std::env::var_os("MISTY_WB_PREVIEW")
        && let Some(m) = misty_engine::world_gen::preview::PreviewMode::ALL
            .into_iter()
            .find(|m| m.label() == mode.to_string_lossy())
    {
        s.world_builder.preview.mode = m;
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
        s.scene_view.camera.set_2d(true);
    }
    // `MISTY_TOOL=Rect` picks a transform tool (Hand, Move, Rotate, Scale, Rect, Transform).
    if let Some(name) = std::env::var_os("MISTY_TOOL")
        && let Some(t) = crate::state::Tool::ALL
            .into_iter()
            .find(|t| format!("{t:?}").eq_ignore_ascii_case(&name.to_string_lossy()))
    {
        s.tool = t;
    }
    // `MISTY_FRAME_SELECTED=1` frames the selection in the Scene view (like F).
    if std::env::var_os("MISTY_FRAME_SELECTED").is_some()
        && let Some((center, radius)) = crate::commands::selection_bounds(s)
    {
        s.scene_view.camera.frame(center, radius);
    }
    // `MISTY_UI_NAVIGATION=1` turns on Visualize Navigation (selectables' links in the Scene
    // view).
    if std::env::var_os("MISTY_UI_NAVIGATION").is_some() {
        crate::ui_gizmos::set_visualizing_navigation(true);
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
    // `MISTY_RAGDOLL_WIZARD=1` opens the Ragdoll Wizard for the selection; `MISTY_CLOTH_PAINT=1`
    // starts Edit Constraints on the selected Cloth.
    if std::env::var_os("MISTY_RAGDOLL_WIZARD").is_some() {
        crate::ragdoll_wizard::open(s, None);
    }
    if std::env::var_os("MISTY_CLOTH_PAINT").is_some() {
        crate::cloth_tools::set_editing(s.selection.active());
    }
    // `MISTY_PLAY=1` enters Play mode (live views such as the Audio Mixer meters).
    if std::env::var_os("MISTY_PLAY").is_some() {
        misty_engine::profile_scope!("Editor.EnterPlay");
        crate::play::start(s);
    }
    // `MISTY_PREFERENCES=1` opens Edit ▸ Preferences (the language picker).
    if std::env::var_os("MISTY_PREFERENCES").is_some() {
        s.code_editor.ui.prefs_open = true;
    }
    // `MISTY_WINDOW` / `MISTY_LOCALE` / `MISTY_SAVE_SLOT` (see `saveloc::apply_env`).
    crate::saveloc::apply_env(s);
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
/// `MISTY_VCS=1` opens Window ▸ Version Control; `MISTY_VCS_SELECT=Assets/…` selects a changed
/// file (`…:objects` shows a scene's object diff, `…:staged` its staged side);
/// `MISTY_VCS_MERGE=Assets/….scene` opens the merge helper for a conflicted scene.
fn vcs_env(s: &mut crate::state::EditorState) {
    let open = std::env::var_os("MISTY_VCS").is_some();
    let select = std::env::var("MISTY_VCS_SELECT").ok();
    let merge = std::env::var("MISTY_VCS_MERGE").ok();
    if !open && select.is_none() && merge.is_none() {
        return;
    }
    let root = s.project.root.clone();
    s.vcs.refresh_now(&root);
    s.vcs.window.open = true;
    if let Some(sel) = select {
        let mut parts = sel.split(':');
        let path = parts.next().unwrap_or_default().to_owned();
        let flags: Vec<&str> = parts.collect();
        if flags.contains(&"objects") {
            s.vcs.window.view = crate::vcs::window::DiffView::Objects;
        }
        s.vcs.window.selected = Some((path, flags.contains(&"staged")));
    }
    if let Some(rel) = merge
        && let Some(repo) = s.vcs.repo.clone()
    {
        match crate::vcs::window::open_merge(&repo, &rel) {
            Ok(m) => s.vcs.window.merge = Some((rel, m)),
            Err(e) => eprintln!("MISTY_VCS_MERGE: {e}"),
        }
    }
}

/// `MISTY_COLOR_FILTER=deuteranopia` (optionally `:simulate` and `:0.5` for the strength) and
/// `MISTY_TEXT_SCALE=1.5` set the accessibility settings in effect; `MISTY_PROJECT_SETTINGS=Player`
/// opens Project Settings on that page (`Player:Accessibility`: only that section open).
fn accessibility_env(s: &mut crate::state::EditorState) {
    if let Ok(v) = std::env::var("MISTY_COLOR_FILTER") {
        let mut parts = v.split(':');
        let mode = parts
            .next()
            .and_then(misty_engine::accessibility::ColorBlindMode::from_name);
        let rest: Vec<&str> = parts.collect();
        misty_engine::accessibility::update(|a| {
            if let Some(m) = mode {
                a.color_filter = m;
            }
            a.simulate = rest.contains(&"simulate");
            if let Some(k) = rest.iter().find_map(|r| r.parse::<f32>().ok()) {
                a.filter_strength = k;
            }
        });
    }
    if let Some(k) = std::env::var("MISTY_TEXT_SCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        misty_engine::accessibility::update(|a| a.text_scale = k);
    }
    if let Ok(spec) = std::env::var("MISTY_PROJECT_SETTINGS") {
        use crate::project_settings::Category;
        // `Player:Accessibility` also opens only that section.
        let (page, section) = spec.split_once(':').unwrap_or((&spec, ""));
        if !section.is_empty() {
            let _ = crate::project_settings::SCREENSHOT_SECTION.set(section.to_owned());
        }
        if let Some(c) = Category::ALL
            .into_iter()
            .find(|c| format!("{c:?}").eq_ignore_ascii_case(page.trim()))
        {
            s.project_settings_window.open = true;
            s.project_settings_window.category = c;
        }
    }
}

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

/// `MISTY_RENDER_SCALE` / `MISTY_UPSCALE` / `MISTY_FXAA` / `MISTY_UPSCALING` (`Off`,
/// `Quality`, `Balanced`, `Performance`) on the previewed quality level.
fn quality_env(s: &mut crate::state::EditorState) {
    use misty_engine::quality::{self, UpscaleFilter, Upscaling};
    // `MISTY_QUALITY_LEVEL=Low` (a level's name or index) previews that level.
    if let Ok(want) = std::env::var("MISTY_QUALITY_LEVEL") {
        let q = &s.project_settings.quality;
        let index = want.parse::<usize>().ok().or_else(|| {
            q.levels
                .iter()
                .position(|l| l.name.eq_ignore_ascii_case(want.trim()))
        });
        if let Some(i) = index {
            quality::install(q, i);
        }
    }
    let scale = std::env::var("MISTY_RENDER_SCALE").ok();
    let filter = std::env::var("MISTY_UPSCALE").ok();
    let fxaa = std::env::var_os("MISTY_FXAA").is_some();
    let upscaling = std::env::var("MISTY_UPSCALING").ok().and_then(|u| {
        Upscaling::ALL
            .into_iter()
            .find(|m| format!("{m:?}").eq_ignore_ascii_case(u.trim()))
    });
    if scale.is_none() && filter.is_none() && !fxaa && upscaling.is_none() {
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
    if let Some(u) = upscaling {
        level.upscaling = u;
    }
    quality::install(q, i);
}
