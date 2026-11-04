//! Color Harmony: pulls every layer's average colour towards one shared tone so layers from
//! different sources look like one landscape. Analysis happens on the CPU (mean linear albedo
//! per layer, splat coverage); the result is a per-layer hue shift and saturation / brightness
//! scale applied in the terrain shader. Source textures are never modified.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::data::{MAX_LAYERS, TerrainData};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarmonyTarget {
    /// Coverage-weighted mean of the layers' own colours.
    #[default]
    Average,
    /// The hue of `tint`.
    Tint,
    /// The hue of the sun and ambient light.
    Lighting,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HarmonySettings {
    pub enabled: bool,
    pub target: HarmonyTarget,
    /// sRGB tint for [`HarmonyTarget::Tint`].
    pub tint: [f32; 3],
    /// 0..1 overall amount.
    pub strength: f32,
    /// Weights of the hue, saturation and brightness shifts (0..1 each).
    pub hue: f32,
    pub saturation: f32,
    pub brightness: f32,
    /// Weight layers by how much of the terrain they cover (else equally).
    pub weight_by_coverage: bool,
    /// Editor preview: show the unadjusted colours (not saved).
    #[serde(skip)]
    pub preview_original: bool,
}

impl Default for HarmonySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            target: HarmonyTarget::Average,
            tint: [0.62, 0.58, 0.42],
            strength: 0.5,
            hue: 0.5,
            saturation: 0.7,
            brightness: 0.3,
            weight_by_coverage: true,
            preview_original: false,
        }
    }
}

/// Linear RGB → HSV (h in turns 0..1).
pub fn rgb_to_hsv(c: Vec3) -> Vec3 {
    let max = c.max_element();
    let min = c.min_element();
    let delta = max - min;
    let h = if delta <= 1e-6 {
        0.0
    } else if max == c.x {
        ((c.y - c.z) / delta).rem_euclid(6.0) / 6.0
    } else if max == c.y {
        ((c.z - c.x) / delta + 2.0) / 6.0
    } else {
        ((c.x - c.y) / delta + 4.0) / 6.0
    };
    let s = if max <= 1e-6 { 0.0 } else { delta / max };
    Vec3::new(h, s, max)
}

pub fn hsv_to_rgb(hsv: Vec3) -> Vec3 {
    let h = hsv.x.rem_euclid(1.0) * 6.0;
    let k = |n: f32| {
        let k = (n + h) % 6.0;
        hsv.z - hsv.z * hsv.y * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    Vec3::new(k(5.0), k(3.0), k(1.0))
}

/// One layer as the analysis sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerTone {
    /// Mean linear albedo.
    pub mean: Vec3,
    /// Weight in the shared target (coverage, or 1).
    pub weight: f32,
}

/// Per-layer shader adjustment: x = hue shift (turns), y = saturation scale,
/// z = brightness scale, w = 1 when it does anything.
pub const IDENTITY: [f32; 4] = [0.0, 1.0, 1.0, 0.0];

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Mean linear colour of RGBA8 sRGB pixels (strided to at most ~65k samples).
pub fn mean_linear(rgba: &[u8]) -> Vec3 {
    let lut: [f32; 256] = std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0));
    let pixels = rgba.len() / 4;
    let stride = (pixels / 65_536).max(1);
    let (mut sum, mut n) = (Vec3::ZERO, 0.0);
    for p in rgba.as_chunks::<4>().0.iter().step_by(stride) {
        sum += Vec3::new(lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize]);
        n += 1.0;
    }
    if n > 0.0 { sum / n } else { Vec3::splat(0.5) }
}

/// Share of the terrain each layer covers (sums to 1 over painted layers).
pub fn coverage(d: &TerrainData) -> [f32; MAX_LAYERS] {
    let stride = (d.alphamap.len() / 65_536).max(1);
    let mut sum = [0.0f32; MAX_LAYERS];
    for px in d.alphamap.iter().step_by(stride) {
        for (s, v) in sum.iter_mut().zip(px) {
            *s += *v as f32;
        }
    }
    let total: f32 = sum.iter().sum::<f32>().max(1e-6);
    sum.map(|s| s / total)
}

/// Shortest signed hue difference `to - from` in turns (-0.5..0.5).
fn hue_delta(from: f32, to: f32) -> f32 {
    (to - from + 0.5).rem_euclid(1.0) - 0.5
}

