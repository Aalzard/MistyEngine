//! Weather sounds: rain loops that follow the intensity, a wind loop for strong wind and
//! blizzards, and thunder that arrives `distance / 343 m/s` after each flash, all through the
//! audio mixer (the volume's Output group).
//!
//! Without clips of their own (Weather ▸ Audio ▸ Rain / Wind / Thunder clips), the sounds are
//! generated once (filtered noise, droplet ticks, cracks and rumbles) into
//! `Library/Weather/` of the project (or the temp folder) and played from there. To use your
//! own recordings, drop WAV/OGG files into the project and pick them in those fields.

use std::path::{Path, PathBuf};

use super::lightning::Rng;
use super::state::{WeatherEvent, WeatherWorld};
use crate::audio::{Audio, VoiceId};

/// Sample rate of the generated clips.
pub const RATE: u32 = 32_000;

/// Generated clip files (absolute paths).
#[derive(Clone, Debug)]
pub struct Clips {
    pub rain_light: PathBuf,
    pub rain_heavy: PathBuf,
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
        let (a, b) = ((t * std::f32::consts::FRAC_PI_2).sin(), (t * std::f32::consts::FRAC_PI_2).cos());
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

/// A rain loop: a hiss of many distant drops plus nearby droplet ticks (`heavy` = a denser
/// roar with fewer distinct ticks).
pub fn rain(seconds: f32, heavy: bool, seed: u64) -> Vec<(f32, f32)> {
    let mut rng = Rng::new(seed);
    let n = (seconds * RATE as f32) as usize;
    let mut out = vec![(0.0, 0.0); n];
    let (mut lp, mut hp) = ([LowPass::default(); 2], [LowPass::default(); 2]);
    let (lo, hi, hiss) = if heavy {
        (5200.0, 250.0, 0.9)
    } else {
        (7000.0, 600.0, 0.35)
    };
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        // Slow swells as gusts drive the rain.
        let swell = 1.0 + 0.15 * (t * 0.37).sin() + 0.08 * (t * 1.13 + 1.0).sin();
        let mut ch = [0.0f32; 2];
        for (c, v) in ch.iter_mut().enumerate() {
            let w = rng.range(-1.0, 1.0);
            let l = lp[c].run(w, coef(lo));
            let h = l - hp[c].run(l, coef(hi));
            *v = h * hiss * swell;
        }
        *s = (ch[0], ch[1]);
    }
    // Droplet ticks: short decaying bursts at random times, panned.
    let ticks = (seconds * if heavy { 260.0 } else { 140.0 }) as usize;
    for _ in 0..ticks {
        let at = (rng.f32() * n as f32) as usize;
        let len = (RATE as f32 * rng.range(0.004, 0.02)) as usize;
        let amp = rng.range(0.05, if heavy { 0.25 } else { 0.5 });
        let pan = rng.f32();
        let freq = rng.range(1800.0, 6000.0);
        let decay = rng.range(250.0, 400.0);
        for k in 0..len.min(n - at) {
            let t = k as f32 / RATE as f32;
            let env = (-t * decay).exp();
            let v = (t * freq * std::f32::consts::TAU).sin() * env * amp;
            let s = &mut out[at + k];
            s.0 += v * (1.0 - pan);
            s.1 += v * pan;
        }
    }
    let mut v = make_loop(out, 0.6);
    normalize(&mut v, 0.7);
    v
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
        .map(|_| (rng.range(0.2, seconds * 0.7), rng.range(0.3, 1.0), rng.range(0.4, 1.6)))
        .collect();
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let mut env = 0.0f32;
        for &(at, amp, width) in &bumps {
            let d = (t - at) / width;
            if d > -1.0 {
                env += amp * if d < 0.0 { (d + 1.0).powi(2) } else { (-d * 2.0).exp() };
            }
        }
        let attack = if near { (t / 0.01).min(1.0) } else { (t / 0.6).min(1.0) };
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

/// The generated clips, written on first use (version in the names: changes regenerate).
pub fn ensure_clips() -> Clips {
    let dir = clip_dir();
    Clips {
        rain_light: write(&dir, "rain_light_v1.wav", || rain(8.0, false, 11)),
        rain_heavy: write(&dir, "rain_heavy_v1.wav", || rain(8.0, true, 12)),
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

/// Loop volumes for the weather now: (light rain, heavy rain, wind), each 0..1 before the
/// volume's Volume.
pub fn loop_volumes(world: &WeatherWorld) -> (f32, f32, f32) {
    let p = &world.current;
    let r = if p.rain_density > 0.0 {
        p.rain_intensity
    } else {
        0.0
    };
    let light = smoothstep(0.0, 0.35, r) * (1.0 - 0.6 * smoothstep(0.5, 0.95, r));
    let heavy = smoothstep(0.3, 0.9, r);
    let wind = smoothstep(4.0, 18.0, world.wind.length()) * 0.9
        + 0.1 * smoothstep(0.0, 1.0, p.snow_intensity) * smoothstep(1.0, 6.0, p.wind_speed);
    (light, heavy, wind.min(1.0))
}

/// Plays the weather's sounds in Play mode.
#[derive(Default)]
pub struct Driver {
    clips: Option<Clips>,
    light: Option<VoiceId>,
    heavy: Option<VoiceId>,
    wind: Option<VoiceId>,
    rng: Rng,
    /// Thunder voices started so far (tests).
    pub thunder_played: u32,
}

impl Driver {
    /// Start, adjust and stop the loops; play thunder that has arrived.
    pub fn update(&mut self, audio: &mut Audio, world: &mut WeatherWorld) {
        let events = world.take_audio_events();
        let settings = world.audio.clone();
        if !world.live || !settings.enabled {
            self.stop(audio);
            return;
        }
        let master = settings.volume.clamp(0.0, 1.0);
        let (light, heavy, wind) = loop_volumes(world);
        let wants_sound = light > 0.0 || heavy > 0.0 || wind > 0.0 || !events.is_empty();
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
        drive(&mut self.light, pick(&settings.rain_clip, &clips.rain_light), light);
        // Your own rain clip plays alone (both loops use it).
        drive(&mut self.heavy, pick(&settings.rain_clip, &clips.rain_heavy), heavy);
        drive(&mut self.wind, pick(&settings.wind_clip, &clips.wind), wind);
        for e in events {
            if let WeatherEvent::Thunder { distance, volume } = e {
                let clip = if !settings.thunder_clips.is_empty() {
                    let i = (self.rng.f32() * settings.thunder_clips.len() as f32) as usize;
                    settings.thunder_clips[i.min(settings.thunder_clips.len() - 1)].clone()
                } else {
                    let i = usize::from(self.rng.f32() > 0.5);
                    let set = if distance < 1500.0 {
                        &clips.thunder_near
                    } else {
                        &clips.thunder_far
                    };
                    set[i].to_string_lossy().into_owned()
                };
                if audio.play_ambient(&clip, &out, volume * master, false).is_some() {
                    self.thunder_played += 1;
                }
            }
        }
    }

    pub fn stop(&mut self, audio: &mut Audio) {
        for id in [self.light.take(), self.heavy.take(), self.wind.take()]
            .into_iter()
            .flatten()
        {
            audio.stop_voice(id);
        }
    }
}
