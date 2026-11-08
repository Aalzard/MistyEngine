//! Animator window (Window ▸ Animation ▸ Animator): the Layers and Parameters lists and the
//! state machine graph of one layer (and sub-state machine) of the selected object's Animator
//! Controller (or a selected `.controller` asset), with a breadcrumb to go back up; a blend
//! tree state opens a blend tree view. In Play mode it shows the live states, progress, layer
//! weights and parameter values.

pub mod blend_view;
mod graph;
mod layers;
mod params;

#[cfg(test)]
mod tests;

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use misty_engine::animation::AnimatorController;
use misty_engine::animation::animator::AnimatorStatus;
use misty_engine::animation::controller::{Node, machine_parent};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Animator, clip_name};

use crate::anim_controllers::interacting;
use crate::state::EditorState;
use crate::theme;

/// Icon of Animator Controllers (Project window, Inspector, tab title).
pub const CONTROLLER_ICON: &str = "⇄";
/// Icon of Avatar Masks.
pub const MASK_ICON: &str = "◐";
/// Icon of humanoid Avatars.
pub const AVATAR_ICON: &str = "☺";
/// Width of the Parameters panel on the left.
const PARAMS_W: f32 = 230.0;

/// Something selected in the graph (shown in the Inspector).
#[derive(Clone, Debug, PartialEq)]
pub enum GraphItem {
    Node(Node),
    /// Index into the controller's transitions.
    Transition(usize),
}

/// Which list the left side shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeftTab {
    Layers,
    #[default]
    Parameters,
}

/// Animator window state (kept while the tab is closed).
#[derive(Default)]
pub struct AnimatorWindow {
    /// Controller shown (project-relative path).
    pub controller: Option<String>,
    /// Layer whose state machine is shown.
    pub layer: usize,
    /// Sub-state machine shown ("" = the layer's top machine).
    pub machine: String,
    /// Blend tree state shown in the blend tree view instead of the graph.
    pub blend_view: Option<String>,
    /// Motion highlighted in blend tree diagrams.
    pub blend_selected: Option<usize>,
    pub left_tab: LeftTab,
    /// Selection in the shown layer.
    pub selected: Option<GraphItem>,
    /// Object whose Animator plays the controller (live view in Play mode).
    pub target: Option<Entity>,
    /// Graph scroll (graph units) and zoom (0 = 1).
    pub pan: Vec2,
    pub zoom: f32,
    /// Make Transition in progress: the arrow starts at this node.
    pub making: Option<Node>,
    /// Make Transition ended on "(Up)": pick the destination at this screen position.
    up_menu: Option<(Node, egui::Pos2)>,
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
            self.show_layer(0);
            self.renaming = None;
        }
    }

    /// Show another layer's top machine.
    pub fn show_layer(&mut self, layer: usize) {
        self.layer = layer;
        self.enter_machine(String::new());
    }

    /// Show a sub-state machine of the shown layer ("" = its top machine).
    pub fn enter_machine(&mut self, path: String) {
        self.machine = path;
        self.selected = None;
        self.making = None;
        self.up_menu = None;
        self.blend_view = None;
        self.blend_selected = None;
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
    if s.animator.layer >= ctrl.layer_count() {
        s.animator.show_layer(0);
    }
    let live_layer = live.as_ref().and_then(|l| l.layers.get(s.animator.layer));

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
        ui.horizontal(|ui| {
            for (tab, label) in [
                (LeftTab::Layers, "Layers"),
                (LeftTab::Parameters, "Parameters"),
            ] {
                ui.selectable_value(&mut s.animator.left_tab, tab, label);
            }
        });
        ui.separator();
        match s.animator.left_tab {
            LeftTab::Layers => layers::show(ui, s, &path, &ctrl, live.as_ref()),
            LeftTab::Parameters => params::show(ui, s, &path, &ctrl, live.as_ref()),
        }
    });
    let bar = Rect::from_min_size(right.min, vec2(right.width(), 22.0));
    toolbar(ui, s, bar, &path, &ctrl);
    let canvas = Rect::from_min_max(pos2(right.left(), bar.bottom()), right.max);
    match s.animator.blend_view.clone() {
        Some(state) => {
            blend_view::show_tree_view(ui, s, canvas, &path, &ctrl, &state, live.as_ref())
        }
        None => graph::show(ui, s, canvas, &path, &ctrl, live_layer),
    }

    // Delete removes the selected state or transition.
    if s.animator.focused
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete))
        && let Some(item) = s.animator.selected.clone()
    {
        delete(s, &path, &item);
    }
}

