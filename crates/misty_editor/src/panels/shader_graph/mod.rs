//! Shader Graph window (double-click a `.shadergraph`): the node canvas, the Blackboard of
//! exposed properties, Graph Settings (target), the main preview and Save Asset.
//!
//! The window edits a working copy of the graph with its own undo/redo; Save Asset writes it and
//! reloads the shader for every material using it. While editing, the generated shader goes to a
//! private preview path, so the main preview shows unsaved changes without touching the scene.

mod blackboard;
pub(crate) mod canvas;
mod previews;
mod search;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use misty_engine::assets::Material;
use misty_engine::shader_asset;
use misty_engine::shader_graph::{GraphError, ShaderGraph, Surface, TargetMaterial, generate};

use crate::layout::Tab;
use crate::material_preview::PreviewShape;
use crate::state::EditorState;
use crate::theme;

/// Icon of Shader Graph assets (Project window, tab title).
pub const ICON: &str = "◈";
/// Undo steps kept.
const UNDO_LIMIT: usize = 100;

/// A graph edit kept for undo.
struct Snapshot {
    label: String,
    graph: ShaderGraph,
}

/// Shader Graph window state (kept while the tab is closed).
pub struct ShaderGraphWindow {
    /// Graph shown (project-relative path).
    pub path: Option<String>,
    /// The working copy.
    pub graph: ShaderGraph,
    /// What is on disk (dirty = differs).
    saved: ShaderGraph,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Label of the edit grouped with the current drag / typing session.
    grouping: Option<String>,
    /// Canvas scroll (graph units) and zoom (0 = 1).
    pub pan: Vec2,
    pub zoom: f32,
    pub selected: BTreeSet<u32>,
    /// Last click landed in this window (Delete, Space and Ctrl+Z/Y/S act on the graph).
    pub focused: bool,
    pub(crate) canvas: canvas::CanvasState,
    pub(crate) search: Option<search::Search>,
    pub(crate) blackboard: blackboard::BlackboardState,
    previews: previews::NodePreviews,
    /// Result of the last compile (None = fine).
    pub error: Option<GraphError>,
    /// Content of the last compile (positions excluded).
    compiled_key: Option<u64>,
    /// Edits since opening (drives recompiles).
    version: u64,
    checked_version: u64,
    /// The graph does not load.
    pub load_error: Option<String>,
    pub show_blackboard: bool,
    pub show_settings: bool,
    pub show_preview: bool,
    pub preview_shape: PreviewShape,
    pub preview_yaw: f32,
    pub preview_pitch: f32,
}

impl Default for ShaderGraphWindow {
    fn default() -> Self {
        Self {
            path: None,
            graph: ShaderGraph::default(),
            saved: ShaderGraph::default(),
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
            previews: Default::default(),
            error: None,
            compiled_key: None,
            version: 1,
            checked_version: 0,
            load_error: None,
            show_blackboard: true,
            show_settings: true,
            show_preview: true,
            preview_shape: PreviewShape::Sphere,
            preview_yaw: 0.0,
            preview_pitch: 0.0,
        }
    }
}

/// Is the user mid-interaction (dragging, or typing in a field)?
pub fn interacting(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()
}

/// Content of a graph that affects the shader and the previews (not node positions).
fn content_key(g: &ShaderGraph) -> u64 {
    let mut c = g.clone();
    for n in &mut c.nodes {
        n.position = [0.0; 2];
    }
    c.notes.clear();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    c.to_json().hash(&mut h);
    h.finish()
}

