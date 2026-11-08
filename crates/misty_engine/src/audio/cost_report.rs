//! CPU cost of the mixer graph, rendered offline on this machine:
//! `cargo test --release -p misty_engine -- --ignored mixer_cost_report --nocapture`.
//! Prints, per voice and per effect instance, the audio-thread time to render one second of
//! 48 kHz stereo, and that as a share of one core.

use std::f32::consts::TAU;
use std::path::Path;
use std::time::Instant;

use glam::Vec3;

use super::spatial::HrtfParams;
use super::*;

const SR: f32 = 48000.0;
const SECONDS: usize = 10;

fn backend() -> GraphBackend {
    let mut b = GraphBackend::offline(SR);
    let tone = wav::synth(1.0, SR as u32, |t| 0.3 * (TAU * 330.0 * t).sin());
    b.preload(Path::new("tone.wav"), tone).unwrap();
    b
}

/// Seconds of CPU to render `SECONDS` of audio.
fn time(b: &mut GraphBackend) -> f64 {
    b.render(4096);
    let start = Instant::now();
    for _ in 0..(SECONDS * SR as usize / 1024) {
        b.render(1024);
    }
    start.elapsed().as_secs_f64()
}

/// µs of CPU per second of audio.
fn per_second(total: f64) -> f64 {
    total / SECONDS as f64 * 1e6
}

fn voices(n: usize, params: VoiceParams) -> f64 {
    let mut b = backend();
    let t = b.add_track(None, &[]).unwrap();
    for _ in 0..n {
        b.play(Path::new("tone.wav"), params, Some(t)).unwrap();
    }
    per_second(time(&mut b))
}

fn with_effect(kind: Option<EffectKind>) -> f64 {
    let mut b = backend();
    let fx: Vec<EffectSpec> = kind
        .into_iter()
        .map(|k| EffectSpec::new(k, k.defaults()).with_id(1))
        .collect();
    let t = b.add_track(None, &fx).unwrap();
    let looping = VoiceParams {
        looping: true,
        ..Default::default()
    };
    b.play(Path::new("tone.wav"), looping, Some(t)).unwrap();
    per_second(time(&mut b))
}

#[test]
#[ignore = "timing report; run on purpose"]
fn mixer_cost_report() {
    let looping = VoiceParams {
        looping: true,
        ..Default::default()
    };
    let pitched = VoiceParams {
        pitch: 1.13,
        ..looping
    };
    let hrtf = VoiceParams {
        binaural: Some(HrtfParams {
            dir: Vec3::new(0.6, 0.0, 0.8).to_array(),
            mix: 1.0,
        }),
        ..pitched
    };
    let n = 32;
    let empty = voices(0, looping);
    println!(
        "Mixer graph cost at 48 kHz (µs of audio-thread CPU per second of audio; % of one core)"
    );
    println!(
        "graph with one empty track: {empty:.0} µs ({:.3} %)",
        empty / 1e4
    );
    for (label, p) in [
        ("voice, 2D, pitch 1", looping),
        ("voice, 2D, resampled", pitched),
        ("voice, spatialized (HRTF)", hrtf),
    ] {
        let per = (voices(n, p) - empty) / n as f64;
        println!(
            "{label}: {per:.0} µs per voice ({:.3} % per voice)",
            per / 1e4
        );
    }
    let base = with_effect(None);
    for kind in EffectKind::ALL {
        if kind == EffectKind::Send || kind == EffectKind::Receive {
            continue;
        }
        let cost = with_effect(Some(kind)) - base;
        println!(
            "{}: {cost:.0} µs per instance ({:.3} %)",
            kind.label(),
            cost / 1e4
        );
    }
}
