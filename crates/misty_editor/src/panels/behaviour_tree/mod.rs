//! Behaviour Tree window (double-click a `.btree`, or Assets ▸ Create ▸ Behaviour Tree), part of
//! the optional Behaviour Trees package.
//!
//! The canvas shows the tree from left to right: a parent's children run top to bottom (the
//! small numbers on the wires). Drag nodes from the Nodes palette onto the canvas, drag from a
//! node's right edge to another node to make it a child, right-click for the node menu or to add
//! nodes there. Comments and groups annotate the tree. The Blackboard lists the tree's keys; the
//! Node panel edits the selected node. The window keeps a working copy with its own undo/redo,
//! copy/paste and Save Asset.
//!
//! In Play mode the window follows the agent selected in the Scene Tree (else the first agent
//! running the tree): running nodes glow yellow, nodes that just succeeded or failed flash green
//! or red, and the Blackboard shows the agent's live values.

mod blackboard;
pub(crate) mod canvas;
mod inspector;
mod palette;

pub use inspector::{assets_with, value_field};
#[cfg(test)]
pub use palette::scan as palette_scan;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use misty_engine::behaviour_tree::compile::{self, CompileError};
use misty_engine::behaviour_tree::runtime::DebugView;
use misty_engine::behaviour_tree::{BehaviourTree, BtNode, NodeKind};
use misty_engine::hecs::Entity;

use crate::i18n::tr;
use crate::layout::Tab;
use crate::packages::optional::{self, BEHAVIOUR_TREES};
use crate::panels::node_canvas::ERROR;
use crate::state::EditorState;
use crate::theme;

/// Icon of Behaviour Tree assets (Asset Files, tab title).
pub const ICON: &str = "♣";
const UNDO_LIMIT: usize = 100;

struct Snapshot {
    label: String,
    tree: BehaviourTree,
}

/// Behaviour Tree window state (kept while the tab is closed).
pub struct BehaviourTreeWindow {
    /// Tree shown (project-relative path).
    pub path: Option<String>,
    /// The working copy.
    pub tree: BehaviourTree,
    saved: BehaviourTree,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    grouping: Option<String>,
    /// Canvas scroll (graph units) and zoom.
    pub pan: Vec2,
    pub zoom: f32,
    pub selected: BTreeSet<u32>,
    /// Last click landed in this window (its shortcuts apply).
    pub focused: bool,
    /// Copied nodes as tree JSON.
    pub clipboard: Option<String>,
    pastes: u32,
    /// Problems of the last check.
    pub errors: Vec<CompileError>,
    version: u64,
    checked_version: u64,
    pub load_error: Option<String>,
    pub show_blackboard: bool,
    pub show_inspector: bool,
    pub show_palette: bool,
    /// What the playing game reports for the watched agent.
    pub live: Option<DebugView>,
    /// Agent picked in the toolbar while playing (None = follow the selection).
    pub watch: Option<Entity>,
    /// Agents running this tree (name, object), refreshed while playing.
    pub agents: Vec<(String, Entity)>,
    pub(crate) canvas: canvas::CanvasState,
    pub(crate) palette: palette::PaletteState,
}

impl Default for BehaviourTreeWindow {
    fn default() -> Self {
        Self {
            path: None,
            tree: BehaviourTree::default(),
            saved: BehaviourTree::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            grouping: None,
            pan: Vec2::ZERO,
            zoom: 1.0,
            selected: BTreeSet::new(),
            focused: false,
            clipboard: None,
            pastes: 0,
            errors: Vec::new(),
            version: 1,
            checked_version: 0,
            load_error: None,
            show_blackboard: true,
            show_inspector: true,
            show_palette: true,
            live: None,
            watch: None,
            agents: Vec::new(),
            canvas: Default::default(),
            palette: Default::default(),
        }
    }
}

thread_local! {
    // Mirrors `EditorState::optional_packages` for menus drawn without the state (Create ▸).
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
}

/// Note whether the project has the Behaviour Trees package (every frame).
pub fn note_package(s: &EditorState) {
    INSTALLED.with(|c| c.set(s.optional_packages.has(BEHAVIOUR_TREES)));
}

