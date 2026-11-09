//! Inspector editors for the Audio Source (with its 3D Sound Settings, Spatialize and the rolloff
//! graph, which edits the curve of Custom Rolloff), the Audio Reverb Zone, the Low Pass / High
//! Pass / Echo filters, and Audio Clip assets (info, import settings, preview).

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::audio::clip::{AudioImporter, LoadType};
use misty_engine::audio::mixer::AudioMixer;
use misty_engine::audio::spatial::distance_gain;
use misty_engine::audio::{output_ref, split_output};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    AudioEchoFilter, AudioHighPassFilter, AudioLowPassFilter, AudioReverbZone, AudioRolloff,
    AudioSource, ReverbPreset, SceneComponent, clip_name,
};

use super::editors::{combo_k, grid, slider};
use super::mixed;
use crate::i18n::tr;
use crate::panels::AssetPayload;
use crate::panels::audio_mixer::MIXER_ICON;
use crate::panels::inspector::card::prop_label;
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
    prop_label(ui, tr(label));
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
        prop_label(ui, tr!("Reverb Preset"));
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
        prop_label(ui, tr!("Cutoff Frequency"));
        let mut ch = slider(ui, "cutoff_frequency", cutoff, 10.0..=22000.0);
        ui.end_row();
        prop_label(ui, tr!("Resonance Q"));
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

// ---- Audio Clip -------------------------------------------------------------------------------

