//! The Recorder's sound: taps placed by their sample clock, silence where no tap reaches,
//! exact lengths, Opus that decodes back, and sound that stays on its frames over minutes of
//! MP4 and WebM.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use misty_engine::audio::dsp::Frame as Sample;
use misty_engine::audio::tap::tap;
use misty_engine::audio::{Audio, wav};

use crate::audio::{AudioFeed, OpusStream, RATE, Resampler, decode_opus};
use crate::schedule::Step;
use crate::session::Recording;
use crate::{Frame, Output, OutputFormat, PatternContext, RangeMode, RecorderSettings, mp4, webm};

fn temp(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("misty_recaudio_{tag}_{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tone(n: usize, hz: f32, rate: f32) -> Vec<Sample> {
    (0..n)
        .map(|i| {
            let v = 0.5 * (TAU * hz * i as f32 / rate).sin();
            Sample { left: v, right: v }
        })
        .collect()
}

/// Rising zero crossings a second of the left channel.
fn frequency(interleaved: &[f32]) -> f32 {
    let left: Vec<f32> = interleaved.iter().step_by(2).copied().collect();
    let ups = left
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    ups as f32 / (left.len() as f32 / RATE as f32)
}

#[test]
fn resampling_keeps_counts_exact_and_pitch_right() {
    let mut r = Resampler::new(44100.0);
    let input = tone(441_000, 1000.0, 44100.0);
    let mut out = Vec::new();
    // Uneven blocks, as device callbacks come.
    for chunk in input.chunks(733) {
        r.push(chunk, &mut out);
    }
    assert!(
        out.len().abs_diff(480_000) <= 3,
        "10 s at 44.1 kHz gave {} samples at 48 kHz",
        out.len()
    );
    let inter: Vec<f32> = out.iter().flat_map(|f| [f.left, f.right]).collect();
    let hz = frequency(&inter);
    assert!((hz - 1000.0).abs() < 1.0, "{hz} Hz");
}

#[test]
fn the_feed_places_samples_by_their_sample_clock() {
    // The recording's first frame is at 1 s of capture time; the tap starts at 0.5 s.
    let (mut w, r) = tap(200_000);
    let mut feed = AudioFeed::new();
    feed.add_tap(r);
    feed.begin(1.0);
    let ones = vec![
        Sample {
            left: 1.0,
            right: -1.0
        };
        48_000
    ];
    w.write(24_000, 48_000.0, &ones);
    let got = feed.pull(1.5);
    // Half a second of it is before the first frame: dropped.
    assert_eq!(got.len(), 24_000 * 2);
    assert_eq!(&got[..2], &[1.0, -1.0]);
    // A new game session's tap from 2 s (nothing covered 1.5 s to 2 s): silence in between.
    let (mut w2, r2) = tap(10_000);
    feed.add_tap(r2);
    drop(w);
    w2.write(96_000, 48_000.0, &ones[..4800]);
    let got = feed.pull(2.2);
    assert_eq!(got.len(), (24_000 + 4800) * 2);
    assert!(got[..48_000].iter().all(|s| *s == 0.0));
    assert_eq!(&got[48_000..48_002], &[1.0, -1.0]);
    assert_eq!(feed.written(), 72_000 + 4800 - 24_000);
}

#[test]
fn no_tap_gives_silence_of_exactly_the_videos_length() {
    let mut feed = AudioFeed::new();
    feed.begin(0.0);
    // Capture time at 5 s: what is more than the allowed lag behind is silence already.
    let got = feed.pull(5.0);
    assert_eq!(got.len(), 3 * 48_000 * 2);
    assert!(got.iter().all(|s| *s == 0.0));
    let rest = feed.finish(5 * 48_000, std::time::Duration::ZERO);
    assert_eq!(got.len() + rest.len(), 5 * 48_000 * 2);
}

#[test]
fn a_device_rate_tap_comes_out_at_48k_with_the_right_count() {
    let (mut w, r) = tap(500_000);
    let mut feed = AudioFeed::new();
    feed.add_tap(r);
    feed.begin(0.0);
    let input = tone(441_000, 440.0, 44100.0);
    let mut total = 0;
    for (k, chunk) in input.chunks(1470).enumerate() {
        w.write(k as u64 * 1470, 44100.0, chunk);
        total += feed.pull((k + 1) as f64 / 30.0).len() / 2;
    }
    assert!(total.abs_diff(480_000) <= 3, "{total}");
    let rest = feed.finish(480_000, std::time::Duration::ZERO);
    assert_eq!(total + rest.len() / 2, 480_000);
}

#[test]
fn opus_round_trips_in_step() {
    let mut enc = OpusStream::new(160_000).unwrap();
    let input: Vec<f32> = tone(48_000, 440.0, 48_000.0)
        .iter()
        .flat_map(|f| [f.left, f.right])
        .collect();
    let mut packets = Vec::new();
    for chunk in input.chunks(1234) {
        packets.extend(enc.push(chunk).unwrap());
    }
    packets.extend(enc.finish().unwrap());
    assert_eq!(enc.samples(), 48_000);
    let mut out = decode_opus(&packets, enc.pre_skip as usize).unwrap();
    assert!(out.len() >= 48_000 * 2, "the delay is flushed out too");
    out.truncate(48_000 * 2);
    assert!((frequency(&out) - 440.0).abs() < 2.0);
    // Lined up with the input: the best match is at no shift.
    let score = |lag: i64| -> f32 {
        (4800..43_200)
            .map(|i| {
                let j = (i as i64 + lag) as usize;
                input[i * 2] * out[j * 2]
            })
            .sum()
    };
    let best = (-40..=40)
        .max_by(|a, b| score(*a).total_cmp(&score(*b)))
        .unwrap();
    assert!(best.abs() <= 1, "decoded sound is {best} samples off");
    let head = enc.head();
    assert_eq!(&head[..8], b"OpusHead");
    assert_eq!(head.len(), 19);
}

/// A project with a short click: 5 ms of 1 kHz.
fn click_project(rate: u32) -> PathBuf {
    let root = temp("proj");
    std::fs::create_dir_all(root.join("Assets/Audio")).unwrap();
    let click = wav::synth(0.005, rate, |t| 0.8 * (TAU * 1000.0 * t).sin());
    std::fs::write(root.join("Assets/Audio/Click.wav"), click).unwrap();
    root
}

fn ctx() -> PatternContext<'static> {
    PatternContext {
        scene: "Demo",
        recorder: "Rec",
        project: "Proj",
        take: 1,
        width: 32,
        height: 32,
        source: "Scene Viewer",
    }
}

