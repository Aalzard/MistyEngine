//! Create Node: a search popup (Space, right-click ▸ Create Node, or a connection dropped on empty
//! canvas). Type to filter; arrows + Enter or a click creates the node. Opened from a dragged
//! port it only lists nodes that fit, and connects the new node to that port.

use egui::{FontId, Key, Pos2, RichText, Ui};
use misty_engine::shader_graph::{NodeKind, ValueType};

use super::canvas::PortRef;
use crate::state::EditorState;
use crate::theme;

pub struct Search {
    /// Where it opened (screen) and the node goes (graph).
    pub screen: Pos2,
    pub graph: Pos2,
    pub query: String,
    pub from: Option<PortRef>,
    pub highlighted: usize,
    focus: bool,
}

impl Search {
    pub fn new(screen: Pos2, graph: Pos2, from: Option<PortRef>) -> Self {
        Self {
            screen,
            graph,
            query: String::new(),
            from,
            highlighted: 0,
            focus: true,
        }
    }
}

/// One entry of the list.
#[derive(Clone)]
pub struct Entry {
    pub kind: NodeKind,
    pub title: String,
    pub category: &'static str,
}

/// Every creatable node: the catalog, then one Property node per Blackboard property.
pub fn entries(g: &misty_engine::shader_graph::ShaderGraph) -> Vec<Entry> {
    let mut out: Vec<Entry> = NodeKind::catalog()
        .into_iter()
        .map(|k| Entry {
            title: k.title(),
            category: k.category(),
            kind: k,
        })
        .collect();
    for p in &g.properties {
        out.push(Entry {
            kind: NodeKind::Property {
                name: p.name.clone(),
            },
            title: format!("Property: {}", p.display),
            category: "Properties",
        });
    }
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
        e.kind.keywords()
    );
    q.split_whitespace().all(|w| hay.contains(w))
}

/// The port of a new `kind` node that a dragged port connects to.
pub fn fitting_port(
    g: &misty_engine::shader_graph::ShaderGraph,
    kind: &NodeKind,
    from: &PortRef,
) -> Option<&'static str> {
    let types = g.types();
    if from.output {
        let ty = types.output(from.node, &from.port)?;
        kind.inputs()
            .into_iter()
            .find(|p| ty.converts_to(p.ty.resolve(ValueType::Float)))
            .map(|p| p.name)
    } else {
        let node = g.node(from.node)?;
        let want = node.input(&from.port)?.ty.resolve(types.dynamic(from.node));
        let prop = match kind {
            NodeKind::Property { name } => g.property(name).map(|p| p.ty.value_type()),
            _ => None,
        };
        kind.outputs(prop)
            .into_iter()
            .find(|p| p.ty.resolve(ValueType::Float).converts_to(want))
            .map(|p| p.name)
    }
}

/// The filtered list for the current query and dragged port.
pub fn filtered(s: &EditorState) -> Vec<Entry> {
    let Some(search) = &s.shader_graph.search else {
        return Vec::new();
    };
    let g = &s.shader_graph.graph;
    entries(g)
        .into_iter()
        .filter(|e| matches(e, &search.query))
        .filter(|e| {
            search
                .from
                .as_ref()
                .is_none_or(|f| fitting_port(g, &e.kind, f).is_some())
        })
        .collect()
}

/// Create the entry's node at the search position (connected to the dragged port).
pub fn create(s: &mut EditorState, e: &Entry) {
    let Some(search) = s.shader_graph.search.take() else {
        return;
    };
    let w = &mut s.shader_graph;
    let kind = e.kind.clone();
    let port = search
        .from
        .as_ref()
        .and_then(|f| fitting_port(&w.graph, &kind, f));
    let at = [search.graph.x, search.graph.y];
    let mut id = 0;
    w.edit("Create Node", false, |g| {
        id = g.add_node(kind, at);
        if let (Some(f), Some(p)) = (&search.from, port) {
            let _ = if f.output {
                g.connect(f.node, &f.port, id, p)
            } else {
                g.connect(id, p, f.node, &f.port)
            };
        }
    });
    w.selected = [id].into();
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let Some(at) = s.shader_graph.search.as_ref().map(|x| x.screen) else {
        return;
    };
    let list = filtered(s);
    let mut pick: Option<Entry> = None;
    let mut close = false;
    let area = egui::Area::new(ui.id().with("sg_search"))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::POPUP_BG)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.set_width(250.0);
                    ui.label(RichText::new("Create Node").strong());
                    let search = s.shader_graph.search.as_mut().expect("open");
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut search.query)
                            .hint_text("Search")
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
                    if esc {
                        close = true;
                    }
                    let highlighted = search.highlighted;
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            let mut last_cat = "";
                            for (i, e) in list.iter().enumerate() {
                                if e.category != last_cat {
                                    last_cat = e.category;
                                    ui.label(
                                        RichText::new(e.category)
                                            .font(FontId::proportional(11.0))
                                            .color(theme::TEXT_DIM),
                                    );
                                }
                                let on = i == highlighted;
                                let r = ui.selectable_label(on, format!("  {}", e.title));
                                if on && (down || up) {
                                    r.scroll_to_me(None);
                                }
                                if r.clicked() {
                                    pick = Some(e.clone());
                                }
                            }
                            if list.is_empty() {
                                ui.label(RichText::new("No matching nodes").color(theme::TEXT_DIM));
                            }
                        });
                });
        });
    // A click outside closes it.
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !area.response.rect.expand(2.0).contains(p))
    });
    if let Some(e) = pick {
        create(s, &e);
    } else if close || outside {
        s.shader_graph.search = None;
    }
}
