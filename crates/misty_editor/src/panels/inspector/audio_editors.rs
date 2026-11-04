//! Inspector editors for the Audio Source (with its 3D Sound Settings and rolloff graph), the
//! Audio Reverb Zone and the Low Pass / High Pass / Echo filters.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::audio::mixer::AudioMixer;
use misty_engine::audio::spatial::rolloff_gain;
use misty_engine::audio::{output_ref, split_output};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    AudioEchoFilter, AudioHighPassFilter, AudioLowPassFilter, AudioReverbZone, AudioRolloff,
    AudioSource, ReverbPreset, SceneComponent, clip_name,
};

use super::editors::{combo_k, grid, slider};
use super::mixed;
use crate::panels::AssetPayload;
use crate::panels::audio_mixer::MIXER_ICON;
use crate::state::EditorState;
use crate::theme;

/// Components handled here besides the Audio Source.
pub const NAMES: [&str; 4] = [
    "Audio Reverb Zone",
    "Audio Low Pass Filter",
    "Audio High Pass Filter",
    "Audio Echo Filter",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Audio Reverb Zone" => "◎",
        "Audio Low Pass Filter" | "Audio High Pass Filter" | "Audio Echo Filter" => "≋",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Audio Reverb Zone" => w.get::<&AudioReverbZone>(e).ok().map(|c| c.enabled),
        "Audio Low Pass Filter" => w.get::<&AudioLowPassFilter>(e).ok().map(|c| c.enabled),
        "Audio High Pass Filter" => w.get::<&AudioHighPassFilter>(e).ok().map(|c| c.enabled),
        "Audio Echo Filter" => w.get::<&AudioEchoFilter>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    let _ = match name {
        "Audio Reverb Zone" => w.get::<&mut AudioReverbZone>(e).map(|mut c| c.enabled = on),
        "Audio Low Pass Filter" => w
            .get::<&mut AudioLowPassFilter>(e)
            .map(|mut c| c.enabled = on),
        "Audio High Pass Filter" => w
            .get::<&mut AudioHighPassFilter>(e)
            .map(|mut c| c.enabled = on),
        "Audio Echo Filter" => w.get::<&mut AudioEchoFilter>(e).map(|mut c| c.enabled = on),
        _ => return,
    };
}

