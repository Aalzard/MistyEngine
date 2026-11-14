//! The Nodes palette: every node kind by family, plus the project's custom leaves: Lua scripts
//! that start with `-- @behaviour-leaf Title` (with `-- @param name default` lines for their
//! settings) and the custom events of its Script Graphs. Drag an entry onto the canvas, or click
//! it to add it in the middle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::{Color32, Rect, RichText, Ui, UiBuilder, pos2};
use misty_engine::behaviour_tree::asset::{Category, NodeKind};

use super::canvas::category_color;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::theme;

/// What a palette entry drags onto the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub kind: NodeKind,
    /// Custom leaves show their own name.
    pub custom: Option<String>,
}

impl PaletteItem {
    pub fn title(&self) -> String {
        match &self.custom {
            Some(t) => t.clone(),
            None => tr(self.kind.label()),
        }
    }
}

#[derive(Default)]
pub(crate) struct PaletteState {
    pub search: String,
    scanned: Option<PathBuf>,
    custom: Vec<PaletteItem>,
}

/// The project's custom leaves (scanned once per project; Refresh scans again).
fn custom(s: &mut EditorState) -> Vec<PaletteItem> {
    let root = s.project.root.clone();
    let p = &mut s.behaviour_tree.palette;
    if p.scanned.as_deref() != Some(root.as_path()) {
        p.custom = scan(&root);
        p.scanned = Some(root);
    }
    p.custom.clone()
}

/// Every entry: built-in nodes, then custom leaves.
pub fn items(s: &mut EditorState) -> Vec<PaletteItem> {
    let mut v: Vec<PaletteItem> = NodeKind::palette()
        .into_iter()
        .map(|kind| PaletteItem { kind, custom: None })
        .collect();
    v.extend(custom(s));
    v
}

/// Lua leaves and Script Graph events under `Assets`.
pub fn scan(root: &Path) -> Vec<PaletteItem> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("Assets")];
    let mut files = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    for p in files {
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if ext == "lua" {
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            if let Some((title, params)) = lua_leaf(&text) {
                out.push(PaletteItem {
                    kind: NodeKind::CustomLua {
                        script: rel,
                        params,
                    },
                    custom: Some(title),
                });
            }
        } else if misty_engine::visual_script::is_graph_path(&rel) {
            let Ok(g) = misty_engine::visual_script::ScriptGraph::load(&p) else {
                continue;
            };
            let stem = misty_engine::scene::clip_name(&rel);
            for n in g.nodes.iter().filter(|n| n.kind == "OnCustomEvent") {
                let event = n.param("name").to_owned();
                if event.is_empty() {
                    continue;
                }
                out.push(PaletteItem {
                    custom: Some(format!("{stem} ▸ {event}")),
                    kind: NodeKind::RunScriptGraph {
                        graph: rel.clone(),
                        event,
                        wait: true,
                    },
                });
            }
        }
    }
    out
}

/// A Lua leaf header: `-- @behaviour-leaf Title` then `-- @param name default` lines.
pub fn lua_leaf(text: &str) -> Option<(String, BTreeMap<String, String>)> {
    let mut title = None;
    let mut params = BTreeMap::new();
    for line in text.lines().take(40) {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("--") else {
            if !l.is_empty() {
                break;
            }
            continue;
        };
        let rest = rest.trim();
        if let Some(t) = rest.strip_prefix("@behaviour-leaf") {
            let t = t.trim();
            title = Some(if t.is_empty() {
                "Lua Leaf".into()
            } else {
                t.to_owned()
            });
        } else if let Some(p) = rest.strip_prefix("@param") {
            let mut parts = p.split_whitespace();
            if let Some(name) = parts.next() {
                let value = parts.collect::<Vec<_>>().join(" ");
                params.insert(name.to_owned(), value);
            }
        }
    }
    title.map(|t| (t, params))
}

pub fn height(s: &mut EditorState) -> f32 {
    let n = items(s).len() as f32;
    // Header, search, category labels and rows.
    60.0 + 6.0 * 22.0 + n * 21.0
}

pub fn show(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.painter()
        .rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(40, 41, 45, 240));
    let items = items(s);
    let mut add: Option<PaletteItem> = None;
    let mut refresh = false;
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(6.0)), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr!("Nodes")).strong());
            if ui
                .small_button("⟳")
                .on_hover_text(tr!("Look for custom leaves again"))
                .clicked()
            {
                refresh = true;
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut s.behaviour_tree.palette.search)
                .hint_text(tr!("Search"))
                .desired_width(f32::INFINITY),
        );
        let search = s.behaviour_tree.palette.search.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("bt_palette")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut cats: Vec<Category> = Vec::new();
                for i in &items {
                    if !cats.contains(&i.kind.category()) {
                        cats.push(i.kind.category());
                    }
                }
                for c in cats {
                    let shown: Vec<&PaletteItem> = items
                        .iter()
                        .filter(|i| i.kind.category() == c)
                        .filter(|i| search.is_empty() || i.title().to_lowercase().contains(&search))
                        .collect();
                    if shown.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(tr(c.label())).small().color(theme::TEXT_DIM));
                    for item in shown {
                        let id = ui.id().with(("bt_pal", item.title()));
                        let r = ui
                            .dnd_drag_source(id, item.clone(), |ui| {
                                ui.horizontal(|ui| {
                                    let (dot, _) = ui.allocate_exact_size(
                                        egui::vec2(8.0, 8.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(
                                        dot.center(),
                                        4.0,
                                        category_color(item.kind.category()),
                                    );
                                    ui.label(crate::i18n::display(&item.title()));
                                });
                            })
                            .response;
                        let r = r.on_hover_text(tr(item.kind.help()));
                        if r.clicked() {
                            add = Some(item.clone());
                        }
                    }
                }
            });
    });
    if refresh {
        s.behaviour_tree.palette.scanned = None;
    }
    if let Some(item) = add {
        let w = &mut s.behaviour_tree;
        let at = w
            .canvas
            .view
            .map(|v| v.graph(v.centre))
            .unwrap_or(pos2(0.0, 0.0));
        w.add_node(item.kind, [at.x, at.y], None);
    }
}
