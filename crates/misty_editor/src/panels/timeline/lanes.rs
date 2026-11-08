//! Right side of the Timeline window: the time ruler (click or drag to scrub), the markers row,
//! and one lane per track with its clips (rounded blocks; eases and blends drawn as diagonals)
//! or signal markers. Clips move by dragging their body and trim by dragging an edge, snapping
//! to frames, other clips and the playhead.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, PointerButton, Pos2, Rect, Sense, Shape,
    Stroke, Ui, pos2, vec2,
};
use misty_engine::timeline::{TimelineAsset, TimelineClip, Track, TrackKind, evaluate};

use super::{
    Axis, Drag, Geometry, MARKERS_H, PAD, RULER_H, Row, TimelineSel, headers, kind_color, ops,
};
use crate::panels::AssetPayload;
use crate::panels::animation::timeline::{tick_step, time_label};
use crate::state::EditorState;
use crate::{theme, timelines};

const LANE_BG: Color32 = Color32::from_rgb(46, 46, 46);
const GROUP_BG: Color32 = Color32::from_rgb(52, 52, 52);
const MARKER: Color32 = Color32::from_rgb(214, 186, 110);
/// Width of a clip's trim handles in pixels.
const EDGE: f32 = 6.0;

/// What is under the pointer.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    ClipBody(String, usize),
    ClipStart(String, usize),
    ClipEnd(String, usize),
    Marker(Option<String>, usize),
    Lane(String),
    Nothing,
}

pub fn show(
    ui: &mut Ui,
    s: &mut EditorState,
    asset: &TimelineAsset,
    path: &str,
    rows: &[Row],
    rect: Rect,
) {
    let busy = crate::anim_controllers::interacting(ui.ctx());
    s.anim_clips.timelines.end_interaction(busy);
    let editing = !s.play.in_play_mode();
    let axis = zoom_and_pan(ui, s, asset, rect);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, theme::BORDER);

    let ruler = Rect::from_min_size(rect.min, vec2(rect.width(), RULER_H));
    let markers = Rect::from_min_size(
        pos2(rect.left(), ruler.bottom()),
        vec2(rect.width(), MARKERS_H),
    );
    let list = Rect::from_min_max(pos2(rect.left(), markers.bottom()), rect.max);
    let y0 = list.top() - s.timeline.scroll_y;
    let mut geometry = Geometry {
        axis,
        ruler,
        markers,
        lanes: Vec::new(),
    };
    let list_painter = painter.with_clip_rect(list.intersect(ui.clip_rect()));
    for row in rows {
        let lane = Rect::from_min_size(
            pos2(rect.left(), y0 + row.top),
            vec2(rect.width(), row.height),
        );
        let track = &asset.tracks[row.track];
        geometry.lanes.push((track.id.clone(), lane));
        if lane.bottom() < list.top() || lane.top() > list.bottom() {
            continue;
        }
        let bg = if track.kind == TrackKind::Group {
            GROUP_BG
        } else {
            LANE_BG
        };
        list_painter.rect_filled(lane.shrink2(vec2(0.0, 2.0)), 0.0, bg);
        draw_track(&list_painter, s, asset, track, lane, axis);
    }
    // Past the end of the timeline is shaded.
    let end_x = axis.x(asset.duration());
    if end_x < rect.right() {
        painter.rect_filled(
            Rect::from_min_max(pos2(end_x.max(rect.left()), markers.top()), rect.max),
            0.0,
            Color32::from_black_alpha(50),
        );
    }

    draw_ruler(&painter, asset, axis, ruler);
    painter.rect_filled(markers, 0.0, theme::TOOLBAR_BG);
    for (i, m) in asset.markers.iter().enumerate() {
        let sel = s.timeline.selected == Some(TimelineSel::Marker(None, i));
        marker_shape(&painter, axis.x(m.time), markers, sel);
    }
    s.timeline.geometry = geometry;

    interact(ui, s, asset, path, rect, editing);

    // Playhead over everything.
    let x = axis.x(s.timeline.time);
    if rect.x_range().contains(x) {
        let color = if s.play.in_play_mode() {
            theme::PLAY_TINT.gamma_multiply(1.6)
        } else {
            Color32::WHITE
        };
        painter.vline(x, ruler.top()..=rect.bottom(), Stroke::new(1.0, color));
        let head = Rect::from_center_size(pos2(x, ruler.bottom() - 7.0), vec2(10.0, 12.0));
        painter.rect_filled(head, CornerRadius::same(3), color);
    }
}

