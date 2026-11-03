//! Audio effects shared by mixer groups and the per-source filter components: their kinds,
//! parameter tables (name, unit, range, default) and the plain description handed to backends.

use serde::{Deserialize, Serialize};

/// An effect that can sit in a mixer group's chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    Lowpass,
    Highpass,
    Echo,
    Reverb,
    Distortion,
    Compressor,
}

/// One tweakable effect parameter.
#[derive(Clone, Copy, Debug)]
pub struct ParamDef {
    pub name: &'static str,
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

const fn p(name: &'static str, unit: &'static str, min: f32, max: f32, default: f32) -> ParamDef {
    ParamDef {
        name,
        unit,
        min,
        max,
        default,
    }
}

const FILTER: [ParamDef; 2] = [
    p("Cutoff freq", "Hz", 10.0, 22000.0, 5000.0),
    p("Resonance", "", 1.0, 10.0, 1.0),
];
const ECHO: [ParamDef; 4] = [
    p("Delay", "ms", 1.0, 5000.0, 500.0),
    p("Decay", "%", 0.0, 100.0, 50.0),
    p("Dry mix", "%", 0.0, 100.0, 100.0),
    p("Wet mix", "%", 0.0, 100.0, 100.0),
];
const REVERB: [ParamDef; 4] = [
    p("Room size", "", 0.0, 1.0, 0.8),
    p("Damping", "", 0.0, 1.0, 0.3),
    p("Width", "", 0.0, 1.0, 1.0),
    p("Wet mix", "%", 0.0, 100.0, 40.0),
];
const DISTORTION: [ParamDef; 1] = [p("Level", "", 0.0, 1.0, 0.5)];
const COMPRESSOR: [ParamDef; 5] = [
    p("Threshold", "dB", -60.0, 0.0, 0.0),
    p("Ratio", ":1", 1.0, 20.0, 4.0),
    p("Attack", "ms", 0.0, 500.0, 50.0),
    p("Release", "ms", 0.0, 1000.0, 50.0),
    p("Make up gain", "dB", 0.0, 30.0, 0.0),
];

impl EffectKind {
    pub const ALL: [Self; 6] = [
        Self::Lowpass,
        Self::Highpass,
        Self::Echo,
        Self::Reverb,
        Self::Distortion,
        Self::Compressor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lowpass => "Lowpass",
            Self::Highpass => "Highpass",
            Self::Echo => "Echo",
            Self::Reverb => "Reverb",
            Self::Distortion => "Distortion",
            Self::Compressor => "Compressor",
        }
    }

    pub fn params(self) -> &'static [ParamDef] {
        match self {
            Self::Lowpass | Self::Highpass => &FILTER,
            Self::Echo => &ECHO,
            Self::Reverb => &REVERB,
            Self::Distortion => &DISTORTION,
            Self::Compressor => &COMPRESSOR,
        }
    }

    pub fn param(self, name: &str) -> Option<&'static ParamDef> {
        self.params().iter().find(|d| d.name == name)
    }

    /// Default values in parameter order.
    pub fn defaults(self) -> Vec<f32> {
        self.params().iter().map(|d| d.default).collect()
    }
}

/// An effect as a backend builds and updates it: kind, values in [`EffectKind::params`] order,
/// and bypass (the effect passes the signal through untouched).
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSpec {
    pub kind: EffectKind,
    pub values: Vec<f32>,
    pub bypass: bool,
}

impl EffectSpec {
    pub fn new(kind: EffectKind, values: Vec<f32>) -> Self {
        Self {
            kind,
            values,
            bypass: false,
        }
    }

    /// Value of parameter `i`, or its default.
    pub fn value(&self, i: usize) -> f32 {
        self.values
            .get(i)
            .copied()
            .unwrap_or_else(|| self.kind.params()[i].default)
    }
}

/// Decibels to linear amplitude; -80 dB and below is silence.
pub fn db_to_amplitude(db: f32) -> f32 {
    if db <= MIN_DB {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// Linear amplitude to decibels, floored at -80 dB.
pub fn amplitude_to_db(amplitude: f32) -> f32 {
    if amplitude <= 1e-4 {
        MIN_DB
    } else {
        (20.0 * amplitude.log10()).max(MIN_DB)
    }
}

/// Quietest level a fader goes to (silence).
pub const MIN_DB: f32 = -80.0;
/// Loudest fader level.
pub const MAX_DB: f32 = 20.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_and_amplitude_round_trip() {
        assert_eq!(db_to_amplitude(0.0), 1.0);
        assert!((db_to_amplitude(-6.0206) - 0.5).abs() < 1e-4);
        assert!((db_to_amplitude(20.0) - 10.0).abs() < 1e-4);
        assert_eq!(
            db_to_amplitude(-80.0),
            0.0,
            "the bottom of the fader is silence"
        );
        assert_eq!(amplitude_to_db(1.0), 0.0);
        assert!((amplitude_to_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(amplitude_to_db(0.0), MIN_DB);
        for db in [-40.0, -12.5, -3.0, 0.0, 6.0] {
            assert!((amplitude_to_db(db_to_amplitude(db)) - db).abs() < 1e-3);
        }
    }

    #[test]
    fn every_kind_has_sane_defaults() {
        for k in EffectKind::ALL {
            for d in k.params() {
                assert!(d.min <= d.default && d.default <= d.max, "{k:?} {}", d.name);
            }
            assert_eq!(k.defaults().len(), k.params().len());
        }
        let spec = EffectSpec::new(EffectKind::Lowpass, vec![]);
        assert_eq!(
            spec.value(0),
            5000.0,
            "missing values fall back to defaults"
        );
    }
}
