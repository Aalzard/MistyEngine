//! Weather sounds: a steady rain loop crossfading into a heavy storm loop as the rain
//! intensity rises, a wind loop for strong wind and blizzards, and thunder that arrives
//! `distance / 343 m/s` after each flash, all through the audio mixer (the volume's Output
//! group). The rain is quieter under cover: rays cast up from the Audio Listener find roofs
//! and other colliders overhead. Every level eases towards its target, so the sounds fade in
//! and out with the timeline's transitions and as you walk under a roof.
//!
//! Without clips of their own (Weather > Audio > Rain Loop / Storm Loop / Wind / Thunder),
//! the rain plays the bundled recordings (`SampleProject/Assets/Audio/Weather`, built into the
//! engine) and wind and thunder are generated once (filtered noise, cracks and rumbles); both
//! are written into `Library/Weather/` of the project (or the temp folder) and played from
//! there. To use your own recordings, drop WAV/OGG files into the project and pick them in
//! those fields.

use std::path::{Path, PathBuf};

use super::lightning::Rng;
use super::state::{WeatherEvent, WeatherWorld};
use crate::audio::{Audio, VoiceId};

/// The bundled rain recordings (Ogg Vorbis, 48 kHz stereo, seamless 30 s loops at -24 LUFS;
/// prepared by `tools/sample_sounds.py`).
pub(crate) const RAIN_STEADY_OGG: &[u8] =
    include_bytes!("../../../../SampleProject/Assets/Audio/Weather/rain_steady_loop.ogg");
pub(crate) const RAIN_STORM_OGG: &[u8] =
    include_bytes!("../../../../SampleProject/Assets/Audio/Weather/storm_heavy_rain_loop.ogg");

/// Sample rate of the generated clips.
pub const RATE: u32 = 32_000;

/// Generated clip files (absolute paths).
#[derive(Clone, Debug)]
pub struct Clips {
    /// The bundled steady rain and storm recordings.
    pub rain_steady: PathBuf,
    pub rain_storm: PathBuf,
    pub wind: PathBuf,
    pub thunder_near: [PathBuf; 2],
    pub thunder_far: [PathBuf; 2],
}

/// One-pole low-pass filter.
#[derive(Clone, Copy, Default)]
struct LowPass {
    y: f32,
}

impl LowPass {
    fn run(&mut self, x: f32, a: f32) -> f32 {
        self.y += a * (x - self.y);
        self.y
    }
}

/// Coefficient of a one-pole filter with cutoff `hz`.
fn coef(hz: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * hz / RATE as f32).exp()
}

/// Make the end flow into the start: crossfade the last `fade` seconds over the first.
fn make_loop(mut v: Vec<(f32, f32)>, fade: f32) -> Vec<(f32, f32)> {
    let n = ((fade * RATE as f32) as usize).min(v.len() / 3);
    let len = v.len() - n;
    for i in 0..n {
        let t = i as f32 / n as f32;
        let tail = v[len + i];
        let head = v[i];
        // Equal-power crossfade.
        let (a, b) = (
            (t * std::f32::consts::FRAC_PI_2).sin(),
            (t * std::f32::consts::FRAC_PI_2).cos(),
        );
        v[i] = (head.0 * a + tail.0 * b, head.1 * a + tail.1 * b);
    }
    v.truncate(len);
    v
}

fn normalize(v: &mut [(f32, f32)], peak: f32) {
    let m = v
        .iter()
        .fold(0.0f32, |m, (l, r)| m.max(l.abs()).max(r.abs()))
        .max(1e-6);
    for s in v.iter_mut() {
        s.0 *= peak / m;
        s.1 *= peak / m;
    }
}