/// Edit a copy of `T` with `f` and store it if anything changed.
fn edit_copy<T: SceneComponent>(
    state: &mut EditorState,
    e: Entity,
    f: impl FnOnce(&mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = f(&mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Audio Reverb Zone" => edit_copy::<AudioReverbZone>(state, e, |z| reverb_zone(ui, z)),
        "Audio Low Pass Filter" => edit_copy::<AudioLowPassFilter>(state, e, |f| {
            filter(ui, "lowpass", &mut f.cutoff_frequency, &mut f.resonance_q)
        }),
        "Audio High Pass Filter" => edit_copy::<AudioHighPassFilter>(state, e, |f| {
            filter(ui, "highpass", &mut f.cutoff_frequency, &mut f.resonance_q)
        }),
        "Audio Echo Filter" => edit_copy::<AudioEchoFilter>(state, e, |f| echo(ui, f)),
        _ => false,
    }
}

fn number(ui: &mut Ui, label: &str, key: &str, v: &mut f32, speed: f64, range: (f32, f32)) -> bool {
    ui.label(label);
    let ch = ui
        .add(
            mixed::drag(key, egui::DragValue::new(v))
                .speed(speed)
                .range(range.0..=range.1),
        )
        .changed();
    ui.end_row();
    ch
}

fn reverb_zone(ui: &mut Ui, z: &mut AudioReverbZone) -> bool {
    grid(ui, "reverb_zone", |ui| {
        let mut ch = number(
            ui,
            "Min Distance",
            "min_distance",
            &mut z.min_distance,
            0.1,
            (0.0, z.max_distance),
        );
        ch |= number(
            ui,
            "Max Distance",
            "max_distance",
            &mut z.max_distance,
            0.1,
            (z.min_distance, 1e6),
        );
        ui.label("Reverb Preset");
        ch |= combo_k(
            ui,
            "zone_preset",
            "preset",
            &mut z.preset,
            &ReverbPreset::ALL,
            ReverbPreset::label,
        );
        ui.end_row();
        ch
    })
}

fn filter(ui: &mut Ui, id: &str, cutoff: &mut f32, q: &mut f32) -> bool {
    grid(ui, id, |ui| {
        ui.label("Cutoff Frequency");
        let mut ch = slider(ui, "cutoff_frequency", cutoff, 10.0..=22000.0);
        ui.end_row();
        ui.label("Resonance Q");
        ch |= slider(ui, "resonance_q", q, 1.0..=10.0);
        ui.end_row();
        ch
    })
}

fn echo(ui: &mut Ui, f: &mut AudioEchoFilter) -> bool {
    grid(ui, "echo_filter", |ui| {
        let mut ch = number(ui, "Delay", "delay", &mut f.delay, 1.0, (10.0, 5000.0));
        for (label, key, v) in [
            ("Decay Ratio", "decay_ratio", &mut f.decay_ratio),
            ("Wet Mix", "wet_mix", &mut f.wet_mix),
            ("Dry Mix", "dry_mix", &mut f.dry_mix),
        ] {
            ui.label(label);
            ch |= slider(ui, key, v, 0.0..=1.0);
            ui.end_row();
        }
        ch
    })
}

// ---- Audio Source -----------------------------------------------------------------------------

/// Audio clip slot: pick from the project's clips, or drop one from the Project window.
fn clip_field(ui: &mut Ui, id: &str, clips: &[String], clip: &mut Option<String>) -> bool {
    let mut ch = false;
    let current = clip
        .as_deref()
        .map(clip_name)
        .unwrap_or_else(|| "None (Audio Clip)".into());
    let slot = egui::ComboBox::from_id_salt(id)
        .selected_text(mixed::text(
            "clip",
            format!("{} {current}", crate::state::AUDIO_ICON),
        ))
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(clip, None, "None (Audio Clip)")
                .changed();
            for c in clips {
                ch |= ui
                    .selectable_value(clip, Some(c.clone()), clip_name(c))
                    .changed();
            }
        })
        .response;
    if let Some(path) = slot.dnd_release_payload::<AssetPayload>()
        && misty_engine::audio::is_clip(&path.0)
    {
        *clip = Some(path.0.clone());
        ch = true;
    }
    ch
}

/// Output slot: every group of every project mixer ("Main ▸ SFX"), or drop a mixer (Master).
fn output_field(ui: &mut Ui, mixers: &[(String, AudioMixer)], output: &mut Option<String>) -> bool {
    let mut ch = false;
    let label = |o: &str| {
        let (path, group) = split_output(o);
        format!("{} ▸ {group}", clip_name(path))
    };
    let current = output
        .as_deref()
        .map(label)
        .unwrap_or_else(|| "None (Audio Mixer Group)".into());
    let slot = egui::ComboBox::from_id_salt("as_output")
        .selected_text(mixed::text("output", format!("{MIXER_ICON} {current}")))
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(output, None, "None (Audio Mixer Group)")
                .changed();
            for (path, m) in mixers {
                for (g, depth) in m.tree() {
                    let Some(group) = m.group(g) else { continue };
                    let o = output_ref(path, &group.name);
                    let text = format!("{}{}", "  ".repeat(depth), label(&o));
                    ch |= ui.selectable_value(output, Some(o), text).changed();
                }
            }
        })
        .response;
    if let Some(path) = slot.dnd_release_payload::<AssetPayload>()
        && misty_engine::audio::is_mixer(&path.0)
    {
        *output = Some(output_ref(&path.0, "Master"));
        ch = true;
    }
    ch
}

