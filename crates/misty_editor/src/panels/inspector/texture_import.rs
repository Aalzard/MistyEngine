//! Texture import settings in the Inspector (selected `.png` / `.jpg`): Texture Type, Sprite
//! Mode, Pixels Per Unit, Pivot, Max Size, Filter Mode and mipmaps on top; Wrap Mode, sRGB and
//! Aniso Level under a collapsed "Advanced"; a grid slicer for Multiple mode, Revert / Apply and
//! a preview that outlines the sprites.
//!
//! Unapplied edits live in egui's memory (per texture) until Apply writes the `.meta` file.

use egui::{Color32, Grid, RichText, Stroke, Ui, pos2, vec2};
use misty_engine::sprites::import::{MAX_SIZES, PIVOTS, max_size_label};
use misty_engine::sprites::slice::{self, GridSlice};
use misty_engine::sprites::{self, FilterMode, SpriteMode, TextureImporter, TextureType, WrapMode};

use crate::state::EditorState;
use crate::theme;

/// Unapplied settings and slicer values of one texture.
#[derive(Clone)]
struct Pending {
    settings: TextureImporter,
    grid: [u32; 6],
    keep_empty: bool,
}

fn pending_id(rel: &str) -> egui::Id {
    egui::Id::new(("misty_texture_import", rel.to_owned()))
}

/// The settings on disk.
fn saved(state: &EditorState, rel: &str) -> TextureImporter {
    TextureImporter::load(&state.assets.absolute(rel))
}

pub fn texture_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let name = std::path::Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new("🖼").size(22.0));
        ui.label(RichText::new(format!("{name} (Texture 2D) Import Settings")).strong());
    });
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
    ui.separator();

    let on_disk = saved(state, rel);
    let id = pending_id(rel);
    let mut p = ui
        .ctx()
        .data(|d| d.get_temp::<Pending>(id))
        .unwrap_or(Pending {
            settings: on_disk.clone(),
            grid: [16, 16, 0, 0, 0, 0],
            keep_empty: false,
        });
    let size = sprites::texture_info(rel).1;
    settings_ui(ui, &mut p.settings);
    if p.settings.is_sprite() && p.settings.sprite_mode == SpriteMode::Multiple {
        slicer(ui, rel, &mut p, size);
    }
    ui.add_space(6.0);
    let dirty = p.settings != on_disk;
    ui.horizontal(|ui| {
        if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
            p.settings = on_disk.clone();
        }
        if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
            apply(state, rel, &p.settings);
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, p.clone()));
    ui.separator();
    preview(ui, state, rel, &p.settings, size);
}

/// Write the settings and make every cache pick them up.
pub fn apply(state: &mut EditorState, rel: &str, settings: &TextureImporter) {
    match settings.save(&state.assets.absolute(rel)) {
        Ok(()) => {
            sprites::invalidate(rel);
            state
                .console
                .info(format!("Applied import settings of {rel}"));
        }
        Err(e) => state
            .console
            .warn(format!("Could not save import settings of {rel}: {e}")),
    }
}

fn combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    all: &[T],
    label: impl Fn(T) -> &'static str,
) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for v in all {
                ch |= ui.selectable_value(value, *v, label(*v)).changed();
            }
        });
    ch
}

