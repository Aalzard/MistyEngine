//! Every editor panel draws under each theme, and no text is drawn in a colour that can't be
//! read on what is behind it (dark on dark, light on light).

use egui::epaint::text::ByteRangeExt as _;
use egui::epaint::{ClippedShape, Shape};
use egui::{Color32, Rect};

use super::presets::Preset;
use super::{Palette, contrast};
use crate::layout::Tab;

fn flatten<'a>(shape: &'a Shape, clip: Rect, out: &mut Vec<(&'a Shape, Rect)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| flatten(s, clip, out)),
        s => out.push((s, clip)),
    }
}

/// Text drawn with too little contrast against the filled rectangle under it (or the panel):
/// "text" (colour on background, ratio).
fn unreadable_text(shapes: &[ClippedShape], p: &Palette) -> Vec<String> {
    let mut flat = Vec::new();
    for c in shapes {
        flatten(&c.shape, c.clip_rect, &mut flat);
    }
    let mut fills: Vec<(Rect, Color32)> = Vec::new();
    let mut out = Vec::new();
    for (shape, clip) in flat {
        match shape {
            Shape::Rect(r) if r.fill.a() > 220 => fills.push((r.rect.intersect(clip), r.fill)),
            Shape::Text(t) => {
                let rect = t.visual_bounding_rect().intersect(clip);
                if !rect.is_positive() || t.galley.text().trim().is_empty() {
                    continue;
                }
                let bg = fills
                    .iter()
                    .rev()
                    .find(|(r, _)| r.contains(rect.center()))
                    .map_or(p.panel_bg, |(_, c)| *c);
                for s in &t.galley.job.sections {
                    let text = s.byte_range.slice(&t.galley.job.text);
                    if text.trim().is_empty() {
                        continue;
                    }
                    let mut color = t.override_text_color.unwrap_or(s.format.color);
                    if color == Color32::PLACEHOLDER {
                        color = t.fallback_color;
                    }
                    if color.a() < 100 {
                        continue;
                    }
                    let color = color.to_opaque();
                    let ratio = contrast(color, bg);
                    if ratio < 1.8 {
                        out.push(format!(
                            "{:?} ({color:?} on {bg:?}, {ratio:.2}:1)",
                            text.chars().take(40).collect::<String>()
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn sample_state() -> crate::state::EditorState {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root));
    let cube = s.scene.spawn("Cube", None);
    s.selection.set(cube);
    s
}

/// Draw one panel (or the Preferences and Theme Editor windows) under a preset; returns the
/// unreadable text found.
fn draw(tab: Option<Tab>, preset: Preset) -> Vec<String> {
    let mut s = sample_state();
    if tab.is_none() {
        s.code_editor.ui.prefs_open = true;
        s.appearance.editor_open = true;
    }
    let palette = preset.palette();
    let mut h = egui_kittest::Harness::builder()
        .with_size([1100.0, 760.0])
        .build_ui_state(
            move |ui, s: &mut crate::state::EditorState| {
                super::apply_palette(ui.ctx(), &palette);
                egui::Frame::new().fill(super::panel_bg()).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    match tab {
                        Some(mut t) => {
                            let mut viewer = crate::panels::PanelViewer {
                                state: s,
                                gpu: None,
                            };
                            egui_dock::TabViewer::ui(&mut viewer, ui, &mut t);
                        }
                        None => {
                            crate::panels::code_editor::show_windows(ui.ctx(), s);
                            super::editor::show_window(ui.ctx(), &mut s.appearance, &mut s.console);
                        }
                    }
                });
            },
            s,
        );
    h.run_steps(3);
    unreadable_text(&h.output().shapes, &palette)
}

fn every_panel() -> Vec<Option<Tab>> {
    Tab::ALL
        .into_iter()
        .chain(Tab::ON_DEMAND)
        .map(Some)
        .chain([None])
        .collect()
}

/// Every panel draws under Light, and its text stays readable.
#[test]
fn every_panel_draws_readably_under_light() {
    let mut problems = Vec::new();
    for tab in every_panel() {
        for issue in draw(tab, Preset::Light) {
            problems.push(format!("{tab:?}: {issue}"));
        }
    }
    assert!(
        problems.is_empty(),
        "unreadable text under Light:\n{}",
        problems.join("\n")
    );
}

/// The other presets draw every panel too (High Contrast, Warm Dark, Midnight, Dark).
#[test]
fn every_panel_draws_under_every_preset() {
    let mut problems = Vec::new();
    for preset in [
        Preset::Dark,
        Preset::HighContrast,
        Preset::WarmDark,
        Preset::Midnight,
    ] {
        for tab in every_panel() {
            for issue in draw(tab, preset) {
                problems.push(format!("{preset:?} {tab:?}: {issue}"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "unreadable text:\n{}",
        problems.join("\n")
    );
}