/// Slider with captions under its ends ("2D" … "3D").
fn captioned(
    ui: &mut Ui,
    key: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    ends: (&str, &str),
) -> bool {
    let mut ch = false;
    ui.vertical(|ui| {
        ch = slider(ui, key, v, range);
        ui.horizontal(|ui| {
            ui.label(RichText::new(ends.0).small().color(theme::TEXT_DIM));
            ui.add_space(100.0);
            ui.label(RichText::new(ends.1).small().color(theme::TEXT_DIM));
        });
    });
    ch
}

pub fn audio_source(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut a) = state
        .scene
        .world
        .get::<&AudioSource>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let clips: Vec<String> = state.assets.clips.iter().cloned().collect();
    let paths: Vec<String> = state.assets.mixers.iter().cloned().collect();
    let mixers: Vec<(String, AudioMixer)> = paths
        .into_iter()
        .filter_map(|p| {
            let m = state.anim_clips.mixers.get(&p).ok()?.clone();
            Some((p, m))
        })
        .collect();
    let mut changed = grid(ui, "audiosource_grid", |ui| {
        let mut ch = false;
        ui.label("AudioClip");
        ch |= clip_field(ui, "as_clip", &clips, &mut a.clip);
        ui.end_row();
        ui.label("Output");
        ch |= output_field(ui, &mixers, &mut a.output);
        ui.end_row();
        for (label, key, flag) in [
            ("Mute", "mute", &mut a.mute),
            ("Bypass Effects", "bypass_effects", &mut a.bypass_effects),
            (
                "Bypass Listener Effects",
                "bypass_listener_effects",
                &mut a.bypass_listener_effects,
            ),
            (
                "Bypass Reverb Zones",
                "bypass_reverb_zones",
                &mut a.bypass_reverb_zones,
            ),
            ("Play On Awake", "play_on_awake", &mut a.play_on_awake),
            ("Loop", "loop", &mut a.looping),
        ] {
            ui.label(label);
            ch |= mixed::checkbox(ui, key, flag, "").changed();
            ui.end_row();
        }
        ui.label("Priority");
        let mut p = a.priority as f32;
        if captioned(ui, "priority", &mut p, 0.0..=256.0, ("High", "Low")) {
            a.priority = p.round() as u16;
            ch = true;
        }
        ui.end_row();
        ui.label("Volume");
        ch |= slider(ui, "volume", &mut a.volume, 0.0..=1.0);
        ui.end_row();
        ui.label("Pitch");
        ch |= slider(ui, "pitch", &mut a.pitch, -3.0..=3.0);
        ui.end_row();
        ui.label("Stereo Pan");
        ch |= captioned(
            ui,
            "stereo_pan",
            &mut a.stereo_pan,
            -1.0..=1.0,
            ("Left", "Right"),
        );
        ui.end_row();
        ui.label("Spatial Blend");
        ch |= captioned(
            ui,
            "spatial_blend",
            &mut a.spatial_blend,
            0.0..=1.0,
            ("2D", "3D"),
        );
        ui.end_row();
        ui.label("Reverb Zone Mix");
        ch |= slider(ui, "reverb_zone_mix", &mut a.reverb_zone_mix, 0.0..=1.1);
        ui.end_row();
        ch
    });
    let listener = listener_distance(state, e);
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "3D Sound Settings"))
        .id_salt("as_3d")
        .default_open(true)
        .show(ui, |ui| {
            changed |= grid(ui, "audiosource_3d", |ui| {
                let mut ch = false;
                ui.label("Doppler Level");
                ch |= slider(ui, "doppler_level", &mut a.doppler_level, 0.0..=5.0);
                ui.end_row();
                ui.label("Spread");
                ch |= slider(ui, "spread", &mut a.spread, 0.0..=360.0);
                ui.end_row();
                ui.label("Volume Rolloff");
                ch |= combo_k(
                    ui,
                    "as_rolloff",
                    "rolloff",
                    &mut a.rolloff,
                    &AudioRolloff::ALL,
                    AudioRolloff::label,
                );
                ui.end_row();
                let max = a.max_distance;
                ch |= number(
                    ui,
                    "Min Distance",
                    "min_distance",
                    &mut a.min_distance,
                    0.1,
                    (0.0, max),
                );
                let min = a.min_distance;
                ch |= number(
                    ui,
                    "Max Distance",
                    "max_distance",
                    &mut a.max_distance,
                    1.0,
                    (min + 0.01, 1e6),
                );
                ch
            });
            rolloff_graph(ui, &a, listener);
        });
    if changed {
        let _ = state.scene.world.insert_one(e, a);
    }
    changed
}

