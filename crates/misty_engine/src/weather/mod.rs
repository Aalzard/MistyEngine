//! Weather: rain, snow, sleet and thunderstorms.
//!
//! - [`Weather`] (GameObject ▸ Rendering ▸ Weather) is a weather volume: Global (everywhere,
//!   the precipitation follows the camera) or an Area (a box or sphere with a soft edge, e.g. a
//!   storm cell or a rainy valley). Volumes blend by priority like Post-process Volumes.
//! - Each volume shows its own [`WeatherParams`] (a preset or free values), optionally driven by
//!   a [`WeatherTimeline`] (a looping sequence or weighted random weather) with smooth
//!   transitions between any two states.
//! - [`WeatherWorld`] (on the [`crate::scene::Scene`]) is the runtime: the clock, the drifting
//!   wind, each volume's timeline and surface wetness / puddles / snow cover, lightning and the
//!   events scripts hear (`OnLightning`, `OnWeatherChanged`). It advances in Play mode and in
//!   Edit mode while a volume has Preview in Edit Mode on.
//! - [`frame::WeatherFrame`] is what the renderer draws (`render::weather`): GPU rain streaks
//!   and snow flakes recycled in a box around the camera, splashes from the depth buffer, a
//!   top-down occlusion map (no rain under roofs), wet / puddled / snow-covered surfaces,
//!   lightning flashes and bolts, haze and lens drops.

pub mod audio;
mod component;
pub mod frame;
pub mod lightning;
mod state;

#[cfg(test)]
mod tests;

pub use component::{
    Coverage, SurfaceSettings, TimelineEntry, TimelineMode, Weather, WeatherAudio, WeatherEffects,
    WeatherTimeline,
};
pub use frame::WeatherFrame;
pub use lightning::{Bolt, Strike};
pub use state::{WeatherEvent, WeatherWorld, area_weight, update_scene, volumes};

use serde::{Deserialize, Serialize};

/// Named weather states (the Inspector's preset dropdown, timeline entries and
/// `Weather.SetState(name)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WeatherPreset {
    #[default]
    Clear,
    Drizzle,
    LightRain,
    Rain,
    HeavyRain,
    Downpour,
    Thunderstorm,
    Sleet,
    Flurries,
    LightSnow,
    Snow,
    HeavySnow,
    Blizzard,
}

impl WeatherPreset {
    pub const ALL: [Self; 13] = [
        Self::Clear,
        Self::Drizzle,
        Self::LightRain,
        Self::Rain,
        Self::HeavyRain,
        Self::Downpour,
        Self::Thunderstorm,
        Self::Sleet,
        Self::Flurries,
        Self::LightSnow,
        Self::Snow,
        Self::HeavySnow,
        Self::Blizzard,
    ];