/// A wind loop: low rumbling noise with slow gusting whooshes.
pub fn wind(seconds: f32, seed: u64) -> Vec<(f32, f32)> {
    let mut rng = Rng::new(seed);
    let n = (seconds * RATE as f32) as usize;
    let mut out = Vec::with_capacity(n);
    let mut lp = [LowPass::default(); 2];
    let mut lp2 = [LowPass::default(); 2];
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let gust = 0.55 + 0.45 * (t * 0.5).sin() * (t * 0.23 + 2.0).sin();
        // The whistle's pitch rides the gust.
        let cut = 300.0 + 900.0 * gust;
        let mut ch = [0.0f32; 2];
        for (c, v) in ch.iter_mut().enumerate() {
            let w = rng.range(-1.0, 1.0);
            let a = lp[c].run(w, coef(cut));
            *v = lp2[c].run(a, coef(cut * 1.5)) * gust;
        }
        out.push((ch[0], ch[1]));
    }
    let mut v = make_loop(out, 1.0);
    normalize(&mut v, 0.7);
    v
}

/// Thunder: `near` = a sharp crack followed by a rolling rumble; else a distant low rumble
/// (the air takes the high frequencies away).
pub fn thunder(near: bool, seed: u64) -> Vec<(f32, f32)> {
    let mut rng = Rng::new(seed);
    let seconds = if near { 6.5 } else { 7.5 };
    let n = (seconds * RATE as f32) as usize;
    let mut out = Vec::with_capacity(n);
    let mut lp = [LowPass::default(); 2];
    let mut lp2 = [LowPass::default(); 2];
    let mut crack = [LowPass::default(); 2];
    // Rumble bumps: echoes off the ground and the cloud channel.
    let bumps: Vec<(f32, f32, f32)> = (0..10)
        .map(|_| {
            (
                rng.range(0.2, seconds * 0.7),
                rng.range(0.3, 1.0),
                rng.range(0.4, 1.6),
            )
        })
        .collect();
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let mut env = 0.0f32;
        for &(at, amp, width) in &bumps {
            let d = (t - at) / width;
            if d > -1.0 {
                env += amp
                    * if d < 0.0 {
                        (d + 1.0).powi(2)
                    } else {
                        (-d * 2.0).exp()
                    };
            }
        }
        let attack = if near {
            (t / 0.01).min(1.0)
        } else {
            (t / 0.6).min(1.0)
        };
        let fade = (1.0 - t / seconds).max(0.0).powf(1.5);
        let mut ch = [0.0f32; 2];
        for (c, v) in ch.iter_mut().enumerate() {
            let w = rng.range(-1.0, 1.0);
            let cut = if near { 260.0 } else { 120.0 };
            let low = lp2[c].run(lp[c].run(w, coef(cut)), coef(cut));
            let mut s = low * 6.0 * env * attack * fade;
            if near && t < 0.35 {
                // The crack: bright broadband noise right at the start.
                let k = (-t * 14.0).exp();
                s += (w - crack[c].run(w, coef(1500.0))) * k * 1.4 + low * 10.0 * k;
            }
            *v = s;
        }
        out.push((ch[0], ch[1]));
    }
    normalize(&mut out, 0.85);
    out
}

/// Folder the generated clips go into: the project's `Library/Weather` when it can be written,
/// else the temp folder.
fn clip_dir() -> PathBuf {
    let lib = crate::assets::absolute("Library/Weather");
    if lib.is_absolute() && std::fs::create_dir_all(&lib).is_ok() {
        return lib;
    }
    let tmp = std::env::temp_dir().join("misty_weather_audio");
    let _ = std::fs::create_dir_all(&tmp);
    tmp
}

fn write(dir: &Path, name: &str, make: impl FnOnce() -> Vec<(f32, f32)>) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        let bytes = crate::audio::wav::pcm16_stereo(&make(), RATE);
        let _ = std::fs::write(&path, bytes);
    }
    path
}

/// A bundled clip, decoded as it plays (Compressed In Memory: a 30 s loop decoded whole would
/// take 11 MB).
fn write_bundled(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    // (Re)written when missing or cut short.
    if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
        let _ = std::fs::write(&path, bytes);
    }
    let import = crate::audio::clip::AudioImporter {
        load_type: crate::audio::clip::LoadType::CompressedInMemory,
        force_to_mono: false,
    };
    if crate::audio::clip::AudioImporter::load(&path) != import {
        let _ = import.save(&path);
    }
    path
}

