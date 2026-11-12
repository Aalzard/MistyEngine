//! Weather Inspector: Preview in Edit Mode and a status line, Coverage, the preset dropdown,
//! then collapsible sections of sliders for every value (Rain, Snow, Wind, Ground, Lightning,
//! Sky, Surfaces, Effects), the Timeline and Audio.
//!
//! What you set here is what you see. Two things can stand between the values and the sky:
//! the volume's own Timeline and a script holding the weather (`Weather.SetState`). Both are
//! named in a notice with a button that turns them off, and picking a preset or moving a
//! weather slider turns them off by itself ([`commit`]), so an edit always shows at once, in
//! the Edit Mode preview and in Play.

use std::ops::RangeInclusive;

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::weather::{
    Coverage, TimelineEntry, TimelineMode, Weather, WeatherPreset, WeatherTimeline,
};

use misty_engine::scene::Scene;

use super::editors::{color_rgb, combo, grid};
use crate::i18n::tr;
use crate::state::EditorState;

pub const NAMES: [&str; 1] = ["Weather"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Weather").then_some("🌧")
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state.scene.world.get::<&Weather>(e).ok().map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    if let Ok(mut c) = state.scene.world.get::<&mut Weather>(e) {
        c.enabled = on;
    }
}

fn slider(ui: &mut Ui, label: &str, tip: &str, v: &mut f32, range: RangeInclusive<f32>) -> bool {
    ui.label(tr(label)).on_hover_text(tr(tip));
    let ch = ui
        .add(egui::Slider::new(v, range).clamping(egui::SliderClamping::Never))
        .on_hover_text(tr(tip))
        .changed();
    ui.end_row();
    ch
}

fn toggle(ui: &mut Ui, label: &str, tip: &str, v: &mut bool) -> bool {
    ui.label(tr(label)).on_hover_text(tr(tip));
    let ch = ui.checkbox(v, "").on_hover_text(tr(tip)).changed();
    ui.end_row();
    ch
}

fn text(ui: &mut Ui, label: &str, tip: &str, v: &mut String) -> bool {
    hinted(ui, label, tip, v, "(generated)")
}

/// A clip path field showing `hint` (what plays when it is empty).
fn hinted(ui: &mut Ui, label: &str, tip: &str, v: &mut String, hint: &str) -> bool {
    ui.label(tr(label)).on_hover_text(tr(tip));
    let ch = ui
        .add(egui::TextEdit::singleline(v).hint_text(tr(hint)))
        .on_hover_text(tr(tip))
        .changed();
    ui.end_row();
    ch
}

fn section(ui: &mut Ui, title: &str, open: bool, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), tr(title)))
        .id_salt(("weather_section", title))
        .default_open(open)
        .show(ui, |ui| {
            ch = grid(ui, &format!("weather_{title}"), body);
        });
    ch
}

fn preset_label(p: Option<WeatherPreset>) -> &'static str {
    p.map_or("Custom", WeatherPreset::label)
}

/// A line about the weather running now (Play or the preview).
fn status(ui: &mut Ui, state: &EditorState, e: Entity, w: &Weather) {
    let world = &state.scene.weather;
    if !world.live {
        ui.weak(tr(
            "Not running: turn on Preview in Edit Mode or enter Play.",
        ));
        return;
    }
    let mut line = tr!(
        "Now: {state} · wet {wet} % · snow {snow} %",
        state = tr(world.current.nearest_preset().label()),
        wet = (world.wetness() * 100.0).round(),
        snow = (world.snow_cover() * 100.0).round()
    );
    if w.timeline.is_on()
        && let Some(v) = world.volume(e)
    {
        let entry = w.timeline.entries.get(v.timeline.index);
        let name = entry.map_or("", TimelineEntry::label);
        let phase = if v.timeline.progress < 1.0 {
            tr!(
                "blending to {entry} ({percent} %)",
                entry = tr(name),
                percent = (v.timeline.progress * 100.0).round()
            )
        } else if v.timeline.hold_left.is_finite() {
            tr!(
                "{entry} for {seconds} s",
                entry = tr(name),
                seconds = v.timeline.hold_left.max(0.0).round()
            )
        } else {
            tr(name)
        };
        line.push_str(" · ");
        line.push_str(&phase);
    }
    if world.scripted() {
        line.push_str(" · ");
        line.push_str(&tr("held by a script"));
    }
    ui.weak(line);
}

