//! Inspector for Audio Mixer assets: a summary, or — when a group is selected in the Audio Mixer
//! window — that group's pitch, volume and effect parameters for the edited snapshot. Values
//! can be exposed to scripts from their right-click menu.

use egui::{Grid, RichText, Ui};
use misty_engine::audio::EffectKind;
use misty_engine::audio::dsp::fx::response;
use misty_engine::audio::effects::{MAX_DB, MIN_DB, WET};
use misty_engine::audio::mixer::{
    AudioMixer, MASTER, MixerEffect, effect_key, pitch_key, volume_key,
};
use misty_engine::audio::{output_ref, split_output};
use misty_engine::scene::clip_name;

use crate::actions::Action;
use crate::anim_controllers::interacting;
use crate::audio_mixers;
use crate::layout::Tab;
use crate::panels::audio_mixer::{MIXER_ICON, MixerItem};
use crate::state::EditorState;
use crate::theme;

pub fn mixer_asset(ui: &mut Ui, s: &mut EditorState, rel: &str) {
    let mixer = match s.anim_clips.mixers.get(rel) {
        Ok(m) => m.clone(),
        Err(e) => {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
            return;
        }
    };
    s.anim_clips.mixers.end_interaction(interacting(ui.ctx()));
    let shown = s.audio_mixer.mixer.as_deref() == Some(rel);
    if let Some(item) = s.audio_mixer.selected.filter(|_| shown)
        && mixer.group(item.group()).is_some()
    {
        group(ui, s, rel, &mixer, item);
        return;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(MIXER_ICON).size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{} (Audio Mixer)", clip_name(rel)),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    Grid::new("mixer_summary").num_columns(2).show(ui, |ui| {
        ui.label("Groups");
        ui.label(mixer.groups.len().to_string());
        ui.end_row();
        ui.label("Snapshots");
        ui.label(mixer.snapshots.len().to_string());
        ui.end_row();
        ui.label("Start Snapshot");
        ui.label(&mixer.snapshots[mixer.start_snapshot].name);
        ui.end_row();
        ui.label("Exposed Parameters");
        ui.label(mixer.exposed.len().to_string());
        ui.end_row();
        ui.label("Output");
        output_combo(ui, s, rel, &mixer);
        ui.end_row();
    });
    ui.add_space(6.0);
    if ui.button("Open Audio Mixer Window").clicked() {
        s.audio_mixer.open(Some(rel.to_owned()));
        s.push(Action::FocusTab(Tab::AudioMixer));
    }
}

/// Nested mixers: the group of another mixer this one's Master plays into.
fn output_combo(ui: &mut Ui, s: &mut EditorState, rel: &str, mixer: &AudioMixer) {
    let current = mixer
        .output
        .as_deref()
        .map_or("None (Audio Listener)".to_owned(), |o| {
            let (p, g) = split_output(o);
            format!("{} ▸ {g}", clip_name(p))
        });
    let others: Vec<String> = s
        .assets
        .mixers
        .iter()
        .filter(|p| p.as_str() != rel)
        .cloned()
        .collect();
    let mut groups: Vec<(String, String)> = Vec::new();
    for p in others {
        if let Ok(m) = s.anim_clips.mixers.get(&p) {
            for (g, depth) in m.tree() {
                if let Some(g) = m.group(g) {
                    let label = format!("{}{} ▸ {}", "  ".repeat(depth), clip_name(&p), g.name);
                    groups.push((output_ref(&p, &g.name), label));
                }
            }
        }
    }
    let mut pick: Option<Option<String>> = None;
    egui::ComboBox::from_id_salt("mixer_output")
        .selected_text(current)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(mixer.output.is_none(), "None (Audio Listener)")
                .clicked()
            {
                pick = Some(None);
            }
            for (o, label) in groups {
                if ui
                    .selectable_label(mixer.output.as_deref() == Some(o.as_str()), label)
                    .clicked()
                {
                    pick = Some(Some(o));
                }
            }
        });
    if let Some(o) = pick {
        audio_mixers::edit(s, rel, "Set Mixer Output", false, |m| m.output = o);
    }
}

/// Value of `key` in the edited snapshot (live while playing).
fn value(s: &mut EditorState, rel: &str, mixer: &AudioMixer, key: &str) -> f32 {
    let live = s
        .runtime
        .as_ref()
        .and_then(|rt| rt.with_audio(|a| a.mixer_at(rel).map(|m| m.value(key))));
    live.unwrap_or_else(|| mixer.value(s.audio_mixer.snapshot, key))
}

