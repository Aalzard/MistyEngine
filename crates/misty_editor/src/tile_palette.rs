//! Tile Palette tools (Window ▸ 2D ▸ Tile Palette): the active palette, brush and tool, and
//! painting in the Scene view over the active Tilemap's grid with a cell highlight.
//!
//! Tools: Select (drag a rectangle), Move (drag the selection), Paint (Shift erases), Box Fill,
//! Pick (drag over tiles to make them the brush), Erase and Flood Fill. `[` / `]` turn the brush
//! a quarter turn, Shift+`[` / Shift+`]` mirror it. Every stroke is one undo step, recorded
//! just before its first change.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use egui::{Rect, Response, Ui};
use glam::{IVec2, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::tilemap::edit::{self, Brush};
use misty_engine::tilemap::palette::{self, TilePalette};
use misty_engine::tilemap::tile::{self, TileAsset, TileKind};
use misty_engine::tilemap::{self, Grid, Tilemap, TilemapRenderer};

use crate::gizmos::Lines;
use crate::state::EditorState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaletteTool {
    Select,
    Move,
    #[default]
    Paint,
    Box,
    Pick,
    Erase,
    Fill,
}

impl PaletteTool {
    pub const ALL: [Self; 7] = [
        Self::Select,
        Self::Move,
        Self::Paint,
        Self::Box,
        Self::Pick,
        Self::Erase,
        Self::Fill,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            Self::Select => "⬚",
            Self::Move => "✥",
            Self::Paint => "🖌",
            Self::Box => "▣",
            Self::Pick => "💧",
            Self::Erase => "⌫",
            Self::Fill => "▧",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select (S)",
            Self::Move => "Move (M)",
            Self::Paint => "Paint (B)",
            Self::Box => "Box Fill (U)",
            Self::Pick => "Pick (I)",
            Self::Erase => "Erase (D)",
            Self::Fill => "Flood Fill (G)",
        }
    }

    fn key(self) -> egui::Key {
        match self {
            Self::Select => egui::Key::S,
            Self::Move => egui::Key::M,
            Self::Paint => egui::Key::B,
            Self::Box => egui::Key::U,
            Self::Pick => egui::Key::I,
            Self::Erase => egui::Key::D,
            Self::Fill => egui::Key::G,
        }
    }
}

/// A drag in progress over the Scene view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    /// Paint / Erase stroke: the last cell touched and whether its undo step is recorded.
    Stroke { last: IVec2, recorded: bool },
    /// Box Fill, Pick or Select: the corner where it started.
    Rect { start: IVec2 },
    /// Move: where the drag started.
    Move { start: IVec2 },
}

/// Decoded sprite textures for the palette grid (nearest filtering for pixel art).
#[derive(Default)]
pub struct SpriteTextures {
    cache: HashMap<String, (Option<SystemTime>, Option<egui::TextureHandle>)>,
}

impl SpriteTextures {
    pub fn get(&mut self, ctx: &egui::Context, texture: &str) -> Option<egui::TextureHandle> {
        let abs = misty_engine::assets::absolute(texture);
        let modified = misty_engine::file_watch::cached_modified(&abs);
        if let Some((m, t)) = self.cache.get(texture)
            && *m == modified
        {
            return t.clone();
        }
        let tex = misty_engine::sprites::load_image(texture).ok().map(|img| {
            let size = [img.width() as usize, img.height() as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            ctx.load_texture(
                format!("tile-sprite:{texture}"),
                image,
                egui::TextureOptions::NEAREST,
            )
        });
        self.cache
            .insert(texture.to_owned(), (modified, tex.clone()));
        tex
    }
}

pub struct TilePaletteState {
    pub tool: PaletteTool,
    /// Project path of the open `.palette`.
    pub palette: Option<String>,
    /// Its file on disk.
    palette_file: Option<PathBuf>,
    /// The open palette's contents (saved after each change).
    pub data: TilePalette,
    /// Tilemap painted on (None = the selected one, else the first in the scene).
    pub target: Option<Entity>,
    pub brush: Brush,
    /// Palette cell the brush came from (highlighted).
    pub picked_cell: Option<IVec2>,
    /// Scene view cell under the mouse.
    pub hover: Option<(Entity, IVec2)>,
    drag: Option<Drag>,
    /// Select tool rectangle (inclusive cells).
    pub selection: Option<(IVec2, IVec2)>,
    /// Pass number the window was last drawn in (tools act only while it is open).
    pub shown_pass: u64,
    /// Palettes found in the project, rescanned every few seconds.
    pub palettes: Vec<String>,
    scanned: Option<Instant>,
    pub sprites: SpriteTextures,
    /// Name typed for Create New Palette.
    pub new_name: Option<String>,
}

impl Default for TilePaletteState {
    fn default() -> Self {
        Self {
            tool: PaletteTool::Paint,
            palette: None,
            palette_file: None,
            data: TilePalette::default(),
            target: None,
            brush: Brush::default(),
            picked_cell: None,
            hover: None,
            drag: None,
            selection: None,
            shown_pass: 0,
            palettes: Vec::new(),
            scanned: None,
            sprites: SpriteTextures::default(),
            new_name: None,
        }
    }
}

impl TilePaletteState {
    /// Open a palette asset of the project at `root`.
    pub fn open(&mut self, root: &std::path::Path, rel: &str) {
        let abs = root.join(rel);
        self.data = TilePalette::load(&abs).unwrap_or_default();
        self.palette = Some(rel.to_owned());
        self.palette_file = Some(abs);
        self.picked_cell = None;
    }

