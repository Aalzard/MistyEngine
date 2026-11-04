//! Water look and wave settings (the Water Surface Inspector's sections) and presets.

use serde::{Deserialize, Serialize};

/// Which wave spectrum drives the FFT ocean.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpectrumKind {
    /// Phillips: the classic wind-driven spectrum.
    Phillips,
    /// JONSWAP: a fetch-limited sea with a sharper peak (more coherent waves).
    #[default]
    Jonswap,
}

impl SpectrumKind {
    pub const ALL: [Self; 2] = [Self::Phillips, Self::Jonswap];

    pub fn label(self) -> &'static str {
        match self {
            Self::Phillips => "Phillips",
            Self::Jonswap => "JONSWAP",
        }
    }
}

/// Everything about how a water surface moves and looks. Colours are sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterSettings {
    // ---- Waves ----
    /// Wind over the water in m/s: bigger, longer, choppier waves.
    pub wind_speed: f32,
    /// Degrees clockwise from +Z the wind blows towards.
    pub wind_direction: f32,
    /// Multiplier on the wind waves' height (1 = a fully developed sea for this wind).
    pub wave_height: f32,
    /// Sideways motion that sharpens crests (0 = round waves).
    pub choppiness: f32,
    pub spectrum: SpectrumKind,
    /// Height in metres of long, smooth swell coming from far away.
    pub swell_height: f32,
    /// Swell wavelength in metres.
    pub swell_length: f32,
    pub swell_direction: f32,
    /// Strength of the fine ripples in the lighting (normal detail).
    pub ripples: f32,
    /// Rivers: surface flow in m/s along the object's forward (+Z) axis.
    pub flow_speed: f32,
    // ---- Color & Clarity ----
    /// Colour of thin, shallow water.
    pub shallow_color: [f32; 3],
    /// Colour the water turns towards with depth.
    pub deep_color: [f32; 3],
    /// How far you can see into the water, in metres.
    pub clarity: f32,
    /// Light glowing through thin wave crests (subsurface scattering).
    pub scattering: f32,
    pub scattering_color: [f32; 3],
    /// Surface gloss (1 = mirror-like sun glitter).
    pub smoothness: f32,
    // ---- Foam ----
    /// White caps on breaking crests.
    pub crest_foam: f32,
    /// Foam where the water meets the shore or objects.
    pub shore_foam: f32,
    /// Width of the shore foam band in metres.
    pub shore_foam_width: f32,
    pub foam_color: [f32; 3],
    // ---- Reflections & Refraction ----
    /// Strength of sky / scene reflections.
    pub reflections: f32,
    /// Reflect what is on screen (islands, ships), else only the sky.
    pub screen_space_reflections: bool,
    /// How much waves bend the view of what is underwater.
    pub refraction: f32,
    // ---- Caustics ----
    /// Brightness of the dancing light patterns on underwater surfaces (0 = off).
    pub caustics: f32,
    /// Size of the caustic pattern in metres.
    pub caustics_scale: f32,
    /// Depth in metres below which caustics fade out.
    pub caustics_depth: f32,
    // ---- Underwater ----
    /// Tint, fog and wobble when the camera dives below the surface.
    pub underwater: bool,
    pub underwater_color: [f32; 3],
    /// Underwater visibility in metres.
    pub underwater_visibility: f32,
    pub underwater_distortion: f32,
}

impl Default for WaterSettings {
    fn default() -> Self {
        WaterPreset::Ocean.settings()
    }
}

impl WaterSettings {
    /// The Calm ↔ Stormy slider position (0..1) these settings correspond to.
    pub fn storminess(&self) -> f32 {
        ((self.wind_speed - 2.0) / 22.0).clamp(0.0, 1.0)
    }

    /// Set wind, wave height, chop, swell and foam together from Calm (0) to Stormy (1).
    pub fn set_storminess(&mut self, t: f32) {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        self.wind_speed = lerp(2.0, 24.0);
        self.wave_height = lerp(0.6, 1.0);
        self.choppiness = lerp(0.5, 1.3);
        self.swell_height = lerp(0.15, 2.5);
        self.crest_foam = lerp(0.0, 0.9);
        self.ripples = lerp(0.6, 1.2);
    }
}

/// Starting points in the Inspector (and for [`super::spawn_water`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WaterPreset {
    Tropical,
    #[default]
    Ocean,
    Stormy,
    Lake,
    MurkyRiver,
}

