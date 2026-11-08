//! Editor application: frame layout, shortcuts and action handling.

use std::path::PathBuf;

use egui::{Key, KeyboardShortcut, Modifiers};
use egui_dock::{DockArea, DockState};

use crate::actions::{Action, Create};
use crate::capture::Capture;
use crate::console::LogLevel;
use crate::layout::{self, BuiltinLayout, Tab};
use crate::layouts::{self, Autosave, LayoutRef};
use crate::panels::{PanelViewer, animation};
use crate::project::ProjectBrowser;
use crate::scene_files::{self, AfterPrompt};
use crate::state::{EditorState, PlayState, Tool};
use crate::viewport::Gpu;
use crate::{
    build_settings, commands, game_input, menu_bar, play, status_bar, theme, title_bar, toolbar,
};

pub struct EditorApp {
    state: EditorState,
    dock: DockState<Tab>,
    gpu: Option<Gpu>,
    capture: Option<Capture>,
    /// Layout to restore after "Play Maximized".
    saved_dock: Option<DockState<Tab>>,
    /// Error count seen so far (for Error Pause).
    errors_seen: usize,
    /// Cursor lock / visibility last sent to the window (`Cursor.lockState` while playing).
    cursor_applied: misty_engine::scripting::CursorState,
    /// Shader pipelines built so far (a rise shows "Preparing shaders" in the status bar).
    shaders_seen: u64,
    /// When to write the pipeline cache: a few seconds after the last shader compiled.
    pipeline_cache_due: Option<std::time::Instant>,
    /// Last window title sent.
    title: String,
    /// Writes the layout to the project's Library folder.
    layout_autosave: Autosave,
}

impl EditorApp {
    /// Open `project` in the window (the splash is on screen meanwhile when loading is slow, see
    /// `launcher`; also used when the Hub hands over).
    pub fn start(
        ctx: &egui::Context,
        render_state: Option<egui_wgpu::RenderState>,
        project: PathBuf,
        updater: crate::updater::Updater,
    ) -> Self {
        theme::apply(ctx);
        // Asset GUIDs: scan the meta files (creating missing ones) before anything loads.
        let guids = {
            misty_engine::profile_scope!(
                "Editor.AssetDb",
                misty_engine::profiler::Category::Others
            );
            misty_engine::asset_db::open(&project)
        };
        let browser = {
            misty_engine::profile_scope!(
                "Editor.ProjectBrowser",
                misty_engine::profiler::Category::Others
            );
            ProjectBrowser::open(project)
        };
        let mut state = {
            misty_engine::profile_scope!("Editor.State", misty_engine::profiler::Category::Others);
            EditorState::new(browser)
        };
        state.assets.note_guids(guids);
        {
            misty_engine::profile_scope!(
                "Editor.OpenInitialScene",
                misty_engine::profiler::Category::Others
            );
            scene_files::open_initial(&mut state);
        }
        scene_files::restore_scene_view(&mut state);
        misty_engine::profile_scope!("Editor.Layouts", misty_engine::profiler::Category::Others);
        let (saved_layouts, dock) = layouts::load(&state.project.root);
        state.layouts = saved_layouts;
        let mut dock = dock;
        // A restart into a new build (`misty --restore <file>`): put the session back.
        if let Some(r) = crate::live_reload::restore::take_pending(&state.project.root) {
            crate::live_reload::restore::apply(&mut state, &mut dock, r);
        }
        let layout_autosave = Autosave::new(&state.layouts, &dock);
        // Run from a source checkout: watch target/ for newer builds of the editor.
        state.live.dev = crate::live_reload::dev_build::DevBuild::detect();
        let capture = Capture::from_env();
        if capture.is_some() {
            misty_engine::profile_scope!(
                "Editor.CaptureEnv",
                misty_engine::profiler::Category::Others
            );
            crate::capture::apply_env(&mut state, &mut dock);
        }
        crate::tutorials::from_env(&mut state);
        state.ui_scale = crate::a11y::UiScale::load();
        // Version control badges: poll the project's git repository (if any).
        let root = state.project.root.clone();
        state.vcs.start(&root);
        // Updater: the one this process runs (started by the launcher, passed on from the Hub).
        state.updater = updater;
        let gpu = {
            misty_engine::profile_scope!(
                "Editor.GpuInit",
                misty_engine::profiler::Category::Rendering
            );
            render_state.map(Gpu::new)
        };
        match &gpu {
            Some(g) => {
                let info = g.state.adapter.get_info();
                if capture.is_some() {
                    eprintln!("renderer: {} ({:?})", info.name, info.backend);
                }
                state
                    .console
                    .info(format!("Renderer: {} ({:?})", info.name, info.backend));
            }
            None => state
                .console
                .warn("wgpu renderer unavailable; 3D views are disabled."),
        }
        Self {
            state,
            dock,
            gpu,
            capture,
            saved_dock: None,
            errors_seen: 0,
            cursor_applied: Default::default(),
            shaders_seen: misty_engine::render::pipelines_built(),
            pipeline_cache_due: None,
            title: String::new(),
            layout_autosave,
        }
    }

