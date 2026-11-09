//! Timeline window (Window ▸ Sequencing ▸ Timeline): the selected Playable Director's timeline.
//! Track headers on the left (Add menu, binding, mute, lock), clips and markers on the right,
//! with a scrubbable ruler that previews the scene in edit mode.
//!
//! - [`preview`]: edit-mode preview around the UI pass (restores the scene when it ends).
//! - `headers`: the track header column.
//! - `lanes`: ruler, markers row, clip lanes and their mouse editing.
//! - `ops`: edits with undo (add / remove tracks, clips and markers, move and trim math).

mod headers;
mod lanes;
pub mod ops;
pub mod preview;
pub mod record;

use egui::{
    Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2,
};
use misty_engine::hecs::Entity;
use misty_engine::scene::PlayableDirector;
use misty_engine::timeline::{TimelineAsset, TrackKind};

use crate::state::EditorState;
use crate::theme;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_r19;

/// Tab and asset icon.
pub const ICON: &str = "🎞";

pub const TOOLBAR_H: f32 = 28.0;
pub const RULER_H: f32 = 22.0;
pub const MARKERS_H: f32 = 18.0;
pub const TRACK_H: f32 = 30.0;
pub const GROUP_H: f32 = 26.0;
pub const HEADER_W: f32 = 250.0;
/// Left margin of the clip area so time 0 is not on the edge.
pub const PAD: f32 = 12.0;
/// Record button and recording tint.
pub const REC: Color32 = Color32::from_rgb(224, 72, 72);
const RIPPLE_TIP: &str = "Ripple: moving, trimming or deleting a clip shifts the clips after it. Off (Mix): clips move alone and blend where they overlap.";

/// Something selected in the window (shown in the Inspector).
#[derive(Clone, Debug, PartialEq)]
pub enum TimelineSel {
    Track(String),
    /// Track id and clip index.
    Clip(String, usize),
    /// Track id (`None` = the markers row) and marker index.
    Marker(Option<String>, usize),
}

/// Seconds ↔ pixels of the clip area.
#[derive(Clone, Copy, Debug, Default)]
pub struct Axis {
    pub left: f32,
    pub start: f32,
    pub pps: f32,
}

impl Axis {
    pub fn x(&self, t: f32) -> f32 {
        self.left + PAD + (t - self.start) * self.pps
    }
    pub fn t(&self, x: f32) -> f32 {
        self.start + (x - self.left - PAD) / self.pps
    }
}

/// A mouse drag editing clips or markers.
#[derive(Clone, Debug)]
pub enum Drag {
    /// Moving a clip (`grab` = pointer time minus the clip's start).
    Move {
        track: String,
        clip: usize,
        grab: f32,
        /// The track's clips when the drag started (Ripple shifts the later ones).
        base: Vec<misty_engine::timeline::TimelineClip>,
    },
    TrimStart {
        track: String,
        clip: usize,
        base: misty_engine::timeline::TimelineClip,
    },
    TrimEnd {
        track: String,
        clip: usize,
        /// The track's clips when the drag started (Ripple shifts the later ones).
        base: Vec<misty_engine::timeline::TimelineClip>,
    },
    Marker {
        track: Option<String>,
        index: usize,
    },
    Scrub,
    Pan {
        last: f32,
    },
}

/// Where things were drawn last frame (tests and hit testing).
#[derive(Clone, Debug)]
pub struct Geometry {
    pub axis: Axis,
    /// Track id → its lane in the clip area.
    pub lanes: Vec<(String, Rect)>,
    pub ruler: Rect,
    pub markers: Rect,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            axis: Axis::default(),
            lanes: Vec::new(),
            ruler: Rect::NOTHING,
            markers: Rect::NOTHING,
        }
    }
}

/// What a right-click landed on: track id (`None` = the markers row), clip index, marker
/// index and the time under the pointer.
pub(crate) type ContextTarget = (Option<String>, Option<usize>, Option<usize>, f32);