    /// Inspector label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Drizzle => "Drizzle",
            Self::LightRain => "Light Rain",
            Self::Rain => "Rain",
            Self::HeavyRain => "Heavy Rain",
            Self::Downpour => "Downpour",
            Self::Thunderstorm => "Thunderstorm",
            Self::Sleet => "Sleet",
            Self::Flurries => "Flurries",
            Self::LightSnow => "Light Snow",
            Self::Snow => "Snow",
            Self::HeavySnow => "Heavy Snow",
            Self::Blizzard => "Blizzard",
        }
    }

    /// Script name (`Weather.SetState("HeavyRain")`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Drizzle => "Drizzle",
            Self::LightRain => "LightRain",
            Self::Rain => "Rain",
            Self::HeavyRain => "HeavyRain",
            Self::Downpour => "Downpour",
            Self::Thunderstorm => "Thunderstorm",
            Self::Sleet => "Sleet",
            Self::Flurries => "Flurries",
            Self::LightSnow => "LightSnow",
            Self::Snow => "Snow",
            Self::HeavySnow => "HeavySnow",
            Self::Blizzard => "Blizzard",
        }
    }

    /// The preset named `name`, ignoring case, spaces, dashes and underscores ("heavy rain",
    /// "HeavyRain", "heavy_rain"). "Storm" is the Thunderstorm, "Medium Rain" the Rain.
    pub fn parse(name: &str) -> Option<Self> {
        let k: String = name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        let alias = match k.as_str() {
            "storm" | "thunder" => Some(Self::Thunderstorm),
            "mediumrain" => Some(Self::Rain),
            "mediumsnow" => Some(Self::Snow),
            "none" | "sunny" => Some(Self::Clear),
            _ => None,
        };
        alias.or_else(|| {
            Self::ALL
                .into_iter()
                .find(|p| p.key().to_lowercase() == k)
        })
    }

    /// The values of this state.
    pub fn params(self) -> WeatherParams {
        let mut p = WeatherParams::default();
        // (rain intensity, drops/m³, drop mm, fall m/s, opacity)
        let rain = |p: &mut WeatherParams, i, d, s, v, o| {
            p.rain_intensity = i;
            p.rain_density = d;
            p.drop_size = s;
            p.rain_speed = v;
            p.rain_opacity = o;
        };
        // (snow intensity, flakes/m³, flake cm, fall m/s, swirl)
        let snow = |p: &mut WeatherParams, i, d, s, v, t| {
            p.snow_intensity = i;
            p.snow_density = d;
            p.flake_size = s;
            p.snow_speed = v;
            p.snow_turbulence = t;
        };
        // (cloud cover, cloud density, sun dimming, ambient dimming, visibility m)
        let sky = |p: &mut WeatherParams, c, d, sd, ad, vis| {
            p.cloud_coverage = c;
            p.cloud_density = d;
            p.sun_dimming = sd;
            p.ambient_dimming = ad;
            p.visibility = vis;
        };
        match self {
            Self::Clear => {}
            Self::Drizzle => {
                rain(&mut p, 0.15, 10.0, 0.6, 3.6, 0.35);
                sky(&mut p, 0.75, 0.6, 0.45, 0.2, 6000.0);
                p.wind_speed = 2.0;
                p.splashes = 0.25;
            }
            Self::LightRain => {
                rain(&mut p, 0.3, 14.0, 1.0, 5.5, 0.45);
                sky(&mut p, 0.8, 0.7, 0.55, 0.28, 3500.0);
                p.wind_speed = 3.0;
                p.splashes = 0.5;
            }
            Self::Rain => {
                rain(&mut p, 0.55, 22.0, 1.6, 7.0, 0.55);
                sky(&mut p, 0.86, 0.85, 0.65, 0.36, 1800.0);
                p.wind_speed = 4.0;
                p.splashes = 1.0;
                p.mist = 0.15;
            }
            Self::HeavyRain => {
                rain(&mut p, 0.8, 34.0, 2.2, 8.2, 0.65);
                sky(&mut p, 0.92, 1.0, 0.75, 0.45, 800.0);
                p.wind_speed = 6.0;
                p.gusts = 0.5;
                p.splashes = 1.5;
                p.mist = 0.5;
            }
            Self::Downpour => {
                rain(&mut p, 1.0, 50.0, 2.8, 9.0, 0.75);
                sky(&mut p, 0.97, 1.2, 0.85, 0.55, 380.0);
                p.wind_speed = 7.0;
                p.gusts = 0.6;
                p.splashes = 2.0;
                p.mist = 0.85;
            }
            Self::Thunderstorm => {
                rain(&mut p, 0.85, 38.0, 2.4, 8.5, 0.7);
                sky(&mut p, 0.97, 1.4, 0.85, 0.6, 650.0);
                p.wind_speed = 9.0;
                p.wind_turbulence = 0.45;
                p.gusts = 0.8;
                p.splashes = 1.6;
                p.mist = 0.55;
                p.lightning_frequency = 6.0;
            }
            Self::Sleet => {
                rain(&mut p, 0.4, 14.0, 1.2, 5.5, 0.5);
                snow(&mut p, 0.4, 6.0, 0.5, 2.2, 0.3);
                sky(&mut p, 0.9, 0.9, 0.7, 0.4, 1500.0);
                p.wind_speed = 5.0;
                p.gusts = 0.4;
                p.splashes = 0.6;
            }
            Self::Flurries => {
                snow(&mut p, 0.15, 2.0, 1.0, 1.0, 0.6);
                sky(&mut p, 0.6, 0.55, 0.3, 0.12, 8000.0);
                p.wind_speed = 1.5;
            }
            Self::LightSnow => {
                snow(&mut p, 0.35, 5.0, 1.1, 1.1, 0.5);
                sky(&mut p, 0.8, 0.7, 0.5, 0.22, 3000.0);
                p.wind_speed = 2.0;
            }
            Self::Snow => {
                snow(&mut p, 0.6, 10.0, 1.3, 1.2, 0.5);
                sky(&mut p, 0.88, 0.85, 0.6, 0.28, 1200.0);
                p.wind_speed = 3.0;
                p.gusts = 0.2;
            }
            Self::HeavySnow => {
                snow(&mut p, 0.85, 18.0, 1.6, 1.3, 0.6);
                sky(&mut p, 0.95, 1.0, 0.7, 0.32, 450.0);
                p.wind_speed = 4.0;
                p.gusts = 0.3;
            }
            Self::Blizzard => {
                snow(&mut p, 1.0, 28.0, 0.7, 1.6, 1.2);
                sky(&mut p, 1.0, 1.2, 0.8, 0.35, 90.0);
                p.wind_speed = 16.0;
                p.wind_turbulence = 0.6;
                p.gusts = 0.9;
            }
        }
        p
    }

    /// The preset `p` matches exactly (None = custom values).
    pub fn matching(p: &WeatherParams) -> Option<Self> {
        Self::ALL.into_iter().find(|s| {
            let mut q = s.params();
            // Wind direction and lightning distances are kept by presets.
            q.wind_direction = p.wind_direction;
            q.lightning_min_distance = p.lightning_min_distance;
            q.lightning_max_distance = p.lightning_max_distance;
            q.lightning_intensity = p.lightning_intensity;
            q == *p
        })
    }

    /// Apply this preset to `p`, keeping its wind direction and lightning distances.
    pub fn apply(self, p: &mut WeatherParams) {
        let keep = *p;
        *p = self.params();
        p.wind_direction = keep.wind_direction;
        p.lightning_min_distance = keep.lightning_min_distance;
        p.lightning_max_distance = keep.lightning_max_distance;
        p.lightning_intensity = keep.lightning_intensity;
    }
}

