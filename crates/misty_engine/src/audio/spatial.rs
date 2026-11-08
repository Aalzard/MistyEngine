//! Distance attenuation, stereo panning and binaural direction for 3D sounds (pure math, no
//! audio device).

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

use crate::scene::{AudioRolloff, AudioSource};

/// Binaural rendering of a voice: the source direction in listener space (x right, y up,
/// z forward) and how much of the voice is rendered binaurally (its spatial blend).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HrtfParams {
    pub dir: [f32; 3],
    pub mix: f32,
}

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
    /// Spatialized 3D sources: rendered with the head model instead of panning.
    pub binaural: Option<HrtfParams>,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            looping: false,
            binaural: None,
        }
    }
}

/// A custom rolloff: (distance as a share of Max Distance, volume) keys, both 0..1, joined by a
/// smooth curve that never overshoots between keys (monotone cubic), like an animation curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RolloffCurve {
    pub keys: Vec<[f32; 2]>,
}

impl Default for RolloffCurve {
    fn default() -> Self {
        Self {
            keys: vec![[0.0, 1.0], [0.2, 0.5], [0.5, 0.15], [1.0, 0.0]],
        }
    }
}

impl RolloffCurve {
    /// Keys sorted by distance and clamped to 0..1.
    pub fn sanitize(&mut self) {
        for k in &mut self.keys {
            k[0] = k[0].clamp(0.0, 1.0);
            k[1] = k[1].clamp(0.0, 1.0);
        }
        self.keys.sort_by(|a, b| a[0].total_cmp(&b[0]));
    }

    /// Volume at `x` (share of Max Distance); flat before the first and after the last key.
    pub fn evaluate(&self, x: f32) -> f32 {
        let k = &self.keys;
        match k.len() {
            0 => return 1.0,
            1 => return k[0][1],
            _ => {}
        }
        if x <= k[0][0] {
            return k[0][1];
        }
        if x >= k[k.len() - 1][0] {
            return k[k.len() - 1][1];
        }
        let i = k.partition_point(|p| p[0] <= x).clamp(1, k.len() - 1) - 1;
        let (x0, y0, x1, y1) = (k[i][0], k[i][1], k[i + 1][0], k[i + 1][1]);
        let h = (x1 - x0).max(1e-6);
        let slope = |a: usize| (k[a + 1][1] - k[a][1]) / (k[a + 1][0] - k[a][0]).max(1e-6);
        // Fritsch-Carlson tangents: zero at extrema, so the curve stays between its keys.
        let tangent = |j: usize| -> f32 {
            if j == 0 {
                slope(0)
            } else if j == k.len() - 1 {
                slope(j - 1)
            } else {
                let (a, b) = (slope(j - 1), slope(j));
                if a * b <= 0.0 {
                    0.0
                } else {
                    2.0 / (1.0 / a + 1.0 / b)
                }
            }
        };
        let (m0, m1) = (tangent(i) * h, tangent(i + 1) * h);
        let t = (x - x0) / h;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * m0
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * m1;
        y.clamp(y0.min(y1), y0.max(y1))
    }
}

/// Gain (0..1) of `source` at `distance`, with its own custom curve when it has one.
pub fn distance_gain(source: &AudioSource, distance: f32) -> f32 {
    match source.rolloff {
        AudioRolloff::Custom => {
            let max = source.max_distance.max(1e-4);
            source.custom_rolloff.evaluate(distance / max)
        }
        r => rolloff_gain(r, source.min_distance, source.max_distance, distance),
    }
}

