//! Whole scenes rendered offline through [`Audio`]: nested mixers, binaural sources, spread,
//! Doppler, custom rolloff curves and mixer edits while playing.

use std::f32::consts::TAU;

use glam::Vec3;

use super::dsp::Frame;
use super::dsp::analysis::{amplitude_at, delay_between, dominant_frequency, max_step, rms, side};
use super::mixer::{MASTER, pitch_key, volume_key};
use super::spatial::RolloffCurve;
use super::*;
use crate::scene::{AudioRolloff, Transform};

const SR: f32 = 48000.0;
/// The default scene's listener (Main Camera), facing +Z.
const EAR: Vec3 = Vec3::new(0.0, 1.0, -10.0);

fn project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_render_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Audio")).unwrap();
    let tone = wav::synth(1.0, SR as u32, |t| 0.5 * (TAU * 500.0 * t).sin());
    std::fs::write(root.join("Assets/Audio/Tone.wav"), tone).unwrap();
    let mut s = 1u32;
    let noise = wav::synth(1.0, SR as u32, |_| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        0.4 * (s as f32 / u32::MAX as f32 * 2.0 - 1.0)
    });
    std::fs::write(root.join("Assets/Audio/Noise.wav"), noise).unwrap();
    root
}

fn source(clip: &str) -> AudioSource {
    AudioSource {
        clip: Some(format!("Assets/Audio/{clip}.wav")),
        looping: true,
        ..Default::default()
    }
}

fn spawn(scene: &mut Scene, src: AudioSource, at: Vec3) -> Entity {
    let e = scene.spawn("Speaker", None);
    scene.add(e, src);
    scene.set_transform(e, Transform::from_position(at));
    e
}

/// Play every source, then render `seconds` in 60 Hz frames (ticking and updating like the
/// runtime does).
fn run(
    audio: &mut Audio,
    scene: &mut Scene,
    seconds: f32,
    mut each: impl FnMut(&mut Scene, f32),
) -> Vec<Frame> {
    audio.play_on_awake(scene, None);
    let frame = SR as usize / 60;
    let mut out = Vec::new();
    let mut t = 0.0;
    while out.len() < (seconds * SR) as usize {
        each(scene, t);
        audio.tick(1.0 / 60.0);
        audio.update(scene);
        out.extend(audio.render(frame).unwrap());
        t += 1.0 / 60.0;
    }
    out
}

