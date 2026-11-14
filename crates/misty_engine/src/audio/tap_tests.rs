//! Taps on the final mix and capture time (the Recorder's sound): nothing happens without a
//! subscriber, a tap gets exactly the samples of the time the game advanced, at the right
//! sample clock, whether the mix runs offline or on the device's thread.

use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};

use super::dsp::Frame;
use super::engine::{ClockStart, Command, Engine, Status};
use super::tap::{MixTap, tap};
use super::*;

const SR: f32 = 48000.0;

fn project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_tap_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Audio")).unwrap();
    let tone = wav::synth(1.0, SR as u32, |t| 0.5 * (TAU * 500.0 * t).sin());
    std::fs::write(root.join("Assets/Audio/Tone.wav"), tone).unwrap();
    root
}

fn drain(tap: &mut MixTap) -> Vec<Frame> {
    let mut out = Vec::new();
    tap.read(&mut out);
    out
}

fn loud(f: &Frame) -> bool {
    f.left.abs() > 1e-4 || f.right.abs() > 1e-4
}

#[test]
fn no_subscriber_means_no_tap_and_the_same_mix() {
    let root = project("none");
    let mut plain = Audio::offline(root.clone(), SR);
    let mut tapped = Audio::offline(root.clone(), SR);
    assert!(plain.play_clip("Assets/Audio/Tone.wav", 1.0));
    assert!(tapped.play_clip("Assets/Audio/Tone.wav", 1.0));
    // A tap whose reader is gone is dropped by the graph on its next block.
    drop(tapped.capture_tap());
    let a = plain.render(4800).unwrap();
    let b = tapped.render(4800).unwrap();
    assert_eq!(a, b, "a tap never changes what is heard");

    let status = Arc::new(Mutex::new(Status::default()));
    let mut engine = Engine::new(SR, None, status);
    let mut out = vec![Frame::ZERO; 512];
    engine.render(&mut out);
    assert_eq!(engine.tap_count(), 0, "no subscriber: nothing to copy into");
    let (w, r) = tap(1024);
    engine.apply(Command::AddTap(w));
    engine.render(&mut out);
    assert_eq!(engine.tap_count(), 1);
    drop(r);
    engine.render(&mut out);
    assert_eq!(engine.tap_count(), 0, "the reader went: the tap is gone");
}

#[test]
fn a_silent_mix_taps_silence() {
    let root = project("silent");
    let mut audio = Audio::offline(root, SR);
    audio.start_capture(0.0);
    let mut tap = audio.capture_tap();
    for k in 1..=30 {
        audio.advance_capture(k as f64 / 30.0);
    }
    let got = drain(&mut tap);
    assert_eq!(got.len(), 48000);
    assert!(!got.iter().any(loud), "nothing plays: the tap is silent");
}

#[test]
fn capture_time_gives_exactly_the_samples_of_the_time_advanced() {
    let root = project("count");
    for (rate, fps, seconds) in [(48000.0, 30u32, 10u32), (44100.0, 24, 7), (48000.0, 60, 3)] {
        let mut audio = Audio::offline(root.clone(), rate);
        assert!(audio.play_clip("Assets/Audio/Tone.wav", 1.0));
        audio.start_capture(0.0);
        let mut tap = audio.capture_tap();
        let mut got = Vec::new();
        for k in 1..=fps * seconds {
            audio.advance_capture(k as f64 / fps as f64);
            tap.read(&mut got);
        }
        assert_eq!(tap.start(), Some(0));
        assert_eq!(tap.sample_rate(), Some(rate));
        assert_eq!(
            got.len(),
            (rate as u32 * seconds) as usize,
            "{rate} Hz at {fps} fps for {seconds} s"
        );
    }
}

#[test]
fn a_tap_starting_later_knows_where_it_is_in_capture_time() {
    let root = project("late");
    let mut audio = Audio::offline(root, SR);
    audio.start_capture(0.0);
    for k in 1..=45 {
        audio.advance_capture(k as f64 / 30.0);
    }
    let mut tap = audio.capture_tap();
    for k in 46..=90 {
        audio.advance_capture(k as f64 / 30.0);
    }
    assert_eq!(tap.start(), Some(72000), "1.5 s at 48 kHz");
    assert_eq!(drain(&mut tap).len(), 72000);
}

