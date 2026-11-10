//! What the renderer draws of the weather this frame ([`WeatherFrame`]), built from the
//! scene's [`super::WeatherWorld`], and the weather's changes to the post-processing (cloud
//! cover, fog).

use std::sync::Arc;

use glam::{Mat4, Vec3};

use super::component::{Coverage, SurfaceSettings, WeatherEffects};
use super::lightning::Bolt;
use super::state::{Layer, WeatherWorld};
use super::WeatherParams;
use crate::scene::{CloudsOverride, Scene};

/// Area volumes the renderer blends per pixel (the ones with the most weight near the
/// camera; others are left out).
pub const MAX_REGIONS: usize = 4;

/// What a layer puts down where it applies.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Amounts {
    /// Drops per m³.
    pub rain: f32,
    /// Flakes per m³.
    pub snow: f32,
    pub wetness: f32,
    pub puddles: f32,
    pub snow_cover: f32,
    /// × splash rate.
    pub splashes: f32,
}

impl Amounts {
    fn of(l: &Layer) -> Self {
        Self {
            rain: l.params.rain_density,
            snow: l.params.snow_density,
            wetness: l.wetness,
            puddles: l.puddles,
            snow_cover: l.snow_cover,
            splashes: l.params.splashes,
        }
    }

    fn lerp(&self, b: &Self, t: f32) -> Self {
        let m = |a: f32, b: f32| a + (b - a) * t;
        Self {
            rain: m(self.rain, b.rain),
            snow: m(self.snow, b.snow),
            wetness: m(self.wetness, b.wetness),
            puddles: m(self.puddles, b.puddles),
            snow_cover: m(self.snow_cover, b.snow_cover),
            splashes: m(self.splashes, b.splashes),
        }
    }
}

/// An area volume as the GPU sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub sphere: bool,
    /// World → the volume's frame (rotation and translation; metres).
    pub inverse: Mat4,
    /// Box half extents in metres (sphere: x = radius).
    pub half: Vec3,
    pub blend: f32,
    pub weight: f32,
    pub amounts: Amounts,
}

/// The weather to draw this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherFrame {
    /// Weather clock (seconds, wrapped hourly).
    pub time: f32,
    /// The weather at the camera.
    pub params: WeatherParams,
    /// Wind at the camera (m/s) and the accumulated drift (metres).
    pub wind: Vec3,
    pub wind_offset: Vec3,
    /// Accumulated fall (metres) of rain and snow.
    pub rain_fall: f32,
    pub snow_fall: f32,
    /// Global volumes blended (under every area).
    pub global: Amounts,
    /// Area volumes, lowest priority first.
    pub regions: Vec<Region>,
    /// The most rain / snow anywhere near the camera (sizes the particle counts).
    pub max_rain: f32,
    pub max_snow: f32,
    /// Lightning flash brightness at the camera and the direction towards it.
    pub flash: f32,
    pub flash_dir: Vec3,
    /// Visible bolts with their brightness now.
    pub bolts: Vec<(Arc<Bolt>, f32)>,
    pub effects: WeatherEffects,
    pub surfaces: SurfaceSettings,
}

impl WeatherFrame {
    /// Anything to draw?
    pub fn visible(&self) -> bool {
        self.max_rain > 0.0
            || self.max_snow > 0.0
            || self.flash > 0.0
            || self.params.visibility < 10_000.0
            || self.global.wetness > 0.0
            || self.global.snow_cover > 0.0
            || self
                .regions
                .iter()
                .any(|r| r.amounts.wetness > 0.0 || r.amounts.snow_cover > 0.0)
    }

    /// Amounts at `p`, blended like the GPU does (globals, then areas by priority).
    pub fn amounts_at(&self, p: Vec3) -> Amounts {
        let mut a = self.global;
        for r in &self.regions {
            let w = r.weight_at(p);
            if w > 0.0 {
                a = a.lerp(&r.amounts, w);
            }
        }
        a
    }
}

impl Region {
    pub fn weight_at(&self, p: Vec3) -> f32 {
        let q = self.inverse.transform_point3(p);
        let inside = if self.sphere {
            self.half.x - q.length()
        } else {
            (self.half - q.abs()).min_element()
        };
        if inside <= 0.0 {
            0.0
        } else if self.blend <= 1e-4 {
            self.weight
        } else {
            let t = (inside / self.blend).clamp(0.0, 1.0);
            self.weight * t * t * (3.0 - 2.0 * t)
        }
    }
}