/// A slider for one mixer value; right-click exposes it. Returns the new value if changed.
#[allow(clippy::too_many_arguments)]
fn value_row(
    ui: &mut Ui,
    s: &mut EditorState,
    rel: &str,
    mixer: &AudioMixer,
    label: &str,
    key: &str,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) {
    let exposed = mixer
        .exposed
        .iter()
        .find(|p| p.key == key)
        .map(|p| p.name.clone());
    let text = match &exposed {
        Some(_) => RichText::new(format!("{label} ➜")).color(theme::PREFAB),
        None => RichText::new(label),
    };
    let label_resp = ui.label(text);
    let mut v = value(s, rel, mixer, key);
    let resp = ui.add(egui::Slider::new(&mut v, range).suffix(suffix));
    if resp.changed() {
        let snap = s.audio_mixer.snapshot;
        let key = key.to_owned();
        audio_mixers::edit(s, rel, &format!("Set {label}"), true, |m| {
            m.set_value(snap, &key, v)
        });
    }
    for r in [label_resp, resp] {
        r.context_menu(|ui| match &exposed {
            Some(name) => {
                if ui.button(format!("Unexpose '{name}'")).clicked() {
                    let key = key.to_owned();
                    audio_mixers::edit(s, rel, "Unexpose Parameter", false, |m| m.unexpose(&key));
                    ui.close();
                }
            }
            None => {
                if ui.button(format!("Expose '{label}' to script")).clicked() {
                    let key = key.to_owned();
                    audio_mixers::edit(s, rel, "Expose Parameter", false, |m| {
                        m.expose(&key);
                    });
                    ui.close();
                }
            }
        });
    }
    ui.end_row();
}

/// Send target picker: the Receive / Duck Volume units of the other groups.
fn send_target_row(
    ui: &mut Ui,
    s: &mut EditorState,
    rel: &str,
    mixer: &AudioMixer,
    gid: u32,
    fx: &MixerEffect,
) {
    ui.label("Receive");
    let current = fx
        .target
        .map_or("None".to_owned(), |t| mixer.effect_label(t));
    let mut pick: Option<Option<u32>> = None;
    egui::ComboBox::from_id_salt(("send_target", gid, fx.id))
        .selected_text(current)
        .show_ui(ui, |ui| {
            if ui.selectable_label(fx.target.is_none(), "None").clicked() {
                pick = Some(None);
            }
            for (g, e) in mixer.receivers() {
                if g == gid {
                    continue;
                }
                let on = fx.target == Some(e);
                if ui.selectable_label(on, mixer.effect_label(e)).clicked() {
                    pick = Some(Some(e));
                }
            }
        });
    if let Some(target) = pick {
        let eid = fx.id;
        audio_mixers::edit(s, rel, "Set Send Target", false, |m| {
            m.set_send_target(gid, eid, target);
        });
    }
    ui.end_row();
}

