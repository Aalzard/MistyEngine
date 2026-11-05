//! Video decoding and playback: the generated clip round-trips through the AVI writer and
//! reader, audio lines up with the frames, MJPEG frames without Huffman tables decode, players
//! fire their events once, loop and seek.

use std::sync::Arc;
use std::time::Duration;

use super::avi::{Avi, AviCodec, strip_huffman_tables, with_huffman_tables};
use super::generate::{BARS, TestPattern};
use super::*;
use crate::audio::Audio;
use crate::scene::{AudioSource, MeshFilter, MeshRenderer, PrimitiveMesh, Scene};

fn small() -> TestPattern {
    TestPattern {
        width: 160,
        height: 96,
        fps: 20,
        seconds: 2,
        sample_rate: 8000,
        ..Default::default()
    }
}

fn open(p: &TestPattern) -> VideoSource {
    VideoSource::open("clip.avi", Arc::from(p.encode())).expect("generated clip opens")
}

fn close(a: [u8; 4], b: [u8; 3], tol: i32) -> bool {
    (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= tol)
}

#[test]
fn generated_clip_round_trips() {
    let p = small();
    let bytes = p.encode();
    let avi = Avi::parse(&bytes).unwrap();
    assert_eq!(avi.codec, AviCodec::Mjpeg);
    assert_eq!((avi.width, avi.height), (160, 96));
    assert_eq!(avi.frames.len(), 40);
    assert!((avi.fps - 20.0).abs() < 1e-9);
    let src = open(&p);
    let info = src.info().clone();
    assert_eq!(info.frame_count, 40);
    assert!((info.length - 2.0).abs() < 1e-9);
    assert_eq!(info.audio, Some((8000, 1)));
    for n in [0u32, 7, 23, 39] {
        let img = src.frame(n as u64).unwrap();
        assert_eq!(p.read_id(&img), Some(n), "frame id of frame {n}");
        // A pixel in the middle of a bar, away from its edges.
        let y = 20;
        let x = (0..p.width)
            .find(|&x| {
                (x.saturating_sub(4)..(x + 4).min(p.width))
                    .all(|xx| p.bar_at(xx, n) == p.bar_at(x, n))
            })
            .unwrap();
        let want = BARS[p.bar_at(x, n)];
        let got = img.get_pixel(x, y).0;
        assert!(close(got, want, 24), "frame {n} x {x}: {got:?} vs {want:?}");
    }
    assert!(src.frame(40).is_err());
}

#[test]
fn frames_without_huffman_tables_decode_like_the_original() {
    let p = small();
    let img = p.frame(5);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&img)
        .unwrap();
    let stripped = strip_huffman_tables(&jpeg);
    assert!(stripped.len() < jpeg.len());
    assert!(
        image::load_from_memory(&stripped).is_err(),
        "needs its tables"
    );
    let fixed = with_huffman_tables(&stripped);
    let a = image::load_from_memory(&jpeg).unwrap().to_rgba8();
    let b = image::load_from_memory(&fixed).unwrap().to_rgba8();
    assert_eq!(a, b, "standard tables decode identically");
    // Frames that carry tables are left alone.
    assert!(matches!(
        with_huffman_tables(&jpeg),
        std::borrow::Cow::Borrowed(_)
    ));
}

#[test]
fn audio_lines_up_with_video_time() {
    let p = small();
    let src = open(&p);
    let pcm = src.audio().unwrap();
    assert_eq!(pcm.rate, 8000);
    assert!((pcm.duration() - 2.0).abs() < 1e-3);
    // The beep starts each second: first non-silent sample after half a second.
    let first = pcm.samples[4000..]
        .iter()
        .position(|s| s.unsigned_abs() > 500)
        .unwrap()
        + 4000;
    let audio_t = first as f64 / pcm.rate as f64;
    assert!((audio_t - 1.0).abs() < 0.002, "beep at {audio_t}");
    // The frame showing at that moment is frame 20 (1 s at 20 fps).
    let frame = src.frame_at(audio_t);
    assert_eq!(frame, 20);
    assert_eq!(p.read_id(&src.frame(frame).unwrap()), Some(20));
    assert!((src.frame_time(frame) - audio_t).abs() < 1.0 / 20.0);
    // The WAV the audio backend gets has the same length.
    let wav = pcm.to_wav();
    let secs = crate::audio::wav::duration_of_bytes(&wav).unwrap();
    assert!((secs - 2.0).abs() < 1e-3, "{secs}");
}