/// What the sky is doing: every value a weather state controls (presets fill them; each can
/// be changed freely). Transitions interpolate every field.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherParams {
    // ---- Rain ----
    /// 0..1: how hard it rains (wets surfaces, splashes, sound, haze). 0 = no rain.
    pub rain_intensity: f32,
    /// Drops per cubic metre near the camera.
    pub rain_density: f32,
    /// Drop diameter in millimetres (0.5 drizzle .. 3 downpour).
    pub drop_size: f32,
    /// Fall speed in metres per second (terminal velocity: 2–9 m/s by drop size).
    pub rain_speed: f32,
    /// × the motion-blur streak length (1 = a 1/60 s camera shutter).
    pub streak_length: f32,
    /// 0..1: how visible the drops are.
    pub rain_opacity: f32,
    /// 0..1: how much the drops bend the scene behind them.
    pub refraction: f32,

    // ---- Snow ----
    /// 0..1: how hard it snows (builds snow cover, haze). 0 = no snow.
    pub snow_intensity: f32,
    /// Flakes per cubic metre near the camera.
    pub snow_density: f32,
    /// Flake size in centimetres.
    pub flake_size: f32,
    /// Fall speed in metres per second (about 1 m/s).
    pub snow_speed: f32,
    /// 0..2: how much flakes swirl and tumble on their way down.
    pub snow_turbulence: f32,
    /// 0..1: how visible the flakes are.
    pub snow_opacity: f32,

    // ---- Wind ----
    /// Direction the wind blows towards, degrees clockwise from +Z (like the clouds).
    pub wind_direction: f32,
    /// Metres per second (slants the rain, drifts the snow).
    pub wind_speed: f32,
    /// 0..1: fast random changes of the wind direction and strength.
    pub wind_turbulence: f32,
    /// 0..1: strength of gusts that change the slant over time.
    pub gusts: f32,
    /// Gusts per minute.
    pub gust_frequency: f32,

    // ---- Ground effects ----
    /// × the splash rate where drops hit (0 = none).
    pub splashes: f32,
    /// 0..1: ground spray / mist where heavy rain hits.
    pub mist: f32,
    /// How far you can see through the rain or snow (metres; haze thickens below this).
    pub visibility: f32,

    // ---- Lightning ----
    /// Strikes per minute (0 = no lightning).
    pub lightning_frequency: f32,
    /// Nearest strike distance (metres).
    pub lightning_min_distance: f32,
    /// Farthest strike distance (metres); far strikes only light the clouds.
    pub lightning_max_distance: f32,
    /// × the flash brightness.
    pub lightning_intensity: f32,

    // ---- Sky ----
    /// 0..1: cloud cover the volumetric clouds move towards (with Drive Clouds).
    pub cloud_coverage: f32,
    /// 0..2: cloud thickness (darker storm clouds).
    pub cloud_density: f32,
    /// 0..1: share of the direct sun / moon light the clouds take away.
    pub sun_dimming: f32,
    /// 0..1: share of the sky's ambient light the clouds take away.
    pub ambient_dimming: f32,
}

