//! Create Node: a search popup (Space, right-click ▸ Create Node, or a wire dropped on empty
//! canvas). With no text it shows the nodes by category; typing filters every node. Opened from
//! a dragged pin it lists only nodes with a pin that fits, and wires the new node to it.
//!
//! The Engine categories are generated from the Lua binding table (the one the Lua API
//! reference is made from): a Get / Set node per property and a node per function, so most of
//! the Lua API is available as nodes.

use std::sync::OnceLock;

use egui::{FontId, Key, Pos2, RichText, Ui};
use misty_engine::visual_script::api::{self, Mode};
use misty_engine::visual_script::{self, Node, PinType, ScriptGraph, nodes};

use super::canvas::PinRef;
use crate::state::EditorState;
use crate::theme;

/// One creatable node.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: String,
    pub params: Vec<(String, String)>,
    pub title: String,
    pub category: String,
    pub keywords: String,
}

impl Entry {
    fn probe(&self) -> Node {
        Node {
            id: u32::MAX,
            kind: self.kind.clone(),
            position: [0.0; 2],
            params: self.params.iter().cloned().collect(),
            values: Default::default(),
        }
    }
}

pub struct Search {
    /// Where it opened (screen) and the node goes (graph).
    pub screen: Pos2,
    pub graph: Pos2,
    pub query: String,
    pub from: Option<PinRef>,
    pub highlighted: usize,
    focus: bool,
    /// Project subgraph assets (path, name).
    pub subgraphs: Vec<(String, String)>,
    /// Entries that fit `from`, with the pin to wire (all entries when not from a pin);
    /// computed on the first frame.
    list: Option<Vec<(Entry, Option<String>)>>,
}

impl Search {
    pub fn new(
        screen: Pos2,
        graph: Pos2,
        from: Option<PinRef>,
        subgraphs: Vec<(String, String)>,
    ) -> Self {
        Self {
            screen,
            graph,
            query: String::new(),
            from,
            highlighted: 0,
            focus: true,
            subgraphs,
            list: None,
        }
    }
}

/// Subgraph assets under the project's Assets (a Graph Input or Output node), not `current`.
pub fn project_subgraphs(
    root: &std::path::Path,
    current: Option<&str>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("Assets")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let Ok(rel) = p.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !visual_script::is_graph_path(&rel) || current == Some(rel.as_str()) {
                continue;
            }
            let Ok(g) = ScriptGraph::load(&p) else {
                continue;
            };
            if g.nodes
                .iter()
                .any(|n| n.kind == "GraphInput" || n.kind == "GraphOutput")
            {
                let name = misty_engine::scene::clip_name(&rel);
                out.push((rel, name));
            }
        }
    }
    out.sort();
    out
}

/// `Input.GetKey(name)` → `["name"]`; optional `[, x]` and `...` are dropped.
fn signature_args(sig: &str) -> Vec<String> {
    let Some(open) = sig.find('(') else {
        return Vec::new();
    };
    let close = sig.rfind(')').unwrap_or(sig.len());
    sig[open + 1..close]
        .split(',')
        .map(|a| a.replace(['[', ']'], "").trim().to_owned())
        .filter(|a| !a.is_empty() && a != "..." && a.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .collect()
}

/// Lua globals that aren't game API (Inspector attributes of Lua script classes).
const NOT_NODES: [&str; 5] = ["Header", "HideInInspector", "Range", "Space", "Tooltip"];

/// One node per member of the Lua binding table (and the graph-only engine members).
pub fn api_entries() -> &'static [Entry] {
    static CELL: OnceLock<Vec<Entry>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut out = Vec::new();
        let mut push = |owner: &str, member: &str, mode: Mode, args: &[String], doc: &str| {
            let params = api::params(owner, member, mode, args);
            let probe = Node {
                id: 0,
                kind: "Api".into(),
                position: [0.0; 2],
                params: params.iter().cloned().collect(),
                values: Default::default(),
            };
            out.push(Entry {
                kind: "Api".into(),
                title: api::title(&probe),
                category: api::category(&probe),
                keywords: format!("{member} {}", doc.to_lowercase()),
                params,
            });
        };
        for sec in crate::api_docs::sections() {
            let owner = sec.owner.as_str();
            for e in &sec.entries {
                if e.name.starts_with('_') || (owner.is_empty() && NOT_NODES.contains(&e.name.as_str())) {
                    continue;
                }
                let doc = if e.summary.is_empty() { &e.notes } else { &e.summary };
                match e.kind {
                    crate::api_docs::Kind::Property => {
                        push(owner, &e.name, Mode::Get, &[], doc);
                        if e.settable {
                            push(owner, &e.name, Mode::Set, &[], doc);
                        }
                    }
                    crate::api_docs::Kind::Function => {
                        let args = signature_args(&e.signature);
                        push(owner, &e.name, Mode::Call, &args, doc);
                    }
                }
            }
        }
        for n in api::NATIVE.iter().filter(|n| n.graph_only) {
            push(n.owner, n.member, n.mode, &[], "");
        }
        out
    })
}

