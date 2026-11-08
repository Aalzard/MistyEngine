//! The mixer graph rendered offline: send / receive levels, the ducking envelope, live chain
//! and routing edits without clicks, and streamed clips matching decoded ones.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use super::clip::{AudioImporter, LoadType};
use super::dsp::Frame;
use super::dsp::analysis::{amplitude_at, max_step, peak, side};
use super::*;

const SR: f32 = 48000.0;

fn tone(freq: f32, amp: f32, seconds: f32) -> Vec<u8> {
    wav::synth(seconds, SR as u32, |t| amp * (TAU * freq * t).sin())
}

fn backend(clips: &[(&str, Vec<u8>)]) -> GraphBackend {
    let mut b = GraphBackend::offline(SR);
    for (p, bytes) in clips {
        b.preload(Path::new(p), bytes.clone()).unwrap();
    }
    b
}

fn fx(kind: EffectKind, values: &[f32], id: u32) -> EffectSpec {
    EffectSpec::new(kind, values.to_vec()).with_id(id)
}

fn send(level_db: f32, id: u32, target: (TrackId, u32)) -> EffectSpec {
    let mut s = fx(EffectKind::Send, &[level_db], id);
    s.target = Some(target);
    s
}

fn looping() -> VoiceParams {
    VoiceParams {
        looping: true,
        ..Default::default()
    }
}

fn secs(s: f32) -> usize {
    (s * SR) as usize
}

/// Amplitude of `freq` on the left between `from` and `to` seconds of `out`.
fn level(out: &[Frame], freq: f32, from: f32, to: f32) -> f32 {
    amplitude_at(&side(&out[secs(from)..secs(to)], false), freq, SR)
}

#[test]
fn sends_deliver_their_level_to_receives() {
    let mut b = backend(&[("tone.wav", tone(440.0, 0.5, 1.0))]);
    let a = b
        .add_track(None, &[fx(EffectKind::Send, &[-80.0], 1)])
        .unwrap();
    let bus = b
        .add_track(None, &[fx(EffectKind::Receive, &[], 2)])
        .unwrap();
    // A's own output muted: only what it sends (at -6 dB) is heard, through the bus.
    b.set_track(a, 0.0, &[send(-6.0206, 1, (bus, 2))]);
    b.play(Path::new("tone.wav"), looping(), Some(a)).unwrap();
    let out = b.render(secs(0.5)).unwrap();
    let got = level(&out, 440.0, 0.1, 0.5);
    assert!((got - 0.25).abs() < 0.003, "send at -6 dB: {got}");
    // Unmuted: the dry path plus the send (in phase).
    b.set_track(a, 1.0, &[send(-6.0206, 1, (bus, 2))]);
    let out = b.render(secs(0.5)).unwrap();
    assert!((level(&out, 440.0, 0.1, 0.5) - 0.75).abs() < 0.005);
    // The receiving bus has its own fader.
    b.set_track(bus, 0.5, &[fx(EffectKind::Receive, &[], 2)]);
    let out = b.render(secs(0.5)).unwrap();
    assert!((level(&out, 440.0, 0.1, 0.5) - 0.625).abs() < 0.005);
    // Send level at the bottom of the fader: nothing goes over.
    b.set_track(a, 1.0, &[send(-80.0, 1, (bus, 2))]);
    let out = b.render(secs(0.5)).unwrap();
    assert!((level(&out, 440.0, 0.1, 0.5) - 0.5).abs() < 0.003);
    // Bypassing the send fades it out too.
    let mut bypassed = send(0.0, 1, (bus, 2));
    bypassed.bypass = true;
    b.set_track(a, 1.0, &[bypassed]);
    let out = b.render(secs(0.5)).unwrap();
    assert!((level(&out, 440.0, 0.1, 0.5) - 0.5).abs() < 0.003);
}

