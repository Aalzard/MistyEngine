//! Project Settings ▸ Input Manager: the axes list (one foldout per axis).

use egui::Ui;

use misty_engine::input::axes::{JOYSTICK_AXES, MAX_JOYSTICKS, axis_label};
use misty_engine::input::{AxisType, InputAxis, InputSettings};

/// Draw the page; true when something changed.
pub fn page(ui: &mut Ui, input: &mut InputSettings) -> bool {
    let mut edited = false;
    egui::CollapsingHeader::new("Axes")
        .id_salt("input_axes")
        .default_open(true)
        .show(ui, |ui| {
            let mut size = input.axes.len();
            edited |= field(ui, "Size", "Number of axis entries.", |ui| {
                ui.add(egui::DragValue::new(&mut size).range(0..=200))
                    .changed()
            });
            if size != input.axes.len() {
                // New entries copy the last one, like growing a list in the Inspector.
                let template = input.axes.last().cloned().unwrap_or_default();
                input.axes.resize(size, template);
                edited = true;
            }
            let mut remove = None;
            let mut duplicate = None;
            for (i, axis) in input.axes.iter_mut().enumerate() {
                let title = if axis.name.is_empty() {
                    format!("Axis {i}")
                } else {
                    axis.name.clone()
                };
                egui::CollapsingHeader::new(title)
                    .id_salt(("input_axis", i))
                    .show(ui, |ui| {
                        edited |= axis_fields(ui, axis);
                        ui.horizontal(|ui| {
                            if ui.small_button("Duplicate").clicked() {
                                duplicate = Some(i);
                            }
                            if ui.small_button("Delete").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
            }
            if let Some(i) = duplicate {
                let copy = input.axes[i].clone();
                input.axes.insert(i + 1, copy);
                edited = true;
            }
            if let Some(i) = remove {
                input.axes.remove(i);
                edited = true;
            }
        });
    edited
}

/// Label + widget row.
fn field(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([170.0, 18.0], egui::Label::new(label).truncate())
            .on_hover_text(tip);
        add(ui)
    })
    .inner
}

fn text(ui: &mut Ui, label: &str, tip: &str, v: &mut String) -> bool {
    field(ui, label, tip, |ui| {
        ui.add(egui::TextEdit::singleline(v).desired_width(180.0))
            .changed()
    })
}

fn number(ui: &mut Ui, label: &str, tip: &str, v: &mut f32) -> bool {
    field(ui, label, tip, |ui| {
        ui.add(
            egui::DragValue::new(v)
                .speed(0.01)
                .range(0.0..=f32::MAX)
                .max_decimals(3),
        )
        .changed()
    })
}

const BUTTON_TIP: &str = "Key name (\"a\", \"left shift\", \"space\"), \"mouse 0\", \"joystick button 0\" or \
     \"joystick 1 button 0\".";

fn axis_fields(ui: &mut Ui, a: &mut InputAxis) -> bool {
    let mut ch = text(
        ui,
        "Name",
        "Used by Input.GetAxis / GetButton.",
        &mut a.name,
    );
    ch |= text(ui, "Descriptive Name", "", &mut a.descriptive_name);
    ch |= text(
        ui,
        "Descriptive Negative Name",
        "",
        &mut a.descriptive_negative_name,
    );
    ch |= text(ui, "Negative Button", BUTTON_TIP, &mut a.negative_button);
    ch |= text(ui, "Positive Button", BUTTON_TIP, &mut a.positive_button);
    ch |= text(
        ui,
        "Alt Negative Button",
        BUTTON_TIP,
        &mut a.alt_negative_button,
    );
    ch |= text(
        ui,
        "Alt Positive Button",
        BUTTON_TIP,
        &mut a.alt_positive_button,
    );
    ch |= number(
        ui,
        "Gravity",
        "Units per second the axis falls back to 0 when no button is held.",
        &mut a.gravity,
    );
    ch |= number(
        ui,
        "Dead",
        "Analog values smaller than this read as 0.",
        &mut a.dead,
    );
    ch |= number(
        ui,
        "Sensitivity",
        "Units per second towards the target (keys), or a multiplier (mouse, joystick).",
        &mut a.sensitivity,
    );
    ch |= field(
        ui,
        "Snap",
        "Pressing the opposite direction jumps to 0 first.",
        |ui| ui.checkbox(&mut a.snap, "").changed(),
    );
    ch |= field(ui, "Invert", "Swap positive and negative.", |ui| {
        ui.checkbox(&mut a.invert, "").changed()
    });
    ch |= field(ui, "Type", "", |ui| {
        let mut changed = false;
        egui::ComboBox::from_id_salt("axis_type")
            .selected_text(a.kind.label())
            .width(180.0)
            .show_ui(ui, |ui| {
                for k in AxisType::ALL {
                    changed |= ui.selectable_value(&mut a.kind, k, k.label()).changed();
                }
            });
        changed
    });
    ch |= field(
        ui,
        "Axis",
        "Mouse: X, Y, wheel. Joystick: see the docs.",
        |ui| {
            let mut changed = false;
            egui::ComboBox::from_id_salt("axis_index")
                .selected_text(axis_label(a.axis))
                .width(180.0)
                .show_ui(ui, |ui| {
                    for i in 0..JOYSTICK_AXES {
                        changed |= ui.selectable_value(&mut a.axis, i, axis_label(i)).changed();
                    }
                });
            changed
        },
    );
    ch |= field(
        ui,
        "Joy Num",
        "Which joystick feeds a Joystick Axis.",
        |ui| {
            let mut changed = false;
            egui::ComboBox::from_id_salt("axis_joy")
                .selected_text(joy_label(a.joy_num))
                .width(180.0)
                .show_ui(ui, |ui| {
                    for j in 0..=MAX_JOYSTICKS {
                        changed |= ui
                            .selectable_value(&mut a.joy_num, j, joy_label(j))
                            .changed();
                    }
                });
            changed
        },
    );
    ch
}

fn joy_label(j: usize) -> String {
    if j == 0 {
        "Get Motion from all Joysticks".into()
    } else {
        format!("Joystick {j}")
    }
}
