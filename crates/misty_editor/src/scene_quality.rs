//! Scene view Quality dropdown: a quick way to the previewed Project Settings ▸ Quality level
//! (level, FXAA / TAA, Upscaling, Render Scale, Upscale Filter). It edits the same values the
//! settings page does, so the Game view and built players use them too.

use egui::Ui;
use misty_engine::quality::{self, QualitySettings, UpscaleFilter, Upscaling};

/// The dropdown body. Switching the level previews it at once (like the Game view's picker);
/// true when a level's values were edited (the caller saves them).
pub fn menu(ui: &mut Ui, q: &mut QualitySettings) -> bool {
    ui.set_min_width(240.0);
    let current = quality::level().min(q.levels.len().saturating_sub(1));
    let mut edited = false;
    egui::Grid::new("scene_quality_menu")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Quality Level");
            egui::ComboBox::from_id_salt("scene_quality_level")
                .selected_text(q.levels.get(current).map_or("", |l| l.name.as_str()))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (i, l) in q.levels.iter().enumerate() {
                        if ui.selectable_label(i == current, &l.name).clicked() {
                            quality::install(q, i);
                        }
                    }
                });
            ui.end_row();
            let Some(level) = q.levels.get_mut(current) else {
                return;
            };
            ui.label("Anti-aliasing");
            edited |= ui
                .checkbox(&mut level.fxaa, "FXAA")
                .on_hover_text("Post-process edge smoothing on top of the renderer's 4x MSAA")
                .changed();
            edited |= ui
                .checkbox(&mut level.temporal_aa, "TAA")
                .on_hover_text("Temporal anti-aliasing: smooth, stable grass and thin details")
                .changed();
            ui.end_row();
            ui.label("Upscaling");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("scene_upscaling")
                    .selected_text(level.upscaling.label())
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for u in Upscaling::ALL {
                            edited |= ui
                                .selectable_value(&mut level.upscaling, u, u.label())
                                .changed();
                        }
                    })
                    .response
                    .on_hover_text(
                        "Temporal upscaling: render smaller, rebuild the full resolution from \
                         jittered frames",
                    );
                if level.upscaling == Upscaling::Custom {
                    edited |= ui
                        .add(
                            egui::DragValue::new(&mut level.upscaling_scale)
                                .range(quality::UPSCALING_MIN..=1.0)
                                .speed(0.01)
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        )
                        .on_hover_text("Share of the resolution rendered per side")
                        .changed();
                }
            });
            ui.end_row();
            ui.label("Render Scale");
            edited |= ui
                .add(
                    egui::Slider::new(&mut level.render_scale, 0.25..=2.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                        .custom_parser(|t| {
                            t.trim()
                                .trim_end_matches('%')
                                .parse::<f64>()
                                .ok()
                                .map(|v| v / 100.0)
                        }),
                )
                .on_hover_text(
                    "Renders the 3D view at this fraction of its size, then stretches it",
                )
                .changed();
            ui.end_row();
            ui.label("Upscale Filter");
            egui::ComboBox::from_id_salt("scene_upscale_filter")
                .selected_text(level.upscale_filter.label())
                .width(120.0)
                .show_ui(ui, |ui| {
                    for f in UpscaleFilter::ALL {
                        edited |= ui
                            .selectable_value(&mut level.upscale_filter, f, f.label())
                            .changed();
                    }
                });
            ui.end_row();
        });
    edited
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn menu_shows_the_four_controls() {
        let mut h = Harness::new_ui_state(
            |ui, q: &mut misty_engine::quality::QualitySettings| {
                super::menu(ui, q);
            },
            misty_engine::quality::QualitySettings::default(),
        );
        h.run();
        for label in [
            "Quality Level",
            "Anti-aliasing",
            "Upscaling",
            "Render Scale",
            "Upscale Filter",
        ] {
            h.get_by_label(label);
        }
    }
}