impl Default for WeatherParams {
    /// Clear weather.
    fn default() -> Self {
        Self {
            rain_intensity: 0.0,
            rain_density: 0.0,
            drop_size: 1.5,
            rain_speed: 7.0,
            streak_length: 1.0,
            rain_opacity: 0.55,
            refraction: 0.6,
            snow_intensity: 0.0,
            snow_density: 0.0,
            flake_size: 1.2,
            snow_speed: 1.2,
            snow_turbulence: 0.5,
            snow_opacity: 0.9,
            wind_direction: 60.0,
            wind_speed: 2.0,
            wind_turbulence: 0.25,
            gusts: 0.0,
            gust_frequency: 4.0,
            splashes: 0.0,
            mist: 0.0,
            visibility: 20000.0,
            lightning_frequency: 0.0,
            lightning_min_distance: 400.0,
            lightning_max_distance: 6000.0,
            lightning_intensity: 1.0,
            // Clear weather adds no clouds (Drive Clouds only ever thickens the scene's).
            cloud_coverage: 0.0,
            cloud_density: 0.0,
            sun_dimming: 0.0,
            ambient_dimming: 0.0,
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Shortest-arc blend of two angles in degrees (result in 0..360).
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let d = (b - a + 540.0).rem_euclid(360.0) - 180.0;
    (a + d * t).rem_euclid(360.0)
}

impl WeatherParams {
    /// `self` at `t` = 0 to `b` at `t` = 1. Densities and sizes blend linearly, the wind
    /// direction along the shortest arc and the visibility by its extinction (so fog comes in
    /// evenly instead of all at the end). Appearance values of a precipitation that is absent
    /// on one side take the other side's (rain → clear keeps the drop size while it thins).
    pub fn lerp(&self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        // Exact at the ends (a state reached is that state).
        if t <= 0.0 {
            return *self;
        }
        if t >= 1.0 {
            return *b;
        }
        let (a, mut b2, mut a2) = (*self, *b, *self);
        // Absent rain or snow borrows the other side's look while it fades in or out.
        if a.rain_density <= 0.0 {
            a2.drop_size = b.drop_size;
            a2.rain_speed = b.rain_speed;
            a2.rain_opacity = b.rain_opacity;
        }
        if b.rain_density <= 0.0 {
            b2.drop_size = a.drop_size;
            b2.rain_speed = a.rain_speed;
            b2.rain_opacity = a.rain_opacity;
        }
        if a.snow_density <= 0.0 {
            a2.flake_size = b.flake_size;
            a2.snow_speed = b.snow_speed;
            a2.snow_turbulence = b.snow_turbulence;
        }
        if b.snow_density <= 0.0 {
            b2.flake_size = a.flake_size;
            b2.snow_speed = a.snow_speed;
            b2.snow_turbulence = a.snow_turbulence;
        }
        let ext = |v: f32| 1.0 / v.max(1.0);
        Self {
            rain_intensity: lerp(a.rain_intensity, b.rain_intensity, t),
            rain_density: lerp(a.rain_density, b.rain_density, t),
            drop_size: lerp(a2.drop_size, b2.drop_size, t),
            rain_speed: lerp(a2.rain_speed, b2.rain_speed, t),
            streak_length: lerp(a.streak_length, b.streak_length, t),
            rain_opacity: lerp(a2.rain_opacity, b2.rain_opacity, t),
            refraction: lerp(a.refraction, b.refraction, t),
            snow_intensity: lerp(a.snow_intensity, b.snow_intensity, t),
            snow_density: lerp(a.snow_density, b.snow_density, t),
            flake_size: lerp(a2.flake_size, b2.flake_size, t),
            snow_speed: lerp(a2.snow_speed, b2.snow_speed, t),
            snow_turbulence: lerp(a2.snow_turbulence, b2.snow_turbulence, t),
            snow_opacity: lerp(a.snow_opacity, b.snow_opacity, t),
            wind_direction: lerp_angle(a.wind_direction, b.wind_direction, t),
            wind_speed: lerp(a.wind_speed, b.wind_speed, t),
            wind_turbulence: lerp(a.wind_turbulence, b.wind_turbulence, t),
            gusts: lerp(a.gusts, b.gusts, t),
            gust_frequency: lerp(a.gust_frequency, b.gust_frequency, t),
            splashes: lerp(a.splashes, b.splashes, t),
            mist: lerp(a.mist, b.mist, t),
            visibility: 1.0 / lerp(ext(a.visibility), ext(b.visibility), t),
            lightning_frequency: lerp(a.lightning_frequency, b.lightning_frequency, t),
            lightning_min_distance: lerp(a.lightning_min_distance, b.lightning_min_distance, t),
            lightning_max_distance: lerp(a.lightning_max_distance, b.lightning_max_distance, t),
            lightning_intensity: lerp(a.lightning_intensity, b.lightning_intensity, t),
            cloud_coverage: lerp(a.cloud_coverage, b.cloud_coverage, t),
            cloud_density: lerp(a.cloud_density, b.cloud_density, t),
            sun_dimming: lerp(a.sun_dimming, b.sun_dimming, t),
            ambient_dimming: lerp(a.ambient_dimming, b.ambient_dimming, t),
        }
    }

    /// Values clamped to their ranges (scripts and old files can hold anything).
    pub fn sanitized(&self) -> Self {
        let p = *self;
        let pos = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        Self {
            rain_intensity: unit(p.rain_intensity),
            rain_density: pos(p.rain_density).min(400.0),
            drop_size: pos(p.drop_size).clamp(0.1, 8.0),
            rain_speed: pos(p.rain_speed).clamp(0.5, 30.0),
            streak_length: pos(p.streak_length).min(10.0),
            rain_opacity: unit(p.rain_opacity),
            refraction: unit(p.refraction),
            snow_intensity: unit(p.snow_intensity),
            snow_density: pos(p.snow_density).min(400.0),
            flake_size: pos(p.flake_size).clamp(0.05, 10.0),
            snow_speed: pos(p.snow_speed).clamp(0.1, 10.0),
            snow_turbulence: pos(p.snow_turbulence).min(4.0),
            snow_opacity: unit(p.snow_opacity),
            wind_direction: if p.wind_direction.is_finite() {
                p.wind_direction.rem_euclid(360.0)
            } else {
                0.0
            },
            wind_speed: pos(p.wind_speed).min(80.0),
            wind_turbulence: unit(p.wind_turbulence),
            gusts: unit(p.gusts),
            gust_frequency: pos(p.gust_frequency).min(60.0),
            splashes: pos(p.splashes).min(4.0),
            mist: unit(p.mist),
            visibility: pos(p.visibility).clamp(10.0, 100_000.0),
            lightning_frequency: pos(p.lightning_frequency).min(120.0),
            lightning_min_distance: pos(p.lightning_min_distance).clamp(20.0, 50_000.0),
            lightning_max_distance: pos(p.lightning_max_distance)
                .clamp(pos(p.lightning_min_distance).max(20.0), 50_000.0),
            lightning_intensity: pos(p.lightning_intensity).min(10.0),
            cloud_coverage: unit(p.cloud_coverage),
            cloud_density: pos(p.cloud_density).min(2.0),
            sun_dimming: unit(p.sun_dimming),
            ambient_dimming: unit(p.ambient_dimming),
        }
    }

    /// Anything falling (or flashing)?
    pub fn is_active(&self) -> bool {
        self.rain_density > 0.0
            || self.snow_density > 0.0
            || self.lightning_frequency > 0.0
            || self.visibility < 10_000.0
    }

    /// The wind vector (m/s, world xz) of the direction and speed, before gusts.
    pub fn wind_vector(&self) -> glam::Vec3 {
        let a = self.wind_direction.to_radians();
        glam::Vec3::new(a.sin(), 0.0, a.cos()) * self.wind_speed
    }

    /// The state these values are closest to (for `OnWeatherChanged` and the HUD): exact
    /// preset matches first, else the nearest by intensity.
    pub fn nearest_preset(&self) -> WeatherPreset {
        if let Some(p) = WeatherPreset::matching(self) {
            return p;
        }
        let score = |q: &WeatherParams| {
            (q.rain_intensity - self.rain_intensity).abs()
                + (q.snow_intensity - self.snow_intensity).abs()
                + (q.lightning_frequency.min(6.0) - self.lightning_frequency.min(6.0)).abs() * 0.1
                + (q.wind_speed - self.wind_speed).abs() * 0.02
        };
        WeatherPreset::ALL
            .into_iter()
            .min_by(|a, b| score(&a.params()).total_cmp(&score(&b.params())))
            .unwrap_or_default()
    }
}
