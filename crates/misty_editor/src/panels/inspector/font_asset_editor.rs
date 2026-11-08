//! Font asset import settings (Project window ▸ a `.ttf` / `.otf`): atlas render mode (SDF or
//! smooth raster), sampling size, SDF padding and fallback fonts, with Apply / Revert and a
//! preview of the glyphs as the atlas will hold them.

use egui::{Color32, RichText, Ui};
use misty_engine::ui::rich::font_asset::{FontImporter, FontRenderMode};
use misty_engine::ui::rich::{fonts, sdf};

use crate::state::EditorState;

/// Characters shown in the atlas preview.
const PREVIEW: &str = "AaBbGgQq@&369";

fn pending_id(rel: &str) -> egui::Id {
    egui::Id::new(("misty_font_import_pending", rel.to_owned()))
}

pub fn font_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let name = std::path::Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new("🗛").size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{name} (Font Asset) Import Settings"),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let abs = state.assets.absolute(rel);
    let on_disk = FontImporter::load(&abs);
    let id = pending_id(rel);
    let mut p = ui
        .data(|d| d.get_temp::<FontImporter>(id))
        .unwrap_or_else(|| on_disk.clone());
    let fonts_list: Vec<String> = state
        .assets
        .fonts
        .iter()
        .filter(|f| f.as_str() != rel)
        .cloned()
        .collect();
    settings_ui(ui, &mut p, &fonts_list);
    ui.add_space(6.0);
    let dirty = p != on_disk;
    ui.horizontal(|ui| {
        if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
            p = on_disk.clone();
        }
        if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
            p.sanitize();
            match p.save(&abs) {
                Ok(()) => {
                    // Texts pick the new atlas settings up on their next draw.
                    fonts::reload(rel);
                    state.scene_dirty = true;
                }
                Err(e) => state.console.warn(format!("Could not save {rel}: {e}")),
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, p.clone()));
    ui.separator();
    preview(ui, rel, &p);
}

fn settings_ui(ui: &mut Ui, s: &mut FontImporter, fonts_list: &[String]) {
    crate::panels::inspector::card::PropertyGrid::new("font_import_grid").show(ui, |ui| {
        ui.label("Rendering Mode").on_hover_text(concat!(
            "SDF stays sharp at any size and allows outlines and soft shadows; ",
            "Raster is smooth coverage, best near the sampling size."
        ));
        egui::ComboBox::from_id_salt("font_mode")
            .selected_text(s.render_mode.label())
            .show_ui(ui, |ui| {
                for m in FontRenderMode::ALL {
                    ui.selectable_value(&mut s.render_mode, m, m.label());
                }
            });
        ui.end_row();
        ui.label("Sampling Point Size")
            .on_hover_text("Em size, in atlas pixels, glyphs are made at.");
        ui.add(egui::Slider::new(
            &mut s.sampling_size,
            FontImporter::MIN_SAMPLING..=FontImporter::MAX_SAMPLING,
        ));
        ui.end_row();
        if s.render_mode == FontRenderMode::Sdf {
            ui.label("Padding")
                .on_hover_text("Distance field reach in atlas pixels (wider outlines need more).");
            ui.add(egui::Slider::new(&mut s.padding, 1.0..=32.0));
            ui.end_row();
        }
        true
    });
    ui.add_space(4.0);
    ui.label("Fallback Font Assets");
    egui::Frame::group(ui.style()).show(ui, |ui| {
        if s.fallback_fonts.is_empty() {
            ui.weak("None: the built-in and system fonts fill in missing characters.");
        }
        let mut remove = None;
        for (i, f) in s.fallback_fonts.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(f);
                if ui.small_button("−").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            s.fallback_fonts.remove(i);
        }
        let choices: Vec<&String> = fonts_list
            .iter()
            .filter(|f| !s.fallback_fonts.contains(f))
            .collect();
        if !choices.is_empty() {
            egui::ComboBox::from_id_salt("font_add_fallback")
                .selected_text("Add Fallback…")
                .show_ui(ui, |ui| {
                    for f in choices {
                        if ui.selectable_label(false, f.as_str()).clicked() {
                            s.fallback_fonts.push(f.clone());
                        }
                    }
                });
        }
    });
}

/// The preview glyphs as an image: (width, height, grey pixels).
fn preview_pixels(rel: &str, s: &FontImporter) -> Option<(usize, usize, Vec<u8>)> {
    let font = fonts::font(fonts::load(rel).ok()?);
    let glyphs: Vec<(u32, u32, Vec<u8>)> = PREVIEW
        .chars()
        .filter_map(|c| {
            let g = font.glyph(c)?;
            let (w, h, _, data) = match s.render_mode {
                FontRenderMode::Raster => sdf::glyph_coverage(&font, g, s.sampling_size)?,
                FontRenderMode::Sdf => sdf::glyph_field_at(&font, g, s.sampling_size, s.padding)?,
            };
            Some((w, h, data))
        })
        .collect();
    let width: u32 = glyphs.iter().map(|g| g.0 + 2).sum::<u32>().max(1);
    let height = glyphs.iter().map(|g| g.1).max().unwrap_or(1).max(1);
    let mut px = vec![0u8; (width * height) as usize];
    let mut x0 = 0;
    for (w, h, data) in glyphs {
        for y in 0..h {
            for x in 0..w {
                px[(y * width + x0 + x) as usize] = data[(y * w + x) as usize];
            }
        }
        x0 += w + 2;
    }
    Some((width as usize, height as usize, px))
}

fn preview(ui: &mut Ui, rel: &str, s: &FontImporter) {
    let key = egui::Id::new((
        "misty_font_preview",
        rel.to_owned(),
        s.render_mode as u8,
        s.sampling_size.to_bits(),
        s.padding.to_bits(),
    ));
    let tex = match ui.data(|d| d.get_temp::<Option<egui::TextureHandle>>(key)) {
        Some(t) => t,
        None => {
            let t = preview_pixels(rel, s).map(|(w, h, px)| {
                let img = egui::ColorImage::from_gray([w, h], &px);
                ui.ctx()
                    .load_texture(format!("font:{rel}"), img, egui::TextureOptions::LINEAR)
            });
            ui.data_mut(|d| d.insert_temp(key, t.clone()));
            t
        }
    };
    let Some(tex) = tex else {
        ui.weak("The font can't be read.");
        return;
    };
    ui.label(match s.render_mode {
        FontRenderMode::Sdf => "Atlas preview (distance field)",
        FontRenderMode::Raster => "Atlas preview (coverage)",
    });
    let size = tex.size_vec2();
    let scale = (ui.available_width().min(320.0) / size.x).min(1.5);
    let (rect, _) = ui.allocate_exact_size(size * scale, egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    ui.painter().image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    ui.add_space(2.0);
    ui.weak(format!(
        "{} px per em, {}",
        s.sampling_size.round(),
        match s.render_mode {
            FontRenderMode::Sdf => format!("{} px padding", s.padding.round()),
            FontRenderMode::Raster => "4×4 supersampled".into(),
        }
    ));
}
