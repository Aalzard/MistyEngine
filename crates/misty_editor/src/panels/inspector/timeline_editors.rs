//! Inspector editors for Timeline: the Playable Director component (asset, update method,
//! wrap mode, one object field per bound track) and a `.timeline` asset showing the clip,
//! marker or track selected in the Timeline window.

use egui::{DragValue, RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::{DirectorUpdateMode, DirectorWrapMode, PlayableDirector, registry};
use misty_engine::timeline::{
    DurationMode, Extrapolation, PostPlayback, TimelineAsset, TimelineClip, TrackKind,
};

use super::card::{grid, grid_sub_header};
use crate::anim_controllers::interacting;
use crate::panels::timeline::{ICON, TimelineSel};
use crate::state::EditorState;
use crate::{theme, timelines};

/// Names of the components handled here.
pub const NAMES: [&str; 1] = ["Playable Director"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Playable Director").then_some(ICON)
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&PlayableDirector>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    let _ = state
        .scene
        .world
        .get::<&mut PlayableDirector>(e)
        .map(|mut c| c.enabled = on);
}

fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    v: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*v))
        .show_ui(ui, |ui| {
            for a in all {
                ch |= ui.selectable_value(v, *a, label(*a)).changed();
            }
        });
    ch
}

/// Playable Director: Playable, Update Method, Play On Awake, Wrap Mode, Initial Time and the
/// track bindings.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut d) = state
        .scene
        .world
        .get::<&PlayableDirector>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let timelines_list: Vec<String> = state.assets.timelines.iter().cloned().collect();
    let asset = d
        .playable
        .clone()
        .and_then(|p| state.anim_clips.timelines.get(&p).ok().cloned());
    let objects: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| (o, state.scene.name(o)))
        .collect();
    let mut open = false;
    let changed = grid(ui, "director_grid", |ui| {
        let mut ch = false;
        ui.label("Playable");
        ui.horizontal(|ui| {
            ch |= timeline_slot(ui, &timelines_list, &mut d.playable);
            open = d.playable.is_some()
                && ui
                    .small_button("Edit")
                    .on_hover_text("Open in the Timeline window")
                    .clicked();
        });
        ui.end_row();
        ui.label("Update Method");
        ch |= combo(
            ui,
            "director_update",
            &mut d.update_method,
            &DirectorUpdateMode::ALL,
            DirectorUpdateMode::label,
        );
        ui.end_row();
        ui.label("Play On Awake");
        ch |= ui.checkbox(&mut d.play_on_awake, "").changed();
        ui.end_row();
        ui.label("Wrap Mode");
        ch |= combo(
            ui,
            "director_wrap",
            &mut d.wrap_mode,
            &DirectorWrapMode::ALL,
            DirectorWrapMode::label,
        );
        ui.end_row();
        ui.label("Initial Time");
        ch |= ui
            .add(
                DragValue::new(&mut d.initial_time)
                    .speed(0.01)
                    .range(0.0..=100_000.0)
                    .suffix(" s"),
            )
            .changed();
        ui.end_row();
        if let Some(asset) = &asset {
            let bound: Vec<_> = asset.tracks.iter().filter(|t| t.kind.bindable()).collect();
            if !bound.is_empty() {
                grid_sub_header(ui, "Bindings");
            }
            for t in bound {
                ui.label(&t.name);
                let need = t.kind.binding_component();
                let fits = |o: Entity| {
                    need.is_empty()
                        || registry::find(need)
                            .is_some_and(|info| (info.has)(&state.scene.world, o))
                };
                let mut v = d.binding(&t.id);
                let name = v
                    .and_then(|b| objects.iter().find(|(o, _)| *o == b))
                    .map_or("None".to_owned(), |(_, n)| n.clone());
                let before = v;
                egui::ComboBox::from_id_salt(("director_binding", &t.id))
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut v, None, "None");
                        for (o, n) in objects.iter().filter(|(o, _)| fits(*o)) {
                            ui.selectable_value(&mut v, Some(*o), n);
                        }
                    });
                if v != before {
                    d.set_binding(&t.id, v);
                    ch = true;
                }
                ui.end_row();
            }
        }
        ch
    });
    if open {
        state.timeline.director = Some(e);
        state.push(crate::actions::Action::FocusTab(
            crate::layout::Tab::Timeline,
        ));
    }
    if changed {
        let _ = state.scene.world.insert_one(e, d);
    }
    changed
}