/// The renderer's weather of `world` (`None` while it isn't running or shows nothing).
pub fn build(world: &WeatherWorld) -> Option<WeatherFrame> {
    if !world.live || world.layers.is_empty() {
        return None;
    }
    let camera = world.camera;
    let mut global = Amounts::default();
    let mut areas: Vec<(f32, Region)> = Vec::new();
    let mut max_rain: f32 = 0.0;
    let mut max_snow: f32 = 0.0;
    for (i, l) in world.layers.iter().enumerate() {
        let a = Amounts::of(l);
        if l.coverage == Coverage::Global {
            global = if i == 0 { a } else { global.lerp(&a, l.weight) };
            continue;
        }
        let (scale, rot, pos) = l.world.to_scale_rotation_translation();
        let inverse = Mat4::from_rotation_translation(rot, pos).inverse();
        let sphere = l.coverage == Coverage::Sphere;
        let half = if sphere {
            Vec3::splat(l.radius.abs() * scale.abs().max_element())
        } else {
            Vec3::from(l.size).abs() * scale.abs() * 0.5
        };
        let region = Region {
            sphere,
            inverse,
            half,
            blend: l.blend,
            weight: l.weight,
            amounts: a,
        };
        // Distance from the camera to the region (0 inside): nearest regions are kept.
        let q = inverse.transform_point3(camera);
        let gap = if sphere {
            (q.length() - half.x).max(0.0)
        } else {
            (q.abs() - half).max(Vec3::ZERO).length()
        };
        // Only regions that can reach the particle box matter for the counts.
        if gap < 120.0 {
            max_rain = max_rain.max(a.rain * l.weight);
            max_snow = max_snow.max(a.snow * l.weight);
        }
        areas.push((gap, region));
    }
    max_rain = max_rain.max(global.rain);
    max_snow = max_snow.max(global.snow);
    if areas.len() > MAX_REGIONS {
        // The nearest, then back in priority order.
        let mut idx: Vec<usize> = (0..areas.len()).collect();
        idx.sort_by(|&a, &b| areas[a].0.total_cmp(&areas[b].0));
        idx.truncate(MAX_REGIONS);
        idx.sort_unstable();
        areas = idx.into_iter().map(|i| areas[i].clone()).collect();
    }
    let regions: Vec<Region> = areas.into_iter().map(|(_, r)| r).collect();
    let (flash, flash_dir) = world.flash();
    let bolts = world
        .lightning
        .strikes
        .iter()
        .filter_map(|s| s.bolt.clone().map(|b| (b, s.envelope() * s.intensity)))
        .collect();
    let frame = WeatherFrame {
        time: (world.clock % 3600.0) as f32,
        params: world.current,
        wind: world.wind,
        wind_offset: world.wind_offset,
        rain_fall: world.rain_fall,
        snow_fall: world.snow_fall,
        global,
        regions,
        max_rain,
        max_snow,
        flash,
        flash_dir,
        bolts,
        effects: world.effects,
        surfaces: world.surfaces,
    };
    frame.visible().then_some(frame)
}

/// The scene's weather for the renderer (`None` = draw no weather).
pub fn scene_frame(scene: &Scene) -> Option<Arc<WeatherFrame>> {
    build(&scene.weather).map(Arc::new)
}

/// The weather's cloud cover: clouds the scene already has thicken towards it; a scene
/// without clouds gets them once the weather is overcast enough (Drive Clouds).
pub fn drive_clouds(clouds: &mut CloudsOverride, p: &WeatherParams) {
    if clouds.is_on() {
        clouds.coverage = clouds.coverage.max(p.cloud_coverage);
        clouds.density = clouds.density.max(p.cloud_density);
    } else {
        let k = ((p.cloud_coverage - 0.45) / 0.55).clamp(0.0, 1.0);
        if k <= 0.0 {
            return;
        }
        let mut c = CloudsOverride {
            active: true,
            enabled: true,
            ..Default::default()
        };
        crate::scene::CloudPreset::Overcast.apply(&mut c);
        c.coverage = p.cloud_coverage * k;
        c.density = p.cloud_density.max(0.3);
        c.wind_direction = p.wind_direction;
        c.wind_speed = (p.wind_speed * 2.0).max(4.0);
        *clouds = c;
    }
}

/// The weather's changes to the scene's post-processing: cloud cover (Drive Clouds) and a
/// thicker volumetric fog when visibility drops (Haze).
pub fn adjust_post(post: &mut crate::render::PostSettings, scene: &Scene) {
    let w = &scene.weather;
    if !w.live {
        return;
    }
    let p = &w.current;
    if w.effects.drive_clouds {
        drive_clouds(&mut post.clouds, p);
    }
    if w.effects.haze && post.fog.is_on() && p.visibility < 10_000.0 {
        let v = &mut post.fog.volume;
        v.attenuation_distance = v.attenuation_distance.min(p.visibility * 0.8).max(5.0);
        v.max_fog_distance = v.max_fog_distance.min(p.visibility * 2.5).max(50.0);
    }
}
