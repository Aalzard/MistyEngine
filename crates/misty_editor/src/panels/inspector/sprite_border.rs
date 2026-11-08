//! Sprite border (9-slice) editing in the texture import settings: left / bottom / right / top
//! pixel fields for a Single sprite or the chosen sprite of a Multiple texture, and the green
//! border lines on the preview. Sliced and Tiled UI Images use these borders.

use egui::{Color32, Rect, Stroke, Ui, vec2};
use misty_engine::sprites::{SpriteMode, TextureImporter};

const BORDER: Color32 = Color32::from_rgb(90, 230, 110);

fn sel_id(rel: &str) -> egui::Id {
    egui::Id::new(("misty_sprite_border_pick", rel.to_owned()))
}

/// Border fields of one sprite of `w`×`h` pixels; true when one changed.
fn fields(ui: &mut Ui, border: &mut [f32; 4], w: f32, h: f32) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        for (i, (label, max)) in [("L", w), ("B", h), ("R", w), ("T", h)]
            .into_iter()
            .enumerate()
        {
            ui.label(label);
            ch |= ui
                .add(
                    egui::DragValue::new(&mut border[i])
                        .speed(0.25)
                        .range(0.0..=max.max(0.0))
                        .max_decimals(1),
                )
                .changed();
        }
    });
    // Opposite borders never cross.
    if border[0] + border[2] > w {
        border[2] = (w - border[0]).max(0.0);
    }
    if border[1] + border[3] > h {
        border[3] = (h - border[1]).max(0.0);
    }
    ch
}

/// The Border rows of a sprite texture's import settings.
pub fn border_ui(ui: &mut Ui, rel: &str, s: &mut TextureImporter, size: Option<[u32; 2]>) {
    if !s.is_sprite() {
        return;
    }
    let [tw, th] = size.unwrap_or([0, 0]).map(|v| v as f32);
    ui.add_space(4.0);
    ui.label(crate::theme::bold(ui.ctx(), "Sprite Border (9-slice)"))
        .on_hover_text("Pixels kept at their size by Sliced and Tiled UI Images.");
    match s.sprite_mode {
        SpriteMode::Single => {
            fields(ui, &mut s.border, tw, th);
        }
        SpriteMode::Multiple => {
            if s.sprites.is_empty() {
                ui.weak("Slice the texture first.");
                return;
            }
            let id = sel_id(rel);
            let mut pick = ui
                .data(|d| d.get_temp::<usize>(id))
                .unwrap_or(0)
                .min(s.sprites.len() - 1);
            egui::ComboBox::from_id_salt(("sprite_border_pick", rel))
                .selected_text(s.sprites[pick].name.clone())
                .show_ui(ui, |ui| {
                    for (i, r) in s.sprites.iter().enumerate() {
                        ui.selectable_value(&mut pick, i, r.name.clone());
                    }
                });
            ui.data_mut(|d| d.insert_temp(id, pick));
            let r = &mut s.sprites[pick];
            let [_, _, w, h] = r.rect;
            fields(ui, &mut r.border, w, h);
        }
    }
}

/// Green border lines over the import preview (`rect` shows the whole `w`×`h` image at
/// `scale` points per pixel).
pub fn paint(painter: &egui::Painter, rect: Rect, scale: f32, h: f32, s: &TextureImporter) {
    if !s.is_sprite() {
        return;
    }
    let stroke = Stroke::new(1.0, BORDER);
    // (sprite rect x, y, w, h in pixels with y up, border)
    let rects: Vec<([f32; 4], [f32; 4])> = match s.sprite_mode {
        SpriteMode::Single => vec![([0.0, 0.0, rect.width() / scale, h], s.border)],
        SpriteMode::Multiple => s.sprites.iter().map(|r| (r.rect, r.border)).collect(),
    };
    for ([x, y, w, rh], b) in rects {
        if b == [0.0; 4] {
            continue;
        }
        let min = rect.min + vec2(x * scale, (h - y - rh) * scale);
        let size = vec2(w * scale, rh * scale);
        let r = Rect::from_min_size(min, size);
        let [l, bo, ri, t] = b.map(|v| v * scale);
        let vline = |px: f32| {
            painter.line_segment(
                [egui::pos2(px, r.top()), egui::pos2(px, r.bottom())],
                stroke,
            )
        };
        let hline = |py: f32| {
            painter.line_segment(
                [egui::pos2(r.left(), py), egui::pos2(r.right(), py)],
                stroke,
            )
        };
        if l > 0.0 {
            vline(r.left() + l);
        }
        if ri > 0.0 {
            vline(r.right() - ri);
        }
        if t > 0.0 {
            hline(r.top() + t);
        }
        if bo > 0.0 {
            hline(r.bottom() - bo);
        }
    }
}
