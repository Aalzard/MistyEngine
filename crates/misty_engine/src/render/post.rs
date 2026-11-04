//! Post-processing values for the composite and bloom passes, resolved from the scene's
//! Post-process Volume (or the baseline when there is none), plus CPU copies of the colour
//! math in `composite.wgsl` for tests.

use glam::Vec3;

use super::{FogSettings, SunShaftSettings, srgb_to_linear};

use super::{AoSettings, ContactShadowSettings};
use crate::scene::{PostProcessVolume, TonemapMode};

/// Effect values for one view. `Default` is the baseline every scene gets without a volume:
/// bloom 0.6, no grading, hue-preserving highlight compression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostSettings {
    pub tonemap: TonemapMode,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub bloom_scatter: f32,
    /// sRGB.
    pub bloom_tint: [f32; 3],
    /// EV.
    pub post_exposure: f32,
    /// -100..100.
    pub contrast: f32,
    /// -100..100.
    pub saturation: f32,
    /// sRGB.
    pub color_filter: [f32; 3],
    pub vignette_intensity: f32,
    pub vignette_smoothness: f32,
    /// sRGB.
    pub vignette_color: [f32; 3],
    /// Volumetric / height fog (off by default).
    pub fog: FogSettings,
    /// Screen-space sun shafts (off by default).
    pub sun_shafts: SunShaftSettings,
    /// Screen-space ambient occlusion (render/ao.rs).
    pub ao: AoSettings,
    pub contact_shadows: ContactShadowSettings,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            tonemap: TonemapMode::None,
            bloom_intensity: 0.6,
            bloom_threshold: 0.0,
            bloom_scatter: 0.7,
            bloom_tint: [1.0; 3],
            post_exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            color_filter: [1.0; 3],
            vignette_intensity: 0.0,
            vignette_smoothness: 0.2,
            vignette_color: [0.0; 3],
            fog: FogSettings::default(),
            sun_shafts: SunShaftSettings::default(),
            ao: AoSettings::default(),
            contact_shadows: ContactShadowSettings::default(),
        }
    }
}

impl PostSettings {
    /// Baseline with the volume's active overrides applied.
    pub fn from_volume(v: &PostProcessVolume) -> Self {
        let mut s = Self::default();
        if v.tonemapping.active {
            s.tonemap = v.tonemapping.mode;
        }
        let b = &v.bloom;
        if b.active {
            s.bloom_intensity = b.intensity;
            s.bloom_threshold = b.threshold;
            s.bloom_scatter = b.scatter;
            s.bloom_tint = b.tint;
        }
        let c = &v.color_adjustments;
        if c.active {
            s.post_exposure = c.post_exposure;
            s.contrast = c.contrast;
            s.saturation = c.saturation;
            s.color_filter = c.color_filter;
        }
        let g = &v.vignette;
        if g.active {
            s.vignette_intensity = g.intensity;
            s.vignette_smoothness = g.smoothness;
            s.vignette_color = g.color;
        }
        if v.fog.active {
            s.fog = FogSettings::from_override(&v.fog);
        }
        if v.sun_shafts.active {
            s.sun_shafts = v.sun_shafts;
        }
        s.ao = AoSettings::from_override(&v.ambient_occlusion);
        s.contact_shadows = ContactShadowSettings::from_override(&v.contact_shadows);
        s
    }

    /// Weight of each smaller bloom level when upsampling (1 at the default scatter 0.7).
    fn scatter_weight(&self) -> f32 {
        0.3 + self.bloom_scatter.clamp(0.0, 1.0)
    }

    /// Post vec4s of the view uniform (layout documented in common.wgsl).
    pub(crate) fn pack(&self) -> [[f32; 4]; 6] {
        let mode = match self.tonemap {
            TonemapMode::None => 0.0,
            TonemapMode::Neutral => 1.0,
            TonemapMode::Aces => 2.0,
        };
        let v4 = |c: [f32; 3], w: f32| {
            let l = srgb_to_linear(c);
            [l[0], l[1], l[2], w]
        };
        [
            [
                mode,
                self.post_exposure.exp2(),
                1.0 + self.contrast.clamp(-100.0, 100.0) / 100.0,
                1.0 + self.saturation.clamp(-100.0, 100.0) / 100.0,
            ],
            v4(self.color_filter, 0.0),
            [
                self.bloom_intensity.max(0.0),
                self.bloom_threshold.max(0.0),
                self.scatter_weight(),
                0.0,
            ],
            v4(self.bloom_tint, 0.0),
            [
                self.vignette_intensity.clamp(0.0, 1.0) * 3.0,
                self.vignette_smoothness.clamp(0.01, 1.0) * 5.0,
                0.0,
                0.0,
            ],
            v4(self.vignette_color, 0.0),
        ]
    }
}

// ---- CPU copies of composite.wgsl -----------------------------------------------------------

/// Over-bright colours keep their hue and whiten gradually (tonemapper "None").
pub fn compress(c: Vec3) -> Vec3 {
    let m = c.max_element();
    if m <= 1.0 {
        return c;
    }
    let white = ((m - 1.0) / (m + 2.0)).clamp(0.0, 1.0) * 0.6;
    (c / m).lerp(Vec3::ONE, white)
}