/// The project has the Behaviour Trees package (as of the last [`note_package`]).
pub fn package_installed() -> bool {
    INSTALLED.with(Cell::get)
}

/// Is the user mid-interaction (dragging, or typing in a field)?
pub fn interacting(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()
}

impl BehaviourTreeWindow {
    /// Load a tree from disk (unsaved edits of the previous one are dropped).
    pub fn open(&mut self, file: &std::path::Path, rel: &str) {
        let loaded = BehaviourTree::load(file);
        *self = Self {
            show_blackboard: self.show_blackboard,
            show_inspector: self.show_inspector,
            show_palette: self.show_palette,
            clipboard: self.clipboard.take(),
            ..Default::default()
        };
        self.path = Some(rel.to_owned());
        match loaded {
            Ok(t) => {
                self.saved = t.clone();
                self.tree = t;
                self.canvas.frame_all = true;
            }
            Err(e) => self.load_error = Some(e),
        }
    }

    pub fn zoom(&self) -> f32 {
        if self.zoom > 0.0 { self.zoom } else { 1.0 }
    }

    pub fn dirty(&self) -> bool {
        self.path.is_some() && self.tree != self.saved
    }

    /// Its shortcuts (Delete, Ctrl+Z…) belong to this window right now.
    pub fn wants_keys(&self) -> bool {
        self.focused && self.path.is_some()
    }

