//! Animation window: clip picker, property list and a dopesheet / curve editor for the
//! selected object's Animation clips, with edit-mode preview and Record mode.

pub mod preview;
mod rows;
pub(crate) mod timeline;

use std::collections::{BTreeSet, HashMap};

use egui::{Align, Color32, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};
use misty_engine::animation::{Property, binding};
use misty_engine::hecs::Entity;

use crate::state::EditorState;
use crate::theme;
use rows::build_rows;

#[cfg(test)]
mod tests;

/// Height of every row, shared by the property list and the dopesheet so they line up.
pub const ROW_H: f32 = 20.0;
/// Width of the property list on the left.
const LIST_W: f32 = 290.0;
/// Record mode colour (button, playhead, timeline tint).
pub const RECORD_RED: Color32 = Color32::from_rgb(220, 60, 60);

/// Animation window state (kept while the tab is closed).
#[derive(Default)]
pub struct AnimationWindow {
    /// Clip being edited (project-relative path).
    pub clip: Option<String>,
    /// Playhead in seconds.
    pub time: f32,
    /// The clip is applied to the scene (scrubbing, playing or recording).
    pub preview: bool,
    /// Window playback (the ▶ button), looping over the clip.
    pub playing: bool,
    /// Record mode: property changes become keys at the playhead.
    pub recording: bool,
    /// Curves view instead of the dopesheet.
    pub curves_view: bool,
    /// Selected keys as (curve index, time bits).
    pub selected: BTreeSet<(usize, u32)>,
    /// Groups folded in the property list: (path, group label).
    pub collapsed: BTreeSet<(String, &'static str)>,
    /// Curves highlighted in the property list (Curves view shows only these when any).
    pub focused_curves: BTreeSet<usize>,
    /// Timeline zoom (pixels per second); 0 = fit the clip.
    pub pixels_per_second: f32,
    /// Seconds at the left edge of the timeline.
    pub scroll: f32,
    /// Last click landed in this window (Delete removes keys instead of objects).
    pub focused: bool,
    /// Object the preview was started on.
    pub(crate) preview_root: Option<Entity>,
    /// Original values of everything the preview touched.
    pub(crate) backup: HashMap<(Entity, Property), f32>,
    /// Values right after sampling this frame (Record mode compares against them).
    pub(crate) baseline: Vec<(Entity, Property, f32)>,
    /// Undo/Redo ran this frame: their changes are not recorded as keys.
    pub skip_record: bool,
    /// Last clip load error shown (to warn once).
    pub(crate) load_error: Option<String>,
    /// Key drag in progress.
    pub(crate) drag: Option<timeline::KeyDrag>,
}

impl AnimationWindow {
    /// Delete should remove keys rather than the selected objects.
    pub fn wants_delete(&self) -> bool {
        self.focused && !self.selected.is_empty()
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    track_focus(ui, s, full);
    if s.play.in_play_mode() {
        centered(
            ui,
            full,
            "Animation preview and editing are not available in Play mode.",
        );
        return;
    }
    let Some(selected) = s.selection.active().filter(|e| s.scene.contains(*e)) else {
        centered(ui, full, "No animatable object selected.");
        return;
    };
    let root = preview::target_root(s);
    let clips = root
        .map(|r| preview::target_clips(s, r))
        .unwrap_or_default();
    let Some(root) = root.filter(|_| !clips.is_empty()) else {
        let name = s.scene.name(selected);
        let ui = &mut ui.new_child(UiBuilder::new().max_rect(full.shrink(16.0)));
        ui.vertical_centered(|ui| {
            ui.add_space(full.height() * 0.3);
            ui.label(format!(
                "To begin animating {name}, create an Animation Clip."
            ));
            ui.add_space(6.0);
            if ui.button("Create").clicked() {
                preview::create_clip(s, selected);
            }
        });
        return;
    };
    let Some(clip) = preview::active_clip(s) else {
        centered(ui, full, "The clip could not be loaded (see the Console).");
        return;
    };
    let rows = build_rows(s, root, &clip);
    let left = Rect::from_min_size(
        full.min,
        vec2(LIST_W.min(full.width() * 0.5), full.height()),
    );
    let right = Rect::from_min_max(pos2(left.right() + 1.0, full.top()), full.max);
    ui.painter()
        .vline(left.right() + 0.5, full.y_range(), (1.0, theme::BORDER));

    let mut list = ui.new_child(UiBuilder::new().max_rect(left));
    list.set_clip_rect(left);
    toolbar(&mut list, s, &clip, left);
    clip_row(&mut list, s, &clips, &clip, left);
    rows::property_list(&mut list, s, root, &clip, &rows, left);
    footer(&mut list, s, root, left);

    let mut tl = ui.new_child(UiBuilder::new().max_rect(right));
    tl.set_clip_rect(right);
    timeline::show(&mut tl, s, &clip, &rows, right);

    if s.animation.focused
        && !s.animation.selected.is_empty()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        && !ui.ctx().egui_wants_keyboard_input()
    {
        preview::delete_selected_keys(s);
    }
    if s.animation.playing {
        ui.ctx().request_repaint();
    }
}

/// Clicks inside the window give it keyboard focus (for Delete); clicks elsewhere take it away.
fn track_focus(ui: &Ui, s: &mut EditorState, rect: Rect) {
    if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        s.animation.focused = rect.contains(p);
    }
}

fn centered(ui: &mut Ui, rect: Rect, text: &str) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        theme::TEXT_DIM,
    );
}