/// Distance from the source to the scene's Audio Listener (for the graph's marker).
fn listener_distance(state: &EditorState, e: Entity) -> Option<f32> {
    let ear = misty_engine::audio::listener(&state.scene)?;
    Some(
        ear.w_axis
            .truncate()
            .distance(state.scene.world_position(e)),
    )
}

/// Volume over distance (0 .. Max Distance × 1.1) with the Min Distance and listener marked.
fn rolloff_graph(ui: &mut Ui, a: &AudioSource, listener: Option<f32>) {
    let width = ui.available_width().clamp(160.0, 320.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 110.0), Sense::hover());
    let p = ui.painter_at(rect.expand(1.0));
    p.rect_filled(rect, 2.0, theme::FIELD_BG);
    let plot = Rect::from_min_max(rect.min + vec2(24.0, 6.0), rect.max - vec2(6.0, 16.0));
    let span = a.max_distance.max(0.01) * 1.1;
    let x_of = |d: f32| plot.left() + d / span * plot.width();
    let y_of = |v: f32| plot.bottom() - v * plot.height();
    for v in [0.0, 0.5, 1.0] {
        p.hline(plot.x_range(), y_of(v), Stroke::new(1.0, theme::BORDER));
        p.text(
            pos2(plot.left() - 3.0, y_of(v)),
            Align2::RIGHT_CENTER,
            format!("{v}"),
            FontId::monospace(9.0),
            theme::TEXT_DIM,
        );
    }
    // Spatial blend line (green) and the volume curve (red), like the reference graph.
    p.hline(
        plot.x_range(),
        y_of(a.spatial_blend.clamp(0.0, 1.0)),
        Stroke::new(1.0, Color32::from_rgb(90, 180, 90)),
    );
    let pts: Vec<egui::Pos2> = (0..=96)
        .map(|i| {
            let d = span * i as f32 / 96.0;
            let v = rolloff_gain(a.rolloff, a.min_distance, a.max_distance, d);
            pos2(x_of(d), y_of(v))
        })
        .collect();
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.5, Color32::from_rgb(220, 70, 60)),
    ));
    for (d, color) in [
        (a.min_distance, theme::TEXT_DIM),
        (a.max_distance, theme::TEXT_DIM),
    ] {
        p.vline(
            x_of(d),
            plot.y_range(),
            Stroke::new(1.0, color.gamma_multiply(0.5)),
        );
    }
    if let Some(d) = listener.filter(|d| *d <= span) {
        p.vline(
            x_of(d),
            plot.y_range(),
            Stroke::new(1.0, Color32::from_rgb(220, 200, 60)),
        );
    }
    p.text(
        pos2(plot.left(), rect.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        "0",
        FontId::monospace(9.0),
        theme::TEXT_DIM,
    );
    p.text(
        pos2(plot.right(), rect.bottom() - 2.0),
        Align2::RIGHT_BOTTOM,
        format!("{:.0} m", span),
        FontId::monospace(9.0),
        theme::TEXT_DIM,
    );
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("━ Volume")
                .small()
                .color(Color32::from_rgb(220, 70, 60)),
        );
        ui.label(
            RichText::new("━ Spatial")
                .small()
                .color(Color32::from_rgb(90, 180, 90)),
        );
        if listener.is_some() {
            ui.label(
                RichText::new("┃ Listener")
                    .small()
                    .color(Color32::from_rgb(220, 200, 60)),
            );
        }
    });
}
