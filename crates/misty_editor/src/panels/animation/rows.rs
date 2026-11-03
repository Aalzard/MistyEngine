//! Property list rows: one per animated property, grouped ("Cube : Position" with x/y/z below).

use egui::{Color32, RichText, Sense, Ui};
use misty_engine::animation::{AnimationClip, Property, binding};
use misty_engine::hecs::Entity;

use super::{ROW_H, in_row, preview, row_rect};
use crate::state::EditorState;
use crate::theme;

pub struct Row {
    pub label: String,
    pub path: String,
    pub group: &'static str,
    /// Curves this row shows keys for (all members for a group header).
    pub curves: Vec<usize>,
    /// Foldable header of a multi-property group.
    pub header: bool,
    /// Member of a group (indented).
    pub child: bool,
    /// The object at `path` does not exist.
    pub missing: bool,
}

/// Rows for the clip's curves, grouped by object path and property group.
pub fn build_rows(s: &EditorState, root: Entity, clip: &AnimationClip) -> Vec<Row> {
    let ordinal = |p: Property| Property::ALL.iter().position(|q| *q == p).unwrap_or(0);
    let mut order: Vec<usize> = (0..clip.curves.len()).collect();
    order.sort_by_key(|&i| {
        (
            clip.curves[i].path.clone(),
            ordinal(clip.curves[i].property),
        )
    });
    let mut rows = Vec::new();
    let mut i = 0;
    while i < order.len() {
        let first = &clip.curves[order[i]];
        let (path, group) = (first.path.clone(), first.property.group());
        let members: Vec<usize> = order[i..]
            .iter()
            .copied()
            .take_while(|&c| {
                clip.curves[c].path == path && clip.curves[c].property.group() == group
            })
            .collect();
        i += members.len();
        let missing = binding::resolve(&s.scene, root, &path).is_none();
        let object = match path.rsplit('/').next() {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => s.scene.name(root),
        };
        let single = members.len() == 1;
        let label = if single {
            format!("{object} : {}", clip.curves[members[0]].property.label())
        } else {
            format!("{object} : {group}")
        };
        rows.push(Row {
            label: if missing {
                format!("{label} (Missing!)")
            } else {
                label
            },
            path: path.clone(),
            group,
            curves: members.clone(),
            header: !single,
            child: false,
            missing,
        });
        if !single && !s.animation.collapsed.contains(&(path.clone(), group)) {
            for c in members {
                rows.push(Row {
                    label: clip.curves[c].property.label(),
                    path: path.clone(),
                    group,
                    curves: vec![c],
                    header: false,
                    child: true,
                    missing,
                });
            }
        }
    }
    rows
}

/// Draw the rows below the toolbar: fold arrow, name and the current value (editing it keys it).
pub fn property_list(
    ui: &mut Ui,
    s: &mut EditorState,
    root: Entity,
    clip: &AnimationClip,
    rows: &[Row],
    left: egui::Rect,
) {
    let footer_top = left.bottom() - ROW_H - 4.0;
    for (i, row) in rows.iter().enumerate() {
        let rect = row_rect(left, i + 2);
        if rect.bottom() > footer_top {
            break;
        }
        let focused = row
            .curves
            .iter()
            .all(|c| s.animation.focused_curves.contains(c));
        let bg = if focused {
            theme::SELECTION
        } else if i % 2 == 0 {
            Color32::from_rgb(58, 58, 58)
        } else {
            Color32::from_rgb(54, 54, 54)
        };
        ui.painter().rect_filled(rect, 0.0, bg);
        let resp = ui.interact(rect, ui.id().with(("anim_row", i)), Sense::click());
        if resp.clicked() {
            let all = &mut s.animation.focused_curves;
            if focused {
                row.curves.iter().for_each(|c| {
                    all.remove(c);
                });
            } else {
                if !ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                    all.clear();
                }
                all.extend(row.curves.iter().copied());
            }
        }
        resp.context_menu(|ui| {
            if ui.button("Add Key").clicked() {
                for &c in &row.curves {
                    let value = current_value(s, root, clip, c);
                    preview::key_curve(s, c, value);
                }
                ui.close();
            }
            if ui.button("Remove Properties").clicked() {
                preview::remove_curves(s, &row.curves);
                ui.close();
            }
        });
        in_row(ui, rect, |ui| {
            ui.add_space(if row.child { 22.0 } else { 0.0 });
            if row.header {
                let key = (row.path.clone(), row.group);
                let open = !s.animation.collapsed.contains(&key);
                if ui
                    .add(egui::Button::new(if open { "⏷" } else { "⏵" }).frame(false))
                    .clicked()
                {
                    if open {
                        s.animation.collapsed.insert(key);
                    } else {
                        s.animation.collapsed.remove(&key);
                    }
                }
            }
            let color = if row.missing {
                Color32::from_rgb(230, 190, 60)
            } else {
                theme::TEXT
            };
            ui.add(
                egui::Label::new(RichText::new(&row.label).color(color))
                    .truncate()
                    .selectable(false),
            );
            if !row.header && !row.missing {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    value_field(ui, s, root, clip, row.curves[0]);
                });
            }
        });
    }
}

/// The property's value right now (from the scene, or the curve if the object is missing).
fn current_value(s: &EditorState, root: Entity, clip: &AnimationClip, c: usize) -> f32 {
    let curve = &clip.curves[c];
    binding::resolve(&s.scene, root, &curve.path)
        .and_then(|e| binding::read(&s.scene, e, curve.property))
        .unwrap_or_else(|| curve.evaluate(s.animation.time))
}

fn value_field(ui: &mut Ui, s: &mut EditorState, root: Entity, clip: &AnimationClip, c: usize) {
    let property = clip.curves[c].property;
    let mut v = current_value(s, root, clip, c);
    // Animated values are tinted (red while recording), like animated Inspector fields.
    let tint = if s.animation.recording {
        super::RECORD_RED
    } else {
        Color32::from_rgb(80, 140, 220)
    };
    let changed = if property == Property::Active {
        let mut on = v >= 0.5;
        let r = ui.checkbox(&mut on, "");
        v = if on { 1.0 } else { 0.0 };
        r.changed()
    } else {
        let r = ui.add(
            egui::DragValue::new(&mut v)
                .speed(0.05)
                .max_decimals(3)
                .custom_formatter(|n, _| format!("{n:.3}")),
        );
        ui.painter()
            .hline(r.rect.x_range(), r.rect.bottom(), (1.0, tint));
        if r.changed() && r.dragged() && !r.drag_started() {
            // Later frames of the same drag update the key without new undo steps.
            set_key_value(s, c, v);
            return;
        }
        r.changed()
    };
    if changed {
        preview::key_curve(s, c, v);
    }
}

fn set_key_value(s: &mut EditorState, c: usize, v: f32) {
    let (Some(path), time) = (s.animation.clip.clone(), s.animation.time) else {
        return;
    };
    if let Some(clip) = s.anim_clips.get_mut(&path) {
        let t = clip.snap(time);
        if let Some(curve) = clip.curves.get_mut(c) {
            curve.keys.set(t, v);
        }
        clip.fit_length();
    }
}