fn settings_ui(ui: &mut Ui, s: &mut TextureImporter) {
    Grid::new("texture_import_grid")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(130.0)
        .show(ui, |ui| {
            ui.label("Texture Type").on_hover_text(
                "Default for 3D materials, Normal Map for bumps, Sprite for 2D and UI.",
            );
            let mut t = s.texture_type;
            if combo(
                ui,
                "tex_type",
                &mut t,
                &TextureType::ALL,
                TextureType::label,
            ) {
                // Sprites clamp and skip mipmaps; normal maps are linear.
                s.set_type(t);
            }
            ui.end_row();
            if s.is_sprite() {
                ui.label("Sprite Mode");
                combo(
                    ui,
                    "sprite_mode",
                    &mut s.sprite_mode,
                    &SpriteMode::ALL,
                    SpriteMode::label,
                );
                ui.end_row();
                ui.label("Pixels Per Unit");
                ui.add(
                    egui::DragValue::new(&mut s.pixels_per_unit)
                        .speed(0.5)
                        .range(0.01..=100000.0),
                );
                ui.end_row();
                if s.sprite_mode == SpriteMode::Single {
                    ui.label("Pivot");
                    pivot(ui, "tex_pivot", &mut s.pivot);
                    ui.end_row();
                }
            }
            ui.label("Max Size")
                .on_hover_text("Larger images are scaled down when loaded (saves GPU memory).");
            egui::ComboBox::from_id_salt("tex_max_size")
                .selected_text(max_size_label(s.max_size))
                .show_ui(ui, |ui| {
                    for v in MAX_SIZES {
                        ui.selectable_value(&mut s.max_size, v, max_size_label(v));
                    }
                });
            ui.end_row();
            ui.label("Filter Mode").on_hover_text(
                "Nearest Neighbor: crisp pixel art. Bilinear: smooth. Trilinear: smooth and \
                 blends between mip levels (best for 3D).",
            );
            combo(
                ui,
                "tex_filter",
                &mut s.filter_mode,
                &FilterMode::ALL,
                FilterMode::label,
            );
            ui.end_row();
            ui.label("Generate Mip Maps")
                .on_hover_text("Smaller copies used far away: less shimmer, faster. Off for UI.");
            ui.checkbox(&mut s.generate_mip_maps, "");
            ui.end_row();
        });
    egui::CollapsingHeader::new("Advanced")
        .id_salt("texture_import_advanced")
        .default_open(false)
        .show(ui, |ui| {
            Grid::new("texture_import_advanced_grid")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .min_col_width(118.0)
                .show(ui, |ui| {
                    ui.label("Wrap Mode").on_hover_text(
                        "What happens outside 0–1 UVs: tile, stretch the edge, or mirror.",
                    );
                    combo(
                        ui,
                        "tex_wrap",
                        &mut s.wrap_mode,
                        &WrapMode::ALL,
                        WrapMode::label,
                    );
                    ui.end_row();
                    ui.label("sRGB (Color Texture)").on_hover_text(
                        "On for colours. Off for data such as masks and metallic maps.",
                    );
                    let normal = s.texture_type == TextureType::NormalMap;
                    ui.add_enabled_ui(!normal, |ui| {
                        let mut on = s.is_srgb();
                        if ui.checkbox(&mut on, "").changed() {
                            s.srgb = on;
                        }
                    });
                    ui.end_row();
                    ui.label("Aniso Level").on_hover_text(
                        "Sharpness at grazing angles (0–16; 1 = off). Needs Trilinear; see \
                         Quality ▸ Anisotropic Textures.",
                    );
                    ui.add(egui::Slider::new(&mut s.aniso_level, 0..=16));
                    ui.end_row();
                });
        });
}

/// Pivot preset dropdown with X/Y fields for Custom.
fn pivot(ui: &mut Ui, id: &str, p: &mut [f32; 2]) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(sprites::import::pivot_name(*p))
            .show_ui(ui, |ui| {
                for (name, v) in PIVOTS {
                    if ui.selectable_label(*p == v, name).clicked() {
                        *p = v;
                    }
                }
            });
        ui.add(egui::DragValue::new(&mut p[0]).speed(0.01).prefix("X "));
        ui.add(egui::DragValue::new(&mut p[1]).speed(0.01).prefix("Y "));
    });
}