#[test]
fn sends_reach_receivers_on_tracks_processed_later_and_loops_still_render() {
    // Reverb bus under Master; SFX under Master sends to it. The bus is processed after SFX,
    // so the send lands in the same block (no latency).
    let mut input = vec![0.0f32; secs(0.2)];
    input[secs(0.05)] = 0.9;
    let click = wav::pcm16_mono(
        &input
            .iter()
            .map(|v| (v * i16::MAX as f32) as i16)
            .collect::<Vec<_>>(),
        SR as u32,
    );
    let mut b = backend(&[("click.wav", click)]);
    let master = b.add_track(None, &[]).unwrap();
    let bus = b
        .add_track(Some(master), &[fx(EffectKind::Receive, &[], 2)])
        .unwrap();
    let sfx = b
        .add_track(Some(master), &[fx(EffectKind::Send, &[0.0], 1)])
        .unwrap();
    b.set_track(sfx, 0.0, &[send(0.0, 1, (bus, 2))]);
    b.play(Path::new("click.wav"), VoiceParams::default(), Some(sfx))
        .unwrap();
    let out = b.render(secs(0.2)).unwrap();
    let at = out.iter().position(|f| f.left.abs() > 0.5).unwrap();
    assert!(at.abs_diff(secs(0.05)) <= 2, "no block of delay: {at}");
    // A send loop (bus back to SFX) renders without hanging or blowing up.
    b.set_chain(
        bus,
        &[fx(EffectKind::Receive, &[], 2), send(-20.0, 3, (sfx, 4))],
    );
    b.set_chain(
        sfx,
        &[fx(EffectKind::Receive, &[], 4), send(0.0, 1, (bus, 2))],
    );
    let out = b.render(secs(0.5)).unwrap();
    assert!(peak(&side(&out, false)) < 2.0);
}

/// Per-millisecond peak level of `x` over one 220 Hz period.
fn envelope(x: &[f32]) -> Vec<f32> {
    let period = (SR / 220.0) as usize + 1;
    (0..x.len().saturating_sub(period))
        .step_by(secs(0.001))
        .map(|i| peak(&x[i..i + period]))
        .collect()
}

#[test]
fn duck_volume_follows_the_send_with_attack_and_release() {
    // Dialogue speaks from 0.5 s to 1.5 s; music is a steady 220 Hz at 0.3.
    let dialogue = wav::synth(3.0, SR as u32, |t| {
        if (0.5..1.5).contains(&t) {
            0.5 * (TAU * 1000.0 * t).sin()
        } else {
            0.0
        }
    });
    let mut b = backend(&[
        ("music.wav", tone(220.0, 0.3, 1.0)),
        ("dialogue.wav", dialogue),
    ]);
    // Threshold -30 dB, 10:1, attack 50 ms, release 300 ms, no make-up, hard knee.
    let duck = fx(
        EffectKind::DuckVolume,
        &[-30.0, 10.0, 50.0, 300.0, 0.0, 0.0, 100.0],
        10,
    );
    let music = b.add_track(None, std::slice::from_ref(&duck)).unwrap();
    let voice = b
        .add_track(None, &[fx(EffectKind::Send, &[0.0], 11)])
        .unwrap();
    // Dialogue's own output muted, so the render is the music alone.
    b.set_track(voice, 0.0, &[send(0.0, 11, (music, 10))]);
    b.play(Path::new("music.wav"), looping(), Some(music))
        .unwrap();
    b.play(
        Path::new("dialogue.wav"),
        VoiceParams::default(),
        Some(voice),
    )
    .unwrap();
    let out = b.render(secs(3.0)).unwrap();
    let env = envelope(&side(&out, false));
    let at = |t: f32| env[(t * 1000.0) as usize];
    assert!(
        (at(0.4) / 0.3 - 1.0).abs() < 0.02,
        "full level before: {}",
        at(0.4)
    );
    // Steady ducking: detected -6 dB is 24 dB over, at 10:1 that's -21.6 dB.
    let ducked = 20.0 * (at(1.2) / 0.3).log10();
    assert!((ducked + 21.6).abs() < 1.5, "ducked by {ducked} dB");
    // Attack: 10 dB of reduction within ~13 ms of the dialogue starting.
    let attack = (500..800).find(|ms| env[*ms] < 0.3 * 0.316).unwrap() as f32 - 500.0;
    assert!((5.0..30.0).contains(&attack), "attack {attack} ms");
    // Release: back within 1 dB about 0.79 s after it stops (300 ms release from -6 dB down to
    // -28.9 dB), rising all the way.
    let back = (1500..3000).find(|ms| env[*ms] > 0.3 * 0.891).unwrap() as f32 / 1000.0 - 1.5;
    assert!((back - 0.79).abs() < 0.08, "released after {back} s");
    for w in env[1520..2200].windows(20).step_by(20) {
        assert!(w[19] >= w[0] - 1e-3, "release only rises");
    }
    assert!(b.track_reduction(music) < -15.0, "the meter saw it");
}

