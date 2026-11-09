//! Script Graph window (double-click a `.vgraph`, or Assets ▸ Create ▸ Visual Script Graph):
//! the node canvas with groups and sticky notes, the Blackboard of variables, the Graph
//! Inspector (settings of the selected node), Save Asset, and the debugger while playing.
//!
//! Right-click or Space opens the node search, organised by category (events, flow, variables,
//! math, the engine API generated from the Lua binding table, Lua, subgraphs); dragging a wire
//! into empty space opens it filtered to nodes that fit. Wires are type-checked; nodes with
//! problems get a red badge.
//!
//! In Play mode the graph shows the instance on the selected object (else the first one): exec
//! wires that just carried the flow glow, hovering a pin shows its value, breakpoints (F9 or the
//! node menu) pause Play before the node runs, and Step / Continue drive the paused flow.
//!
//! The window edits a working copy with its own undo/redo. Macros (subgraphs stored in the
//! file) open in the same window: double-click their node; the toolbar's path goes back.

mod blackboard;
pub(crate) mod canvas;
mod inspector;
pub(crate) mod search;

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use misty_engine::visual_script::{
    self, FileResolver, GraphError, Macro, Node, ScriptGraph, compile,
};

use crate::layout::Tab;
use crate::panels::node_canvas::ERROR;
use crate::state::EditorState;
use crate::theme;

/// Icon of Script Graph assets (Project window, tab title).
pub const ICON: &str = "◇";
const UNDO_LIMIT: usize = 100;

struct Snapshot {
    label: String,
    graph: ScriptGraph,
}

/// What the playing game reports for the open graph.
#[derive(Clone, Debug, Default)]
pub struct Live {
    /// Exec entries reached: (node, input pin) → runtime frame.
    pub hits: HashMap<(u32, String), u64>,
    pub frame: u64,
    /// Node the flow is paused before.
    pub paused: Option<u32>,
    /// Run-time errors by node.
    pub errors: HashMap<u32, String>,
    /// Variables of the watched instance (name, value).
    pub variables: Vec<(String, String)>,
    /// The object shown.
    pub watched: Option<String>,
}

/// Script Graph window state (kept while the tab is closed).
pub struct ScriptGraphWindow {
    /// Graph shown (project-relative path).
    pub path: Option<String>,
    /// The working copy (the whole file).
    pub graph: ScriptGraph,
    saved: ScriptGraph,
    /// Macro being edited, by name from the file's graph (empty = the graph itself).
    pub macro_path: Vec<String>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    grouping: Option<String>,
    /// Canvas scroll (graph units) and zoom.
    pub pan: Vec2,
    pub zoom: f32,
    pub selected: BTreeSet<u32>,
    /// Last click landed in this window (Delete, Space, Ctrl+Z/Y/S/C/V act on the graph).
    pub focused: bool,
    pub(crate) canvas: canvas::CanvasState,
    pub(crate) search: Option<search::Search>,
    pub(crate) blackboard: blackboard::BlackboardState,
    /// Problems of the last check (empty = it compiles).
    pub errors: Vec<GraphError>,
    version: u64,
    checked_version: u64,
    pub load_error: Option<String>,
    pub show_blackboard: bool,
    pub show_inspector: bool,
    /// Copied nodes as graph JSON (kept while other graphs are opened).
    pub clipboard: Option<String>,
    pastes: u32,
    /// Breakpoints (node ids of the file's graph).
    pub breakpoints: BTreeSet<u32>,
    /// Debug state from the game while playing.
    pub live: Option<Live>,
}

impl Default for ScriptGraphWindow {
    fn default() -> Self {
        Self {
            path: None,
            graph: ScriptGraph::default(),
            saved: ScriptGraph::default(),
            macro_path: Vec::new(),
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
            version: 1,
            checked_version: 0,
            load_error: None,
            show_blackboard: true,
            show_inspector: true,
            clipboard: None,
            pastes: 0,
            breakpoints: BTreeSet::new(),
            live: None,
        }
    }
}

/// Is the user mid-interaction (dragging, or typing in a field)?
pub fn interacting(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()
}