/// Hable's filmic curve.
fn filmic(x: Vec3) -> Vec3 {
    let (a, b, c, d, e, f) = (0.15, 0.50, 0.10, 0.20, 0.02, 0.30);
    (x * (x * a + c * b) + d * e) / (x * (x * a + b) + d * f) - Vec3::splat(e / f)
}

/// Neutral: filmic curve scaled so mid grey (0.18) stays about where it is.
pub fn tonemap_neutral(c: Vec3) -> Vec3 {
    let white = filmic(Vec3::splat(11.2));
    (filmic(c.max(Vec3::ZERO) * 2.9) / white).clamp(Vec3::ZERO, Vec3::ONE)
}

/// ACES: Stephen Hill's fit of the RRT + sRGB ODT.
pub fn tonemap_aces(c: Vec3) -> Vec3 {
    let input = glam::Mat3::from_cols(
        Vec3::new(0.59719, 0.07600, 0.02840),
        Vec3::new(0.35458, 0.90834, 0.13383),
        Vec3::new(0.04823, 0.01566, 0.83777),
    );
    let output = glam::Mat3::from_cols(
        Vec3::new(1.60475, -0.10208, -0.00327),
        Vec3::new(-0.53108, 1.10813, -0.07276),
        Vec3::new(-0.07367, -0.00605, 1.07602),
    );
    let v = input * c.max(Vec3::ZERO);
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (v * 0.983729 + 0.432951) + 0.238081;
    (output * (a / b)).clamp(Vec3::ZERO, Vec3::ONE)
}

pub fn luminance(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// Exposure, colour filter, contrast, saturation and tonemapping of one linear colour
/// (bloom and vignette happen before this).
pub fn grade(c: Vec3, s: &PostSettings) -> Vec3 {
    let p = s.pack();
    let mut c = c * p[0][1] * Vec3::from_slice(&p[1][..3]);
    if p[0][2] != 1.0 {
        let mid = 0.18f32.log2();
        let l = (c.max(Vec3::splat(1e-6))).to_array().map(|v| v.log2());
        c = Vec3::from(l.map(|v| ((v - mid) * p[0][2] + mid).exp2()));
    }
    if p[0][3] != 1.0 {
        let y = luminance(c);
        c = (Vec3::splat(y) + (c - y) * p[0][3]).max(Vec3::ZERO);
    }
    match s.tonemap {
        TonemapMode::None => compress(c),
        TonemapMode::Neutral => tonemap_neutral(c),
        TonemapMode::Aces => tonemap_aces(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_only_compresses_highlights() {
        let s = PostSettings::default();
        let c = Vec3::new(0.2, 0.5, 0.9);
        assert_eq!(grade(c, &s), c);
        let hot = grade(Vec3::new(4.0, 2.0, 1.0), &s);
        assert!(hot.max_element() <= 1.0 && hot.x > hot.y && hot.y > hot.z);
        // Same values as before these settings existed.
        let p = s.pack();
        assert_eq!(p[2][0], 0.6);
        assert_eq!(p[2][1], 0.0);
        assert_eq!(p[2][2], 1.0);
        assert_eq!(p[4][0], 0.0);
        assert_eq!(PostSettings::from_volume(&PostProcessVolume::default()), s);
    }

    #[test]
    fn tonemappers_are_monotonic_and_bounded() {
        for f in [tonemap_neutral, tonemap_aces] {
            assert!(f(Vec3::ZERO).max_element() < 0.01);
            let mut last = -1.0;
            for i in 0..200 {
                let v = f(Vec3::splat(i as f32 * 0.25)).x;
                assert!(v >= last && v <= 1.0, "{i}: {v}");
                last = v;
            }
            assert!(f(Vec3::splat(100.0)).x > 0.95);
        }
        // Neutral keeps mid grey; ACES darkens it a little.
        assert!((tonemap_neutral(Vec3::splat(0.18)).x - 0.18).abs() < 0.01);
        let aces = tonemap_aces(Vec3::splat(0.18)).x;
        assert!((0.08..0.15).contains(&aces), "{aces}");
    }

    #[test]
    fn grading_controls() {
        let mut s = PostSettings {
            post_exposure: 1.0,
            ..Default::default()
        };
        assert!((grade(Vec3::splat(0.2), &s).x - 0.4).abs() < 1e-6);
        s.post_exposure = 0.0;
        s.saturation = -100.0;
        let g = grade(Vec3::new(0.6, 0.2, 0.1), &s);
        assert!((g.x - g.y).abs() < 1e-6 && (g.y - g.z).abs() < 1e-6);
        s.saturation = 0.0;
        s.contrast = 100.0;
        // Mid grey is the contrast pivot; darker gets darker.
        assert!((grade(Vec3::splat(0.18), &s).x - 0.18).abs() < 1e-5);
        assert!(grade(Vec3::splat(0.05), &s).x < 0.05);
        s.contrast = 0.0;
        s.color_filter = [1.0, 0.0, 0.0];
        assert_eq!(grade(Vec3::ONE * 0.5, &s), Vec3::new(0.5, 0.0, 0.0));
    }
}