/// What is driving the weather instead of the volume's own values.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Drivers {
    /// The volume's timeline is on: the entry it shows or blends to ("" while not running).
    pub timeline: Option<String>,
    /// A script holds the weather in this state.
    pub script: Option<WeatherPreset>,
}

/// The timeline and script override standing between volume `e`'s values and the weather.
pub(crate) fn drivers(scene: &Scene, e: Entity, w: &Weather) -> Drivers {
    let world = &scene.weather;
    Drivers {
        timeline: w.timeline.is_on().then(|| {
            world
                .volume(e)
                .filter(|_| world.live)
                .and_then(|v| w.timeline.entries.get(v.timeline.index))
                .map_or(String::new(), |entry| entry.label().to_owned())
        }),
        script: world.script_state(),
    }
}

/// The notice lines for `d`, translated.
pub(crate) fn notice_lines(d: &Drivers) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(entry) = &d.timeline {
        lines.push(if entry.is_empty() {
            tr("Timeline is driving the weather.")
        } else {
            tr!(
                "Timeline is driving the weather (now: {entry}).",
                entry = tr(entry)
            )
        });
    }
    if let Some(p) = d.script {
        lines.push(tr!("Script override: {state}", state = tr(p.label())));
    }
    lines
}

/// Store the edited volume. Changing its weather (a preset or any weather value) is a wish
/// to see it: the timeline goes off (manual; its states are kept) and a script's hold on
/// the weather is cleared, so the new values show at once.
pub(crate) fn commit(scene: &mut Scene, e: Entity, before: &Weather, mut after: Weather) {
    if after.state != before.state {
        if after.timeline.is_on() {
            after.timeline.mode = TimelineMode::Off;
        }
        if scene.weather.scripted() {
            scene.weather.release(0.0);
        }
    }
    let _ = scene.world.insert_one(e, after);
}

