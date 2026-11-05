//! Tile Palette window (Window ▸ 2D ▸ Tile Palette): the active Tilemap, the painting tools,
//! the palette asset and its grid of tiles. Drop sprites, textures or `.tile` assets on the grid
//! to add tiles (textures make one tile per sprite); click a tile to paint with it; right-click
//! removes it from the palette.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use glam::IVec2;
use misty_engine::tilemap::palette::{self, TilePalette};
use misty_engine::tilemap::{TileTransform, tile};

use crate::panels::AssetPayload;
use crate::state::EditorState;
use crate::tile_palette::PaletteTool;

pub const ICON: &str = "▦";
/// Size of a palette cell in points.
const CELL: f32 = 44.0;

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    state.tile_palette.shown_pass = ui.ctx().cumulative_pass_nr();
    let root = state.project.root.clone();
    state.tile_palette.scan(&root, false);
    ui.add_space(2.0);
    target_row(ui, state);
    ui.separator();
    tools_row(ui, state);
    ui.separator();
    palette_row(ui, state, &root);
    ui.add_space(4.0);
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .max_height((ui.available_height() - 52.0).max(60.0))
        .show(ui, |ui| palette_grid(ui, state));
    ui.separator();
    brush_row(ui, state);
}

fn target_row(ui: &mut Ui, state: &mut EditorState) {
    let maps = crate::tile_palette::tilemaps(state);
    let active = crate::tile_palette::active_tilemap(state);
    let names: Vec<String> = maps.iter().map(|e| state.scene.name(*e)).collect();
    let shown = active.map_or("None".to_owned(), |e| state.scene.name(e));
    ui.horizontal(|ui| {
        ui.label("Active Tilemap");
        let mut pick = None;
        egui::ComboBox::from_id_salt("tile_palette_target")
            .selected_text(shown)
            .width(180.0)
            .show_ui(ui, |ui| {
                for (e, n) in maps.iter().zip(&names) {
                    if ui.selectable_label(active == Some(*e), n).clicked() {
                        pick = Some(*e);
                    }
                }
            });
        if let Some(e) = pick {
            state.tile_palette.target = Some(e);
        }
        if maps.is_empty() {
            ui.weak("No Tilemap: GameObject ▸ 2D Object ▸ Tilemap");
            if ui.button("Create Tilemap").clicked() {
                crate::tile_palette::create_tilemap(
                    state,
                    None,
                    misty_engine::tilemap::GridLayout::Rectangle,
                );
            }
        }
    });
}

fn tools_row(ui: &mut Ui, state: &mut EditorState) {
    ui.horizontal(|ui| {
        for t in PaletteTool::ALL {
            let on = state.tile_palette.tool == t;
            let b = egui::Button::new(egui::RichText::new(t.icon()).size(16.0))
                .selected(on)
                .min_size(vec2(30.0, 26.0));
            if ui.add(b).on_hover_text(t.label()).clicked() {
                state.tile_palette.tool = t;
            }
        }
        ui.separator();
        let tp = &mut state.tile_palette;
        if ui
            .button("⟲")
            .on_hover_text("Rotate counter-clockwise ([)")
            .clicked()
        {
            tp.brush = tp.brush.rotated(true);
        }
        if ui
            .button("⟳")
            .on_hover_text("Rotate clockwise (])")
            .clicked()
        {
            tp.brush = tp.brush.rotated(false);
        }
        if ui.button("⇋").on_hover_text("Flip X (Shift+[)").clicked() {
            tp.brush = tp.brush.flipped(true);
        }
        if ui.button("⇵").on_hover_text("Flip Y (Shift+])").clicked() {
            tp.brush = tp.brush.flipped(false);
        }
    });
}

