//! Effect Graph window (double-click an `.effect`): contexts drawn as vertical stacks of blocks,
//! operator nodes wired into block inputs, the Blackboard of exposed properties and Save Asset.
//!
//! The window edits a working copy with its own undo/redo. While it differs from the file, the
//! working copy is compiled into the effect library in place of the asset, so every Visual
//! Effect using it in the Scene view shows the unsaved graph live; Save Asset writes the file.

mod blackboard;
pub(crate) use blackboard::value_ui as blackboard_value;
mod canvas;
pub(crate) mod fields;
pub(crate) mod layout;
mod search;

#[cfg(test)]
mod tests;

use crate::i18n::tr;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use misty_engine::vfx::{EffectGraph, GraphError, library};

use crate::layout::Tab;
use crate::state::EditorState;
use crate::theme;

/// Icon of Effect Graph assets (Project window, tab title).
pub const ICON: &str = "✺";
const UNDO_LIMIT: usize = 100;

struct Snapshot {
    label: String,
    graph: EffectGraph,
}

/// Effect Graph window state (kept while the tab is closed).
pub struct EffectGraphWindow {
    /// Graph shown (project-relative path).
    pub path: Option<String>,
    /// The working copy.
    pub graph: EffectGraph,
    saved: EffectGraph,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    grouping: Option<String>,
    /// Canvas scroll (graph units) and zoom (0 = 1).
    pub pan: Vec2,
    pub zoom: f32,
    /// Selected contexts, blocks and operators (by id).
    pub selected: BTreeSet<u32>,
    /// Last click landed in this window (Delete, Space and Ctrl+Z/Y/S act on the graph).
    pub focused: bool,
    pub(crate) canvas: canvas::CanvasState,
    pub(crate) search: Option<search::Search>,
    pub(crate) blackboard: blackboard::BlackboardState,
    /// Problems of the last compile (empty = it compiles).
    pub errors: Vec<GraphError>,
    compiled_key: Option<u64>,
    version: u64,
    checked_version: u64,
    pub load_error: Option<String>,
    pub show_blackboard: bool,
}

impl Default for EffectGraphWindow {
    fn default() -> Self {
        Self {
            path: None,
            graph: EffectGraph::default(),
            saved: EffectGraph::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            grouping: None,
            pan: Vec2::ZERO,
            zoom: 1.0,
            selected: BTreeSet::new(),
            focused: false,
            canvas: Default::default(),
            search: None,
            blackboard: Default::default(),
            errors: Vec::new(),
            compiled_key: None,
            version: 1,
            checked_version: 0,
            load_error: None,
            show_blackboard: true,
        }
    }
}

/// Is the user mid-interaction (dragging, or typing in a field)?
pub fn interacting(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()
}

/// Content of a graph that affects the effect (not positions or notes).
fn content_key(g: &EffectGraph) -> u64 {
    let mut c = g.clone();
    for x in &mut c.contexts {
        x.position = [0.0; 2];
    }
    for o in &mut c.operators {
        o.position = [0.0; 2];
    }
    c.notes.clear();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    c.to_json().hash(&mut h);
    h.finish()
}

impl EffectGraphWindow {
    /// Load a graph from disk (replacing the working copy; an unsaved one is dropped).
    pub fn open(&mut self, file: &std::path::Path, rel: &str) {
        if let Some(old) = self.path.clone() {
            library::clear_working_copy(&old);
            if misty_engine::vfx::subgraph::is_subgraph_path(&old) {
                misty_engine::vfx::subgraph::forget(&old);
                for e in library::users_of(&old) {
                    library::reload(&e);
                }
            }
        }
        let loaded = EffectGraph::load(file);
        *self = Self {
            show_blackboard: self.show_blackboard,
            ..Default::default()
        };
        self.path = Some(rel.to_owned());
        match loaded {
            Ok(g) => {
                self.saved = g.clone();
                self.graph = g;
                self.canvas.frame_all = true;
            }
            Err(e) => self.load_error = Some(e),
        }
    }

    /// Is the open document a sub-graph (`.vfxoperator`: operators and Output nodes only)?
    pub fn is_subgraph(&self) -> bool {
        self.path
            .as_deref()
            .is_some_and(misty_engine::vfx::subgraph::is_subgraph_path)
    }

    pub fn zoom(&self) -> f32 {
        if self.zoom > 0.0 { self.zoom } else { 1.0 }
    }

    pub fn dirty(&self) -> bool {
        self.path.is_some() && self.graph != self.saved
    }

    /// Delete / Space / Ctrl+Z belong to this window right now.
    pub fn wants_keys(&self) -> bool {
        self.focused && self.path.is_some()
    }

