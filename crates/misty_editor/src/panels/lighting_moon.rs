//! Lighting window ▸ Environment ▸ Moon: the moon disc, its phase and position, the moonlight
//! that takes over the main Directional Light at night, the stars, and the texture (built-in
//! or a project image; see `misty_engine::moon`).

use egui::{RichText, Ui};
use misty_engine::moon::night_sky::NightSkySettings;
use misty_engine::moon::{self, MoonMode, MoonSettings};

use super::lighting::{color, row};
use crate::i18n::tr;

/// English phase name for a 0..1 phase (0 = new, 0.5 = full); shown through `tr`.
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

/// The Position dropdown's entry for `mode`, translated.
fn mode_label(mode: MoonMode) -> String {
    match mode {
        MoonMode::FollowSun => tr!("Follow Sun"),
        MoonMode::Manual => tr!("Manual#moon"),
        MoonMode::DateTime => tr!("Date and Time"),
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
    ui.label(RichText::new(tr!("Moon")).strong());
    super::lighting::grid(ui, "lighting_moon", |ui| {
        let mut ch = ui.checkbox(&mut m.enabled, tr!("Moon")).changed();
        ui.end_row();
        if !m.enabled {
            return ch;
        }
        ch |= row(ui, &tr!("Position"), |ui| {
            let mut changed = false;
            egui::ComboBox::from_id_salt("moon_mode")
                .selected_text(mode_label(m.mode))
                .width(160.0)
                .show_ui(ui, |ui| {
                    for o in MoonMode::ALL {
                        changed |= ui.selectable_value(&mut m.mode, o, mode_label(o)).changed();
                    }
                });
            changed
        });
        match m.mode {
            MoonMode::FollowSun | MoonMode::Manual => {
                ch |= row(ui, &tr!("Phase#moon"), |ui| {
                    let c = slider(ui, &mut m.phase, 0.0..=1.0, "");
                    ui.weak(tr(phase_name(m.phase)));
                    c
                });
                if m.mode == MoonMode::Manual {
                    ch |= row(ui, &tr!("Azimuth"), |ui| {
                        slider(ui, &mut m.azimuth, 0.0..=360.0, "°")
                    });
                    ch |= row(ui, &tr!("Altitude"), |ui| {
                        slider(ui, &mut m.altitude, -10.0..=90.0, "°")
                    });
                }
            }
            MoonMode::DateTime => {
                ch |= row(ui, &tr!("Date"), |ui| {
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
                ch |= row(ui, &tr!("Time#clock"), |ui| {
                    slider(ui, &mut m.time, 0.0..=24.0, " h")
                });
                ch |= row(ui, &tr!("UTC Offset"), |ui| {
                    slider(ui, &mut m.utc_offset, -12.0..=14.0, " h")
                });
                if let Some(sky) = &cx.sky {
                    row(ui, &tr!("Phase#moon"), |ui| {
                        let name = match (sky.illumination, sky.waxing) {
                            (k, _) if k < 0.02 => tr!("New Moon"),
                            (k, _) if k > 0.98 => tr!("Full Moon"),
                            (_, true) => tr!("Waxing"),
                            (_, false) => tr!("Waning"),
                        };
                        let percent = format!("{:.0}", sky.illumination * 100.0);
                        ui.weak(tr!(
                            "{phase}, {percent}% lit",
                            phase = name,
                            percent = percent
                        ));
                        false
                    });
                }
            }
        }
        if m.mode != MoonMode::Manual {
            ch |= row(ui, &tr!("Latitude"), |ui| {
                slider(ui, &mut m.latitude, -90.0..=90.0, "°")
            });
        }
        if m.mode == MoonMode::DateTime {
            ch |= row(ui, &tr!("Longitude"), |ui| {
                slider(ui, &mut m.longitude, -180.0..=180.0, "°")
            });
        }
        ch |= row(ui, &tr!("Size"), |ui| {
            slider(ui, &mut m.size, 0.5..=12.0, "×")
        });
        ch |= row(ui, &tr!("Brightness"), |ui| {
            slider(ui, &mut m.brightness, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Earthshine"), |ui| {
            slider(ui, &mut m.earthshine, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Texture"), |ui| {
            let c = crate::panels::inspector::slots::texture_slot(
                ui,
                "moon_texture",
                &mut m.texture,
                &cx.textures,
            );
            if m.texture.is_none() {
                ui.weak(tr!("Built-in")).on_hover_text(tr!(
                    "Albedo only, evenly lit. 2:1 = whole moon, square = near-side photo."
                ));
            }
            c
        });
        ch |= row(ui, &tr!("Moonlight"), |ui| {
            slider(ui, &mut m.light_intensity, 0.0..=2.0, "")
        });
        ch |= row(ui, &tr!("Moonlight Color"), |ui| {
            color(ui, &mut m.light_color)
        });
        ch
    })
}

/// One collapsible group of Night Sky rows; `id` is its English title, `title` what is shown.
fn group(ui: &mut Ui, id: &str, title: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    egui::CollapsingHeader::new(title)
        .id_salt(("night_sky", id))
        .default_open(crate::capture::lighting_scroll().is_some())
        .show(ui, |ui| {
            super::lighting::grid(ui, &format!("night_{id}"), body)
        })
        .body_returned
        .unwrap_or(false)
}

/// The Night Sky section (stars, Milky Way, light pollution, aurora, meteors); `date_time` =
/// the moon's Date and Time mode sets the sky's turn. True when something changed.
pub(super) fn night_section(ui: &mut Ui, n: &mut NightSkySettings, date_time: bool) -> bool {
    ui.add_space(4.0);
    ui.label(RichText::new(tr!("Night Sky")).strong());
    let mut ch = super::lighting::grid(ui, "lighting_night", |ui| {
        let mut ch = ui.checkbox(&mut n.enabled, tr!("Night Sky")).changed();
        ui.end_row();
        if n.enabled {
            ch |= row(ui, &tr!("Night Sky Darkness"), |ui| {
                slider(ui, &mut n.darkness, 0.0..=1.0, "")
            });
            if !date_time {
                ch |= row(ui, &tr!("Sky Time"), |ui| {
                    slider(ui, &mut n.sky_time, 0.0..=24.0, " h")
                });
            }
        }
        ch
    });
    if !n.enabled {
        return ch;
    }
    ch |= group(ui, "Stars", &tr!("Stars"), |ui| {
        let mut ch = row(ui, &tr!("Brightness"), |ui| {
            slider(ui, &mut n.star_brightness, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Density"), |ui| {
            slider(ui, &mut n.star_density, 0.0..=3.0, "")
        });
        ch |= row(ui, &tr!("Magnitude Spread"), |ui| {
            slider(ui, &mut n.star_magnitude_spread, 0.0..=1.0, "")
        });
        ch |= row(ui, &tr!("Glow"), |ui| {
            slider(ui, &mut n.star_glow, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Color Variation"), |ui| {
            slider(ui, &mut n.star_color_variation, 0.0..=1.0, "")
        });
        ch |= row(ui, &tr!("Twinkle"), |ui| {
            slider(ui, &mut n.star_twinkle, 0.0..=2.0, "")
        });
        ch |= row(ui, &tr!("Twinkle Speed"), |ui| {
            slider(ui, &mut n.star_twinkle_speed, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Fade in Bright Sky"), |ui| {
            slider(ui, &mut n.star_fade, 0.0..=4.0, "")
        });
        ch
    });
    ch |= group(ui, "Milky Way", &tr!("Milky Way"), |ui| {
        let mut ch = row(ui, &tr!("Visibility"), |ui| {
            slider(ui, &mut n.milky_way, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Core#galaxy"), |ui| {
            slider(ui, &mut n.milky_way_core, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Dust Lanes"), |ui| {
            slider(ui, &mut n.milky_way_dust, 0.0..=2.0, "")
        });
        ch |= row(ui, &tr!("Width"), |ui| {
            slider(ui, &mut n.milky_way_width, 0.2..=3.0, "×")
        });
        ch |= row(ui, &tr!("Tint"), |ui| color(ui, &mut n.milky_way_tint));
        ch
    });
    ch |= group(ui, "Light Pollution", &tr!("Light Pollution"), |ui| {
        let mut ch = row(ui, &tr!("Amount"), |ui| {
            slider(ui, &mut n.light_pollution, 0.0..=2.0, "")
        });
        ch |= row(ui, &tr!("Color"), |ui| {
            color(ui, &mut n.light_pollution_color)
        });
        ch
    });
    ch |= group(ui, "Aurora", &tr!("Aurora"), |ui| {
        let mut ch = row(ui, &tr!("Visibility"), |ui| {
            slider(ui, &mut n.aurora, 0.0..=4.0, "")
        });
        ch |= row(ui, &tr!("Color"), |ui| color(ui, &mut n.aurora_color));
        ch |= row(ui, &tr!("Top Color"), |ui| {
            color(ui, &mut n.aurora_top_color)
        });
        ch |= row(ui, &tr!("Height"), |ui| {
            slider(ui, &mut n.aurora_height, 60.0..=300.0, " km")
        });
        ch |= row(ui, &tr!("Thickness"), |ui| {
            slider(ui, &mut n.aurora_thickness, 20.0..=500.0, " km")
        });
        ch |= row(ui, &tr!("Curtains"), |ui| {
            ui.add(egui::Slider::new(&mut n.aurora_curtains, 1..=6))
                .changed()
        });
        ch |= row(ui, &tr!("Waviness"), |ui| {
            slider(ui, &mut n.aurora_waviness, 0.0..=3.0, "")
        });
        ch |= row(ui, &tr!("Ray Detail"), |ui| {
            slider(ui, &mut n.aurora_rays, 0.0..=3.0, "")
        });
        ch |= row(ui, &tr!("Direction"), |ui| {
            slider(ui, &mut n.aurora_azimuth, 0.0..=360.0, "°")
        });
        ch |= row(ui, &tr!("Distance"), |ui| {
            slider(ui, &mut n.aurora_distance, 0.0..=1500.0, " km")
        });
        ch |= row(ui, &tr!("Speed"), |ui| {
            slider(ui, &mut n.aurora_speed, 0.0..=5.0, "")
        });
        ch |= row(ui, &tr!("Pulse#aurora"), |ui| {
            slider(ui, &mut n.aurora_pulse, 0.0..=2.0, "")
        });
        ch |= row(ui, &tr!("Scene Light"), |ui| {
            slider(ui, &mut n.aurora_ambient, 0.0..=4.0, "")
        });
        ch
    });
    ch |= group(ui, "Meteors", &tr!("Meteors"), |ui| {
        let mut ch = row(ui, &tr!("Per Minute"), |ui| {
            slider(ui, &mut n.meteor_rate, 0.0..=60.0, "")
        });
        ch |= row(ui, &tr!("Brightness"), |ui| {
            slider(ui, &mut n.meteor_brightness, 0.0..=4.0, "")
        });
        ch
    });
    ch
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