    /// File ▸ Open Misty Hub was confirmed: `Some(new_project)` once (the launcher then shows
    /// the Hub, after calling `on_exit`).
    pub fn take_open_hub(&mut self) -> Option<bool> {
        self.state.files.open_hub.take()
    }

    /// Hand the updater on (back to the Hub); the editor keeps a disabled one.
    pub fn take_updater(&mut self) -> crate::updater::Updater {
        std::mem::take(&mut self.state.updater)
    }

    /// Start tutorial `id` (Learn ▸ Start in the Hub, once the project has loaded).
    pub fn start_tutorial(&mut self, id: &str) {
        if !crate::tutorials::launch(&mut self.state, id) {
            self.state.console.warn(format!("No tutorial '{id}'."));
        }
    }

    /// Report how long opening the project took (Console).
    pub fn note_load_time(&mut self, took: std::time::Duration) {
        self.state
            .console
            .info(format!("Project opened in {} ms.", took.as_millis()));
    }

    /// Save the layout if it changed (debounced), or now when `force` is set.
    /// During Play Maximized the layout from before Play is the one kept.
    fn save_layout(&mut self, force: bool) {
        let dock = self.saved_dock.as_ref().unwrap_or(&self.dock);
        let s = &self.state;
        self.layout_autosave
            .tick(&s.project.root, &s.layouts, dock, force);
    }

    /// The Hub shows the last view the user was looking at as the project's preview.
    fn save_hub_thumbnail(&self) {
        let Some((w, h, rgba)) = self.gpu.as_ref().and_then(|g| g.last_view_pixels()) else {
            return;
        };
        let Some(img) = crate::project_thumbnail::fit(w, h, rgba) else {
            return;
        };
        if let Err(e) = crate::project_thumbnail::save(&self.state.project.root, &img) {
            eprintln!("project thumbnail not saved: {e}");
        }
    }

    /// Window ▸ Layouts commands.
    fn apply_layout(&mut self, action: Action) {
        let s = &mut self.state;
        match action {
            Action::LoadLayout(layout) => {
                let Some(dock) = s.layouts.get(&layout) else {
                    return;
                };
                self.dock = dock;
                self.saved_dock = None;
                s.layouts.current_name = match layout {
                    LayoutRef::Builtin(b) => b.name().to_owned(),
                    LayoutRef::User(name) => name,
                };
            }
            Action::SaveLayout(name) => {
                let dock = self.saved_dock.as_ref().unwrap_or(&self.dock).clone();
                s.layouts.user.insert(name.clone(), dock);
                s.console.info(format!("Saved layout \"{name}\"."));
                s.layouts.current_name = name;
            }
            Action::DeleteLayout(name) => {
                s.layouts.user.remove(&name);
            }
            Action::ResetAllLayouts => {
                s.layouts.user.clear();
                s.layouts.current_name = BuiltinLayout::Default.name().to_owned();
                self.dock = layout::default_layout();
                self.saved_dock = None;
            }
            _ => return,
        }
        self.save_layout(true);
    }