/// Row `i` of the left column.
fn row_rect(left: Rect, i: usize) -> Rect {
    Rect::from_min_size(
        pos2(left.left(), left.top() + i as f32 * ROW_H),
        vec2(left.width(), ROW_H),
    )
}

fn in_row(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(4.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 2.0;
    add(&mut child);
}

/// Preview, Record, transport buttons and the frame field.
fn toolbar(
    ui: &mut Ui,
    s: &mut EditorState,
    clip: &misty_engine::animation::AnimationClip,
    left: Rect,
) {
    let rect = row_rect(left, 0);
    ui.painter()
        .rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 60));
    let mut stop = false;
    in_row(ui, rect, |ui| {
        let w = &mut s.animation;
        if ui
            .add(egui::Button::new("Preview").selected(w.preview))
            .on_hover_text("Apply the clip to the scene")
            .clicked()
        {
            stop = w.preview;
            w.preview = true;
        }
        let rec = RichText::new("⏺").color(if w.recording { RECORD_RED } else { theme::TEXT });
        if ui
            .add(egui::Button::new(rec).selected(w.recording))
            .on_hover_text("Enable/disable keyframe recording mode")
            .clicked()
        {
            w.recording = !w.recording;
            w.preview |= w.recording;
        }
        let times = clip.key_times();
        let eps = 1e-4;
        let prev = times.iter().rev().find(|t| **t < w.time - eps).copied();
        let next = times.iter().find(|t| **t > w.time + eps).copied();
        // Transport button that jumps the playhead to `to` when clicked.
        let jump = |ui: &mut Ui, icon: &str, tip: &str, to: Option<f32>| {
            let clicked = ui
                .add_enabled(to.is_some(), egui::Button::new(icon))
                .on_hover_text(tip)
                .clicked();
            to.filter(|_| clicked)
        };
        let mut target = jump(ui, "⏮", "Go to the beginning", Some(0.0));
        target = target.or(jump(ui, "⏴", "Go to the previous keyframe", prev));
        let play = if w.playing { "⏸" } else { "▶" };
        if ui
            .add(egui::Button::new(play).selected(w.playing))
            .on_hover_text("Play the clip")
            .clicked()
        {
            w.playing = !w.playing;
            w.preview |= w.playing;
        }
        target = target.or(jump(ui, "⏵", "Go to the next keyframe", next));
        target = target.or(jump(ui, "⏭", "Go to the end", Some(clip.length)));
        if let Some(t) = target {
            w.time = t;
            w.playing = false;
            w.preview = true;
        }
        let mut frame = (w.time * clip.sample_rate).round() as i64;
        if ui
            .add(egui::DragValue::new(&mut frame).range(0..=100_000))
            .on_hover_text("Current frame")
            .changed()
        {
            w.time = frame as f32 / clip.sample_rate;
            w.preview = true;
        }
    });
    if stop {
        preview::stop_preview(s);
    }
}

