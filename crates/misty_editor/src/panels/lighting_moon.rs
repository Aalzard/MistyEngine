//! Lighting window ▸ Environment ▸ Moon: the moon disc, its phase and position, the moonlight
//! that takes over the main Directional Light at night, the stars, and the texture (built-in
//! or a project image; see `misty_engine::moon`).

use egui::{RichText, Ui};
use misty_engine::moon::{self, MoonMode, MoonSettings};

use super::lighting::{color, row};

/// Phase name for a 0..1 phase (0 = new, 0.5 = full).
pub(super) fn phase_name(p: f32) -> &'static str {
    let p = p.rem_euclid(1.0);
    match p {
        _ if !(0.03..=0.97).contains(&p) => "New Moon",
        _ if p < 0.22 => "Waxing Crescent",
        _ if p < 0.28 => "First Quarter",
        _ if p < 0.47 => "Waxing Gibbous",
        _ if p < 0.53 => "Full Moon",
        _ if p < 0.72 => "Waning Gibbous",
        _ if p < 0.78 => "Last Quarter",
        _ => "Waning Crescent",
    }
}

/// What the rows show besides the settings.
pub(super) struct MoonContext {
    /// Project textures for the Texture slot.
    pub textures: Vec<String>,
    /// Lit fraction and whether it waxes, for Date and Time (computed for the settings).
    pub sky: Option<moon::ephemeris::Sky>,
}

impl MoonContext {
    pub fn new(textures: Vec<String>, s: &MoonSettings) -> Self {
        let sky = (s.mode == MoonMode::DateTime).then(|| {
            moon::ephemeris::sky(
                s.year,
                s.month.clamp(1, 12),
                s.day.clamp(1, 31),
                s.time as f64,
                s.utc_offset as f64,
                s.latitude as f64,
                s.longitude as f64,
            )
        });
        Self { textures, sky }
    }
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) -> bool {
    ui.add(egui::Slider::new(v, range).suffix(suffix)).changed()
}

/// The Moon section; true when something changed.
pub(super) fn section(ui: &mut Ui, m: &mut MoonSettings, cx: &MoonContext) -> bool {
    ui.add_space(4.0);
    ui.label(RichText::new("Moon").strong());
    super::lighting::grid(ui, "lighting_moon", |ui| {
        let mut ch = ui.checkbox(&mut m.enabled, "Moon").changed();
        ui.end_row();
        if !m.enabled {
            return ch;
        }
        ch |= row(ui, "Position", |ui| {
            let mut changed = false;
            egui::ComboBox::from_id_salt("moon_mode")
                .selected_text(m.mode.label())
                .width(160.0)
                .show_ui(ui, |ui| {
                    for o in MoonMode::ALL {
                        changed |= ui.selectable_value(&mut m.mode, o, o.label()).changed();
                    }
                });
            changed
        });
        match m.mode {
            MoonMode::FollowSun | MoonMode::Manual => {
                ch |= row(ui, "Phase", |ui| {
                    let c = slider(ui, &mut m.phase, 0.0..=1.0, "");
                    ui.weak(phase_name(m.phase));
                    c
                });
                if m.mode == MoonMode::Manual {
                    ch |= row(ui, "Azimuth", |ui| {
                        slider(ui, &mut m.azimuth, 0.0..=360.0, "°")
                    });
                    ch |= row(ui, "Altitude", |ui| {
                        slider(ui, &mut m.altitude, -10.0..=90.0, "°")
                    });
                }
            }
            MoonMode::DateTime => {
                ch |= row(ui, "Date", |ui| {
                    let mut c = ui
                        .add(egui::DragValue::new(&mut m.year).range(1800..=2200))
                        .changed();
                    c |= ui
                        .add(egui::DragValue::new(&mut m.month).range(1..=12))
                        .changed();
                    c |= ui
                        .add(egui::DragValue::new(&mut m.day).range(1..=31))
                        .changed();
                    c
                });
                ch |= row(ui, "Time", |ui| slider(ui, &mut m.time, 0.0..=24.0, " h"));
                ch |= row(ui, "UTC Offset", |ui| {
                    slider(ui, &mut m.utc_offset, -12.0..=14.0, " h")
                });
                if let Some(sky) = &cx.sky {
                    row(ui, "Phase", |ui| {
                        let name = match (sky.illumination, sky.waxing) {
                            (k, _) if k < 0.02 => "New Moon",
                            (k, _) if k > 0.98 => "Full Moon",
                            (_, true) => "Waxing",
                            (_, false) => "Waning",
                        };
                        ui.weak(format!("{name}, {:.0}% lit", sky.illumination * 100.0));
                        false
                    });
                }
            }
        }
        if m.mode != MoonMode::Manual {
            ch |= row(ui, "Latitude", |ui| {
                slider(ui, &mut m.latitude, -90.0..=90.0, "°")
            });
        }
        if m.mode == MoonMode::DateTime {
            ch |= row(ui, "Longitude", |ui| {
                slider(ui, &mut m.longitude, -180.0..=180.0, "°")
            });
        }
        ch |= row(ui, "Size", |ui| slider(ui, &mut m.size, 0.5..=12.0, "×"));
        ch |= row(ui, "Brightness", |ui| {
            slider(ui, &mut m.brightness, 0.0..=4.0, "")
        });
        ch |= row(ui, "Earthshine", |ui| {
            slider(ui, &mut m.earthshine, 0.0..=4.0, "")
        });
        ch |= row(ui, "Texture", |ui| {
            let c = crate::panels::inspector::slots::texture_slot(
                ui,
                "moon_texture",
                &mut m.texture,
                &cx.textures,
            );
            if m.texture.is_none() {
                ui.weak("Built-in").on_hover_text(
                    "Albedo only, full and evenly lit: 2:1 = whole moon (near side centred),                      square = near-side photo. The sky makes the phases.",
                );
            }
            c
        });
        ch |= row(ui, "Moonlight", |ui| {
            slider(ui, &mut m.light_intensity, 0.0..=2.0, "")
        });
        ch |= row(ui, "Moonlight Color", |ui| color(ui, &mut m.light_color));
        ch |= row(ui, "Stars", |ui| slider(ui, &mut m.stars, 0.0..=4.0, ""));
        ch
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_names() {
        assert_eq!(phase_name(0.0), "New Moon");
        assert_eq!(phase_name(0.12), "Waxing Crescent");
        assert_eq!(phase_name(0.25), "First Quarter");
        assert_eq!(phase_name(0.5), "Full Moon");
        assert_eq!(phase_name(0.75), "Last Quarter");
        assert_eq!(phase_name(0.9), "Waning Crescent");
        assert_eq!(phase_name(1.0), "New Moon");
    }
}