    /// Global keyboard shortcuts (skipped while typing in a text field).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+K toggles the search palette; while it is open it owns the keyboard.
        if self.state.search.open {
            if ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::K))
            }) {
                crate::search::close(&mut self.state);
            }
            return;
        }
        // The Code Editor owns the keyboard while its text has focus (only Play keys stay global).
        let code_editor = crate::panels::code_editor::global_shortcuts(ctx, &mut self.state);
        // Ctrl+K is a chord prefix inside the Code Editor, so it only opens search elsewhere.
        if !code_editor
            && ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::K))
            })
        {
            crate::search::open(&mut self.state, "");
            return;
        }
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let ctrl_alt = Modifiers::COMMAND | Modifiers::ALT;
        let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
        let create_empty = Action::Create {
            what: Create::Empty,
            parent: None,
        };
        let bindings = [
            (ctrl, Key::P, Action::TogglePlay),
            (ctrl_shift, Key::P, Action::TogglePause),
            (ctrl_alt, Key::P, Action::Step),
            (ctrl, Key::N, Action::NewScene),
            (ctrl, Key::O, Action::OpenScene),
            (ctrl, Key::S, Action::SaveScene),
            (ctrl_shift, Key::S, Action::SaveSceneAs),
            (ctrl_shift, Key::R, Action::ReloadScene),
            (ctrl_shift, Key::B, Action::OpenBuildSettings),
            (ctrl, Key::B, Action::BuildAndRun),
            (ctrl_shift, Key::N, create_empty),
            (ctrl_shift, Key::G, Action::CreateEmptyParent),
            (ctrl, Key::D, Action::DuplicateSelected),
            (ctrl, Key::A, Action::SelectAll),
            (ctrl, Key::Z, Action::Undo),
            (ctrl, Key::Y, Action::Redo),
            (ctrl_shift, Key::Z, Action::Redo),
            (ctrl, Key::H, Action::FocusTab(Tab::UndoHistory)),
            (ctrl_alt, Key::F, Action::MoveToView),
            (ctrl_shift, Key::F, Action::AlignWithView),
            (alt_shift, Key::A, Action::ToggleActive),
            (ctrl, Key::Num1, Action::FocusTab(Tab::Scene)),
            (ctrl, Key::Num2, Action::FocusTab(Tab::Game)),
            (ctrl, Key::Num3, Action::FocusTab(Tab::Inspector)),
            (ctrl, Key::Num4, Action::FocusTab(Tab::Hierarchy)),
            (ctrl, Key::Num5, Action::FocusTab(Tab::Project)),
            (ctrl, Key::Num6, Action::FocusTab(Tab::Animation)),
            (ctrl, Key::Num7, Action::FocusTab(Tab::Profiler)),
            (ctrl_shift, Key::C, Action::FocusTab(Tab::Console)),
            (Modifiers::SHIFT, Key::D, Action::DeselectAll),
            (Modifiers::SHIFT, Key::F, Action::ToggleLockView),
            (Modifiers::NONE, Key::Delete, Action::DeleteSelected),
            (Modifiers::NONE, Key::F2, Action::RenameSelected),
        ];
        // Most specific modifiers first so Ctrl+Shift+P is not eaten by Ctrl+P.
        let mut sorted: Vec<_> = bindings.into_iter().collect();
        sorted.sort_by_key(|(m, _, _)| std::cmp::Reverse(m.shift as u8 + m.alt as u8));
        // Plain keys belong to text fields while typing, and to the game while it has focus.
        let typing = ctx.egui_wants_keyboard_input()
            || (self.state.play.in_play_mode() && self.state.game_view.focused);
        // Flying with right mouse: WASD/QE (and Shift for speed) move the camera, so Shift+D
        // must not deselect and Shift+F must not lock the view.
        let flying = self.state.scene_view.camera.drag == crate::scene_camera::Drag::Look;
        // Mesh Builder modes (1–4) and operations first, so Delete removes elements.
        crate::mesh_builder::shortcuts(ctx, &mut self.state, typing || code_editor);
        for (mods, key, action) in sorted {
            // While typing, text fields own plain keys, Shift+keys and their own undo.
            let text_key = mods == Modifiers::NONE
                || mods == Modifiers::SHIFT
                || matches!(key, Key::Z | Key::Y | Key::A | Key::D);
            if (typing || flying) && text_key {
                continue;
            }
            if code_editor
                && !matches!(
                    action,
                    Action::TogglePlay | Action::TogglePause | Action::Step
                )
            {
                continue;
            }
            // Delete removes selected keys / states when the Animation / Animator window has focus.
            // The Shader Graph window handles its own Delete, undo/redo, save and duplicate.
            if (self.state.shader_graph.wants_keys() || self.state.effect_graph.wants_keys())
                && matches!(key, Key::Delete | Key::Z | Key::Y | Key::S | Key::D)
            {
                continue;
            }
            // Delete / Ctrl+D act on the light probes being edited.
            if matches!(key, Key::Delete | Key::D) && self.state.light_probes.wants_keys() {
                continue;
            }
            if key == Key::Delete
                && (self.state.animation.wants_delete()
                    || self.state.animator.wants_delete()
                    || self.state.timeline.wants_delete()
                    || self.state.splines.wants_delete())
            {
                continue;
            }
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key)))
                && let Some(action) = crate::asset_actions::redirect(&mut self.state, action)
            {
                self.state.push(action);
            }
        }
        if !typing && !code_editor {
            // Editor script menu shortcuts, after the built-in ones.
            crate::editor_scripts::menus::shortcuts(ctx, &mut self.state);
        }
        if !typing && !self.state.project.focused {
            self.clipboard_shortcuts(ctx);
        }
        if !code_editor
            && ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl, Key::R)))
        {
            self.state.refresh_assets();
        }
        // Transform tools: plain Q/W/E/R/T/Y, unless typing or flying with right mouse.
        let flying = flying || ctx.input(|i| i.pointer.secondary_down());
        if !typing
            && !flying
            && !self.state.shader_graph.wants_keys()
            && !self.state.effect_graph.wants_keys()
        {
            for tool in Tool::ALL {
                if ctx.input(|i| i.modifiers.is_none() && i.key_pressed(tool.key())) {
                    self.state.tool = tool;
                }
            }
            if ctx.input(|i| i.modifiers.is_none() && i.key_pressed(Key::F)) {
                self.state.push(Action::FrameSelected);
            }
        }
    }

    /// Ctrl+C / Ctrl+X / Ctrl+V on GameObjects. The OS delivers these as Copy/Cut/Paste events.
    fn clipboard_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl = Modifiers::COMMAND;
        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Copy => actions.push(Action::CopyObjects),
                    egui::Event::Cut => actions.push(Action::CutObjects),
                    egui::Event::Paste(text) => actions.push(Action::PasteObjects {
                        text: Some(text.clone()),
                        as_child: false,
                    }),
                    _ => {}
                }
            }
            for (key, action) in [
                (Key::C, Action::CopyObjects),
                (Key::X, Action::CutObjects),
                (
                    Key::V,
                    Action::PasteObjects {
                        text: None,
                        as_child: false,
                    },
                ),
            ] {
                if i.consume_shortcut(&KeyboardShortcut::new(ctrl, key)) {
                    actions.push(action);
                }
            }
        });
        for a in actions {
            self.state.push(a);
        }
    }

    /// Apply queued commands after the UI pass.
    fn apply_actions(&mut self, ctx: &egui::Context) {
        let actions = std::mem::take(&mut self.state.actions);
        for action in actions {
            // The Animation window's preview pose must not be saved or played.
            if matches!(
                action,
                Action::TogglePlay
                    | Action::Step
                    | Action::SaveScene
                    | Action::SaveSceneAs
                    | Action::NewScene
                    | Action::OpenScene
                    | Action::OpenScenePath(_)
                    | Action::OpenSceneAdditive(_)
                    | Action::Scene(..)
                    | Action::Quit
                    | Action::OpenHub { .. }
            ) {
                animation::preview::stop_preview(&mut self.state);
                crate::panels::timeline::preview::stop_preview(&mut self.state);
            }
            if let Some(action) = commands::apply(&mut self.state, action) {
                self.apply(ctx, action);
            }
            ctx.request_repaint();
        }
    }

    /// Window, layout and Play-mode commands.
    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        let s = &mut self.state;
        match action {
            Action::FocusTab(tab) => layout::focus_or_open(&mut self.dock, tab),
            Action::CloseTab(tab) => {
                if let Some(path) = self.dock.find_tab(&tab) {
                    self.dock.remove_tab(path);
                }
            }
            Action::LoadLayout(_)
            | Action::SaveLayout(_)
            | Action::DeleteLayout(_)
            | Action::ResetAllLayouts => self.apply_layout(action),
            Action::OpenHub { new_project } => {
                if s.code_editor.any_dirty() {
                    s.console
                        .warn("Save or close the changed scripts in the Code Editor first.");
                } else if let Some(a) = scene_files::guard(s, AfterPrompt::OpenHub(new_project)) {
                    scene_files::perform(s, a);
                }
            }
            Action::Quit => {
                if let Some(a) = scene_files::guard(s, AfterPrompt::Quit)
                    && scene_files::perform(s, a)
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Action::NewScene => {
                if let Some(a) = scene_files::guard(s, AfterPrompt::NewScene) {
                    scene_files::perform(s, a);
                }
            }
            Action::OpenScene => {
                if let Some(rel) = scene_files::pick_scene(s)
                    && let Some(a) = scene_files::guard(s, AfterPrompt::Open(rel))
                {
                    scene_files::perform(s, a);
                }
            }
            Action::OpenScenePath(rel) => {
                if let Some(a) = scene_files::guard(s, AfterPrompt::Open(rel)) {
                    scene_files::perform(s, a);
                }
            }
            Action::SaveScene => {
                scene_files::save(s);
            }
            Action::SaveSceneAs => {
                scene_files::save_as(s);
            }
            Action::ReloadScene => {
                if let Some(a) = scene_files::guard(s, AfterPrompt::ReloadScenes) {
                    scene_files::perform(s, a);
                }
            }
            Action::RebuildAndRestart => {
                crate::live_reload::dev_build::start_rebuild(s, Some(ctx.clone()))
            }
            Action::RestartIntoNewBuild => crate::live_reload::dev_build::request_restart(s),

            Action::Build => build_settings::build(s, false, true),
            Action::BuildAndRun => build_settings::build(s, true, false),
            Action::TogglePlay => {
                if s.play.in_play_mode() {
                    play::stop(s);
                    if let Some(dock) = self.saved_dock.take() {
                        self.dock = dock;
                    }
                } else {
                    play::start(s);
                    self.errors_seen = s.console.count(LogLevel::Error);
                    // Play Maximized: the Game view fills the window until Stop.
                    if s.game_view.play_mode_target == 1 {
                        self.saved_dock = Some(self.dock.clone());
                        self.dock = DockState::new(vec![Tab::Game]);
                    } else if s.game_view.play_mode_target == 0 {
                        layout::focus_or_open(&mut self.dock, Tab::Game);
                    }
                }
            }
            Action::TogglePause => {
                s.play = match s.play {
                    PlayState::Playing => PlayState::Paused,
                    PlayState::Paused => PlayState::Playing,
                    PlayState::Edit => {
                        s.pause_on_play = !s.pause_on_play;
                        PlayState::Edit
                    }
                };
            }
            Action::Step => {
                // Step in Edit mode enters Play mode paused.
                if !s.play.in_play_mode() {
                    s.pause_on_play = true;
                    play::start(s);
                    s.pause_on_play = false;
                }
                s.play = PlayState::Paused;
                s.step_requested = true;
            }
            Action::NotImplemented(what) => {
                s.console.warn(format!("'{what}' is not implemented yet."))
            }
            // Scene commands are handled in `commands`.
            _ => {}
        }
    }

    /// Advance the game while in Play mode; Error Pause stops on new errors.
    fn game_frame(&mut self, ctx: &egui::Context) {
        let s = &mut self.state;
        misty_engine::profile_scope!("Editor.GameFrame.Pre");
        for msg in std::mem::take(&mut s.assets.messages) {
            s.console.info(msg);
        }
        // Window ▸ General ▸ Test Runner: a slice of the running tests per frame.
        if crate::panels::test_runner::tick(s) {
            ctx.request_repaint();
        }
        // Editor scripts: (re)load, update callbacks, asset processors.
        let scripts_busy = {
            misty_engine::profile_scope!("Editor.EditorScripts");
            crate::editor_scripts::frame(s)
        };
        if scripts_busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        let mut renderer_reloaded = Vec::new();
        if let Some(gpu) = self.gpu.as_mut() {
            // Textures, models, shaders and effect graphs reload when their files change (Edit
            // mode only).
            if !s.play.in_play_mode() {
                renderer_reloaded = gpu.renderer.reload_changed_assets();
            }
            for w in gpu.renderer.take_warnings() {
                s.console.warn(w);
            }
            for e in gpu.renderer.take_shader_errors() {
                s.console.log(LogLevel::Error, e);
            }
            for w in gpu.ui.take_warnings() {
                s.console.warn(w);
            }
        }
        if !s.play.in_play_mode() {
            self.sync_cursor(ctx);
            let s = &mut self.state;
            // Spline Extrude meshes and Spline Instantiate copies follow their splines.
            misty_engine::splines::update(&mut s.scene);
            // Selected Particle Systems play in the Scene view.
            if crate::particle_preview::tick(s, ctx.input(|i| i.stable_dt)) {
                ctx.request_repaint();
            }
            // Visual Effects simulate in the Scene view while editing.
            if crate::vfx_preview::tick(s, ctx.input(|i| i.stable_dt)) {
                ctx.request_repaint();
            }
            // Scripts reload when their files change; syntax errors go to the Console.
            for err in s.assets.poll_scripts() {
                s.console.log(LogLevel::Error, err);
            }
            // Assets added, copied or moved outside the editor get meta files and show up.
            if s.assets.poll_guids() {
                s.refresh_assets();
            }
            // Open scenes and editor-held assets follow their files (see `live_reload`).
            if crate::live_reload::frame(s, renderer_reloaded) {
                ctx.request_repaint();
            }
            // Prefab Mode's Auto Save.
            crate::prefab_mode::update(s, ctx);
            return;
        }
        // Clicking outside the Game view gives keyboard/mouse back to the editor (not while
        // the game holds the cursor: it sits at the window centre then).
        let locked = self.cursor_applied.lock == misty_engine::scripting::CursorLockMode::Locked;
        if !locked
            && let Some(p) =
                ctx.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()))
        {
            s.game_view.focused = s.game_view.view_rect.contains(p);
        }
        s.game_input = if s.game_view.focused && !ctx.egui_wants_keyboard_input() {
            game_input::capture(ctx, s.game_view.view_rect, locked)
        } else {
            Default::default()
        };
        s.game_input.joysticks = game_input::gamepads();
        // Escape always gives the cursor back, whatever the scripts do.
        if locked
            && s.game_input.key_down("escape")
            && let Some(r) = s.runtime.as_ref()
        {
            r.release_cursor();
        }
        {
            misty_engine::profile_scope!("Play.Tick");
            play::tick(s, ctx.input(|i| i.stable_dt));
        }
        // Gamepad rumble scripts asked for.
        if let Some(motors) = s.runtime.as_ref().and_then(|r| r.take_rumble()) {
            game_input::set_rumble(motors);
        }
        self.sync_cursor(ctx);
        let s = &mut self.state;
        let errors = s.console.count(LogLevel::Error);
        if s.console.error_pause && errors > self.errors_seen && s.play == PlayState::Playing {
            s.play = PlayState::Paused;
        }
        self.errors_seen = errors;
        if s.play == PlayState::Playing {
            ctx.request_repaint();
        }
    }

    /// `Cursor.lockState` / `Cursor.visible` of the running game reach the window while the
    /// Game view has the input; Stop, pause or a lost focus give the cursor back.
    fn sync_cursor(&mut self, ctx: &egui::Context) {
        use misty_engine::scripting::{CursorLockMode, CursorState};
        let s = &self.state;
        let wanted = match s.runtime.as_ref() {
            Some(r) if s.play == PlayState::Playing && s.game_view.focused => r.cursor(),
            _ => CursorState::default(),
        };
        if wanted == self.cursor_applied {
            return;
        }
        self.cursor_applied = wanted;
        let grab = match wanted.lock {
            CursorLockMode::None => egui::viewport::CursorGrab::None,
            CursorLockMode::Locked => egui::viewport::CursorGrab::Locked,
            CursorLockMode::Confined => egui::viewport::CursorGrab::Confined,
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(grab));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(wanted.visible));
    }

    /// Save prompt, window-close interception and the window title.
    fn scene_file_frame(&mut self, ctx: &egui::Context) {
        let s = &mut self.state;
        // Closing the window with unsaved changes asks first (scripts, then the scene).
        let closing = ctx.input(|i| i.viewport().close_requested());
        if closing && s.code_editor.cancel_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else if closing && s.shader_graph.cancel_close() {
            // The Shader Graph window asks about its unsaved graph first.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else if closing && !s.files.quit_confirmed && crate::prefab_mode::active(s) {
            // Leave Prefab Mode first (Auto Save saves, otherwise its bar asks), then close.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if crate::prefab_mode::leave_for_scene_change(s) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if closing && !s.files.quit_confirmed {
            if s.play.in_play_mode() {
                play::stop(s);
            }
            if !scene_files::dirty_now(s).is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                s.files.prompt = Some(AfterPrompt::Quit);
            }
        }
        if let Some(action) = layouts::save_prompt_ui(ctx, s) {
            s.push(action);
            ctx.request_repaint();
        }
        if let Some(action) = scene_files::prompt_ui(ctx, s)
            && scene_files::perform(s, action)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        scene_files::refresh_dirty(s);
        let title = scene_files::window_title(s);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// The frameless window's title bar: menus, "Misty Engine — project — scene", window buttons.
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let style = title_bar::BarStyle {
            bg: theme::TAB_BAR_BG,
            text: theme::TEXT,
            dim: theme::TEXT_DIM,
            hover: egui::Color32::from_white_alpha(18),
        };
        let (project, scene) = scene_files::title_parts(&self.state);
        // Empty parts are left out, so there is never a "— —" gap.
        let rest: String = [project, scene]
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| format!("  —  {p}"))
            .collect();
        let title = title_bar::title_job(crate::branding::PRODUCT, &rest, &style);
        title_bar::show(ui, &style, title, |ui| {
            menu_bar::show_menus(ui, &mut self.state)
        });
    }

    /// Restart into the new build: write the restore file, start the new exe with
    /// `--restore`, then close (scenes were saved or discarded at the prompt before).
    fn restart(&mut self, ctx: &egui::Context) {
        let dock = self.saved_dock.as_ref().unwrap_or(&self.dock);
        let file = match crate::live_reload::restore::write(&self.state, dock) {
            Ok(f) => f,
            Err(e) => {
                self.state.console.log(
                    LogLevel::Error,
                    format!("Restart: could not write the restore file: {e}"),
                );
                return;
            }
        };
        // What on_exit would save, before the new process reads it.
        self.save_layout(true);
        scene_files::remember_scene_view(&self.state);
        match self.state.live.dev.launch(&file) {
            Ok((exe, note)) => {
                if let Some(n) = note {
                    self.state.console.info(n);
                }
                self.state
                    .console
                    .info(format!("Restarting into {}…", exe.display()));
                self.state.files.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&file);
                self.state
                    .console
                    .log(LogLevel::Error, format!("Restart failed: {e}"));
            }
        }
    }

    /// The user is mid-interaction: a mouse button is down or a text field has focus.
    fn interacting(ctx: &egui::Context) -> bool {
        ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()
    }
}

