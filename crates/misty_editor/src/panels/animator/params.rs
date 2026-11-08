//! The Animator window's Parameters list: add (+ ▸ Float / Int / Bool / Trigger), rename
//! (double-click), delete (right-click), and each value. In Edit mode values are the defaults
//! saved in the controller; in Play mode they are the live values (editable too).

use egui::Ui;
use misty_engine::animation::AnimatorController;
use misty_engine::animation::animator::AnimatorStatus;
use misty_engine::animation::controller::ParamKind;

use crate::anim_controllers::{self, interacting};
use crate::state::EditorState;

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
            ui.menu_button("+", |ui| {
                for kind in ParamKind::ALL {
                    if ui.button(kind.label()).clicked() {
                        let mut name = String::new();
                        anim_controllers::edit(s, path, "Add Parameter", false, |c| {
                            name = c.add_parameter(kind);
                        });
                        // New parameters start renaming, like a fresh asset.
                        s.animator.renaming = Some((name.clone(), name));
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Add parameter");
        });
    });
    ui.separator();
    if c.parameters.is_empty() {
        ui.weak("List is Empty");
    }
    egui::ScrollArea::vertical()
        .id_salt("animator_params")
        .auto_shrink(false)
        .show(ui, |ui| {
            for p in &c.parameters {
                ui.horizontal(|ui| row(ui, s, path, p, live, busy));
            }
        });
}

fn row(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    p: &misty_engine::animation::controller::Parameter,
    live: Option<&AnimatorStatus>,
    busy: bool,
) {
    let width = ui.available_width();
    // Name: a label, or a text field while renaming.
    let renaming = s
        .animator
        .renaming
        .as_ref()
        .is_some_and(|(n, _)| *n == p.name);
    if renaming {
        let (_, text) = s.animator.renaming.as_mut().expect("renaming");
        let r = ui.add(egui::TextEdit::singleline(text).desired_width(width - 90.0));
        if !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        if r.lost_focus() {
            let (old, new) = s.animator.renaming.take().expect("renaming");
            if new != old {
                anim_controllers::edit(s, path, "Rename Parameter", false, |c| {
                    c.rename_parameter(&old, &new);
                });
            }
        }
    } else {
        let label = ui
            .add_sized(
                [width - 90.0, 18.0],
                egui::Label::new(&p.name)
                    .sense(egui::Sense::click())
                    .truncate(),
            )
            .on_hover_text(p.kind.label());
        if label.double_clicked() {
            s.animator.renaming = Some((p.name.clone(), p.name.clone()));
        }
        label.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                s.animator.renaming = Some((p.name.clone(), p.name.clone()));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                let name = p.name.clone();
                anim_controllers::edit(s, path, "Delete Parameter", false, |c| {
                    c.remove_parameter(&name)
                });
                ui.close();
            }
        });
    }
    // Value: the live one while playing, else the default.
    let live_value = live.and_then(|st| {
        st.params
            .iter()
            .find(|(n, _, _)| *n == p.name)
            .map(|(_, _, v)| *v)
    });
    let mut v = live_value.unwrap_or(p.default);
    let changed = match p.kind {
        ParamKind::Float => ui
            .add(egui::DragValue::new(&mut v).speed(0.01).max_decimals(3))
            .changed(),
        ParamKind::Int => {
            let mut i = v.round() as i32;
            let ch = ui.add(egui::DragValue::new(&mut i)).changed();
            v = i as f32;
            ch
        }
        ParamKind::Bool | ParamKind::Trigger => {
            let mut on = v >= 0.5;
            let r = if p.kind == ParamKind::Trigger {
                ui.radio(on, "").on_hover_text("Trigger")
            } else {
                ui.checkbox(&mut on, "")
            };
            if p.kind == ParamKind::Trigger && r.clicked() {
                on = !on;
            }
            v = if on { 1.0 } else { 0.0 };
            r.clicked() || r.changed()
        }
    };
    if !changed {
        return;
    }
    if live_value.is_some() {
        if let (Some(rt), Some(e)) = (s.runtime.as_mut(), s.animator.target) {
            rt.set_animator_param(&s.scene, e, &p.name, v);
        }
    } else {
        let name = p.name.clone();
        anim_controllers::edit(s, path, "Parameter Default", busy, |c| {
            if let Some(q) = c.parameters.iter_mut().find(|q| q.name == name) {
                q.default = v;
            }
        });
    }
}