/// Breadcrumb ("Base Layer ▸ Combat ▸ Locomotion"; click a part to go back) and the
/// controller's name.
fn toolbar(ui: &mut Ui, s: &mut EditorState, bar: Rect, path: &str, c: &AnimatorController) {
    ui.painter().rect_filled(bar, 0.0, theme::TAB_BAR_BG);
    ui.scope_builder(
        UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                let parts = graph::breadcrumb(c, s.animator.layer, &s.animator.machine);
                let last = parts.len() - 1;
                let in_tree = s.animator.blend_view.clone();
                for (i, (label, machine)) in parts.into_iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("▸").color(theme::TEXT_DIM));
                    }
                    let text = RichText::new(label).strong();
                    if i == last && in_tree.is_none() {
                        ui.label(text);
                    } else if ui.add(egui::Button::new(text).frame(false)).clicked() {
                        s.animator.enter_machine(machine);
                    }
                }
                if let Some(state) = in_tree {
                    ui.label(RichText::new("▸").color(theme::TEXT_DIM));
                    ui.label(RichText::new(format!("{state} (Blend Tree)")).strong());
                }
                if !s.animator.machine.is_empty() && s.animator.blend_view.is_none() {
                    let up = machine_parent(&s.animator.machine).to_owned();
                    if ui.small_button("⏶").on_hover_text("Up one level").clicked() {
                        s.animator.enter_machine(up);
                    }
                }
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

/// Screenshot setup (`MISTY_ANIMATOR`): comma-separated `layers` (Layers tab), `layer:N`,
/// `machine:Path`, `blend:State` (blend tree view), `motion:N` (highlighted blend motion),
/// `select:State` / `transition:N` / `machine-node:Path` (selected, shown in the Inspector) for the
/// selected object's controller.
pub fn demo(s: &mut EditorState, spec: &str) {
    let ctrl = s
        .selection
        .active()
        .and_then(|e| s.scene.world.get::<&Animator>(e).ok().map(|a| (e, a.controller.clone())));
    let Some((e, Some(path))) = ctrl else {
        return;
    };
    s.animator.open(Some(path.clone()));
    s.animator.target = Some(e);
    s.animator.followed = s.selection.active();
    for item in spec.split(',').map(str::trim) {
        let (key, value) = item.split_once(':').unwrap_or((item, ""));
        let item_selected = |s: &mut EditorState, g: GraphItem| {
            s.animator.selected = Some(g);
            s.selection.clear();
            s.animator.followed = None;
            s.inspector_asset = Some(path.clone());
        };
        match key {
            "layers" => s.animator.left_tab = LeftTab::Layers,
            "layer" => s.animator.show_layer(value.parse().unwrap_or(0)),
            "machine" => s.animator.enter_machine(value.to_owned()),
            "blend" => s.animator.blend_view = Some(value.to_owned()),
            "motion" => s.animator.blend_selected = value.parse().ok(),
            "select" => item_selected(s, GraphItem::Node(Node::State(value.to_owned()))),
            "machine-node" => item_selected(s, GraphItem::Node(Node::Machine(value.to_owned()))),
            "transition" => {
                if let Ok(i) = value.parse() {
                    item_selected(s, GraphItem::Transition(i));
                }
            }
            _ => {}
        }
    }
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

/// Delete a state (with its transitions), a sub-state machine (with everything in it) or one
/// transition of the shown layer.
pub(super) fn delete(s: &mut EditorState, path: &str, item: &GraphItem) {
    match item {
        GraphItem::Node(Node::State(name)) => {
            let name = name.clone();
            graph::edit_machine(s, path, "Delete State", false, |m| m.remove_state(&name));
        }
        GraphItem::Node(Node::Machine(p)) => {
            let p = p.clone();
            graph::edit_machine(s, path, "Delete Sub-State Machine", false, |m| {
                m.remove_sub_machine(&p)
            });
        }
        GraphItem::Transition(i) => {
            let i = *i;
            graph::edit_machine(s, path, "Delete Transition", false, |m| {
                if i < m.transitions.len() {
                    m.transitions.remove(i);
                }
            });
        }
        // Entry, Any State, Exit and Up always exist.
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