#[test]
fn nested_mixer_plays_through_the_other_mixers_group() {
    let root = project("nested");
    let mut main = AudioMixer::default();
    let music = main.add_group(MASTER, "Music");
    main.set_value(0, &volume_key(music), -6.0206);
    main.set_value(0, &pitch_key(music), 2.0);
    main.save(&root.join("Assets/Audio/Main.mixer")).unwrap();
    let sub = AudioMixer {
        output: Some(output_ref("Assets/Audio/Main.mixer", "Music")),
        ..Default::default()
    };
    sub.save(&root.join("Assets/Audio/Sub.mixer")).unwrap();
    let mut scene = Scene::default_3d("t");
    spawn(
        &mut scene,
        AudioSource {
            output: Some("Assets/Audio/Sub.mixer".into()),
            ..source("Tone")
        },
        EAR,
    );
    let mut audio = Audio::offline(root.clone(), SR);
    let out = run(&mut audio, &mut scene, 0.5, |_, _| {});
    let x = side(&out[4800..], false);
    // Music's -6 dB fader halves it; Music's pitch (×2) carries down into the nested mixer.
    assert!(
        (amplitude_at(&x, 1000.0, SR) - 0.25).abs() < 0.01,
        "{}",
        amplitude_at(&x, 1000.0, SR)
    );
    assert!((dominant_frequency(&x, SR, 300.0, 1500.0) - 1000.0).abs() < 2.0);
    let sub_i = audio
        .mixers
        .iter()
        .position(|m| m.path.ends_with("Sub.mixer"))
        .unwrap();
    let main_i = audio
        .mixers
        .iter()
        .position(|m| m.path.ends_with("Main.mixer"))
        .unwrap();
    assert_eq!(audio.mixers[sub_i].output_group, Some((main_i, music)));
    assert!(audio.take_logs().is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn nested_mixer_loops_are_refused() {
    let root = project("loop");
    for (name, other) in [("A", "B"), ("B", "A")] {
        let m = AudioMixer {
            output: Some(output_ref(&format!("Assets/Audio/{other}.mixer"), "Master")),
            ..Default::default()
        };
        m.save(&root.join(format!("Assets/Audio/{name}.mixer")))
            .unwrap();
    }
    let mut scene = Scene::default_3d("t");
    spawn(
        &mut scene,
        AudioSource {
            output: Some("Assets/Audio/A.mixer".into()),
            ..source("Tone")
        },
        EAR,
    );
    let mut audio = Audio::offline(root.clone(), SR);
    let out = run(&mut audio, &mut scene, 0.2, |_, _| {});
    assert!(rms(&side(&out, false)) > 0.1, "still heard");
    let logs = audio.take_logs();
    assert!(logs.iter().any(|l| l.contains("would loop")), "{logs:?}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn spatialized_sources_get_time_and_level_differences() {
    let root = project("hrtf");
    let mut scene = Scene::default_3d("t");
    let src = AudioSource {
        spatial_blend: 1.0,
        spatialize: true,
        min_distance: 10.0,
        ..source("Noise")
    };
    // 3 m to the listener's right (inside Min Distance: full volume).
    let e = spawn(&mut scene, src.clone(), EAR + Vec3::new(3.0, 0.0, 0.0));
    let mut audio = Audio::offline(root.clone(), SR);
    let out = run(&mut audio, &mut scene, 0.5, |_, _| {});
    let (l, r) = (side(&out[4800..], false), side(&out[4800..], true));
    let ild = 20.0 * (rms(&r) / rms(&l)).log10();
    assert!(ild > 6.0, "right ear louder by {ild} dB");
    let lag = delay_between(&r, &l, 64);
    let expected = super::dsp::hrtf::itd(Vec3::X) * SR;
    assert!(
        (lag as f32 - expected).abs() <= 1.5,
        "left ear {lag} samples late, expected {expected}"
    );
    // Spread 180° removes the sideways cues (like it centres a panned source).
    scene.world.get::<&mut AudioSource>(e).unwrap().spread = 180.0;
    let out = run(&mut audio, &mut scene, 0.5, |_, _| {});
    let (l, r) = (side(&out[9600..], false), side(&out[9600..], true));
    assert!((rms(&l) / rms(&r) - 1.0).abs() < 0.02, "centred");
    assert_eq!(delay_between(&r, &l, 64), 0);
    // Without Spatialize the same source is panned: level only, no delay.
    {
        let mut s = scene.world.get::<&mut AudioSource>(e).unwrap();
        s.spread = 0.0;
        s.spatialize = false;
    }
    audio.stop_all();
    let out = run(&mut audio, &mut scene, 0.3, |_, _| {});
    let (l, r) = (side(&out[4800..], false), side(&out[4800..], true));
    assert!(rms(&l) < 1e-4 && rms(&r) > 0.1, "hard right");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn doppler_shifts_the_rendered_pitch() {
    let root = project("doppler");
    let mut scene = Scene::default_3d("t");
    let start = EAR + Vec3::new(0.0, 0.0, 60.0);
    let e = spawn(
        &mut scene,
        AudioSource {
            spatial_blend: 1.0,
            min_distance: 100.0,
            ..source("Tone")
        },
        start,
    );
    let mut audio = Audio::offline(root.clone(), SR);
    // Approaching at 34.3 m/s: (c) / (c - v) = 343 / 308.7 ≈ 1.111.
    let out = run(&mut audio, &mut scene, 1.0, |scene, t| {
        scene.set_transform(e, Transform::from_position(start - Vec3::Z * 34.3 * t));
    });
    let x = side(&out[(0.3 * SR) as usize..], false);
    let f = dominant_frequency(&x, SR, 300.0, 800.0);
    assert!((f / 500.0 - 343.0 / 308.7).abs() < 0.004, "{f} Hz");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn custom_rolloff_curve_sets_the_volume() {
    let root = project("rolloff");
    let mut scene = Scene::default_3d("t");
    let curve = RolloffCurve {
        keys: vec![[0.0, 1.0], [0.5, 0.25], [1.0, 0.0]],
    };
    spawn(
        &mut scene,
        AudioSource {
            spatial_blend: 1.0,
            rolloff: AudioRolloff::Custom,
            max_distance: 20.0,
            custom_rolloff: curve.clone(),
            ..source("Tone")
        },
        EAR + Vec3::new(0.0, 0.0, 10.0),
    );
    let mut audio = Audio::offline(root.clone(), SR);
    let out = run(&mut audio, &mut scene, 0.3, |_, _| {});
    let a = amplitude_at(&side(&out[4800..], false), 500.0, SR);
    assert!((a - 0.5 * 0.25).abs() < 0.003, "half of Max Distance: {a}");
    // The curve is smooth and stays between its keys.
    let mut last = 1.0;
    for i in 0..=100 {
        let v = curve.evaluate(i as f32 / 100.0);
        assert!(
            v <= last + 1e-6 && (0.0..=1.0).contains(&v),
            "monotone: {v} after {last}"
        );
        last = v;
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn mixer_edits_while_playing_render_without_clicks() {
    let root = project("live");
    let mut m = AudioMixer::default();
    let music = m.add_group(MASTER, "Music");
    m.save(&root.join("Assets/Audio/Main.mixer")).unwrap();
    let mut scene = Scene::default_3d("t");
    spawn(
        &mut scene,
        AudioSource {
            output: Some(output_ref("Assets/Audio/Main.mixer", "Music")),
            ..source("Tone")
        },
        EAR,
    );
    let mut audio = Audio::offline(root.clone(), SR);
    let mut out = run(&mut audio, &mut scene, 0.2, |_, _| {});
    let edit = |audio: &mut Audio, f: &dyn Fn(&mut AudioMixer)| {
        let mut a = audio.mixer(0).unwrap().asset.clone();
        f(&mut a);
        audio.replace_mixer("Assets/Audio/Main.mixer", a);
    };
    edit(&mut audio, &|a| {
        let lp = a.add_effect(music, EffectKind::Lowpass).unwrap();
        a.set_value(0, &mixer::effect_key(music, lp, "Cutoff freq"), 400.0);
    });
    out.extend(run(&mut audio, &mut scene, 0.2, |_, _| {}));
    edit(&mut audio, &|a| {
        a.add_effect(music, EffectKind::Chorus);
    });
    out.extend(run(&mut audio, &mut scene, 0.2, |_, _| {}));
    edit(&mut audio, &|a| {
        let bus = a.add_group(MASTER, "Bus");
        a.reparent(music, bus);
    });
    out.extend(run(&mut audio, &mut scene, 0.2, |_, _| {}));
    edit(&mut audio, &|a| {
        let fx: Vec<u32> = a
            .group(music)
            .unwrap()
            .effects
            .iter()
            .map(|e| e.id)
            .collect();
        a.move_effect_to(music, fx[1], 0);
    });
    out.extend(run(&mut audio, &mut scene, 0.2, |_, _| {}));
    edit(&mut audio, &|a| a.remove_group(music));
    out.extend(run(&mut audio, &mut scene, 0.2, |_, _| {}));
    let worst = max_step(&side(&out, false));
    assert!(worst < 0.08, "largest jump {worst}");
    assert_eq!(audio.voice_count(), 1, "the source never restarted");
    let tail = side(&out[out.len() - 4800..], false);
    assert!(
        (amplitude_at(&tail, 500.0, SR) - 0.5).abs() < 0.01,
        "back to the dry tone on Bus"
    );
    std::fs::remove_dir_all(root).ok();
}
