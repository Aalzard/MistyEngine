//! Search popups: Create Node (contexts, operators, Blackboard properties; Space or right-click)
//! and Add Block (blocks the context accepts; its "+ Add Block" footer, or Space over it).
//! Type to filter; arrows + Enter or a click picks.

use egui::{FontId, Key, Pos2, RichText, Ui};
use misty_engine::vfx::{BlockKind, ContextType, EventSource, OperatorKind};

use crate::state::EditorState;
use crate::theme;

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Create a node at a graph point.
    Nodes { graph: Pos2 },
    /// Add a block to a context.
    Blocks { context: u32 },
}

pub struct Search {
    pub screen: Pos2,
    pub mode: Mode,
    pub query: String,
    pub highlighted: usize,
    focus: bool,
}

impl Search {
    pub fn nodes(screen: Pos2, graph: Pos2) -> Self {
        Self {
            screen,
            mode: Mode::Nodes { graph },
            query: String::new(),
            highlighted: 0,
            focus: true,
        }
    }

    pub fn blocks(screen: Pos2, context: u32) -> Self {
        Self {
            screen,
            mode: Mode::Blocks { context },
            query: String::new(),
            highlighted: 0,
            focus: true,
        }
    }
}

#[derive(Clone, Debug)]
pub enum What {
    Context(ContextType),
    /// A GPU Event system listening to the main system.
    EventSystem(EventSource),
    Operator(OperatorKind),
    Block(BlockKind),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub what: What,
    pub title: String,
    pub category: &'static str,
    keywords: &'static str,
}

/// Everything the popup can list right now.
pub fn entries(s: &EditorState) -> Vec<Entry> {
    let Some(search) = &s.effect_graph.search else {
        return Vec::new();
    };
    let g = &s.effect_graph.graph;
    let subgraph = s.effect_graph.is_subgraph();
    match &search.mode {
        Mode::Nodes { .. } => {
            let mut out: Vec<Entry> = Vec::new();
            if subgraph {
                out.push(Entry {
                    what: What::Operator(OperatorKind::SubgraphOutput {
                        name: free_output_name(g),
                    }),
                    title: "Output".into(),
                    category: "Sub-graph",
                    keywords: "subgraph result return",
                });
            } else {
                out.extend(
                    ContextType::ALL
                        .into_iter()
                        .filter(|t| *t != ContextType::GpuEvent)
                        .map(|t| Entry {
                            what: What::Context(t),
                            title: format!("Context: {}", t.label()),
                            category: "Contexts",
                            keywords: "system stack",
                        }),
                );
                out.extend(EventSource::ALL.into_iter().map(|src| Entry {
                    what: What::EventSystem(src),
                    title: format!("GPU Event System ({})", src.label()),
                    category: "Contexts",
                    keywords: "spawn child sub emitter trigger die death collide collision",
                }));
            }
            out.extend(OperatorKind::catalog().into_iter().map(|k| Entry {
                title: k.title(),
                category: k.category(),
                keywords: k.keywords(),
                what: What::Operator(k),
            }));
            let open = s.effect_graph.path.clone().unwrap_or_default();
            for path in &s.assets.subgraphs {
                if *path == open {
                    continue;
                }
                out.push(Entry {
                    what: What::Operator(OperatorKind::Subgraph { path: path.clone() }),
                    title: format!("Sub-graph: {}", misty_engine::vfx::subgraph::title(path)),
                    category: "Sub-graph",
                    keywords: "subgraph operator function",
                });
            }
            for p in &g.properties {
                if p.value.value_type().is_none() {
                    continue;
                }
                out.push(Entry {
                    what: What::Operator(OperatorKind::Property {
                        name: p.name.clone(),
                    }),
                    title: format!("Property: {}", p.name),
                    category: "Properties",
                    keywords: "exposed blackboard",
                });
            }
            out
        }
        Mode::Blocks { context } => {
            let Some(ty) = g.context(*context).map(|c| c.kind.ty()) else {
                return Vec::new();
            };
            BlockKind::catalog()
                .into_iter()
                .filter(|k| k.allowed_in(ty))
                .map(|k| Entry {
                    title: k.title(),
                    category: k.category(),
                    keywords: k.keywords(),
                    what: What::Block(k),
                })
                .collect()
        }
    }
}