/// Timeline asset picker: project `.timeline` files, or one dragged from the Project window.
fn timeline_slot(ui: &mut Ui, list: &[String], value: &mut Option<String>) -> bool {
    let mut ch = false;
    let none = "None (Timeline)";
    let current = value.as_deref().map(misty_engine::scene::clip_name);
    let slot = egui::ComboBox::from_id_salt("director_playable")
        .selected_text(format!("{ICON} {}", current.as_deref().unwrap_or(none)))
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, none).changed();
            for t in list {
                ch |= ui
                    .selectable_value(value, Some(t.clone()), misty_engine::scene::clip_name(t))
                    .changed();
            }
        })
        .response;
    if let Some(p) = slot.dnd_release_payload::<crate::panels::AssetPayload>()
        && misty_engine::timeline::is_timeline(&p.0)
    {
        *value = Some(p.0.clone());
        ch = true;
    }
    ch
}

fn header(ui: &mut Ui, title: &str, kind: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(ICON).size(20.0));
        ui.label(theme::bold(ui.ctx(), title));
        if !kind.is_empty() {
            ui.label(RichText::new(kind).color(theme::TEXT_DIM));
        }
    });
    ui.add_space(4.0);
}

/// A `.timeline` asset: the Timeline window's selected clip / marker / track, or its settings.
pub fn timeline_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let asset = match state.anim_clips.timelines.get(rel) {
        Ok(t) => t.clone(),
        Err(e) => {
            ui.colored_label(theme::AXIS_X, e);
            return;
        }
    };
    let busy = interacting(ui.ctx());
    let shown = crate::panels::timeline::current(state).is_some_and(|(_, p)| p == rel);
    let sel = shown.then(|| state.timeline.selected.clone()).flatten();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| match sel {
            Some(TimelineSel::Clip(id, i)) => {
                if let Some(c) = asset.track(&id).and_then(|t| t.clips.get(i)) {
                    let kind = asset.track(&id).map_or(TrackKind::Activation, |t| t.kind);
                    clip_editor(
                        ui,
                        state,
                        rel,
                        &id,
                        i,
                        kind,
                        c.clone(),
                        asset.frame_rate,
                        busy,
                    );
                }
            }
            Some(TimelineSel::Marker(track, i)) => {
                marker_editor(ui, state, rel, &asset, track, i, busy)
            }
            Some(TimelineSel::Track(id)) => track_editor(ui, state, rel, &asset, &id, busy),
            None => summary(ui, state, rel, &asset, busy),
        });
}