/// Magnitude response of a filter / EQ (20 Hz – 20 kHz, -24..+12 dB).
fn response_curve(ui: &mut Ui, kind: EffectKind, values: &[f32]) {
    let width = ui.available_width().clamp(160.0, 320.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 70.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, theme::FIELD_BG);
    let y_of = |db: f32| rect.bottom() - (db + 24.0) / 36.0 * rect.height();
    p.hline(
        rect.x_range(),
        y_of(0.0),
        egui::Stroke::new(1.0, theme::BORDER),
    );
    let pts: Vec<egui::Pos2> = (0..=120)
        .map(|i| {
            let x = i as f32 / 120.0;
            let f = 20.0 * 1000f32.powf(x);
            let db = 20.0 * response(kind, values, f, 48000.0).max(1e-4).log10();
            egui::pos2(rect.left() + x * rect.width(), y_of(db.clamp(-24.0, 12.0)))
        })
        .collect();
    p.add(egui::Shape::line(
        pts,
        egui::Stroke::new(1.5, egui::Color32::from_rgb(110, 180, 230)),
    ));
    for (f, label) in [(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        let x = rect.left() + (f / 20.0f32).log10() / 3.0 * rect.width();
        p.text(
            egui::pos2(x, rect.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            label,
            egui::FontId::monospace(9.0),
            theme::TEXT_DIM,
        );
    }
}

fn group(ui: &mut Ui, s: &mut EditorState, rel: &str, mixer: &AudioMixer, item: MixerItem) {
    let MixerItem::Group(gid, highlight) = item;
    let Some(g) = mixer.group(gid).cloned() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(MIXER_ICON).size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{} (Audio Mixer Group)", g.name),
        ));
    });
    ui.label(
        RichText::new(format!(
            "{} · snapshot {}",
            mixer.path(gid),
            mixer.snapshots[s.audio_mixer.snapshot.min(mixer.snapshots.len() - 1)].name
        ))
        .color(theme::TEXT_DIM)
        .small(),
    );
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            Grid::new("mixer_group")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    let mut name = g.name.clone();
                    let resp = ui.add_enabled(gid != MASTER, egui::TextEdit::singleline(&mut name));
                    if resp.changed() {
                        audio_mixers::edit(s, rel, "Rename Group", true, |m| {
                            m.rename_group(gid, &name);
                        });
                    }
                    ui.end_row();
                    value_row(
                        ui,
                        s,
                        rel,
                        mixer,
                        "Pitch",
                        &pitch_key(gid),
                        0.01..=10.0,
                        "×",
                    );
                    value_row(
                        ui,
                        s,
                        rel,
                        mixer,
                        "Volume",
                        &volume_key(gid),
                        MIN_DB..=MAX_DB,
                        " dB",
                    );
                });
            for fx in &g.effects {
                let open = highlight == Some(fx.id);
                let id = ui.make_persistent_id(("mixer_fx", gid, fx.id));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    true,
                )
                .show_header(ui, |ui| {
                    let mut on = !fx.bypass;
                    if ui.checkbox(&mut on, "").changed() {
                        let eid = fx.id;
                        audio_mixers::edit(s, rel, "Bypass Effect", false, |m| {
                            if let Some(e) = m
                                .group_mut(gid)
                                .and_then(|g| g.effects.iter_mut().find(|e| e.id == eid))
                            {
                                e.bypass = !on;
                            }
                        });
                    }
                    let title = crate::theme::bold(ui.ctx(), fx.kind.label());
                    ui.label(if open {
                        title.color(theme::PREFAB)
                    } else {
                        title
                    });
                })
                .body(|ui| {
                    Grid::new(("fx_grid", gid, fx.id))
                        .num_columns(2)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            if fx.kind == EffectKind::Send {
                                send_target_row(ui, s, rel, mixer, gid, fx);
                            }
                            if fx.kind.receives_sends() {
                                ui.label("Sends from");
                                let from: Vec<String> = mixer
                                    .sends_to(fx.id)
                                    .iter()
                                    .filter_map(|(g, _)| mixer.group(*g).map(|g| g.name.clone()))
                                    .collect();
                                ui.label(if from.is_empty() {
                                    "None (add a Send on another group)".to_owned()
                                } else {
                                    from.join(", ")
                                });
                                ui.end_row();
                            }
                            for d in fx.kind.params() {
                                let suffix = if d.unit.is_empty() {
                                    String::new()
                                } else {
                                    format!(" {}", d.unit)
                                };
                                value_row(
                                    ui,
                                    s,
                                    rel,
                                    mixer,
                                    d.name,
                                    &effect_key(gid, fx.id, d.name),
                                    d.min..=d.max,
                                    &suffix,
                                );
                            }
                            if fx.kind.has_wet() {
                                value_row(
                                    ui,
                                    s,
                                    rel,
                                    mixer,
                                    "Wet Mix Level",
                                    &effect_key(gid, fx.id, WET),
                                    0.0..=100.0,
                                    " %",
                                );
                            }
                            if matches!(fx.kind, EffectKind::DuckVolume | EffectKind::Compressor)
                                && s.runtime.is_some()
                            {
                                ui.label("Gain reduction");
                                ui.label(format!("{:.1} dB", s.audio_mixer.reduction(gid)));
                                ui.end_row();
                            }
                        });
                    if matches!(
                        fx.kind,
                        EffectKind::Lowpass | EffectKind::Highpass | EffectKind::ParamEQ
                    ) {
                        let values: Vec<f32> = fx
                            .kind
                            .params()
                            .iter()
                            .map(|d| value(s, rel, mixer, &effect_key(gid, fx.id, d.name)))
                            .collect();
                        response_curve(ui, fx.kind, &values);
                    }
                });
            }
            ui.add_space(6.0);
            crate::panels::audio_mixer::add_effect_menu(ui, s, rel, gid);
        });
}