/// "Out", "Out 1", … whichever no Output node of the sub-graph uses yet.
fn free_output_name(g: &misty_engine::vfx::EffectGraph) -> String {
    let used = |n: &str| {
        g.operators
            .iter()
            .any(|o| matches!(&o.kind, OperatorKind::SubgraphOutput { name } if name == n))
    };
    let mut name = "Out".to_owned();
    let mut i = 1;
    while used(&name) {
        name = format!("Out {i}");
        i += 1;
    }
    name
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

pub fn filtered(s: &EditorState) -> Vec<Entry> {
    let q = s
        .effect_graph
        .search
        .as_ref()
        .map(|x| x.query.clone())
        .unwrap_or_default();
    entries(s).into_iter().filter(|e| matches(e, &q)).collect()
}

/// Carry out the pick.
pub fn create(s: &mut EditorState, e: &Entry) {
    let Some(search) = s.effect_graph.search.take() else {
        return;
    };
    let w = &mut s.effect_graph;
    let mut new_id = None;
    match (&search.mode, &e.what) {
        (Mode::Nodes { graph }, What::Context(t)) => {
            let t = *t;
            let at = [graph.x, graph.y];
            // A new context joins the system of the nearest context.
            let system = w
                .graph
                .contexts
                .iter()
                .min_by(|a, b| {
                    let d = |c: &&misty_engine::vfx::graph::Context| {
                        (c.position[0] - at[0]).powi(2) + (c.position[1] - at[1]).powi(2)
                    };
                    d(a).total_cmp(&d(b))
                })
                .map_or(0, |c| c.system);
            w.edit("Create Context", false, |g| {
                let id = g.add_context(t, at);
                if let Some(c) = g.context_mut(id) {
                    c.system = system;
                }
                new_id = Some(id);
            });
        }
        (Mode::Nodes { graph }, What::EventSystem(src)) => {
            let (src, at) = (*src, [graph.x, graph.y]);
            w.edit("GPU Event System", false, |g| {
                let sys = g.add_event_system(0, src, at);
                new_id = g.contexts.iter().find(|c| c.system == sys).map(|c| c.id);
            });
        }
        (Mode::Nodes { graph }, What::Operator(k)) => {
            let k = k.clone();
            let at = [graph.x, graph.y];
            w.edit("Create Node", false, |g| {
                new_id = Some(g.add_operator(k, at));
            });
        }
        (Mode::Blocks { context }, What::Block(k)) => {
            let (ctx, k) = (*context, k.clone());
            w.edit("Add Block", false, |g| {
                new_id = g.add_block(ctx, k);
            });
        }
        _ => {}
    }
    if let Some(id) = new_id {
        w.selected = [id].into();
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let Some((at, title)) = s.effect_graph.search.as_ref().map(|x| {
        (
            x.screen,
            match x.mode {
                Mode::Nodes { .. } => "Create Node",
                Mode::Blocks { .. } => "Add Block",
            },
        )
    }) else {
        return;
    };
    let list = filtered(s);
    let mut pick: Option<Entry> = None;
    let mut close = false;
    let area = egui::Area::new(ui.id().with("vfx_search"))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::POPUP_BG)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.set_width(270.0);
                    ui.label(RichText::new(title).strong());
                    let search = s.effect_graph.search.as_mut().expect("open");
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
                        .max_height(320.0)
                        .show(ui, |ui| {
                            let mut last = "";
                            for (i, e) in list.iter().enumerate() {
                                if e.category != last {
                                    last = e.category;
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
                                ui.label(RichText::new("Nothing matches").color(theme::TEXT_DIM));
                            }
                        });
                });
        });
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !area.response.rect.expand(2.0).contains(p))
    });
    if let Some(e) = pick {
        create(s, &e);
    } else if close || outside {
        s.effect_graph.search = None;
    }
}