impl ShaderGraphWindow {
    /// Load a graph from disk (replacing the working copy).
    pub fn open(&mut self, file: &std::path::Path, rel: &str) {
        let loaded = ShaderGraph::load(file);
        *self = Self {
            show_blackboard: self.show_blackboard,
            show_settings: self.show_settings,
            show_preview: self.show_preview,
            preview_shape: self.preview_shape,
            ..Default::default()
        };
        self.path = Some(rel.to_owned());
        match loaded {
            Ok(g) => {
                self.saved = g.clone();
                self.graph = g;
                self.frame_all_later();
            }
            Err(e) => self.load_error = Some(e),
        }
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

    fn frame_all_later(&mut self) {
        self.canvas.frame_all = true;
    }

    /// Change the graph with undo. Edits during one interaction with the same label share a
    /// single undo step.
    pub fn edit(&mut self, label: &str, interacting: bool, f: impl FnOnce(&mut ShaderGraph)) {
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

    /// Undo the last edit; returns its label.
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
        self.selected.retain(|id| self.graph.node(*id).is_some());
    }

    /// Delete the selected nodes (not the master stack).
    pub fn delete_selected(&mut self) {
        let ids: Vec<u32> = self.selected.iter().copied().collect();
        if ids
            .iter()
            .all(|&id| id == misty_engine::shader_graph::MASTER)
        {
            return;
        }
        self.edit("Delete Nodes", false, |g| {
            for id in &ids {
                g.remove_node(*id);
            }
        });
        self.selected.clear();
    }

    /// Duplicate the selected nodes (and the connections between them), offset a little.
    pub fn duplicate_selected(&mut self) {
        let ids: Vec<u32> = self
            .selected
            .iter()
            .copied()
            .filter(|&id| id != misty_engine::shader_graph::MASTER)
            .collect();
        if ids.is_empty() {
            return;
        }
        let mut new_ids = Vec::new();
        self.edit("Duplicate Nodes", false, |g| {
            let mut map = std::collections::HashMap::new();
            for id in &ids {
                let Some(n) = g.node(*id).cloned() else {
                    continue;
                };
                let new = g.add_node(n.kind.clone(), [n.position[0] + 30.0, n.position[1] + 30.0]);
                if let Some(m) = g.node_mut(new) {
                    m.values = n.values.clone();
                    m.hide_preview = n.hide_preview;
                }
                map.insert(*id, new);
                new_ids.push(new);
            }
            let inner: Vec<_> = g
                .connections
                .iter()
                .filter(|c| map.contains_key(&c.from) && map.contains_key(&c.to))
                .cloned()
                .collect();
            for c in inner {
                let _ = g.connect(map[&c.from], &c.output, map[&c.to], &c.input);
            }
        });
        self.selected = new_ids.into_iter().collect();
    }

    /// Write the graph and reload the shader for the materials using it.
    pub fn save(&mut self, file: &std::path::Path) -> Result<(), String> {
        let Some(rel) = self.path.clone() else {
            return Err("no graph open".into());
        };
        self.graph.save(file)?;
        self.saved = self.graph.clone();
        shader_asset::reload(&rel);
        Ok(())
    }

    /// Recompile when the graph's content changed: the error badge, the main preview's shader
    /// and the node previews.
    fn check(&mut self, ctx: &egui::Context) {
        if self.checked_version == self.version {
            self.previews.animate(ctx, &self.graph);
            return;
        }
        self.checked_version = self.version;
        let key = content_key(&self.graph);
        if self.compiled_key != Some(key) {
            self.compiled_key = Some(key);
            self.error = misty_engine::shader_graph::compile(&self.graph).err();
            if let (Some(rel), Ok(cg)) = (&self.path, generate(&self.graph)) {
                shader_asset::set_source(&preview_path(rel), &cg.source);
            }
            self.previews.invalidate();
        }
        self.previews.update(ctx, &self.graph);
    }

    /// The material drawn in the main preview: the graph's working copy with its defaults.
    pub fn preview_material(&self) -> Option<Material> {
        let rel = self.path.as_ref()?;
        Some(Material {
            shader: preview_path(rel),
            properties: self.graph.default_values(),
            ..Default::default()
        })
    }
}

/// Private shader path the working copy compiles under (never a file).
pub fn preview_path(rel: &str) -> String {
    let stem = std::path::Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "Library/ShaderGraphPreview/{stem}.{}",
        misty_engine::shader_graph::EXTENSION
    )
}

/// Open a graph in the window and bring the tab forward.
pub fn open(s: &mut EditorState, rel: &str) {
    if s.shader_graph.path.as_deref() != Some(rel) {
        let file = s.assets.absolute(rel);
        s.shader_graph.open(&file, rel);
    }
    s.push(crate::actions::Action::FocusTab(Tab::ShaderGraph));
}

