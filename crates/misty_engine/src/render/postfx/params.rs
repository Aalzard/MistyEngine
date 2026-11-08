//! The post effects' uniform (`FxParams` in shaders/postfx_params.wgsl) and CPU copies of
//! the math in the shaders, for tests.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::camera::{Camera, Projection};
use crate::scene::{
    AdaptationMode, AutoExposureOverride, DepthOfFieldOverride, DofMode, MeteringMode,
    MotionBlurOverride, PostFxQuality,
};

/// Height of the sensor the Bokeh lens images onto (a 36 × 24 mm full-frame sensor).
pub const SENSOR_HEIGHT: f32 = 0.024;
/// Gaussian Max Radius 1 = this many pixels at 1080p.
pub const GAUSSIAN_RADIUS_PX: f32 = 10.0;
/// log2 luminance of the first histogram bin and the range the bins cover.
pub const HISTOGRAM_MIN: f32 = -12.0;
pub const HISTOGRAM_RANGE: f32 = 24.0;
/// Pixels per near / far CoC tile side (half resolution).
pub const DOF_TILE: u32 = 8;
/// Output pixels per motion blur tile side.
pub const MB_TILE: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct FxParams {
    pub inv_view_proj: [[f32; 4]; 4],
    pub prev_view_proj: [[f32; 4]; 4],
    pub camera: [f32; 4],
    pub size: [f32; 4],
    pub dof: [f32; 4],
    pub dof_shape: [f32; 4],
    pub dof_tiles: [f32; 4],
    pub mb: [f32; 4],
    pub ae: [f32; 4],
    pub ae_limits: [f32; 4],
    pub ae_adapt: [f32; 4],
}

/// Largest Bokeh CoC radius in pixels at 1080p for a quality level.
pub fn bokeh_max_radius(q: PostFxQuality) -> f32 {
    match q {
        PostFxQuality::Low => 8.0,
        PostFxQuality::Medium => 12.0,
        PostFxQuality::High => 16.0,
    }
}

/// Gather rings of the Bokeh pass (1 + 4·r·(r + 1) samples: 25, 49, 81).
pub fn bokeh_rings(q: PostFxQuality) -> u32 {
    match q {
        PostFxQuality::Low => 2,
        PostFxQuality::Medium => 3,
        PostFxQuality::High => 4,
    }
}

/// Bokeh CoC scale in output pixels (radius): a surface at distance d has a CoC of
/// `scale · (1 − P / d)` for focus distance P (thin lens, the image on a 24 mm sensor).
pub fn bokeh_scale(d: &DepthOfFieldOverride, out_height: f32) -> f32 {
    let f = d.focal_length.clamp(1.0, 1000.0) / 1000.0;
    let n = d.aperture.clamp(0.7, 64.0);
    let p = d.focus_distance.max(f + 0.01);
    let aperture = f / n;
    // Diameter on the sensor of a point at infinity: A·F / (P − F).
    let diameter = aperture * f / (p - f);
    diameter / SENSOR_HEIGHT * out_height * 0.5
}

/// Largest CoC radius in output pixels (Gaussian: Max Radius; Bokeh: by quality), scaled
/// with the output height so the look holds at any resolution.
pub fn dof_max_radius(d: &DepthOfFieldOverride, out_height: f32) -> f32 {
    let at_1080 = match d.mode {
        DofMode::Gaussian => d.gaussian_max_radius.clamp(0.5, 1.5) * GAUSSIAN_RADIUS_PX,
        DofMode::Bokeh => bokeh_max_radius(d.quality),
    };
    at_1080 * out_height / 1080.0
}

/// CPU copy of `dof_coc` (dof.wgsl): signed CoC radius in output pixels at eye distance `eye`.
#[cfg(test)]
pub fn coc(d: &DepthOfFieldOverride, out_height: f32, eye: f32) -> f32 {
    let r = dof_max_radius(d, out_height);
    match d.mode {
        DofMode::Bokeh => {
            let p = d.focus_distance.max(d.focal_length / 1000.0 + 0.01);
            (bokeh_scale(d, out_height) * (1.0 - p / eye.max(1e-4))).clamp(-r, r)
        }
        DofMode::Gaussian => {
            let span = (d.gaussian_end - d.gaussian_start).max(1e-4);
            ((eye - d.gaussian_start) / span).clamp(0.0, 1.0) * r
        }
    }
}

/// Exposure target (EV) for an average log2 luminance (CPU copy of `cs_average`).
#[cfg(test)]
pub fn target_ev(average_log2: f32, a: &AutoExposureOverride) -> f32 {
    let (lo, hi) = limits(a);
    (average_log2 - 0.18f32.log2()).clamp(lo, hi)
}

/// Exposure multiplier of an adapted EV.
#[cfg(test)]
pub fn exposure_multiplier(ev: f32, a: &AutoExposureOverride) -> f32 {
    (a.compensation - ev).exp2()
}

