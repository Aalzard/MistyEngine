//! Our effect descriptions on kira's built-in effects, plus a peak meter effect for VU meters.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use kira::effect::compressor::{CompressorBuilder, CompressorHandle};
use kira::effect::delay::{DelayBuilder, DelayHandle};
use kira::effect::distortion::{DistortionBuilder, DistortionHandle, DistortionKind};
use kira::effect::filter::{FilterBuilder, FilterHandle, FilterMode};
use kira::effect::reverb::{ReverbBuilder, ReverbHandle};
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;
use kira::track::TrackBuilder;
use kira::{Decibels, Frame, Mix, Tween};

use super::effects::{EffectKind, EffectSpec, amplitude_to_db};

/// Handle of one built effect, for live parameter changes.
pub enum FxHandle {
    Filter(FilterHandle),
    Delay(DelayHandle),
    Reverb(ReverbHandle),
    Distortion(DistortionHandle),
    Compressor(CompressorHandle),
}

/// Quality factor (1..10) to kira's resonance (0..1, self-oscillating near 1).
fn resonance(q: f32) -> f64 {
    ((q.max(1.0) - 1.0) / q.max(1.0) * 0.9) as f64
}

fn mix(spec: &EffectSpec, wet: f32) -> Mix {
    if spec.bypass { Mix::DRY } else { Mix(wet) }
}

/// Echo: wet share of the dry + wet levels.
fn echo_mix(spec: &EffectSpec) -> f32 {
    let (dry, wet) = (spec.value(2), spec.value(3));
    if dry + wet <= 0.0 {
        0.0
    } else {
        wet / (dry + wet)
    }
}

fn ms(v: f32) -> Duration {
    Duration::from_secs_f32(v.max(0.0) / 1000.0)
}

/// Add `spec` to a track being built.
pub fn add(track: &mut TrackBuilder, spec: &EffectSpec) -> FxHandle {
    let v = |i| spec.value(i);
    match spec.kind {
        EffectKind::Lowpass | EffectKind::Highpass => {
            let mode = if spec.kind == EffectKind::Lowpass {
                FilterMode::LowPass
            } else {
                FilterMode::HighPass
            };
            FxHandle::Filter(
                track.add_effect(
                    FilterBuilder::new()
                        .mode(mode)
                        .cutoff(v(0) as f64)
                        .resonance(resonance(v(1)))
                        .mix(mix(spec, 1.0)),
                ),
            )
        }
        // The delay time is fixed when the track is built (kira can't change it live).
        EffectKind::Echo => FxHandle::Delay(
            track.add_effect(
                DelayBuilder::new()
                    .delay_time(ms(v(0)))
                    .feedback(Decibels(amplitude_to_db(v(1) / 100.0)))
                    .mix(mix(spec, echo_mix(spec))),
            ),
        ),
        EffectKind::Reverb => FxHandle::Reverb(
            track.add_effect(
                ReverbBuilder::new()
                    .feedback(v(0) as f64)
                    .damping(v(1) as f64)
                    .stereo_width(v(2) as f64)
                    .mix(mix(spec, v(3) / 100.0)),
            ),
        ),
        EffectKind::Distortion => FxHandle::Distortion(
            track.add_effect(
                DistortionBuilder::new()
                    .kind(DistortionKind::SoftClip)
                    .drive(Decibels(v(0) * 36.0))
                    .mix(mix(spec, 1.0)),
            ),
        ),
        EffectKind::Compressor => FxHandle::Compressor(
            track.add_effect(
                CompressorBuilder::new()
                    .threshold(v(0) as f64)
                    .ratio(v(1) as f64)
                    .attack_duration(ms(v(2)))
                    .release_duration(ms(v(3)))
                    .makeup_gain(Decibels(v(4)))
                    .mix(mix(spec, 1.0)),
            ),
        ),
    }
}

/// Push `spec`'s current values to a built effect.
pub fn set(handle: &mut FxHandle, spec: &EffectSpec) {
    let t = Tween::default();
    let v = |i| spec.value(i);
    match handle {
        FxHandle::Filter(h) => {
            h.set_cutoff(v(0) as f64, t);
            h.set_resonance(resonance(v(1)), t);
            h.set_mix(mix(spec, 1.0), t);
        }
        FxHandle::Delay(h) => {
            h.set_feedback(Decibels(amplitude_to_db(v(1) / 100.0)), t);
            h.set_mix(mix(spec, echo_mix(spec)), t);
        }
        FxHandle::Reverb(h) => {
            h.set_feedback(v(0) as f64, t);
            h.set_damping(v(1) as f64, t);
            h.set_stereo_width(v(2) as f64, t);
            h.set_mix(mix(spec, v(3) / 100.0), t);
        }
        FxHandle::Distortion(h) => {
            h.set_drive(Decibels(v(0) * 36.0), t);
            h.set_mix(mix(spec, 1.0), t);
        }
        FxHandle::Compressor(h) => {
            h.set_threshold(v(0) as f64, t);
            h.set_ratio(v(1) as f64, t);
            h.set_attack_duration(ms(v(2)), t);
            h.set_release_duration(ms(v(3)), t);
            h.set_makeup_gain(Decibels(v(4)), t);
            h.set_mix(mix(spec, 1.0), t);
        }
    }
}

/// Last effect of every track: records the loudest sample since the meter was last read.
pub struct PeakMeter(pub Arc<AtomicU32>);

impl EffectBuilder for PeakMeter {
    type Handle = ();

    fn build(self) -> (Box<dyn Effect>, ()) {
        (Box::new(self), ())
    }
}

impl Effect for PeakMeter {
    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        let peak = input
            .iter()
            .map(|f| f.left.abs().max(f.right.abs()))
            .fold(0.0f32, f32::max);
        // Non-negative floats order like their bit patterns.
        self.0.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }
}

/// Read and reset a meter.
pub fn take_peak(meter: &AtomicU32) -> f32 {
    f32::from_bits(meter.swap(0, Ordering::Relaxed))
}
