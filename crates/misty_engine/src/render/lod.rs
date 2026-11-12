//! LOD selection: which level of a LOD Group each camera draws, from the group's height on
//! screen (× the LOD Bias), with an optional dithered cross-fade between neighbouring levels.
//!
//! Cross-fades (LOD Groups, terrain trees and their impostors, detail meshes at their draw
//! distance): inside a band at each switch both levels draw; the outgoing one drops the pixels
//! whose dither threshold is at or above its fade value, the incoming one keeps exactly those
//! (`lod_dithered_out` in common.wgsl, mirrored by [`dithered_out`]). The two always cover the
//! surface once, with no blending or sorting, and only objects inside a band draw twice. The
//! same per-draw fade goes to the shadow casters, so shadows switch with what the camera sees.

use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;

use crate::camera::{Camera, Projection};

/// 4 × 4 ordered (Bayer) dither ranks, row by row (`lod_dither` in common.wgsl).
pub const BAYER4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Dither threshold (0..1) of pixel `p` in frame phase `phase` (0..7: TAA's jitter phase,
/// each step adds 1/8 to every threshold; `lod_dither` in common.wgsl).
pub fn dither(p: [u32; 2], phase: u32) -> f32 {
    let (x, y) = (p[0] % 4, p[1] % 4);
    let t = (BAYER4[(y * 4 + x) as usize] as f32 + 0.5) / 16.0 + (phase % 8) as f32 / 8.0;
    t.fract()
}

/// Is a pixel of dither threshold `t` dropped by a draw of fade value `fade`? A fade of 0 is
/// drawn whole; a positive one fades out and keeps `t < fade`; a negative one fades in and
/// keeps `t >= 1 + fade` (`lod_dithered_out` in common.wgsl).
pub fn dithered_out(fade: f32, t: f32) -> bool {
    if fade == 0.0 {
        false
    } else if fade > 0.0 {
        t >= fade
    } else {
        t < 1.0 + fade
    }
}

/// Cross-fades on (the default). Off: every switch is instant, as before cross-fades (the
/// before / after harnesses; `MISTY_LOD_FADE=0` in the environment).
static FADES: AtomicBool = AtomicBool::new(true);

pub fn set_fades(on: bool) {
    FADES.store(on, Ordering::Relaxed);
}

pub fn fades() -> bool {
    static ENV: std::sync::Once = std::sync::Once::new();
    ENV.call_once(|| {
        if std::env::var("MISTY_LOD_FADE").is_ok_and(|v| v == "0") {
            FADES.store(false, Ordering::Relaxed);
        }
    });
    FADES.load(Ordering::Relaxed)
}

/// Share (0..1) of the way from one level to the next at distance `d` for a switch at `edge`
/// with a cross-fade band of `width` × `edge` centred on it (0 before the band, 1 past it; a
/// step at `edge` when `width` is 0). Terrain trees: trees.rs and `fade_at` in
/// trees_cull.wgsl.
pub fn distance_fade(d: f32, edge: f32, width: f32) -> f32 {
    let half = width.max(0.0) * 0.5 * edge;
    if half <= 0.0 {
        return if d >= edge { 1.0 } else { 0.0 };
    }
    ((d - (edge - half)) / (2.0 * half)).clamp(0.0, 1.0)
}

/// A draw's place in its LOD Group (set by the scene gather, chosen per camera by the renderer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodRange {
    /// World centre and size (largest dimension) of the group's bounds.
    pub center: Vec3,
    pub size: f32,
    /// This level draws while the screen-relative height `h` is in `min..max`
    /// (`max` = infinity for LOD 0; `min` = 0 when the last level is never culled).
    pub min: f32,
    pub max: f32,
    /// Cross-fade band above each threshold, as a fraction of it (0 = switch instantly).
    pub fade: f32,
    /// Set when the group's LOD 0 is a virtual geometry mesh (see [`VirtualLod`]).
    pub virtual_lod: Option<VirtualLod>,
}

/// A LOD Group whose LOD 0 is a virtual geometry mesh: while virtual geometry draws that mesh
/// (its clusters pick their own level of detail), LOD 0 draws at every distance down to the
/// group's cull height and the other levels are skipped, like Nanite meshes ignore their LODs.
/// Without virtual geometry (Quality setting off, unsupported, or the build not ready yet) the
/// group switches levels as usual.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualLod {
    /// Tells the levels of one group apart within a frame.
    pub group: u32,
    /// This draw belongs to LOD 0.
    pub first: bool,
    /// The group's lowest threshold (0 = never culled).
    pub cull: f32,
}