/// The notices with their buttons. Returns true when the volume changed.
fn driver_notices(ui: &mut Ui, state: &mut EditorState, e: Entity, w: &mut Weather) -> bool {
    let d = drivers(&state.scene, e, w);
    let lines = notice_lines(&d);
    if lines.is_empty() {
        return false;
    }
    let mut ch = false;
    let warn = ui.visuals().warn_fg_color;
    let mut lines = lines.into_iter();
    if d.timeline.is_some() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(warn, lines.next().unwrap_or_default());
            if ui
                .button(tr("Turn Timeline Off"))
                .on_hover_text(tr(
                    "Show this volume's own values. Its states are kept: turn it on again under Timeline, Mode. Picking a preset or changing a value below does the same.",
                ))
                .clicked()
            {
                w.timeline.mode = TimelineMode::Off;
                ch = true;
            }
        });
    }
    if d.script.is_some() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(warn, lines.next().unwrap_or_default());
            if ui
                .button(tr("Clear Override"))
                .on_hover_text(tr(
                    "A script called Weather.SetState (in WeatherDemo: keys 1 to 8) and holds the weather until Weather.ClearState (key 0). Picking a preset or changing a value below clears it too.",
                ))
                .clicked()
            {
                state.scene.weather.release(0.0);
            }
        });
    }
    ch
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut w) = state.scene.world.get::<&Weather>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let before = w.clone();
    let mut ch = false;
    ch |= ui
        .checkbox(&mut w.preview_in_edit_mode, tr("Preview in Edit Mode"))
        .on_hover_text(tr(
            "Show the weather in the Editor Scene and the Scene Viewer without entering Play.",
        ))
        .changed();
    status(ui, state, e, &w);
    ch |= driver_notices(ui, state, e, &mut w);
    ch |= grid(ui, "weather_top", |ui| {
        let mut ch = false;
        ui.label(tr("Coverage")).on_hover_text(tr(
            "Global: everywhere, the rain follows the camera. Area: only inside a box or sphere, fading at its edges.",
        ));
        ch |= combo(
            ui,
            "weather_cov",
            &mut w.coverage,
            &Coverage::ALL,
            Coverage::label,
        );
        ui.end_row();
        match w.coverage {
            Coverage::Global => {}
            Coverage::Box => {
                ui.label(tr("Size"))
                    .on_hover_text(tr("Box size in metres (× the object's scale)."));
                ui.horizontal(|ui| {
                    for (axis, v) in ["X", "Y", "Z"].into_iter().zip(w.size.iter_mut()) {
                        ui.label(axis);
                        ch |= ui
                            .add(egui::DragValue::new(v).speed(0.5).range(0.0..=100_000.0))
                            .changed();
                    }
                });
                ui.end_row();
            }
            Coverage::Sphere => {
                ch |= slider(
                    ui,
                    "Radius",
                    "Sphere radius in metres (× the object's largest scale).",
                    &mut w.radius,
                    0.0..=2000.0,
                );
            }
        }
        if w.coverage != Coverage::Global {
            ch |= slider(
                ui,
                "Blend Distance",
                "Metres over which the weather fades out at the edge.",
                &mut w.blend_distance,
                0.0..=200.0,
            );
        }
        ch |= slider(
            ui,
            "Priority",
            "Higher priority volumes blend over lower ones.",
            &mut w.priority,
            -10.0..=10.0,
        );
        ch |= slider(
            ui,
            "Weight",
            "How much of this volume's weather applies.",
            &mut w.weight,
            0.0..=1.0,
        );
        ui.label(tr("Preset")).on_hover_text(tr(
            "Fills every value below with a weather state (wind direction and lightning distances are kept).",
        ));
        let mut current = WeatherPreset::matching(&w.state);
        let before = current;
        egui::ComboBox::from_id_salt("weather_preset")
            .selected_text(tr(preset_label(current)))
            .show_ui(ui, |ui| {
                for p in WeatherPreset::ALL {
                    ui.selectable_value(&mut current, Some(p), tr(p.label()));
                }
            });
        if current != before
            && let Some(p) = current
        {
            p.apply(&mut w.state);
            ch = true;
        }
        ui.end_row();
        ch
    });
    let s = &mut w.state;
    ch |= section(ui, "Rain", true, |ui| {
        let mut ch = false;
        ch |= slider(
            ui,
            "Intensity",
            "How hard it rains: wets surfaces, splashes, sound. 0 = no rain.",
            &mut s.rain_intensity,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Drop Density",
            "Drops per cubic metre near the camera.",
            &mut s.rain_density,
            0.0..=100.0,
        );
        ch |= slider(
            ui,
            "Drop Size (mm)",
            "Drop diameter: 0.5 for drizzle up to 3 for a downpour.",
            &mut s.drop_size,
            0.2..=6.0,
        );
        ch |= slider(
            ui,
            "Fall Speed (m/s)",
            "Real drops fall at 2 to 9 m/s depending on their size.",
            &mut s.rain_speed,
            0.5..=20.0,
        );
        ch |= slider(
            ui,
            "Streak Length",
            "Motion blur length (1 = a film camera's shutter).",
            &mut s.streak_length,
            0.0..=4.0,
        );
        ch |= slider(
            ui,
            "Opacity",
            "How visible the drops are.",
            &mut s.rain_opacity,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Refraction",
            "How much the drops bend the scene behind them.",
            &mut s.refraction,
            0.0..=1.0,
        );
        ch
    });
    ch |= section(ui, "Snow", false, |ui| {
        let mut ch = false;
        ch |= slider(
            ui,
            "Intensity",
            "How hard it snows: builds snow cover. 0 = no snow.",
            &mut s.snow_intensity,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Flake Density",
            "Flakes per cubic metre near the camera.",
            &mut s.snow_density,
            0.0..=100.0,
        );
        ch |= slider(
            ui,
            "Flake Size (cm)",
            "Average flake size; a few flakes are much bigger.",
            &mut s.flake_size,
            0.1..=5.0,
        );
        ch |= slider(
            ui,
            "Fall Speed (m/s)",
            "Snow falls at about 1 m/s.",
            &mut s.snow_speed,
            0.1..=5.0,
        );
        ch |= slider(
            ui,
            "Swirl",
            "How much the flakes swirl and tumble on their way down.",
            &mut s.snow_turbulence,
            0.0..=3.0,
        );
        ch |= slider(
            ui,
            "Opacity",
            "How visible the flakes are.",
            &mut s.snow_opacity,
            0.0..=1.0,
        );
        ch
    });
    ch |= section(ui, "Wind", false, |ui| {
        let mut ch = false;
        ch |= slider(
            ui,
            "Direction (°)",
            "Where the wind blows towards, clockwise from +Z (north).",
            &mut s.wind_direction,
            0.0..=360.0,
        );
        ch |= slider(
            ui,
            "Speed (m/s)",
            "Slants the rain and drifts the snow.",
            &mut s.wind_speed,
            0.0..=40.0,
        );
        ch |= slider(
            ui,
            "Turbulence",
            "Fast random changes of the wind's direction and strength.",
            &mut s.wind_turbulence,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Gusts",
            "Strength of the gusts that change the slant over time.",
            &mut s.gusts,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Gusts per Minute",
            "How often gusts come.",
            &mut s.gust_frequency,
            0.0..=30.0,
        );
        ch |= toggle(
            ui,
            "Use Wind Zone",
            "Take the direction (and × strength) from the scene's Wind Zone.",
            &mut w.effects.use_wind_zone,
        );
        ch
    });
    ch |= section(ui, "Ground", false, |ui| {
        let mut ch = false;
        ch |= slider(
            ui,
            "Splashes",
            "Splash rate where drops hit the ground (0 = none).",
            &mut s.splashes,
            0.0..=3.0,
        );
        ch |= slider(
            ui,
            "Mist",
            "Spray over the ground where heavy rain hits.",
            &mut s.mist,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Visibility (m)",
            "How far you can see through the rain or snow.",
            &mut s.visibility,
            30.0..=20_000.0,
        );
        ch
    });
    let mut strike = false;
    ch |= section(ui, "Lightning", false, |ui| {
        let mut ch = false;
        ch |= slider(
            ui,
            "Strikes per Minute",
            "0 = no lightning.",
            &mut s.lightning_frequency,
            0.0..=30.0,
        );
        ch |= slider(
            ui,
            "Nearest (m)",
            "Closest strike distance. Thunder takes distance / 343 m/s to arrive.",
            &mut s.lightning_min_distance,
            50.0..=10_000.0,
        );
        ch |= slider(
            ui,
            "Farthest (m)",
            "Farthest strike distance; far strikes only light the clouds.",
            &mut s.lightning_max_distance,
            100.0..=30_000.0,
        );
        ch |= slider(
            ui,
            "Brightness",
            "× the flash brightness.",
            &mut s.lightning_intensity,
            0.0..=4.0,
        );
        ui.label("");
        strike = ui
            .button(tr("Strike Now"))
            .on_hover_text(tr("A strike 1.5 km away (while the weather runs)."))
            .clicked();
        ui.end_row();
        ch
    });
    ch |= section(ui, "Sky", false, |ui| {
        let mut ch = false;
        ch |= toggle(
            ui,
            "Drive Clouds",
            "Move the volumetric clouds towards this cover (turning them on when overcast) and dim the sun and sky.",
            &mut w.effects.drive_clouds,
        );
        ch |= slider(
            ui,
            "Cloud Cover",
            "0 = clear sky, 1 = overcast.",
            &mut s.cloud_coverage,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Cloud Density",
            "Thicker, darker storm clouds.",
            &mut s.cloud_density,
            0.0..=2.0,
        );
        ch |= slider(
            ui,
            "Sun Dimming",
            "Share of the direct sun or moon light the clouds take away.",
            &mut s.sun_dimming,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Sky Dimming",
            "Share of the sky's ambient light the clouds take away.",
            &mut s.ambient_dimming,
            0.0..=1.0,
        );
        ch
    });
    let f = &mut w.surfaces;
    ch |= section(ui, "Surfaces", false, |ui| {
        let mut ch = false;
        ch |= toggle(
            ui,
            "Wetness",
            "Surfaces get darker and glossier while it rains, and dry afterwards.",
            &mut f.wetness,
        );
        ch |= slider(
            ui,
            "Wet In (s)",
            "Seconds of hard rain until everything is wet.",
            &mut f.wet_time,
            1.0..=600.0,
        );
        ch |= slider(
            ui,
            "Dry In (s)",
            "Seconds until wet surfaces have dried.",
            &mut f.dry_time,
            1.0..=1800.0,
        );
        ch |= slider(
            ui,
            "Puddles",
            "How much of the flat open ground puddles cover.",
            &mut f.puddles,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Puddle Size (m)",
            "Size of the puddle pattern.",
            &mut f.puddle_scale,
            0.3..=20.0,
        );
        ch |= toggle(
            ui,
            "Ripples",
            "Raindrop rings on puddles while it rains.",
            &mut f.ripples,
        );
        ch |= toggle(
            ui,
            "Snow Cover",
            "Snow builds up on upward faces while it snows, and melts afterwards.",
            &mut f.snow_cover,
        );
        ch |= slider(
            ui,
            "Snow Builds In (s)",
            "Seconds of heavy snow until the ground is white.",
            &mut f.snow_time,
            1.0..=1800.0,
        );
        ch |= slider(
            ui,
            "Melts In (s)",
            "Seconds for full cover to melt (rain melts it faster).",
            &mut f.melt_time,
            1.0..=3600.0,
        );
        ch |= slider(
            ui,
            "Max Snow",
            "The most snow cover allowed.",
            &mut f.max_snow,
            0.0..=1.0,
        );
        ch |= toggle(
            ui,
            "Snow Line",
            "Snow only settles above a height.",
            &mut f.snow_line,
        );
        if f.snow_line {
            ch |= slider(
                ui,
                "Snow Line Height (m)",
                "Snow settles above this height.",
                &mut f.snow_line_height,
                -500.0..=5000.0,
            );
        }
        ui.label(tr("Snow Colour"));
        ch |= color_rgb(ui, &mut f.snow_color);
        ui.end_row();
        ch
    });
    let x = &mut w.effects;
    ch |= section(ui, "Effects", false, |ui| {
        let mut ch = false;
        ch |= toggle(
            ui,
            "Splashes",
            "Crown splashes where drops hit, from the depth buffer (not under roofs).",
            &mut x.splashes,
        );
        ch |= toggle(ui, "Mist", "Ground spray in heavy rain.", &mut x.mist);
        ch |= toggle(
            ui,
            "Far Sheets",
            "Soft distant sheets so the density reads far away.",
            &mut x.far_sheets,
        );
        ch |= toggle(
            ui,
            "Haze",
            "Lower visibility (also thickens the volumetric fog).",
            &mut x.haze,
        );
        ch |= toggle(
            ui,
            "Sheltered Areas",
            "No rain or snow under roofs, trees and other shadow casters (a depth map from above).",
            &mut x.occlusion,
        );
        ch |= toggle(
            ui,
            "Lightning Bolts",
            "Draw bolts in the sky (flashes light the scene either way).",
            &mut x.bolts,
        );
        ch |= toggle(
            ui,
            "Lens Drops",
            "Drops on the camera lens while rain falls on it.",
            &mut x.lens_drops,
        );
        ch
    });
    ch |= timeline(ui, &mut w.timeline);
    let a = &mut w.audio;
    ch |= section(ui, "Audio", false, |ui| {
        let mut ch = false;
        ch |= toggle(
            ui,
            "Sound",
            "Rain that follows the intensity (steady rain crossfading into a storm), wind, and thunder delayed by distance.",
            &mut a.enabled,
        );
        ch |= slider(
            ui,
            "Volume",
            "Volume of every weather sound.",
            &mut a.volume,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Under Cover",
            "Share of the rain and wind left under a roof (colliders above the Audio Listener; needs Sheltered Areas).",
            &mut a.under_cover,
            0.0..=1.0,
        );
        ch |= hinted(
            ui,
            "Rain Loop",
            "Loop for light and steady rain (project path); empty = the built-in steady rain recording.",
            &mut a.rain_clip,
            "(built-in steady rain)",
        );
        ch |= hinted(
            ui,
            "Storm Loop",
            "Loop the rain crossfades into as it gets heavy; empty = the built-in storm recording.",
            &mut a.storm_clip,
            "(built-in storm)",
        );
        ch |= text(
            ui,
            "Wind Clip",
            "Your own wind loop; empty = generated.",
            &mut a.wind_clip,
        );
        // One thunder clip here (scripts and files may list several, picked at random).
        let mut thunder = a.thunder_clips.first().cloned().unwrap_or_default();
        if text(
            ui,
            "Thunder Clip",
            "Plays after each lightning flash, later the farther the strike; empty = generated thunder.",
            &mut thunder,
        ) {
            ch = true;
            if thunder.trim().is_empty() {
                if !a.thunder_clips.is_empty() {
                    a.thunder_clips.remove(0);
                }
            } else if let Some(first) = a.thunder_clips.first_mut() {
                *first = thunder;
            } else {
                a.thunder_clips.push(thunder);
            }
        }
        ch |= text(
            ui,
            "Output",
            "Audio Mixer group, e.g. Assets/Audio/Main.mixer#SFX (empty = straight out).",
            &mut a.output,
        );
        ui.label(tr("Seed"))
            .on_hover_text(tr("Same seed, same storm."));
        ch |= ui.add(egui::DragValue::new(&mut w.seed)).changed();
        ui.end_row();
        ch
    });
    if strike {
        let cam = state.scene.weather.camera;
        state.scene.weather.strike(
            cam + glam::Vec3::new(900.0, 0.0, 1200.0),
            w.state.lightning_intensity,
        );
    }
    if ch {
        commit(&mut state.scene, e, &before, w);
    }
    ch
}