#[test]
fn unsupported_formats_say_how_to_convert() {
    let err = VideoSource::open("movie.mp4", Arc::from(vec![0u8; 16]))
        .err()
        .unwrap();
    assert!(
        err.contains("Unsupported video format") && err.contains("ffmpeg"),
        "{err}"
    );
    assert!(is_video_like("a/b.MOV") && !is_video("a/b.mov") && is_video("x.avi"));
    let bad = VideoSource::open("x.avi", Arc::from(b"RIFF\0\0\0\0WAVE".to_vec()))
        .err()
        .unwrap();
    assert!(bad.contains("not an AVI"), "{bad}");
}

#[test]
fn gif_clips_and_transcoding() {
    use image::codecs::gif::GifEncoder;
    use image::{Delay, Frame, Rgba, RgbaImage};
    let mut gif = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut gif);
        for c in [[255u8, 0, 0], [0, 255, 0], [0, 0, 255]] {
            let img = RgbaImage::from_pixel(16, 8, Rgba([c[0], c[1], c[2], 255]));
            enc.encode_frame(Frame::from_parts(
                img,
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            ))
            .unwrap();
        }
    }
    let src = VideoSource::open("anim.gif", Arc::from(gif)).unwrap();
    let info = src.info();
    assert_eq!((info.frame_count, info.width), (3, 16));
    assert!((info.length - 0.3).abs() < 1e-6);
    assert_eq!(src.frame_at(0.15), 1);
    assert_eq!(src.frame(2).unwrap().get_pixel(3, 3).0, [0, 0, 255, 255]);
    let avi = transcode(&src, 90).unwrap();
    let back = VideoSource::open("anim.avi", Arc::from(avi)).unwrap();
    assert_eq!(back.info().frame_count, 3);
    assert!(close(
        back.frame(1).unwrap().get_pixel(8, 4).0,
        [0, 255, 0],
        12
    ));
}

#[test]
fn decoder_thread_delivers_frames_in_order_and_seeks() {
    let src = Arc::new(open(&small()));
    let mut d = decoder::Decoder::start(src.clone(), false);
    let a = d.next_timeout(Duration::from_secs(10)).unwrap();
    let b = d.next_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!((a.index, b.index), (0, 1));
    d.seek(30);
    let c = d.next_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!((c.index, c.seq), (30, 30));
    assert_eq!(small().read_id(c.image.as_ref().unwrap()), Some(30));
}

// ---- playback --------------------------------------------------------------------------------