/// Mouse wheel zooms around the pointer, Shift+wheel (or a sideways wheel) pans.
fn zoom_and_pan(ui: &Ui, s: &mut EditorState, asset: &TimelineAsset, rect: Rect) -> Axis {
    let w = &mut s.timeline;
    if w.pixels_per_second <= 0.0 {
        let span = asset.duration().max(5.0);
        w.pixels_per_second =
            ((rect.width() - 2.0 * PAD).max(50.0) / span * 0.92).clamp(5.0, 2000.0);
        w.scroll = 0.0;
    }
    let mut axis = Axis {
        left: rect.left(),
        start: w.scroll,
        pps: w.pixels_per_second,
    };
    let list = Rect::from_min_max(pos2(rect.left(), rect.top()), rect.max);
    if ui.rect_contains_pointer(list) {
        let (delta, shift, pointer) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.modifiers.shift,
                i.pointer.hover_pos(),
            )
        });
        let (zoom, pan) = if shift {
            (0.0, delta.y + delta.x)
        } else {
            (delta.y, delta.x)
        };
        if zoom != 0.0
            && let Some(p) = pointer
        {
            let t = axis.t(p.x);
            axis.pps = (axis.pps * (1.0 + zoom * 0.003)).clamp(5.0, 5000.0);
            axis.start = t - (p.x - axis.left - PAD) / axis.pps;
        }
        axis.start -= pan / axis.pps;
    }
    axis.start = axis.start.max(-1.0);
    w.pixels_per_second = axis.pps;
    w.scroll = axis.start;
    axis
}

fn draw_ruler(p: &egui::Painter, asset: &TimelineAsset, axis: Axis, rect: Rect) {
    p.rect_filled(rect, 0.0, theme::TOOLBAR_BG);
    let rate = asset.frame_rate;
    let step = tick_step(rate, axis.pps) / rate;
    let minor = step / 5.0;
    let first = (axis.t(rect.left()) / minor).floor().max(0.0) as i64;
    let last = (axis.t(rect.right()) / minor).ceil() as i64;
    for i in first..=last.min(first + 2000) {
        let t = i as f32 * minor;
        let x = axis.x(t);
        let major = i % 5 == 0;
        let h = if major { RULER_H * 0.5 } else { RULER_H * 0.22 };
        p.vline(
            x,
            rect.bottom() - h..=rect.bottom(),
            Stroke::new(1.0, theme::TEXT_DIM.gamma_multiply(0.7)),
        );
        if major {
            p.text(
                pos2(x + 3.0, rect.top() + 2.0),
                Align2::LEFT_TOP,
                time_label(t, rate),
                FontId::proportional(10.0),
                theme::TEXT_DIM,
            );
        }
    }
}

