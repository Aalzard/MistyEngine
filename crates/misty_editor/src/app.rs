//! Editor application: frame layout, shortcuts and action handling.

use std::path::PathBuf;

use egui::{Key, KeyboardShortcut, Modifiers};
use egui_dock::{DockArea, DockState};

use crate::actions::{Action, Create};
use crate::capture::Capture;
use crate::console::LogLevel;
use crate::layout::{self, Tab};
use crate::panels::PanelViewer;
use crate::project::ProjectBrowser;
use crate::state::{EditorState, PlayState, Tool};
use crate::viewport::Gpu;
use crate::{commands, menu_bar, play, status_bar, theme, toolbar};

pub struct EditorApp {
    state: EditorState,
    dock: DockState<Tab>,
    gpu: Option<Gpu>,
    capture: Option<Capture>,
    /// Layout to restore after "Play Maximized".
    saved_dock: Option<DockState<Tab>>,
    /// Error count seen so far (for Error Pause).
    errors_seen: usize,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, project: PathBuf) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut state = EditorState::new(ProjectBrowser::open(project));
        let gpu = cc.wgpu_render_state.clone().map(Gpu::new);
        match &gpu {
            Some(g) => {
                let info = g.state.adapter.get_info();
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
            dock: layout::default_layout(),
            gpu,
            capture: Capture::from_env(),
            saved_dock: None,
            errors_seen: 0,
        }
    }

    /// Global keyboard shortcuts (skipped while typing in a text field).
    fn shortcuts(&mut self, ctx: &egui::Context) {
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
            (ctrl_shift, Key::N, create_empty),
            (ctrl_shift, Key::G, Action::CreateEmptyParent),
            (ctrl, Key::D, Action::DuplicateSelected),
            (ctrl, Key::A, Action::SelectAll),
            (ctrl, Key::Z, Action::Undo),
            (ctrl, Key::Y, Action::Redo),
            (ctrl_shift, Key::Z, Action::Redo),
            (ctrl_alt, Key::F, Action::MoveToView),
            (ctrl_shift, Key::F, Action::AlignWithView),
            (alt_shift, Key::A, Action::ToggleActive),
            (ctrl, Key::Num1, Action::FocusTab(Tab::Scene)),
            (ctrl, Key::Num2, Action::FocusTab(Tab::Game)),
            (ctrl, Key::Num3, Action::FocusTab(Tab::Inspector)),
            (ctrl, Key::Num4, Action::FocusTab(Tab::Hierarchy)),
            (ctrl, Key::Num5, Action::FocusTab(Tab::Project)),
            (ctrl_shift, Key::C, Action::FocusTab(Tab::Console)),
            (Modifiers::SHIFT, Key::D, Action::DeselectAll),
            (Modifiers::NONE, Key::Delete, Action::DeleteSelected),
            (Modifiers::NONE, Key::F2, Action::RenameSelected),
        ];
        // Most specific modifiers first so Ctrl+Shift+P is not eaten by Ctrl+P.
        let mut sorted: Vec<_> = bindings.into_iter().collect();
        sorted.sort_by_key(|(m, _, _)| std::cmp::Reverse(m.shift as u8 + m.alt as u8));
        let typing = ctx.egui_wants_keyboard_input();
        for (mods, key, action) in sorted {
            // While typing, text fields own plain keys, Shift+keys and their own undo.
            let text_key = mods == Modifiers::NONE
                || mods == Modifiers::SHIFT
                || matches!(key, Key::Z | Key::Y | Key::A);
            if typing && text_key {
                continue;
            }
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key))) {
                self.state.push(action);
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl, Key::R))) {
            self.state.project.refresh();
        }
        // Transform tools: plain Q/W/E/R/T/Y, unless typing or flying with right mouse.
        let flying = ctx.input(|i| i.pointer.secondary_down());
        if !typing && !flying {
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

    /// Apply queued commands after the UI pass.
    fn apply_actions(&mut self, ctx: &egui::Context) {
        let actions = std::mem::take(&mut self.state.actions);
        for action in actions {
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
            Action::ResetLayout => self.dock = layout::default_layout(),
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
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
        if !s.play.in_play_mode() {
            return;
        }
        play::tick(s, ctx.input(|i| i.stable_dt));
        let errors = s.console.count(LogLevel::Error);
        if s.console.error_pause && errors > self.errors_seen && s.play == PlayState::Playing {
            s.play = PlayState::Paused;
        }
        self.errors_seen = errors;
        if s.play == PlayState::Playing {
            ctx.request_repaint();
        }
    }

    /// The user is mid-interaction: a mouse button is down or a text field has focus.
    fn interacting(ctx: &egui::Context) -> bool {
        ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let s = &mut self.state;
        s.undo
            .begin_frame(Self::interacting(&ctx), &s.scene, &s.selection);
        self.shortcuts(&ctx);
        self.game_frame(&ctx);

        let bar_frame = egui::Frame::new()
            .fill(theme::TAB_BAR_BG)
            .inner_margin(egui::Margin::symmetric(4, 2));
        egui::Panel::top("menu_bar")
            .frame(bar_frame)
            .show(ui, |ui| menu_bar::show(ui, &mut self.state));
        egui::Panel::top("main_toolbar")
            .frame(bar_frame.inner_margin(egui::Margin::symmetric(6, 4)))
            .show(ui, |ui| toolbar::show(ui, &mut self.state));
        egui::Panel::bottom("status_bar")
            .frame(bar_frame)
            .show(ui, |ui| status_bar::show(ui, &mut self.state));

        let mut style = theme::dock_style(&ctx);
        if self.state.play.in_play_mode() {
            // Play-mode tint, as a reminder that changes will be reverted.
            style.tab_bar.bg_fill = theme::PLAY_TINT.gamma_multiply(0.45);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BORDER))
            .show(ui, |ui| {
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

        self.apply_actions(&ctx);
        self.state.undo.end_frame(Self::interacting(&ctx));
        if let Some(capture) = &mut self.capture {
            capture.update(&ctx);
        }
    }
}