    /// Write the open palette back to its file.
    pub fn save(&self) -> Result<(), String> {
        match &self.palette_file {
            Some(abs) => self.data.save(abs),
            None => Ok(()),
        }
    }

    /// Project `.palette` files (rescanned at most every 3 seconds, or when `force`).
    pub fn scan(&mut self, root: &std::path::Path, force: bool) {
        if !force
            && self
                .scanned
                .is_some_and(|t| t.elapsed().as_secs_f32() < 3.0)
        {
            return;
        }
        self.scanned = Some(Instant::now());
        self.palettes = files_with(root, palette::EXTENSION);
        if self.palette.is_none()
            && let Some(first) = self.palettes.first().cloned()
        {
            self.open(root, &first);
        }
    }

    /// Use a palette tile as the brush.
    pub fn pick_palette(&mut self, cell: IVec2) {
        if let Some(t) = self.data.get(cell) {
            self.brush = Brush::single(t, Default::default());
            self.picked_cell = Some(cell);
            if matches!(self.tool, PaletteTool::Pick | PaletteTool::Erase) {
                self.tool = PaletteTool::Paint;
            }
        }
    }
}

/// Project-relative paths of files with an extension under `Assets`.
pub fn files_with(root: &std::path::Path, ext: &str) -> Vec<String> {
    fn walk(dir: &std::path::Path, ext: &str, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, ext, out);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("Assets"), ext, &mut out);
    let mut rels: Vec<String> = out.iter().map(|p| crate::asset_ops::rel(root, p)).collect();
    rels.sort();
    rels
}

/// Is the Tile Palette window open (drawn this pass or the previous one)?
pub fn window_open(state: &EditorState, ctx: &egui::Context) -> bool {
    state.tile_palette.shown_pass + 1 >= ctx.cumulative_pass_nr()
}

/// Tilemaps in the scene, in Hierarchy order.
pub fn tilemaps(state: &EditorState) -> Vec<Entity> {
    let mut list: Vec<Entity> = state
        .scene
        .world
        .query::<(Entity, &Tilemap)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    state.scene.sort_hierarchy_order(&mut list);
    list
}

/// The Tilemap the tools paint on: the chosen one, the selected one, else the first.
pub fn active_tilemap(state: &EditorState) -> Option<Entity> {
    let ok = |e: Entity| state.scene.world.get::<&Tilemap>(e).is_ok();
    if let Some(e) = state.tile_palette.target.filter(|e| ok(*e)) {
        return Some(e);
    }
    if let Some(e) = state.selection.active().filter(|e| ok(*e)) {
        return Some(e);
    }
    tilemaps(state).into_iter().next()
}

/// Do the palette tools own the Scene view's left mouse button right now?
pub fn tools_active(state: &EditorState, ctx: &egui::Context) -> bool {
    window_open(state, ctx) && active_tilemap(state).is_some()
}

