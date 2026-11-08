//! The Animator window's Layers list: every layer with its weight and badges (Mask, Additive,
//! Synced); clicking one shows its state machine. "+" adds a layer; the selected layer shows its
//! settings (name, blending, Avatar Mask, sync source, delete). In Play mode the weights are the
//! live ones and dragging them changes the running Animator.

use egui::{Color32, RichText, Ui};
use misty_engine::animation::AnimatorController;
use misty_engine::animation::animator::AnimatorStatus;
use misty_engine::animation::controller::LayerBlending;
use misty_engine::scene::clip_name;

use super::MASK_ICON;
use crate::anim_controllers::{self, interacting};
use crate::state::EditorState;
use crate::theme;

pub fn show(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    c: &AnimatorController,
    live: Option<&AnimatorStatus>,
) {
    let busy = interacting(ui.ctx());
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+").on_hover_text("Add layer").clicked() {
                let mut index = 0;
                anim_controllers::edit(s, path, "Add Layer", false, |c| {
                    index = c.add_layer("New Layer");
                });
                s.animator.show_layer(index);
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("animator_layers")
        .auto_shrink(false)
        .show(ui, |ui| {
            for i in 0..c.layer_count() {
                row(ui, s, path, c, i, live, busy);
                ui.add_space(2.0);
            }
        });
}

fn row(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    c: &AnimatorController,
    i: usize,
    live: Option<&AnimatorStatus>,
    busy: bool,
) {
    let selected = s.animator.layer == i;
    let fill = if selected {
        Color32::from_rgb(44, 70, 104)
    } else {
        theme::PANEL_BG.gamma_multiply(1.3)
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let name = c.layer_name(i).to_owned();
                if ui
                    .selectable_label(selected, RichText::new(&name).strong())
                    .clicked()
                {
                    s.animator.show_layer(i);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(l) = c.layer(i) {
                        if l.sync_source.is_some() {
                            ui.label(RichText::new("S").color(theme::TEXT_DIM))
                                .on_hover_text("Synced");
                        }
                        if l.blending == LayerBlending::Additive {
                            ui.label(RichText::new("A").color(theme::TEXT_DIM))
                                .on_hover_text("Additive");
                        }
                        if l.mask.is_some() {
                            ui.label(RichText::new("M").color(theme::TEXT_DIM))
                                .on_hover_text("Avatar Mask");
                        }
                    }
                });
            });
            // Weight: the base layer always has weight 1.
            let live_w = live.and_then(|l| l.layers.get(i)).map(|l| l.weight);
            let mut w = live_w.unwrap_or_else(|| c.layer(i).map_or(1.0, |l| l.weight));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Weight").color(theme::TEXT_DIM));
                let r = ui.add_enabled(i > 0, egui::Slider::new(&mut w, 0.0..=1.0).max_decimals(2));
                if r.changed() {
                    set_weight(s, path, i, w, live_w.is_some(), busy);
                }
            });
            // The selected layer's settings.
            if selected && i > 0 {
                ui.separator();
                settings(ui, s, path, c, i, busy);
            }
        });
}

fn set_weight(s: &mut EditorState, path: &str, i: usize, w: f32, live: bool, busy: bool) {
    if live {
        if let (Some(rt), Some(e)) = (s.runtime.as_mut(), s.animator.target) {
            rt.set_animator_layer_weight(&s.scene, e, i, w);
        }
        return;
    }
    anim_controllers::edit(s, path, "Layer Weight", busy, |c| {
        if let Some(l) = c.layer_mut(i) {
            l.weight = w;
        }
    });
}

/// Settings of the selected layer `i` ≥ 1.
fn settings(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    c: &AnimatorController,
    i: usize,
    busy: bool,
) {
    let Some(mut l) = c.layer(i).cloned() else {
        return;
    };
    let masks: Vec<String> = s.assets.masks.iter().cloned().collect();
    let mut changed = false;
    let mut rename = None;
    egui::Grid::new(("layer_settings", i))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Name");
            let mut name = l.name.clone();
            if ui.text_edit_singleline(&mut name).changed() {
                rename = Some(name);
            }
            ui.end_row();
            ui.label("Mask");
            let none = "None (Avatar Mask)";
            egui::ComboBox::from_id_salt(("layer_mask", i))
                .selected_text(match &l.mask {
                    Some(m) => format!("{MASK_ICON} {}", clip_name(m)),
                    None => none.to_owned(),
                })
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut l.mask, None, none).changed();
                    for m in &masks {
                        changed |= ui
                            .selectable_value(&mut l.mask, Some(m.clone()), clip_name(m))
                            .changed();
                    }
                });
            ui.end_row();
            ui.label("Blending");
            egui::ComboBox::from_id_salt(("layer_blending", i))
                .selected_text(l.blending.label())
                .show_ui(ui, |ui| {
                    for b in LayerBlending::ALL {
                        changed |= ui.selectable_value(&mut l.blending, b, b.label()).changed();
                    }
                });
            ui.end_row();
            ui.label("Sync");
            let sources: Vec<usize> = (0..c.layer_count())
                .filter(|&j| j != i && c.layer(j).is_none_or(|x| x.sync_source.is_none()))
                .collect();
            egui::ComboBox::from_id_salt(("layer_sync", i))
                .selected_text(l.sync_source.map_or("None", |j| c.layer_name(j)))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut l.sync_source, None, "None")
                        .changed();
                    for j in sources {
                        changed |= ui
                            .selectable_value(&mut l.sync_source, Some(j), c.layer_name(j))
                            .changed();
                    }
                });
            ui.end_row();
        });
    if changed {
        anim_controllers::edit(s, path, "Layer Settings", busy, |c| {
            if let Some(x) = c.layer_mut(i) {
                let name = x.name.clone();
                *x = l.clone();
                x.name = name;
            }
        });
    }
    if let Some(new) = rename {
        anim_controllers::edit(s, path, "Rename Layer", true, |c| {
            c.rename_layer(i, &new);
        });
    }
    if ui.small_button("Delete Layer").clicked() {
        anim_controllers::edit(s, path, "Delete Layer", false, |c| c.remove_layer(i));
        s.animator.show_layer(0);
    }
}