#[test]
fn chain_and_routing_edits_while_playing_never_click() {
    let mut b = backend(&[("tone.wav", tone(440.0, 0.5, 1.0))]);
    let master = b.add_track(None, &[]).unwrap();
    let t = b.add_track(Some(master), &[]).unwrap();
    let v = b.play(Path::new("tone.wav"), looping(), Some(t)).unwrap();
    let lp = fx(EffectKind::Lowpass, &[1000.0, 1.0], 1);
    let echo = fx(EffectKind::Echo, &[120.0, 0.0, 100.0, 30.0], 2);
    let mut out = b.render(secs(0.2)).unwrap();
    let step = |b: &mut GraphBackend, out: &mut Vec<Frame>| {
        out.extend(b.render(secs(0.2)).unwrap());
    };
    // Insert, insert before, reorder, bypass, remove everything.
    b.set_chain(t, std::slice::from_ref(&lp));
    step(&mut b, &mut out);
    b.set_chain(t, &[echo.clone(), lp.clone()]);
    step(&mut b, &mut out);
    b.set_chain(t, &[lp.clone(), echo.clone()]);
    step(&mut b, &mut out);
    let mut off = lp.clone();
    off.bypass = true;
    b.set_track(t, 1.0, &[off, echo.clone()]);
    step(&mut b, &mut out);
    b.set_chain(t, &[]);
    step(&mut b, &mut out);
    // Reroute into a quieter bus, then remove the bus's effect-free parent link.
    let bus = b.add_track(Some(master), &[]).unwrap();
    b.set_track(bus, 0.5, &[]);
    step(&mut b, &mut out);
    b.set_parent(t, Some(bus));
    step(&mut b, &mut out);
    b.move_voice(v, Some(master));
    step(&mut b, &mut out);
    // A 440 Hz sine at 0.65 at most moves 0.038 per sample; a hard switch jumps far more.
    let worst = max_step(&side(&out, false));
    assert!(worst < 0.06, "largest jump {worst}");
    assert!(b.is_alive(v), "never restarted");
    let pos = b.position(v);
    let played = out.len() as f64 / SR as f64;
    assert!(
        (pos - played.rem_euclid(1.0)).abs() < 1e-3,
        "position {pos} after {played}"
    );
    // The edits took effect: the bus at 0.5 is what's heard at the end.
    let tail = &out[out.len() - secs(0.1)..];
    assert!((amplitude_at(&side(tail, false), 440.0, SR) - 0.5).abs() < 0.01);
}

#[test]
fn removing_a_track_stops_its_voices() {
    let mut b = backend(&[("tone.wav", tone(440.0, 0.5, 1.0))]);
    let t = b.add_track(None, &[]).unwrap();
    let v = b.play(Path::new("tone.wav"), looping(), Some(t)).unwrap();
    b.render(secs(0.1));
    b.remove_track(t);
    let out = b.render(secs(0.2)).unwrap();
    assert!(!b.is_alive(v));
    assert!(
        peak(&side(&out[secs(0.05)..], false)) < 1e-6,
        "silent once faded"
    );
    assert!(max_step(&side(&out, false)) < 0.06, "faded, not cut");
}

