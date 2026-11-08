//! Details of the selected event: what was drawn, the shader, render target and pipeline
//! state, bound textures (thumbnails) and material values.

use egui::{Color32, RichText, Ui};
use misty_engine::render::{EventTarget, FrameEvent, TargetView};

use super::tree::{kind_color, kind_icon};
use crate::state::EditorState;
use crate::theme;

/// A soft card (no outline) with a title.
fn card(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::CARD_BG)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).strong().color(theme::TEXT));
            ui.add_space(4.0);
            add(ui);
        });
    ui.add_space(6.0);
}

fn rows(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(("frame_debugger_rows", id))
        .num_columns(2)
        .spacing([12.0, 3.0])
        .min_col_width(90.0)
        .show(ui, |ui| {
            for (k, v) in rows {
                if v.is_empty() {
                    continue;
                }
                ui.label(RichText::new(*k).color(theme::TEXT_DIM));
                ui.add(egui::Label::new(RichText::new(v).color(theme::TEXT)).wrap());
                ui.end_row();
            }
        });
}

/// What the Game view shows with `view` picked (Color: the event's own target).
pub fn view_label(view: TargetView, t: EventTarget) -> &'static str {
    match view {
        TargetView::Color => display_label(t),
        TargetView::Depth => "scene depth (depth prepass)",
        TargetView::Normals => "world normals (depth prepass)",
        TargetView::Gi => "Misty GI gather (irradiance)",
        TargetView::Emission => "emission only (materials unlit)",
    }
}

/// What the Game view shows while this event is selected.
pub fn display_label(t: EventTarget) -> &'static str {
    match t {
        EventTarget::None => "nothing (no colour target)",
        EventTarget::Hdr => "scene colour (HDR, tonemapped)",
        EventTarget::Shadow { .. } => "shadow map depth",
        EventTarget::Bloom(_) => "bloom mip (tonemapped)",
        EventTarget::PreFinish => "composite before anti-aliasing",
        EventTarget::Output => "final image",
    }
}

/// The detail rows of an event (also used by tests).
pub fn event_rows(e: &FrameEvent) -> Vec<(&'static str, String)> {
    let opt = |v: &Option<String>| v.clone().unwrap_or_default();
    let count = |n: u32| if n > 0 { n.to_string() } else { String::new() };
    vec![
        ("Pass", e.pass.join(" ▸ ")),
        ("Type", e.kind().label().to_owned()),
        ("Object", opt(&e.object)),
        ("Mesh", opt(&e.mesh)),
        (
            "Submesh",
            e.submesh.map(|s| s.to_string()).unwrap_or_default(),
        ),
        ("Material", opt(&e.material)),
        ("Vertices", count(e.vertices)),
        ("Indices", count(e.indices)),
        (
            "Instances",
            if matches!(
                e.kind(),
                misty_engine::render::EventKind::Draw
                    | misty_engine::render::EventKind::DrawIndirect
            ) {
                e.instances.to_string()
            } else {
                String::new()
            },
        ),
        ("Batching", opt(&e.batch)),
    ]
}

pub fn show(ui: &mut Ui, s: &mut EditorState, e: &FrameEvent, index: usize, total: usize) {
    egui::ScrollArea::vertical()
        .id_salt("frame_debugger_details")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(kind_icon(e.kind()))
                        .size(16.0)
                        .color(kind_color(e.kind())),
                );
                ui.label(
                    RichText::new(&e.name)
                        .size(15.0)
                        .strong()
                        .color(theme::TEXT),
                );
            });
            ui.label(
                RichText::new(format!(
                    "Event {} of {} · Game view shows the {}",
                    index + 1,
                    total,
                    view_label(s.frame_debugger.view, e.display)
                ))
                .color(theme::TEXT_DIM),
            );
            ui.add_space(8.0);
            card(ui, "Event", |ui| rows(ui, "event", &event_rows(e)));
            if !e.shader.is_empty() {
                card(ui, "Shader", |ui| {
                    ui.label(RichText::new(&e.shader).monospace().color(theme::TEXT));
                });
            }
            if !e.bindings.is_empty() {
                card(ui, "Bindings", |ui| {
                    let b: Vec<(&str, String)> = e
                        .bindings
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.clone()))
                        .collect();
                    rows(ui, "bindings", &b);
                });
            }
            let t = &e.target;
            if !t.name.is_empty() {
                let size = match t.width {
                    0 => String::new(),
                    _ => format!("{} × {}", t.width, t.height),
                };
                card(ui, "Render Target", |ui| {
                    rows(
                        ui,
                        "target",
                        &[
                            ("Target", t.name.clone()),
                            ("Size", size),
                            ("Format", t.format.clone()),
                        ],
                    )
                });
            }
            if !(e.blend.is_empty() && e.depth.is_empty() && e.cull.is_empty()) {
                card(ui, "Pipeline State", |ui| {
                    rows(
                        ui,
                        "state",
                        &[
                            ("Blend", e.blend.clone()),
                            ("Depth", e.depth.clone()),
                            ("Cull", e.cull.clone()),
                        ],
                    )
                });
            }
            if !e.textures.is_empty() {
                card(ui, "Textures", |ui| textures(ui, s, e));
            }
            if !e.properties.is_empty() {
                card(ui, "Material Properties", |ui| {
                    let props: Vec<(&str, String)> = e
                        .properties
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.clone()))
                        .collect();
                    rows(ui, "props", &props);
                });
            }
        });
}

fn textures(ui: &mut Ui, s: &mut EditorState, e: &FrameEvent) {
    for (slot, path) in &e.textures {
        ui.horizontal(|ui| {
            let abs = misty_engine::assets::absolute(path);
            let thumb = s.project.thumbnails.get(ui.ctx(), &abs);
            let size = egui::vec2(40.0, 40.0);
            match thumb {
                Some(t) => {
                    ui.add(
                        egui::Image::new(&t)
                            .fit_to_exact_size(size)
                            .corner_radius(4.0),
                    );
                }
                None => {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().rect_filled(rect, 4.0, Color32::from_gray(48));
                }
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(slot).color(theme::TEXT_DIM));
                ui.label(RichText::new(path).color(theme::TEXT));
            });
        });
    }
}