/// A project folder holding the generated clip, and a scene with a screen playing it.
fn setup(name: &str, looping: bool) -> (tempdir::Dir, Scene, hecs::Entity) {
    let dir = tempdir::Dir::new(name);
    std::fs::create_dir_all(dir.0.join("Assets")).unwrap();
    std::fs::write(dir.0.join("Assets/clip.avi"), small().encode()).unwrap();
    let mut s = Scene::new("Video");
    let e = s.spawn("Screen", None);
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Quad));
    s.add(e, MeshRenderer::default());
    s.add(e, AudioSource::default());
    s.add(
        e,
        VideoPlayer {
            clip: Some("Assets/clip.avi".into()),
            looping,
            ..Default::default()
        },
    );
    (dir, s, e)
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Dir {
        pub fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("misty_video_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            Self(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn count(events: &[VideoEvent], f: impl Fn(&VideoEvent) -> bool) -> usize {
    events.iter().filter(|e| f(e)).count()
}

#[test]
fn player_prepares_plays_loops_and_fires_events_once() {
    let (dir, mut s, e) = setup("loop", true);
    let (mut audio, log) = Audio::null(dir.0.clone());
    let mut vp = VideoPlayback::new(dir.0.clone());
    let mut events = Vec::new();
    vp.play_on_awake(&mut s, &mut audio, None);
    assert!(
        vp.is_playing(e),
        "waiting for the first frame counts as playing"
    );
    vp.wait_prepared(Duration::from_secs(10));
    events.extend(vp.update(&mut s, &mut audio, 0.0));
    assert_eq!(count(&events, |v| *v == VideoEvent::PrepareCompleted(e)), 1);
    assert_eq!(count(&events, |v| *v == VideoEvent::Started(e)), 1);
    // The renderer's material now samples the player's texture.
    let tex = vp.texture(e).unwrap();
    let r = (*s.world.get::<&MeshRenderer>(e).unwrap()).clone();
    assert_eq!(
        r.overrides_at(0).unwrap().main_texture.as_deref(),
        Some(tex.as_str())
    );
    assert!(textures::get(&tex).is_some());
    assert_eq!(
        s.world.get::<&VideoPlayer>(e).unwrap().runtime_texture,
        Some(tex.clone())
    );
    // Audio went through the Audio Source with the clip's track.
    assert!(
        log.lock()
            .unwrap()
            .played
            .iter()
            .any(|p| p.ends_with("clip.avi#audio"))
    );
    // Play 2.5 loops of a 2 s clip in 50 ms steps.
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(1));
        events.extend(vp.update(&mut s, &mut audio, 0.05));
    }
    let loops = count(&events, |v| *v == VideoEvent::LoopPointReached(e));
    assert_eq!(loops, 2, "{events:?}");
    assert!((vp.time(e) - 1.0).abs() < 1e-6, "time {}", vp.time(e));
    assert!(vp.is_playing(e));
    assert_eq!(count(&events, |v| *v == VideoEvent::Started(e)), 1);
    vp.stop(&mut s, &mut audio, e);
    assert!(!vp.is_playing(e));
    assert!(textures::get(&tex).is_none());
    let r = (*s.world.get::<&MeshRenderer>(e).unwrap()).clone();
    assert!(r.overrides_at(0).is_none(), "override removed");
}

#[test]
fn non_looping_clip_stops_at_its_end() {
    let (dir, mut s, e) = setup("end", false);
    let (mut audio, _) = Audio::null(dir.0.clone());
    let mut vp = VideoPlayback::new(dir.0.clone());
    vp.play(&mut s, &mut audio, e);
    vp.wait_prepared(Duration::from_secs(10));
    let mut events = vp.update(&mut s, &mut audio, 0.0);
    for _ in 0..60 {
        events.extend(vp.update(&mut s, &mut audio, 0.05));
    }
    assert_eq!(count(&events, |v| *v == VideoEvent::LoopPointReached(e)), 1);
    assert!(!vp.is_playing(e));
    assert!((vp.time(e) - 2.0).abs() < 1e-9);
    // Play again restarts from the beginning.
    let again = vp.play(&mut s, &mut audio, e);
    assert!(again.contains(&VideoEvent::Started(e)));
    assert_eq!(vp.time(e), 0.0);
}

#[test]
fn seeking_shows_the_requested_frame() {
    let (dir, mut s, e) = setup("seek", false);
    let (mut audio, _) = Audio::null(dir.0.clone());
    let mut vp = VideoPlayback::new(dir.0.clone());
    vp.play(&mut s, &mut audio, e);
    vp.wait_prepared(Duration::from_secs(10));
    vp.update(&mut s, &mut audio, 0.0);
    vp.pause(&mut audio, e);
    assert!(vp.is_paused(e));
    vp.set_frame(&mut audio, e, 25);
    assert!((vp.time(e) - 1.25).abs() < 1e-9);
    // Paused: the clock holds; resume and present the frame due.
    vp.play(&mut s, &mut audio, e);
    let tex = vp.texture(e).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while vp.frame(e) != 25 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
        vp.update(&mut s, &mut audio, 0.0);
    }
    assert_eq!(vp.frame(e), 25);
    let f = textures::get(&tex).unwrap();
    let img = image::RgbaImage::from_raw(f.width, f.height, (*f.pixels).clone()).unwrap();
    assert_eq!(small().read_id(&img), Some(25));
    vp.set_time(&mut audio, e, 99.0);
    assert!((vp.time(e) - 2.0).abs() < 1e-9, "clamped to the length");
}