/// The shared target tone (HSV). Hue is a circular mean weighted by weight × saturation
/// (greys have no hue); Tint / Lighting take only the hue from that colour.
pub fn target(s: &HarmonySettings, tones: &[LayerTone], light: Option<Vec3>) -> Vec3 {
    let (mut hx, mut hy, mut sat, mut val, mut w) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for t in tones {
        let hsv = rgb_to_hsv(t.mean);
        let a = hsv.x * std::f32::consts::TAU;
        hx += a.cos() * t.weight * hsv.y;
        hy += a.sin() * t.weight * hsv.y;
        sat += hsv.y * t.weight;
        val += hsv.z * t.weight;
        w += t.weight;
    }
    let w = w.max(1e-6);
    let mean_hue = hy.atan2(hx).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
    let hue = match s.target {
        HarmonyTarget::Average => mean_hue,
        HarmonyTarget::Tint => rgb_to_hsv(Vec3::from(s.tint.map(srgb_to_linear))).x,
        HarmonyTarget::Lighting => light.map(|l| rgb_to_hsv(l).x).unwrap_or(mean_hue),
    };
    Vec3::new(hue, sat / w, val / w)
}

/// Per-layer adjustments moving each tone towards the target by the strength and weights.
pub fn adjustments(
    s: &HarmonySettings,
    tones: &[LayerTone],
    light: Option<Vec3>,
) -> [[f32; 4]; MAX_LAYERS] {
    let mut out = [IDENTITY; MAX_LAYERS];
    let k = s.strength.clamp(0.0, 1.0);
    if !s.enabled || s.preview_original || k == 0.0 || tones.is_empty() {
        return out;
    }
    let goal = target(s, tones, light);
    for (o, t) in out.iter_mut().zip(tones) {
        let hsv = rgb_to_hsv(t.mean);
        let shift = hue_delta(hsv.x, goal.x) * k * s.hue.clamp(0.0, 1.0);
        let sat = hsv.y + (goal.y - hsv.y) * k * s.saturation.clamp(0.0, 1.0);
        let val = hsv.z + (goal.z - hsv.z) * k * s.brightness.clamp(0.0, 1.0);
        *o = [
            shift,
            if hsv.y > 1e-4 { sat / hsv.y } else { 1.0 },
            if hsv.z > 1e-4 { val / hsv.z } else { 1.0 },
            1.0,
        ];
    }
    out
}

/// CPU mirror of the shader's adjustment of one linear colour.
pub fn apply(c: Vec3, adjust: [f32; 4]) -> Vec3 {
    if adjust[3] == 0.0 {
        return c;
    }
    let hsv = rgb_to_hsv(c);
    hsv_to_rgb(Vec3::new(
        hsv.x + adjust[0],
        (hsv.y * adjust[1]).clamp(0.0, 1.0),
        hsv.z * adjust[2],
    ))
}

/// Tones of a terrain's layers from their mean colours (`means[i]` = None: grey).
pub fn tones(d: &TerrainData, means: &[Option<Vec3>]) -> Vec<LayerTone> {
    let cover = coverage(d);
    let count = d.layers.len().min(MAX_LAYERS);
    (0..count)
        .map(|i| LayerTone {
            mean: means.get(i).copied().flatten().unwrap_or(Vec3::splat(0.5)),
            weight: if d.harmony.weight_by_coverage {
                cover[i]
            } else {
                1.0
            },
        })
        .collect()
}

type MeanCache = HashMap<String, (Option<SystemTime>, Option<Vec3>)>;
static MEANS: LazyLock<Mutex<MeanCache>> = LazyLock::new(Default::default);