/// A signal marker: a small rounded pin.
fn marker_shape(p: &egui::Painter, x: f32, lane: Rect, selected: bool) {
    let color = if selected { Color32::WHITE } else { MARKER };
    let cy = lane.center().y;
    let h = (lane.height() - 6.0).clamp(8.0, 14.0);
    let r = Rect::from_center_size(pos2(x, cy - 1.0), vec2(8.0, h - 3.0));
    p.rect_filled(r, CornerRadius::same(3), color);
    p.add(Shape::convex_polygon(
        vec![
            pos2(x - 4.0, r.bottom() - 1.0),
            pos2(x + 4.0, r.bottom() - 1.0),
            pos2(x, r.bottom() + 3.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Pixel rect of a clip in its lane.
fn clip_rect(lane: Rect, axis: Axis, c: &TimelineClip) -> Rect {
    Rect::from_min_max(
        pos2(axis.x(c.start), lane.top() + 4.0),
        pos2(
            axis.x(c.end()).max(axis.x(c.start) + 2.0),
            lane.bottom() - 4.0,
        ),
    )
}

fn draw_track(
    p: &egui::Painter,
    s: &EditorState,
    asset: &TimelineAsset,
    track: &Track,
    lane: Rect,
    axis: Axis,
) {
    let muted = asset.is_muted(track);
    if track.kind == TrackKind::Signal {
        for (i, m) in track.markers.iter().enumerate() {
            let sel = s.timeline.selected == Some(TimelineSel::Marker(Some(track.id.clone()), i));
            marker_shape(p, axis.x(m.time), lane, sel);
        }
        return;
    }
    if track.kind == TrackKind::Animation && track.clips.is_empty() && track.recorded.is_some() {
        let len = track.recorded.as_ref().map_or(0.0, |r| r.length);
        let r = Rect::from_min_max(
            pos2(axis.x(0.0), lane.top() + 4.0),
            pos2(axis.x(len.max(asset.duration())), lane.bottom() - 4.0),
        );
        p.rect_filled(
            r,
            CornerRadius::same(4),
            kind_color(track.kind).gamma_multiply(0.6),
        );
        label(p, r, "Recorded");
        return;
    }
    for (i, c) in track.clips.iter().enumerate() {
        let r = clip_rect(lane, axis, c);
        if r.right() < lane.left() || r.left() > lane.right() {
            continue;
        }
        let selected = s.timeline.selected == Some(TimelineSel::Clip(track.id.clone(), i));
        let mut fill = kind_color(track.kind);
        if selected {
            fill = fill.lerp_to_gamma(Color32::WHITE, 0.22);
        }
        if muted {
            fill = fill.gamma_multiply(0.4);
        }
        p.rect_filled(r, CornerRadius::same(4), fill);
        // Fades: the part above the diagonal is shaded (weight rising from 0 to 1).
        let (fin, fout) = evaluate::fades(&track.clips, i);
        let shade = Color32::from_black_alpha(115);
        let line = Stroke::new(1.0, Color32::from_white_alpha(170));
        if fin > 0.0 {
            let xi = axis.x(c.start + fin).min(r.right());
            p.add(Shape::convex_polygon(
                vec![r.left_top(), pos2(xi, r.top()), r.left_bottom()],
                shade,
                Stroke::NONE,
            ));
            p.line_segment([r.left_bottom(), pos2(xi, r.top())], line);
        }
        if fout > 0.0 {
            let xo = axis.x(c.end() - fout).max(r.left());
            p.add(Shape::convex_polygon(
                vec![pos2(xo, r.top()), r.right_top(), r.right_bottom()],
                shade,
                Stroke::NONE,
            ));
            p.line_segment([pos2(xo, r.top()), r.right_bottom()], line);
        }
        let mut name = if c.name.is_empty() {
            c.asset
                .as_deref()
                .map(misty_engine::scene::clip_name)
                .unwrap_or_else(|| "Active".into())
        } else {
            c.name.clone()
        };
        // Camera / Control clips whose object is gone say so.
        if track.kind.clips_reference_objects() {
            let found = s
                .timeline
                .director
                .and_then(|d| super::director(s, d))
                .and_then(|d| d.reference(c.reference.as_deref()?))
                .is_some_and(|e| s.scene.contains(e));
            if !found {
                name.push_str("  (no object)");
            }
        }
        label(p, r, &name);
    }
}

fn label(p: &egui::Painter, r: Rect, text: &str) {
    if r.width() < 18.0 {
        return;
    }
    p.with_clip_rect(r.shrink2(vec2(5.0, 0.0)).intersect(p.clip_rect()))
        .text(
            pos2(r.left() + 7.0, r.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(11.5),
            Color32::from_rgb(236, 236, 236),
        );
}

/// What is at `pos` (clips and markers of the lanes, or the markers row).
fn hit(s: &EditorState, asset: &TimelineAsset, pos: Pos2) -> Hit {
    let g = &s.timeline.geometry;
    let axis = g.axis;
    if g.markers.contains(pos) {
        return asset
            .markers
            .iter()
            .position(|m| (axis.x(m.time) - pos.x).abs() <= 5.0)
            .map_or(Hit::Nothing, |i| Hit::Marker(None, i));
    }
    let Some((id, lane)) = g.lanes.iter().find(|(_, r)| r.contains(pos)) else {
        return Hit::Nothing;
    };
    let Some(track) = asset.track(id) else {
        return Hit::Nothing;
    };
    if track.kind == TrackKind::Signal {
        if let Some(i) = track
            .markers
            .iter()
            .position(|m| (axis.x(m.time) - pos.x).abs() <= 5.0)
        {
            return Hit::Marker(Some(id.clone()), i);
        }
        return Hit::Lane(id.clone());
    }
    // Later clips are drawn on top: test them first.
    for (i, c) in track.clips.iter().enumerate().rev() {
        let r = clip_rect(*lane, axis, c);
        if !r.expand2(vec2(2.0, 0.0)).contains(pos) {
            continue;
        }
        let edge = EDGE.min(r.width() / 3.0);
        return if pos.x <= r.left() + edge {
            Hit::ClipStart(id.clone(), i)
        } else if pos.x >= r.right() - edge {
            Hit::ClipEnd(id.clone(), i)
        } else {
            Hit::ClipBody(id.clone(), i)
        };
    }
    Hit::Lane(id.clone())
}

/// The track of a hit, if it is locked (locked tracks don't edit).
fn locked(asset: &TimelineAsset, id: Option<&str>) -> bool {
    id.and_then(|id| asset.track(id))
        .is_some_and(|t| asset.is_locked(t))
}

fn interact(
    ui: &mut Ui,
    s: &mut EditorState,
    asset: &TimelineAsset,
    path: &str,
    rect: Rect,
    editing: bool,
) {
    let axis = s.timeline.geometry.axis;
    let ruler = s.timeline.geometry.ruler;
    let resp = ui.interact(rect, ui.id().with("tl_lanes"), Sense::click_and_drag());
    let pointer = resp.hover_pos();
    let hovered = pointer.map(|p| hit(s, asset, p));
    if editing
        && matches!(hovered, Some(Hit::ClipStart(..) | Hit::ClipEnd(..)))
        && s.timeline.drag.is_none()
    {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    let snap = !s.timeline.snap_off;
    let radius = ops::SNAP_PX / axis.pps.max(1e-3);
    let frame = 1.0 / asset.frame_rate.max(1.0);

    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
        let t = axis.t(origin.x);
        s.timeline.drag = if resp.dragged_by(PointerButton::Middle) {
            Some(Drag::Pan { last: origin.x })
        } else if ruler.contains(origin) {
            Some(Drag::Scrub)
        } else if !editing {
            None
        } else {
            match hit(s, asset, origin) {
                Hit::ClipBody(id, i) if !locked(asset, Some(&id)) => {
                    let start = asset.track(&id).map_or(0.0, |tr| tr.clips[i].start);
                    let base = asset
                        .track(&id)
                        .map(|tr| tr.clips.clone())
                        .unwrap_or_default();
                    select(s, path, TimelineSel::Clip(id.clone(), i));
                    Some(Drag::Move {
                        track: id,
                        clip: i,
                        grab: t - start,
                        base,
                    })
                }
                Hit::ClipStart(id, i) if !locked(asset, Some(&id)) => {
                    let base = asset
                        .track(&id)
                        .map(|tr| tr.clips[i].clone())
                        .unwrap_or_default();
                    select(s, path, TimelineSel::Clip(id.clone(), i));
                    Some(Drag::TrimStart {
                        track: id,
                        clip: i,
                        base,
                    })
                }
                Hit::ClipEnd(id, i) if !locked(asset, Some(&id)) => {
                    let base = asset
                        .track(&id)
                        .map(|tr| tr.clips.clone())
                        .unwrap_or_default();
                    select(s, path, TimelineSel::Clip(id.clone(), i));
                    Some(Drag::TrimEnd {
                        track: id,
                        clip: i,
                        base,
                    })
                }
                Hit::Marker(track, i) if !locked(asset, track.as_deref()) => {
                    select(s, path, TimelineSel::Marker(track.clone(), i));
                    Some(Drag::Marker { track, index: i })
                }
                _ => None,
            }
        };
    }
    if resp.dragged()
        && let (Some(p), Some(drag)) = (resp.interact_pointer_pos(), s.timeline.drag.clone())
    {
        let t = axis.t(p.x);
        let frame_snap = |x: f32| if snap { asset.snap(x) } else { x };
        match drag {
            Drag::Pan { last } => {
                s.timeline.scroll -= (p.x - last) / axis.pps;
                s.timeline.drag = Some(Drag::Pan { last: p.x });
            }
            Drag::Scrub => {
                let w = &mut s.timeline;
                w.time = frame_snap(t).max(0.0);
                w.preview = editing;
                w.playing = false;
            }
            Drag::Move {
                track,
                clip,
                grab,
                base,
            } => {
                let dur = asset
                    .track(&track)
                    .map_or(0.0, |tr| tr.clips[clip].duration);
                let points = if s.timeline.ripple {
                    vec![0.0, s.timeline.time]
                } else {
                    ops::snap_points(asset, &track, clip, s.timeline.time)
                };
                let start = ops::snapped_start(asset, t - grab, dur, &points, radius, snap);
                let ripple = s.timeline.ripple;
                timelines::edit(s, path, "Move Clip", true, |a| {
                    if let Some(tr) = a.track_mut(&track)
                        && clip < tr.clips.len()
                    {
                        if ripple {
                            let delta = start - base[clip].start;
                            ops::ripple(&mut tr.clips, &base, clip, base[clip].end(), delta);
                        }
                        tr.clips[clip].start = start;
                    }
                });
            }
            Drag::TrimStart { track, clip, base } => {
                let points = ops::snap_points(asset, &track, clip, s.timeline.time);
                let mut to = frame_snap(t);
                if snap && let Some(e) = ops::snap_to(to, &points, radius) {
                    to = e;
                }
                let trimmed = ops::trim_start(&base, to, frame);
                timelines::edit(s, path, "Trim Clip", true, |a| {
                    if let Some(c) = a.track_mut(&track).and_then(|tr| tr.clips.get_mut(clip)) {
                        *c = trimmed;
                    }
                });
            }
            Drag::TrimEnd { track, clip, base } => {
                let points = ops::snap_points(asset, &track, clip, s.timeline.time);
                let mut to = frame_snap(t);
                if snap && let Some(e) = ops::snap_to(to, &points, radius) {
                    to = e;
                }
                let ripple = s.timeline.ripple;
                timelines::edit(s, path, "Trim Clip", true, |a| {
                    if let Some(tr) = a.track_mut(&track)
                        && clip < tr.clips.len()
                    {
                        let mut c = base[clip].clone();
                        ops::trim_end(&mut c, to, frame);
                        if ripple {
                            let delta = c.end() - base[clip].end();
                            ops::ripple(&mut tr.clips, &base, clip, base[clip].end(), delta);
                        }
                        tr.clips[clip].duration = c.duration;
                        tr.clips[clip].ease_in = c.ease_in;
                        tr.clips[clip].ease_out = c.ease_out;
                    }
                });
            }
            Drag::Marker { track, index } => {
                let mut to = frame_snap(t).max(0.0);
                if snap && let Some(e) = ops::snap_to(to, &[0.0, s.timeline.time], radius) {
                    to = e;
                }
                timelines::edit(s, path, "Move Marker", true, |a| {
                    let list = match &track {
                        Some(id) => a.track_mut(id).map(|tr| &mut tr.markers),
                        None => Some(&mut a.markers),
                    };
                    if let Some(m) = list.and_then(|l| l.get_mut(index)) {
                        m.time = to;
                    }
                });
            }
        }
    }
    if resp.drag_stopped() {
        s.timeline.drag = None;
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        if ruler.contains(p) {
            let w = &mut s.timeline;
            w.time = if snap {
                asset.snap(axis.t(p.x))
            } else {
                axis.t(p.x)
            }
            .max(0.0);
            w.preview = editing;
            w.playing = false;
        } else {
            match hit(s, asset, p) {
                Hit::ClipBody(id, i) | Hit::ClipStart(id, i) | Hit::ClipEnd(id, i) => {
                    select(s, path, TimelineSel::Clip(id, i));
                }
                Hit::Marker(track, i) => select(s, path, TimelineSel::Marker(track, i)),
                _ => s.timeline.selected = None,
            }
        }
    }
    if resp.secondary_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let t = axis.t(p.x).max(0.0);
        s.timeline.context = match hit(s, asset, p) {
            Hit::ClipBody(id, i) | Hit::ClipStart(id, i) | Hit::ClipEnd(id, i) => {
                Some((Some(id), Some(i), None, t))
            }
            Hit::Marker(track, i) => Some((track, None, Some(i), t)),
            Hit::Lane(id) => Some((Some(id), None, None, t)),
            Hit::Nothing if s.timeline.geometry.markers.contains(p) => Some((None, None, None, t)),
            Hit::Nothing => None,
        };
    }
    if editing {
        resp.context_menu(|ui| context_menu(ui, s, asset, path));
    }
    // Clips dragged in from the Project window.
    if editing
        && let Some(payload) = resp.dnd_release_payload::<AssetPayload>()
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
        && let Hit::Lane(id) | Hit::ClipBody(id, _) = hit(s, asset, p)
        && let Some(track) = asset.track(&id)
        && !asset.is_locked(track)
    {
        let fits = match track.kind {
            TrackKind::Animation => misty_engine::animation::is_clip(&payload.0),
            TrackKind::Audio => misty_engine::audio::is_clip(&payload.0),
            _ => false,
        };
        if fits {
            let t = axis.t(p.x).max(0.0);
            ops::add_clip(s, path, &id, Some(&payload.0), Some(t));
        }
    }
}

fn select(s: &mut EditorState, path: &str, sel: TimelineSel) {
    s.timeline.selected = Some(sel);
    headers::show_in_inspector(s, path);
}

/// Right-click menu of the lanes: add clips or markers at the pointer, delete the clicked one.
fn context_menu(ui: &mut Ui, s: &mut EditorState, asset: &TimelineAsset, path: &str) {
    let Some((track, clip, marker, t)) = s.timeline.context.clone() else {
        ui.close();
        return;
    };
    if locked(asset, track.as_deref()) {
        ui.label("The track is locked.");
        return;
    }
    if clip.is_some() || marker.is_some() {
        if let Some(i) = clip {
            s.timeline.selected = Some(TimelineSel::Clip(track.clone().unwrap_or_default(), i));
        } else if let Some(i) = marker {
            s.timeline.selected = Some(TimelineSel::Marker(track.clone(), i));
        }
        if ui.button("Delete").clicked() {
            ops::delete_selected(s, path);
            ui.close();
        }
        return;
    }
    match track.as_deref().and_then(|id| asset.track(id)) {
        None => {
            if ui.button("Add Marker").clicked() {
                ops::add_marker(s, path, None, t);
                ui.close();
            }
        }
        Some(tr) if tr.kind == TrackKind::Signal => {
            if ui.button("Add Signal Marker").clicked() {
                let id = tr.id.clone();
                ops::add_marker(s, path, Some(&id), t);
                ui.close();
            }
        }
        Some(tr) if tr.kind.has_clips() => add_clip_items(ui, s, path, tr, Some(t)),
        Some(_) => {
            ui.close();
        }
    }
}

/// "Add … Clip" entries for a track (at `time`, or after its last clip).
pub fn add_clip_items(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    track: &Track,
    time: Option<f32>,
) {
    let id = track.id.clone();
    match track.kind {
        TrackKind::Activation => {
            if ui.button("Add Activation Clip").clicked() {
                ops::add_clip(s, path, &id, None, time);
                ui.close();
            }
        }
        TrackKind::Camera => {
            let cams: Vec<(misty_engine::hecs::Entity, String)> =
                misty_engine::vcam::rig::active_cameras(&s.scene)
                    .into_iter()
                    .map(|e| (e, s.scene.name(e)))
                    .collect();
            ui.menu_button("Add Camera Shot", |ui| {
                if cams.is_empty() {
                    ui.label("No virtual cameras in the scene.");
                }
                for (e, n) in cams {
                    if ui.button(n).clicked() {
                        ops::add_reference_clip(s, path, &id, e, time);
                        ui.close();
                    }
                }
            });
        }
        TrackKind::Control => {
            let objects = ops::controllable(s);
            ui.menu_button("Add Control Clip", |ui| {
                if objects.is_empty() {
                    ui.label("No objects in the scene.");
                }
                for (e, n) in objects {
                    if ui.button(n).clicked() {
                        ops::add_reference_clip(s, path, &id, e, time);
                        ui.close();
                    }
                }
            });
        }
        TrackKind::Animation | TrackKind::Audio => {
            let (title, list): (&str, Vec<String>) = if track.kind == TrackKind::Animation {
                (
                    "Add Animation Clip",
                    s.assets.anims.iter().cloned().collect(),
                )
            } else {
                ("Add Audio Clip", s.assets.clips.iter().cloned().collect())
            };
            ui.menu_button(title, |ui| {
                if list.is_empty() {
                    ui.label("No clips in the project.");
                }
                for a in list {
                    if ui.button(misty_engine::scene::clip_name(&a)).clicked() {
                        ops::add_clip(s, path, &id, Some(&a), time);
                        ui.close();
                    }
                }
            });
        }
        _ => {}
    }
}