/// Record `seconds` the way the editor does, with a click every `every` seconds played on the
/// game's frame: game update, the mix advanced one frame, the frame captured, the sound pumped.
/// The mix runs offline at `mix_rate`.
fn record_clicks(format: OutputFormat, seconds: u32, every: u32, mix_rate: f32) -> PathBuf {
    let fps = 30u32;
    let root = click_project(mix_rate as u32);
    let settings = RecorderSettings {
        format,
        fps,
        range: RangeMode::Manual,
        folder: root.join("Out").to_string_lossy().into_owned(),
        quality: 50,
        ..Default::default()
    };
    let out = Output::new(&settings, &root, &ctx(), 32, 32);
    assert!(out.audio, "Record Audio is on by default");
    let mut rec = Recording::start(settings, out).unwrap();
    let mut audio = Audio::offline(root.clone(), mix_rate);
    audio.start_capture(0.0);
    rec.add_tap(audio.capture_tap());
    for k in 0..fps * seconds {
        if k % (fps * every) == 0 {
            assert!(audio.play_clip("Assets/Audio/Click.wav", 1.0));
        }
        let now = k as f64 / fps as f64;
        let next = (k + 1) as f64 / fps as f64;
        audio.advance_capture(next);
        match rec.step() {
            Step::Capture(i) => {
                rec.mark_frame(now);
                rec.push(Frame::solid(i, 32, 32, [(k % 255) as u8, 40, 90, 255]));
            }
            other => panic!("{other:?}"),
        }
        rec.pump_audio(next);
    }
    let summary = rec.finish().unwrap();
    assert_eq!(summary.frames, (fps * seconds) as u64);
    summary.files[0].clone()
}

/// Where each click starts in `pcm` (interleaved, 48 kHz), searched near where it should be.
fn click_errors(pcm: &[f32], seconds: u32, every: u32) -> Vec<i64> {
    (0..seconds / every)
        .map(|n| {
            let want = (n * every * RATE) as i64;
            let from = (want - 2400).max(0) as usize;
            let at = (from..from + 4800)
                .find(|i| pcm.get(i * 2).is_some_and(|s| s.abs() > 0.1))
                .unwrap_or_else(|| panic!("click {n} not found"));
            at as i64 - want
        })
        .collect()
}

fn check_sync(errors: &[i64]) {
    let (lo, hi) = (*errors.iter().min().unwrap(), *errors.iter().max().unwrap());
    // Every click within 2 ms of its frame, and no drift from the first to the last.
    assert!(lo >= -96 && hi <= 96, "click offsets {errors:?} (samples)");
    assert!(hi - lo <= 24, "drift over the clip: {errors:?} (samples)");
}

