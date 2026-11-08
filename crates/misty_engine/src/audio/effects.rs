//! Audio effects shared by mixer groups and the per-source filter components: their kinds,
//! parameter tables (name, unit, range, default) and the plain description handed to backends.
//!
//! Besides the signal effects there are three routing units, as in the familiar mixer: **Send**
//! copies the signal at its place in the chain (scaled by its Send level) to a **Receive** or a
//! **Duck Volume** unit on another group; Receive mixes what it is sent into its own group;
//! Duck Volume turns its group down while what it is sent is loud (a side-chain compressor).

use serde::{Deserialize, Serialize};

use super::backend::TrackId;

/// An effect that can sit in a mixer group's chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    Lowpass,
    Highpass,
    Echo,
    Reverb,
    Distortion,
    Compressor,
    Chorus,
    Flange,
    PitchShifter,
    ParamEQ,
    Send,
    Receive,
    DuckVolume,
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
const CHORUS: [ParamDef; 7] = [
    p("Dry mix", "%", 0.0, 100.0, 50.0),
    p("Wet mix tap 1", "%", 0.0, 100.0, 50.0),
    p("Wet mix tap 2", "%", 0.0, 100.0, 50.0),
    p("Wet mix tap 3", "%", 0.0, 100.0, 50.0),
    p("Delay", "ms", 0.1, 100.0, 40.0),
    p("Rate", "Hz", 0.0, 20.0, 0.8),
    p("Depth", "%", 0.0, 100.0, 3.0),
];
const FLANGE: [ParamDef; 4] = [
    p("Dry mix", "%", 0.0, 100.0, 45.0),
    p("Wet mix", "%", 0.0, 100.0, 55.0),
    p("Depth", "", 0.01, 1.0, 1.0),
    p("Rate", "Hz", 0.1, 20.0, 0.1),
];
const PITCH: [ParamDef; 3] = [
    p("Pitch", "×", 0.5, 2.0, 1.0),
    p("FFT size", "", 256.0, 4096.0, 1024.0),
    p("Overlap", "", 2.0, 32.0, 4.0),
];
const PARAM_EQ: [ParamDef; 3] = [
    p("Center freq", "Hz", 20.0, 22000.0, 8000.0),
    p("Octave range", "", 0.2, 5.0, 1.0),
    p("Frequency gain", "×", 0.05, 3.0, 1.0),
];
const SEND: [ParamDef; 1] = [p("Send level", "dB", MIN_DB, MAX_DB, MIN_DB)];
const DUCK: [ParamDef; 7] = [
    p("Threshold", "dB", -60.0, 0.0, -25.0),
    p("Ratio", ":1", 1.0, 20.0, 4.0),
    p("Attack", "ms", 0.0, 1000.0, 50.0),
    p("Release", "ms", 0.0, 5000.0, 300.0),
    p("Make up gain", "dB", 0.0, 30.0, 0.0),
    p("Knee", "dB", 0.0, 50.0, 10.0),
    p("Sidechain mix", "%", 0.0, 100.0, 100.0),
];

/// Name of the wet mix level every signal effect has besides its own parameters (keys
/// `"<group>/<effect>/Wet"`): how much of the effect's output replaces the dry signal.
pub const WET: &str = "Wet";
const WET_DEF: ParamDef = p(WET, "%", 0.0, 100.0, 100.0);

impl EffectKind {
    pub const ALL: [Self; 13] = [
        Self::Lowpass,
        Self::Highpass,
        Self::Echo,
        Self::Reverb,
        Self::Distortion,
        Self::Compressor,
        Self::Chorus,
        Self::Flange,
        Self::PitchShifter,
        Self::ParamEQ,
        Self::Send,
        Self::Receive,
        Self::DuckVolume,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lowpass => "Lowpass",
            Self::Highpass => "Highpass",
            Self::Echo => "Echo",
            Self::Reverb => "Reverb",
            Self::Distortion => "Distortion",
            Self::Compressor => "Compressor",
            Self::Chorus => "Chorus",
            Self::Flange => "Flange",
            Self::PitchShifter => "Pitch Shifter",
            Self::ParamEQ => "ParamEQ",
            Self::Send => "Send",
            Self::Receive => "Receive",
            Self::DuckVolume => "Duck Volume",
        }
    }

    pub fn params(self) -> &'static [ParamDef] {
        match self {
            Self::Lowpass | Self::Highpass => &FILTER,
            Self::Echo => &ECHO,
            Self::Reverb => &REVERB,
            Self::Distortion => &DISTORTION,
            Self::Compressor => &COMPRESSOR,
            Self::Chorus => &CHORUS,
            Self::Flange => &FLANGE,
            Self::PitchShifter => &PITCH,
            Self::ParamEQ => &PARAM_EQ,
            Self::Send => &SEND,
            Self::Receive => &[],
            Self::DuckVolume => &DUCK,
        }
    }

    /// Routing units (Send, Receive, Duck Volume) have no wet mix: they don't replace the
    /// signal with a processed copy.
    pub fn has_wet(self) -> bool {
        !self.is_routing()
    }

    pub fn is_routing(self) -> bool {
        matches!(self, Self::Send | Self::Receive | Self::DuckVolume)
    }

    /// Whether a Send can feed this kind of unit.
    pub fn receives_sends(self) -> bool {
        matches!(self, Self::Receive | Self::DuckVolume)
    }

    /// A parameter by name, including the wet mix level.
    pub fn param(self, name: &str) -> Option<&'static ParamDef> {
        self.keyed_params().find(|d| d.name == name)
    }

    /// Every value a snapshot stores for this effect: its parameters, then the wet level.
    pub fn keyed_params(self) -> impl Iterator<Item = &'static ParamDef> {
        let wet: &'static [ParamDef] = if self.has_wet() {
            std::slice::from_ref(&WET_DEF)
        } else {
            &[]
        };
        self.params().iter().chain(wet)
    }

    /// Default values in parameter order.
    pub fn defaults(self) -> Vec<f32> {
        self.params().iter().map(|d| d.default).collect()
    }
}

/// An effect as a backend builds and updates it: kind, values in [`EffectKind::params`] order,
/// bypass (the effect passes the signal through untouched), wet level and, for a Send, the
/// unit it feeds.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSpec {
    pub kind: EffectKind,
    pub values: Vec<f32>,
    pub bypass: bool,
    /// Identity across chain edits: an effect keeps its state (delay lines, reverb tail) while
    /// effects around it are added, removed or moved. 0 = anonymous (matched by position).
    pub id: u32,
    /// 0..1: share of the processed signal (1 = only the effect's output).
    pub wet: f32,
    /// Send only: the track and effect id of the Receive / Duck Volume it feeds.
    pub target: Option<(TrackId, u32)>,
}

impl EffectSpec {
    pub fn new(kind: EffectKind, values: Vec<f32>) -> Self {
        Self {
            kind,
            values,
            bypass: false,
            id: 0,
            wet: 1.0,
            target: None,
        }
    }

    pub fn with_id(mut self, id: u32) -> Self {
        self.id = id;
        self
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