fn save(s: &mut EditorState) {
    let file = s
        .assets
        .absolute(s.shader_graph.path.as_deref().unwrap_or_default());
    match s.shader_graph.save(&file) {
        Ok(()) => {
            let rel = s.shader_graph.path.clone().unwrap_or_default();
            s.console.info(format!("Saved {rel}"));
            s.refresh_assets();
        }
        Err(e) => s
            .console
            .warn(format!("Could not save the Shader Graph: {e}")),
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        s.shader_graph.focused = full.contains(p);
    }
    if s.shader_graph.path.is_none() {
        centered(
            ui,
            full,
            "Double-click a Shader Graph asset, or use Assets ▸ Create ▸ Shader Graph.",
        );
        return;
    }
    if let Some(e) = s.shader_graph.load_error.clone() {
        centered(ui, full, &e);
        return;
    }
    let busy = interacting(ui.ctx());
    s.shader_graph.end_interaction(busy);
    s.shader_graph.check(ui.ctx());

    let bar = Rect::from_min_size(full.min, vec2(full.width(), 26.0));
    toolbar(ui, s, bar);
    let area = Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max);
    canvas::show(ui, s, area);

    // Floating panels over the canvas.
    let pad = 8.0;
    if s.shader_graph.show_blackboard {
        let r = Rect::from_min_size(
            area.min + vec2(pad, pad),
            vec2(
                230.0f32.min(area.width() * 0.4),
                (area.height() - 2.0 * pad).min(blackboard::height(&s.shader_graph)),
            ),
        );
        blackboard::show(ui, s, r);
    }
    let right_w = 230.0f32.min(area.width() * 0.35);
    let mut top = area.top() + pad;
    if s.shader_graph.show_settings {
        let r = Rect::from_min_size(
            pos2(area.right() - right_w - pad, top),
            vec2(right_w, 150.0),
        );
        settings_panel(ui, s, r);
        top = r.bottom() + pad;
    }
    if s.shader_graph.show_preview {
        let h = (right_w + 30.0).min(area.bottom() - top - pad);
        if h > 80.0 {
            let r = Rect::from_min_size(
                pos2(area.right() - right_w - pad, area.bottom() - h - pad),
                vec2(right_w, h),
            );
            main_preview(ui, s, r);
        }
    }
    search::show(ui, s);
    shortcuts(ui, s);
}

/// Keys while the window has focus (the editor skips its own bindings for these).
fn shortcuts(ui: &mut Ui, s: &mut EditorState) {
    if !s.shader_graph.wants_keys() || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    use egui::{Key, KeyboardShortcut, Modifiers};
    let ctrl = Modifiers::COMMAND;
    let pressed =
        |ui: &mut Ui, m, k| ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
    if pressed(ui, ctrl | Modifiers::SHIFT, Key::Z) || pressed(ui, ctrl, Key::Y) {
        if let Some(l) = s.shader_graph.redo() {
            s.console.info(format!("Redo {l}"));
        }
    } else if pressed(ui, ctrl, Key::Z)
        && let Some(l) = s.shader_graph.undo()
    {
        s.console.info(format!("Undo {l}"));
    }
    if pressed(ui, ctrl, Key::S) {
        save(s);
    }
    if pressed(ui, ctrl, Key::D) {
        s.shader_graph.duplicate_selected();
    }
    if pressed(ui, Modifiers::NONE, Key::Delete) || pressed(ui, Modifiers::NONE, Key::Backspace) {
        s.shader_graph.delete_selected();
    }
    if pressed(ui, Modifiers::NONE, Key::F) {
        s.shader_graph.canvas.frame_all = true;
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
                let dirty = s.shader_graph.dirty();
                let save_text = if dirty {
                    RichText::new("Save Asset").strong()
                } else {
                    RichText::new("Save Asset")
                };
                if ui
                    .add_enabled(dirty, egui::Button::new(save_text))
                    .on_hover_text("Write the graph (Ctrl+S)")
                    .clicked()
                {
                    save(s);
                }
                if ui.button("Show In Project").clicked()
                    && let Some(rel) = s.shader_graph.path.clone()
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
                let undo = s.shader_graph.can_undo();
                if ui
                    .add_enabled(undo, egui::Button::new("⟲"))
                    .on_hover_text("Undo (Ctrl+Z)")
                    .clicked()
                {
                    s.shader_graph.undo();
                }
                let redo = s.shader_graph.can_redo();
                if ui
                    .add_enabled(redo, egui::Button::new("⟳"))
                    .on_hover_text("Redo (Ctrl+Y)")
                    .clicked()
                {
                    s.shader_graph.redo();
                }
                ui.separator();
                let name = s
                    .shader_graph
                    .path
                    .as_deref()
                    .map(misty_engine::scene::clip_name)
                    .unwrap_or_default();
                ui.label(
                    RichText::new(format!("{ICON} {name}{}", if dirty { " *" } else { "" }))
                        .color(theme::TEXT),
                );
                match &s.shader_graph.error {
                    Some(e) => {
                        ui.label(RichText::new(format!("⚠ {}", short(&e.message))).color(ERROR))
                            .on_hover_text(&e.message);
                    }
                    None => {
                        ui.label(RichText::new("✔ Compiled").color(theme::TEXT_DIM));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let w = &mut s.shader_graph;
                    ui.toggle_value(&mut w.show_preview, "Main Preview");
                    ui.toggle_value(&mut w.show_settings, "Graph Settings");
                    ui.toggle_value(&mut w.show_blackboard, "Blackboard");
                });
            });
        },
    );
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