    /// Change the tree with undo; edits during one interaction with the same label share one
    /// undo step.
    pub fn edit(&mut self, label: &str, interacting: bool, f: impl FnOnce(&mut BehaviourTree)) {
        let grouped = interacting && self.grouping.as_deref() == Some(label);
        let before = self.tree.clone();
        f(&mut self.tree);
        if self.tree == before {
            return;
        }
        if !grouped {
            self.undo.push(Snapshot {
                label: label.to_owned(),
                tree: before,
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
        let current = std::mem::replace(&mut self.tree, s.tree);
        self.redo.push(Snapshot {
            label: s.label.clone(),
            tree: current,
        });
        self.after_history();
        Some(s.label)
    }

    pub fn redo(&mut self) -> Option<String> {
        let s = self.redo.pop()?;
        let current = std::mem::replace(&mut self.tree, s.tree);
        self.undo.push(Snapshot {
            label: s.label.clone(),
            tree: current,
        });
        self.after_history();
        Some(s.label)
    }

    fn after_history(&mut self) {
        self.grouping = None;
        self.version += 1;
        let ids: BTreeSet<u32> = self.tree.nodes.iter().map(|n| n.id).collect();
        self.selected.retain(|id| ids.contains(id));
    }

    /// Add a node and select it; with `parent`, it becomes that node's child.
    pub fn add_node(&mut self, kind: NodeKind, at: [f32; 2], parent: Option<u32>) -> u32 {
        let mut id = 0;
        self.edit("Create Node", false, |t| {
            id = t.next_id();
            t.nodes.push(BtNode::new(id, kind, at));
            if let Some(p) = parent {
                let _ = t.link(p, id);
            }
        });
        self.selected = [id].into();
        id
    }

    /// Make `child` a child of `parent`; Err holds why not.
    pub fn connect(&mut self, parent: u32, child: u32) -> Result<(), String> {
        self.tree.can_link(parent, child)?;
        self.edit("Connect", false, |t| {
            let _ = t.link(parent, child);
        });
        Ok(())
    }

    pub fn disconnect(&mut self, child: u32) {
        self.edit("Disconnect", false, |t| t.unlink(child));
    }

    /// Delete the selected nodes (the Root stays).
    pub fn delete_selected(&mut self) {
        let ids: Vec<u32> = self
            .selected
            .iter()
            .copied()
            .filter(|id| {
                self.tree
                    .node(*id)
                    .is_some_and(|n| n.kind != NodeKind::Root)
            })
            .collect();
        if ids.is_empty() {
            return;
        }
        self.edit("Delete", false, |t| {
            for id in &ids {
                t.remove_node(*id);
            }
        });
        self.selected.clear();
    }

    /// The selected nodes (not the Root) and the links between them, as tree JSON.
    pub fn copy_selected(&mut self) -> Option<String> {
        let nodes: Vec<BtNode> = self
            .tree
            .nodes
            .iter()
            .filter(|n| self.selected.contains(&n.id) && n.kind != NodeKind::Root)
            .map(|n| {
                let mut c = n.clone();
                c.children.retain(|k| self.selected.contains(k));
                c
            })
            .collect();
        if nodes.is_empty() {
            return None;
        }
        let part = BehaviourTree {
            nodes,
            ..Default::default()
        };
        let text = part.to_json();
        self.clipboard = Some(text.clone());
        self.pastes = 0;
        Some(text)
    }

    pub fn cut_selected(&mut self) -> Option<String> {
        let text = self.copy_selected()?;
        self.delete_selected();
        Some(text)
    }

    /// Paste copied nodes (or `text` when it holds tree JSON), a little further each time;
    /// the pasted nodes are selected. Keys they use that the tree lacks are added.
    pub fn paste(&mut self, text: Option<&str>) {
        let part = text
            .and_then(|t| BehaviourTree::from_json(t).ok())
            .filter(|t| !t.nodes.is_empty())
            .or_else(|| {
                self.clipboard
                    .as_deref()
                    .and_then(|t| BehaviourTree::from_json(t).ok())
            });
        let Some(part) = part else {
            return;
        };
        self.pastes += 1;
        let offset = 30.0 * self.pastes as f32;
        let mut new_ids = Vec::new();
        self.edit("Paste", false, |t| {
            let mut map = HashMap::new();
            for n in &part.nodes {
                if n.kind == NodeKind::Root {
                    continue;
                }
                let id = t.next_id();
                let mut c = n.clone();
                c.id = id;
                c.position = [n.position[0] + offset, n.position[1] + offset];
                c.children.clear();
                t.nodes.push(c);
                map.insert(n.id, id);
                new_ids.push(id);
            }
            for n in &part.nodes {
                let Some(&p) = map.get(&n.id) else { continue };
                let kids: Vec<u32> = n
                    .children
                    .iter()
                    .filter_map(|k| map.get(k).copied())
                    .collect();
                if let Some(pn) = t.node_mut(p) {
                    pn.children = kids;
                }
            }
            for k in &part.keys {
                if t.key(&k.name).is_none() {
                    t.keys.push(k.clone());
                }
            }
        });
        self.selected = new_ids.into_iter().collect();
    }

    /// Duplicate the selection (Ctrl+D) without touching the clipboard.
    pub fn duplicate_selected(&mut self) {
        let keep = self.clipboard.clone();
        let pastes = self.pastes;
        if let Some(text) = self.copy_selected() {
            self.pastes = 0;
            self.paste(Some(&text));
        }
        self.clipboard = keep;
        self.pastes = pastes;
    }

    /// Frame the selected nodes with a group (Ctrl+G).
    pub fn group_selected(&mut self) {
        let ids: Vec<u32> = self.selected.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        self.edit("Group Selection", false, |t| {
            for g in &mut t.groups {
                g.nodes.retain(|n| !ids.contains(n));
            }
            t.groups.retain(|g| !g.nodes.is_empty());
            t.groups.push(misty_engine::behaviour_tree::asset::Group {
                title: "New Group".into(),
                nodes: ids,
            });
        });
    }

    /// Add a comment at `at` (graph units).
    pub fn add_comment(&mut self, at: [f32; 2]) {
        self.edit("Add Comment", false, |t| {
            t.comments
                .push(misty_engine::behaviour_tree::asset::Comment {
                    text: "Comment".into(),
                    position: at,
                    size: [200.0, 70.0],
                });
        });
    }

    /// Write the tree.
    pub fn save(&mut self, file: &std::path::Path) -> Result<(), String> {
        if self.path.is_none() {
            return Err("no tree open".into());
        }
        self.tree.save(file)?;
        self.saved = self.tree.clone();
        Ok(())
    }

    /// Check the tree when it changed.
    fn check(&mut self) {
        if self.checked_version == self.version {
            return;
        }
        self.checked_version = self.version;
        self.errors = compile::compile(&self.tree).errors;
    }

    /// The first problem of a node.
    pub fn error_of(&self, id: u32) -> Option<&str> {
        self.errors
            .iter()
            .find(|e| e.node == Some(id))
            .map(|e| e.message.as_str())
    }

    /// Live state of a node: (current, last result, ticks since it).
    pub fn live_of(
        &self,
        id: u32,
    ) -> Option<(
        misty_engine::behaviour_tree::Status,
        misty_engine::behaviour_tree::Status,
        u64,
    )> {
        let live = self.live.as_ref()?;
        live.nodes
            .iter()
            .find(|n| n.0 == id)
            .map(|n| (n.1, n.2, n.3))
    }
}

/// Open a tree in the window and bring the tab forward.
pub fn open(s: &mut EditorState, rel: &str) {
    if s.behaviour_tree.path.as_deref() != Some(rel) {
        let file = s.assets.absolute(rel);
        s.behaviour_tree.open(&file, rel);
    }
    s.push(crate::actions::Action::FocusTab(Tab::BehaviourTree));
}

fn save(s: &mut EditorState) {
    let file = s
        .assets
        .absolute(s.behaviour_tree.path.as_deref().unwrap_or_default());
    match s.behaviour_tree.save(&file) {
        Ok(()) => {
            let rel = s.behaviour_tree.path.clone().unwrap_or_default();
            s.console.info(tr!("Saved {path}", path = rel));
            s.refresh_assets();
        }
        Err(e) => s
            .console
            .warn(tr!("Could not save the Behaviour Tree: {error}", error = e)),
    }
}

/// After a game frame: read the watched agent's node states and blackboard.
pub fn read_debugger(s: &mut EditorState) {
    let Some(path) = s.behaviour_tree.path.clone() else {
        return;
    };
    let Some(rt) = s.runtime.as_ref() else {
        s.behaviour_tree.live = None;
        s.behaviour_tree.agents.clear();
        return;
    };
    let agents = rt.behaviour_agents(&path);
    // The picked agent, else the selected object when it runs this tree.
    let watch = s
        .behaviour_tree
        .watch
        .filter(|e| agents.contains(e))
        .or_else(|| s.selection.active().filter(|e| agents.contains(e)));
    s.behaviour_tree.live = rt.behaviour_debug(&path, watch);
    s.behaviour_tree.agents = agents
        .into_iter()
        .filter(|e| s.scene.contains(*e))
        .map(|e| (s.scene.name(e), e))
        .collect();
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        s.behaviour_tree.focused = full.contains(p);
    }
    note_package(s);
    if !package_installed() {
        not_installed(ui, s, full);
        return;
    }
    if s.behaviour_tree.path.is_none() {
        centered(
            ui,
            full,
            &tr!("Double-click a Behaviour Tree asset, or use Assets ▸ Create ▸ Behaviour Tree."),
        );
        return;
    }
    if let Some(e) = s.behaviour_tree.load_error.clone() {
        centered(ui, full, &e);
        return;
    }
    if !s.play.in_play_mode() {
        s.behaviour_tree.live = None;
        s.behaviour_tree.agents.clear();
    }
    let busy = interacting(ui.ctx());
    s.behaviour_tree.end_interaction(busy);
    s.behaviour_tree.check();

    let bar = Rect::from_min_size(full.min, vec2(full.width(), 26.0));
    toolbar(ui, s, bar);
    let area = Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max);
    canvas::show(ui, s, area);
    let pad = 8.0;
    let mut left_top = area.top() + pad;
    if s.behaviour_tree.show_palette {
        let h = (area.height() * 0.45).min(palette::height(s));
        let r = Rect::from_min_size(
            pos2(area.left() + pad, left_top),
            vec2(210.0f32.min(area.width() * 0.3), h),
        );
        palette::show(ui, s, r);
        left_top = r.bottom() + pad;
    }
    if s.behaviour_tree.show_blackboard {
        let r = Rect::from_min_max(
            pos2(area.left() + pad, left_top),
            pos2(
                area.left() + pad + 240.0f32.min(area.width() * 0.32),
                (left_top + blackboard::height(s)).min(area.bottom() - pad),
            ),
        );
        if r.height() > 40.0 {
            blackboard::show(ui, s, r);
        }
    }
    if s.behaviour_tree.show_inspector {
        let w = 260.0f32.min(area.width() * 0.35);
        let r = Rect::from_min_size(
            pos2(area.right() - w - pad, area.top() + pad),
            vec2(w, (area.height() - 2.0 * pad).min(inspector::height(s))),
        );
        inspector::show(ui, s, r);
    }
    shortcuts(ui, s);
}

/// The window without the package: what it is and an Install button.
fn not_installed(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(24.0)), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(rect.height() * 0.3);
            ui.heading(tr!("Behaviour Trees"));
            ui.label(tr!(
                "The Behaviour Trees package is not installed in this project."
            ));
            ui.add_space(6.0);
            if ui.button(tr!("Install Behaviour Trees")).clicked() {
                let root = s.project.root.clone();
                s.packages.ensure_loaded(&root);
                s.packages.install_optional(&root, BEHAVIOUR_TREES);
                s.optional_packages = optional::Installed::load(&root);
                note_package(s);
                if let Some((err, m)) = s.packages.status.clone() {
                    if err {
                        s.console.warn(m);
                    } else {
                        s.console.info(m);
                    }
                }
            }
            if ui.link(tr!("Open the Package Manager")).clicked() {
                s.push(crate::actions::Action::FocusTab(Tab::PackageManager));
            }
        });
    });
}

