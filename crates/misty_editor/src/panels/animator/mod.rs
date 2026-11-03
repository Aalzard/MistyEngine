//! Animator window (Window ▸ Animation ▸ Animator): the Parameters list and the base layer's
//! state machine graph of the selected object's Animator Controller (or a selected
//! `.controller` asset). In Play mode it shows the live state, progress and parameter values.

mod graph;
mod params;

#[cfg(test)]
mod tests;

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use misty_engine::animation::animator::AnimatorStatus;
use misty_engine::animation::controller::Node;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Animator, clip_name};

use crate::anim_controllers::{self, interacting};
use crate::state::EditorState;
use crate::theme;

/// Icon of Animator Controllers (Project window, Inspector, tab title).
pub const CONTROLLER_ICON: &str = "⇄";
/// Width of the Parameters panel on the left.
const PARAMS_W: f32 = 230.0;

/// Something selected in the graph (shown in the Inspector).
#[derive(Clone, Debug, PartialEq)]
pub enum GraphItem {
    Node(Node),
    /// Index into the controller's transitions.
    Transition(usize),
}

/// Animator window state (kept while the tab is closed).
#[derive(Default)]
pub struct AnimatorWindow {
    /// Controller shown (project-relative path).
    pub controller: Option<String>,
    pub selected: Option<GraphItem>,
    /// Object whose Animator plays the controller (live view in Play mode).
    pub target: Option<Entity>,
    /// Graph scroll (graph units) and zoom (0 = 1).
    pub pan: Vec2,
    pub zoom: f32,
    /// Make Transition in progress: the arrow starts at this node.
    pub making: Option<Node>,
    /// Node being dragged.
    drag: Option<Node>,
    /// What the context menu was opened on and where (graph coordinates).
    menu: Option<(Option<GraphItem>, egui::Pos2)>,
    /// Parameter being renamed and the text typed so far.
    pub renaming: Option<(String, String)>,
    /// Last click landed in this window (Delete removes the selection).
    pub focused: bool,
    /// Selected object last frame (the window follows selection changes).
    followed: Option<Entity>,
}

impl AnimatorWindow {
    /// Show a controller (a new one clears the graph selection).
    pub fn open(&mut self, path: Option<String>) {
        if self.controller != path {
            self.controller = path;
            self.selected = None;
            self.making = None;
            self.renaming = None;
        }
    }

    pub fn zoom(&self) -> f32 {
        if self.zoom > 0.0 { self.zoom } else { 1.0 }
    }

    /// Delete should remove the selected state or transition rather than scene objects.
    pub fn wants_delete(&self) -> bool {
        self.focused && self.selected.is_some()
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        s.animator.focused = full.contains(p);
    }
    follow_selection(s);
    let Some(path) = s.animator.controller.clone() else {
        centered(
            ui,
            full,
            "Select a GameObject with an Animator, or an Animator Controller asset.",
        );
        return;
    };
    let ctrl = match s.anim_clips.controllers.get(&path) {
        Ok(c) => c.clone(),
        Err(e) => {
            centered(ui, full, &e);
            return;
        }
    };
    s.anim_clips
        .controllers
        .end_interaction(interacting(ui.ctx()));
    let live = live_status(s, &path);

    let left = Rect::from_min_size(
        full.min,
        vec2(PARAMS_W.min(full.width() * 0.4), full.height()),
    );
    let right = Rect::from_min_max(pos2(left.right() + 1.0, full.top()), full.max);
    ui.painter().rect_filled(left, 0.0, theme::PANEL_BG);
    ui.painter().vline(
        left.right(),
        full.y_range(),
        egui::Stroke::new(1.0, theme::BORDER),
    );
    ui.scope_builder(UiBuilder::new().max_rect(left.shrink(4.0)), |ui| {
        params::show(ui, s, &path, &ctrl, live.as_ref());
    });
    let bar = Rect::from_min_size(right.min, vec2(right.width(), 22.0));
    toolbar(ui, s, bar, &path);
    let canvas = Rect::from_min_max(pos2(right.left(), bar.bottom()), right.max);
    graph::show(ui, s, canvas, &path, &ctrl, live.as_ref());

    // Delete removes the selected state or transition.
    if s.animator.focused
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete))
        && let Some(item) = s.animator.selected.clone()
    {
        delete(s, &path, &item);
    }
}

/// Breadcrumb ("Base Layer") and the controller's name.
fn toolbar(ui: &mut Ui, s: &EditorState, bar: Rect, path: &str) {
    ui.painter().rect_filled(bar, 0.0, theme::TAB_BAR_BG);
    ui.scope_builder(
        UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Base Layer").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{CONTROLLER_ICON} {}", clip_name(path)))
                            .color(theme::TEXT_DIM),
                    );
                    if s.play.in_play_mode() && s.animator.target.is_some() {
                        ui.label(RichText::new("● Live").color(graph::LIVE_BLUE));
                    }
                });
            });
        },
    );
}

/// The selected object (or its nearest parent) with an Animator decides the controller.
fn follow_selection(s: &mut EditorState) {
    let active = s.selection.active();
    if active == s.animator.followed {
        return;
    }
    s.animator.followed = active;
    let mut cur = active;
    while let Some(e) = cur {
        let ctrl = s
            .scene
            .world
            .get::<&Animator>(e)
            .ok()
            .map(|a| a.controller.clone());
        if let Some(ctrl) = ctrl {
            s.animator.target = Some(e);
            if ctrl.is_some() {
                s.animator.open(ctrl);
            }
            return;
        }
        cur = s.scene.parent(e);
    }
}

/// Live state of the window's target (or the first object playing this controller).
fn live_status(s: &mut EditorState, path: &str) -> Option<AnimatorStatus> {
    let rt = s.runtime.as_ref()?;
    let uses = |e: Entity| {
        s.scene
            .world
            .get::<&Animator>(e)
            .is_ok_and(|a| a.controller.as_deref() == Some(path))
    };
    let target = s
        .animator
        .target
        .filter(|e| s.scene.contains(*e) && uses(*e))
        .or_else(|| {
            s.scene
                .walk()
                .into_iter()
                .map(|(e, _)| e)
                .find(|e| uses(*e))
        })?;
    s.animator.target = Some(target);
    rt.animator_status(&s.scene, target)
}

/// Select something in the graph; the Inspector shows it.
pub(super) fn select(s: &mut EditorState, path: &str, item: Option<GraphItem>) {
    s.animator.selected = item;
    if s.animator.selected.is_some() {
        s.selection.clear();
        s.animator.followed = None;
        s.inspector_asset = Some(path.to_owned());
    }
}

/// Delete a state (with its transitions) or one transition.
pub(super) fn delete(s: &mut EditorState, path: &str, item: &GraphItem) {
    match item {
        GraphItem::Node(Node::State(name)) => {
            let name = name.clone();
            anim_controllers::edit(s, path, "Delete State", false, |c| c.remove_state(&name));
        }
        GraphItem::Transition(i) => {
            let i = *i;
            anim_controllers::edit(s, path, "Delete Transition", false, |c| {
                if i < c.transitions.len() {
                    c.transitions.remove(i);
                }
            });
        }
        // Entry, Any State and Exit always exist.
        GraphItem::Node(_) => return,
    }
    s.animator.selected = None;
}

fn centered(ui: &mut Ui, rect: Rect, text: &str) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        theme::TEXT_DIM,
    );
}
