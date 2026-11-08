//! The event tree: pass groups (collapsible, with event counts) and their events.

use std::collections::HashSet;

use egui::{Align2, Color32, FontId, Sense, Ui, vec2};
use misty_engine::render::{EventKind, FrameCapture, FrameEvent};

use crate::theme;

/// One visible row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A pass group: path key, name, nesting, events inside and its last event.
    Pass {
        key: String,
        name: String,
        depth: usize,
        events: usize,
        last: usize,
    },
    /// An event (index into the capture) at a nesting depth.
    Event { index: usize, depth: usize },
}

/// Rows in frame order. A group is a run of consecutive events sharing a pass prefix; groups
/// in `collapsed` hide what they contain. A non-empty `filter` lists matching events only
/// (flat, case-insensitive on the name, object, mesh and material).
pub fn rows(events: &[FrameEvent], collapsed: &HashSet<String>, filter: &str) -> Vec<Row> {
    let filter = filter.trim().to_lowercase();
    if !filter.is_empty() {
        return events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                [
                    Some(&e.name),
                    e.object.as_ref(),
                    e.mesh.as_ref(),
                    e.material.as_ref(),
                ]
                .into_iter()
                .flatten()
                .any(|t| t.to_lowercase().contains(&filter))
            })
            .map(|(index, _)| Row::Event { index, depth: 0 })
            .collect();
    }
    let mut out = Vec::new();
    let mut current: &[String] = &[];
    for (i, e) in events.iter().enumerate() {
        let common = current
            .iter()
            .zip(&e.pass)
            .take_while(|(a, b)| a == b)
            .count();
        for d in common..e.pass.len() {
            let prefix = &e.pass[..=d];
            let run = events[i..]
                .iter()
                .take_while(|x| x.pass.len() > d && x.pass[..=d] == *prefix)
                .count();
            if !hidden(&e.pass[..d], collapsed) {
                out.push(Row::Pass {
                    key: prefix.join(" ▸ "),
                    name: e.pass[d].clone(),
                    depth: d,
                    events: run,
                    last: i + run - 1,
                });
            }
        }
        current = &e.pass;
        if !hidden(&e.pass, collapsed) {
            out.push(Row::Event {
                index: i,
                depth: e.pass.len(),
            });
        }
    }
    out
}

/// Some group on `path` (or the path itself) is collapsed.
fn hidden(path: &[String], collapsed: &HashSet<String>) -> bool {
    (1..=path.len()).any(|n| collapsed.contains(&path[..n].join(" ▸ ")))
}

pub fn kind_icon(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Clear => "⊘",
        EventKind::Draw => "▲",
        EventKind::DrawIndirect => "◭",
        EventKind::Fullscreen => "▣",
        EventKind::Compute => "⚙",
        EventKind::Copy => "⎘",
        EventKind::Ui => "▤",
    }
}

const ROW_H: f32 = 20.0;