/// Audio clip asset: format and length, import settings (Load Type, Force To Mono) and a
/// ▶ / ■ preview player.
pub fn clip_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::state::AUDIO_ICON).size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{} (Audio Clip)", clip_name(rel)),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let abs = state.assets.absolute(rel);
    match misty_engine::audio::clip::info(&abs) {
        Some(info) => {
            ui.label(info.summary());
            ui.label(
                RichText::new(format!("Format: {}", info.format))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
        None => {
            ui.colored_label(
                theme::WARN,
                tr!("Can't read this clip (WAV, OGG Vorbis or MP3)"),
            );
        }
    }
    let mut imp = AudioImporter::load(&abs);
    let before = imp.clone();
    grid(ui, "clip_import", |ui| {
        prop_label(ui, tr!("Load Type"));
        let ch = combo_k(
            ui,
            "clip_load_type",
            "load_type",
            &mut imp.load_type,
            &LoadType::ALL,
            LoadType::label,
        );
        ui.end_row();
        prop_label(ui, tr!("Force To Mono"));
        ui.checkbox(&mut imp.force_to_mono, "");
        ui.end_row();
        ch
    });
    ui.label(
        RichText::new(match imp.load_type {
            LoadType::DecompressOnLoad => "Decoded into memory when first played.",
            LoadType::CompressedInMemory => "Kept encoded in memory; decoded while playing.",
            LoadType::Streaming => "Read from disk and decoded while playing (long music).",
        })
        .small()
        .color(theme::TEXT_DIM),
    );
    if imp != before {
        match imp.save(&abs) {
            Ok(()) => {
                if let Some(p) = state.clip_preview.as_mut() {
                    p.forget_clips();
                }
                if let Some(rt) = state.runtime.as_mut() {
                    rt.with_audio(|a| a.forget_clips());
                }
            }
            Err(e) => state
                .console
                .warn(format!("Could not save import settings: {e}")),
        }
    }
    let playing = state.clip_preview.as_ref().is_some_and(|a| a.any_playing());
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!playing, egui::Button::new("▶ Play"))
            .on_hover_text(tr!("Preview this clip"))
            .clicked()
        {
            let root = state.project.root.clone();
            let preview = state
                .clip_preview
                .get_or_insert_with(|| misty_engine::audio::Audio::with_device(root));
            preview.stop_all();
            preview.play_clip(rel, 1.0);
            for w in preview.take_logs() {
                state.console.warn(w);
            }
        }
        if ui
            .add_enabled(playing, egui::Button::new("■ Stop"))
            .clicked()
            && let Some(p) = state.clip_preview.as_mut()
        {
            p.stop_all();
        }
    });
    if playing {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

// ---- Audio Source -----------------------------------------------------------------------------

/// Audio clip slot: pick from the project's clips, or drop one from the Project window.
fn clip_field(ui: &mut Ui, id: &str, clips: &[String], clip: &mut Option<String>) -> bool {
    let mut ch = false;
    let current = clip
        .as_deref()
        .map(clip_name)
        .unwrap_or_else(|| tr!("None (Audio Clip)"));
    let slot = egui::ComboBox::from_id_salt(id)
        .selected_text(mixed::text(
            "clip",
            format!("{} {current}", crate::state::AUDIO_ICON),
        ))
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(clip, None, tr!("None (Audio Clip)"))
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
        .unwrap_or_else(|| tr!("None (Audio Mixer Group)"));
    let slot = egui::ComboBox::from_id_salt("as_output")
        .selected_text(mixed::text("output", format!("{MIXER_ICON} {current}")))
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(output, None, tr!("None (Audio Mixer Group)"))
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
    ends: (String, String),
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
        prop_label(ui, tr!("AudioClip"));
        ch |= clip_field(ui, "as_clip", &clips, &mut a.clip);
        ui.end_row();
        prop_label(ui, tr!("Output"));
        ch |= output_field(ui, &mixers, &mut a.output);
        ui.end_row();
        for (label, key, flag) in [
            (tr!("Mute"), "mute", &mut a.mute),
            (
                tr!("Bypass Effects"),
                "bypass_effects",
                &mut a.bypass_effects,
            ),
            (
                tr!("Bypass Listener Effects"),
                "bypass_listener_effects",
                &mut a.bypass_listener_effects,
            ),
            (
                tr!("Bypass Reverb Zones"),
                "bypass_reverb_zones",
                &mut a.bypass_reverb_zones,
            ),
            (tr!("Play On Awake"), "play_on_awake", &mut a.play_on_awake),
            (tr!("Loop"), "loop", &mut a.looping),
        ] {
            prop_label(ui, label);
            ch |= mixed::checkbox(ui, key, flag, "").changed();
            ui.end_row();
        }
        prop_label(ui, tr!("Priority"));
        let mut p = a.priority as f32;
        if captioned(
            ui,
            "priority",
            &mut p,
            0.0..=256.0,
            (tr!("High"), tr!("Low")),
        ) {
            a.priority = p.round() as u16;
            ch = true;
        }
        ui.end_row();
        prop_label(ui, tr!("Volume"));
        ch |= slider(ui, "volume", &mut a.volume, 0.0..=1.0);
        ui.end_row();
        prop_label(ui, tr!("Pitch"));
        ch |= slider(ui, "pitch", &mut a.pitch, -3.0..=3.0);
        ui.end_row();
        prop_label(ui, tr!("Stereo Pan"));
        ch |= captioned(
            ui,
            "stereo_pan",
            &mut a.stereo_pan,
            -1.0..=1.0,
            (tr!("Left"), tr!("Right")),
        );
        ui.end_row();
        prop_label(ui, tr!("Spatial Blend"));
        ch |= captioned(
            ui,
            "spatial_blend",
            &mut a.spatial_blend,
            0.0..=1.0,
            ("2D".into(), "3D".into()),
        );
        ui.end_row();
        prop_label(ui, tr!("Reverb Zone Mix"));
        ch |= slider(ui, "reverb_zone_mix", &mut a.reverb_zone_mix, 0.0..=1.1);
        ui.end_row();
        ch
    });
    let listener = listener_distance(state, e);
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), tr!("3D Sound Settings")))
        .id_salt("as_3d")
        .default_open(true)
        .show(ui, |ui| {
            changed |= grid(ui, "audiosource_3d", |ui| {
                let mut ch = false;
                prop_label(ui, tr!("Spatialize"));
                ch |= mixed::checkbox(ui, "spatialize", &mut a.spatialize, "")
                    .on_hover_text(tr!(
                        "Binaural rendering (head model) instead of panning: best on headphones"
                    ))
                    .changed();
                ui.end_row();
                prop_label(ui, tr!("Doppler Level"));
                ch |= slider(ui, "doppler_level", &mut a.doppler_level, 0.0..=5.0);
                ui.end_row();
                prop_label(ui, tr!("Spread"));
                ch |= slider(ui, "spread", &mut a.spread, 0.0..=360.0);
                ui.end_row();
                prop_label(ui, tr!("Volume Rolloff"));
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
            changed |= rolloff_graph(ui, &mut a, listener);
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
/// With Custom Rolloff it is a curve editor: drag keys, double-click to add one, right-click a
/// key to delete it. Returns whether the curve changed.
fn rolloff_graph(ui: &mut Ui, a: &mut AudioSource, listener: Option<f32>) -> bool {
    let width = ui.available_width().clamp(160.0, 320.0);
    let custom = a.rolloff == AudioRolloff::Custom;
    let sense = if custom {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 110.0), sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Rolloff curve"));
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
    let mut changed = false;
    if custom {
        changed = edit_curve(ui, &resp, a, &x_of, &y_of, plot, span);
    }
    let pts: Vec<egui::Pos2> = (0..=96)
        .map(|i| {
            let d = span * i as f32 / 96.0;
            let v = distance_gain(a, d);
            pos2(x_of(d), y_of(v))
        })
        .collect();
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.5, Color32::from_rgb(220, 70, 60)),
    ));
    if custom {
        for k in &a.custom_rolloff.keys {
            let c = pos2(x_of(k[0] * a.max_distance), y_of(k[1]));
            p.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, theme::TEXT);
        }
    }
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
    if custom {
        ui.label(
            RichText::new(tr!("Drag keys · double-click adds · right-click deletes"))
                .small()
                .color(theme::TEXT_DIM),
        );
    }
    changed
}