/// Timeline window state (kept while the tab is closed).
#[derive(Default)]
pub struct TimelineWindow {
    /// Director being edited (stays when the selection moves to an object without one).
    pub director: Option<Entity>,
    /// Asset opened without a director (double-clicked in the Project window).
    pub asset: Option<String>,
    /// Playhead in seconds.
    pub time: f32,
    /// The timeline is applied to the scene (scrubbing or playing).
    pub preview: bool,
    /// Window playback (▶), looping over the timeline.
    pub playing: bool,
    /// Snap clips and the playhead to frames and clip edges.
    pub snap_off: bool,
    /// Zoom (pixels per second); 0 = fit the timeline.
    pub pixels_per_second: f32,
    /// Seconds at the left edge.
    pub scroll: f32,
    /// Track list scrolled down by this many pixels.
    pub scroll_y: f32,
    pub selected: Option<TimelineSel>,
    pub drag: Option<Drag>,
    /// Last click landed in this window (Delete removes clips instead of objects).
    pub focused: bool,
    /// Right-click target: (track id or `None` for the markers row, clip / marker under the
    /// pointer, time).
    pub(crate) context: Option<ContextTarget>,
    /// Track whose name is being edited.
    pub(crate) renaming: Option<(String, String)>,
    pub geometry: Geometry,
    /// Director the preview was started on.
    pub(crate) preview_director: Option<Entity>,
    /// Values the preview overrode, put back when it ends.
    pub(crate) base: misty_engine::timeline::Baseline,
    pub(crate) load_error: Option<String>,
    /// Ripple editing: moving, trimming or deleting a clip shifts the clips after it.
    pub ripple: bool,
    /// Animation track armed for recording: property changes made in the Scene view or the
    /// Inspector become keys at the playhead.
    pub recording: Option<String>,
    /// Recordable values as the preview left them last frame.
    pub(crate) rec_snapshot:
        std::collections::HashMap<(Entity, misty_engine::animation::Property), (String, f32)>,
}

impl TimelineWindow {
    /// Delete should remove the selected clip / marker / track rather than objects.
    pub fn wants_delete(&self) -> bool {
        self.focused && self.selected.is_some()
    }
}

/// The director component of `e`, if any.
pub fn director(s: &EditorState, e: Entity) -> Option<PlayableDirector> {
    s.scene
        .world
        .get::<&PlayableDirector>(e)
        .ok()
        .map(|d| (*d).clone())
}

/// Follow the selection: an object with a director becomes the window's target.
pub fn sync_target(s: &mut EditorState) {
    if let Some(e) = s.selection.active()
        && s.scene.contains(e)
        && director(s, e).is_some()
    {
        s.timeline.director = Some(e);
    }
    if let Some(d) = s.timeline.director
        && (!s.scene.contains(d) || director(s, d).is_none())
    {
        s.timeline.director = None;
    }
}

/// `MISTY_TIMELINE=<seconds>[,record=<track name>][,clip=<track name>:<index>]` (screenshots):
/// preview the selected director's timeline at that time, optionally arming a track for
/// recording and showing a clip in the Inspector.
pub fn apply_env(s: &mut EditorState, value: &str) {
    sync_target(s);
    let parts = value.split(',').map(str::trim);
    for t in parts.clone().filter_map(|t| t.parse::<f32>().ok()) {
        s.timeline.time = t.max(0.0);
        s.timeline.preview = true;
    }
    let Some((_, path)) = current(s) else { return };
    let Some(asset) = active_asset(s, &path) else {
        return;
    };
    let track = |name: &str| {
        asset
            .tracks
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.id.clone())
    };
    for p in parts {
        if let Some(name) = p.strip_prefix("record=")
            && let Some(id) = track(name)
        {
            record::arm(s, Some(id));
        }
        if let Some((name, i)) = p.strip_prefix("clip=").and_then(|c| c.rsplit_once(':'))
            && let (Some(id), Ok(i)) = (track(name), i.parse::<usize>())
        {
            s.timeline.selected = Some(TimelineSel::Clip(id, i));
            headers::show_in_inspector(s, &path);
        }
    }
}

/// `MISTY_CAPTURE_PLAY_TIME=<seconds>` (screenshots of cutscenes): hold the screenshot until
/// Play mode has run that many seconds of game time.
pub fn capture_waiting(s: &EditorState) -> bool {
    let Some(at) = std::env::var("MISTY_CAPTURE_PLAY_TIME")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    else {
        return false;
    };
    !(s.play.in_play_mode() && s.time.time >= at)
}

/// The timeline being shown: (director, asset path).
pub fn current(s: &EditorState) -> Option<(Option<Entity>, String)> {
    if let Some(d) = s.timeline.director
        && let Some(path) = director(s, d).and_then(|c| c.playable)
    {
        return Some((Some(d), path));
    }
    s.timeline.asset.clone().map(|p| (None, p))
}