    /// Change the graph with undo; edits during one interaction with the same label share a
    /// single undo step.
    pub fn edit(&mut self, label: &str, interacting: bool, f: impl FnOnce(&mut EffectGraph)) {
        let grouped = interacting && self.grouping.as_deref() == Some(label);
        let before = self.graph.clone();
        f(&mut self.graph);
        if self.graph == before {
            return;
        }
        if !grouped {
            self.undo.push(Snapshot {
                label: label.to_owned(),
                graph: before,
            });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.grouping = interacting.then(|| label.to_owned());
        self.version += 1;
    }

    pub fn end_interaction(&mut self, interacting: bool) {
        if !interacting {
            self.grouping = None;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Option<String> {
        let s = self.undo.pop()?;
        let current = std::mem::replace(&mut self.graph, s.graph);
        self.redo.push(Snapshot {
            label: s.label.clone(),
            graph: current,
        });
        self.after_history();
        Some(s.label)
    }

    pub fn redo(&mut self) -> Option<String> {
        let s = self.redo.pop()?;
        let current = std::mem::replace(&mut self.graph, s.graph);
        self.undo.push(Snapshot {
            label: s.label.clone(),
            graph: current,
        });
        self.after_history();
        Some(s.label)
    }

    fn after_history(&mut self) {
        self.grouping = None;
        self.version += 1;
        self.selected.retain(|id| self.graph.find(*id).is_some());
    }

    /// Delete the selected contexts, blocks and operators.
    pub fn delete_selected(&mut self) {
        let ids: Vec<u32> = self.selected.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        self.edit("Delete", false, |g| {
            for id in &ids {
                g.remove(*id);
            }
        });
        self.selected.clear();
    }

    /// Duplicate the selected operators (and the links between them).
    pub fn duplicate_selected(&mut self) {
        let ids: Vec<u32> = self
            .selected
            .iter()
            .copied()
            .filter(|id| self.graph.operator(*id).is_some())
            .collect();
        if ids.is_empty() {
            return;
        }
        let mut new_ids = Vec::new();
        self.edit("Duplicate", false, |g| {
            let mut map = std::collections::HashMap::new();
            for id in &ids {
                let Some(o) = g.operator(*id).cloned() else {
                    continue;
                };
                let new =
                    g.add_operator(o.kind.clone(), [o.position[0] + 30.0, o.position[1] + 30.0]);
                if let Some(n) = g.operator_mut(new) {
                    n.values = o.values.clone();
                }
                map.insert(*id, new);
                new_ids.push(new);
            }
            let inner: Vec<_> = g
                .links
                .iter()
                .filter(|l| map.contains_key(&l.from) && map.contains_key(&l.to))
                .cloned()
                .collect();
            for l in inner {
                let _ = g.connect(map[&l.from], &l.output, map[&l.to], &l.input);
            }
        });
        self.selected = new_ids.into_iter().collect();
    }

    /// Write the graph and reload it everywhere it is used.
    pub fn save(&mut self, file: &std::path::Path) -> Result<(), String> {
        let Some(rel) = self.path.clone() else {
            return Err("no graph open".into());
        };
        self.graph.save(file)?;
        self.saved = self.graph.clone();
        library::reload(&rel);
        Ok(())
    }

    /// Recompile when the content changed: error badges and the live working copy.
    fn check(&mut self) {
        if self.checked_version == self.version {
            return;
        }
        self.checked_version = self.version;
        let key = content_key(&self.graph);
        if self.compiled_key == Some(key) {
            return;
        }
        self.compiled_key = Some(key);
        if self.is_subgraph() {
            // Sub-graphs compile inside the effects that use them; those follow the edit live.
            self.errors.clear();
            if let Some(rel) = &self.path {
                misty_engine::vfx::subgraph::register(rel, self.graph.clone());
                for e in library::users_of(rel) {
                    library::reload(&e);
                }
            }
            return;
        }
        self.errors = misty_engine::vfx::compile(&self.graph)
            .err()
            .unwrap_or_default();
        if let Some(rel) = &self.path {
            if self.graph == self.saved {
                library::clear_working_copy(rel);
            } else {
                library::set_working_copy(rel, &self.graph);
            }
        }
    }

    /// The error shown on a node (context, block or operator).
    pub fn error_of(&self, id: u32) -> Option<&GraphError> {
        self.errors.iter().find(|e| e.node == Some(id))
    }
}

/// Open a graph in the window and bring the tab forward.
pub fn open(s: &mut EditorState, rel: &str) {
    if s.effect_graph.path.as_deref() != Some(rel) {
        let file = s.assets.absolute(rel);
        s.effect_graph.open(&file, rel);
    }
    s.push(crate::actions::Action::FocusTab(Tab::EffectGraph));
}

fn save(s: &mut EditorState) {
    let file = s
        .assets
        .absolute(s.effect_graph.path.as_deref().unwrap_or_default());
    match s.effect_graph.save(&file) {
        Ok(()) => {
            let rel = s.effect_graph.path.clone().unwrap_or_default();
            s.console.info(format!("Saved {rel}"));
            s.refresh_assets();
        }
        Err(e) => s
            .console
            .warn(format!("Could not save the Effect Graph: {e}")),
    }
}

/// Entities whose Visual Effect uses the open graph (the live preview targets).
pub fn preview_targets(s: &EditorState) -> Vec<misty_engine::hecs::Entity> {
    let Some(rel) = s.effect_graph.path.as_deref() else {
        return Vec::new();
    };
    s.scene
        .world
        .query::<(misty_engine::hecs::Entity, &misty_engine::vfx::VisualEffect)>()
        .iter()
        .filter(|(_, v)| v.effect.as_deref() == Some(rel))
        .map(|(e, _)| e)
        .collect()
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        s.effect_graph.focused = full.contains(p);
    }
    if s.effect_graph.path.is_none() {
        centered(
            ui,
            full,
            "Double-click an Effect Graph asset, or use Assets ▸ Create ▸ Visual Effects ▸ Effect Graph.",
        );
        return;
    }
    if let Some(e) = s.effect_graph.load_error.clone() {
        centered(ui, full, &e);
        return;
    }
    let busy = interacting(ui.ctx());
    s.effect_graph.end_interaction(busy);
    s.effect_graph.check();

    let bar = Rect::from_min_size(full.min, vec2(full.width(), 26.0));
    toolbar(ui, s, bar);
    let area = Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max);
    canvas::show(ui, s, area);
    if s.effect_graph.show_blackboard {
        let pad = 8.0;
        let r = Rect::from_min_size(
            area.min + vec2(pad, pad),
            vec2(
                250.0f32.min(area.width() * 0.4),
                (area.height() - 2.0 * pad).min(blackboard::height(&s.effect_graph)),
            ),
        );
        blackboard::show(ui, s, r);
    }
    search::show(ui, s);
    shortcuts(ui, s);
}

/// Keys while the window has focus (the editor skips its own bindings for these).
fn shortcuts(ui: &mut Ui, s: &mut EditorState) {
    if !s.effect_graph.wants_keys() || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    use egui::{Key, KeyboardShortcut, Modifiers};
    let ctrl = Modifiers::COMMAND;
    let pressed =
        |ui: &mut Ui, m, k| ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
    if pressed(ui, ctrl | Modifiers::SHIFT, Key::Z) || pressed(ui, ctrl, Key::Y) {
        if let Some(l) = s.effect_graph.redo() {
            s.console.info(format!("Redo {l}"));
        }
    } else if pressed(ui, ctrl, Key::Z)
        && let Some(l) = s.effect_graph.undo()
    {
        s.console.info(format!("Undo {l}"));
    }
    if pressed(ui, ctrl, Key::S) {
        save(s);
    }
    if pressed(ui, ctrl, Key::D) {
        s.effect_graph.duplicate_selected();
    }
    if pressed(ui, Modifiers::NONE, Key::Delete) || pressed(ui, Modifiers::NONE, Key::Backspace) {
        s.effect_graph.delete_selected();
    }
    if pressed(ui, Modifiers::NONE, Key::F) {
        s.effect_graph.canvas.frame_all = true;
    }
    if pressed(ui, Modifiers::NONE, Key::Space)
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
    {
        canvas::open_search_at(s, p);
    }
}

fn toolbar(ui: &mut Ui, s: &mut EditorState, bar: Rect) {
    ui.painter().rect_filled(bar, 0.0, theme::TAB_BAR_BG);
    let targets = preview_targets(s);
    ui.scope_builder(
        UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                let dirty = s.effect_graph.dirty();
                let text = if dirty {
                    RichText::new("Save Asset").strong()
                } else {
                    RichText::new("Save Asset")
                };
                if ui
                    .add_enabled(dirty, egui::Button::new(text))
                    .on_hover_text("Write the graph (Ctrl+S)")
                    .clicked()
                {
                    save(s);
                }
                if ui.button(tr!("Show in Asset Files")).clicked()
                    && let Some(rel) = s.effect_graph.path.clone()
                {
                    let abs = s.assets.absolute(&rel);
                    if let Some(dir) = abs.parent() {
                        s.project.current = dir.to_path_buf();
                        s.project.refresh();
                    }
                    s.project.selected = Some(abs);
                    s.push(crate::actions::Action::FocusTab(Tab::Project));
                }
                ui.separator();
                let undo = s.effect_graph.can_undo();
                if ui
                    .add_enabled(undo, egui::Button::new("⟲"))
                    .on_hover_text("Undo (Ctrl+Z)")
                    .clicked()
                {
                    s.effect_graph.undo();
                }
                let redo = s.effect_graph.can_redo();
                if ui
                    .add_enabled(redo, egui::Button::new("⟳"))
                    .on_hover_text("Redo (Ctrl+Y)")
                    .clicked()
                {
                    s.effect_graph.redo();
                }
                ui.separator();
                let name = s
                    .effect_graph
                    .path
                    .as_deref()
                    .map(misty_engine::scene::clip_name)
                    .unwrap_or_default();
                ui.label(
                    RichText::new(format!("{ICON} {name}{}", if dirty { " *" } else { "" }))
                        .color(theme::TEXT),
                );
                match s.effect_graph.errors.first() {
                    Some(e) => {
                        let n = s.effect_graph.errors.len();
                        let more = if n > 1 {
                            format!(" (+{} more)", n - 1)
                        } else {
                            String::new()
                        };
                        ui.label(
                            RichText::new(format!("⚠ {}{more}", short(&e.message))).color(ERROR),
                        )
                        .on_hover_text(&e.message);
                    }
                    None => {
                        ui.label(RichText::new("✔ Compiled").color(theme::TEXT_DIM));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut s.effect_graph.show_blackboard, "Blackboard");
                    ui.separator();
                    // Play controls for the Visual Effects using this graph (Scene view preview).
                    let any = !targets.is_empty();
                    let mut cmd = None;
                    if ui
                        .add_enabled(any, egui::Button::new("⟲ Reinit"))
                        .on_hover_text("Clear the particles and restart")
                        .clicked()
                    {
                        cmd = Some(Preview::Reinit);
                    }
                    if ui.add_enabled(any, egui::Button::new("■ Stop")).clicked() {
                        cmd = Some(Preview::Stop);
                    }
                    if ui.add_enabled(any, egui::Button::new("▶ Play")).clicked() {
                        cmd = Some(Preview::Play);
                    }
                    let label = match targets.len() {
                        0 => "No Visual Effect uses this graph".to_owned(),
                        1 => format!("Previewing {}", s.scene.name(targets[0])),
                        n => format!("Previewing {n} effects"),
                    };
                    ui.label(RichText::new(label).color(theme::TEXT_DIM));
                    if let Some(c) = cmd {
                        preview_command(s, &targets, c);
                    }
                });
            });
        },
    );
}