/// Draw the rows; returns the clicked event and the group whose arrow was toggled.
pub fn show(
    ui: &mut Ui,
    capture: &FrameCapture,
    rows: &[Row],
    selected: usize,
    collapsed: &HashSet<String>,
    scroll_to_selected: bool,
) -> (Option<usize>, Option<String>) {
    let (mut clicked, mut toggled) = (None, None);
    egui::ScrollArea::vertical()
        .id_salt("frame_debugger_tree")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for row in rows {
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                let label = match row {
                    Row::Pass { name, events, .. } => format!("{name} ({events})"),
                    Row::Event { index, .. } => capture.events[*index].name.clone(),
                };
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
                });
                let painter = ui.painter_at(rect);
                let (depth, is_selected, replayed) = match row {
                    Row::Pass { depth, last, .. } => (*depth, false, *last <= selected),
                    Row::Event { index, depth } => (*depth, *index == selected, *index <= selected),
                };
                let fill = if is_selected {
                    theme::SELECTION
                } else if resp.hovered() {
                    Color32::from_gray(66)
                } else {
                    Color32::TRANSPARENT
                };
                painter.rect_filled(rect, 4.0, fill);
                let x = rect.left() + 6.0 + depth as f32 * 14.0;
                let text_color = match (is_selected, replayed) {
                    (true, _) => Color32::WHITE,
                    (false, true) => theme::TEXT,
                    (false, false) => theme::TEXT_DIM,
                };
                match row {
                    Row::Pass {
                        key, name, events, ..
                    } => {
                        let open = !collapsed.contains(key);
                        painter.text(
                            rect.left_center() + vec2(x - rect.left(), 0.0),
                            Align2::LEFT_CENTER,
                            if open { "⏷" } else { "⏵" },
                            FontId::proportional(11.0),
                            theme::TEXT_DIM,
                        );
                        painter.text(
                            rect.left_center() + vec2(x - rect.left() + 14.0, 0.0),
                            Align2::LEFT_CENTER,
                            name,
                            FontId::proportional(13.0),
                            text_color,
                        );
                        // Count badge.
                        let badge = format!("{events}");
                        let galley =
                            painter.layout_no_wrap(badge, FontId::monospace(11.0), theme::TEXT_DIM);
                        let size = galley.size() + vec2(10.0, 2.0);
                        let at = egui::Rect::from_min_size(
                            rect.right_center() - vec2(size.x + 6.0, size.y / 2.0),
                            size,
                        );
                        painter.rect_filled(at, 7.0, Color32::from_gray(48));
                        painter.galley(at.min + vec2(5.0, 1.0), galley, theme::TEXT_DIM);
                        if resp.clicked() {
                            let arrow = rect.left() + depth as f32 * 14.0 + 20.0;
                            match resp.interact_pointer_pos().is_some_and(|p| p.x < arrow) {
                                true => toggled = Some(key.clone()),
                                false => clicked = Some(row_last(row)),
                            }
                        }
                        if resp.double_clicked() {
                            toggled = Some(key.clone());
                        }
                    }
                    Row::Event { index, .. } => {
                        let e = &capture.events[*index];
                        painter.text(
                            rect.left_center() + vec2(x - rect.left(), 0.0),
                            Align2::LEFT_CENTER,
                            kind_icon(e.kind()),
                            FontId::proportional(10.0),
                            kind_color(e.kind()),
                        );
                        painter.text(
                            rect.left_center() + vec2(x - rect.left() + 16.0, 0.0),
                            Align2::LEFT_CENTER,
                            &e.name,
                            FontId::proportional(13.0),
                            text_color,
                        );
                        if e.instances > 1 {
                            painter.text(
                                rect.right_center() - vec2(8.0, 0.0),
                                Align2::RIGHT_CENTER,
                                format!("×{}", e.instances),
                                FontId::monospace(11.0),
                                theme::TEXT_DIM,
                            );
                        }
                        if resp.clicked() {
                            clicked = Some(*index);
                        }
                        if is_selected && scroll_to_selected {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                }
            }
        });
    (clicked, toggled)
}

fn row_last(row: &Row) -> usize {
    match row {
        Row::Pass { last, .. } => *last,
        Row::Event { index, .. } => *index,
    }
}

pub fn kind_color(kind: EventKind) -> Color32 {
    match kind {
        EventKind::Clear => Color32::from_rgb(150, 150, 150),
        EventKind::Draw => Color32::from_rgb(110, 190, 120),
        EventKind::DrawIndirect => Color32::from_rgb(70, 200, 190),
        EventKind::Fullscreen => Color32::from_rgb(120, 170, 240),
        EventKind::Compute => Color32::from_rgb(230, 170, 80),
        EventKind::Copy => Color32::from_rgb(180, 140, 220),
        EventKind::Ui => Color32::from_rgb(230, 110, 160),
    }
}