/// Share of the way to the target taken in `dt` seconds at `speed` (frame-rate independent).
pub fn adapt_share(speed: f32, dt: f32) -> f32 {
    1.0 - (-speed.max(0.0) * dt.max(0.0)).exp()
}

fn limits(a: &AutoExposureOverride) -> (f32, f32) {
    let lo = a.limit_min.clamp(-10.0, 10.0);
    let hi = a.limit_max.clamp(-10.0, 10.0);
    (lo.min(hi), lo.max(hi))
}

/// What one render's post effects use.
pub(crate) struct ParamInput<'a> {
    pub camera: &'a Camera,
    pub aspect: f32,
    pub prev_view_proj: Mat4,
    pub out_size: (u32, u32),
    pub internal_size: (u32, u32),
    pub dof: Option<&'a DepthOfFieldOverride>,
    pub motion_blur: Option<&'a MotionBlurOverride>,
    pub exposure: Option<&'a AutoExposureOverride>,
    /// Seconds since this view's last render (adaptation speed).
    pub dt: f32,
    /// The exposure must start afresh (first frame, camera cut).
    pub reset: bool,
}

pub(crate) fn pack(i: &ParamInput) -> FxParams {
    let cam = i.camera;
    let ortho = matches!(cam.projection, Projection::Orthographic { .. });
    let (ow, oh) = (i.out_size.0 as f32, i.out_size.1 as f32);
    let mut p = FxParams {
        inv_view_proj: cam.view_projection(i.aspect).inverse().to_cols_array_2d(),
        prev_view_proj: i.prev_view_proj.to_cols_array_2d(),
        camera: [cam.near, cam.far, if ortho { 1.0 } else { 0.0 }, 0.0],
        size: [ow, oh, i.internal_size.0 as f32, i.internal_size.1 as f32],
        ..Zeroable::zeroed()
    };
    if let Some(d) = i.dof {
        let r = dof_max_radius(d, oh);
        match d.mode {
            DofMode::Gaussian => {
                p.dof = [1.0, d.gaussian_start, d.gaussian_end, r];
                let taps = if d.high_quality_sampling { 8.0 } else { 4.0 };
                p.dof_shape = [0.0, 1.0, 0.0, taps];
            }
            DofMode::Bokeh => {
                let focus = d.focus_distance.max(d.focal_length / 1000.0 + 0.01);
                p.dof = [2.0, focus, bokeh_scale(d, oh), r];
                p.dof_shape = [
                    d.blade_count.clamp(3, 9) as f32,
                    d.blade_curvature.clamp(0.0, 1.0),
                    d.blade_rotation.to_radians(),
                    bokeh_rings(d.quality) as f32,
                ];
            }
        }
        // Tiles the near CoC must reach across (in half-resolution pixels).
        let reach = (r * 0.5 / DOF_TILE as f32).ceil().max(1.0);
        p.dof_tiles = [reach, 0.0, 0.0, 0.0];
    }
    if let Some(m) = i.motion_blur {
        let camera = if m.camera_motion_blur {
            m.camera_clamp.clamp(0.0, 0.2) * oh
        } else {
            -1.0
        };
        p.mb = [
            m.intensity.clamp(0.0, 1.0),
            m.maximum_velocity.clamp(0.0, 0.2) * oh,
            m.sample_count.clamp(4, 32) as f32,
            camera,
        ];
    }
    if let Some(a) = i.exposure {
        let lo = (a.histogram_low.clamp(0.0, 100.0) / 100.0).min(0.99);
        let hi = (a.histogram_high.clamp(0.0, 100.0) / 100.0).max(lo + 0.01);
        p.ae = [HISTOGRAM_MIN, HISTOGRAM_RANGE, lo, hi];
        let (min, max) = limits(a);
        let metering = match a.metering {
            MeteringMode::Average => 0.0,
            MeteringMode::Spot => 1.0,
            MeteringMode::CenterWeighted => 2.0,
        };
        p.ae_limits = [min, max, a.compensation.clamp(-15.0, 15.0), metering];
        let jump = a.adaptation == AdaptationMode::Fixed || i.reset;
        p.ae_adapt = [
            adapt_share(a.speed_dark_to_light, i.dt),
            adapt_share(a.speed_light_to_dark, i.dt),
            if jump { 1.0 } else { 0.0 },
            0.0,
        ];
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bokeh() -> DepthOfFieldOverride {
        DepthOfFieldOverride {
            active: true,
            mode: DofMode::Bokeh,
            focus_distance: 2.0,
            focal_length: 50.0,
            aperture: 1.4,
            ..Default::default()
        }
    }

    #[test]
    fn bokeh_coc_follows_the_thin_lens() {
        let d = bokeh();
        // In focus at the focus distance, near negative, far positive, growing with distance.
        assert!(coc(&d, 1080.0, 2.0).abs() < 1e-4);
        assert!(coc(&d, 1080.0, 1.0) < 0.0);
        assert!(coc(&d, 1080.0, 4.0) > 0.0);
        assert!(coc(&d, 1080.0, 8.0) > coc(&d, 1080.0, 4.0));
        // 50 mm f/1.4 focused at 2 m: a point at infinity blurs to a 0.915 mm circle on the
        // sensor, 41 px across at 1080p (before the quality cap).
        let r = bokeh_scale(&d, 1080.0);
        assert!((r * 2.0 - 41.2).abs() < 0.5, "{r}");
        assert_eq!(
            coc(&d, 1080.0, 1e6),
            bokeh_max_radius(PostFxQuality::Medium)
        );
        // Stopping down halves the blur per stop of diameter.
        let f28 = DepthOfFieldOverride { aperture: 2.8, ..d };
        assert!((bokeh_scale(&f28, 1080.0) * 2.0 - r).abs() < 1e-3);
        // Twice the resolution, twice the pixels.
        assert!((bokeh_scale(&d, 2160.0) - 2.0 * r).abs() < 1e-3);
    }

    #[test]
    fn gaussian_coc_ramps_between_start_and_end() {
        let d = DepthOfFieldOverride {
            active: true,
            gaussian_start: 10.0,
            gaussian_end: 20.0,
            ..Default::default()
        };
        assert_eq!(coc(&d, 1080.0, 5.0), 0.0);
        assert_eq!(coc(&d, 1080.0, 15.0), 5.0);
        assert_eq!(coc(&d, 1080.0, 50.0), 10.0);
        assert_eq!(coc(&d, 540.0, 50.0), 5.0);
    }

    #[test]
    fn exposure_targets_mid_grey_within_the_limits() {
        let a = AutoExposureOverride {
            active: true,
            ..Default::default()
        };
        let grey = 0.18f32.log2();
        assert!(target_ev(grey, &a).abs() < 1e-6);
        assert!((exposure_multiplier(0.0, &a) - 1.0).abs() < 1e-6);
        // A scene two stops brighter is darkened two stops; past the limits it stops.
        assert!((target_ev(grey + 2.0, &a) - 2.0).abs() < 1e-5);
        assert!((exposure_multiplier(2.0, &a) - 0.25).abs() < 1e-6);
        assert_eq!(target_ev(grey + 9.0, &a), 3.0);
        assert_eq!(target_ev(grey - 9.0, &a), -3.0);
        let brighter = AutoExposureOverride {
            compensation: 1.0,
            ..a
        };
        assert!((exposure_multiplier(0.0, &brighter) - 2.0).abs() < 1e-6);
        // Adaptation is frame-rate independent: two half steps make one whole.
        let one = adapt_share(3.0, 0.1);
        let half = adapt_share(3.0, 0.05);
        assert!(((1.0 - half) * (1.0 - half) - (1.0 - one)).abs() < 1e-6);
        assert_eq!(adapt_share(3.0, 0.0), 0.0);
    }

    #[test]
    fn params_pack_only_what_is_on() {
        let cam = Camera {
            position: glam::Vec3::ZERO,
            rotation: glam::Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 500.0,
        };
        let input = |dof, motion_blur, exposure| ParamInput {
            camera: &cam,
            aspect: 16.0 / 9.0,
            prev_view_proj: Mat4::IDENTITY,
            out_size: (1920, 1080),
            internal_size: (960, 540),
            dof,
            motion_blur,
            exposure,
            dt: 1.0 / 60.0,
            reset: false,
        };
        let none = pack(&input(None, None, None));
        assert_eq!(none.dof, [0.0; 4]);
        assert_eq!(none.mb, [0.0; 4]);
        assert_eq!(none.size, [1920.0, 1080.0, 960.0, 540.0]);
        let d = bokeh();
        let m = MotionBlurOverride {
            active: true,
            camera_motion_blur: false,
            ..Default::default()
        };
        let a = AutoExposureOverride {
            active: true,
            adaptation: AdaptationMode::Fixed,
            ..Default::default()
        };
        let all = pack(&input(Some(&d), Some(&m), Some(&a)));
        assert_eq!(all.dof[0], 2.0);
        assert_eq!(all.dof_shape[3], 3.0);
        assert!(all.dof_tiles[0] >= 1.0);
        assert_eq!(all.mb[0], 0.5);
        assert!((all.mb[1] - 54.0).abs() < 1e-3);
        assert!(all.mb[3] < 0.0, "camera blur off");
        assert_eq!(all.ae_adapt[2], 1.0, "fixed adaptation jumps");
        assert_eq!(all.ae[2], 0.4);
        assert_eq!(std::mem::size_of::<FxParams>(), 128 + 16 * 9);
    }
}
