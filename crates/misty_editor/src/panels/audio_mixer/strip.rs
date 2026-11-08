//! One channel strip per mixer group: name, level readout, vertical fader with a dB scale and a
//! live VU meter, Mute / Solo / Bypass buttons, and the effect slots with Add Effect. Slots show
//! where Sends go and how many feed a Receive / Duck Volume, a gain-reduction bar while ducking,
//! and can be dragged to reorder the chain.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use misty_engine::audio::EffectKind;
use misty_engine::audio::effects::{MAX_DB, MIN_DB};
use misty_engine::audio::mixer::{AudioMixer, MixerEffect, MixerGroup, volume_key};

use super::{Live, MixerItem};
use crate::audio_mixers;
use crate::state::EditorState;
use crate::theme;

pub const STRIP_W: f32 = 100.0;

/// Fader height that leaves room for the buttons and `max_fx` effect slots in a strip of
/// `strip_h` pixels.
pub fn fader_height(strip_h: f32, max_fx: usize) -> f32 {
    (strip_h - 22.0 - 28.0 - 36.0 - (max_fx + 2) as f32 * 21.0 - 8.0).clamp(60.0, 220.0)
}

/// Fader position (0 bottom .. 1 top) of each scale mark: more travel near 0 dB.
const KNOTS: [(f32, f32); 7] = [
    (MIN_DB, 0.0),
    (-40.0, 0.22),
    (-20.0, 0.42),
    (-10.0, 0.58),
    (0.0, 0.78),
    (10.0, 0.9),
    (MAX_DB, 1.0),
];

/// dB to fader position.
pub fn db_to_pos(db: f32) -> f32 {
    let db = db.clamp(MIN_DB, MAX_DB);
    for w in KNOTS.windows(2) {
        let ((d0, p0), (d1, p1)) = (w[0], w[1]);
        if db <= d1 {
            return p0 + (db - d0) / (d1 - d0) * (p1 - p0);
        }
    }
    1.0
}

/// Fader position to dB.
pub fn pos_to_db(pos: f32) -> f32 {
    let pos = pos.clamp(0.0, 1.0);
    for w in KNOTS.windows(2) {
        let ((d0, p0), (d1, p1)) = (w[0], w[1]);
        if pos <= p1 {
            return d0 + (pos - p0) / (p1 - p0) * (d1 - d0);
        }
    }
    MAX_DB
}

/// Header colour of a group's strip (stable per id).
fn group_color(id: u32) -> Color32 {
    const COLORS: [Color32; 6] = [
        Color32::from_rgb(90, 140, 200),
        Color32::from_rgb(200, 140, 70),
        Color32::from_rgb(110, 180, 110),
        Color32::from_rgb(190, 100, 160),
        Color32::from_rgb(200, 190, 80),
        Color32::from_rgb(90, 180, 180),
    ];
    COLORS[id as usize % COLORS.len()]
}