/// What a camera does with one draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LodState {
    Hidden,
    Visible,
    /// Dithered: the per-draw fade value (see `lod_dithered_out` in draw.wgsl).
    Fading(f32),
}

/// Height of a box of `size` at `center` as a fraction of the screen height (× `bias`).
pub fn screen_height(camera: &Camera, center: Vec3, size: f32, bias: f32) -> f32 {
    let h = match camera.projection {
        Projection::Perspective { fov_y } => {
            let d = (center - camera.position).length().max(1e-4);
            size / (2.0 * d * (fov_y.to_radians() * 0.5).tan())
        }
        Projection::Orthographic { size: half } => size / (2.0 * half.max(1e-4)),
    };
    h * bias
}

impl LodRange {
    /// The state at screen height `h` when the group's LOD 0 draws through virtual geometry.
    pub fn virtual_state(&self, v: VirtualLod, h: f32) -> LodState {
        if !v.first || h < v.cull {
            return LodState::Hidden;
        }
        // Fades out to nothing just above the cull height (the dithered draw leaves the
        // cluster path for that band).
        let band = if fades() {
            v.cull * self.fade.max(0.0)
        } else {
            0.0
        };
        if band > 0.0 && h < v.cull + band {
            let keep = (h - v.cull) / band;
            return if keep <= 0.0 {
                LodState::Hidden
            } else {
                LodState::Fading(keep)
            };
        }
        LodState::Visible
    }