/// Gain (0..1) at `distance` from a 3D source (Custom: the default custom curve).
pub fn rolloff_gain(rolloff: AudioRolloff, min: f32, max: f32, distance: f32) -> f32 {
    let min = min.max(0.0);
    let max = max.max(min + 1e-4);
    if rolloff == AudioRolloff::Custom {
        return RolloffCurve::default().evaluate(distance / max);
    }
    if distance <= min {
        return 1.0;
    }
    match rolloff {
        AudioRolloff::Custom => 1.0,
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

/// Spread (0..360°) narrows the 3D pan: a point source at 0°, centred at 180°, mirrored at 360°.
pub fn spread_factor(spread: f32) -> f32 {
    (spread.clamp(0.0, 360.0).to_radians() * 0.5).cos()
}

/// Speed of sound used for the Doppler effect (m/s).
pub const SPEED_OF_SOUND: f32 = 343.0;

/// Pitch factor from motion: `listener` and `source` positions with their velocities, scaled
/// by the source's Doppler Level (0 = off). Clamped to half / double speed.
pub fn doppler(
    listener: Vec3,
    listener_vel: Vec3,
    source: Vec3,
    source_vel: Vec3,
    level: f32,
) -> f32 {
    let dir = (source - listener).normalize_or_zero();
    if dir == Vec3::ZERO || level <= 0.0 {
        return 1.0;
    }
    let c = SPEED_OF_SOUND;
    // Listener moving towards the source and source moving away both count along `dir`.
    let towards = listener_vel.dot(dir).clamp(-c * 0.5, c * 0.5);
    let away = source_vel.dot(dir).clamp(-c * 0.5, c * 0.5);
    let factor = (c + towards) / (c + away);
    (1.0 + (factor - 1.0) * level).clamp(0.5, 2.0)
}

/// Direction from a listener with world matrix `listener` to `source`, in listener space
/// (x right, y up, z forward), unit length (zero when they coincide).
pub fn local_direction(listener: Mat4, source: Vec3) -> Vec3 {
    let (_, rotation, position) = listener.to_scale_rotation_translation();
    (rotation.inverse() * (source - position)).normalize_or_zero()
}

/// Mix an Audio Source at world position `at` for a listener (`None` = no listener: only 2D).
pub fn mix(source: &AudioSource, at: Vec3, listener: Option<Mat4>, muted: bool) -> VoiceParams {
    let blend = source.spatial_blend.clamp(0.0, 1.0);
    let spread = spread_factor(source.spread);
    let (gain3d, pan3d) = match listener {
        Some(l) => {
            let distance = at.distance(l.w_axis.truncate());
            (distance_gain(source, distance), pan(l, at) * spread)
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
    // Spatialize: the 3D share goes through the head model; spread narrows its sideways cues
    // just as it narrows the pan.
    let binaural = match listener {
        Some(l) if source.spatialize && blend > 0.0 => {
            // The sideways share removed by spread moves to the front (or back), so the
            // direction stays a unit vector and a full spread sits straight ahead.
            let d = local_direction(l, at);
            let x = d.x * spread;
            let rest = (1.0 - x * x - d.y * d.y).max(0.0).sqrt();
            let z = if d.z < 0.0 { -rest } else { rest };
            Some(HrtfParams {
                dir: [x, d.y, z],
                mix: blend,
            })
        }
        _ => None,
    };
    let pan = if binaural.is_some() {
        pan2d
    } else {
        pan2d * (1.0 - blend) + pan3d * blend
    };
    VoiceParams {
        volume,
        pitch: source.pitch,
        pan: pan.clamp(-1.0, 1.0),
        looping: source.looping,
        binaural,
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
    fn spread_narrows_the_pan() {
        let mut s = AudioSource {
            spatial_blend: 1.0,
            ..Default::default()
        };
        let right = Vec3::new(4.0, 0.0, 0.0);
        let l = Some(Mat4::IDENTITY);
        assert!(close(mix(&s, right, l, false).pan, 1.0));
        s.spread = 180.0;
        assert!(
            close(mix(&s, right, l, false).pan, 0.0),
            "fills both speakers"
        );
        s.spread = 360.0;
        assert!(close(mix(&s, right, l, false).pan, -1.0), "mirrored");
    }

    #[test]
    fn doppler_raises_pitch_when_approaching() {
        let ear = Vec3::ZERO;
        let at = Vec3::new(0.0, 0.0, 10.0);
        let still = doppler(ear, Vec3::ZERO, at, Vec3::ZERO, 1.0);
        assert_eq!(still, 1.0);
        let closing = doppler(ear, Vec3::ZERO, at, Vec3::new(0.0, 0.0, -34.3), 1.0);
        assert!((closing - 343.0 / 308.7).abs() < 1e-3, "{closing}");
        let leaving = doppler(ear, Vec3::ZERO, at, Vec3::new(0.0, 0.0, 34.3), 1.0);
        assert!(leaving < 1.0);
        let walking_up = doppler(ear, Vec3::new(0.0, 0.0, 34.3), at, Vec3::ZERO, 1.0);
        assert!((walking_up - 1.1).abs() < 1e-4);
        assert_eq!(
            doppler(ear, Vec3::ZERO, at, Vec3::new(0.0, 0.0, -34.3), 0.0),
            1.0
        );
        let half = doppler(ear, Vec3::ZERO, at, Vec3::new(0.0, 0.0, -34.3), 0.5);
        assert!((half - 1.0 - (closing - 1.0) * 0.5).abs() < 1e-5);
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