/// Every creatable node for graph `g`: built-ins, variables, macros, project subgraphs and
/// the engine API.
pub fn entries(g: &ScriptGraph, subgraphs: &[(String, String)]) -> Vec<Entry> {
    let mut out: Vec<Entry> = nodes::catalog()
        .into_iter()
        .map(|c| Entry {
            kind: c.kind,
            params: c.params,
            title: c.title,
            category: c.category,
            keywords: c.keywords,
        })
        .collect();
    for v in &g.variables {
        for (kind, verb) in [("GetVariable", "Get"), ("SetVariable", "Set")] {
            out.push(Entry {
                kind: kind.into(),
                params: vec![("name".into(), v.name.clone())],
                title: format!("{verb} {}", v.name),
                category: "Variables".into(),
                keywords: format!("variable {}", v.ty.label().to_lowercase()),
            });
        }
    }
    for m in &g.macros {
        out.push(Entry {
            kind: "Subgraph".into(),
            params: vec![("macro".into(), m.name.clone())],
            title: format!("Macro: {}", m.name),
            category: "Subgraphs".into(),
            keywords: "macro subgraph".into(),
        });
    }
    for (path, name) in subgraphs {
        out.push(Entry {
            kind: "Subgraph".into(),
            params: vec![("path".into(), path.clone())],
            title: format!("Subgraph: {name}"),
            category: "Subgraphs".into(),
            keywords: "subgraph asset".into(),
        });
    }
    out.extend(api_entries().iter().cloned());
    out
}

fn matches(e: &Entry, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let hay = format!(
        "{} {} {}",
        e.title.to_lowercase(),
        e.category.to_lowercase(),
        e.keywords
    );
    q.split_whitespace().all(|w| hay.contains(w))
}

/// The pin of a new `e` node that a dragged pin wires to (exact types first).
pub fn fitting_pin(
    g: &ScriptGraph,
    e: &Entry,
    from: &PinRef,
    files: &dyn visual_script::Resolver,
) -> Option<String> {
    let ports = visual_script::ports(&e.probe(), g, files);
    if ports.problem.is_some() && e.kind != "Api" {
        return None;
    }
    let list = if from.output {
        &ports.inputs
    } else {
        &ports.outputs
    };
    let fits = |t: PinType| {
        if from.output {
            from.ty.connects_to(t)
        } else {
            t.connects_to(from.ty)
        }
    };
    list.iter()
        .find(|p| p.ty == from.ty)
        .or_else(|| {
            list.iter()
                .find(|p| fits(p.ty) && p.ty != PinType::Any && from.ty != PinType::Any)
        })
        .or_else(|| list.iter().find(|p| fits(p.ty)))
        .map(|p| p.name.clone())
}

fn ensure_list(s: &mut EditorState) {
    let w = &mut s.script_graph;
    let Some(search) = w.search.as_ref() else {
        return;
    };
    if search.list.is_some() {
        return;
    }
    let g = w.view();
    let all = entries(g, &search.subgraphs);
    let list = match &search.from {
        None => all.into_iter().map(|e| (e, None)).collect(),
        Some(from) => all
            .into_iter()
            .filter_map(|e| {
                let pin = fitting_pin(g, &e, from, &w.canvas.files)?;
                Some((e, Some(pin)))
            })
            .collect(),
    };
    if let Some(search) = w.search.as_mut() {
        search.list = Some(list);
    }
}