#[test]
fn audio_voice_follows_video_time() {
    let (dir, mut s, e) = setup("av", false);
    s.world.get::<&mut VideoPlayer>(e).unwrap().audio_output = VideoAudioOutput::Direct;
    let (mut audio, log) = Audio::null(dir.0.clone());
    let mut vp = VideoPlayback::new(dir.0.clone());
    vp.play(&mut s, &mut audio, e);
    vp.wait_prepared(Duration::from_secs(10));
    vp.update(&mut s, &mut audio, 0.0);
    assert_eq!(vp.audio_time(&audio, e), Some(0.0));
    vp.set_time(&mut audio, e, 1.5);
    // The silent backend's voice position is the last seek: equal to the video clock.
    let a = vp.audio_time(&audio, e).unwrap();
    assert!(
        (a - vp.time(e)).abs() < 1.0 / 20.0,
        "audio {a} vs video {}",
        vp.time(e)
    );
    vp.pause(&mut audio, e);
    let paused = log.lock().unwrap().voices.values().all(|v| v.paused);
    assert!(paused);
}

#[test]
fn render_texture_players_publish_under_their_target_name() {
    let (dir, mut s, e) = setup("rt", false);
    {
        let mut v = s.world.get::<&mut VideoPlayer>(e).unwrap();
        v.render_mode = VideoRenderMode::RenderTexture;
        v.target_texture = "VideoTestTarget".into();
    }
    let (mut audio, _) = Audio::null(dir.0.clone());
    let mut vp = VideoPlayback::new(dir.0.clone());
    vp.play(&mut s, &mut audio, e);
    vp.wait_prepared(Duration::from_secs(10));
    vp.update(&mut s, &mut audio, 0.0);
    assert_eq!(vp.texture(e).as_deref(), Some("runtime://VideoTestTarget"));
    let f = textures::get("runtime://VideoTestTarget").expect("frame published");
    let img = image::RgbaImage::from_raw(f.width, f.height, (*f.pixels).clone()).unwrap();
    assert_eq!(small().read_id(&img), Some(0));
    let out = s.world.get::<&VideoPlayer>(e).unwrap().output_texture();
    assert_eq!(out.as_deref(), Some("runtime://VideoTestTarget"));
    let r = (*s.world.get::<&MeshRenderer>(e).unwrap()).clone();
    assert!(r.overrides_at(0).is_none(), "no material override");
    vp.stop(&mut s, &mut audio, e);
    assert!(textures::get("runtime://VideoTestTarget").is_none());
}

#[test]
fn missing_clips_report_an_error_event() {
    let (dir, mut s, e) = setup("missing", false);
    s.world.get::<&mut VideoPlayer>(e).unwrap().clip = Some("Assets/none.avi".into());
    let (mut audio, _) = Audio::null(dir.0.clone());
    let mut vp = VideoPlayback::new(dir.0.clone());
    let ev = vp.play(&mut s, &mut audio, e);
    assert!(matches!(ev.as_slice(), [VideoEvent::Error(x, _)] if *x == e));
    assert!(!vp.take_logs().is_empty());
}

#[test]
fn aspect_ratio_modes_fit_the_screen() {
    use glam::Vec2;
    let v = Vec2::new(320.0, 180.0);
    let s = Vec2::new(400.0, 400.0);
    assert_eq!(
        VideoAspectRatio::FitInside.fit(v, s),
        Vec2::new(400.0, 225.0)
    );
    let out = VideoAspectRatio::FitOutside.fit(v, s);
    assert!(
        (out.x - 711.111).abs() < 0.01 && (out.y - 400.0).abs() < 0.01,
        "{out}"
    );
    assert_eq!(VideoAspectRatio::Stretch.fit(v, s), s);
    assert_eq!(VideoAspectRatio::NoScaling.fit(v, s), v);
    assert!((VideoAspectRatio::FitHorizontally.fit(v, s).x - 400.0).abs() < 1e-3);
    assert!((VideoAspectRatio::FitVertically.fit(v, s).y - 400.0).abs() < 1e-3);
}