/// Keys while the window has focus.
fn shortcuts(ui: &mut Ui, s: &mut EditorState) {
    if !s.behaviour_tree.wants_keys() || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    use egui::{Key, KeyboardShortcut, Modifiers};
    let ctrl = Modifiers::COMMAND;
    let pressed =
        |ui: &mut Ui, m, k| ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
    if pressed(ui, ctrl | Modifiers::SHIFT, Key::Z) || pressed(ui, ctrl, Key::Y) {
        if let Some(l) = s.behaviour_tree.redo() {
            s.console.info(tr!("Redo {action}", action = tr(&l)));
        }
    } else if pressed(ui, ctrl, Key::Z)
        && let Some(l) = s.behaviour_tree.undo()
    {
        s.console.info(tr!("Undo {action}", action = tr(&l)));
    }
    if pressed(ui, ctrl, Key::S) {
        save(s);
    }
    if pressed(ui, ctrl, Key::D) {
        s.behaviour_tree.duplicate_selected();
    }
    if pressed(ui, ctrl, Key::G) {
        s.behaviour_tree.group_selected();
    }
    let events = ui.input(|i| i.events.clone());
    for e in events {
        match e {
            egui::Event::Copy => {
                if let Some(text) = s.behaviour_tree.copy_selected() {
                    ui.ctx().copy_text(text);
                }
            }
            egui::Event::Cut => {
                if let Some(text) = s.behaviour_tree.cut_selected() {
                    ui.ctx().copy_text(text);
                }
            }
            egui::Event::Paste(text) => s.behaviour_tree.paste(Some(&text)),
            _ => {}
        }
    }
    if pressed(ui, Modifiers::NONE, Key::Delete) || pressed(ui, Modifiers::NONE, Key::Backspace) {
        s.behaviour_tree.delete_selected();
    }
    if pressed(ui, Modifiers::NONE, Key::F) {
        s.behaviour_tree.canvas.frame_all = true;
    }
}

