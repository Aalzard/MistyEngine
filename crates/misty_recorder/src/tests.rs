//! Recorder tests without a GPU: frame ranges give exactly their frames, and every format
//! writes files that read back with the right frame count, size and contents.

use std::path::{Path, PathBuf};

use crate::schedule::{Schedule, Step};
use crate::session::Recording;
use crate::{
    Frame, OutputFormat, Output, PatternContext, RangeMode, RecorderSettings, gif, h264, mp4,
    webm,
};

fn temp(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("misty_rec_{tag}_{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx(w: u32, h: u32) -> PatternContext<'static> {
    PatternContext {
        scene: "Demo",
        recorder: "Rec",
        project: "Proj",
        take: 3,
        width: w,
        height: h,
        source: "Scene Viewer",
    }
}

/// A frame whose colour changes with `i` (a moving gradient, so video frames differ).
fn frame(i: u64, w: u32, h: u32) -> Frame {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = ((x as u64 * 255 / w as u64 + i * 8) % 256) as u8;
            let g = (y * 255 / h) as u8;
            rgba.extend_from_slice(&[r, g, 96, 255]);
        }
    }
    Frame {
        index: i,
        width: w,
        height: h,
        rgba,
    }
}

/// Run a recording the way the editor does: one step per frame of capture time, a frame for
/// every capture, until the range is done (or `max` steps for open-ended ones).
fn record(settings: RecorderSettings, project: &Path, w: u32, h: u32, max: u64) -> Vec<PathBuf> {
    let out = Output::new(&settings, project, &ctx(w, h), w, h);
    let mut rec = Recording::start(settings, out).unwrap();
    for _ in 0..max {
        match rec.step() {
            Step::Skip => {}
            Step::Capture(i) => rec.push(frame(i, w, h)),
            Step::Done => break,
        }
        if rec.complete() {
            break;
        }
    }
    let requested = rec.requested();
    let summary = rec.finish().unwrap();
    assert_eq!(summary.frames, requested);
    summary.files
}

#[test]
fn schedules_hand_out_exactly_the_range() {
    let mut s = Schedule::new(Some((3, 7)));
    let steps: Vec<Step> = (0..10).map(|_| s.step()).collect();
    let captured: Vec<u64> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Capture(i) => Some(*i),
            _ => None,
        })
        .collect();
    assert_eq!(captured, vec![3, 4, 5, 6, 7]);
    assert_eq!(steps[..3], [Step::Skip; 3]);
    assert_eq!(steps[8], Step::Done);
    assert!(s.complete());
    let mut open = Schedule::new(None);
    for i in 0..100 {
        assert_eq!(open.step(), Step::Capture(i));
    }
    assert!(!open.complete());
}