#[test]
fn a_sound_started_on_a_frame_starts_on_that_frames_sample() {
    let root = project("onset");
    let mut audio = Audio::offline(root, SR);
    audio.start_capture(0.0);
    let mut tap = audio.capture_tap();
    for k in 1..=30u32 {
        if k == 11 {
            // Played during frame 10's update: heard from frame 10's first sample (1/3 s).
            assert!(audio.play_clip("Assets/Audio/Tone.wav", 1.0));
        }
        audio.advance_capture(k as f64 / 30.0);
    }
    let got = drain(&mut tap);
    let first = got.iter().position(loud).unwrap();
    assert!(
        (16000..16010).contains(&first),
        "the tone starts at sample {first}, expected 16000"
    );
}

#[test]
fn the_device_thread_catches_up_and_keeps_command_order() {
    // The device's queue: commands for several frames arrive between two device blocks (the
    // game running faster than real time). Each frame's commands apply at its own sample.
    let root = project("device");
    let path = root.join("Assets/Audio/Tone.wav");
    let bytes = std::fs::read(&path).unwrap();
    let data = kira::sound::static_sound::StaticSoundData::from_cursor(std::io::Cursor::new(bytes))
        .unwrap();
    let (mut tx, rx) = rtrb::RingBuffer::new(64);
    let status = Arc::new(Mutex::new(Status::default()));
    let mut engine = Engine::new(SR, Some(rx), status);
    let (w, mut r) = tap(48000);
    tx.push(Command::Clock(Some(ClockStart::new(0.0, SR))))
        .ok()
        .unwrap();
    tx.push(Command::AddTap(w)).ok().unwrap();
    for k in 1..=6u32 {
        if k == 4 {
            tx.push(Command::Play {
                id: 1,
                source: engine::Source::Static {
                    frames: data.frames.clone(),
                    rate: data.sample_rate,
                },
                params: VoiceParams::default(),
                track: None,
            })
            .ok()
            .unwrap();
        }
        tx.push(Command::Advance(k as f64 / 60.0)).ok().unwrap();
    }
    let mut out = vec![Frame::ZERO; 512];
    engine.render(&mut out);
    let mut heard = out.clone();
    let got = drain(&mut r);
    assert_eq!(got.len(), 4800, "six 60 Hz frames rendered in one device block");
    let first = got.iter().position(loud).unwrap();
    assert!(
        (2400..2410).contains(&first),
        "played on frame 3 (of 0..6): starts at sample {first}, expected 2400"
    );
    // Nothing advanced since: the device plays what was rendered, then silence.
    for _ in 0..20 {
        engine.render(&mut out);
        heard.extend_from_slice(&out);
    }
    assert_eq!(heard.iter().position(loud), Some(first), "the device hears the same mix");
    assert!(!out.iter().any(loud), "game paused: the device is silent");
    assert_eq!(drain(&mut r).len(), 0);
}

#[test]
fn capture_time_holds_over_a_long_clip_at_device_rates() {
    // Ten minutes at 30 fps: the sample count is exact, with no drift from rounding.
    let root = project("long");
    for rate in [44100.0, 48000.0] {
        let mut audio = Audio::offline(root.clone(), rate);
        audio.start_capture(0.0);
        let mut tap = audio.capture_tap();
        let mut total = 0usize;
        let mut got = Vec::new();
        for k in 1..=30 * 600u32 {
            audio.advance_capture(k as f64 / 30.0);
            got.clear();
            total += tap.read(&mut got);
            let want = (k as f64 / 30.0 * rate as f64).round() as usize;
            assert!(total.abs_diff(want) == 0, "frame {k}: {total} vs {want}");
        }
    }
}

#[test]
fn leaving_capture_time_returns_to_real_time() {
    let root = project("leave");
    let mut audio = Audio::offline(root, SR);
    assert!(audio.play_clip("Assets/Audio/Tone.wav", 1.0));
    audio.start_capture(0.0);
    assert!(audio.capturing());
    audio.advance_capture(0.1);
    audio.stop_capture();
    assert!(!audio.capturing());
    let out = audio.render(4800).unwrap();
    assert!(out.iter().any(loud), "real time again: the mix renders on demand");
}