/// Clip dropdown (with Create New Clip), Add Keyframe and the Samples field.
fn clip_row(
    ui: &mut Ui,
    s: &mut EditorState,
    clips: &[String],
    clip: &misty_engine::animation::AnimationClip,
    left: Rect,
) {
    let rect = row_rect(left, 1);
    in_row(ui, rect, |ui| {
        let current = s.animation.clip.clone();
        let mut chosen = None;
        let mut create = false;
        egui::ComboBox::from_id_salt("anim_clip")
            .selected_text(preview::clip_label(current.as_deref()))
            .width(120.0)
            .show_ui(ui, |ui| {
                for c in clips {
                    let name = misty_engine::scene::clip_name(c);
                    if ui
                        .selectable_label(current.as_ref() == Some(c), name)
                        .clicked()
                    {
                        chosen = Some(c.clone());
                    }
                }
                ui.separator();
                create = ui.button("Create New Clip...").clicked();
            });
        if let Some(c) = chosen {
            s.animation.clip = Some(c);
            s.animation.selected.clear();
        }
        if create && let Some(root) = preview::target_root(s) {
            preview::create_clip(s, root);
        }
        if ui
            .button("◆+")
            .on_hover_text("Add keyframe (all properties at the playhead)")
            .clicked()
        {
            preview::key_all(s);
        }
        ui.label(RichText::new("Samples").color(theme::TEXT_DIM));
        let mut rate = clip.sample_rate;
        let r = ui.add(
            egui::DragValue::new(&mut rate)
                .range(1.0..=240.0)
                .speed(1.0)
                .max_decimals(0),
        );
        if r.changed() {
            preview::edit_clip(s, "Change Sample Rate", |c| c.sample_rate = rate.round());
        }
    });
}

/// Add Property menu and the Dopesheet / Curves switch at the bottom of the list.
fn footer(ui: &mut Ui, s: &mut EditorState, root: Entity, left: Rect) {
    let rect = Rect::from_min_max(pos2(left.left(), left.bottom() - ROW_H - 4.0), left.max);
    ui.painter()
        .rect_filled(rect, 0.0, Color32::from_rgb(52, 52, 52));
    in_row(ui, rect, |ui| {
        ui.menu_button("Add Property", |ui| add_property_menu(ui, s, root));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let w = &mut s.animation;
            if ui
                .add(egui::Button::new("Curves").selected(w.curves_view))
                .clicked()
            {
                w.curves_view = true;
            }
            if ui
                .add(egui::Button::new("Dopesheet").selected(!w.curves_view))
                .clicked()
            {
                w.curves_view = false;
            }
        });
    });
}

/// Every animatable property group of the root (listed directly) and its children (submenus).
fn add_property_menu(ui: &mut Ui, s: &mut EditorState, root: Entity) {
    let objects: Vec<(Entity, String)> = s
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| s.scene.is_ancestor(root, *e))
        .filter_map(|e| Some((e, binding::path_of(&s.scene, root, e)?)))
        .collect();
    let groups = |s: &EditorState, e: Entity| -> Vec<Property> {
        let mut seen = Vec::new();
        for p in Property::ALL {
            if binding::has(&s.scene, e, p)
                && !seen.iter().any(|q: &Property| q.group() == p.group())
            {
                seen.push(p);
            }
        }
        seen
    };
    let mut add = None;
    for (e, path) in &objects {
        let mut entries = |ui: &mut Ui, s: &EditorState| {
            for p in groups(s, *e) {
                let label = format!("{} ▸ {}", p.component(), p.group());
                if ui.button(label).clicked() {
                    add = Some((path.clone(), p.siblings()));
                    ui.close();
                }
            }
        };
        if path.is_empty() {
            ui.label(RichText::new(s.scene.name(*e)).color(theme::TEXT_DIM));
            entries(ui, s);
            if objects.len() > 1 {
                ui.separator();
            }
        } else {
            ui.menu_button(path.as_str(), |ui| entries(ui, s));
        }
    }
    if let Some((path, props)) = add {
        preview::add_property(s, root, &path, &props);
    }
}