#[allow(clippy::too_many_arguments)]
fn clip_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    track: &str,
    i: usize,
    kind: TrackKind,
    mut c: TimelineClip,
    rate: f32,
    busy: bool,
) {
    header(ui, &c.name, "  Clip");
    let before = c.clone();
    let secs = |v: &mut f32, ui: &mut Ui| {
        ui.add(
            DragValue::new(v)
                .speed(1.0 / rate)
                .range(0.0..=100_000.0)
                .suffix(" s"),
        )
        .changed()
    };
    grid(ui, "tl_clip_grid", |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut c.name);
        ui.end_row();
        ui.label("Start");
        secs(&mut c.start, ui);
        ui.end_row();
        ui.label("Duration");
        secs(&mut c.duration, ui);
        ui.end_row();
        ui.label("End");
        ui.label(format!("{:.2} s", c.end()));
        ui.end_row();
        if kind != TrackKind::Activation {
            grid_sub_header(ui, "Blend");
            ui.label("Ease In");
            secs(&mut c.ease_in, ui);
            ui.end_row();
            ui.label("Ease Out");
            secs(&mut c.ease_out, ui);
            ui.end_row();
            grid_sub_header(ui, "Clip");
            ui.label("Clip In");
            secs(&mut c.clip_in, ui);
            ui.end_row();
            ui.label("Speed");
            ui.add(DragValue::new(&mut c.speed).speed(0.01).range(0.01..=100.0));
            ui.end_row();
        }
        if kind == TrackKind::Animation {
            ui.label("Post-Extrapolate");
            combo(
                ui,
                "tl_clip_extrap",
                &mut c.post_extrapolation,
                &[Extrapolation::None, Extrapolation::Hold],
                |e| match e {
                    Extrapolation::None => "None",
                    Extrapolation::Hold => "Hold",
                },
            );
            ui.end_row();
        }
        if let Some(a) = &c.asset {
            ui.label("Source");
            ui.label(RichText::new(a.as_str()).color(theme::TEXT_DIM));
            ui.end_row();
        }
        if kind.clips_reference_objects() {
            reference_row(ui, state, rel, kind, &mut c);
        }
        if kind == TrackKind::Control {
            grid_sub_header(ui, "Control");
            let o = &mut c.control;
            for (label, v, tip) in [
                (
                    "Activation",
                    &mut o.activation,
                    "The object is active only while the clip plays",
                ),
                (
                    "Sub-Timeline",
                    &mut o.sub_timeline,
                    "Play the object's Playable Director timeline at the clip's local time",
                ),
                (
                    "Particles",
                    &mut o.particles,
                    "Play its particle systems while the clip plays (Play mode)",
                ),
                (
                    "Audio",
                    &mut o.audio,
                    "Play its Audio Source while the clip plays (Play mode)",
                ),
                (
                    "Impulse",
                    &mut o.impulse,
                    "Fire its Impulse Source when the clip starts (Play mode)",
                ),
            ] {
                ui.label(label).on_hover_text(tip);
                ui.checkbox(v, "");
                ui.end_row();
            }
            ui.label("Post-playback State");
            combo(
                ui,
                "tl_ctl_post",
                &mut o.post_playback,
                &PostPlayback::ALL,
                PostPlayback::label,
            );
            ui.end_row();
        }
    });
    c.duration = c.duration.max(1.0 / rate);
    c.ease_in = c.ease_in.min(c.duration);
    c.ease_out = c.ease_out.min(c.duration);
    if c != before {
        let track = track.to_owned();
        timelines::edit(state, rel, "Edit Clip", busy, |t| {
            if let Some(x) = t.track_mut(&track).and_then(|tr| tr.clips.get_mut(i)) {
                *x = c;
            }
        });
    }
}

/// Camera / Control clip: the scene object it points at, kept on the window's director (a
/// clip without a reference name gets one).
fn reference_row(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    kind: TrackKind,
    c: &mut TimelineClip,
) {
    let label = if kind == TrackKind::Camera {
        "Virtual Camera"
    } else {
        "Source Object"
    };
    ui.label(label);
    let Some((Some(d), _)) = crate::panels::timeline::current(state) else {
        ui.label(RichText::new("Open it on a director to pick").color(theme::TEXT_DIM));
        ui.end_row();
        return;
    };
    let current = c
        .reference
        .as_deref()
        .and_then(|r| crate::panels::timeline::director(state, d)?.reference(r))
        .filter(|e| state.scene.contains(*e));
    let objects: Vec<(Entity, String)> = if kind == TrackKind::Camera {
        misty_engine::vcam::rig::active_cameras(&state.scene)
            .into_iter()
            .map(|e| (e, state.scene.name(e)))
            .collect()
    } else {
        crate::panels::timeline::ops::controllable(state)
    };
    let shown = current.map_or("None".to_owned(), |e| state.scene.name(e));
    let mut chosen = current;
    egui::ComboBox::from_id_salt("tl_clip_reference")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, "None");
            for (e, n) in &objects {
                ui.selectable_value(&mut chosen, Some(*e), n);
            }
        });
    ui.end_row();
    if chosen != current {
        let name = match c.reference.clone() {
            Some(n) => n,
            None => {
                let n = state
                    .anim_clips
                    .timelines
                    .get(rel)
                    .map(|t| t.next_reference())
                    .unwrap_or_else(|_| "r1".into());
                c.reference = Some(n.clone());
                n
            }
        };
        crate::panels::timeline::ops::set_reference(state, d, &name, chosen);
        if let Some(e) = chosen {
            c.name = state.scene.name(e);
        }
    }
}

