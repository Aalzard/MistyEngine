//! Project Settings ▸ Quality: how shadows are filtered and how far the sun's cascades reach.
//! The renderer reads the installed copy process-wide (see [`set`]).

use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// Soft-shadow sample budget: blocker search taps and filter taps per shadowed light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowQuality {
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}

impl ShadowQuality {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }

    /// (blocker search samples, filter samples).
    pub fn samples(self) -> (u32, u32) {
        match self {
            Self::Low => (8, 12),
            Self::Medium => (12, 20),
            Self::High => (16, 32),
            Self::Ultra => (24, 64),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowSettings {
    pub quality: ShadowQuality,
    /// Directional light cascades (1–4).
    pub cascades: u32,
    /// Metres from the camera that the sun's shadows reach (they fade out over the last 10%).
    pub distance: f32,
    /// 0 = uniform splits, 1 = logarithmic; values between mix the two (practical split).
    pub split_lambda: f32,
    /// Fraction of each cascade blended into the next one.
    pub cascade_blend: f32,
}

impl Default for ShadowSettings {
    fn default() -> Self {
        Self {
            quality: ShadowQuality::Medium,
            cascades: 4,
            distance: 150.0,
            split_lambda: 0.8,
            cascade_blend: 0.1,
        }
    }
}

impl ShadowSettings {
    /// Clamp to values the renderer supports.
    pub fn sanitize(&mut self) {
        let d = Self::default();
        self.cascades = self.cascades.clamp(1, 4);
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        self.distance = finite(self.distance, d.distance).clamp(1.0, 10_000.0);
        self.split_lambda = finite(self.split_lambda, d.split_lambda).clamp(0.0, 1.0);
        self.cascade_blend = finite(self.cascade_blend, d.cascade_blend).clamp(0.0, 0.5);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QualitySettings {
    pub shadows: ShadowSettings,
}

impl QualitySettings {
    pub fn sanitize(&mut self) {
        self.shadows.sanitize();
    }
}

static SHADOWS: RwLock<Option<ShadowSettings>> = RwLock::new(None);

/// Install the shadow settings every renderer uses from now on.
pub fn set(settings: &QualitySettings) {
    if let Ok(mut s) = SHADOWS.write() {
        *s = Some(settings.shadows);
    }
}

/// The installed shadow settings (defaults until [`set`] is called).
pub fn shadows() -> ShadowSettings {
    SHADOWS.read().ok().and_then(|s| *s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_and_old_files_load() {
        let mut s = ShadowSettings {
            cascades: 9,
            distance: f32::NAN,
            split_lambda: 3.0,
            ..Default::default()
        };
        s.sanitize();
        assert_eq!(s.cascades, 4);
        assert_eq!(s.distance, 150.0);
        assert_eq!(s.split_lambda, 1.0);
        let q: QualitySettings =
            serde_json::from_str(r#"{"shadows":{"quality":"Ultra"}}"#).unwrap();
        assert_eq!(q.shadows.quality, ShadowQuality::Ultra);
        assert_eq!(q.shadows.cascades, 4);
        assert!(ShadowQuality::Ultra.samples().1 > ShadowQuality::Low.samples().1);
    }
}