fn toolbar(ui: &mut Ui, s: &mut EditorState, bar: Rect) {
    ui.painter().rect_filled(bar, 0.0, theme::tab_bar_bg());
    ui.scope_builder(
        UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                let dirty = s.behaviour_tree.dirty();
                let text = if dirty {
                    RichText::new(tr!("Save Asset")).strong()
                } else {
                    RichText::new(tr!("Save Asset"))
                };
                if ui
                    .add_enabled(dirty, egui::Button::new(text))
                    .on_hover_text(tr!("Write the tree (Ctrl+S)"))
                    .clicked()
                {
                    save(s);
                }
                if ui.button(tr!("Show in Asset Files")).clicked()
                    && let Some(rel) = s.behaviour_tree.path.clone()
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
                if ui
                    .add_enabled(s.behaviour_tree.can_undo(), egui::Button::new("⟲"))
                    .on_hover_text(tr!("Undo (Ctrl+Z)"))
                    .clicked()
                {
                    s.behaviour_tree.undo();
                }
                if ui
                    .add_enabled(s.behaviour_tree.can_redo(), egui::Button::new("⟳"))
                    .on_hover_text(tr!("Redo (Ctrl+Y)"))
                    .clicked()
                {
                    s.behaviour_tree.redo();
                }
                if ui
                    .button(tr!("Arrange"))
                    .on_hover_text(tr!("Lay the tree out from the Root"))
                    .clicked()
                {
                    s.behaviour_tree.edit("Arrange", false, |t| t.arrange());
                    s.behaviour_tree.canvas.frame_all = true;
                }
                ui.separator();
                let name = s
                    .behaviour_tree
                    .path
                    .as_deref()
                    .map(misty_engine::scene::clip_name)
                    .unwrap_or_default();
                ui.label(
                    RichText::new(format!("{ICON} {name}{}", if dirty { " *" } else { "" }))
                        .color(theme::text()),
                );
                let problems: Vec<&CompileError> = s
                    .behaviour_tree
                    .errors
                    .iter()
                    .filter(|e| !compile::is_warning(e))
                    .collect();
                match problems.first() {
                    Some(e) => {
                        let n = problems.len();
                        let more = if n > 1 {
                            tr!(" (+{count} more)", count = n - 1)
                        } else {
                            String::new()
                        };
                        ui.label(RichText::new(format!("⚠ {}{more}", e.message)).color(ERROR))
                            .on_hover_text(&e.message);
                    }
                    None if s.behaviour_tree.live.is_none() => {
                        ui.label(RichText::new(tr!("✔ No problems")).color(theme::text_dim()));
                    }
                    None => {}
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let w = &mut s.behaviour_tree;
                    ui.toggle_value(&mut w.show_inspector, tr!("Node"));
                    ui.toggle_value(&mut w.show_blackboard, tr!("Blackboard"));
                    ui.toggle_value(&mut w.show_palette, tr!("Nodes"));
                    if s.play.in_play_mode() {
                        ui.separator();
                        agent_picker(ui, s);
                    }
                });
            });
        },
    );
}

