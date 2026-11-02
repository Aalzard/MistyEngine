//! Editor application: frame layout, shortcuts and action handling.

use std::path::PathBuf;

use egui::{Key, KeyboardShortcut, Modifiers};
use egui_dock::{DockArea, DockState};

use crate::actions::Action;
use crate::capture::Capture;
use crate::layout::{self, Tab};
use crate::panels::PanelViewer;
use crate::project::ProjectBrowser;
use crate::state::{self, EditorState, ObjectKind, PlaceholderObject, PlayState, Tool};
use crate::viewport::Gpu;
use crate::{menu_bar, status_bar, theme, toolbar};

pub struct EditorApp {
    state: EditorState,
    dock: DockState<Tab>,
    gpu: Option<Gpu>,
    capture: Option<Capture>,
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
        }
    }

    /// Global keyboard shortcuts (skipped while typing in a text field).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let ctrl_alt = Modifiers::COMMAND | Modifiers::ALT;
        let bindings: [(Modifiers, Key, Action); 14] = [
            (ctrl, Key::P, Action::TogglePlay),
            (ctrl_shift, Key::P, Action::TogglePause),
            (ctrl_alt, Key::P, Action::Step),
            (ctrl, Key::N, Action::NewScene),
            (ctrl_shift, Key::N, Action::CreateEmpty),
            (ctrl, Key::D, Action::DuplicateSelected),
            (ctrl, Key::Num1, Action::FocusTab(Tab::Scene)),
            (ctrl, Key::Num2, Action::FocusTab(Tab::Game)),
            (ctrl, Key::Num3, Action::FocusTab(Tab::Inspector)),
            (ctrl, Key::Num4, Action::FocusTab(Tab::Hierarchy)),
            (ctrl, Key::Num5, Action::FocusTab(Tab::Project)),
            (ctrl_shift, Key::C, Action::FocusTab(Tab::Console)),
            (Modifiers::SHIFT, Key::D, Action::DeselectAll),
            (Modifiers::NONE, Key::Delete, Action::DeleteSelected),
        ];
        // Most specific modifiers first so Ctrl+Shift+P is not eaten by Ctrl+P.
        let mut sorted: Vec<_> = bindings.into_iter().collect();
        sorted.sort_by_key(|(m, _, _)| std::cmp::Reverse(m.shift as u8 + m.alt as u8));
        let typing = ctx.egui_wants_keyboard_input();
        for (mods, key, action) in sorted {
            if typing && mods == Modifiers::NONE {
                continue;
            }
            if typing && mods == Modifiers::SHIFT {
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
            self.apply(ctx, action);
        }
    }

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        let s = &mut self.state;
        match action {
            Action::FocusTab(tab) => layout::focus_or_open(&mut self.dock, tab),
            Action::ResetLayout => self.dock = layout::default_layout(),
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::NewScene => {
                s.objects = state::default_scene();
                s.selected = None;
                s.scene_name = "Untitled".into();
                s.scene_dirty = false;
                s.console.info("Created new scene.");
            }
            // New objects appear at the Scene view pivot, like "create in front of the view".
            Action::CreateEmpty => {
                let p = spawn_point(s);
                add_object(
                    s,
                    PlaceholderObject::new("GameObject", ObjectKind::Empty, p, [0.0; 3]),
                )
            }
            Action::CreatePrimitive(prim) => {
                let p = spawn_point(s);
                add_object(
                    s,
                    PlaceholderObject::new(prim.name(), ObjectKind::Primitive(prim), p, [0.0; 3]),
                )
            }
            Action::CreateLight(l) => {
                let p = spawn_point(s);
                let rot = if l == crate::actions::LightKind::Point {
                    [0.0; 3]
                } else {
                    [50.0, -30.0, 0.0]
                };
                add_object(
                    s,
                    PlaceholderObject::new(l.name(), ObjectKind::Light(l), p, rot),
                )
            }
            Action::CreateCamera => {
                let p = spawn_point(s);
                add_object(
                    s,
                    PlaceholderObject::new("Camera", ObjectKind::Camera, p, [0.0; 3]),
                )
            }
            Action::DeleteSelected => {
                if let Some(i) = s.selected.take() {
                    let obj = s.objects.remove(i);
                    s.scene_dirty = true;
                    s.console.info(format!("Deleted '{}'.", obj.name));
                }
            }
            Action::DuplicateSelected => {
                if let Some(i) = s.selected {
                    let mut copy = s.objects[i].clone();
                    copy.name = next_free_name(&s.objects, &copy.name);
                    s.objects.insert(i + 1, copy);
                    s.selected = Some(i + 1);
                    s.scene_dirty = true;
                }
            }
            Action::DeselectAll => s.selected = None,
            Action::FrameSelected => {
                if let Some(o) = s.selected.and_then(|i| s.objects.get(i)) {
                    let radius = o.scale.iter().fold(0.5_f32, |m, v| m.max(v.abs() * 0.5));
                    s.scene_view
                        .camera
                        .frame(glam::Vec3::from(o.position), radius);
                    ctx.request_repaint();
                }
            }
            Action::TogglePlay => {
                s.play = if s.play.in_play_mode() {
                    s.console.info("Exited Play mode.");
                    PlayState::Edit
                } else {
                    if s.console.clear_on_play {
                        s.console.clear();
                    }
                    s.console.info("Entered Play mode.");
                    layout::focus_or_open(&mut self.dock, Tab::Game);
                    PlayState::Playing
                };
            }
            Action::TogglePause => {
                s.play = match s.play {
                    PlayState::Playing => PlayState::Paused,
                    PlayState::Paused => PlayState::Playing,
                    PlayState::Edit => PlayState::Edit,
                };
            }
            Action::Step => {}
            Action::NotImplemented(what) => {
                s.console.warn(format!("'{what}' is not implemented yet."))
            }
        }
    }
}

/// Where new objects are placed: the Scene camera pivot, snapped to whole units.
fn spawn_point(s: &EditorState) -> [f32; 3] {
    s.scene_view.camera.pivot().round().to_array()
}

fn add_object(s: &mut EditorState, mut obj: PlaceholderObject) {
    obj.name = next_free_name(&s.objects, &obj.name);
    s.console.info(format!("Created '{}'.", obj.name));
    s.objects.push(obj);
    s.selected = Some(s.objects.len() - 1);
    s.scene_dirty = true;
}

/// "Cube", "Cube (1)", "Cube (2)", ...
fn next_free_name(objects: &[PlaceholderObject], base: &str) -> String {
    let base = match base.rfind(" (") {
        Some(i) if base.ends_with(')') => &base[..i],
        _ => base,
    };
    if !objects.iter().any(|o| o.name == base) {
        return base.to_owned();
    }
    (1..)
        .map(|n| format!("{base} ({n})"))
        .find(|name| !objects.iter().any(|o| &o.name == name))
        .unwrap_or_default()
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);

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
        if let Some(capture) = &mut self.capture {
            capture.update(&ctx);
        }
    }
}
