//! Distance attenuation and stereo panning for 3D sounds (pure math, no audio device).

use glam::{Mat4, Vec3};

use crate::scene::{AudioRolloff, AudioSource};

/// Volume, pitch and pan a voice should play with this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    /// Linear amplitude (1 = unchanged).
    pub volume: f32,
    /// Playback speed (1 = normal).
    pub pitch: f32,
    /// -1 (left) .. 1 (right).
    pub pan: f32,
    pub looping: bool,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            looping: false,
        }
    }
}

/// Gain (0..1) at `distance` from a 3D source.
pub fn rolloff_gain(rolloff: AudioRolloff, min: f32, max: f32, distance: f32) -> f32 {
    let min = min.max(0.0);
    let max = max.max(min + 1e-4);
    if distance <= min {
        return 1.0;
    }
    match rolloff {
        // Holds its level past the max distance instead of cutting off.
        AudioRolloff::Logarithmic => {
            if min <= 0.0 {
                0.0
            } else {
                min / distance.min(max)
            }
        }
        AudioRolloff::Linear => 1.0 - ((distance - min) / (max - min)).clamp(0.0, 1.0),
    }
}

/// Pan (-1..1) of a sound at `source` heard by a listener with world matrix `listener`:
/// the sideways part of the direction to the source.
pub fn pan(listener: Mat4, source: Vec3) -> f32 {
    let (_, rotation, position) = listener.to_scale_rotation_translation();
    let dir = (source - position).normalize_or_zero();
    dir.dot(rotation * Vec3::X).clamp(-1.0, 1.0)
}

/// Mix an Audio Source at world position `at` for a listener (`None` = no listener: only 2D).
pub fn mix(source: &AudioSource, at: Vec3, listener: Option<Mat4>, muted: bool) -> VoiceParams {
    let blend = source.spatial_blend.clamp(0.0, 1.0);
    let (gain3d, pan3d) = match listener {
        Some(l) => {
            let distance = at.distance(l.w_axis.truncate());
            let gain = rolloff_gain(
                source.rolloff,
                source.min_distance,
                source.max_distance,
                distance,
            );
            (gain, pan(l, at))
        }
        None => (1.0, 0.0),
    };
    let gain = (1.0 - blend) + blend * gain3d;
    let volume = if muted || source.mute {
        0.0
    } else {
        source.volume.clamp(0.0, 1.0) * gain
    };
    let pan2d = source.stereo_pan.clamp(-1.0, 1.0);
    VoiceParams {
        volume,
        pitch: source.pitch,
        pan: (pan2d * (1.0 - blend) + pan3d * blend).clamp(-1.0, 1.0),
        looping: source.looping,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn logarithmic_rolloff_is_inverse_distance_and_holds_past_max() {
        let r = AudioRolloff::Logarithmic;
        assert_eq!(rolloff_gain(r, 1.0, 500.0, 0.5), 1.0);
        assert!(close(rolloff_gain(r, 1.0, 500.0, 2.0), 0.5));
        assert!(close(rolloff_gain(r, 2.0, 500.0, 8.0), 0.25));
        assert!(close(rolloff_gain(r, 1.0, 10.0, 100.0), 0.1));
    }

    #[test]
    fn linear_rolloff_fades_to_silence_at_max() {
        let r = AudioRolloff::Linear;
        assert_eq!(rolloff_gain(r, 1.0, 11.0, 1.0), 1.0);
        assert!(close(rolloff_gain(r, 1.0, 11.0, 6.0), 0.5));
        assert_eq!(rolloff_gain(r, 1.0, 11.0, 50.0), 0.0);
    }

    #[test]
    fn pan_follows_listener_orientation() {
        let at_origin = Mat4::IDENTITY;
        assert!(close(pan(at_origin, Vec3::new(5.0, 0.0, 0.0)), 1.0));
        assert!(close(pan(at_origin, Vec3::new(-5.0, 0.0, 0.0)), -1.0));
        assert!(close(pan(at_origin, Vec3::new(0.0, 0.0, 5.0)), 0.0));
        // Listener turned 180° around Y: its right is world -X.
        let turned = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::PI),
            Vec3::ZERO,
        );
        assert!(close(pan(turned, Vec3::new(5.0, 0.0, 0.0)), -1.0));
        assert_eq!(
            pan(at_origin, Vec3::ZERO),
            0.0,
            "source on the listener is centred"
        );
    }

    #[test]
    fn spatial_blend_mixes_2d_and_3d() {
        let listener = Some(Mat4::IDENTITY);
        let far_right = Vec3::new(4.0, 0.0, 0.0);
        let mut s = AudioSource {
            volume: 0.8,
            ..Default::default()
        };
        let flat = mix(&s, far_right, listener, false);
        assert!(
            close(flat.volume, 0.8) && close(flat.pan, 0.0),
            "2D ignores position"
        );
        s.spatial_blend = 1.0;
        let full = mix(&s, far_right, listener, false);
        assert!(close(full.volume, 0.2) && close(full.pan, 1.0));
        s.spatial_blend = 0.5;
        let half = mix(&s, far_right, listener, false);
        assert!(close(half.volume, 0.8 * (0.5 + 0.5 * 0.25)) && close(half.pan, 0.5));
        assert_eq!(
            mix(&s, far_right, None, false).volume,
            0.8,
            "no listener = 2D"
        );
    }

    #[test]
    fn mute_silences() {
        let s = AudioSource {
            mute: true,
            ..Default::default()
        };
        assert_eq!(mix(&s, Vec3::ZERO, None, false).volume, 0.0);
        let s = AudioSource::default();
        assert_eq!(mix(&s, Vec3::ZERO, None, true).volume, 0.0, "Mute Audio");
    }
}