fn macro_in<'a>(g: &'a ScriptGraph, path: &[String]) -> Option<&'a ScriptGraph> {
    let mut cur = g;
    for name in path {
        cur = cur.macro_graph(name)?;
    }
    Some(cur)
}

fn macro_in_mut<'a>(g: &'a mut ScriptGraph, path: &[String]) -> Option<&'a mut ScriptGraph> {
    let mut cur = g;
    for name in path {
        cur = &mut cur.macros.iter_mut().find(|m| &m.name == name)?.graph;
    }
    Some(cur)
}

impl ScriptGraphWindow {
    /// Load a graph from disk (replacing the working copy; unsaved edits are dropped).
    pub fn open(&mut self, file: &std::path::Path, rel: &str) {
        let loaded = ScriptGraph::load(file);
        *self = Self {
            show_blackboard: self.show_blackboard,
            show_inspector: self.show_inspector,
            clipboard: self.clipboard.take(),
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

    /// The graph on the canvas (the file's graph, or the macro being edited).
    pub fn view(&self) -> &ScriptGraph {
        macro_in(&self.graph, &self.macro_path).unwrap_or(&self.graph)
    }

    /// Editing the file's own graph (not a macro): debugging applies.
    pub fn at_root(&self) -> bool {
        self.macro_path.is_empty()
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

    /// The project folder (for subgraph assets).
    fn files(root: &std::path::Path) -> FileResolver {
        FileResolver(root.to_path_buf())
    }

    /// Change the graph on the canvas with undo; edits during one interaction with the same
    /// label share a single undo step.
    pub fn edit(&mut self, label: &str, interacting: bool, f: impl FnOnce(&mut ScriptGraph)) {
        let grouped = interacting && self.grouping.as_deref() == Some(label);
        let before = self.graph.clone();
        let path = self.macro_path.clone();
        match macro_in_mut(&mut self.graph, &path) {
            Some(g) => f(g),
            None => f(&mut self.graph),
        }
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
        if macro_in(&self.graph, &self.macro_path).is_none() {
            self.macro_path.clear();
        }
        let ids: BTreeSet<u32> = self.view().nodes.iter().map(|n| n.id).collect();
        self.selected.retain(|id| ids.contains(id));
    }

    /// Open a macro of the graph on the canvas.
    pub fn enter_macro(&mut self, name: &str) {
        if self.view().macro_graph(name).is_none() {
            return;
        }
        self.macro_path.push(name.to_owned());
        self.after_navigation();
    }

    /// Back to the graph `depth` macros deep (0 = the file's graph).
    pub fn leave_macro(&mut self, depth: usize) {
        self.macro_path.truncate(depth);
        self.after_navigation();
    }

    fn after_navigation(&mut self) {
        self.selected.clear();
        self.search = None;
        self.canvas.frame_all = true;
        self.version += 1;
    }

    /// Wire two pins (either order), with undo; Err holds why not.
    pub fn connect(
        &mut self,
        root: &std::path::Path,
        a: (u32, &str, bool),
        b: (u32, &str, bool),
    ) -> Result<(), String> {
        let ((fn_, fp), (tn, tp)) = match (a.2, b.2) {
            (true, false) => ((a.0, a.1), (b.0, b.1)),
            (false, true) => ((b.0, b.1), (a.0, a.1)),
            _ => return Err("connect an output to an input".into()),
        };
        let files = Self::files(root);
        if let Some(e) = self.view().check_connection(fn_, fp, tn, tp, &files) {
            return Err(e);
        }
        let (fp, tp) = (fp.to_owned(), tp.to_owned());
        self.edit("Connect", false, |g| {
            let _ = g.connect(fn_, &fp, tn, &tp, &files);
        });
        Ok(())
    }

    /// Add a node and select it; returns its id.
    pub fn add_node(&mut self, kind: &str, params: &[(String, String)], at: [f32; 2]) -> u32 {
        let mut id = 0;
        let kind = kind.to_owned();
        let params = params.to_vec();
        self.edit("Create Node", false, |g| {
            id = g.next_id();
            g.nodes.push(Node {
                id,
                kind,
                position: at,
                params: params.into_iter().collect(),
                values: Default::default(),
            });
        });
        self.selected = [id].into();
        id
    }

    /// Delete the selected nodes.
    pub fn delete_selected(&mut self) {
        let ids: Vec<u32> = self.selected.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        self.edit("Delete", false, |g| {
            for id in &ids {
                g.remove_node(*id);
            }
        });
        for id in &ids {
            self.breakpoints.remove(id);
        }
        self.selected.clear();
    }

    /// The selected nodes and the wires between them, as graph JSON.
    pub fn copy_selected(&mut self) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        let g = self.view();
        let part = ScriptGraph {
            nodes: g
                .nodes
                .iter()
                .filter(|n| self.selected.contains(&n.id))
                .cloned()
                .collect(),
            connections: g
                .connections
                .iter()
                .filter(|c| self.selected.contains(&c.from) && self.selected.contains(&c.to))
                .cloned()
                .collect(),
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

    /// Paste copied nodes (or `text` from the system clipboard when it holds graph JSON), a
    /// little further along each time; the pasted nodes are selected.
    pub fn paste(&mut self, text: Option<&str>) {
        let part = text
            .and_then(|t| ScriptGraph::from_json(t).ok())
            .filter(|g| !g.nodes.is_empty())
            .or_else(|| {
                self.clipboard
                    .as_deref()
                    .and_then(|t| ScriptGraph::from_json(t).ok())
            });
        let Some(part) = part else {
            return;
        };
        self.pastes += 1;
        let offset = 30.0 * self.pastes as f32;
        let mut new_ids = Vec::new();
        self.edit("Paste", false, |g| {
            let mut map = HashMap::new();
            for n in &part.nodes {
                let id = g.next_id();
                let mut copy = n.clone();
                copy.id = id;
                copy.position = [n.position[0] + offset, n.position[1] + offset];
                g.nodes.push(copy);
                map.insert(n.id, id);
                new_ids.push(id);
            }
            for c in &part.connections {
                if let (Some(&from), Some(&to)) = (map.get(&c.from), map.get(&c.to)) {
                    g.connections.push(visual_script::Connection {
                        from,
                        output: c.output.clone(),
                        to,
                        input: c.input.clone(),
                    });
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
        self.edit("Group Selection", false, |g| {
            for grp in &mut g.groups {
                grp.nodes.retain(|n| !ids.contains(n));
            }
            g.groups.retain(|grp| !grp.nodes.is_empty());
            g.groups.push(visual_script::Group {
                title: "New Group".into(),
                nodes: ids,
            });
        });
    }

    /// Turn the selected nodes into a macro: they move into a new macro with Graph Input and
    /// Graph Output nodes for the wires that crossed the selection, and a Subgraph node takes
    /// their place.
    pub fn collapse_to_macro(&mut self, root: &std::path::Path) -> Option<String> {
        let ids: BTreeSet<u32> = self.selected.clone();
        if ids.is_empty() {
            return None;
        }
        let files = Self::files(root);
        let view = self.view().clone();
        if view
            .nodes
            .iter()
            .any(|n| ids.contains(&n.id) && visual_script::nodes::is_event(&n.kind))
        {
            return None;
        }
        let mut name = "Macro".to_owned();
        let mut k = 1;
        while view.macro_graph(&name).is_some() {
            k += 1;
            name = format!("Macro {k}");
        }
        let pin_type = |node: u32, pin: &str, output: bool| {
            view.node(node)
                .map(|n| visual_script::ports(n, &view, &files))
                .and_then(|p| {
                    if output {
                        p.output(pin).map(|x| x.ty)
                    } else {
                        p.input(pin).map(|x| x.ty)
                    }
                })
                .unwrap_or(visual_script::PinType::Any)
        };
        let mut inner = ScriptGraph {
            nodes: view
                .nodes
                .iter()
                .filter(|n| ids.contains(&n.id))
                .cloned()
                .collect(),
            connections: view
                .connections
                .iter()
                .filter(|c| ids.contains(&c.from) && ids.contains(&c.to))
                .cloned()
                .collect(),
            variables: view.variables.clone(),
            ..Default::default()
        };
        let left = inner
            .nodes
            .iter()
            .map(|n| n.position[0])
            .fold(f32::MAX, f32::min);
        let right = inner
            .nodes
            .iter()
            .map(|n| n.position[0])
            .fold(f32::MIN, f32::max);
        let top = inner
            .nodes
            .iter()
            .map(|n| n.position[1])
            .fold(f32::MAX, f32::min);
        // Wires into the selection become Graph Input pins, wires out of it Graph Output pins
        // (one pin per inside end).
        let mut ins: Vec<(String, visual_script::PinType)> = Vec::new();
        let mut outs: Vec<(String, visual_script::PinType)> = Vec::new();
        let mut outer = Vec::new();
        let gin = inner.next_id();
        let gout = gin + 1;
        let mut in_names: HashMap<(u32, String), String> = HashMap::new();
        let mut out_names: HashMap<(u32, String), String> = HashMap::new();
        let unique = |list: &[(String, visual_script::PinType)], base: &str| {
            let mut n = base.to_owned();
            let mut i = 2;
            while list.iter().any(|(x, _)| *x == n) {
                n = format!("{base} {i}");
                i += 1;
            }
            n
        };
        for c in &view.connections {
            match (ids.contains(&c.from), ids.contains(&c.to)) {
                (false, true) => {
                    let key = (c.to, c.input.clone());
                    let pin = match in_names.get(&key) {
                        Some(p) => p.clone(),
                        None => {
                            let p = unique(&ins, &c.input);
                            ins.push((p.clone(), pin_type(c.to, &c.input, false)));
                            in_names.insert(key, p.clone());
                            inner.connections.push(visual_script::Connection {
                                from: gin,
                                output: p.clone(),
                                to: c.to,
                                input: c.input.clone(),
                            });
                            p
                        }
                    };
                    outer.push((c.from, c.output.clone(), true, pin));
                }
                (true, false) => {
                    let key = (c.from, c.output.clone());
                    let pin = match out_names.get(&key) {
                        Some(p) => p.clone(),
                        None => {
                            let p = unique(&outs, &c.output);
                            outs.push((p.clone(), pin_type(c.from, &c.output, true)));
                            out_names.insert(key, p.clone());
                            inner.connections.push(visual_script::Connection {
                                from: c.from,
                                output: c.output.clone(),
                                to: gout,
                                input: p.clone(),
                            });
                            p
                        }
                    };
                    outer.push((c.to, c.input.clone(), false, pin));
                }
                _ => {}
            }
        }
        inner.nodes.push(Node {
            id: gin,
            kind: "GraphInput".into(),
            position: [left - 260.0, top],
            params: [("pins".to_owned(), visual_script::nodes::format_pins(&ins))].into(),
            values: Default::default(),
        });
        inner.nodes.push(Node {
            id: gout,
            kind: "GraphOutput".into(),
            position: [right + 260.0, top],
            params: [("pins".to_owned(), visual_script::nodes::format_pins(&outs))].into(),
            values: Default::default(),
        });
        let at = [left, top];
        let macro_name = name.clone();
        let mut new_id = 0;
        self.edit("Collapse To Macro", false, |g| {
            for id in &ids {
                g.remove_node(*id);
            }
            g.macros.push(Macro {
                name: macro_name.clone(),
                graph: inner,
            });
            new_id = g.next_id();
            g.nodes.push(Node {
                id: new_id,
                kind: "Subgraph".into(),
                position: at,
                params: [("macro".to_owned(), macro_name.clone())].into(),
                values: Default::default(),
            });
            for (other, pin, other_is_source, inner_pin) in &outer {
                let c = if *other_is_source {
                    visual_script::Connection {
                        from: *other,
                        output: pin.clone(),
                        to: new_id,
                        input: inner_pin.clone(),
                    }
                } else {
                    visual_script::Connection {
                        from: new_id,
                        output: inner_pin.clone(),
                        to: *other,
                        input: pin.clone(),
                    }
                };
                if !g.connections.contains(&c) {
                    g.connections.push(c);
                }
            }
        });
        self.selected = [new_id].into();
        Some(name)
    }

    /// Toggle a breakpoint on a node of the file's graph.
    pub fn toggle_breakpoint(&mut self, id: u32) {
        if !self.at_root() {
            return;
        }
        if !self.breakpoints.remove(&id) {
            self.breakpoints.insert(id);
        }
    }

    /// Write the graph.
    pub fn save(&mut self, file: &std::path::Path) -> Result<(), String> {
        if self.path.is_none() {
            return Err("no graph open".into());
        }
        self.graph.save(file)?;
        self.saved = self.graph.clone();
        Ok(())
    }

    /// Check the graph when it changed: error badges.
    fn check(&mut self, root: &std::path::Path) {
        if self.checked_version == self.version {
            return;
        }
        self.checked_version = self.version;
        // Subgraph assets may have been saved since: read them again.
        self.canvas.files.clear();
        let files = Self::files(root);
        // A macro is checked on its own (its errors point at its nodes).
        self.errors = compile::check(self.view(), &files);
        if !self.at_root() {
            // Macros alone have no events; errors about a missing Graph Input are expected.
            self.errors.retain(|e| e.node.is_some());
        }
    }

    /// The first problem of a node (compile errors, then run-time errors while playing).
    pub fn error_of(&self, id: u32) -> Option<String> {
        if let Some(e) = self.errors.iter().find(|e| e.node == Some(id)) {
            return Some(e.message.clone());
        }
        if self.at_root() {
            return self.live.as_ref()?.errors.get(&id).cloned();
        }
        None
    }
}

/// Open a graph in the window and bring the tab forward.
pub fn open(s: &mut EditorState, rel: &str) {
    if s.script_graph.path.as_deref() != Some(rel) {
        let file = s.assets.absolute(rel);
        s.script_graph.open(&file, rel);
    }
    s.push(crate::actions::Action::FocusTab(Tab::ScriptGraph));
}

fn save(s: &mut EditorState) {
    let file = s
        .assets
        .absolute(s.script_graph.path.as_deref().unwrap_or_default());
    match s.script_graph.save(&file) {
        Ok(()) => {
            let rel = s.script_graph.path.clone().unwrap_or_default();
            s.console.info(format!("Saved {rel}"));
            s.refresh_assets();
        }
        Err(e) => s
            .console
            .warn(format!("Could not save the Script Graph: {e}")),
    }
}

/// Before a game frame: tell the runtime what to debug (the open graph, its breakpoints, the
/// selected object).
pub fn sync_debugger(s: &mut EditorState) {
    let path = s.script_graph.path.clone();
    let breakpoints = s.script_graph.breakpoints.clone();
    let watch = s.selection.active();
    let Some(rt) = s.runtime.as_mut() else {
        s.script_graph.live = None;
        return;
    };
    let d = rt.graph_debugger();
    d.path = path;
    d.breakpoints = breakpoints.into_iter().collect();
    d.watch = watch;
}

/// After a game frame: read what the runtime reports for the open graph; a flow stopped at a
/// breakpoint pauses Play.
pub fn read_debugger(s: &mut EditorState) {
    let Some(rt) = s.runtime.as_mut() else {
        s.script_graph.live = None;
        return;
    };
    let variables = rt.graph_variables();
    let d = rt.graph_debugger();
    let watched = d.watched.map(|(e, _)| e);
    let live = Live {
        hits: d
            .hits
            .iter()
            .map(|((n, p), f)| ((*n, p.to_string()), *f))
            .collect(),
        frame: d.frame,
        paused: d.paused.as_ref().map(|p| p.node),
        errors: d.errors.clone(),
        variables,
        watched: watched
            .filter(|e| s.scene.contains(*e))
            .map(|e| s.scene.name(e)),
    };
    let newly_paused =
        live.paused.is_some() && s.script_graph.live.as_ref().and_then(|l| l.paused) != live.paused;
    s.script_graph.live = Some(live);
    if newly_paused && s.play == crate::state::PlayState::Playing {
        s.play = crate::state::PlayState::Paused;
        s.console.info(
            "Script Graph breakpoint: Play paused (Step or Continue in the Script Graph window)",
        );
        s.push(crate::actions::Action::FocusTab(Tab::ScriptGraph));
    }
}

/// Step or Continue the paused flow.
pub fn continue_flow(s: &mut EditorState, step: bool) {
    let Some(rt) = s.runtime.as_mut() else {
        return;
    };
    rt.graph_continue(&mut s.scene, step);
    for (kind, text) in rt.take_logs() {
        let level = match kind {
            misty_engine::scripting::LogKind::Info => crate::console::LogLevel::Info,
            misty_engine::scripting::LogKind::Warning => crate::console::LogLevel::Warning,
            misty_engine::scripting::LogKind::Error => crate::console::LogLevel::Error,
        };
        s.console.log(level, text);
    }
    read_debugger(s);
    if !step
        && s.script_graph
            .live
            .as_ref()
            .is_none_or(|l| l.paused.is_none())
    {
        s.play = crate::state::PlayState::Playing;
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        s.script_graph.focused = full.contains(p);
    }
    if s.script_graph.path.is_none() {
        centered(
            ui,
            full,
            "Double-click a Script Graph asset, or use Assets ▸ Create ▸ Visual Script Graph.",
        );
        return;
    }
    if let Some(e) = s.script_graph.load_error.clone() {
        centered(ui, full, &e);
        return;
    }
    if !s.play.in_play_mode() {
        s.script_graph.live = None;
    }
    let busy = interacting(ui.ctx());
    s.script_graph.end_interaction(busy);
    let root = s.project.root.clone();
    s.script_graph.check(&root);

    let bar = Rect::from_min_size(full.min, vec2(full.width(), 26.0));
    toolbar(ui, s, bar);
    let area = Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max);
    canvas::show(ui, s, area);
    let pad = 8.0;
    if s.script_graph.show_blackboard {
        let r = Rect::from_min_size(
            area.min + vec2(pad, pad),
            vec2(
                250.0f32.min(area.width() * 0.35),
                (area.height() - 2.0 * pad).min(blackboard::height(&s.script_graph)),
            ),
        );
        blackboard::show(ui, s, r);
    }
    if s.script_graph.show_inspector {
        let w = 260.0f32.min(area.width() * 0.35);
        let r = Rect::from_min_size(
            pos2(area.right() - w - pad, area.top() + pad),
            vec2(w, (area.height() - 2.0 * pad).min(inspector::height(s))),
        );
        inspector::show(ui, s, r);
    }
    search::show(ui, s);
    shortcuts(ui, s);
}

/// Keys while the window has focus (the editor skips its own bindings for these).
fn shortcuts(ui: &mut Ui, s: &mut EditorState) {
    if !s.script_graph.wants_keys() || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    use egui::{Key, KeyboardShortcut, Modifiers};
    let ctrl = Modifiers::COMMAND;
    let pressed =
        |ui: &mut Ui, m, k| ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
    if pressed(ui, ctrl | Modifiers::SHIFT, Key::Z) || pressed(ui, ctrl, Key::Y) {
        if let Some(l) = s.script_graph.redo() {
            s.console.info(format!("Redo {l}"));
        }
    } else if pressed(ui, ctrl, Key::Z)
        && let Some(l) = s.script_graph.undo()
    {
        s.console.info(format!("Undo {l}"));
    }
    if pressed(ui, ctrl, Key::S) {
        save(s);
    }
    if pressed(ui, ctrl, Key::D) {
        s.script_graph.duplicate_selected();
    }
    if pressed(ui, ctrl, Key::G) {
        s.script_graph.group_selected();
    }
    let events = ui.input(|i| i.events.clone());
    for e in events {
        match e {
            egui::Event::Copy => {
                if let Some(text) = s.script_graph.copy_selected() {
                    ui.ctx().copy_text(text);
                }
            }
            egui::Event::Cut => {
                if let Some(text) = s.script_graph.cut_selected() {
                    ui.ctx().copy_text(text);
                }
            }
            egui::Event::Paste(text) => s.script_graph.paste(Some(&text)),
            _ => {}
        }
    }
    if pressed(ui, Modifiers::NONE, Key::Delete) || pressed(ui, Modifiers::NONE, Key::Backspace) {
        s.script_graph.delete_selected();
    }
    if pressed(ui, Modifiers::NONE, Key::F) {
        s.script_graph.canvas.frame_all = true;
    }
    if pressed(ui, Modifiers::NONE, Key::F9) {
        let ids: Vec<u32> = s.script_graph.selected.iter().copied().collect();
        for id in ids {
            s.script_graph.toggle_breakpoint(id);
        }
    }
    if pressed(ui, Modifiers::NONE, Key::F10) {
        continue_flow(s, true);
    }
    if pressed(ui, Modifiers::NONE, Key::Space)
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
    {
        canvas::open_search_at(s, p, None);
    }
}

fn toolbar(ui: &mut Ui, s: &mut EditorState, bar: Rect) {
    ui.painter().rect_filled(bar, 0.0, theme::TAB_BAR_BG);
    ui.scope_builder(
        UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                let dirty = s.script_graph.dirty();
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
                if ui.button("Show In Project").clicked()
                    && let Some(rel) = s.script_graph.path.clone()
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
                    .add_enabled(s.script_graph.can_undo(), egui::Button::new("⟲"))
                    .on_hover_text("Undo (Ctrl+Z)")
                    .clicked()
                {
                    s.script_graph.undo();
                }
                if ui
                    .add_enabled(s.script_graph.can_redo(), egui::Button::new("⟳"))
                    .on_hover_text("Redo (Ctrl+Y)")
                    .clicked()
                {
                    s.script_graph.redo();
                }
                ui.separator();
                // The path: graph ▸ macro ▸ macro.
                let name = s
                    .script_graph
                    .path
                    .as_deref()
                    .map(misty_engine::scene::clip_name)
                    .unwrap_or_default();
                let label = format!("{ICON} {name}{}", if dirty { " *" } else { "" });
                if s.script_graph.at_root() {
                    ui.label(RichText::new(label).color(theme::TEXT));
                } else {
                    if ui.link(label).clicked() {
                        s.script_graph.leave_macro(0);
                    }
                    let path = s.script_graph.macro_path.clone();
                    for (i, m) in path.iter().enumerate() {
                        ui.label(RichText::new("▸").color(theme::TEXT_DIM));
                        if i + 1 == path.len() {
                            ui.label(RichText::new(format!("Macro: {m}")).color(theme::TEXT));
                        } else if ui.link(format!("Macro: {m}")).clicked() {
                            s.script_graph.leave_macro(i + 1);
                        }
                    }
                }
                match s.script_graph.errors.first() {
                    Some(e) => {
                        let n = s.script_graph.errors.len();
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
                        ui.label(RichText::new("✔ No problems").color(theme::TEXT_DIM));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut s.script_graph.show_inspector, "Graph Inspector");
                    ui.toggle_value(&mut s.script_graph.show_blackboard, "Blackboard");
                    if let Some(live) = s.script_graph.live.clone() {
                        ui.separator();
                        if let Some(node) = live.paused {
                            if ui
                                .button("▶ Continue")
                                .on_hover_text("Let the flow run on and resume Play")
                                .clicked()
                            {
                                continue_flow(s, false);
                            }
                            if ui
                                .button("⤼ Step")
                                .on_hover_text("Run the paused node and stop at the next one (F10)")
                                .clicked()
                            {
                                continue_flow(s, true);
                            }
                            let title = s
                                .script_graph
                                .graph
                                .node(node)
                                .map(|n| visual_script::nodes::title(n, &s.script_graph.graph))
                                .unwrap_or_default();
                            ui.label(
                                RichText::new(format!("⏸ Paused at {title}")).color(theme::WARN),
                            );
                        }
                        let who = live
                            .watched
                            .map_or("no object uses this graph".to_owned(), |n| {
                                format!("Debugging {n}")
                            });
                        ui.label(RichText::new(who).color(theme::TEXT_DIM));
                    }
                });
            });
        },
    );
}

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

/// A new Script Graph asset (Assets ▸ Create ▸ Visual Script Graph): Start and Update events.
pub fn new_graph() -> ScriptGraph {
    ScriptGraph::new_default()
}
