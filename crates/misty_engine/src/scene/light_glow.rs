//! Light glows: the Light's Halo section (a soft sprite around point and spot lights), its
//! Glow section (how strongly and how widely the light's visible shape glows) and the Lens
//! Flare component (procedural glow, ring, streak and ghosts). All are drawn by `render/glow`;
//! scenes without them save exactly as before.

use serde::{Deserialize, Serialize};

use super::registry::SceneComponent;

/// Light ▸ Halo: a camera-facing glow at the light that grows in fog.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightHalo {
    pub enabled: bool,
    /// Radius of the glow in metres (in clear air; fog widens it).
    pub size: f32,
    /// Brightness multiplier on top of the light's colour × intensity.
    pub intensity: f32,
    /// sRGB tint; `None` = the light's colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    /// 0 = tight bright core, 1 = wide soft falloff.
    pub softness: f32,
    /// How strongly local fog widens and brightens the halo (0 = ignores fog).
    pub fog_response: f32,
}

impl Default for LightHalo {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 1.0,
            intensity: 1.0,
            color: None,
            softness: 0.5,
            fog_response: 1.0,
        }
    }
}

impl LightHalo {
    /// Left out of scene files while untouched.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Light ▸ Glow: how the light's visible shape (a Rectangle light's emitter, a point or spot
/// light's Halo) glows, independent of how much it lights the scene.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightGlow {
    /// Multiplier on the glow (bloom and halo) of the light's visible shape.
    pub intensity: f32,
    /// 0 = tight, 1 = wide and soft: the same glow energy over a wider, smoother falloff, so
    /// the core gets less harsh as it goes up, with no extra brightness.
    pub spread: f32,
    /// 0..1: compresses the clipped white core of the visible shape (its glow is unchanged).
    pub soft_core: f32,
}

impl Default for LightGlow {
    fn default() -> Self {
        Self {
            intensity: 1.0,
            spread: 0.0,
            soft_core: 0.0,
        }
    }
}

impl LightGlow {
    /// Left out of scene files while untouched.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Clamped copy (negative intensity, spread and soft core outside 0..1 are ignored).
    pub fn clamped(&self) -> Self {
        Self {
            intensity: self.intensity.max(0.0),
            spread: self.spread.clamp(0.0, 1.0),
            soft_core: self.soft_core.clamp(0.0, 1.0),
        }
    }
}

/// Lens Flare: screen-space elements along the line from the light through the screen
/// centre. Uses the Light on the same object for its colour (white without one).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensFlare {
    pub enabled: bool,
    /// Brightness multiplier of every element.
    pub intensity: f32,
    /// Size multiplier of every element.
    pub scale: f32,
    /// sRGB tint multiplied with the light colour.
    pub tint: [f32; 3],
    /// A thin ring around the light.
    pub ring: bool,
    /// A horizontal streak through the light.
    pub streak: bool,
    /// Ghost discs along the screen-centre line (0–8).
    pub ghosts: u32,
}

impl Default for LensFlare {
    fn default() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            scale: 1.0,
            tint: [1.0, 1.0, 1.0],
            ring: true,
            streak: true,
            ghosts: 4,
        }
    }
}

/// Most ghosts a flare draws.
pub const MAX_GHOSTS: u32 = 8;

impl SceneComponent for LensFlare {
    const NAME: &'static str = "Lens Flare";
    const CATEGORY: &'static str = "Rendering";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Light;

    #[test]
    fn untouched_halo_is_not_saved() {
        let json = serde_json::to_string(&Light::default()).unwrap();
        assert!(!json.contains("halo"), "{json}");
        let mut l = Light::default();
        l.halo.enabled = true;
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains("\"halo\""), "{json}");
        assert!(!json.contains("color\":null"), "None colour is left out");
        let back: Light = serde_json::from_str(&json).unwrap();
        assert_eq!(back.halo, l.halo);
    }

    #[test]
    fn untouched_glow_is_not_saved() {
        for l in [Light::default(), Light::rectangle(), Light::directional()] {
            let json = serde_json::to_string(&l).unwrap();
            assert!(!json.contains("glow"), "{json}");
        }
        let mut l = Light::rectangle();
        l.glow.spread = 0.5;
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains("\"glow\":{"), "{json}");
        let back: Light = serde_json::from_str(&json).unwrap();
        assert_eq!(back.glow, l.glow);
        // Only the touched field matters; the others load at their defaults.
        let partial: Light =
            serde_json::from_str("{\"kind\":\"Rectangle\",\"glow\":{\"spread\":1.0}}").unwrap();
        assert_eq!(partial.glow.spread, 1.0);
        assert_eq!(partial.glow.intensity, 1.0);
        assert_eq!(partial.glow.soft_core, 0.0);
    }

    #[test]
    fn old_lights_load_with_the_default_glow() {
        let old = "{\"kind\":\"Rectangle\",\"intensity\":40.0,\"width\":2.0}";
        let l: Light = serde_json::from_str(old).unwrap();
        assert!(l.glow.is_default());
        // Re-saved unchanged: no new keys.
        let json = serde_json::to_string(&l).unwrap();
        assert!(!json.contains("glow"), "{json}");
    }

    #[test]
    fn old_lights_load_without_a_halo() {
        let l: Light = serde_json::from_str("{\"kind\":\"Point\"}").unwrap();
        assert!(!l.halo.enabled);
        let f: LensFlare = serde_json::from_str("{}").unwrap();
        assert_eq!(f, LensFlare::default());
    }
}