/// Draw the strip of `group` in `rect`.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    mixer: &AudioMixer,
    group: &MixerGroup,
    snap: usize,
    live: Option<&Live>,
    rect: Rect,
    fader_h: f32,
) {
    let selected = s
        .audio_mixer
        .selected
        .is_some_and(|i| i.group() == group.id);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::FIELD_BG);
    if selected {
        painter.rect_stroke(
            rect,
            2.0,
            Stroke::new(1.5, theme::SELECTION),
            egui::StrokeKind::Inside,
        );
    }
    // Header: click selects the group.
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), 22.0));
    painter.rect_filled(head, 2.0, group_color(group.id).gamma_multiply(0.6));
    painter.text(
        head.center(),
        Align2::CENTER_CENTER,
        &group.name,
        FontId::proportional(12.0),
        theme::TEXT,
    );
    let head_resp = ui.interact(head, ui.id().with(("strip_head", group.id)), Sense::click());
    head_resp
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &group.name));
    if head_resp.clicked() {
        super::select(s, path, MixerItem::Group(group.id, None));
    }

    let key = volume_key(group.id);
    let db = live.map_or_else(|| mixer.value(snap, &key), |l| l.value(&key, mixer));
    painter.text(
        pos2(rect.center().x, head.bottom() + 10.0),
        Align2::CENTER_CENTER,
        format!("{db:.1} dB"),
        FontId::monospace(11.0),
        theme::TEXT_DIM,
    );
    let fader = Rect::from_min_size(
        pos2(rect.left() + 8.0, head.bottom() + 28.0),
        vec2(rect.width() - 16.0, fader_h),
    );
    fader_and_meter(ui, s, path, group.id, snap, db, fader);

    // Mute / Solo / Bypass.
    let buttons = Rect::from_min_size(
        pos2(rect.left() + 6.0, fader.bottom() + 8.0),
        vec2(rect.width() - 12.0, 20.0),
    );
    ui.scope_builder(UiBuilder::new().max_rect(buttons), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let id = group.id;
            let flags = [
                ("M", "Mute", group.mute, Color32::from_rgb(200, 60, 60)),
                ("S", "Solo", group.solo, Color32::from_rgb(210, 190, 60)),
                (
                    "B",
                    "Bypass effects",
                    group.bypass_effects,
                    Color32::from_rgb(70, 140, 210),
                ),
            ];
            for (i, (label, tip, on, color)) in flags.into_iter().enumerate() {
                let text = egui::RichText::new(label).strong().color(if on {
                    Color32::BLACK
                } else {
                    theme::TEXT
                });
                let mut b = egui::Button::new(text).min_size(vec2(26.0, 18.0));
                if on {
                    b = b.fill(color);
                }
                if ui.add(b).on_hover_text(tip).clicked() {
                    audio_mixers::edit(s, path, tip, false, |m| {
                        if let Some(g) = m.group_mut(id) {
                            let flag = match i {
                                0 => &mut g.mute,
                                1 => &mut g.solo,
                                _ => &mut g.bypass_effects,
                            };
                            *flag = !*flag;
                        }
                    });
                }
            }
        });
    });

    // Effect slots: Attenuation (the fader) always comes first.
    let mut y = buttons.bottom() + 8.0;
    let slot =
        |y: f32| Rect::from_min_size(pos2(rect.left() + 4.0, y), vec2(rect.width() - 8.0, 18.0));
    let att = slot(y);
    painter.rect_filled(att, 2.0, theme::BUTTON_BG.gamma_multiply(0.6));
    painter.text(
        att.center(),
        Align2::CENTER_CENTER,
        "Attenuation",
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
    y += 21.0;
    let reduction = s.audio_mixer.reduction(group.id);
    for (index, fx) in group.effects.iter().enumerate() {
        let r = slot(y);
        let sel = s.audio_mixer.selected == Some(MixerItem::Group(group.id, Some(fx.id)));
        let fill = if sel {
            theme::SELECTION
        } else if fx.bypass || group.bypass_effects {
            theme::BUTTON_BG.gamma_multiply(0.4)
        } else if fx.kind.is_routing() {
            ROUTING_FILL
        } else {
            theme::BUTTON_BG
        };
        painter.rect_filled(r, 2.0, fill);
        // Ducking / compression in Play mode: an orange bar growing from the right.
        if matches!(fx.kind, EffectKind::DuckVolume | EffectKind::Compressor) && reduction < -0.5 {
            let w = (-reduction / 30.0).clamp(0.0, 1.0) * r.width();
            let bar = Rect::from_min_max(pos2(r.right() - w, r.bottom() - 3.0), r.max);
            painter.rect_filled(bar, 1.0, Color32::from_rgb(230, 140, 40));
        }
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            slot_label(mixer, fx),
            FontId::proportional(11.0),
            if fx.bypass {
                theme::TEXT_DIM
            } else {
                theme::TEXT
            },
        );
        let resp = ui.interact(
            r,
            ui.id().with(("fx", group.id, fx.id)),
            Sense::click_and_drag(),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, slot_label(mixer, fx))
        });
        if resp.clicked() {
            super::select(s, path, MixerItem::Group(group.id, Some(fx.id)));
        }
        // Drag a slot onto another one of the same group to move it there.
        if resp.drag_started() {
            resp.dnd_set_drag_payload(FxDrag(group.id, fx.id));
        }
        if let Some(p) = resp.dnd_hover_payload::<FxDrag>()
            && p.0 == group.id
            && p.1 != fx.id
        {
            painter.hline(
                r.x_range(),
                r.top() - 1.5,
                Stroke::new(2.0, theme::SELECTION),
            );
        }
        if let Some(p) = resp.dnd_release_payload::<FxDrag>()
            && p.0 == group.id
            && p.1 != fx.id
        {
            let (gid, eid) = (p.0, p.1);
            audio_mixers::edit(s, path, "Move Effect", false, |m| {
                m.move_effect_to(gid, eid, index)
            });
        }
        let (gid, eid, bypass) = (group.id, fx.id, fx.bypass);
        resp.context_menu(|ui| {
            if ui
                .button(if bypass { "Enable" } else { "Bypass" })
                .clicked()
            {
                audio_mixers::edit(s, path, "Bypass Effect", false, |m| {
                    if let Some(fx) = m
                        .group_mut(gid)
                        .and_then(|g| g.effects.iter_mut().find(|e| e.id == eid))
                    {
                        fx.bypass = !fx.bypass;
                    }
                });
                ui.close();
            }
            for (label, by) in [("Move Up", -1), ("Move Down", 1)] {
                if ui.button(label).clicked() {
                    audio_mixers::edit(s, path, "Move Effect", false, |m| {
                        m.move_effect(gid, eid, by)
                    });
                    ui.close();
                }
            }
            if ui.button("Remove").clicked() {
                audio_mixers::edit(s, path, "Remove Effect", false, |m| {
                    m.remove_effect(gid, eid)
                });
                s.audio_mixer.selected = Some(MixerItem::Group(gid, None));
                ui.close();
            }
        });
        y += 21.0;
    }
    let add = slot(y);
    ui.scope_builder(UiBuilder::new().max_rect(add), |ui| {
        add_effect_menu(ui, s, path, group.id);
    });
}