fn marker_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    asset: &TimelineAsset,
    track: Option<String>,
    i: usize,
    busy: bool,
) {
    let list = match &track {
        Some(id) => asset.track(id).map(|t| &t.markers),
        None => Some(&asset.markers),
    };
    let Some(mut m) = list.and_then(|l| l.get(i)).cloned() else {
        return;
    };
    header(ui, "Signal", "  Marker");
    let before = m.clone();
    grid(ui, "tl_marker_grid", |ui| {
        ui.label("Time");
        ui.add(
            DragValue::new(&mut m.time)
                .speed(1.0 / asset.frame_rate)
                .range(0.0..=100_000.0)
                .suffix(" s"),
        );
        ui.end_row();
        ui.label("Method");
        ui.text_edit_singleline(&mut m.method)
            .on_hover_text("Script method called on the bound object (SendMessage)");
        ui.end_row();
        ui.label("Parameter");
        let mut text = m.parameter.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut text).changed() {
            m.parameter = (!text.is_empty()).then_some(text);
        }
        ui.end_row();
        ui.label("Retroactive");
        ui.checkbox(&mut m.retroactive, "")
            .on_hover_text("Also fires when playback starts after it");
        ui.end_row();
    });
    if m != before {
        timelines::edit(state, rel, "Edit Marker", busy, |t| {
            let list = match &track {
                Some(id) => t.track_mut(id).map(|tr| &mut tr.markers),
                None => Some(&mut t.markers),
            };
            if let Some(x) = list.and_then(|l| l.get_mut(i)) {
                *x = m;
            }
        });
    }
}

fn track_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    asset: &TimelineAsset,
    id: &str,
    busy: bool,
) {
    let Some(mut t) = asset.track(id).cloned() else {
        return;
    };
    header(ui, &t.name, &format!("  {}", t.kind.label()));
    let before = t.clone();
    grid(ui, "tl_track_grid", |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut t.name);
        ui.end_row();
        ui.label("Mute");
        ui.checkbox(&mut t.muted, "");
        ui.end_row();
        ui.label("Lock");
        ui.checkbox(&mut t.locked, "");
        ui.end_row();
        if t.kind == TrackKind::Activation {
            ui.label("Post-playback State");
            combo(
                ui,
                "tl_post_playback",
                &mut t.post_playback,
                &PostPlayback::ALL,
                PostPlayback::label,
            );
            ui.end_row();
        }
    });
    if t != before {
        timelines::edit(state, rel, "Edit Track", busy, |a| {
            if let Some(x) = a.track_mut(id) {
                *x = t;
            }
        });
    }
}

fn summary(ui: &mut Ui, state: &mut EditorState, rel: &str, asset: &TimelineAsset, busy: bool) {
    header(ui, &misty_engine::scene::clip_name(rel), "  Timeline");
    let mut t = asset.clone();
    grid(ui, "tl_asset_grid", |ui| {
        ui.label("Frame Rate");
        ui.add(
            DragValue::new(&mut t.frame_rate)
                .range(1.0..=240.0)
                .speed(1.0)
                .max_decimals(0),
        );
        ui.end_row();
        ui.label("Duration Mode");
        combo(
            ui,
            "tl_duration_mode",
            &mut t.duration_mode,
            &[DurationMode::BasedOnClips, DurationMode::FixedLength],
            |d| match d {
                DurationMode::BasedOnClips => "Based On Clips",
                DurationMode::FixedLength => "Fixed Length",
            },
        );
        ui.end_row();
        ui.label("Duration");
        if t.duration_mode == DurationMode::FixedLength {
            ui.add(
                DragValue::new(&mut t.fixed_duration)
                    .speed(0.05)
                    .range(0.0..=100_000.0)
                    .suffix(" s"),
            );
        } else {
            ui.label(format!("{:.2} s", t.duration()));
        }
        ui.end_row();
        ui.label("Tracks");
        ui.label(t.tracks.len().to_string());
        ui.end_row();
    });
    if t != *asset {
        timelines::edit(state, rel, "Edit Timeline", busy, |a| *a = t);
    }
}
