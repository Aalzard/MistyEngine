//! Audio Reverb Zone and the per-source filter components (Low Pass, High Pass, Echo).
//! Filters act on the Audio Source of the same object; zones add reverb to sources while the
//! Audio Listener is inside them.

use serde::{Deserialize, Serialize};

use super::SceneComponent;
use crate::audio::effects::{EffectKind, EffectSpec};

/// `skip_serializing_if` helpers for fields added after scenes were saved.
pub mod defaults {
    pub fn is_false(v: &bool) -> bool {
        !*v
    }
    pub fn is_one(v: &f32) -> bool {
        *v == 1.0
    }
    pub fn is_zero(v: &f32) -> bool {
        *v == 0.0
    }
    pub fn is_priority(v: &u16) -> bool {
        *v == 128
    }
    pub fn is_default_curve(v: &crate::audio::spatial::RolloffCurve) -> bool {
        *v == Default::default()
    }
}

/// Room character of a reverb zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReverbPreset {
    #[default]
    Generic,
    Room,
    Bathroom,
    Hallway,
    Cave,
    Arena,
}

impl ReverbPreset {
    pub const ALL: [Self; 6] = [
        Self::Generic,
        Self::Room,
        Self::Bathroom,
        Self::Hallway,
        Self::Cave,
        Self::Arena,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Generic => "Generic",
            Self::Room => "Room",
            Self::Bathroom => "Bathroom",
            Self::Hallway => "Hallway",
            Self::Cave => "Cave",
            Self::Arena => "Arena",
        }
    }

    /// Reverb effect values: room size, damping, width, wet mix %.
    pub fn values(self) -> [f32; 4] {
        match self {
            Self::Generic => [0.6, 0.4, 1.0, 30.0],
            Self::Room => [0.5, 0.6, 0.8, 25.0],
            Self::Bathroom => [0.7, 0.15, 0.6, 45.0],
            Self::Hallway => [0.75, 0.4, 0.5, 40.0],
            Self::Cave => [0.92, 0.3, 1.0, 55.0],
            Self::Arena => [0.88, 0.5, 1.0, 45.0],
        }
    }
}

/// Reverb heard while the listener is within Max Distance (full inside Min Distance).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioReverbZone {
    pub enabled: bool,
    pub min_distance: f32,
    pub max_distance: f32,
    pub preset: ReverbPreset,
}

impl Default for AudioReverbZone {
    fn default() -> Self {
        Self {
            enabled: true,
            min_distance: 10.0,
            max_distance: 15.0,
            preset: ReverbPreset::Generic,
        }
    }
}

impl AudioReverbZone {
    /// 0..1: how much of the zone the listener hears at `distance` from its centre.
    pub fn weight(&self, distance: f32) -> f32 {
        let (min, max) = (
            self.min_distance.max(0.0),
            self.max_distance.max(self.min_distance),
        );
        if distance <= min {
            1.0
        } else if distance >= max {
            0.0
        } else {
            1.0 - (distance - min) / (max - min)
        }
    }
}

/// Cuts frequencies above the cutoff on this object's Audio Source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioLowPassFilter {
    pub enabled: bool,
    pub cutoff_frequency: f32,
    /// Resonance quality factor (1..10).
    pub resonance_q: f32,
}

impl Default for AudioLowPassFilter {
    fn default() -> Self {
        Self {
            enabled: true,
            cutoff_frequency: 5000.0,
            resonance_q: 1.0,
        }
    }
}

/// Cuts frequencies below the cutoff on this object's Audio Source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioHighPassFilter {
    pub enabled: bool,
    pub cutoff_frequency: f32,
    pub resonance_q: f32,
}

impl Default for AudioHighPassFilter {
    fn default() -> Self {
        Self {
            enabled: true,
            cutoff_frequency: 5000.0,
            resonance_q: 1.0,
        }
    }
}

/// Repeats this object's Audio Source after a delay, each echo quieter by the decay ratio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioEchoFilter {
    pub enabled: bool,
    /// Milliseconds (fixed once the sound starts).
    pub delay: f32,
    /// 0..1 volume of each echo relative to the previous one.
    pub decay_ratio: f32,
    pub dry_mix: f32,
    pub wet_mix: f32,
}

impl Default for AudioEchoFilter {
    fn default() -> Self {
        Self {
            enabled: true,
            delay: 500.0,
            decay_ratio: 0.5,
            dry_mix: 1.0,
            wet_mix: 1.0,
        }
    }
}

impl AudioLowPassFilter {
    pub fn spec(&self) -> EffectSpec {
        filter(
            EffectKind::Lowpass,
            self.cutoff_frequency,
            self.resonance_q,
            self.enabled,
        )
    }
}

impl AudioHighPassFilter {
    pub fn spec(&self) -> EffectSpec {
        filter(
            EffectKind::Highpass,
            self.cutoff_frequency,
            self.resonance_q,
            self.enabled,
        )
    }
}

impl AudioEchoFilter {
    pub fn spec(&self) -> EffectSpec {
        let mut s = EffectSpec::new(
            EffectKind::Echo,
            vec![
                self.delay,
                self.decay_ratio * 100.0,
                self.dry_mix * 100.0,
                self.wet_mix * 100.0,
            ],
        );
        s.bypass = !self.enabled;
        s
    }
}

fn filter(kind: EffectKind, cutoff: f32, q: f32, enabled: bool) -> EffectSpec {
    let mut s = EffectSpec::new(kind, vec![cutoff, q]);
    s.bypass = !enabled;
    s
}

impl SceneComponent for AudioReverbZone {
    const NAME: &'static str = "Audio Reverb Zone";
    const CATEGORY: &'static str = "Audio";
}
impl SceneComponent for AudioLowPassFilter {
    const NAME: &'static str = "Audio Low Pass Filter";
    const CATEGORY: &'static str = "Audio";
}
impl SceneComponent for AudioHighPassFilter {
    const NAME: &'static str = "Audio High Pass Filter";
    const CATEGORY: &'static str = "Audio";
}
impl SceneComponent for AudioEchoFilter {
    const NAME: &'static str = "Audio Echo Filter";
    const CATEGORY: &'static str = "Audio";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_weight_fades_between_min_and_max() {
        let z = AudioReverbZone::default();
        assert_eq!(z.weight(5.0), 1.0);
        assert_eq!(z.weight(12.5), 0.5);
        assert_eq!(z.weight(20.0), 0.0);
    }

    #[test]
    fn filters_describe_their_effect() {
        let lp = AudioLowPassFilter {
            cutoff_frequency: 800.0,
            ..Default::default()
        };
        assert_eq!(lp.spec().values, [800.0, 1.0]);
        let echo = AudioEchoFilter {
            enabled: false,
            ..Default::default()
        };
        assert!(echo.spec().bypass);
        assert_eq!(echo.spec().value(1), 50.0);
    }

    #[test]
    fn old_audio_sources_save_unchanged() {
        let text = serde_json::to_string(&crate::scene::AudioSource::default()).unwrap();
        for field in [
            "output",
            "priority",
            "doppler_level",
            "spread",
            "bypass_effects",
        ] {
            assert!(!text.contains(field), "{field} left out at its default");
        }
    }
}