impl WaterPreset {
    pub const ALL: [Self; 5] = [
        Self::Tropical,
        Self::Ocean,
        Self::Stormy,
        Self::Lake,
        Self::MurkyRiver,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tropical => "Tropical",
            Self::Ocean => "Ocean",
            Self::Stormy => "Stormy",
            Self::Lake => "Lake",
            Self::MurkyRiver => "Murky River",
        }
    }

    pub fn settings(self) -> WaterSettings {
        let ocean = WaterSettings {
            wind_speed: 9.0,
            wind_direction: 30.0,
            wave_height: 0.9,
            choppiness: 0.9,
            spectrum: SpectrumKind::Jonswap,
            swell_height: 0.6,
            swell_length: 90.0,
            swell_direction: 10.0,
            ripples: 1.0,
            flow_speed: 0.0,
            shallow_color: [0.12, 0.55, 0.6],
            deep_color: [0.01, 0.09, 0.18],
            clarity: 8.0,
            scattering: 1.0,
            scattering_color: [0.1, 0.65, 0.55],
            smoothness: 0.96,
            crest_foam: 0.35,
            shore_foam: 0.8,
            shore_foam_width: 1.5,
            foam_color: [0.95, 0.97, 1.0],
            reflections: 1.0,
            screen_space_reflections: true,
            refraction: 0.5,
            caustics: 1.0,
            caustics_scale: 4.0,
            caustics_depth: 8.0,
            underwater: true,
            underwater_color: [0.04, 0.25, 0.32],
            underwater_visibility: 25.0,
            underwater_distortion: 0.5,
        };
        match self {
            Self::Ocean => ocean,
            Self::Tropical => WaterSettings {
                wind_speed: 5.5,
                wave_height: 0.8,
                choppiness: 0.7,
                swell_height: 0.35,
                swell_length: 70.0,
                shallow_color: [0.2, 0.85, 0.8],
                deep_color: [0.0, 0.25, 0.45],
                clarity: 25.0,
                scattering_color: [0.15, 0.9, 0.75],
                crest_foam: 0.15,
                caustics: 1.6,
                underwater_color: [0.05, 0.45, 0.55],
                underwater_visibility: 45.0,
                ..ocean
            },
            Self::Stormy => {
                let mut s = WaterSettings {
                    shallow_color: [0.12, 0.3, 0.32],
                    deep_color: [0.02, 0.05, 0.08],
                    clarity: 4.0,
                    scattering_color: [0.15, 0.4, 0.4],
                    smoothness: 0.9,
                    caustics: 0.4,
                    underwater_color: [0.03, 0.12, 0.15],
                    underwater_visibility: 10.0,
                    ..ocean
                };
                s.set_storminess(0.95);
                s
            }
            Self::Lake => WaterSettings {
                wind_speed: 3.0,
                wave_height: 0.5,
                choppiness: 0.4,
                swell_height: 0.0,
                ripples: 0.7,
                shallow_color: [0.25, 0.45, 0.35],
                deep_color: [0.03, 0.12, 0.12],
                clarity: 5.0,
                scattering: 0.5,
                scattering_color: [0.25, 0.5, 0.35],
                crest_foam: 0.0,
                shore_foam: 0.4,
                shore_foam_width: 0.6,
                caustics: 0.8,
                caustics_scale: 2.0,
                underwater_color: [0.06, 0.2, 0.18],
                underwater_visibility: 12.0,
                ..ocean
            },
            Self::MurkyRiver => WaterSettings {
                wind_speed: 2.0,
                wave_height: 0.4,
                choppiness: 0.3,
                swell_height: 0.0,
                ripples: 1.1,
                flow_speed: 1.5,
                shallow_color: [0.45, 0.4, 0.25],
                deep_color: [0.12, 0.1, 0.05],
                clarity: 1.2,
                scattering: 0.3,
                scattering_color: [0.4, 0.4, 0.25],
                smoothness: 0.92,
                crest_foam: 0.0,
                shore_foam: 0.6,
                shore_foam_width: 0.8,
                foam_color: [0.85, 0.82, 0.72],
                refraction: 0.25,
                caustics: 0.2,
                underwater_color: [0.2, 0.17, 0.08],
                underwater_visibility: 3.0,
                ..ocean
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storminess_round_trips_and_old_files_load() {
        let mut s = WaterSettings::default();
        s.set_storminess(0.25);
        assert!((s.storminess() - 0.25).abs() < 1e-5);
        assert!(
            WaterPreset::Stormy.settings().wind_speed > WaterPreset::Lake.settings().wind_speed
        );
        let partial: WaterSettings = serde_json::from_str(r#"{ "clarity": 3.0 }"#).unwrap();
        assert_eq!(partial.clarity, 3.0);
        assert_eq!(partial.wind_speed, WaterSettings::default().wind_speed);
    }
}
