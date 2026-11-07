//! LOD selection: which level of a LOD Group each camera draws, from the group's height on
//! screen (× the LOD Bias), with an optional dithered cross-fade between neighbouring levels.

use glam::Vec3;

use crate::camera::{Camera, Projection};

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
        if v.first && h >= v.cull {
            LodState::Visible
        } else {
            LodState::Hidden
        }
    }

    /// The level's state at screen height `h`.
    pub fn state(&self, h: f32) -> LodState {
        let w = self.fade.max(0.0);
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
    }
}
