//! Scene view icon gizmos (cameras, lights, audio…): size and tint, set from the Gizmos ▾
//! dropdown and saved per project in `Library/EditorSettings.json`.

use egui::{Color32, Ui};
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Icon side in points at size 1×.
pub const BASE_PX: f32 = 24.0;
/// Icon Size slider range (multiplier).
pub const SIZE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.0;
/// With 3D Icons on, icons are full size up to this distance and shrink beyond it.
const FULL_SIZE_DISTANCE: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IconSettings {
    /// Multiplier over `BASE_PX`.
    pub size: f32,
    /// Tint multiplied over the white icons (sRGB).
    pub color: [u8; 3],
    /// Icons shrink with distance from the Scene camera.
    pub three_d: bool,
}

impl Default for IconSettings {
    fn default() -> Self {
        Self {
            size: 1.0,
            color: [255; 3],
            three_d: false,
        }
    }
}

impl IconSettings {
    /// Colour of an unselected icon: the default light grey times the tint.
    pub fn tint(&self) -> Color32 {
        let [r, g, b] = self.color;
        Color32::from_gray(235) * Color32::from_rgb(r, g, b)
    }

    /// Icon side in points for an object at `at`, seen from `eye`.
    pub fn side(&self, eye: Vec3, at: Vec3) -> f32 {
        let size = self.size.clamp(*SIZE_RANGE.start(), *SIZE_RANGE.end());
        let falloff = if self.three_d {
            (FULL_SIZE_DISTANCE / eye.distance(at).max(1e-3)).clamp(0.25, 1.0)
        } else {
            1.0
        };
        BASE_PX * size * falloff
    }
}

/// The Gizmos ▾ dropdown body.
pub fn menu(ui: &mut Ui, s: &mut IconSettings) {
    ui.set_min_width(210.0);
    egui::Grid::new("scene_icon_settings")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Icon Size");
            ui.add(egui::Slider::new(&mut s.size, SIZE_RANGE).suffix("×"));
            ui.end_row();
            ui.label("Icon Color");
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut s.color);
                if ui
                    .add_enabled(s.color != [255; 3], egui::Button::new("Reset"))
                    .clicked()
                {
                    s.color = [255; 3];
                }
            });
            ui.end_row();
            ui.label("3D Icons");
            ui.checkbox(&mut s.three_d, "")
                .on_hover_text("Icons shrink with distance");
            ui.end_row();
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_tint_keeps_the_default_grey_and_yellow_tints() {
        let mut s = IconSettings::default();
        assert_eq!(s.tint(), Color32::from_gray(235));
        s.color = [255, 255, 0];
        let t = s.tint();
        assert!(t.r() > 200 && t.g() > 200 && t.b() == 0, "{t:?}");
    }

    #[test]
    fn size_scales_and_3d_icons_shrink_with_distance() {
        let mut s = IconSettings {
            size: 2.0,
            ..Default::default()
        };
        assert_eq!(s.side(Vec3::ZERO, Vec3::Z * 100.0), 48.0);
        s.three_d = true;
        assert_eq!(s.side(Vec3::ZERO, Vec3::Z * 5.0), 48.0);
        assert!(s.side(Vec3::ZERO, Vec3::Z * 20.0) < 48.0);
    }

    #[test]
    fn old_settings_files_load_with_defaults() {
        let s: IconSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, IconSettings::default());
    }
}
