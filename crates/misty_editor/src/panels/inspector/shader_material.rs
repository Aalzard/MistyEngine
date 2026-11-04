//! Material Inspector for `.shader` assets: one field per declared `@property` (slider, number,
//! colour, vector or texture slot, with the property's tooltip), the render state read from the
//! shader's header, and the compile status.

use egui::{Color32, RichText, Ui};
use misty_engine::assets::Material;
use misty_engine::shader_asset::{self, Blend, Cull, PropertyKind, PropertyValue, ShaderModel};

use super::slots::texture_slot;
use crate::theme;

fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// Shader dropdown: the built-in shaders, then the project's `.shader` assets.
pub fn shader_combo(ui: &mut Ui, id: &str, m: &mut Material, shaders: &[String]) -> bool {
    use misty_engine::render::ShaderKind;
    let mut ch = false;
    let current = if m.is_custom() {
        format!("Shaders/{}", file_stem(&m.shader))
    } else {
        m.shader_kind().name().to_owned()
    };
    egui::ComboBox::from_id_salt(format!("{id}_shader"))
        .selected_text(current)
        .show_ui(ui, |ui| {
            for k in ShaderKind::ALL {
                let selected = !m.is_custom() && m.shader_kind() == k;
                if ui.selectable_label(selected, k.name()).clicked() && !selected {
                    m.shader = k.name().to_owned();
                    ch = true;
                }
            }
            if !shaders.is_empty() {
                ui.separator();
            }
            for s in shaders {
                let selected = m.shader == *s;
                if ui
                    .selectable_label(selected, format!("Shaders/{}", file_stem(s)))
                    .on_hover_text(s)
                    .clicked()
                    && !selected
                {
                    m.shader = s.clone();
                    ch = true;
                }
            }
        });
    ch
}

fn color_rgba(ui: &mut Ui, c: &mut [f32; 4]) -> bool {
    let mut srgba = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = crate::panels::inspector::card::wide(ui, |ui| {
        ui.color_edit_button_srgba_unmultiplied(&mut srgba)
    })
    .changed();
    if changed {
        *c = srgba.map(|v| v as f32 / 255.0);
    }
    changed
}

/// Property rows of a `.shader` material (inside the material's grid).
pub fn custom_fields(ui: &mut Ui, id: &str, m: &mut Material, textures: &[String]) -> bool {
    let asset = match shader_asset::load(&m.shader) {
        Ok(a) => a,
        Err(e) => {
            ui.label("Shader");
            ui.label(RichText::new(format!("Error: {e}")).color(Color32::from_rgb(230, 90, 90)));
            ui.end_row();
            return false;
        }
    };
    let mut ch = false;
    if let Err(e) = misty_engine::render::custom_shader::check(&asset.source) {
        ui.label("");
        ui.label(
            RichText::new(format!("Does not compile (drawn magenta): {e}"))
                .color(Color32::from_rgb(230, 90, 90)),
        )
        .on_hover_text("Fix the shader in the Code Editor; it recompiles on save");
        ui.end_row();
    }
    for p in &asset.properties {
        let tip = p.tooltip.as_deref().unwrap_or(&p.name);
        ui.label(&p.display).on_hover_text(tip);
        let mut value = m
            .properties
            .get(&p.name)
            .filter(|v| v.fits(p.kind))
            .cloned()
            .unwrap_or_else(|| p.default.clone());
        let changed = match (&mut value, p.kind) {
            (PropertyValue::Float(v), PropertyKind::Range(lo, hi)) => ui
                .add(egui::Slider::new(v, lo..=hi))
                .on_hover_text(tip)
                .changed(),
            (PropertyValue::Float(v), _) => ui
                .add(egui::DragValue::new(v).speed(0.01))
                .on_hover_text(tip)
                .changed(),
            (PropertyValue::Vector(c), PropertyKind::Color) => color_rgba(ui, c),
            (PropertyValue::Vector(v), _) => {
                ui.horizontal(|ui| {
                    let mut ch = false;
                    for (axis, c) in ["X", "Y", "Z", "W"].into_iter().zip(v.iter_mut()) {
                        ui.label(axis);
                        ch |= ui.add(egui::DragValue::new(c).speed(0.01)).changed();
                    }
                    ch
                })
                .inner
            }
            (PropertyValue::Texture(t), _) => {
                texture_slot(ui, &format!("{id}_{}", p.name), t, textures)
            }
        };
        ui.end_row();
        if changed {
            m.properties.insert(p.name.clone(), value);
            ch = true;
        }
    }
    let s = asset.settings;
    ui.label("Render Queue");
    let blend = match s.blend {
        Blend::Off => "Opaque",
        Blend::Alpha => "Alpha blend",
        Blend::Premultiply => "Premultiplied",
        Blend::Additive => "Additive",
    };
    let cull = match s.cull {
        Cull::Back => "Cull Back",
        Cull::Front => "Cull Front",
        Cull::Off => "Cull Off",
    };
    let model = match s.model {
        ShaderModel::Surface => "Lit",
        ShaderModel::Unlit => "Unlit",
    };
    ui.label(RichText::new(format!("From Shader  {}", s.queue)).color(theme::TEXT_DIM))
        .on_hover_text(format!(
            "{model}, {blend}, {cull}, depth write {}",
            if s.zwrite { "on" } else { "off" }
        ));
    ui.end_row();
    ch
}