/// While playing: which agent the window follows.
fn agent_picker(ui: &mut Ui, s: &mut EditorState) {
    let w = &mut s.behaviour_tree;
    if w.agents.is_empty() {
        ui.label(RichText::new(tr!("No object runs this tree")).color(theme::text_dim()));
        return;
    }
    let shown = w
        .live
        .as_ref()
        .and_then(|l| l.entity)
        .and_then(|e| w.agents.iter().find(|a| a.1 == e))
        .map(|a| a.0.clone())
        .unwrap_or_default();
    let follow = tr!("Follow Selection");
    egui::ComboBox::from_id_salt("bt_agent")
        .selected_text(tr!(
            "Debugging {object}",
            object = crate::i18n::display(&shown)
        ))
        .show_ui(ui, |ui| {
            if ui.selectable_label(w.watch.is_none(), &follow).clicked() {
                w.watch = None;
            }
            let agents = w.agents.clone();
            for (name, e) in agents {
                if ui
                    .selectable_label(w.watch == Some(e), crate::i18n::display(&name))
                    .clicked()
                {
                    w.watch = Some(e);
                }
            }
        });
}

fn centered(ui: &mut Ui, rect: Rect, text: &str) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        theme::text_dim(),
    );
}

/// A new Behaviour Tree asset (a Root and a Selector).
pub fn new_tree() -> BehaviourTree {
    BehaviourTree::new_default()
}
