//! Screen-space contact shadows: settings from the Post-process Volume. The ray march runs in
//! the lit shaders (`contact_shadow` in shaders/screen_space.wgsl) for each light whose
//! "Contact Shadows" toggle is on, over the depth prepass of `screen_space.rs`.

use crate::scene::ContactShadowsOverride;

/// Contact shadows of one view. `Default` = off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactShadowSettings {
    pub enabled: bool,
    /// Ray length towards the light (metres).
    pub length: f32,
    /// Camera distance where they have faded out (fade over the last 20%).
    pub distance_fade: f32,
    pub opacity: f32,
    pub sample_count: u32,
    /// Distant shadows past the shadow maps (0..1, 0 = off).
    pub far: f32,
}

impl Default for ContactShadowSettings {
    fn default() -> Self {
        Self::from_override(&ContactShadowsOverride::default())
    }
}

impl ContactShadowSettings {
    pub fn from_override(o: &ContactShadowsOverride) -> Self {
        Self {
            enabled: o.active,
            length: o.length.clamp(0.0, 5.0),
            distance_fade: o.distance_fade.max(0.1),
            opacity: o.opacity.clamp(0.0, 1.0),
            sample_count: o.sample_count.clamp(4, 64),
            far: o.far_shadows.clamp(0.0, 1.0),
        }
    }

    pub fn active(&self) -> bool {
        self.enabled && self.length > 0.0 && self.opacity > 0.0
    }

    /// Distant shadows run (the volume's Contact Shadows on with a Distant Shadows amount).
    pub fn far_active(&self) -> bool {
        self.enabled && self.far > 0.0
    }

    /// How far behind a surface a ray sample still counts as blocked (the shader adds 1% of
    /// the depth): thin, so rays pass behind round shapes and objects standing in front,
    /// but at least two steps so a ray entering an occluder is caught.
    pub fn thickness(&self) -> f32 {
        (self.length / self.sample_count as f32 * 2.0).max(0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default() {
        assert!(!ContactShadowSettings::default().active());
        let on = ContactShadowSettings::from_override(&ContactShadowsOverride {
            active: true,
            ..Default::default()
        });
        assert!(on.active() && on.thickness() >= 0.1);
    }
}
