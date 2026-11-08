//! Each effect rendered offline and measured against what it should do.

use super::super::analysis::{amplitude_at, dominant_frequency, peak, side, sine};
use super::*;
use crate::audio::effects::EffectKind;

const SR: f32 = 48000.0;

/// Run `input` through a fresh `kind` with `values`, in graph-sized blocks.
fn render(kind: EffectKind, values: &[f32], input: &[Frame]) -> Vec<Frame> {
    let mut p = Processor::new(kind, SR);
    let mut out = input.to_vec();
    for block in out.chunks_mut(super::super::BLOCK) {
        p.process(kind, values, block, SR, None);
    }
    out
}

/// Amplitude of `freq` in the left side, after the first `skip` seconds settle.
fn amp(frames: &[Frame], freq: f32, skip: f32) -> f32 {
    let x = side(&frames[(skip * SR) as usize..], false);
    amplitude_at(&x, freq, SR)
}

fn db(a: f32) -> f32 {
    20.0 * a.log10()
}

#[test]
fn param_eq_band_gain_at_its_frequency() {
    for gain in [2.0, 0.5, 3.0] {
        let values = [1000.0, 1.0, gain];
        let at = amp(
            &render(EffectKind::ParamEQ, &values, &sine(1000.0, 0.25, 0.5, SR)),
            1000.0,
            0.1,
        );
        assert!(
            (at / 0.25 - gain).abs() < gain * 0.02,
            "gain {gain}: {} at the centre",
            at / 0.25
        );
        let far = amp(
            &render(EffectKind::ParamEQ, &values, &sine(60.0, 0.25, 0.5, SR)),
            60.0,
            0.1,
        );
        assert!(
            (far / 0.25 - 1.0).abs() < 0.05,
            "{} four octaves away",
            far / 0.25
        );
    }
    // Half the gain (in dB) at the band edges, half an octave either side of the centre.
    let edge = response(
        EffectKind::ParamEQ,
        &[1000.0, 1.0, 4.0],
        1000.0 * 2f32.sqrt(),
        SR,
    );
    assert!(
        (db(edge) - db(2.0)).abs() < 0.8,
        "{} dB at the band edge",
        db(edge)
    );
}

#[test]
fn lowpass_and_highpass_corners() {
    let lp = [2000.0, 1.0];
    let at = amp(
        &render(EffectKind::Lowpass, &lp, &sine(2000.0, 0.5, 0.3, SR)),
        2000.0,
        0.05,
    );
    assert!(
        (db(at / 0.5) + 3.0).abs() < 0.3,
        "{} dB at the corner",
        db(at / 0.5)
    );
    let above = amp(
        &render(EffectKind::Lowpass, &lp, &sine(20000.0, 0.5, 0.3, SR)),
        20000.0,
        0.05,
    );
    assert!(
        db(above / 0.5) < -36.0,
        "12 dB/octave: {} dB",
        db(above / 0.5)
    );
    let below = amp(
        &render(EffectKind::Lowpass, &lp, &sine(200.0, 0.5, 0.3, SR)),
        200.0,
        0.05,
    );
    assert!((below / 0.5 - 1.0).abs() < 0.01);
    let hp = [500.0, 1.0];
    let at = amp(
        &render(EffectKind::Highpass, &hp, &sine(500.0, 0.5, 0.3, SR)),
        500.0,
        0.05,
    );
    assert!((db(at / 0.5) + 3.0).abs() < 0.3);
    let low = amp(
        &render(EffectKind::Highpass, &hp, &sine(50.0, 0.5, 0.5, SR)),
        50.0,
        0.2,
    );
    assert!(db(low / 0.5) < -36.0, "{}", db(low / 0.5));
    // More resonance: a peak at the corner.
    let q = amp(
        &render(
            EffectKind::Lowpass,
            &[2000.0, 4.0],
            &sine(2000.0, 0.1, 0.3, SR),
        ),
        2000.0,
        0.05,
    );
    assert!(q / 0.1 > 2.5, "resonance 4: {}", q / 0.1);
}

#[test]
fn compressor_reduces_gain_above_the_threshold_by_the_ratio() {
    // -6 dBFS sine, threshold -20 dB, 4:1: the output settles near -20 + 14 / 4 = -16.5 dBFS.
    let values = [-20.0, 4.0, 5.0, 100.0, 0.0];
    let out = render(EffectKind::Compressor, &values, &sine(440.0, 0.5, 1.0, SR));
    let level = db(amp(&out, 440.0, 0.5));
    assert!((level + 16.5).abs() < 0.6, "{level} dBFS");
    // Make-up gain adds on top; below the threshold nothing changes.
    let up = render(
        EffectKind::Compressor,
        &[-20.0, 4.0, 5.0, 100.0, 6.0],
        &sine(440.0, 0.5, 1.0, SR),
    );
    assert!((db(amp(&up, 440.0, 0.5)) - level - 6.0).abs() < 0.2);
    let quiet = render(EffectKind::Compressor, &values, &sine(440.0, 0.05, 1.0, SR));
    assert!((amp(&quiet, 440.0, 0.5) / 0.05 - 1.0).abs() < 0.01);
    let mut p = Processor::new(EffectKind::Compressor, SR);
    let mut loud = sine(440.0, 0.5, 0.5, SR);
    p.process(EffectKind::Compressor, &values, &mut loud, SR, None);
    assert!(
        (p.reduction_db() + 10.5).abs() < 0.6,
        "{}",
        p.reduction_db()
    );
}