impl eframe::App for EditorApp {
    fn on_exit(&mut self) {
        crate::mcp::live::shutdown(&mut self.state);
        self.save_layout(true);
        crate::editor_scripts::on_exit(&mut self.state);
        self.save_hub_thumbnail();
        // Screenshot runs leave the saved view alone so they stay repeatable.
        if self.capture.is_none() {
            scene_files::remember_scene_view(&self.state);
        }
        if let Err(e) = misty_engine::profiler::capture::finish() {
            eprintln!("profiler capture: {e}");
        }
        if let Err(e) = misty_engine::render::save_pipeline_cache() {
            eprintln!("pipeline cache not saved: {e}");
        }
    }

    /// Only the window position/size is persisted by eframe; editor state lives in the project.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::scene_camera::bench::frame_start(&ctx);
        apply_vsync(&mut self.state, frame);
        self.state.ui_scale.apply(&ctx);
        let s = &mut self.state;
        let (clips, clip) = (&mut s.anim_clips, &s.animation.clip);
        s.undo
            .begin_frame(Self::interacting(&ctx), &s.scene, &s.selection, || {
                clips.snapshot(clip.as_deref()?)
            });
        self.shortcuts(&ctx);
        {
            misty_engine::profile_scope!(
                "Editor.GameFrame",
                misty_engine::profiler::Category::Others
            );
            self.game_frame(&ctx);
        }
        animation::preview::pre_frame(&mut self.state, ctx.input(|i| i.stable_dt));
        crate::panels::timeline::preview::pre_frame(&mut self.state, ctx.input(|i| i.stable_dt));

