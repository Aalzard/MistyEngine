//! The track header column: the Add menu, then one rounded header per track with its name,
//! the object it is bound to, and mute / lock toggles. Groups fold their tracks away.

use egui::{
    Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2,
};
use misty_engine::hecs::Entity;
use misty_engine::scene::registry;
use misty_engine::timeline::{TimelineAsset, Track, TrackKind};

use super::{MARKERS_H, RULER_H, Row, TimelineSel, kind_color, ops};
use crate::state::EditorState;
use crate::{theme, timelines};

/// Header fill of the selected track.
const SELECTED: Color32 = Color32::from_rgb(58, 76, 98);

pub fn show(
    ui: &mut Ui,
    s: &mut EditorState,
    asset: &TimelineAsset,
    path: &str,
    director: Option<Entity>,
    rows: &[Row],
    rect: Rect,
) {
    let editing = !s.play.in_play_mode();
    let top = Rect::from_min_size(rect.min, vec2(rect.width(), RULER_H + MARKERS_H));
    ui.painter().rect_filled(top, 0.0, theme::TOOLBAR_BG);
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(top.shrink2(vec2(6.0, 2.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    bar.add_enabled_ui(editing, |ui| {
        ui.menu_button("Add", |ui| add_menu(ui, s, asset, path));
    });

    let list = Rect::from_min_max(pos2(rect.left(), top.bottom()), rect.max);
    // Empty space under the headers (they sit on top): right-click for the Add menu.
    let background = ui.interact(list, ui.id().with("timeline_headers_bg"), Sense::click());
    if editing {
        background.context_menu(|ui| add_menu(ui, s, asset, path));
    }
    // Wheel over the headers scrolls the track list.
    if ui.rect_contains_pointer(list) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        s.timeline.scroll_y -= dy;
    }
    let mut body = ui.new_child(UiBuilder::new().max_rect(list));
    body.set_clip_rect(list.intersect(ui.clip_rect()));
    let y0 = list.top() - s.timeline.scroll_y;
    for row in rows {
        let r = Rect::from_min_size(
            pos2(rect.left(), y0 + row.top),
            vec2(rect.width(), row.height),
        );
        if r.bottom() < list.top() || r.top() > list.bottom() {
            continue;
        }
        let track = &asset.tracks[row.track];
        header(
            &mut body, s, asset, path, director, track, row.nested, r, editing,
        );
    }
}

/// Add ▸ track kinds (into the selected group, if any).
fn add_menu(ui: &mut Ui, s: &mut EditorState, asset: &TimelineAsset, path: &str) {
    let group = match &s.timeline.selected {
        Some(TimelineSel::Track(id)) => asset.track(id).and_then(|t| match t.kind {
            TrackKind::Group => Some(t.id.clone()),
            _ => t.group.clone(),
        }),
        _ => None,
    };
    for kind in TrackKind::ALL {
        if kind == TrackKind::Group {
            ui.separator();
        }
        if ui.button(kind.label()).clicked() {
            let g = if kind == TrackKind::Group {
                None
            } else {
                group.as_deref()
            };
            ops::add_track(s, path, kind, g);
            ui.close();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut Ui,
    s: &mut EditorState,
    asset: &TimelineAsset,
    path: &str,
    director: Option<Entity>,
    track: &Track,
    nested: bool,
    rect: Rect,
    editing: bool,
) {
    let id = track.id.clone();
    let inner = rect.shrink2(vec2(4.0, 2.0));
    let resp = ui.interact(inner, ui.id().with(("tl_header", &id)), Sense::click());
    let selected = s.timeline.selected == Some(TimelineSel::Track(id.clone()));
    let fill = if selected {
        SELECTED
    } else if resp.hovered() {
        theme::CARD_HEADER
    } else {
        theme::CARD_BG
    };
    let p = ui.painter();
    p.rect_filled(inner, CornerRadius::same(5), fill);
    let muted = asset.is_muted(track);
    let accent = if muted {
        kind_color(track.kind).gamma_multiply(0.35)
    } else {
        kind_color(track.kind)
    };
    let bar = Rect::from_min_size(inner.min, vec2(4.0, inner.height()));
    p.rect_filled(
        bar,
        CornerRadius {
            nw: 5,
            sw: 5,
            ne: 0,
            se: 0,
        },
        accent,
    );
    if resp.clicked() {
        s.timeline.selected = Some(TimelineSel::Track(id.clone()));
        show_in_inspector(s, path);
    }
    if editing {
        resp.context_menu(|ui| context_menu(ui, s, asset, path, track));
    }

    let content = inner.shrink2(vec2(8.0, 0.0));
    let content = Rect::from_min_max(
        pos2(
            content.left() + if nested { 12.0 } else { 0.0 },
            content.top(),
        ),
        content.max,
    );
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(content)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 4.0;
    if track.kind == TrackKind::Group {
        let arrow = if track.collapsed { "⏵" } else { "⏷" };
        if ui
            .add(egui::Button::new(arrow).frame_when_inactive(false).small())
            .clicked()
        {
            timelines::edit(s, path, "Fold Group", false, |t| {
                if let Some(g) = t.track_mut(&id) {
                    g.collapsed = !g.collapsed;
                }
            });
        }
    }
    name_label(ui, s, path, track, muted);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_enabled_ui(editing, |ui| {
            let lock = toggle(ui, "🔒", track.locked, "Lock: the track can't be edited");
            let mute = toggle(ui, "🔇", track.muted, "Mute: the track does nothing");
            if track.kind == TrackKind::Animation {
                record_button(ui, s, path, track);
            }
            if lock {
                timelines::edit(s, path, "Lock Track", false, |t| {
                    if let Some(x) = t.track_mut(&id) {
                        x.locked = !x.locked;
                    }
                });
            }
            if mute {
                timelines::edit(s, path, "Mute Track", false, |t| {
                    if let Some(x) = t.track_mut(&id) {
                        x.muted = !x.muted;
                    }
                });
            }
            if track.kind.bindable()
                && let Some(d) = director
            {
                binding_field(ui, s, d, track);
            }
        });
    });
}

/// Track name, or a text field while renaming.
fn name_label(ui: &mut Ui, s: &mut EditorState, path: &str, track: &Track, muted: bool) {
    if let Some((id, text)) = s.timeline.renaming.as_mut()
        && *id == track.id
    {
        let r = ui.add(
            egui::TextEdit::singleline(text)
                .id(egui::Id::new(("timeline_track_rename", track.id.as_str())))
                .desired_width(110.0),
        );
        let outcome = crate::hold_rename::field(ui, &r, text);
        if outcome == crate::hold_rename::Field::Cancel {
            s.timeline.renaming = None;
        } else if outcome == crate::hold_rename::Field::Commit {
            let (id, name) = s.timeline.renaming.take().expect("renaming");
            let name = name.trim().to_owned();
            if !name.is_empty() {
                timelines::edit(s, path, "Rename Track", false, |t| {
                    if let Some(x) = t.track_mut(&id) {
                        x.name = name;
                    }
                });
            }
        }
        return;
    }
    let color = if muted { theme::TEXT_DIM } else { theme::TEXT };
    let r = ui.add(
        egui::Label::new(RichText::new(&track.name).color(color))
            .truncate()
            .sense(Sense::click()),
    );
    if r.double_clicked() {
        s.timeline.renaming = Some((track.id.clone(), track.name.clone()));
    }
}

/// ⏺ on an Animation track: arm it so changes to its object become keys at the playhead.
fn record_button(ui: &mut Ui, s: &mut EditorState, path: &str, track: &Track) {
    let on = s.timeline.recording.as_deref() == Some(track.id.as_str());
    let can = super::record::can_record(s, path, &track.id);
    let color = if on { super::REC } else { theme::TEXT_DIM };
    let tip = if can {
        "Record: changes to the bound object in the Editor Scene or the Properties panel become keys at the playhead"
    } else {
        "Record needs a bound Animation track without clips (keys go to the track itself)"
    };
    let r = ui
        .add_enabled(
            can || on,
            egui::Button::new(RichText::new("⏺").color(color).size(12.0))
                .frame_when_inactive(false)
                .small(),
        )
        .on_hover_text(tip)
        .on_disabled_hover_text(tip);
    if r.clicked() {
        let arm = (!on).then(|| track.id.clone());
        super::record::arm(s, arm);
    }
}

/// A small icon toggle: bright when on. Returns true when clicked.
fn toggle(ui: &mut Ui, icon: &str, on: bool, tip: &str) -> bool {
    let color = if on { theme::WARN } else { theme::TEXT_DIM };
    ui.add(
        egui::Button::new(RichText::new(icon).color(color).size(12.0))
            .frame_when_inactive(false)
            .small(),
    )
    .on_hover_text(tip)
    .clicked()
}

/// Object field: the scene object the track drives (objects with the component it needs).
fn binding_field(ui: &mut Ui, s: &mut EditorState, director: Entity, track: &Track) {
    let current = super::director(s, director).and_then(|d| d.binding(&track.id));
    let need = track.kind.binding_component();
    let fits = |e: Entity| {
        need.is_empty() || registry::find(need).is_some_and(|info| (info.has)(&s.scene.world, e))
    };
    let objects: Vec<(Entity, String)> = s
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| fits(*e))
        .map(|e| (e, s.scene.name(e)))
        .collect();
    let label = current
        .filter(|e| s.scene.contains(*e))
        .map_or("None".to_owned(), |e| s.scene.name(e));
    let mut chosen = current;
    egui::ComboBox::from_id_salt(("tl_binding", &track.id))
        .selected_text(RichText::new(label).size(11.0))
        .width(92.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, "None");
            for (e, n) in &objects {
                ui.selectable_value(&mut chosen, Some(*e), n);
            }
        })
        .response
        .on_hover_text(if need.is_empty() {
            "Object this track drives".to_owned()
        } else {
            format!("{need} this track drives")
        });
    if chosen != current {
        ops::set_binding(s, director, &track.id, chosen);
    }
}

fn context_menu(
    ui: &mut Ui,
    s: &mut EditorState,
    asset: &TimelineAsset,
    path: &str,
    track: &Track,
) {
    let id = track.id.clone();
    if ui.button("Rename").clicked() {
        s.timeline.renaming = Some((id.clone(), track.name.clone()));
        ui.close();
    }
    if track.kind.has_clips() && !asset.is_locked(track) {
        super::lanes::add_clip_items(ui, s, path, track, None);
    }
    if track.kind == TrackKind::Signal
        && !asset.is_locked(track)
        && ui.button("Add Signal Marker").clicked()
    {
        let t = s.timeline.time;
        ops::add_marker(s, path, Some(&id), t);
        ui.close();
    }
    ui.separator();
    if ui.button("Delete").clicked() {
        ops::remove_track(s, path, &id);
        ui.close();
    }
}

/// The Inspector shows the window's selection (like a selected Animator state).
pub fn show_in_inspector(s: &mut EditorState, path: &str) {
    s.selection.clear();
    s.inspector_asset = Some(path.to_owned());
}