/// Index of the curve key drawn within 7 px of `at`.
fn key_near(
    a: &AudioSource,
    at: egui::Pos2,
    x_of: &dyn Fn(f32) -> f32,
    y_of: &dyn Fn(f32) -> f32,
) -> Option<usize> {
    a.custom_rolloff
        .keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            (
                i,
                pos2(x_of(k[0] * a.max_distance), y_of(k[1])).distance(at),
            )
        })
        .filter(|(_, d)| *d < 7.0)
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(i, _)| i)
}

/// Curve-editor interaction on the rolloff graph.
fn edit_curve(
    ui: &Ui,
    resp: &egui::Response,
    a: &mut AudioSource,
    x_of: &dyn Fn(f32) -> f32,
    y_of: &dyn Fn(f32) -> f32,
    plot: Rect,
    span: f32,
) -> bool {
    let id = resp.id.with("dragged_key");
    let Some(at) = resp.interact_pointer_pos().or(resp.hover_pos()) else {
        return false;
    };
    let max = a.max_distance.max(0.01);
    let to_key = |p: egui::Pos2| {
        let d = (p.x - plot.left()) / plot.width() * span;
        let v = (plot.bottom() - p.y) / plot.height();
        [(d / max).clamp(0.0, 1.0), v.clamp(0.0, 1.0)]
    };
    if resp.drag_started() {
        // Where the button went down: the drag only starts after a few pixels of movement.
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(at);
        let k = key_near(a, origin, x_of, y_of);
        ui.data_mut(|d| d.insert_temp(id, k));
        return false;
    }
    if resp.dragged() {
        let dragged: Option<usize> = ui.data(|d| d.get_temp(id)).flatten();
        if let Some(i) = dragged.filter(|i| *i < a.custom_rolloff.keys.len()) {
            a.custom_rolloff.keys[i] = to_key(at);
            // Keep the dragged key under the pointer after sorting.
            let moved = a.custom_rolloff.keys[i];
            a.custom_rolloff.sanitize();
            let now = a.custom_rolloff.keys.iter().position(|k| *k == moved);
            ui.data_mut(|d| d.insert_temp(id, now));
            return true;
        }
        return false;
    }
    if resp.double_clicked() {
        a.custom_rolloff.keys.push(to_key(at));
        a.custom_rolloff.sanitize();
        return true;
    }
    if resp.secondary_clicked()
        && a.custom_rolloff.keys.len() > 2
        && let Some(i) = key_near(a, at, x_of, y_of)
    {
        a.custom_rolloff.keys.remove(i);
        return true;
    }
    false
}