/// Mean linear colour of a project texture (cached until the file changes).
pub fn layer_mean(rel: &str) -> Option<Vec3> {
    let abs = crate::assets::absolute(rel);
    let stamp = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
    let mut cache = MEANS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, m)) = cache.get(rel)
        && *s == stamp
    {
        return *m;
    }
    let mean = image::open(&abs)
        .ok()
        .map(|img| mean_linear(img.into_rgba8().as_raw()));
    cache.insert(rel.to_owned(), (stamp, mean));
    mean
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> HarmonySettings {
        HarmonySettings {
            enabled: true,
            strength: 1.0,
            hue: 1.0,
            saturation: 1.0,
            brightness: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn hsv_round_trips() {
        for c in [
            Vec3::new(0.2, 0.5, 0.1),
            Vec3::new(0.9, 0.1, 0.4),
            Vec3::new(0.05, 0.05, 0.6),
            Vec3::splat(0.3),
        ] {
            assert!(hsv_to_rgb(rgb_to_hsv(c)).distance(c) < 1e-5, "{c}");
        }
        assert!((rgb_to_hsv(Vec3::new(1.0, 0.0, 0.0)).x).abs() < 1e-6);
        assert!((rgb_to_hsv(Vec3::new(0.0, 1.0, 0.0)).x - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn mean_is_taken_in_linear_light() {
        // Half black, half white (sRGB) averages to 0.5 linear, not sRGB 0.5 (= 0.214).
        let px: Vec<u8> = (0..64)
            .flat_map(|i| if i % 2 == 0 { [0, 0, 0, 255] } else { [255; 4] })
            .collect();
        assert!((mean_linear(&px) - Vec3::splat(0.5)).length() < 1e-5);
    }

    #[test]
    fn target_is_the_weighted_circular_mean() {
        let red = Vec3::new(0.6, 0.1, 0.1);
        let yellow = Vec3::new(0.6, 0.6, 0.1);
        let tones = [
            LayerTone {
                mean: red,
                weight: 1.0,
            },
            LayerTone {
                mean: yellow,
                weight: 1.0,
            },
        ];
        let t = target(&on(), &tones, None);
        assert!(
            (t.x - 1.0 / 12.0).abs() < 1e-3,
            "orange between them: {}",
            t.x
        );
        // Coverage pulls the hue towards the bigger layer.
        let tones = [
            LayerTone {
                mean: red,
                weight: 3.0,
            },
            LayerTone {
                mean: yellow,
                weight: 1.0,
            },
        ];
        assert!(target(&on(), &tones, None).x < 1.0 / 12.0);
        // Hues wrap: magenta-red and orange-red meet at red, not at cyan.
        let tones = [
            LayerTone {
                mean: hsv_to_rgb(Vec3::new(0.95, 0.8, 0.5)),
                weight: 1.0,
            },
            LayerTone {
                mean: hsv_to_rgb(Vec3::new(0.05, 0.8, 0.5)),
                weight: 1.0,
            },
        ];
        let h = target(&on(), &tones, None).x;
        assert!(!(0.02..0.98).contains(&h), "{h}");
    }

    #[test]
    fn full_strength_reaches_the_target_and_zero_is_identity() {
        let tones = [
            LayerTone {
                mean: Vec3::new(0.2, 0.4, 0.05),
                weight: 0.7,
            },
            LayerTone {
                mean: Vec3::new(0.35, 0.3, 0.25),
                weight: 0.3,
            },
        ];
        let s = on();
        let goal = target(&s, &tones, None);
        let adj = adjustments(&s, &tones, None);
        for (t, a) in tones.iter().zip(adj) {
            let out = rgb_to_hsv(apply(t.mean, a));
            assert!((out - goal).abs().max_element() < 1e-3, "{out} vs {goal}");
        }
        // Strength 0: nothing changes, bit for bit.
        let off = HarmonySettings {
            strength: 0.0,
            ..on()
        };
        let adj = adjustments(&off, &tones, None);
        assert!(adj.iter().all(|a| *a == IDENTITY));
        for c in [Vec3::new(0.123, 0.456, 0.789), Vec3::splat(0.2)] {
            assert_eq!(apply(c, adj[0]), c);
        }
        // Preview Before shows the originals too.
        let before = HarmonySettings {
            preview_original: true,
            ..on()
        };
        assert!(
            adjustments(&before, &tones, None)
                .iter()
                .all(|a| *a == IDENTITY)
        );
    }

    #[test]
    fn tint_and_lighting_set_only_the_hue() {
        let tones = [LayerTone {
            mean: Vec3::new(0.2, 0.4, 0.05),
            weight: 1.0,
        }];
        let tint = HarmonySettings {
            target: HarmonyTarget::Tint,
            tint: [0.0, 0.0, 1.0],
            ..on()
        };
        let t = target(&tint, &tones, None);
        assert!((t.x - 2.0 / 3.0).abs() < 1e-4);
        let orig = rgb_to_hsv(tones[0].mean);
        assert!((t.y - orig.y).abs() < 1e-5 && (t.z - orig.z).abs() < 1e-5);
        let light = HarmonySettings {
            target: HarmonyTarget::Lighting,
            ..on()
        };
        let warm = Vec3::new(1.0, 0.5, 0.0);
        assert!((target(&light, &tones, Some(warm)).x - 1.0 / 12.0).abs() < 1e-4);
    }
}
