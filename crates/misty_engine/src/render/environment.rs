//! Scene environment for the renderer: the Lighting window settings plus the sun (main
//! Directional Light). Packs the sky/fog part of the view uniform and computes ambient light
//! on the CPU with the same sky formula as `common.wgsl::sky_color`.

use glam::Vec3;

use super::{LightData, srgb_to_linear};
use crate::scene::LightType;
use crate::scene::render_settings::{AmbientSource, FogMode, RenderSettings, SkyboxKind, SunDisk};

/// The light the procedural sky follows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// Unit vector pointing towards the sun.
    pub direction: Vec3,
    /// sRGB.
    pub color: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Environment {
    pub settings: RenderSettings,
    pub sun: Option<Sun>,
    /// Reflection probes (none = sky reflections and ambient everywhere).
    pub probes: super::probes::ProbeSet,
    /// Local Fog Volumes (seen by volumetric fog only).
    pub fog_volumes: Vec<super::LocalFog>,
    /// Light halos and lens flares (render/glow).
    pub glows: Vec<super::GlowSource>,
    /// Decal Projectors (render/decals).
    pub decals: Vec<super::DecalDraw>,
    /// The decoded HDRI while the skybox is HDRI and its file loads.
    pub hdri: Option<crate::hdri::HdriHandle>,
    /// 2D lights and their shadow maps (see [`crate::light2d`]); empty = sprites unlit.
    pub lights_2d: crate::light2d::Lights2D,
    /// Baked lightmaps of the scene (see [`crate::lightmap`]).
    pub lightmaps: crate::lightmap::LightmapSet,
    /// Draws the camera's baked occlusion data hides (see [`crate::occlusion`]).
    pub occlusion: Option<crate::occlusion::OcclusionMask>,
    /// The scene's Wind Zone (grass bends with it; None = each terrain's own wind).
    pub wind: Option<Wind>,
    /// Fixed clock for the grass wind in seconds (tests, recordings); None = the renderer's.
    pub grass_time: Option<f32>,
}

/// A Wind Zone as the grass sees it: where it blows and its multipliers of the grass's own
/// wind settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wind {
    /// World direction the wind blows towards (only xz is used).
    pub direction: Vec3,
    /// × wind strength.
    pub main: f32,
    /// × flutter.
    pub turbulence: f32,
    /// × gust strength.
    pub pulse_magnitude: f32,
    /// × gust travel speed.
    pub pulse_frequency: f32,
}

/// Linear sky colours of the default gradient (zenith, horizon).
const ZENITH: [f32; 3] = [0.40, 0.55, 0.76];
const HORIZON: [f32; 3] = [0.76, 0.79, 0.83];
const SUNSET: [f32; 3] = [1.0, 0.55, 0.25];
/// Ambient = sky colour × these (sky, equator, ground), matching the old fixed ambient.
const AMBIENT_SCALE: [f32; 3] = [0.45, 0.28, 0.3];