fn palette_row(ui: &mut Ui, state: &mut EditorState, root: &std::path::Path) {
    ui.horizontal(|ui| {
        let current = state.tile_palette.palette.clone();
        let label = current
            .as_deref()
            .map(stem)
            .unwrap_or_else(|| "No Palette".into());
        let mut open = None;
        egui::ComboBox::from_id_salt("tile_palette_asset")
            .selected_text(format!("{ICON} {label}"))
            .width(180.0)
            .show_ui(ui, |ui| {
                for p in &state.tile_palette.palettes {
                    if ui
                        .selectable_label(current.as_deref() == Some(p), stem(p))
                        .on_hover_text(p)
                        .clicked()
                    {
                        open = Some(p.clone());
                    }
                }
                ui.separator();
                if ui.button("Create New Palette").clicked() {
                    state.tile_palette.new_name = Some("New Palette".into());
                }
            });
        if let Some(p) = open {
            state.tile_palette.open(root, &p);
        }
        if state.tile_palette.palette.is_none()
            && state.tile_palette.new_name.is_none()
            && ui.button("Create New Palette").clicked()
        {
            state.tile_palette.new_name = Some("New Palette".into());
        }
        if let Some(mut name) = state.tile_palette.new_name.take() {
            let edit = ui.text_edit_singleline(&mut name);
            let create = ui.button("Create").clicked()
                || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if create {
                let dir = root.join("Assets/Palettes");
                let _ = std::fs::create_dir_all(&dir);
                let path = crate::asset_ops::unique_path(&dir, name.trim(), palette::EXTENSION);
                match TilePalette::default().save(&path) {
                    Ok(()) => {
                        let rel = crate::asset_ops::rel(root, &path);
                        state.tile_palette.open(root, &rel);
                        state.tile_palette.scan(root, true);
                        state.refresh_assets();
                    }
                    Err(e) => state
                        .console
                        .warn(format!("Could not create the palette: {e}")),
                }
            } else if !ui.button("Cancel").clicked() {
                state.tile_palette.new_name = Some(name);
            }
        }
    });
}