#[derive(Clone, Copy)]
enum Preview {
    Play,
    Stop,
    Reinit,
}

fn preview_command(s: &mut EditorState, targets: &[misty_engine::hecs::Entity], c: Preview) {
    for e in targets {
        misty_engine::vfx::with_instance(&mut s.scene, *e, |i, v| match c {
            Preview::Play => i.play(v),
            Preview::Stop => i.stop(),
            Preview::Reinit => i.reinit(v),
        });
    }
}

/// Error text colour (soft red).
pub const ERROR: Color32 = Color32::from_rgb(232, 96, 90);

fn short(text: &str) -> String {
    let first = text.lines().next().unwrap_or("");
    if first.chars().count() > 70 {
        format!("{}…", first.chars().take(70).collect::<String>())
    } else {
        first.to_owned()
    }
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

/// A new Effect Graph asset (Assets ▸ Create ▸ Visual Effects ▸ Effect Graph): the default
/// system (constant rate, sphere, velocity, lifetime, drag, colour over life).
pub fn new_graph() -> EffectGraph {
    EffectGraph::new_default()
}

/// A new Effect Sub-graph (Assets ▸ Create ▸ Visual Effects ▸ Effect Sub-graph): an input
/// property doubled into one Output, to start from.
pub fn new_subgraph() -> EffectGraph {
    use misty_engine::vfx::{OperatorKind, PropertyValue, types::Value};
    let mut g = EffectGraph::default();
    g.add_property("In", PropertyValue::Float(1.0));
    let p = g.add_operator(OperatorKind::Property { name: "In".into() }, [0.0, 0.0]);
    let m = g.add_operator(OperatorKind::Multiply, [200.0, 0.0]);
    if let Some(o) = g.operator_mut(m) {
        o.values.insert("B".into(), Value::Float(2.0));
    }
    let out = g.add_operator(
        OperatorKind::SubgraphOutput { name: "Out".into() },
        [400.0, 0.0],
    );
    let _ = g.connect(p, "Out", m, "A");
    let _ = g.connect(m, "Out", out, "Value");
    g
}