/// Apply `f` to the active Tilemap's tiles; when it changes something, record an undo step
/// first (once per stroke when `recorded` is given) and mark the scene dirty. Returns the
/// number of cells changed.
fn edit_map(
    state: &mut EditorState,
    e: Entity,
    label: &str,
    recorded: Option<&mut bool>,
    f: impl FnOnce(&mut Tilemap) -> usize,
) -> usize {
    let Ok(mut map) = state.scene.world.get::<&Tilemap>(e).map(|m| (*m).clone()) else {
        return 0;
    };
    let n = f(&mut map);
    if n == 0 {
        return 0;
    }
    let first = match recorded {
        Some(r) => !std::mem::replace(r, true),
        None => true,
    };
    if first {
        state.record(label);
    }
    state.scene_dirty = true;
    let _ = state.scene.world.insert_one(e, map);
    n
}

/// Keyboard: tool shortcuts and `[` `]` rotate / Shift mirror the brush.
pub fn keys(ui: &Ui, state: &mut EditorState) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let (rot_ccw, rot_cw, shift) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::OpenBracket),
            i.key_pressed(egui::Key::CloseBracket),
            i.modifiers.shift,
        )
    });
    let tp = &mut state.tile_palette;
    if rot_ccw {
        tp.brush = if shift {
            tp.brush.flipped(true)
        } else {
            tp.brush.rotated(true)
        };
    }
    if rot_cw {
        tp.brush = if shift {
            tp.brush.flipped(false)
        } else {
            tp.brush.rotated(false)
        };
    }
    for t in PaletteTool::ALL {
        if ui.input(|i| i.modifiers.is_none() && i.key_pressed(t.key())) {
            tp.tool = t;
        }
    }
}

/// Scene view input while the palette tools are active. Returns true when the left button
/// was used (so the click does not select objects).
pub fn scene_view(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    response: &Response,
) -> bool {
    if !tools_active(state, ui.ctx()) {
        state.tile_palette.hover = None;
        state.tile_palette.drag = None;
        return false;
    }
    let Some(e) = active_tilemap(state) else {
        return false;
    };
    let dragging = state.tile_palette.drag.is_some();
    let cell = response
        .hover_pos()
        .filter(|_| response.hovered() || dragging)
        .and_then(|at| {
            let (origin, dir) = crate::picking::ray(camera, rect, at);
            let hit = tilemap::ray_to_plane(&state.scene, e, origin, dir)?;
            Some(tilemap::world_to_cell(&state.scene, e, hit))
        });
    state.tile_palette.hover = cell.map(|c| (e, c));
    if response.hovered() {
        keys(ui, state);
    }
    let (pressed, down, shift, alt) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
            i.modifiers.alt,
        )
    });
    // Alt+drag orbits / the view tools keep the mouse.
    if alt || state.tool == crate::state::Tool::Hand {
        return false;
    }
    let Some(cell) = cell else {
        if !down {
            state.tile_palette.drag = None;
        }
        return false;
    };
    let tool = state.tile_palette.tool;
    let mut used = false;
    if pressed && response.hovered() {
        used = true;
        state.tile_palette.drag = Some(match tool {
            PaletteTool::Paint | PaletteTool::Erase => Drag::Stroke {
                last: cell,
                recorded: false,
            },
            PaletteTool::Move => Drag::Move { start: cell },
            _ => Drag::Rect { start: cell },
        });
        match tool {
            PaletteTool::Paint | PaletteTool::Erase => {
                let mut recorded = false;
                stroke(state, e, cell, cell, tool, shift, &mut recorded);
                state.tile_palette.drag = Some(Drag::Stroke {
                    last: cell,
                    recorded,
                });
            }
            PaletteTool::Fill => {
                let brush = state.tile_palette.brush.clone();
                let first = brush.cells.first().cloned();
                let tile = if shift {
                    None
                } else {
                    first.as_ref().and_then(|c| c.tile.clone())
                };
                let t = first.map(|c| c.transform).unwrap_or_default();
                if tile.is_some() || shift {
                    edit_map(state, e, "Flood Fill Tiles", None, |m| {
                        edit::flood_fill(m, cell, tile.as_deref(), t)
                    });
                }
                state.tile_palette.drag = None;
            }
            _ => {}
        }
    }
    match state.tile_palette.drag {
        Some(Drag::Stroke { last, mut recorded }) if down => {
            used = true;
            if cell != last {
                stroke(state, e, last, cell, tool, shift, &mut recorded);
            }
            state.tile_palette.drag = Some(Drag::Stroke {
                last: cell,
                recorded,
            });
        }
        Some(Drag::Rect { start }) if !down => {
            used = true;
            state.tile_palette.drag = None;
            match tool {
                PaletteTool::Box => {
                    let brush = state.tile_palette.brush.clone();
                    if shift {
                        edit_map(state, e, "Erase Tiles", None, |m| {
                            edit::box_erase(m, start, cell)
                        });
                    } else if !brush.is_empty() {
                        edit_map(state, e, "Box Fill Tiles", None, |m| {
                            edit::box_fill(m, start, cell, &brush)
                        });
                    }
                }
                PaletteTool::Pick => {
                    if let Ok(map) = state.scene.world.get::<&Tilemap>(e) {
                        let b = edit::pick(&map, start, cell);
                        if b.first_tile().is_some() {
                            state.tile_palette.brush = b;
                            state.tile_palette.picked_cell = None;
                        }
                    }
                    state.tile_palette.tool = PaletteTool::Paint;
                }
                PaletteTool::Select => {
                    state.tile_palette.selection = Some(edit::rect(start, cell));
                }
                _ => {}
            }
        }
        Some(Drag::Move { start }) if !down => {
            used = true;
            state.tile_palette.drag = None;
            if let Some((lo, hi)) = state.tile_palette.selection {
                let delta = cell - start;
                if edit_map(state, e, "Move Tiles", None, |m| {
                    edit::move_cells(m, lo, hi, delta)
                }) > 0
                {
                    state.tile_palette.selection = Some((lo + delta, hi + delta));
                }
            }
        }
        Some(_) if !down => state.tile_palette.drag = None,
        Some(_) => used = true,
        None => {}
    }
    used
}