/// Fill of Send / Receive / Duck Volume slots (routing, not processing).
const ROUTING_FILL: Color32 = Color32::from_rgb(52, 70, 92);

/// An effect slot being dragged: (group, effect).
#[derive(Clone, Copy)]
struct FxDrag(u32, u32);

/// What an effect slot says: the kind, plus where a Send goes and how many feed a Receive.
pub fn slot_label(mixer: &AudioMixer, fx: &MixerEffect) -> String {
    match fx.kind {
        EffectKind::Send => match fx.target.and_then(|t| mixer.group_of_effect(t)) {
            Some(g) => format!("Send → {}", mixer.group(g).map_or("?", |g| &g.name)),
            None => "Send (none)".into(),
        },
        EffectKind::Receive | EffectKind::DuckVolume => {
            let n = mixer.sends_to(fx.id).len();
            if n == 0 {
                fx.kind.label().into()
            } else {
                format!("{} ← {n}", fx.kind.label())
            }
        }
        k => k.label().into(),
    }
}

/// "Add..." menu listing the effect kinds (signal effects, then routing units).
pub fn add_effect_menu(ui: &mut Ui, s: &mut EditorState, path: &str, group: u32) {
    ui.menu_button("Add...", |ui| {
        for kind in EffectKind::ALL {
            if kind == EffectKind::Send {
                ui.separator();
            }
            if ui.button(kind.label()).clicked() {
                let mut added = None;
                audio_mixers::edit(s, path, "Add Effect", false, |m| {
                    added = m.add_effect(group, kind)
                });
                s.audio_mixer.selected = Some(MixerItem::Group(group, added));
                ui.close();
            }
        }
    });
}

/// Vertical fader on the left of `rect`, dB scale in the middle, VU meter on the right.
fn fader_and_meter(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    group: u32,
    snap: usize,
    db: f32,
    rect: Rect,
) {
    let painter = ui.painter_at(rect.expand(12.0));
    let track = Rect::from_min_size(
        pos2(rect.left() + 10.0, rect.top()),
        vec2(6.0, rect.height()),
    );
    let meter = Rect::from_min_size(
        pos2(rect.right() - 16.0, rect.top()),
        vec2(10.0, rect.height()),
    );
    let y_of = |db: f32| rect.bottom() - db_to_pos(db) * rect.height();
    // Scale.
    for mark in [20.0, 10.0, 0.0, -10.0, -20.0, -40.0, -80.0] {
        let y = y_of(mark);
        for x in [track.right() + 3.0, meter.left() - 7.0] {
            painter.hline(x..=x + 4.0, y, Stroke::new(1.0, theme::TEXT_DIM));
        }
        painter.text(
            pos2((track.right() + meter.left()) * 0.5, y),
            Align2::CENTER_CENTER,
            format!("{mark:.0}"),
            FontId::monospace(9.0),
            theme::TEXT_DIM,
        );
    }
    painter.rect_filled(track, 3.0, theme::TAB_BAR_BG);
    // Meter: green / yellow above -10 dB / red above 0 dB, plus a peak line.
    painter.rect_filled(meter, 1.0, Color32::from_rgb(20, 20, 20));
    let level = s.audio_mixer.level(group);
    if level > MIN_DB {
        let top = y_of(level);
        for (from, to, color) in [
            (MIN_DB, -10.0, Color32::from_rgb(60, 190, 80)),
            (-10.0, 0.0, Color32::from_rgb(220, 200, 60)),
            (0.0, MAX_DB, Color32::from_rgb(220, 60, 50)),
        ] {
            if level > from {
                let seg = Rect::from_x_y_ranges(meter.x_range(), top.max(y_of(to))..=y_of(from));
                painter.rect_filled(seg.shrink2(vec2(1.0, 0.0)), 0.0, color);
            }
        }
    }
    // Fader knob.
    let knob_y = y_of(db);
    let knob = Rect::from_center_size(pos2(track.center().x, knob_y), vec2(22.0, 10.0));
    painter.rect_filled(knob, 2.0, theme::TEXT);
    painter.hline(knob.x_range(), knob_y, Stroke::new(1.0, theme::BORDER));
    let area = Rect::from_x_y_ranges(track.left() - 8.0..=track.right() + 8.0, rect.y_range());
    let resp = ui.interact(
        area,
        ui.id().with(("fader", group)),
        Sense::click_and_drag(),
    );
    let set = |s: &mut EditorState, value: f32, interacting: bool| {
        let key = volume_key(group);
        audio_mixers::edit(s, path, "Set Volume", interacting, |m| {
            m.set_value(snap, &key, value)
        });
    };
    if resp.double_clicked() {
        set(s, 0.0, false);
    } else if (resp.dragged() || resp.drag_started())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let pos = (rect.bottom() - p.y) / rect.height();
        set(s, (pos_to_db(pos) * 10.0).round() / 10.0, true);
    }
    resp.on_hover_text("Drag to set the volume; double-click resets to 0 dB");
}
