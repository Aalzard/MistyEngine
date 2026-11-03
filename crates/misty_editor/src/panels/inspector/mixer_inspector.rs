//! Inspector for Audio Mixer assets: a summary, or — when a group is selected in the Audio Mixer
//! window — that group's pitch, volume and effect parameters for the edited snapshot. Values
//! can be exposed to scripts from their right-click menu.

use egui::{Grid, RichText, Ui};
use misty_engine::audio::effects::{MAX_DB, MIN_DB};
use misty_engine::audio::mixer::{AudioMixer, MASTER, effect_key, pitch_key, volume_key};
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
        ui.label(RichText::new(format!("{} (Audio Mixer)", clip_name(rel))).strong());
    });
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
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
    });
    ui.add_space(6.0);
    if ui.button("Open Audio Mixer Window").clicked() {
        s.audio_mixer.open(Some(rel.to_owned()));
        s.push(Action::FocusTab(Tab::AudioMixer));
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

fn group(ui: &mut Ui, s: &mut EditorState, rel: &str, mixer: &AudioMixer, item: MixerItem) {
    let MixerItem::Group(gid, highlight) = item;
    let Some(g) = mixer.group(gid).cloned() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(MIXER_ICON).size(22.0));
        ui.label(RichText::new(format!("{} (Audio Mixer Group)", g.name)).strong());
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
                    let title = RichText::new(fx.kind.label()).strong();
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
                        });
                });
            }
            ui.add_space(6.0);
            crate::panels::audio_mixer::add_effect_menu(ui, s, rel, gid);
        });
}