/// Paint (or erase) along a dragged segment.
fn stroke(
    state: &mut EditorState,
    e: Entity,
    from: IVec2,
    to: IVec2,
    tool: PaletteTool,
    shift: bool,
    recorded: &mut bool,
) {
    let brush = state.tile_palette.brush.clone();
    let erase = tool == PaletteTool::Erase || shift;
    if erase {
        let size = if tool == PaletteTool::Erase {
            IVec2::ONE
        } else {
            brush.size()
        };
        edit_map(state, e, "Erase Tiles", Some(recorded), |m| {
            edit::line(from, to)
                .into_iter()
                .map(|c| edit::erase(m, c, size))
                .sum()
        });
    } else if !brush.is_empty() {
        edit_map(state, e, "Paint Tiles", Some(recorded), |m| {
            edit::paint_line(m, from, to, &brush)
        });
    }
}

/// Cell highlight, brush footprint, drag rectangle and selection, plus the active Tilemap's
/// grid around the mouse.
pub fn lines(lines: &mut Lines, state: &EditorState) {
    let Some((e, cell)) = state.tile_palette.hover else {
        if let Some(e) = active_tilemap(state) {
            selection_lines(lines, state, e);
        }
        return;
    };
    let s = &state.scene;
    let faint = [1.0, 1.0, 1.0, 0.12];
    // Nearby grid.
    for dy in -6..=6 {
        for dx in -6..=6 {
            let c = cell + IVec2::new(dx, dy);
            let [a, b, _, d] = tilemap::cell_corners(s, e, c);
            lines.line(a, b, faint);
            lines.line(a, d, faint);
        }
    }
    let tp = &state.tile_palette;
    let (lo, hi) = match tp.drag {
        Some(Drag::Rect { start }) => edit::rect(start, cell),
        _ => match tp.tool {
            PaletteTool::Paint | PaletteTool::Box if !tp.brush.is_empty() => {
                (cell, cell + tp.brush.size() - IVec2::ONE)
            }
            _ => (cell, cell),
        },
    };
    let color = match tp.tool {
        PaletteTool::Erase => [1.0, 0.35, 0.3, 1.0],
        PaletteTool::Select | PaletteTool::Move => [0.4, 0.75, 1.0, 1.0],
        _ => [1.0, 0.85, 0.3, 1.0],
    };
    rect_lines(lines, state, e, lo, hi, color);
    selection_lines(lines, state, e);
}

fn selection_lines(lines: &mut Lines, state: &EditorState, e: Entity) {
    if let Some((lo, hi)) = state.tile_palette.selection {
        rect_lines(lines, state, e, lo, hi, [0.3, 0.6, 1.0, 1.0]);
    }
}