fn lin(c: [f32; 3]) -> Vec3 {
    Vec3::from(srgb_to_linear(c))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Environment {
    /// Settings plus the brightest enabled Directional Light as the sun.
    pub fn new(settings: RenderSettings, lights: &[LightData]) -> Self {
        let sun = lights
            .iter()
            .filter(|l| l.kind == LightType::Directional && l.intensity > 0.0)
            .max_by(|a, b| a.intensity.total_cmp(&b.intensity))
            .map(|l| Sun {
                direction: -l.direction.normalize_or(Vec3::Z),
                color: l.color,
                intensity: l.intensity,
            });
        let hdri = match (&settings.skybox.kind, &settings.skybox.hdri.texture) {
            (SkyboxKind::Hdri, Some(rel)) => crate::hdri::store::load(rel),
            _ => None,
        };
        Self {
            settings,
            sun,
            probes: Default::default(),
            fog_volumes: Vec::new(),
            glows: Vec::new(),
            decals: Vec::new(),
            hdri,
            lights_2d: Default::default(),
            lightmaps: Default::default(),
            occlusion: None,
            wind: None,
            grass_time: None,
        }
    }

    /// The HDRI sky when the skybox shows one.
    pub fn hdri_sky(&self) -> Option<&crate::hdri::HdriSky> {
        let kind = self.settings.skybox.kind;
        self.hdri
            .as_ref()
            .filter(|_| kind == SkyboxKind::Hdri)
            .map(|h| &*h.0)
    }

    /// Linear tint × exposure of the HDRI.
    pub fn hdri_multiplier(&self) -> Vec3 {
        Vec3::from(self.settings.skybox.hdri.multiplier())
    }

    /// Diffuse SH (irradiance / π, world space, intensity applied) for surfaces no probe
    /// covers while the sky is an HDRI: the HDRI's own SH for the Skybox ambient source, else
    /// the Gradient / Color ambient projected onto SH.
    pub fn ambient_sh(&self) -> [Vec3; 9] {
        let env = &self.settings.environment_lighting;
        let k = env.intensity.max(0.0);
        if env.source == AmbientSource::Skybox
            && let Some(sh) = self.hdri_world_sh()
        {
            return sh.map(|c| c * k);
        }
        let [up, equator, down] = self.ambient();
        let gradient = crate::hdri::HdrImage::from_fn(32, 16, |n| {
            if n.y >= 0.0 {
                equator.lerp(up, n.y)
            } else {
                equator.lerp(down, -n.y)
            }
        });
        crate::hdri::project_sh(&gradient).map(|c| c * k)
    }

    /// The HDRI's diffuse SH rotated into world space, tint and exposure applied.
    pub(crate) fn hdri_world_sh(&self) -> Option<[Vec3; 9]> {
        let sky = self.hdri_sky()?;
        let m = self.hdri_multiplier();
        Some(
            sky.sh_rotated(self.settings.skybox.hdri.rotation)
                .map(|c| c * m),
        )
    }

    /// The same environment lit by probes.
    pub fn with_probes(mut self, probes: super::probes::ProbeSet) -> Self {
        self.probes = probes;
        self
    }

    /// The same environment without fog (Scene view effects off).
    pub fn without_fog(&self) -> Self {
        let mut e = self.clone();
        e.settings.fog.enabled = false;
        e
    }

    /// 0 = sun high, 1 = sun at or below the horizon.
    fn sunset(&self) -> f32 {
        self.sun
            .map_or(0.0, |s| 1.0 - smoothstep(0.0, 0.42, s.direction.y))
    }

    /// Night darkening (1 = day).
    fn daylight(&self) -> f32 {
        self.sun
            .map_or(1.0, |s| 0.04 + 0.96 * smoothstep(-0.3, 0.05, s.direction.y))
    }

    fn tint(&self) -> Vec3 {
        lin(self.settings.skybox.sky_tint) / lin([0.5; 3])
    }

    /// Linear sky colour in `dir` without the sun disc (mirror of `sky_color` in common.wgsl).
    pub fn sky_color(&self, dir: Vec3) -> Vec3 {
        let sky = &self.settings.skybox;
        if sky.kind == SkyboxKind::SolidColor {
            return lin(sky.color);
        }
        let dir = dir.normalize_or(Vec3::Y);
        if let Some(h) = self.hdri_sky() {
            let local = crate::hdri::rotation(sky.hdri.rotation).inverse() * dir;
            return h.small.sample(local) * self.hdri_multiplier();
        }
        let (zenith0, horizon0) = (lin(ZENITH), lin(HORIZON));
        let t = sky.atmosphere_thickness;
        let tint = self.tint();
        let thin = t.clamp(0.0, 1.0);
        let haze = ((t - 1.0) * 0.25).clamp(0.0, 1.0);
        let mut zenith = zenith0.lerp(horizon0, haze * 0.8) * tint * (thin * thin);
        let mut horizon = horizon0 * Vec3::ONE.lerp(tint, 0.5) * (0.3 + 0.7 * thin);
        let sunset = self.sunset();
        if sunset > 0.0
            && let Some(sun) = self.sun
        {
            let toward = dir.dot(sun.direction) * 0.5 + 0.5;
            let glow = lin(SUNSET) * (0.3 + 1.1 * toward * toward);
            horizon = horizon.lerp(glow, sunset * (0.5 + 0.5 * toward));
            zenith *= 1.0 - 0.65 * sunset;
        }
        let ground = lin(sky.ground) * (1.0 - 0.4 * sunset);
        let c = if dir.y >= 0.0 {
            horizon.lerp(zenith, dir.y.powf(0.6 - 0.2 * sunset))
        } else {
            horizon.lerp(ground, (-dir.y * 6.0).min(1.0).sqrt())
        };
        super::moon::night_mix(self, dir, c * self.daylight() * sky.exposure)
    }

    /// Ambient sky / equator / ground colours (linear, before the intensity multiplier).
    pub fn ambient(&self) -> [Vec3; 3] {
        let env = &self.settings.environment_lighting;
        let s = AMBIENT_SCALE;
        match env.source {
            AmbientSource::Skybox if self.hdri_sky().is_some() => {
                // Irradiance from the HDRI's SH (already a diffuse quantity: no extra scale).
                let sh = self.hdri_world_sh().unwrap_or_default();
                let at = |d: Vec3| crate::render::probes::sh::eval(&sh, d).max(Vec3::ZERO);
                let equator = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]
                    .iter()
                    .map(|d| at(*d))
                    .sum::<Vec3>()
                    / 4.0;
                [at(Vec3::Y), equator, at(Vec3::NEG_Y)]
            }
            AmbientSource::Skybox => {
                let equator = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]
                    .iter()
                    .map(|d| self.sky_color(*d))
                    .sum::<Vec3>()
                    / 4.0;
                [
                    self.sky_color(Vec3::Y) * s[0],
                    equator * s[1],
                    self.sky_color(Vec3::NEG_Y) * s[2],
                ]
            }
            AmbientSource::Gradient => [
                lin(env.sky_color),
                lin(env.equator_color),
                lin(env.ground_color),
            ],
            AmbientSource::Color => [lin(env.sky_color); 3],
        }
    }

    /// Sky and fog vec4s of the view uniform (layout documented in common.wgsl).
    pub(crate) fn pack(&self, far: f32) -> [[f32; 4]; 8] {
        let sky = &self.settings.skybox;
        let (sun_dir, sun_color) = match self.sun {
            Some(s) => {
                let c = lin(s.color) * s.intensity;
                (s.direction.extend(1.0), c)
            }
            None => (Vec3::Y.extend(0.0), Vec3::ZERO),
        };
        let disc = match sky.sun_disk {
            SunDisk::None => 0.0,
            SunDisk::Simple => 1.0,
            SunDisk::HighQuality => 2.0,
        };
        let tint = self.tint();
        let ground = lin(sky.ground);
        let solid = lin(sky.color);
        let fog = &self.settings.fog;
        let fog_mode = match (fog.enabled, fog.mode) {
            (false, _) => 0.0,
            (true, FogMode::Linear) => 1.0,
            (true, FogMode::Exponential) => 2.0,
            (true, FogMode::ExponentialSquared) => 3.0,
        };
        let fog_color = lin(fog.color);
        let solid_flag = if sky.kind == SkyboxKind::SolidColor {
            1.0
        } else {
            0.0
        };
        [
            sun_dir.to_array(),
            [sun_color.x, sun_color.y, sun_color.z, disc],
            [tint.x, tint.y, tint.z, sky.atmosphere_thickness.max(0.0)],
            [ground.x, ground.y, ground.z, sky.exposure.max(0.0)],
            [solid.x, solid.y, solid.z, solid_flag],
            [0.004 + sky.sun_size.clamp(0.0, 1.0) * 0.2, far, 0.0, 0.0],
            [fog_color.x, fog_color.y, fog_color.z, fog_mode],
            [fog.density.max(0.0), fog.start, fog.end, 0.0],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Shadows;

    fn directional(direction: Vec3, intensity: f32) -> LightData {
        LightData {
            kind: LightType::Directional,
            position: Vec3::ZERO,
            direction,
            color: [1.0; 3],
            intensity,
            range: 0.0,
            spot_angle: 0.0,
            shadows: Shadows::None,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        }
    }

    /// The default scene light (rotation 50, -30, 0) points down at 50°.
    fn default_sun() -> LightData {
        let rot = crate::camera::euler_to_quat(Vec3::new(50.0, -30.0, 0.0));
        directional(rot * Vec3::Z, 1.0)
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).abs().max_element() < 1e-6
    }

    /// Defaults must reproduce the fixed gradient and ambient that scenes had before.
    #[test]
    fn defaults_match_the_old_fixed_sky_and_ambient() {
        let env = Environment::new(RenderSettings::default(), &[default_sun()]);
        assert!(env.sun.unwrap().direction.y > 0.7);
        assert!(close(env.sky_color(Vec3::Y), lin(ZENITH)));
        assert!(close(env.sky_color(Vec3::X), lin(HORIZON)));
        assert!(close(env.sky_color(Vec3::NEG_Y), lin([0.32, 0.30, 0.29])));
        let a = env.ambient();
        assert!(close(a[0], lin([0.40, 0.55, 0.76]) * 0.45));
        assert!(close(a[1], lin([0.76, 0.79, 0.83]) * 0.28));
        assert!(close(a[2], lin([0.32, 0.30, 0.29]) * 0.3));
        // No sun at all: same sky.
        let none = Environment::new(RenderSettings::default(), &[]);
        assert!(close(none.sky_color(Vec3::Y), lin(ZENITH)));
    }

    #[test]
    fn low_sun_warms_the_horizon_and_night_is_dark() {
        let low = Environment::new(
            RenderSettings::default(),
            &[directional(Vec3::new(0.0, -0.05, 1.0).normalize(), 1.0)],
        );
        // Looking towards the sun (sun is at -Z): warm, red above blue.
        let h = low.sky_color(Vec3::new(0.0, 0.05, -1.0));
        assert!(h.x > h.z, "{h:?}");
        let night = Environment::new(RenderSettings::default(), &[directional(Vec3::Y, 1.0)]);
        assert!(night.sky_color(Vec3::Y).max_element() < 0.05);
        // The brightest directional light is the sun.
        let two = Environment::new(
            RenderSettings::default(),
            &[directional(Vec3::Y, 0.5), directional(Vec3::NEG_Y, 2.0)],
        );
        assert_eq!(two.sun.unwrap().direction, Vec3::Y);
    }

    #[test]
    fn ambient_sources() {
        let mut s = RenderSettings::default();
        s.environment_lighting.source = AmbientSource::Color;
        s.environment_lighting.sky_color = [1.0, 0.0, 0.0];
        let a = Environment::new(s.clone(), &[]).ambient();
        assert!(a.iter().all(|c| close(*c, Vec3::X)));
        s.environment_lighting.source = AmbientSource::Gradient;
        let a = Environment::new(s.clone(), &[]).ambient();
        assert!(close(a[1], lin([0.114, 0.125, 0.133])));
        s.environment_lighting.source = AmbientSource::Skybox;
        s.skybox.kind = SkyboxKind::SolidColor;
        s.skybox.color = [0.0, 1.0, 0.0];
        let a = Environment::new(s, &[]).ambient();
        assert!(close(a[0], Vec3::Y * 0.45));
    }
}