/// Double-click on a `.timeline`: show it (on a director that plays it, if the scene has one).
pub fn open_asset(s: &mut EditorState, rel: &str) {
    let user = s
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .find(|e| director(s, *e).is_some_and(|d| d.playable.as_deref() == Some(rel)));
    preview::stop_preview(s);
    s.timeline.director = user;
    s.timeline.asset = Some(rel.to_owned());
    s.timeline.selected = None;
    s.push(crate::actions::Action::FocusTab(
        crate::layout::Tab::Timeline,
    ));
}

/// Copy of the shown timeline, if it loads.
pub fn active_asset(s: &mut EditorState, path: &str) -> Option<TimelineAsset> {
    match s.anim_clips.timelines.get(path) {
        Ok(t) => Some(t.clone()),
        Err(e) => {
            if s.timeline.load_error.as_deref() != Some(e.as_str()) {
                s.console.warn(e.clone());
                s.timeline.load_error = Some(e);
            }
            None
        }
    }
}

/// Track colour (clips and the header accent).
pub fn kind_color(kind: TrackKind) -> Color32 {
    match kind {
        TrackKind::Activation => Color32::from_rgb(84, 138, 98),
        TrackKind::Animation => Color32::from_rgb(70, 112, 170),
        TrackKind::Audio => Color32::from_rgb(178, 124, 62),
        TrackKind::Signal => Color32::from_rgb(132, 104, 170),
        TrackKind::Group => Color32::from_rgb(96, 96, 96),
        TrackKind::Camera => Color32::from_rgb(178, 82, 82),
        TrackKind::Control => Color32::from_rgb(64, 140, 150),
    }
}

/// One visible row of the track list.
#[derive(Clone, Debug)]
pub struct Row {
    pub track: usize,
    pub top: f32,
    pub height: f32,
    /// Inside a group.
    pub nested: bool,
}

/// Visible rows (children of folded groups are hidden), relative to the top of the list.
pub fn rows(asset: &TimelineAsset) -> Vec<Row> {
    let mut out = Vec::new();
    let mut y = 0.0;
    for (i, t) in asset.tracks.iter().enumerate() {
        let parent = t.group.as_deref().and_then(|g| asset.track(g));
        if parent.is_some_and(|g| g.collapsed) {
            continue;
        }
        let h = if t.kind == TrackKind::Group {
            GROUP_H
        } else {
            TRACK_H
        };
        out.push(Row {
            track: i,
            top: y,
            height: h,
            nested: parent.is_some(),
        });
        y += h;
    }
    out
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if let Some(p) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        s.timeline.focused = full.contains(p);
    }
    sync_target(s);
    let Some((director, path)) = current(s) else {
        empty_state(ui, s, full);
        return;
    };
    let Some(asset) = active_asset(s, &path) else {
        centered(
            ui,
            full,
            "The timeline could not be loaded (see the Console).",
        );
        return;
    };
    let toolbar = Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR_H));
    toolbar_row(ui, s, &asset, &path, toolbar);
    let body = Rect::from_min_max(pos2(full.left(), toolbar.bottom()), full.max);
    let left = Rect::from_min_size(
        body.min,
        vec2(HEADER_W.min(body.width() * 0.45), body.height()),
    );
    let right = Rect::from_min_max(pos2(left.right() + 1.0, body.top()), body.max);
    ui.painter().rect_filled(body, 0.0, theme::PANEL_BG);
    let rows = rows(&asset);
    clamp_scroll_y(s, &rows, body.height() - RULER_H - MARKERS_H);

    let mut head = ui.new_child(UiBuilder::new().max_rect(left));
    head.set_clip_rect(left);
    headers::show(&mut head, s, &asset, &path, director, &rows, left);

    let mut lane_ui = ui.new_child(UiBuilder::new().max_rect(right));
    lane_ui.set_clip_rect(right);
    lanes::show(&mut lane_ui, s, &asset, &path, &rows, right);

    let editing = !s.play.in_play_mode();
    if editing
        && s.timeline.focused
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        && !ui.ctx().egui_wants_keyboard_input()
    {
        ops::delete_selected(s, &path);
    }
    if s.timeline.playing || s.play.in_play_mode() {
        ui.ctx().request_repaint();
    }
}