fn stem(p: &str) -> String {
    std::path::Path::new(p)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Draw a sprite into `rect` turned by `t` (corners mapped like the tile quad).
pub fn paint_sprite(
    ui: &Ui,
    textures: &mut crate::tile_palette::SpriteTextures,
    rect: Rect,
    sprite: &str,
    t: TileTransform,
    tint: Color32,
) {
    let Some(s) = misty_engine::sprites::resolve(sprite) else {
        return;
    };
    let Some(tex) = textures.get(ui.ctx(), &s.texture) else {
        return;
    };
    let (lo, hi) = s.uv_rect();
    // egui UVs run down; sprite rects run up.
    let uv = [
        pos2(lo.x, 1.0 - lo.y),
        pos2(hi.x, 1.0 - lo.y),
        pos2(hi.x, 1.0 - hi.y),
        pos2(lo.x, 1.0 - hi.y),
    ];
    // Fit the sprite's aspect inside the cell.
    let size = s.size();
    let k = (rect.width() / size.x.max(1e-4)).min(rect.height() / size.y.max(1e-4));
    let half = glam::Vec2::new(size.x, size.y) * k * 0.5;
    let m = t.matrix();
    let c = rect.center();
    let corners = [
        glam::Vec2::new(-half.x, -half.y),
        glam::Vec2::new(half.x, -half.y),
        glam::Vec2::new(half.x, half.y),
        glam::Vec2::new(-half.x, half.y),
    ]
    .map(|p| {
        let q = m * p;
        // Screen y runs down.
        pos2(c.x + q.x, c.y - q.y)
    });
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (p, uv) in corners.into_iter().zip(uv) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: p,
            uv,
            color: tint,
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn palette_grid(ui: &mut Ui, state: &mut EditorState) {
    if state.tile_palette.palette.is_none() {
        ui.weak("Create or choose a palette, then drag sprites, textures or tiles here.");
        // Dropping onto the empty area still works once a palette exists.
        return;
    }
    let extent = state.tile_palette.data.extent();
    let cols = extent.x.max(palette::COLUMNS);
    let rows = extent.y.max(1) + 1;
    let size = vec2(cols as f32 * CELL, rows as f32 * CELL);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_gray(36));
    let cell_rect = |c: IVec2| {
        Rect::from_min_size(
            rect.min + vec2(c.x as f32 * CELL, c.y as f32 * CELL),
            Vec2::splat(CELL),
        )
    };
    let cell_at = |p: Pos2| {
        let local = p - rect.min;
        IVec2::new(
            (local.x / CELL).floor() as i32,
            (local.y / CELL).floor() as i32,
        )
    };
    for y in 0..rows {
        for x in 0..cols {
            let r = cell_rect(IVec2::new(x, y)).shrink(1.0);
            painter.rect_stroke(
                r,
                1.0,
                Stroke::new(1.0, Color32::from_gray(52)),
                egui::StrokeKind::Inside,
            );
        }
    }
    let entries = state.tile_palette.data.tiles.clone();
    for e in &entries {
        let c = IVec2::new(e.x, e.y);
        let r = cell_rect(c).shrink(4.0);
        if let Some(asset) = tile::get(&e.tile)
            && let Some(sprite) = asset.preview_sprite()
        {
            let tint = Color32::from_rgba_unmultiplied(
                (asset.color[0] * 255.0) as u8,
                (asset.color[1] * 255.0) as u8,
                (asset.color[2] * 255.0) as u8,
                (asset.color[3] * 255.0) as u8,
            );
            paint_sprite(
                ui,
                &mut state.tile_palette.sprites,
                r,
                sprite,
                TileTransform::default(),
                tint,
            );
            if asset.kind != tile::TileKind::Tile {
                let badge = if asset.kind == tile::TileKind::RuleTile {
                    "R"
                } else {
                    "A"
                };
                painter.text(
                    r.right_top(),
                    egui::Align2::RIGHT_TOP,
                    badge,
                    egui::FontId::proportional(10.0),
                    Color32::from_rgb(255, 210, 90),
                );
            }
        } else {
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(14.0),
                Color32::LIGHT_RED,
            );
        }
    }
    if let Some(c) = state.tile_palette.picked_cell {
        painter.rect_stroke(
            cell_rect(c).shrink(1.0),
            2.0,
            Stroke::new(2.0, Color32::from_rgb(255, 200, 60)),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(p) = response.hover_pos() {
        let c = cell_at(p);
        if let Some(t) = state.tile_palette.data.get(c) {
            response.clone().on_hover_text(t.to_owned());
        }
        painter.rect_stroke(
            cell_rect(c).shrink(1.0),
            2.0,
            Stroke::new(1.0, Color32::from_gray(150)),
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        state.tile_palette.pick_palette(cell_at(p));
    }
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let c = cell_at(p);
        if state.tile_palette.data.get(c).is_some() {
            state.tile_palette.data.set(c, None);
            let _ = state.tile_palette.save();
        }
    }
    if let Some(payload) = response.dnd_release_payload::<AssetPayload>() {
        let at = ui
            .input(|i| i.pointer.hover_pos())
            .map(cell_at)
            .filter(|c| state.tile_palette.data.get(*c).is_none());
        let added = crate::tile_palette::drop_on_palette(state, &payload.0, at);
        if added.is_empty() {
            state
                .console
                .warn(format!("{} is not a sprite, texture or tile.", payload.0));
        }
    }
}

fn brush_row(ui: &mut Ui, state: &mut EditorState) {
    ui.horizontal(|ui| {
        let tp = &state.tile_palette;
        match tp.brush.first_tile() {
            Some(t) => {
                let size = tp.brush.size();
                let tr = tp.brush.cells[0].transform;
                let mut text = format!("Brush: {}", stem(t));
                if size != IVec2::ONE {
                    text += &format!("  ({}×{})", size.x, size.y);
                }
                if tr.rotation() > 0 {
                    text += &format!("  ↻{}°", tr.rotation() as u32 * 90);
                }
                if tr.flip_x() {
                    text += "  flip X";
                }
                if tr.flip_y() {
                    text += "  flip Y";
                }
                ui.label(text);
                let t = t.to_owned();
                if ui.small_button("Select Tile").clicked() {
                    let abs = state.assets.absolute(&t);
                    state.project.selected = Some(abs);
                    state.selection.clear();
                    state.inspector_asset = Some(t);
                }
            }
            None => {
                ui.weak("Brush: none (click a palette tile)");
            }
        }
    });
}