    /// The level's state at screen height `h`.
    pub fn state(&self, h: f32) -> LodState {
        let w = if fades() { self.fade.max(0.0) } else { 0.0 };
        if h < self.min {
            return LodState::Hidden;
        }
        if h >= self.max {
            // Just above the threshold the finer level fades out: fade this one in.
            let band = self.max * w;
            if band > 0.0 && h < self.max + band {
                let keep_finer = (h - self.max) / band;
                return LodState::Fading(keep_finer - 1.0);
            }
            return LodState::Hidden;
        }
        let band = self.min * w;
        if band > 0.0 && h < self.min + band {
            let keep = (h - self.min) / band;
            if keep <= 0.0 {
                return LodState::Hidden;
            }
            return LodState::Fading(keep);
        }
        LodState::Visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn camera(z: f32) -> Camera {
        Camera {
            position: Vec3::new(0.0, 0.0, z),
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 1000.0,
        }
    }

    #[test]
    fn heights_and_bias() {
        let tan = 30f32.to_radians().tan();
        let h = screen_height(&camera(-10.0), Vec3::ZERO, 2.0, 1.0);
        assert!((h - 2.0 / (20.0 * tan)).abs() < 1e-6);
        assert!((screen_height(&camera(-10.0), Vec3::ZERO, 2.0, 2.0) - 2.0 * h).abs() < 1e-6);
        let mut ortho = camera(-10.0);
        ortho.projection = Projection::Orthographic { size: 5.0 };
        assert_eq!(screen_height(&ortho, Vec3::ZERO, 2.0, 1.0), 0.2);
    }

    #[test]
    fn levels_switch_and_cross_fade_cover_each_pixel_once() {
        let lod0 = LodRange {
            center: Vec3::ZERO,
            size: 1.0,
            min: 0.5,
            max: f32::INFINITY,
            fade: 0.0,
            virtual_lod: None,
        };
        let lod1 = LodRange {
            min: 0.1,
            max: 0.5,
            ..lod0
        };
        assert_eq!(lod0.state(0.6), LodState::Visible);
        assert_eq!(lod1.state(0.6), LodState::Hidden);
        assert_eq!(lod0.state(0.3), LodState::Hidden);
        assert_eq!(lod1.state(0.3), LodState::Visible);
        assert_eq!(lod1.state(0.05), LodState::Hidden, "culled");

        let (f0, f1) = (
            LodRange { fade: 0.2, ..lod0 },
            LodRange { fade: 0.2, ..lod1 },
        );
        // Inside the band above 0.5 both draw, with complementary dithers.
        let (LodState::Fading(a), LodState::Fading(b)) = (f0.state(0.55), f1.state(0.55)) else {
            panic!("{:?} {:?}", f0.state(0.55), f1.state(0.55));
        };
        assert!(a > 0.0 && b < 0.0 && (a - (1.0 + b)).abs() < 1e-6);
        assert_eq!(f0.state(0.65), LodState::Visible);
        assert_eq!(f1.state(0.65), LodState::Hidden);
        // The last level fades out to nothing above its cull height.
        assert!(matches!(f1.state(0.11), LodState::Fading(k) if k > 0.0 && k < 1.0));
    }

    #[test]
    fn virtual_lod_zero_draws_at_every_distance_down_to_the_cull_height() {
        let v = VirtualLod {
            group: 1,
            first: true,
            cull: 0.01,
        };
        let lod0 = LodRange {
            center: Vec3::ZERO,
            size: 1.0,
            min: 0.5,
            max: f32::INFINITY,
            fade: 0.0,
            virtual_lod: Some(v),
        };
        let lod1 = LodRange {
            min: 0.01,
            max: 0.5,
            ..lod0
        };
        let rest = VirtualLod { first: false, ..v };
        for h in [2.0, 0.6, 0.3, 0.02] {
            assert_eq!(lod0.virtual_state(v, h), LodState::Visible, "{h}");
            assert_eq!(lod1.virtual_state(rest, h), LodState::Hidden, "{h}");
        }
        assert_eq!(lod0.virtual_state(v, 0.005), LodState::Hidden, "culled");
        // With a fade band it thins out to nothing above the cull height instead.
        let faded = LodRange { fade: 0.5, ..lod0 };
        assert!(matches!(faded.virtual_state(v, 0.012), LodState::Fading(k) if k > 0.0 && k < 1.0));
        assert_eq!(faded.virtual_state(v, 0.02), LodState::Visible);
        assert_eq!(faded.virtual_state(v, 0.005), LodState::Hidden);
    }

    fn fade_of(s: LodState) -> Option<f32> {
        match s {
            LodState::Hidden => None,
            LodState::Visible => Some(0.0),
            LodState::Fading(f) => Some(f),
        }
    }

    /// Pixels of a 4 × 4 tile (in frame phase `phase`) a draw of state `s` keeps.
    fn kept(s: LodState, phase: u32) -> [bool; 16] {
        let mut out = [false; 16];
        if let Some(f) = fade_of(s) {
            for (i, k) in out.iter_mut().enumerate() {
                let p = [i as u32 % 4, i as u32 / 4];
                *k = !dithered_out(f, dither(p, phase));
            }
        }
        out
    }

    /// LOD selection with fade bands: walking a three-level group from close up to culled,
    /// every screen height has each pixel drawn by exactly one level (two inside a band, with
    /// complementary dithers; one elsewhere), and the share of the finer level falls steadily
    /// through each band.
    #[test]
    fn bands_select_one_or_two_levels_and_cover_every_pixel_once() {
        let base = LodRange {
            center: Vec3::ZERO,
            size: 1.0,
            min: 0.0,
            max: 0.0,
            fade: 0.1,
            virtual_lod: None,
        };
        let levels = [
            LodRange {
                min: 0.5,
                max: f32::INFINITY,
                ..base
            },
            LodRange {
                min: 0.2,
                max: 0.5,
                ..base
            },
            LodRange {
                min: 0.05,
                max: 0.2,
                ..base
            },
        ];
        let mut h = 1.0f32;
        let mut last_share = 16usize;
        while h > 0.01 {
            let states: Vec<LodState> = levels.iter().map(|l| l.state(h)).collect();
            let drawn = states.iter().filter(|s| **s != LodState::Hidden).count();
            let in_band = |t: f32| h >= t && h < t * 1.1;
            let culled = h < 0.05;
            let expect = if [0.5, 0.2].into_iter().any(in_band) {
                2
            } else if culled {
                0
            } else {
                1
            };
            assert_eq!(drawn, expect, "h {h}: {states:?}");
            for phase in [0, 3, 6] {
                let mut count = [0u8; 16];
                for s in &states {
                    for (c, k) in count.iter_mut().zip(kept(*s, phase)) {
                        *c += k as u8;
                    }
                }
                let fading_out = in_band(0.05);
                for c in count {
                    if culled {
                        assert_eq!(c, 0, "h {h}");
                    } else if fading_out {
                        assert!(c <= 1, "h {h}");
                    } else {
                        assert_eq!(c, 1, "h {h}: {states:?}");
                    }
                }
            }
            // Moving away, LOD 0's share of the pixels only falls.
            let share = kept(states[0], 0).iter().filter(|k| **k).count();
            assert!(share <= last_share, "h {h}: {share} after {last_share}");
            last_share = share;
            h *= 0.995;
        }
    }

    /// The outgoing level's pattern and the incoming one's are complements: at every fade
    /// value, frame phase and pixel exactly one of them keeps the pixel, and the outgoing level
    /// keeps about `fade` of the pixels.
    #[test]
    fn complementary_dithers_sum_to_full_coverage() {
        for step in 1..64 {
            let keep = step as f32 / 64.0;
            for phase in 0..8 {
                let mut out_kept = 0;
                for i in 0..16u32 {
                    let t = dither([i % 4, i / 4], phase);
                    let a = !dithered_out(keep, t);
                    let b = !dithered_out(keep - 1.0, t);
                    assert!(a ^ b, "fade {keep}, pixel {i}, phase {phase}");
                    out_kept += a as u32;
                }
                assert!((out_kept as f32 / 16.0 - keep).abs() <= 1.0 / 16.0 + 1e-6);
            }
        }
        // Every threshold of the tile is distinct: the pattern has 16 levels.
        let mut ranks = BAYER4;
        ranks.sort();
        assert_eq!(ranks, std::array::from_fn(|i| i as u8));
        // Not fading: drawn whole.
        assert!((0..16).all(|i| !dithered_out(0.0, dither([i % 4, i / 4], 0))));
    }

    /// With TAA every pixel's threshold walks through the jitter cycle: the eight phases the
    /// camera's jitter gives (as the shader derives them) are all different, and over them a
    /// pixel of a half-faded pair shows each level in half the frames, so the history averages
    /// the two instead of keeping a fixed pattern.
    #[test]
    fn the_dither_moves_with_the_taa_jitter() {
        let phases: Vec<u32> = (0..8)
            .map(|n| {
                let j = super::super::taa::jitter_pixels(n, 8).x;
                ((j + 0.5).fract() * 8.0).floor().clamp(0.0, 7.0) as u32
            })
            .collect();
        let mut sorted = phases.clone();
        sorted.sort();
        assert_eq!(sorted, (0..8).collect::<Vec<_>>(), "{phases:?}");
        for i in 0..16u32 {
            let shown = phases
                .iter()
                .filter(|&&k| !dithered_out(0.5, dither([i % 4, i / 4], k)))
                .count();
            assert_eq!(shown, 4, "pixel {i}");
        }
        // Consecutive frames change the pattern.
        for w in phases.windows(2) {
            let a: Vec<bool> = (0..16)
                .map(|i| dithered_out(0.5, dither([i % 4, i / 4], w[0])))
                .collect();
            let b: Vec<bool> = (0..16)
                .map(|i| dithered_out(0.5, dither([i % 4, i / 4], w[1])))
                .collect();
            assert_ne!(a, b, "phases {w:?}");
        }
    }

    /// The shader's dither matrix is the one tested here.
    #[test]
    fn shader_dither_matches_the_tested_one() {
        let src = include_str!("shaders/common.wgsl");
        let start = src
            .find("fn lod_dither")
            .expect("lod_dither in common.wgsl");
        let body = &src[start..];
        let open = body.find("array<f32, 16>(").expect("matrix") + "array<f32, 16>(".len();
        let close = open + body[open..].find(')').unwrap();
        let ranks: Vec<u8> = body[open..close]
            .split(',')
            .map(|v| v.trim().trim_end_matches(".0").parse().unwrap())
            .collect();
        assert_eq!(ranks, BAYER4);
    }

    #[test]
    fn distance_fades_are_centred_on_the_switch() {
        assert_eq!(distance_fade(40.0, 50.0, 0.1), 0.0);
        assert_eq!(distance_fade(50.0, 50.0, 0.1), 0.5);
        assert_eq!(distance_fade(52.5, 50.0, 0.1), 1.0);
        assert!((distance_fade(48.75, 50.0, 0.1) - 0.25).abs() < 1e-6);
        // No band: a step at the switch.
        assert_eq!(distance_fade(49.9, 50.0, 0.0), 0.0);
        assert_eq!(distance_fade(50.0, 50.0, 0.0), 1.0);
    }
}
