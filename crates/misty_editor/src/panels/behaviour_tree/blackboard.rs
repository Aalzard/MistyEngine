//! The Blackboard panel: the tree's keys (name, type, default, shared), and the watched agent's
//! live values while playing.

use egui::{Color32, Rect, RichText, Ui, UiBuilder};
use misty_engine::behaviour_tree::blackboard::{KeyDef, KeyType};

use super::inspector::value_field;
use super::interacting;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::theme;

pub fn height(s: &EditorState) -> f32 {
    let w = &s.behaviour_tree;
    let rows = w.tree.keys.len().max(1) as f32;
    let live = if w.live.is_some() { rows * 20.0 + 30.0 } else { 0.0 };
    70.0 + rows * 50.0 + live
}

pub fn show(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.painter()
        .rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(40, 41, 45, 240));
    let busy = interacting(ui.ctx());
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(8.0)), |ui| {
        let w = &mut s.behaviour_tree;
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr!("Blackboard")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(tr!("＋ Key"), |ui| {
                    for ty in KeyType::ALL {
                        if ui.button(tr(ty.label())).clicked() {
                            let mut name = ty.label().to_owned();
                            let mut k = 2;
                            while w.tree.key(&name).is_some() {
                                name = format!("{}{k}", ty.label());
                                k += 1;
                            }
                            w.edit("Add Key", false, |t| t.keys.push(KeyDef::new(&name, ty)));
                            ui.close();
                        }
                    }
                });
            });
        });
        egui::ScrollArea::vertical()
            .id_salt("bt_blackboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if w.tree.keys.is_empty() {
                    ui.weak(tr!("No keys yet. Keys are the tree's memory: targets, flags, numbers."));
                }
                let mut remove = None;
                for i in 0..w.tree.keys.len() {
                    let before = w.tree.keys[i].clone();
                    let mut k = before.clone();
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut k.name).desired_width(90.0));
                        egui::ComboBox::from_id_salt(("bt_key_ty", i))
                            .selected_text(tr(k.ty.label()))
                            .width(64.0)
                            .show_ui(ui, |ui| {
                                for ty in KeyType::ALL {
                                    if ui.selectable_label(k.ty == ty, tr(ty.label())).clicked() {
                                        k.ty = ty;
                                        k.default = k.default.convert(ty).unwrap_or_else(|| ty.zero());
                                    }
                                }
                            });
                        if ui.small_button("✖").on_hover_text(tr!("Delete key")).clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.weak(tr!("Default"));
                        let ty = k.ty;
                        value_field(ui, &mut k.default, ty);
                        ui.checkbox(&mut k.shared, tr!("Shared"))
                            .on_hover_text(tr!("One value for every agent of the share group"));
                    });
                    ui.separator();
                    if k != before {
                        let renamed = k.name != before.name && !k.name.trim().is_empty();
                        let (old, new) = (before.name.clone(), k.name.clone());
                        w.edit("Edit Key", busy, |t| {
                            if renamed {
                                t.rename_key(&old, &new);
                            }
                            if let Some(x) = t.keys.get_mut(i) {
                                *x = k;
                            }
                        });
                    }
                }
                if let Some(i) = remove {
                    w.edit("Delete Key", false, |t| {
                        if i < t.keys.len() {
                            t.keys.remove(i);
                        }
                    });
                }
                if let Some(live) = &w.live {
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr!("Live values")).strong());
                    egui::Grid::new("bt_live_keys").num_columns(2).show(ui, |ui| {
                        for (name, _ty, value, shared) in &live.keys {
                            let label = if *shared {
                                format!("{name} ⇄")
                            } else {
                                name.clone()
                            };
                            ui.label(RichText::new(label).color(theme::TEXT_DIM));
                            ui.label(crate::i18n::display(value));
                            ui.end_row();
                        }
                    });
                }
            });
    });
}
