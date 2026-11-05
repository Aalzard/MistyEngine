//! Post-processing values for the composite and bloom passes, resolved from the scene's
//! Post-process Volume (or the baseline when there is none), plus CPU copies of the colour
//! math in `composite.wgsl` for tests.

use glam::Vec3;

use super::{FogSettings, SunShaftSettings, srgb_to_linear};

use super::{AoSettings, ContactShadowSettings};
use crate::scene::{CloudsOverride, PostProcessVolume, TonemapLook, TonemapMode};

/// Effect values for one view. `Default` is the baseline every scene gets without a volume:
/// bloom 0.6, no grading, hue-preserving highlight compression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostSettings {
    pub tonemap: TonemapMode,
    /// AgX look.
    pub tonemap_look: TonemapLook,
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
    /// White balance, -100..100 each (folded into the colour filter when packed).
    pub temperature: f32,
    pub tint: f32,
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
    /// Screen Space Global Illumination (render/screen_trace).
    pub ssgi: super::SsgiSettings,
    /// Screen Space Reflections (render/screen_trace).
    pub ssr: super::SsrSettings,
    /// Volumetric clouds (off by default; drawn with the skybox).
    pub clouds: CloudsOverride,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            tonemap: TonemapMode::None,
            tonemap_look: TonemapLook::Base,
            bloom_intensity: 0.6,
            bloom_threshold: 0.0,
            bloom_scatter: 0.7,
            bloom_tint: [1.0; 3],
            post_exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            color_filter: [1.0; 3],
            temperature: 0.0,
            tint: 0.0,
            vignette_intensity: 0.0,
            vignette_smoothness: 0.2,
            vignette_color: [0.0; 3],
            fog: FogSettings::default(),
            sun_shafts: SunShaftSettings::default(),
            ao: AoSettings::default(),
            contact_shadows: ContactShadowSettings::default(),
            ssgi: Default::default(),
            ssr: Default::default(),
            clouds: CloudsOverride::default(),
        }
    }
}