fn clamp_scroll_y(s: &mut EditorState, rows: &[Row], visible: f32) {
    let total = rows.last().map_or(0.0, |r| r.top + r.height);
    let max = (total - visible + 8.0).max(0.0);
    s.timeline.scroll_y = s.timeline.scroll_y.clamp(0.0, max);
}

/// No timeline to show: say how to get one (and offer to create it for the selection).
fn empty_state(ui: &mut Ui, s: &mut EditorState, full: Rect) {
    let Some(sel) = s.selection.active().filter(|e| s.scene.contains(*e)) else {
        centered(ui, full, "To start creating a timeline, select an object.");
        return;
    };
    let name = s.scene.name(sel);
    let ui = &mut ui.new_child(UiBuilder::new().max_rect(full.shrink(16.0)));
    ui.vertical_centered(|ui| {
        ui.add_space(full.height() * 0.3);
        ui.label(format!(
            "To begin a new timeline with {name}, create a Director archetype and a Timeline asset."
        ));
        ui.add_space(6.0);
        if ui.button("Create").clicked() {
            ops::create_for(s, sel);
        }
    });
}

fn centered(ui: &mut Ui, rect: Rect, text: &str) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        theme::TEXT_DIM,
    );
}

/// Preview, transport, frame counter and snapping.
fn toolbar_row(ui: &mut Ui, s: &mut EditorState, asset: &TimelineAsset, path: &str, rect: Rect) {
    ui.painter().rect_filled(rect, 0.0, theme::TOOLBAR_BG);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut child;
    ui.spacing_mut().item_spacing.x = 3.0;
    let playing_game = s.play.in_play_mode();
    let rate = asset.frame_rate;
    let duration = asset.duration();
    let mut stop = false;
    ui.add_enabled_ui(!playing_game, |ui| {
        let w = &mut s.timeline;
        if ui
            .add(egui::Button::new("Preview").selected(w.preview))
            .on_hover_text("Apply the timeline to the scene")
            .clicked()
        {
            stop = w.preview;
            w.preview = !w.preview;
        }
        ui.add_space(4.0);
        let frame = 1.0 / rate;
        let jump = |ui: &mut Ui, icon: &str, tip: &str| {
            ui.add(egui::Button::new(icon).frame_when_inactive(false))
                .on_hover_text(tip)
                .clicked()
        };
        let mut target = None;
        if jump(ui, "⏮", "Go to the beginning") {
            target = Some(0.0);
        }
        if jump(ui, "⏴", "Previous frame") {
            target = Some((w.time - frame).max(0.0));
        }
        let play = if w.playing { "⏸" } else { "▶" };
        if ui
            .add(egui::Button::new(play).selected(w.playing))
            .on_hover_text("Play the timeline in the Editor Scene")
            .clicked()
        {
            w.playing = !w.playing;
            w.preview |= w.playing;
        }
        if jump(ui, "⏵", "Next frame") {
            target = Some(w.time + frame);
        }
        if jump(ui, "⏭", "Go to the end") {
            target = Some(duration);
        }
        if let Some(t) = target {
            w.time = (t * rate).round() / rate;
            w.playing = false;
            w.preview = true;
        }
        let mut f = (w.time * rate).round() as i64;
        if ui
            .add(egui::DragValue::new(&mut f).range(0..=1_000_000))
            .on_hover_text("Current frame")
            .changed()
        {
            w.time = f as f32 / rate;
            w.preview = true;
        }
    });
    if stop {
        preview::stop_preview(s);
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let w = &mut s.timeline;
        let snap = !w.snap_off;
        if ui
            .add(egui::Button::new("Snap").selected(snap))
            .on_hover_text("Snap to frames and clip edges")
            .clicked()
        {
            w.snap_off = snap;
        }
        if ui
            .add(egui::Button::new("Ripple").selected(w.ripple))
            .on_hover_text(RIPPLE_TIP)
            .clicked()
        {
            w.ripple = !w.ripple;
        }
        if w.recording.is_some() {
            ui.label(RichText::new("⏺ Recording").color(REC));
        }
        let name = misty_engine::scene::clip_name(path);
        ui.label(
            RichText::new(format!(
                "{name}  ·  {}",
                crate::panels::animation::timeline::time_label(duration, rate)
            ))
            .color(theme::TEXT_DIM),
        );
    });
}