#[test]
fn a_fixed_rate_time_range_writes_exactly_n_frames_as_png() {
    let dir = temp("png");
    let settings = RecorderSettings {
        format: OutputFormat::PngSequence,
        range: RangeMode::TimeRange,
        fps: 24,
        start_time: 0.5,
        end_time: 1.5,
        folder: "Out".into(),
        pattern: "<Scene>_<Take>_<Frame>".into(),
        ..Default::default()
    };
    // One second at 24 fps from frame 12: frames 12..=35.
    let files = record(settings, &dir, 32, 18, 1000);
    assert_eq!(files.len(), 24);
    assert_eq!(files[0], dir.join("Out/Demo_003_0012.png"));
    assert_eq!(files[23], dir.join("Out/Demo_003_0035.png"));
    // The files decode to the frames that went in.
    let img = image::open(&files[5]).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (32, 18));
    assert_eq!(img.into_raw(), frame(17, 32, 18).rgba);
    let on_disk = std::fs::read_dir(dir.join("Out")).unwrap().count();
    assert_eq!(on_disk, 24);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn frame_range_and_jpg_sequence() {
    let dir = temp("jpg");
    let settings = RecorderSettings {
        format: OutputFormat::JpgSequence,
        range: RangeMode::FrameRange,
        start_frame: 0,
        end_frame: 9,
        folder: dir.to_string_lossy().into_owned(),
        pattern: "shot".into(),
        quality: 90,
        ..Default::default()
    };
    let files = record(settings, Path::new("unused"), 40, 30, 1000);
    // No <Frame> in the pattern: sequences get one.
    assert_eq!(files.len(), 10);
    assert_eq!(files[9], dir.join("shot_0009.jpg"));
    let img = image::open(&files[0]).unwrap().to_rgb8();
    assert_eq!(img.dimensions(), (40, 30));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gif_output_has_every_frame_and_its_timing() {
    let dir = temp("gif");
    let settings = RecorderSettings {
        format: OutputFormat::Gif,
        range: RangeMode::FrameRange,
        start_frame: 0,
        end_frame: 14,
        fps: 30,
        folder: dir.to_string_lossy().into_owned(),
        pattern: "<Scene>_<Take>_<Frame>".into(),
        ..Default::default()
    };
    let files = record(settings, Path::new("."), 48, 32, 1000);
    assert_eq!(files, vec![dir.join("Demo_003.gif")]);
    use image::AnimationDecoder;
    let file = std::io::BufReader::new(std::fs::File::open(&files[0]).unwrap());
    let frames = image::codecs::gif::GifDecoder::new(file)
        .unwrap()
        .into_frames()
        .collect_frames()
        .unwrap();
    assert_eq!(frames.len(), 15);
    assert_eq!(frames[0].buffer().dimensions(), (48, 32));
    // 30 fps in hundredths: 3, 3, 4, ... adding up to half a second.
    let total_ms: u32 = frames
        .iter()
        .map(|f| {
            let (n, d) = f.delay().numer_denom_ms();
            n / d
        })
        .sum();
    assert_eq!(total_ms, 500);
    assert_eq!(gif::delay_cs(0, 30) + gif::delay_cs(1, 30) + gif::delay_cs(2, 30), 10);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mp4_output_decodes_back_to_every_frame() {
    let dir = temp("mp4");
    let (w, h) = (64u32, 48u32);
    let settings = RecorderSettings {
        format: OutputFormat::Mp4,
        range: RangeMode::FrameRange,
        start_frame: 0,
        end_frame: 39,
        fps: 20,
        folder: dir.to_string_lossy().into_owned(),
        quality: 90,
        ..Default::default()
    };
    let files = record(settings, Path::new("."), w, h, 1000);
    assert_eq!(files, vec![dir.join("Demo_003.mp4")]);
    let info = mp4::read_samples(&files[0]).unwrap();
    assert_eq!((info.width, info.height), (w, h));
    assert_eq!(info.samples.len(), 40);
    // Two seconds at 20 fps.
    assert_eq!(info.duration as f64 / info.timescale as f64, 2.0);
    assert_eq!(info.keys.first(), Some(&1));
    assert_eq!(h264::Encoded::nal_type(&info.sps), h264::NAL_SPS);
    // Decode every sample with OpenH264.
    let mut dec = openh264::decoder::Decoder::new().unwrap();
    let annex_b = |nal: &[u8]| [&[0u8, 0, 0, 1][..], nal].concat();
    let mut stream = [annex_b(&info.sps), annex_b(&info.pps)].concat();
    let mut decoded = 0;
    let mut middle = None;
    for (i, nals) in info.samples.iter().enumerate() {
        for nal in nals {
            stream.extend_from_slice(&annex_b(nal));
        }
        if let Some(yuv) = dec.decode(&stream).unwrap() {
            use openh264::formats::YUVSource;
            assert_eq!(yuv.dimensions(), (w as usize, h as usize));
            if i == 20 {
                let mut rgb = vec![0u8; (w * h * 3) as usize];
                yuv.write_rgb8(&mut rgb);
                middle = Some(rgb);
            }
            decoded += 1;
        }
        stream.clear();
    }
    decoded += dec.flush_remaining().unwrap().len();
    assert_eq!(decoded, 40);
    // Lossy, but close to what went in.
    let want = frame(20, w, h).rgb();
    let got = middle.expect("frame 20 decoded");
    let err: f64 = want
        .iter()
        .zip(&got)
        .map(|(a, b)| (*a as f64 - *b as f64).abs())
        .sum::<f64>()
        / want.len() as f64;
    assert!(err < 12.0, "mean error {err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn webm_output_holds_every_frame() {
    let dir = temp("webm");
    let (w, h) = (64u32, 48u32);
    let settings = RecorderSettings {
        format: OutputFormat::WebM,
        range: RangeMode::FrameRange,
        start_frame: 0,
        end_frame: 11,
        fps: 24,
        folder: dir.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let files = record(settings, Path::new("."), w, h, 1000);
    assert_eq!(files, vec![dir.join("Demo_003.webm")]);
    let info = webm::read_blocks(&files[0]).unwrap();
    assert_eq!(info.doc_type, "webm");
    assert_eq!(info.codec, "V_AV1");
    assert_eq!((info.width, info.height), (w, h));
    assert_eq!(info.blocks.len(), 12);
    assert!(info.blocks[0].1, "first frame is a key frame");
    assert_eq!(info.duration_ms, 500.0);
    // Timestamps go up a frame (41.7 ms) at a time.
    let times: Vec<u64> = info.blocks.iter().map(|b| b.0).collect();
    assert_eq!(times[0], 0);
    assert_eq!(times[11], 458);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn annex_b_streams_split_into_nal_units() {
    let data = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4, 5];
    let nals = h264::split_annex_b(&data);
    assert_eq!(
        nals,
        vec![&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4, 5][..]]
    );
}

#[test]
fn output_paths_fill_frame_numbers() {
    let s = RecorderSettings {
        format: OutputFormat::PngSequence,
        folder: "Captures".into(),
        pattern: "<Recorder>-<Frame>".into(),
        ..Default::default()
    };
    let out = Output::new(&s, Path::new("/p"), &ctx(8, 8), 8, 8);
    assert_eq!(out.path(Some(7)), Path::new("/p/Captures/Rec-0007.png"));
    let video = Output {
        format: OutputFormat::Mp4,
        ..out
    };
    assert_eq!(video.path(None), Path::new("/p/Captures/Rec.mp4"));
}