fn timeline(ui: &mut Ui, t: &mut WeatherTimeline) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), tr("Timeline")))
        .id_salt("weather_timeline")
        .default_open(t.is_on())
        .show(ui, |ui| {
            ch |= grid(ui, "weather_tl", |ui| {
                let mut ch = false;
                ui.label(tr("Mode")).on_hover_text(tr(
                    "Sequence: the states in order. Random: weighted random states, each held between its min and max.",
                ));
                ch |= combo(ui, "weather_tl_mode", &mut t.mode, &TimelineMode::ALL, TimelineMode::label);
                ui.end_row();
                if t.mode == TimelineMode::Sequence {
                    ch |= toggle(ui, "Loop", "Start again after the last state.", &mut t.looping);
                }
                ch |= slider(ui, "Time Scale", "× how fast the timeline runs (60 = a minute per second).", &mut t.time_scale, 0.1..=120.0);
                ch
            });
            let random = t.mode == TimelineMode::Random;
            let mut remove = None;
            let mut swap = None;
            let count = t.entries.len();
            for (i, entry) in t.entries.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", i + 1));
                    let mut p = entry.preset;
                    if combo(ui, &format!("weather_tl_{i}"), &mut p, &WeatherPreset::ALL, WeatherPreset::label) {
                        entry.preset = p;
                        entry.custom = None;
                        ch = true;
                    }
                    ui.label(tr("Hold"));
                    ch |= ui
                        .add(egui::DragValue::new(&mut entry.duration).speed(1.0).range(0.0..=86_400.0).suffix(" s"))
                        .on_hover_text(tr(if random { "Shortest hold (seconds)." } else { "Seconds this state holds." }))
                        .changed();
                    if random {
                        ui.label(tr("to"));
                        ch |= ui
                            .add(egui::DragValue::new(&mut entry.max_duration).speed(1.0).range(0.0..=86_400.0).suffix(" s"))
                            .on_hover_text(tr("Longest hold (seconds)."))
                            .changed();
                    }
                    ui.label(tr("Blend"));
                    ch |= ui
                        .add(egui::DragValue::new(&mut entry.transition).speed(0.5).range(0.0..=3600.0).suffix(" s"))
                        .on_hover_text(tr("Seconds to blend into this state."))
                        .changed();
                    if random {
                        ui.label(tr("Weight"));
                        ch |= ui
                            .add(egui::DragValue::new(&mut entry.weight).speed(0.05).range(0.0..=100.0))
                            .on_hover_text(tr("How likely this state is picked next."))
                            .changed();
                    }
                    if i > 0 && ui.small_button("⏶").on_hover_text(tr("Move up")).clicked() {
                        swap = Some(i);
                    }
                    if i + 1 < count && ui.small_button("⏷").on_hover_text(tr("Move down")).clicked() {
                        swap = Some(i + 1);
                    }
                    if ui.small_button("✕").on_hover_text(tr("Remove")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = swap {
                t.entries.swap(i - 1, i);
                ch = true;
            }
            if let Some(i) = remove {
                t.entries.remove(i);
                ch = true;
            }
            ui.horizontal(|ui| {
                if ui.button(tr("Add State")).clicked() {
                    let last = t.entries.last().map_or(WeatherPreset::Clear, |e| e.preset);
                    let next = WeatherPreset::ALL
                        [(WeatherPreset::ALL.iter().position(|p| *p == last).unwrap_or(0) + 1) % WeatherPreset::ALL.len()];
                    t.entries.push(TimelineEntry::new(next, 60.0, 15.0));
                    if t.mode == TimelineMode::Off {
                        t.mode = TimelineMode::Sequence;
                        t.looping = true;
                    }
                    ch = true;
                }
                if ui
                    .button(tr("Demo Cycle"))
                    .on_hover_text(tr("Clear → Light Rain → Thunderstorm → Snow → Clear, looping."))
                    .clicked()
                {
                    *t = WeatherTimeline::demo_cycle();
                    ch = true;
                }
            });
        });
    ch
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use misty_engine::weather::update_scene;

    use super::*;

    const EYE: Vec3 = Vec3::new(0.0, 1.7, 0.0);

    /// A scene with one global Weather volume, running (as in Play or the Edit Mode preview).
    fn scene_with(w: Weather) -> (Scene, Entity) {
        let mut s = Scene::default_3d("WeatherInspector");
        let e = s.spawn("Weather", None);
        s.add(e, w);
        update_scene(&mut s, 0.1, EYE);
        (s, e)
    }

    fn volume(s: &Scene, e: Entity) -> Weather {
        (*s.world.get::<&Weather>(e).unwrap()).clone()
    }

    /// What the Properties panel does when the user picks `preset` in the dropdown.
    fn pick(s: &mut Scene, e: Entity, preset: WeatherPreset) {
        let before = volume(s, e);
        let mut after = before.clone();
        preset.apply(&mut after.state);
        commit(s, e, &before, after);
    }

    fn run(s: &mut Scene, seconds: f32) {
        for _ in 0..(seconds * 30.0) as usize {
            update_scene(s, 1.0 / 30.0, EYE);
        }
    }

    #[test]
    fn edits_reach_the_running_weather_without_a_timeline() {
        let (mut s, e) = scene_with(Weather::with_preset(WeatherPreset::Rain));
        assert_eq!(s.weather.current.nearest_preset(), WeatherPreset::Rain);
        assert_eq!(drivers(&s, e, &volume(&s, e)), Drivers::default());
        // A preset: on the very next frame.
        pick(&mut s, e, WeatherPreset::Blizzard);
        update_scene(&mut s, 1.0 / 30.0, EYE);
        assert_eq!(s.weather.current.nearest_preset(), WeatherPreset::Blizzard);
        // A slider: the preset reads Custom and the value is live.
        let before = volume(&s, e);
        let mut after = before.clone();
        after.state.snow_density = 3.0;
        commit(&mut s, e, &before, after);
        update_scene(&mut s, 1.0 / 30.0, EYE);
        assert_eq!(s.weather.current.snow_density, 3.0);
        assert_eq!(WeatherPreset::matching(&volume(&s, e).state), None);
    }

    #[test]
    fn editing_a_value_takes_over_from_the_timeline() {
        let mut w = Weather::with_preset(WeatherPreset::LightRain);
        w.timeline = WeatherTimeline::demo_cycle();
        let (mut s, e) = scene_with(w);
        // The timeline drives: its first state (Clear), not the volume's Light Rain.
        assert_eq!(s.weather.current.rain_density, 0.0);
        let d = drivers(&s, e, &volume(&s, e));
        assert_eq!(d.timeline.as_deref(), Some("Clear"));
        let lines = notice_lines(&d);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("Timeline") && lines[0].contains("Clear"),
            "{lines:?}"
        );
        // Picking a preset: the timeline goes off (its states kept) and the preset shows.
        pick(&mut s, e, WeatherPreset::HeavySnow);
        update_scene(&mut s, 1.0 / 30.0, EYE);
        assert_eq!(s.weather.current.nearest_preset(), WeatherPreset::HeavySnow);
        let w = volume(&s, e);
        assert_eq!(w.timeline.mode, TimelineMode::Off);
        assert_eq!(w.timeline.entries.len(), 4);
        assert!(notice_lines(&drivers(&s, e, &w)).is_empty());
        // And it stays: the timeline no longer moves it.
        run(&mut s, 60.0);
        assert_eq!(s.weather.current.nearest_preset(), WeatherPreset::HeavySnow);
        // A slider does the same.
        let mut w2 = Weather::with_preset(WeatherPreset::Rain);
        w2.timeline = WeatherTimeline::demo_cycle();
        let (mut s2, e2) = scene_with(w2);
        let before = volume(&s2, e2);
        let mut after = before.clone();
        after.state.rain_density = 77.0;
        commit(&mut s2, e2, &before, after);
        update_scene(&mut s2, 1.0 / 30.0, EYE);
        assert_eq!(s2.weather.current.rain_density, 77.0);
        // An edit of something else (surfaces, effects) leaves a running timeline alone.
        let mut w3 = Weather::with_preset(WeatherPreset::Rain);
        w3.timeline = WeatherTimeline::demo_cycle();
        let (mut s3, e3) = scene_with(w3);
        let before = volume(&s3, e3);
        let mut after = before.clone();
        after.surfaces.puddles = 0.9;
        commit(&mut s3, e3, &before, after);
        assert!(volume(&s3, e3).timeline.is_on());
    }

    /// The user's report: a Blizzard set by the demo's key 7 (a script), then Clear picked
    /// in Properties, and the weather stayed a blizzard.
    #[test]
    fn clear_picked_after_a_script_blizzard_becomes_clear() {
        let mut w = Weather::with_preset(WeatherPreset::LightRain);
        w.timeline = WeatherTimeline::demo_cycle();
        let (mut s, e) = scene_with(w);
        // Weather.SetState("Blizzard", 4), as WeatherDemo.lua does.
        s.weather
            .set_state(WeatherPreset::Blizzard.params(), 4.0, "Blizzard");
        run(&mut s, 5.0);
        assert_eq!(s.weather.current.nearest_preset(), WeatherPreset::Blizzard);
        // The override is named in the panel.
        let d = drivers(&s, e, &volume(&s, e));
        assert_eq!(d.script, Some(WeatherPreset::Blizzard));
        let lines = notice_lines(&d);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Script override") && l.contains("Blizzard")),
            "{lines:?}"
        );
        // Clear picked in Properties: clear weather on the next frame, and it stays clear.
        pick(&mut s, e, WeatherPreset::Clear);
        update_scene(&mut s, 1.0 / 30.0, EYE);
        assert_eq!(s.weather.current.nearest_preset(), WeatherPreset::Clear);
        assert_eq!(s.weather.current.snow_density, 0.0);
        assert!(!s.weather.scripted());
        run(&mut s, 30.0);
        assert_eq!(s.weather.current.snow_density, 0.0);
        assert!(notice_lines(&drivers(&s, e, &volume(&s, e))).is_empty());
    }

    #[test]
    fn presets_apply_after_a_script_override_is_cleared() {
        let (mut s, e) = scene_with(Weather::with_preset(WeatherPreset::Rain));
        s.weather
            .set_state(WeatherPreset::Thunderstorm.params(), 0.0, "Thunderstorm");
        run(&mut s, 1.0);
        assert_eq!(
            s.weather.current.nearest_preset(),
            WeatherPreset::Thunderstorm
        );
        // The Clear Override button.
        s.weather.release(0.0);
        update_scene(&mut s, 1.0 / 30.0, EYE);
        assert_eq!(s.weather.current.nearest_preset(), WeatherPreset::Rain);
        assert_eq!(s.weather.script_state(), None);
        for p in [
            WeatherPreset::Snow,
            WeatherPreset::HeavySnow,
            WeatherPreset::Flurries,
        ] {
            pick(&mut s, e, p);
            update_scene(&mut s, 1.0 / 30.0, EYE);
            assert_eq!(s.weather.current.nearest_preset(), p);
        }
    }
}