fn rect_lines(
    lines: &mut Lines,
    state: &EditorState,
    e: Entity,
    lo: IVec2,
    hi: IVec2,
    c: [f32; 4],
) {
    let s = &state.scene;
    let a = tilemap::cell_corners(s, e, lo)[0];
    let b = tilemap::cell_corners(s, e, IVec2::new(hi.x, lo.y))[1];
    let t = tilemap::cell_corners(s, e, hi)[2];
    let d = tilemap::cell_corners(s, e, IVec2::new(lo.x, hi.y))[3];
    // Lift towards the camera a little so the outline draws over the tiles.
    let lift = Vec3::new(0.0, 0.0, -0.01);
    lines.line(a + lift, b + lift, c);
    lines.line(b + lift, t + lift, c);
    lines.line(t + lift, d + lift, c);
    lines.line(d + lift, a + lift, c);
}

// ---- creating things ----------------------------------------------------------------------------

/// GameObject ▸ 2D Object ▸ Tilemap ▸ Rectangular / Isometric: a Grid with a child Tilemap
/// (with a Tilemap Renderer), or just the Tilemap under a selected Grid.
pub fn create_tilemap(
    state: &mut EditorState,
    parent: Option<Entity>,
    layout: tilemap::GridLayout,
) -> Entity {
    state.record("Create Tilemap");
    let grid_parent = parent.filter(|p| state.scene.world.get::<&Grid>(*p).is_ok());
    let grid = match grid_parent {
        Some(g) => g,
        None => {
            let g = state.scene.spawn("Grid", parent);
            state.scene.add(
                g,
                Grid {
                    layout,
                    cell_size: match layout {
                        tilemap::GridLayout::Rectangle => glam::Vec2::ONE,
                        tilemap::GridLayout::Isometric => glam::Vec2::new(1.0, 0.5),
                    },
                    ..Default::default()
                },
            );
            g
        }
    };
    let t = state.scene.spawn("Tilemap", Some(grid));
    state.scene.add(t, Tilemap::default());
    state.scene.add(t, TilemapRenderer::default());
    state.select(t);
    state.tile_palette.target = Some(t);
    t
}

/// GameObject ▸ Light ▸ Global / Point / Spot Light 2D.
pub fn create_light_2d(
    state: &mut EditorState,
    parent: Option<Entity>,
    kind: misty_engine::light2d::Light2DType,
) -> Entity {
    use misty_engine::light2d::{Light2D, Light2DType};
    let name = match kind {
        Light2DType::Global => "Global Light 2D",
        Light2DType::Point => "Point Light 2D",
        Light2DType::Spot => "Spot Light 2D",
    };
    state.record(&format!("Create {name}"));
    let e = state.scene.spawn(name, parent);
    let mut light = Light2D {
        kind,
        ..Default::default()
    };
    if kind == Light2DType::Spot {
        light.inner_angle = 40.0;
        light.outer_angle = 70.0;
    }
    state.scene.add(e, light);
    state.select(e);
    e
}

/// What Assets ▸ Create ▸ 2D makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileCreate {
    Tile,
    RuleTile,
    AnimatedTile,
    Palette,
}

impl TileCreate {
    pub const ALL: [Self; 4] = [
        Self::Tile,
        Self::RuleTile,
        Self::AnimatedTile,
        Self::Palette,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tile => "Tile",
            Self::RuleTile => "Rule Tile",
            Self::AnimatedTile => "Animated Tile",
            Self::Palette => "Tile Palette",
        }
    }
}

/// Write a new tile or palette asset in `dir`; returns its path.
pub fn create_asset(dir: &std::path::Path, what: TileCreate) -> Result<PathBuf, String> {
    let (stem, ext) = match what {
        TileCreate::Tile => ("New Tile", tile::EXTENSION),
        TileCreate::RuleTile => ("New Rule Tile", tile::EXTENSION),
        TileCreate::AnimatedTile => ("New Animated Tile", tile::EXTENSION),
        TileCreate::Palette => ("New Palette", palette::EXTENSION),
    };
    let path = crate::asset_ops::unique_path(dir, stem, ext);
    match what {
        TileCreate::Palette => TilePalette::default().save(&path)?,
        _ => {
            let kind = match what {
                TileCreate::RuleTile => TileKind::RuleTile,
                TileCreate::AnimatedTile => TileKind::AnimatedTile,
                _ => TileKind::Tile,
            };
            TileAsset {
                kind,
                ..Default::default()
            }
            .save(&path)?
        }
    }
    Ok(path)
}