/// A project folder with the same stereo clip three times, one per load type.
fn clip_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_stream_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    let rate = 44100;
    let frames: Vec<(f32, f32)> = (0..(2.0 * rate as f32) as usize)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let wobble = ((i as u32).wrapping_mul(2654435761) >> 16) as f32 / 65536.0 - 0.5;
            (
                0.4 * (TAU * 300.0 * t).sin() + 0.05 * wobble,
                0.3 * (TAU * 450.0 * t).sin(),
            )
        })
        .collect();
    let bytes = wav::pcm16_stereo(&frames, rate);
    for (name, load) in [
        ("Decoded.wav", LoadType::DecompressOnLoad),
        ("Streamed.wav", LoadType::Streaming),
        ("Compressed.wav", LoadType::CompressedInMemory),
    ] {
        let path = root.join("Assets").join(name);
        std::fs::write(&path, &bytes).unwrap();
        if load != LoadType::DecompressOnLoad {
            AudioImporter {
                load_type: load,
                ..Default::default()
            }
            .save(&path)
            .unwrap();
        }
    }
    root
}

/// Render `clip` alone: play with `params`, optionally seek, render `seconds`.
fn render_clip(path: &Path, params: VoiceParams, seek: Option<f64>, seconds: f32) -> Vec<Frame> {
    let mut b = GraphBackend::offline(SR);
    let v = b.play(path, params, None).unwrap();
    if let Some(s) = seek {
        b.seek(v, s);
    }
    b.render(secs(seconds)).unwrap()
}

#[test]
fn streamed_and_compressed_clips_play_exactly_like_decoded_ones() {
    let root = clip_project("match");
    let decoded = root.join("Assets/Decoded.wav");
    let cases = [
        (VoiceParams::default(), None, 2.5),
        (looping(), None, 5.0),
        (
            VoiceParams {
                pitch: 1.37,
                ..looping()
            },
            None,
            3.0,
        ),
        (VoiceParams::default(), Some(1.25), 1.0),
    ];
    for (params, seek, seconds) in cases {
        let want = render_clip(&decoded, params, seek, seconds);
        assert!(peak(&side(&want, false)) > 0.3, "the reference plays");
        for other in ["Streamed.wav", "Compressed.wav"] {
            let got = render_clip(&root.join("Assets").join(other), params, seek, seconds);
            let worst = want
                .iter()
                .zip(&got)
                .map(|(a, b)| (a.left - b.left).abs().max((a.right - b.right).abs()))
                .fold(0.0f32, f32::max);
            assert!(
                worst < 1e-6,
                "{other} {params:?} seek {seek:?}: differs by {worst}"
            );
        }
    }
    // A finished stream ends its voice.
    let mut b = GraphBackend::offline(SR);
    let v = b
        .play(
            &root.join("Assets/Streamed.wav"),
            VoiceParams::default(),
            None,
        )
        .unwrap();
    b.render(secs(2.2));
    assert!(!b.is_alive(v));
    assert_eq!(b.underruns(), 0, "offline renders wait for the worker");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn clip_info_and_force_to_mono() {
    let root = clip_project("info");
    let path = root.join("Assets/Decoded.wav");
    let info = clip::info(&path).unwrap();
    assert_eq!(info.summary(), "44100 Hz, Stereo, 2.000 s");
    assert_eq!(info.format, "WAV (PCM)");
    AudioImporter {
        force_to_mono: true,
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let out = render_clip(&path, VoiceParams::default(), None, 0.5);
    assert!(out[1000..].iter().all(|f| (f.left - f.right).abs() < 1e-6));
    assert!(peak(&side(&out, true)) > 0.2, "both sides carry the mix");
    std::fs::remove_dir_all(root).ok();
}