/// Slice ▸ Grid By Cell Size (the Sprite Editor's grid slicer).
fn slicer(ui: &mut Ui, rel: &str, p: &mut Pending, size: Option<[u32; 2]>) {
    ui.add_space(4.0);
    ui.label(RichText::new("Slice: Grid By Cell Size").strong());
    let g = &mut p.grid;
    let row = |ui: &mut Ui, label: &str, a: &mut u32, b: &mut u32, min: u32| {
        ui.label(label);
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(a).range(min..=8192).prefix("X "));
            ui.add(egui::DragValue::new(b).range(min..=8192).prefix("Y "));
        });
        ui.end_row();
    };
    Grid::new("texture_slice_grid")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(130.0)
        .show(ui, |ui| {
            let [cw, ch, ox, oy, px, py] = g;
            row(ui, "Pixel Size", cw, ch, 1);
            row(ui, "Offset", ox, oy, 0);
            row(ui, "Padding", px, py, 0);
            ui.label("Keep Empty Rects");
            ui.checkbox(&mut p.keep_empty, "");
            ui.end_row();
        });
    let count = p.settings.sprites.len();
    ui.horizontal(|ui| {
        let Some([w, h]) = size else {
            ui.weak("The image could not be read.");
            return;
        };
        if ui.button("Slice").clicked() {
            let image = sprites::load_image(rel).ok();
            let base = std::path::Path::new(rel)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let grid = GridSlice {
                cell: [g[0], g[1]],
                offset: [g[2], g[3]],
                padding: [g[4], g[5]],
                pivot: [0.5, 0.5],
                skip_empty: !p.keep_empty,
            };
            p.settings.sprites = slice::grid(w, h, &grid, &base, image.as_ref());
        }
        ui.weak(format!("{count} sprites"));
    });
}

/// The image (filtered like its Filter Mode) with sprite rects outlined.
fn preview(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    s: &TextureImporter,
    size: Option<[u32; 2]>,
) {
    let abs = state.assets.absolute(rel);
    let Some(tex) = preview_texture(ui.ctx(), rel, &abs, s.filter_mode) else {
        return;
    };
    let [w, h] = size.unwrap_or([1, 1]).map(|v| v.max(1) as f32);
    let avail = ui.available_width().min(320.0);
    let scale = (avail / w).min(256.0 / h);
    let (rect, _) = ui.allocate_exact_size(vec2(w * scale, h * scale), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    // Checkerboard behind transparent pixels.
    painter.rect_filled(rect, 0.0, Color32::from_gray(60));
    painter.image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    if s.is_sprite() && s.sprite_mode == SpriteMode::Multiple {
        let stroke = Stroke::new(1.0, Color32::from_rgb(80, 160, 255));
        for r in &s.sprites {
            let [x, y, rw, rh] = r.rect;
            let min = rect.min + vec2(x * scale, (h - y - rh) * scale);
            let r = egui::Rect::from_min_size(min, vec2(rw * scale, rh * scale));
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        }
    }
    let info = match size {
        Some([w, h]) => format!("{w}×{h}"),
        None => "unreadable".into(),
    };
    ui.weak(info);
}

/// Full-size egui texture of the image, cached per file version and filter.
fn preview_texture(
    ctx: &egui::Context,
    rel: &str,
    abs: &std::path::Path,
    filter: FilterMode,
) -> Option<egui::TextureHandle> {
    let modified = std::fs::metadata(abs).and_then(|m| m.modified()).ok();
    let id = egui::Id::new(("misty_texture_preview", rel.to_owned(), filter as u8));
    type Entry = (Option<std::time::SystemTime>, Option<egui::TextureHandle>);
    if let Some((m, t)) = ctx.data(|d| d.get_temp::<Entry>(id))
        && m == modified
    {
        return t;
    }
    let tex = sprites::load_image(rel).ok().map(|img| {
        let size = [img.width() as usize, img.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        let options = match filter {
            FilterMode::Nearest => egui::TextureOptions::NEAREST,
            FilterMode::Bilinear | FilterMode::Trilinear => egui::TextureOptions::LINEAR,
        };
        ctx.load_texture(format!("import:{rel}"), color, options)
    });
    ctx.data_mut(|d| d.insert_temp::<Entry>(id, (modified, tex.clone())));
    tex
}