impl PostSettings {
    /// Baseline with the volume's active overrides applied.
    pub fn from_volume(v: &PostProcessVolume) -> Self {
        let mut s = Self::default();
        if v.tonemapping.active {
            s.tonemap = v.tonemapping.mode;
            s.tonemap_look = v.tonemapping.look;
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
            s.temperature = c.temperature;
            s.tint = c.tint;
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
        s.ssgi = super::SsgiSettings::from_override(&v.screen_space_gi);
        s.ssr = super::SsrSettings::from_override(&v.screen_space_reflections);
        s.clouds = v.clouds;
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
            TonemapMode::Agx => 3.0,
            TonemapMode::PbrNeutral => 4.0,
        };
        let v4 = |c: [f32; 3], w: f32| {
            let l = srgb_to_linear(c);
            [l[0], l[1], l[2], w]
        };
        let look = match self.tonemap_look {
            TonemapLook::Base => 0.0,
            TonemapLook::Punchy => 1.0,
        };
        let balance = white_balance(self.temperature, self.tint);
        let filter = {
            let f = v4(self.color_filter, look);
            [f[0] * balance.x, f[1] * balance.y, f[2] * balance.z, f[3]]
        };
        [
            [
                mode,
                self.post_exposure.exp2(),
                1.0 + self.contrast.clamp(-100.0, 100.0) / 100.0,
                1.0 + self.saturation.clamp(-100.0, 100.0) / 100.0,
            ],
            filter,
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

/// AgX: log2 encoding of a slightly desaturated ("inset") input between -12.47 and +4.03 EV
/// around mid grey, a sigmoid fitted to the AgX base contrast, an optional look, then the
/// outset back and display decoding (2.2). Mid grey 0.18 lands near 0.18 after decoding.
pub fn tonemap_agx(c: Vec3, look: TonemapLook) -> Vec3 {
    let inset = glam::Mat3::from_cols(
        Vec3::new(0.842_479_06, 0.042_328_24, 0.042_375_65),
        Vec3::new(0.078_433_6, 0.878_468_6, 0.078_433_6),
        Vec3::new(0.079_223_75, 0.079_166_13, 0.879_143),
    );
    let outset = glam::Mat3::from_cols(
        Vec3::new(1.196_879, -0.052_896_85, -0.052_971_64),
        Vec3::new(-0.098_020_88, 1.151_903_1, -0.098_043_45),
        Vec3::new(-0.099_029_74, -0.098_961_18, 1.151_073_7),
    );
    let (lo, hi) = (-12.473_93_f32, 4.026_069_f32);
    let v = inset * c.max(Vec3::ZERO);
    let v = Vec3::from(
        v.to_array()
            .map(|x| ((x.max(1e-10).log2().clamp(lo, hi) - lo) / (hi - lo)).clamp(0.0, 1.0)),
    );
    let mut v = Vec3::from(v.to_array().map(agx_sigmoid));
    if look == TonemapLook::Punchy {
        let y = luminance(v);
        v = Vec3::from(v.max(Vec3::ZERO).to_array().map(|x| x.powf(1.35)));
        v = Vec3::splat(y) + (v - y) * 1.4;
    }
    let v = outset * v;
    Vec3::from(v.max(Vec3::ZERO).to_array().map(|x| x.powf(2.2))).clamp(Vec3::ZERO, Vec3::ONE)
}

/// Polynomial fit of the AgX base contrast curve on the 0..1 log encoding.
fn agx_sigmoid(x: f32) -> f32 {
    let x2 = x * x;
    let x4 = x2 * x2;
    15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x
        - 0.00232
}

/// Khronos PBR Neutral: a small toe offset, then highlights above 0.76 roll off towards white
/// and desaturate a little; colours below stay as authored.
pub fn tonemap_pbr_neutral(c: Vec3) -> Vec3 {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    let mut c = c.max(Vec3::ZERO);
    let x = c.min_element();
    let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
    c -= Vec3::splat(offset);
    let peak = c.max_element();
    if peak < start {
        return c.clamp(Vec3::ZERO, Vec3::ONE);
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    c.lerp(Vec3::splat(new_peak), g)
        .clamp(Vec3::ZERO, Vec3::ONE)
}

/// Linear-RGB gains of a white balance (temperature, tint in -100..100): the chromaticity of
/// a daylight-like illuminant is shifted along the Planckian direction (temperature) and
/// green–magenta (tint), and the von Kries ratio to D65 in LMS space is applied to white.
/// (0, 0) gives exactly 1.
pub fn white_balance(temperature: f32, tint: f32) -> Vec3 {
    if temperature == 0.0 && tint == 0.0 {
        return Vec3::ONE;
    }
    let t1 = temperature.clamp(-100.0, 100.0) / 65.0;
    let t2 = tint.clamp(-100.0, 100.0) / 65.0;
    // Illuminant chromaticity: warmer temperature = a *cooler* source to correct for, so
    // the image warms up.
    let x = 0.31271 - t1 * if t1 < 0.0 { 0.1 } else { 0.05 };
    let y = 2.87 * x - 3.0 * x * x - 0.275_095_07 + t2 * 0.05;
    let xyz = |x: f32, y: f32| Vec3::new(x / y, 1.0, (1.0 - x - y) / y);
    // CAT02-like XYZ → LMS.
    let to_lms = glam::Mat3::from_cols(
        Vec3::new(0.7328, -0.7036, 0.0030),
        Vec3::new(0.4296, 1.6975, 0.0136),
        Vec3::new(-0.1624, 0.0061, 0.9834),
    );
    // Linear sRGB ↔ XYZ (D65).
    let rgb_to_xyz = glam::Mat3::from_cols(
        Vec3::new(0.4124, 0.2126, 0.0193),
        Vec3::new(0.3576, 0.7152, 0.1192),
        Vec3::new(0.1805, 0.0722, 0.9505),
    );
    let d65 = to_lms * xyz(0.31271, 0.32902);
    let src = to_lms * xyz(x, y);
    let gain = d65 / src;
    let rgb_to_lms = to_lms * rgb_to_xyz;
    let w = rgb_to_lms.inverse() * (gain * (rgb_to_lms * Vec3::ONE));
    let w = w.max(Vec3::splat(0.01));
    // Keep brightness: normalise so white's luminance is unchanged.
    w / luminance(w)
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
        TonemapMode::Agx => tonemap_agx(c, s.tonemap_look),
        TonemapMode::PbrNeutral => tonemap_pbr_neutral(c),
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
    fn agx_and_pbr_neutral_are_monotonic_and_bounded() {
        let agx = |c| tonemap_agx(c, TonemapLook::Base);
        let punchy = |c| tonemap_agx(c, TonemapLook::Punchy);
        for f in [&agx as &dyn Fn(Vec3) -> Vec3, &punchy, &tonemap_pbr_neutral] {
            assert!(f(Vec3::ZERO).max_element() < 0.01);
            let mut last = -1.0;
            for i in 0..400 {
                let v = f(Vec3::splat(i as f32 * 0.05)).x;
                assert!(v >= last - 1e-5 && v <= 1.0, "{i}: {v}");
                last = v;
            }
            assert!(f(Vec3::splat(40.0)).x > 0.9);
        }
        // Mid grey stays mid grey-ish; PBR Neutral keeps it nearly exact.
        let g = agx(Vec3::splat(0.18)).x;
        assert!((0.13..0.25).contains(&g), "{g}");
        assert!((tonemap_pbr_neutral(Vec3::splat(0.18)).x - 0.14).abs() < 0.01);
        // Punchy is darker in the shadows and keeps more saturation than Base.
        assert!(punchy(Vec3::splat(0.05)).x < agx(Vec3::splat(0.05)).x);
        let red = Vec3::new(0.5, 0.05, 0.05);
        let sat = |c: Vec3| (c.x - c.z) / c.x.max(1e-4);
        assert!(sat(punchy(red)) > sat(agx(red)));
        // A very bright saturated colour goes towards white (AgX's signature).
        let hot = agx(Vec3::new(60.0, 6.0, 1.0));
        assert!(hot.z > 0.4, "{hot}");
        // AgX neutral greys stay neutral.
        let n = agx(Vec3::splat(0.7));
        assert!((n.x - n.y).abs() < 1e-3 && (n.y - n.z).abs() < 1e-3, "{n}");
    }

    #[test]
    fn white_balance_warms_and_cools() {
        assert_eq!(white_balance(0.0, 0.0), Vec3::ONE);
        let warm = white_balance(40.0, 0.0);
        let cool = white_balance(-40.0, 0.0);
        assert!(warm.x > 1.0 && warm.z < 1.0, "{warm}");
        assert!(cool.x < 1.0 && cool.z > 1.0, "{cool}");
        let magenta = white_balance(0.0, 40.0);
        assert!(magenta.y < magenta.x && magenta.y < magenta.z, "{magenta}");
        for w in [warm, cool, magenta] {
            assert!((luminance(w) - 1.0).abs() < 1e-4);
        }
        // Folded into the packed colour filter, next to the AgX look flag.
        let s = PostSettings {
            temperature: 40.0,
            tonemap: TonemapMode::Agx,
            tonemap_look: TonemapLook::Punchy,
            ..Default::default()
        };
        let p = s.pack();
        assert_eq!(p[0][0], 3.0);
        assert_eq!(p[1][3], 1.0);
        assert!((p[1][0] - warm.x).abs() < 1e-5);
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