#[test]
fn mp4_sound_stays_on_its_frames_over_three_minutes() {
    let seconds = 180;
    // A 44.1 kHz device mix, resampled to 48 kHz on the way.
    let path = record_clicks(OutputFormat::Mp4, seconds, 10, 44100.0);
    let video = mp4::read_samples(&path).unwrap();
    assert_eq!(video.samples.len(), 30 * seconds as usize);
    let a = mp4::read_audio(&path).unwrap().expect("an audio track");
    assert_eq!(a.codec, "Opus");
    assert_eq!((a.channels, a.sample_rate), (2, 48_000));
    assert_eq!(
        a.samples,
        (seconds * RATE) as u64,
        "exactly the video's length"
    );
    let pcm = decode_opus(&a.packets, a.pre_skip as usize).unwrap();
    assert!(pcm.len() / 2 >= a.samples as usize);
    check_sync(&click_errors(&pcm, seconds, 10));
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn webm_sound_stays_on_its_frames_over_a_minute() {
    let seconds = 60;
    let path = record_clicks(OutputFormat::WebM, seconds, 5, 48000.0);
    let info = webm::read_blocks(&path).unwrap();
    assert_eq!(info.codec, "V_AV1");
    assert_eq!(info.blocks.len(), 30 * seconds as usize);
    assert_eq!(info.audio_codec, "A_OPUS");
    assert_eq!(info.channels, 2);
    assert_eq!(info.sampling_frequency, 48_000.0);
    assert_eq!(info.duration_ms, seconds as f64 * 1000.0);
    // Blocks in time order, 20 ms a packet.
    assert!(info.audio_blocks.windows(2).all(|w| w[1].0 == w[0].0 + 20));
    let pre_skip = (info.codec_delay_ns * RATE as u64).div_ceil(1_000_000_000) as usize;
    assert_eq!(&info.audio_private[..8], b"OpusHead");
    let head_skip = u16::from_le_bytes([info.audio_private[10], info.audio_private[11]]);
    assert_eq!(pre_skip, head_skip as usize);
    let packets: Vec<Vec<u8>> = info.audio_blocks.iter().map(|b| b.1.clone()).collect();
    let pcm = decode_opus(&packets, pre_skip).unwrap();
    check_sync(&click_errors(&pcm, seconds, 5));
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn image_sequences_get_a_wav_of_the_same_length() {
    let dir = temp("seqwav");
    let settings = RecorderSettings {
        format: OutputFormat::PngSequence,
        range: RangeMode::FrameRange,
        start_frame: 0,
        end_frame: 11,
        fps: 24,
        folder: dir.to_string_lossy().into_owned(),
        pattern: "shot_<Frame>".into(),
        ..Default::default()
    };
    let out = Output::new(&settings, Path::new("."), &ctx(), 16, 16);
    let mut rec = Recording::start(settings, out).unwrap();
    while let Step::Capture(i) = rec.step() {
        rec.mark_frame(i as f64 / 24.0);
        rec.push(Frame::solid(i, 16, 16, [1, 2, 3, 255]));
    }
    let files = rec.finish().unwrap().files;
    assert_eq!(files.len(), 13);
    let wav_path = dir.join("shot.wav");
    assert_eq!(files[12], wav_path);
    let bytes = std::fs::read(&wav_path).unwrap();
    // Half a second of 16-bit stereo at 48 kHz.
    assert_eq!(bytes.len(), 44 + 24_000 * 4);
    assert_eq!(wav::duration_of(&wav_path), Some(0.5));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gifs_and_record_audio_off_have_no_sound() {
    let dir = temp("nosound");
    for (format, on) in [(OutputFormat::Gif, true), (OutputFormat::Mp4, false)] {
        let s = RecorderSettings {
            format,
            record_audio: on,
            folder: dir.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = Output::new(&s, Path::new("."), &ctx(), 16, 16);
        assert!(!out.audio, "{format:?}");
    }
    let s = RecorderSettings {
        format: OutputFormat::Mp4,
        record_audio: false,
        range: RangeMode::FrameRange,
        start_frame: 0,
        end_frame: 4,
        folder: dir.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let out = Output::new(&s, Path::new("."), &ctx(), 16, 16);
    let mut rec = Recording::start(s, out).unwrap();
    while let Step::Capture(i) = rec.step() {
        rec.push(Frame::solid(i, 16, 16, [1, 2, 3, 255]));
    }
    let files = rec.finish().unwrap().files;
    assert!(mp4::read_audio(&files[0]).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