/// A sprite (or texture) dropped on the palette: a new `.tile` next to the palette for each
/// sprite (reusing an existing tile of that sprite), added at the next free cells. A `.tile`
/// dropped is added as it is. Returns the tiles added.
pub fn drop_on_palette(state: &mut EditorState, rel: &str, at: Option<IVec2>) -> Vec<String> {
    let root = state.project.root.clone();
    let tiles: Vec<String> = if tile::is_tile(rel) {
        vec![rel.to_owned()]
    } else if misty_engine::assets::is_texture(rel) {
        let existing = files_with(&root, tile::EXTENSION);
        let dir = state
            .tile_palette
            .palette
            .as_deref()
            .and_then(|p| std::path::Path::new(p).parent())
            .map(|p| root.join(p))
            .unwrap_or_else(|| root.join("Assets"));
        misty_engine::sprites::references(rel)
            .into_iter()
            .filter_map(|sprite| {
                let reuse = existing.iter().find(|t| {
                    TileAsset::load(&root.join(t)).is_ok_and(|a| {
                        a.kind == TileKind::Tile && a.sprite.as_deref() == Some(&sprite)
                    })
                });
                if let Some(t) = reuse {
                    return Some(t.clone());
                }
                let name = misty_engine::sprites::display_name(Some(&sprite));
                let path = crate::asset_ops::unique_path(&dir, &name, tile::EXTENSION);
                TileAsset::sprite(&sprite).save(&path).ok()?;
                Some(crate::asset_ops::rel(&root, &path))
            })
            .collect()
    } else {
        Vec::new()
    };
    if tiles.is_empty() {
        return tiles;
    }
    let tp = &mut state.tile_palette;
    for (i, t) in tiles.iter().enumerate() {
        match at {
            Some(c) if tiles.len() == 1 => tp.data.set(c, Some(t)),
            Some(c) => tp.data.set(
                c + IVec2::new(i as i32 % palette::COLUMNS, i as i32 / palette::COLUMNS),
                Some(t),
            ),
            None => {
                tp.data.add(t);
            }
        }
    }
    if let Err(e) = tp.save() {
        state
            .console
            .warn(format!("Could not save the palette: {e}"));
    }
    state.refresh_assets();
    tiles
}

#[cfg(test)]
mod tests;

/// Gizmos of selected 2D lights: inner and outer radius (and the spot cone).
pub fn light_2d_lines(lines: &mut Lines, state: &EditorState) {
    use misty_engine::light2d::{Light2D, Light2DType};
    for &e in state.selection.list() {
        let Ok(l) = state.scene.world.get::<&Light2D>(e) else {
            continue;
        };
        if l.kind == Light2DType::Global {
            continue;
        }
        let m = state.scene.world_matrix(e);
        let center = m.transform_point3(Vec3::ZERO);
        let up = m
            .transform_vector3(Vec3::Y)
            .truncate()
            .normalize_or(glam::Vec2::Y);
        let color = [l.color[0], l.color[1], l.color[2], 0.9];
        let faint = [l.color[0], l.color[1], l.color[2], 0.45];
        let half = if l.kind == Light2DType::Spot {
            (l.outer_angle.clamp(0.0, 360.0) * 0.5).to_radians()
        } else {
            std::f32::consts::PI
        };
        let base = up.y.atan2(up.x);
        const N: usize = 64;
        for (r, c) in [(l.outer_radius, color), (l.inner_radius, faint)] {
            if r <= 0.01 {
                continue;
            }
            let at = |a: f32| center + Vec3::new(a.cos(), a.sin(), 0.0) * r;
            for i in 0..N {
                let a0 = base - half + i as f32 / N as f32 * half * 2.0;
                let a1 = base - half + (i + 1) as f32 / N as f32 * half * 2.0;
                lines.line(at(a0), at(a1), c);
            }
            if half < std::f32::consts::PI {
                lines.line(center, at(base - half), c);
                lines.line(center, at(base + half), c);
            }
        }
    }
}