/// The bundled and generated clips, written on first use (version in the names: changes
/// write new files).
pub fn ensure_clips() -> Clips {
    let dir = clip_dir();
    Clips {
        rain_steady: write_bundled(&dir, "rain_steady_v2.ogg", RAIN_STEADY_OGG),
        rain_storm: write_bundled(&dir, "rain_storm_v2.ogg", RAIN_STORM_OGG),
        wind: write(&dir, "wind_v1.wav", || wind(10.0, 13)),
        thunder_near: [
            write(&dir, "thunder_near_a_v1.wav", || thunder(true, 21)),
            write(&dir, "thunder_near_b_v1.wav", || thunder(true, 22)),
        ],
        thunder_far: [
            write(&dir, "thunder_far_a_v1.wav", || thunder(false, 31)),
            write(&dir, "thunder_far_b_v1.wav", || thunder(false, 32)),
        ],
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Volume left at `cover` (0 = open sky .. 1 = under a roof) when full cover leaves
/// `under_cover` of it.
pub fn cover_gain(cover: f32, under_cover: f32) -> f32 {
    1.0 - cover.clamp(0.0, 1.0) * (1.0 - under_cover.clamp(0.0, 1.0))
}

/// The rain loops' volumes (steady, storm), 0..1 before the volume's Volume, for rain
/// `intensity` 0..1: silent when dry, louder as the rain gets heavier, the steady loop giving
/// way to the storm loop (an equal-power crossfade) from intensity ~0.45 to ~0.9, both scaled
/// by [`cover_gain`].
pub fn rain_volumes(intensity: f32, cover: f32, under_cover: f32) -> (f32, f32) {
    let r = if intensity.is_finite() {
        intensity.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let level = smoothstep(0.0, 0.3, r) * (0.6 + 0.4 * r) * cover_gain(cover, under_cover);
    let storm = smoothstep(0.45, 0.9, r) * std::f32::consts::FRAC_PI_2;
    (level * storm.cos(), level * storm.sin())
}

/// Loop volumes for the weather now: (steady rain, storm rain, wind), each 0..1 before the
/// volume's Volume, with `cover` overhead (0..1).
pub fn loop_volumes(world: &WeatherWorld, cover: f32) -> (f32, f32, f32) {
    let p = &world.current;
    let r = if p.rain_density > 0.0 {
        p.rain_intensity
    } else {
        0.0
    };
    let under = world.audio.under_cover;
    let (steady, storm) = rain_volumes(r, cover, under);
    let wind = smoothstep(4.0, 18.0, world.wind.length()) * 0.9
        + 0.1 * smoothstep(0.0, 1.0, p.snow_intensity) * smoothstep(1.0, 6.0, p.wind_speed);
    // A roof keeps the wind off less than the rain.
    let wind = wind.min(1.0) * cover_gain(cover, under.clamp(0.0, 1.0).sqrt());
    (steady, storm, wind)
}

/// Rays cast up from the ear (straight up, and eight 35° from it): the share of them that hit
/// a collider within this many metres is how covered the spot is.
const COVER_RAY_LENGTH: f32 = 40.0;

/// 0 (open sky) .. 1 (a roof all round overhead) at `ear`: rays cast up through the physics
/// world, skipping the Player layer and Ignore Raycast (and colliders a ray starts in, like the
/// player's own capsule).
pub fn cover(physics: &crate::physics::Physics, ear: glam::Vec3) -> f32 {
    use crate::physics::layers;
    let mut mask = layers::DEFAULT_RAYCAST_LAYERS;
    let player = layers::name_to_layer("Player");
    if (0..32).contains(&player) {
        mask &= !(1u32 << player);
    }
    let (s, c) = 35f32.to_radians().sin_cos();
    let mut dirs = vec![glam::Vec3::Y];
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        dirs.push(glam::Vec3::new(s * a.cos(), c, s * a.sin()));
    }
    let hits = dirs
        .iter()
        .filter(|d| physics.raycast(ear, **d, COVER_RAY_LENGTH, mask).is_some())
        .count();
    hits as f32 / dirs.len() as f32
}

/// Seconds the loop levels and the cover take to ease most of the way to a new value.
const LEVEL_EASE: f32 = 0.6;
const COVER_EASE: f32 = 0.35;

/// Plays the weather's sounds in Play mode.
#[derive(Default)]
pub struct Driver {
    clips: Option<Clips>,
    steady: Option<VoiceId>,
    storm: Option<VoiceId>,
    wind: Option<VoiceId>,
    /// Eased levels (steady rain, storm rain, wind) and cover.
    levels: [f32; 3],
    cover: f32,
    rng: Rng,
    /// Thunder voices started so far (tests).
    pub thunder_played: u32,
}

impl Driver {
    /// Start, adjust and stop the loops; play thunder that has arrived. `cover` is this
    /// frame's [`cover`] at the listener (0 when not measured), `dt` the frame's seconds.
    pub fn update(&mut self, audio: &mut Audio, world: &mut WeatherWorld, dt: f32, cover: f32) {
        let events = world.take_audio_events();
        let settings = world.audio.clone();
        if !world.live || !settings.enabled {
            self.stop(audio);
            return;
        }
        let ease = |tau: f32| 1.0 - (-dt.max(0.0) / tau).exp();
        let cover = if world.effects.occlusion && cover.is_finite() {
            cover.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.cover += (cover - self.cover) * ease(COVER_EASE);
        let (steady, storm, wind) = loop_volumes(world, self.cover);
        for (l, t) in self.levels.iter_mut().zip([steady, storm, wind]) {
            *l += (t - *l) * ease(LEVEL_EASE);
            if t == 0.0 && *l < 1e-4 {
                *l = 0.0;
            }
        }
        let [steady, storm, wind] = self.levels;
        let master = settings.volume.clamp(0.0, 1.0);
        let wants_sound = steady > 0.0 || storm > 0.0 || wind > 0.0 || !events.is_empty();
        if !wants_sound && self.clips.is_none() {
            return;
        }
        let clips = self.clips.get_or_insert_with(ensure_clips).clone();
        let pick = |own: &str, made: &Path| -> String {
            if own.is_empty() {
                made.to_string_lossy().into_owned()
            } else {
                own.to_owned()
            }
        };
        let out = settings.output.clone();
        let mut drive = |slot: &mut Option<VoiceId>, clip: String, v: f32| {
            let v = v * master;
            match *slot {
                Some(id) if !audio.voice_alive(id) => *slot = None,
                Some(id) if v <= 0.001 => {
                    audio.stop_voice(id);
                    *slot = None;
                }
                Some(id) => audio.set_voice_volume(id, v, true),
                None => {}
            }
            if slot.is_none() && v > 0.001 {
                *slot = audio.play_ambient(&clip, &out, v, true);
            }
        };
        drive(
            &mut self.steady,
            pick(&settings.rain_clip, &clips.rain_steady),
            steady,
        );
        drive(
            &mut self.storm,
            pick(&settings.storm_clip, &clips.rain_storm),
            storm,
        );
        drive(&mut self.wind, pick(&settings.wind_clip, &clips.wind), wind);
        for e in events {
            if let WeatherEvent::Thunder { distance, volume } = e {
                let own: Vec<&String> = settings
                    .thunder_clips
                    .iter()
                    .filter(|c| !c.is_empty())
                    .collect();
                let clip = if !own.is_empty() {
                    let i = (self.rng.f32() * own.len() as f32) as usize;
                    own[i.min(own.len() - 1)].clone()
                } else {
                    let i = usize::from(self.rng.f32() > 0.5);
                    let set = if distance < 1500.0 {
                        &clips.thunder_near
                    } else {
                        &clips.thunder_far
                    };
                    set[i].to_string_lossy().into_owned()
                };
                if audio
                    .play_ambient(&clip, &out, volume * master, false)
                    .is_some()
                {
                    self.thunder_played += 1;
                }
            }
        }
    }

    /// The eased loop levels (steady rain, storm rain, wind) and cover.
    pub fn levels(&self) -> ([f32; 3], f32) {
        (self.levels, self.cover)
    }

    pub fn stop(&mut self, audio: &mut Audio) {
        for id in [self.steady.take(), self.storm.take(), self.wind.take()]
            .into_iter()
            .flatten()
        {
            audio.stop_voice(id);
        }
        self.levels = [0.0; 3];
        self.cover = 0.0;
    }
}