/// The entries shown for the current text.
pub fn filtered(s: &mut EditorState) -> Vec<(Entry, Option<String>)> {
    ensure_list(s);
    let Some(search) = &s.script_graph.search else {
        return Vec::new();
    };
    let q = search.query.trim().to_lowercase();
    let mut out: Vec<(Entry, Option<String>)> = search
        .list
        .as_ref()
        .map(|l| {
            l.iter()
                .filter(|(e, _)| matches(e, &search.query))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if !q.is_empty() {
        // Titles holding every word first, then titles holding some, shortest first.
        let words: Vec<&str> = q.split_whitespace().collect();
        out.sort_by_key(|(e, _)| {
            let t = e.title.to_lowercase();
            let hits = words.iter().filter(|w| t.contains(**w)).count();
            let rank = if hits == words.len() {
                0
            } else if hits > 0 {
                1
            } else {
                2
            };
            (rank, !t.starts_with(words[0]), e.title.len())
        });
    }
    out
}

/// Create the node at the search position (wired to the dragged pin).
pub fn create(s: &mut EditorState, e: &Entry, pin: Option<&str>) {
    let Some(search) = s.script_graph.search.take() else {
        return;
    };
    let at = [search.graph.x, search.graph.y];
    let id = s.script_graph.add_node(&e.kind, &e.params, at);
    if let (Some(f), Some(p)) = (&search.from, pin) {
        let root = s.project.root.clone();
        let r = s
            .script_graph
            .connect(&root, (f.node, &f.pin, f.output), (id, p, !f.output));
        if let Err(err) = r {
            s.script_graph.canvas.flash = Some((format!("Can't connect: {err}"), -1.0));
        }
    }
}

/// Category order of the tree.
const ORDER: [&str; 13] = [
    "Events",
    "Flow",
    "Variables",
    "Math",
    "Logic",
    "Comparison",
    "Constants",
    "String",
    "Random",
    "List",
    "Engine",
    "Lua",
    "Subgraphs",
];

fn rank(category: &str) -> usize {
    let top = category.split('/').next().unwrap_or("");
    ORDER.iter().position(|c| *c == top).unwrap_or(ORDER.len())
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let Some(at) = s.script_graph.search.as_ref().map(|x| x.screen) else {
        return;
    };
    let list = filtered(s);
    let mut pick: Option<(Entry, Option<String>)> = None;
    let mut close = false;
    let area = egui::Area::new(ui.id().with("vs_search"))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::POPUP_BG)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.set_width(300.0);
                    let search = s.script_graph.search.as_mut().expect("open");
                    let title = match &search.from {
                        Some(f) => format!("Create Node ({} {})", f.ty.label(), if f.output { "in" } else { "out" }),
                        None => "Create Node".to_owned(),
                    };
                    ui.label(RichText::new(title).strong());
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut search.query)
                            .hint_text("Search nodes and the engine API")
                            .desired_width(f32::INFINITY),
                    );
                    if search.focus {
                        edit.request_focus();
                        search.focus = false;
                    }
                    if edit.changed() {
                        search.highlighted = 0;
                    }
                    let (down, up, enter, esc) = ui.input(|i| {
                        (
                            i.key_pressed(Key::ArrowDown),
                            i.key_pressed(Key::ArrowUp),
                            i.key_pressed(Key::Enter),
                            i.key_pressed(Key::Escape),
                        )
                    });
                    if esc {
                        close = true;
                    }
                    let flat = !search.query.trim().is_empty() || search.from.is_some();
                    ui.add_space(4.0);
                    if flat {
                        if down {
                            search.highlighted =
                                (search.highlighted + 1).min(list.len().saturating_sub(1));
                        }
                        if up {
                            search.highlighted = search.highlighted.saturating_sub(1);
                        }
                        if enter && let Some(e) = list.get(search.highlighted) {
                            pick = Some(e.clone());
                        }
                        let highlighted = search.highlighted;
                        egui::ScrollArea::vertical()
                            .max_height(340.0)
                            .show(ui, |ui| {
                                for (i, e) in list.iter().take(300).enumerate() {
                                    let on = i == highlighted;
                                    let r = ui.selectable_label(
                                        on,
                                        RichText::new(format!("{}   ", e.0.title)),
                                    );
                                    let r = r.on_hover_text(&e.0.category);
                                    if on && (down || up) {
                                        r.scroll_to_me(None);
                                    }
                                    if r.clicked() {
                                        pick = Some(e.clone());
                                    }
                                }
                                if list.len() > 300 {
                                    ui.label(
                                        RichText::new(format!("… {} more: type more", list.len() - 300))
                                            .color(theme::TEXT_DIM),
                                    );
                                }
                                if list.is_empty() {
                                    ui.label(
                                        RichText::new("No matching nodes").color(theme::TEXT_DIM),
                                    );
                                }
                            });
                    } else {
                        if enter && let Some(e) = list.first() {
                            pick = Some(e.clone());
                        }
                        egui::ScrollArea::vertical()
                            .max_height(340.0)
                            .show(ui, |ui| tree(ui, &list, &mut pick));
                    }
                });
        });
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !area.response.rect.expand(2.0).contains(p))
    });
    if let Some((e, pin)) = pick {
        create(s, &e, pin.as_deref());
    } else if close || outside {
        s.script_graph.search = None;
    }
}

/// The nodes by category (Engine and Math in sub-folders).
fn tree(ui: &mut Ui, list: &[(Entry, Option<String>)], pick: &mut Option<(Entry, Option<String>)>) {
    let mut cats: Vec<&str> = list.iter().map(|(e, _)| e.category.as_str()).collect();
    cats.sort_by_key(|c| (rank(c), c.to_owned()));
    cats.dedup();
    let mut tops: Vec<&str> = cats
        .iter()
        .map(|c| c.split('/').next().unwrap_or(""))
        .collect();
    tops.dedup();
    for top in tops {
        egui::CollapsingHeader::new(RichText::new(top).font(FontId::proportional(13.0)))
            .id_salt(("vs_cat", top))
            .show(ui, |ui| {
                for cat in cats.iter().filter(|c| c.split('/').next() == Some(top)) {
                    let items = list.iter().filter(|(e, _)| e.category == *cat);
                    match cat.split_once('/') {
                        Some((_, sub)) => {
                            egui::CollapsingHeader::new(sub)
                                .id_salt(("vs_cat", *cat))
                                .show(ui, |ui| {
                                    for e in items {
                                        if ui.selectable_label(false, &e.0.title).clicked() {
                                            *pick = Some(e.clone());
                                        }
                                    }
                                });
                        }
                        None => {
                            for e in items {
                                if ui.selectable_label(false, &e.0.title).clicked() {
                                    *pick = Some(e.clone());
                                }
                            }
                        }
                    }
                }
            });
    }
}