        let bar_frame = egui::Frame::new()
            .fill(theme::TAB_BAR_BG)
            .inner_margin(egui::Margin::symmetric(4, 2));
        egui::Panel::top("menu_bar")
            .exact_size(title_bar::HEIGHT)
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| self.title_bar(ui));
        egui::Panel::top("main_toolbar")
            .frame(bar_frame.inner_margin(egui::Margin::symmetric(6, 4)))
            .show(ui, |ui| toolbar::show(ui, &mut self.state));
        egui::Panel::bottom("status_bar")
            .frame(bar_frame)
            .show(ui, |ui| status_bar::show(ui, &mut self.state));
        // "X.scene changed on disk — Reload / Keep mine" (non-modal, under the toolbar).
        if !self.state.live.scenes.conflicts.is_empty() {
            egui::Panel::top("scene_changed_bar")
                .frame(
                    egui::Frame::new()
                        .fill(crate::live_reload::scenes::BAR_FILL)
                        .inner_margin(egui::Margin::symmetric(10, 4)),
                )
                .show(ui, |ui| {
                    crate::live_reload::scenes::bar(ui, &mut self.state)
                });
        }
        // Before the panels, so arrow keys and Enter reach the palette first.
        crate::search::palette::show(&ctx, &mut self.state);

        let mut style = theme::dock_style(&ctx);
        if self.state.play.in_play_mode() {
            // Play-mode tint, as a reminder that changes will be reverted.
            style.tab_bar.bg_fill = theme::PLAY_TINT.gamma_multiply(0.45);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BORDER))
            .show(ui, |ui| {
                misty_engine::profile_scope!("Editor.Panels", misty_engine::profiler::Category::Ui);
                DockArea::new(&mut self.dock)
                    .style(style)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(
                        ui,
                        &mut PanelViewer {
                            state: &mut self.state,
                            gpu: self.gpu.as_mut(),
                        },
                    );
            });

        // Shader variants compile the first time a view needs them (a new scene, a material
        // or effect first seen): say so, since that frame takes longer.
        let shaders = misty_engine::render::pipelines_built();
        if shaders > self.shaders_seen {
            let n = shaders - self.shaders_seen;
            self.shaders_seen = shaders;
            self.state
                .live
                .toast(format!("Preparing shaders… ({n} compiled)"));
            self.pipeline_cache_due =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
        }
        if self
            .pipeline_cache_due
            .is_some_and(|t| std::time::Instant::now() >= t)
        {
            self.pipeline_cache_due = None;
            let _ = std::thread::Builder::new()
                .name("misty pipeline cache".into())
                .spawn(|| {
                    if let Err(e) = misty_engine::render::save_pipeline_cache() {
                        eprintln!("pipeline cache not saved: {e}");
                    }
                });
        }
        // Lightmap bakes run on their own thread: pick up the result when it is done.
        if crate::lightmap_tools::poll(&mut self.state) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Material previews asked for while drawing the panels; probe bakes and Realtime probes.
        if let Some(gpu) = self.gpu.as_mut() {
            if crate::probe_tools::gpu_frame(gpu, &mut self.state) {
                ctx.request_repaint();
            }
            if crate::light_probe_tools::gpu_frame(gpu, &mut self.state) {
                ctx.request_repaint();
            }
            let previews = &mut self.state.project.thumbnails.materials;
            crate::material_preview::render(gpu, previews);
            if previews.pending() {
                ctx.request_repaint();
            }
        }
        crate::project_settings::show(&ctx, &mut self.state);
        build_settings::show(&ctx, &mut self.state);
        crate::snap_window::show(&ctx, &mut self.state);
        crate::vcs::window::show(&ctx, &mut self.state);
        let root = self.state.project.root.clone();
        if self.state.vcs.frame(&root) {
            ctx.request_repaint();
        } else if self.state.vcs.enabled {
            // Wake up for the next status poll while the editor sits idle.
            ctx.request_repaint_after(std::time::Duration::from_secs(3));
        }
        crate::vgeo_convert::show(&ctx, &mut self.state);
        crate::saveloc::show(&ctx, &mut self.state);
        crate::multiplayer::show(&ctx, &mut self.state);
        crate::mesh_builder::shape_window(&ctx, &mut self.state);
        crate::ragdoll_wizard::window(&ctx, &mut self.state);
        crate::panels::preset_window(&ctx, &mut self.state);
        crate::branding::about::show(&ctx, &mut self.state.about_open);
        crate::panels::code_editor::show_windows(&ctx, &mut self.state);
        crate::panels::shader_graph::show_windows(&ctx, &mut self.state);
        crate::editor_scripts::progress_ui(&ctx, &self.state);
        // Last, so the tutorial card and highlight sit on top of everything drawn above.
        crate::tutorials::frame(&ctx, &mut self.state);
        if self.capture.as_mut().is_some_and(Capture::play_due) {
            self.state.push(Action::TogglePlay);
        }
        if let Some(rel) = self.capture.as_mut().and_then(Capture::scene_due) {
            self.state.push(Action::OpenScenePath(rel));
        }
        // AI tools (MCP): requests from the loopback bridge, before queued actions apply.
        crate::mcp::live::frame(&ctx, &mut self.state, self.gpu.as_mut());
        self.apply_actions(&ctx);
        crate::saveloc::frame(&mut self.state);
        // SaveSystem.Save asked for a thumbnail of the game's last frame.
        if let Some(path) = misty_engine::save::take_thumbnail_request()
            && let Some((w, h, rgba)) = self.gpu.as_ref().and_then(|g| g.game_view_pixels())
            && let Err(e) = misty_engine::save::write_thumbnail(&path, w, h, rgba)
        {
            self.state.console.warn(format!("Save thumbnail: {e}"));
        }
        if let Some(text) = self.state.os_clipboard_out.take() {
            ctx.copy_text(text);
        }
        animation::preview::post_frame(&mut self.state, Self::interacting(&ctx));
        crate::terrain_tools::end_frame(&mut self.state, Self::interacting(&ctx));
        // A nav mesh bake finishing while its window is hidden.
        crate::panels::navigation::poll(&mut self.state);
        self.state.undo.end_frame(Self::interacting(&ctx));
        crate::updater::editor::frame(&ctx, &mut self.state);
        // Dev builds: a newer exe in target/, a rebuild finishing.
        if crate::live_reload::dev_build::frame(&mut self.state) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.scene_file_frame(&ctx);
        if std::mem::take(&mut self.state.live.restart_now) {
            self.restart(&ctx);
        }
        self.save_layout(false);
        if let Some(capture) = &mut self.capture {
            if let Some(gpu) = &self.gpu {
                let stats = gpu.renderer.stats();
                capture.note_loading(stats.loading > 0 || stats.grass_pending > 0);
            }
            capture.hold(crate::panels::timeline::capture_waiting(&self.state));
            capture.update(&ctx);
            // Screenshot runs close without a save prompt (and never save the scene).
            if capture.saved() {
                self.state.files.quit_confirmed = true;
            }
        }
        crate::scene_camera::bench::frame_end();
        // One profiler frame per editor frame; a Profiler out of sight stops recording.
        crate::panels::profiler::sync(&mut self.state, &ctx);
        misty_engine::profiler::end_frame();
        if misty_engine::profiler::enabled() {
            ctx.request_repaint();
        }
    }
}

/// The Game view's VSync toggle (saved in `hub.json`, read once): the window's present mode
/// follows it, switched on the next paint.
fn apply_vsync(s: &mut crate::state::EditorState, frame: &mut eframe::Frame) {
    let vsync = *s
        .game_view
        .vsync
        .get_or_insert_with(crate::hub::config::editor_vsync);
    let mode = match vsync {
        true => egui_wgpu::wgpu::PresentMode::AutoVsync,
        false => egui_wgpu::wgpu::PresentMode::AutoNoVsync,
    };
    if let Some(mut c) = frame.wgpu_surface_config()
        && c.present_mode != mode
    {
        c.present_mode = mode;
        frame.set_wgpu_surface_config(c);
    }
}