/// A floating card (rounded, soft shadow) with a title row; returns the content rect.
pub(crate) fn card(ui: &Ui, rect: Rect, title: &str) -> Rect {
    let p = ui.painter();
    p.rect_filled(
        rect.translate(vec2(0.0, 3.0)),
        8.0,
        Color32::from_black_alpha(60),
    );
    p.rect_filled(rect, 8.0, theme::CARD_BG.gamma_multiply(0.97));
    p.text(
        rect.min + vec2(10.0, 8.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(13.0),
        theme::TEXT,
    );
    Rect::from_min_max(rect.min + vec2(8.0, 28.0), rect.max - vec2(8.0, 8.0))
}

fn settings_panel(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.interact(
        rect,
        ui.id().with("sg_settings_bg"),
        Sense::click_and_drag(),
    );
    let inner = card(ui, rect, "Graph Settings");
    let mut t = s.shader_graph.graph.target;
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        egui::Grid::new("sg_settings")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Material");
                egui::ComboBox::from_id_salt("sg_material")
                    .selected_text(match t.material {
                        TargetMaterial::Lit => "Lit",
                        TargetMaterial::Unlit => "Unlit",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.material, TargetMaterial::Lit, "Lit");
                        ui.selectable_value(&mut t.material, TargetMaterial::Unlit, "Unlit");
                    });
                ui.end_row();
                ui.label("Surface");
                egui::ComboBox::from_id_salt("sg_surface")
                    .selected_text(match t.surface {
                        Surface::Opaque => "Opaque",
                        Surface::Transparent => "Transparent",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.surface, Surface::Opaque, "Opaque");
                        ui.selectable_value(&mut t.surface, Surface::Transparent, "Transparent");
                    });
                ui.end_row();
                ui.label("Alpha Clipping");
                ui.checkbox(&mut t.alpha_clip, "");
                ui.end_row();
                ui.label("Two Sided");
                ui.checkbox(&mut t.two_sided, "");
                ui.end_row();
            });
    });
    if t != s.shader_graph.graph.target {
        s.shader_graph
            .edit("Graph Settings", false, |g| g.target = t);
    }
}

fn main_preview(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    let inner = card(ui, rect, "Main Preview");
    let image = Rect::from_min_max(inner.min, inner.max - vec2(0.0, 24.0));
    let resp = ui.interact(image, ui.id().with("sg_main_preview"), Sense::drag());
    if resp.dragged() {
        let d = resp.drag_delta();
        let w = &mut s.shader_graph;
        w.preview_yaw -= d.x * 0.6;
        w.preview_pitch = (w.preview_pitch - d.y * 0.6).clamp(-89.0, 89.0);
    }
    if let Some(m) = s.shader_graph.preview_material() {
        let size = [
            (image.width() * ui.ctx().pixels_per_point()) as u32,
            (image.height() * ui.ctx().pixels_per_point()) as u32,
        ];
        let w = &s.shader_graph;
        let animated = misty_engine::shader_graph::eval::uses_time(&w.graph);
        let id = s.project.thumbnails.materials.graph_preview(
            &m,
            size,
            w.preview_yaw,
            w.preview_pitch,
            w.preview_shape,
            animated,
        );
        match id {
            Some(id) => {
                let r = image.shrink(2.0);
                ui.painter().image(
                    id,
                    r,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                ui.painter().text(
                    image.center(),
                    Align2::CENTER_CENTER,
                    "Rendering…",
                    FontId::proportional(12.0),
                    theme::TEXT_DIM,
                );
            }
        }
        if animated {
            ui.ctx().request_repaint();
        }
    }
    let bar = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 22.0), inner.max);
    ui.scope_builder(UiBuilder::new().max_rect(bar), |ui| {
        ui.horizontal(|ui| {
            for (shape, label) in [
                (PreviewShape::Sphere, "Sphere"),
                (PreviewShape::Cube, "Cube"),
                (PreviewShape::Plane, "Plane"),
            ] {
                let on = s.shader_graph.preview_shape == shape;
                if ui.selectable_label(on, label).clicked() {
                    s.shader_graph.preview_shape = shape;
                }
            }
        });
    });
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

/// Write a new graph asset (Assets ▸ Create ▸ Shader Graph): a starter graph with a Color
/// property feeding Base Color.
pub fn new_graph(material: TargetMaterial) -> ShaderGraph {
    use misty_engine::shader_graph::{NodeKind, PropertyType, master};
    let mut g = ShaderGraph::new(material);
    let name = g.add_property("Base Color", PropertyType::Color);
    let n = g.add_node(NodeKind::Property { name }, [0.0, 0.0]);
    let _ = g.connect(
        n,
        "Out",
        misty_engine::shader_graph::MASTER,
        master::BASE_COLOR,
    );
    g
}