#[test]
fn soft_knee_curve() {
    let p = DynParams {
        threshold_db: -20.0,
        ratio: 4.0,
        attack: 0.0,
        release: 0.0,
        makeup_db: 0.0,
        knee_db: 10.0,
        side_mix: 1.0,
    };
    assert_eq!(p.curve(-30.0), 0.0, "below the knee");
    assert!(
        (p.curve(-5.0) + 11.25).abs() < 1e-4,
        "above: 15 dB over at 4:1"
    );
    let at = p.curve(-20.0);
    assert!(
        at < 0.0 && at > -1.0,
        "inside the knee it bends gently: {at}"
    );
}

#[test]
fn pitch_shifter_ratio_by_frequency_detection() {
    for ratio in [1.5, 0.75, 2.0, 1.0] {
        let out = render(
            EffectKind::PitchShifter,
            &[ratio, 1024.0, 4.0],
            &sine(440.0, 0.5, 1.0, SR),
        );
        let x = side(&out[(0.2 * SR) as usize..], false);
        let f = dominant_frequency(&x, SR, 150.0, 1200.0);
        assert!((f / 440.0 - ratio).abs() < 0.01, "ratio {ratio}: {f} Hz");
    }
}

#[test]
fn echo_repeats_at_the_delay_with_decay() {
    let mut input = vec![Frame::ZERO; (0.5 * SR) as usize];
    input[0] = Frame::from_mono(1.0);
    // 100 ms, decay 50 %, dry 100 %, wet 80 %.
    let out = render(EffectKind::Echo, &[100.0, 50.0, 100.0, 80.0], &input);
    let at = |s: f32| out[(s * SR) as usize].left;
    assert!((out[0].left - 1.0).abs() < 1e-6, "dry");
    assert!((at(0.1) - 0.8).abs() < 0.02, "first echo {}", at(0.1));
    assert!((at(0.2) - 0.4).abs() < 0.02, "second echo {}", at(0.2));
    assert!(out[(0.05 * SR) as usize].left.abs() < 1e-6);
}

#[test]
fn reverb_tail_grows_with_room_size() {
    let mut input = vec![Frame::ZERO; (2.0 * SR) as usize];
    input[0] = Frame::from_mono(1.0);
    let tail = |room: f32| {
        let out = render(EffectKind::Reverb, &[room, 0.3, 1.0, 100.0], &input);
        let late = side(&out[(0.8 * SR) as usize..(1.2 * SR) as usize], false);
        super::super::analysis::rms(&late)
    };
    let (small, big) = (tail(0.2), tail(0.95));
    assert!(small > 0.0 && big > small * 4.0, "{small} vs {big}");
    // Width 1 decorrelates the sides.
    let out = render(EffectKind::Reverb, &[0.8, 0.3, 1.0, 100.0], &input);
    let differ = out[1000..20000]
        .iter()
        .any(|f| (f.left - f.right).abs() > 1e-4);
    assert!(differ);
}

#[test]
fn distortion_adds_odd_harmonics_and_stays_bounded() {
    let out = render(EffectKind::Distortion, &[0.8], &sine(300.0, 0.8, 0.3, SR));
    assert!(peak(&side(&out, false)) <= 1.0);
    let x = side(&out[2000..], false);
    assert!(amplitude_at(&x, 900.0, SR) > 0.1, "third harmonic");
    assert!(
        amplitude_at(&x, 600.0, SR) < 0.01,
        "symmetric clipping: no even ones"
    );
}

#[test]
fn chorus_and_flange_mix_delayed_copies() {
    let mut input = vec![Frame::ZERO; 8000];
    input[0] = Frame::from_mono(1.0);
    // Chorus without modulation: tap 1 on the left at exactly the delay.
    let out = render(
        EffectKind::Chorus,
        &[0.0, 100.0, 0.0, 0.0, 10.0, 0.0, 0.0],
        &input,
    );
    let at = (0.01 * SR) as usize;
    assert!(
        (out[at - 1].left - 1.0).abs() < 1e-3,
        "{}",
        out[at - 1].left
    );
    assert!(out[at - 1].right.abs() < 1e-6, "tap 1 is the left side");
    // Modulated chorus keeps the tone and widens it.
    let tone = render(
        EffectKind::Chorus,
        &[50.0, 50.0, 50.0, 50.0, 20.0, 1.5, 10.0],
        &sine(440.0, 0.3, 0.5, SR),
    );
    let x = side(&tone[4800..], false);
    assert!(
        (dominant_frequency(&x, SR, 200.0, 800.0) - 440.0).abs() < 15.0,
        "still the tone (with vibrato)"
    );
    assert!(
        tone[10000..]
            .iter()
            .any(|f| (f.left - f.right).abs() > 1e-3)
    );
    // Flange: the wet copy arrives within the sweep (≤ 5 ms).
    let out = render(EffectKind::Flange, &[0.0, 100.0, 1.0, 0.1], &input);
    let first = out.iter().position(|f| f.left.abs() > 0.05).unwrap();
    assert!(first <= (0.005 * SR) as usize + 2, "{first}");
}
