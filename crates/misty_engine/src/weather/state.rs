//! The weather runtime: each volume's timeline and surface state, the blend at the camera,
//! the script layer (`Weather.SetState`), wind, lightning and events.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use hecs::Entity;

use super::component::{Coverage, SurfaceSettings, TimelineMode, Weather, WeatherEffects};
use super::lightning::{Lightning, Rng};
use super::{WeatherParams, WeatherPreset};
use crate::scene::Scene;

/// Wind drift and fall offsets wrap at this many metres (a multiple of every particle box
/// size, so wrapping never moves a drop).
pub const WRAP: f32 = 1024.0;

/// Something scripts hear (and the audio plays).
#[derive(Clone, Debug, PartialEq)]
pub enum WeatherEvent {
    /// A lightning strike (its flash is seen now).
    Lightning {
        position: Vec3,
        distance: f32,
        intensity: f32,
    },
    /// Thunder of an earlier strike reaches the camera.
    Thunder { distance: f32, volume: f32 },
    /// The weather started moving to another state (a timeline step or `Weather.SetState`).
    Changed { state: String, transition: f32 },
}

/// Where a volume's timeline is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimelineState {
    pub started: bool,
    /// Entry shown (or being blended into).
    pub index: usize,
    /// Seconds left before the next entry (while holding).
    pub hold_left: f32,
    /// 0..1 progress of the blend into `index` (1 = holding).
    pub progress: f32,
    /// Seconds the current blend takes.
    pub transition: f32,
    /// Values when the blend began.
    pub from: WeatherParams,
}

/// One volume's runtime state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeState {
    /// Values now (timeline applied, before the script layer).
    pub params: WeatherParams,
    pub timeline: TimelineState,
    /// 0..1 surface wetness inside this volume.
    pub wetness: f32,
    /// 0..1 puddle fill.
    pub puddles: f32,
    /// 0..1 snow cover.
    pub snow_cover: f32,
    rng: Option<Rng>,
}

/// `Weather.SetState`: a layer over every volume, blending to the script's values.
#[derive(Clone, Debug, PartialEq)]
struct ScriptLayer {
    from: WeatherParams,
    to: WeatherParams,
    /// Seconds into the blend of `from` → `to`.
    elapsed: f32,
    duration: f32,
    /// Layer weight now and where it is heading (0 when released).
    weight: f32,
    weight_target: f32,
    weight_speed: f32,
}

impl ScriptLayer {
    fn params(&self) -> WeatherParams {
        let t = if self.duration <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        };
        self.from.lerp(&self.to, smooth(t))
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A weather layer as blended: a volume (or the implicit clear base) with its values now.
#[derive(Clone, Debug)]
pub struct Layer {
    /// `None` = the implicit clear base under every volume.
    pub entity: Option<Entity>,
    pub coverage: Coverage,
    pub world: Mat4,
    pub size: [f32; 3],
    pub radius: f32,
    pub blend: f32,
    pub priority: f32,
    pub weight: f32,
    /// Values with the script layer applied.
    pub params: WeatherParams,
    pub wetness: f32,
    pub puddles: f32,
    pub snow_cover: f32,
}

impl Layer {
    /// 0..1: how much of this layer applies at `p` (× its weight).
    pub fn weight_at(&self, p: Vec3) -> f32 {
        self.weight
            * area_weight(
                self.coverage,
                &self.world,
                self.size,
                self.radius,
                self.blend,
                p,
            )
    }
}

/// 0..1 coverage of a volume at `p`: 1 everywhere for Global; for areas 1 inside, fading to
/// 0 over `blend` metres inside the edge.
pub fn area_weight(
    coverage: Coverage,
    world: &Mat4,
    size: [f32; 3],
    radius: f32,
    blend: f32,
    p: Vec3,
) -> f32 {
    let (scale, rot, pos) = world.to_scale_rotation_translation();
    let inside = match coverage {
        Coverage::Global => return 1.0,
        Coverage::Box => {
            let half = Vec3::from(size).abs() * scale.abs() * 0.5;
            let q = (rot.inverse() * (p - pos)).abs();
            (half - q).min_element()
        }
        Coverage::Sphere => {
            let r = radius.abs() * scale.abs().max_element();
            r - (p - pos).length()
        }
    };
    if inside <= 0.0 {
        0.0
    } else if blend <= 1e-4 {
        1.0
    } else {
        smooth(inside / blend)
    }
}

/// The weather runtime of a scene (not saved): clocks, volume states, the blend at the
/// camera, lightning and pending events.
#[derive(Clone, Debug, Default)]
pub struct WeatherWorld {
    /// Seconds of weather time.
    pub clock: f64,
    /// Updated this frame (Play, or an Edit-mode preview): only then is weather drawn.
    pub live: bool,
    /// Where the camera was.
    pub camera: Vec3,
    /// Weather at the camera: every layer blended, script layer included.
    pub current: WeatherParams,
    /// The wind at the camera with gusts and turbulence (m/s).
    pub wind: Vec3,
    /// Accumulated wind drift (metres, wrapped at [`WRAP`]).
    pub wind_offset: Vec3,
    /// Accumulated rain and snow fall (metres, wrapped at [`WRAP`]).
    pub rain_fall: f32,
    pub snow_fall: f32,
    /// Layers blended this frame (base first, then volumes by priority).
    pub layers: Vec<Layer>,
    /// The volume with the most weight at the camera (its effect, surface and audio
    /// settings are used).
    pub dominant: Option<Entity>,
    pub effects: WeatherEffects,
    pub surfaces: SurfaceSettings,
    pub audio: super::WeatherAudio,
    pub lightning: Lightning,
    volumes: HashMap<Entity, VolumeState>,
    base: VolumeState,
    script: Option<ScriptLayer>,
    events: Vec<WeatherEvent>,
    /// Events kept for the audio driver (thunder) until it takes them.
    audio_events: Vec<WeatherEvent>,
}

/// Every enabled, active Weather volume: (entity, component, world matrix), lowest priority
/// first (Hierarchy order on ties).
pub fn volumes(scene: &Scene) -> Vec<(Entity, Weather, Mat4)> {
    if !scene.has_any::<Weather>() {
        return Vec::new();
    }
    let mut list = scene.active_with::<Weather>();
    scene.sort_hierarchy_order(&mut list);
    let mut out: Vec<(Entity, Weather, Mat4)> = list
        .into_iter()
        .filter_map(|e| {
            let w = scene.world.get::<&Weather>(e).ok()?;
            w.enabled
                .then(|| (e, (*w).clone(), scene.world_matrix(e)))
        })
        .collect();
    // Stable: equal priorities keep Hierarchy order.
    out.sort_by(|a, b| a.1.priority.total_cmp(&b.1.priority));
    out
}

impl WeatherWorld {
    /// Advance the weather `dt` seconds with the camera at `camera`.
    pub fn update(&mut self, scene_volumes: &[(Entity, Weather, Mat4)], dt: f32, camera: Vec3) {
        self.update_with_wind(scene_volumes, dt, camera, None);
    }

    /// [`WeatherWorld::update`] with the scene's Wind Zone (direction, × strength) for volumes
    /// that use it.
    pub fn update_with_wind(
        &mut self,
        scene_volumes: &[(Entity, Weather, Mat4)],
        dt: f32,
        camera: Vec3,
        wind_zone: Option<(Vec3, f32)>,
    ) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 1.0) } else { 0.0 };
        self.live = true;
        self.clock += f64::from(dt);
        self.camera = camera;
        // Volumes: timelines, then surfaces.
        self.volumes
            .retain(|e, _| scene_volumes.iter().any(|(v, _, _)| v == e));
        let script = self.advance_script(dt);
        let mut layers = Vec::with_capacity(scene_volumes.len() + 1);
        {
            let base_params = apply_script(WeatherPreset::Clear.params(), &script);
            let surfaces = scene_volumes
                .iter()
                .rev()
                .find(|(_, w, _)| !w.is_area())
                .map(|(_, w, _)| w.surfaces)
                .unwrap_or_default();
            self.base.params = WeatherPreset::Clear.params();
            accumulate(&mut self.base, &base_params, &surfaces, dt);
            layers.push(Layer {
                entity: None,
                coverage: Coverage::Global,
                world: Mat4::IDENTITY,
                size: [0.0; 3],
                radius: 0.0,
                blend: 0.0,
                priority: f32::NEG_INFINITY,
                weight: 1.0,
                params: base_params,
                wetness: self.base.wetness,
                puddles: self.base.puddles,
                snow_cover: self.base.snow_cover,
            });
        }
        for (e, w, m) in scene_volumes {
            let state = self.volumes.entry(*e).or_default();
            let changed = advance_timeline(state, w, dt);
            if let Some((name, transition)) = changed {
                self.events.push(WeatherEvent::Changed {
                    state: name,
                    transition,
                });
            }
            let params = apply_script(state.params, &script);
            accumulate(state, &params, &w.surfaces, dt);
            layers.push(Layer {
                entity: Some(*e),
                coverage: w.coverage,
                world: *m,
                size: w.size,
                radius: w.radius,
                blend: w.blend_distance.max(0.0),
                priority: w.priority,
                weight: w.weight.clamp(0.0, 1.0),
                params,
                wetness: state.wetness,
                puddles: state.puddles,
                snow_cover: state.snow_cover,
            });
        }
        // Blend at the camera, lowest priority first.
        let mut current = layers[0].params;
        let mut best: Option<(f32, usize)> = None;
        for (i, l) in layers.iter().enumerate().skip(1) {
            let w = l.weight_at(camera);
            if w > 0.0 {
                current = current.lerp(&l.params, w);
            }
            // The dominant volume: most weight, the higher priority on ties (later wins).
            if best.is_none_or(|(bw, _)| w >= bw) && (w > 0.0 || best.is_none()) {
                best = Some((w, i));
            }
        }
        let dominant = best.map(|(_, i)| i);
        self.dominant = dominant.and_then(|i| layers[i].entity);
        let (effects, surfaces, audio) = dominant
            .and_then(|i| {
                let e = layers[i].entity?;
                scene_volumes
                    .iter()
                    .find(|(v, _, _)| *v == e)
                    .map(|(_, w, _)| (w.effects, w.surfaces, w.audio.clone()))
            })
            .unwrap_or_default();
        self.effects = effects;
        self.surfaces = surfaces;
        self.audio = audio;
        self.current = current.sanitized();
        self.layers = layers;

        // Wind: the weather's (or the Wind Zone's direction), with gusts and turbulence.
        let p = self.current;
        let base = match (self.effects.use_wind_zone, wind_zone) {
            (true, Some((dir, main))) => {
                let d = Vec3::new(dir.x, 0.0, dir.z).normalize_or(Vec3::Z);
                d * p.wind_speed * main
            }
            _ => p.wind_vector(),
        };
        let t = self.clock as f32;
        let gust = p.gusts * (noise1(t * p.gust_frequency / 60.0 * 2.0 + 17.0) * 0.5 + 0.5);
        let swing = p.wind_turbulence * noise1(t * 0.35 + 3.0) * 0.6;
        let flutter = 1.0 + p.wind_turbulence * noise1(t * 1.7 + 9.0) * 0.25;
        let wind = glam::Quat::from_rotation_y(swing) * base * (1.0 + gust * 1.2) * flutter;
        self.wind = wind;
        self.wind_offset = wrap3(self.wind_offset + wind * dt);
        self.rain_fall = (self.rain_fall + p.rain_speed * dt).rem_euclid(WRAP);
        self.snow_fall = (self.snow_fall + p.snow_speed * dt).rem_euclid(WRAP);

        // Lightning.
        let seed = scene_volumes.first().map_or(1, |(_, w, _)| w.seed);
        self.lightning.seed(u64::from(seed));
        let ev = self.lightning.update(
            dt,
            p.lightning_frequency,
            p.lightning_min_distance,
            p.lightning_max_distance,
            p.lightning_intensity,
            camera,
            self.effects.bolts,
        );
        for (position, distance, intensity) in ev.strikes {
            self.events.push(WeatherEvent::Lightning {
                position,
                distance,
                intensity,
            });
        }
        for (distance, volume) in ev.thunder {
            let e = WeatherEvent::Thunder { distance, volume };
            self.events.push(e.clone());
            self.audio_events.push(e);
        }
        // Don't grow without bound when nobody listens.
        if self.events.len() > 256 {
            self.events.drain(..self.events.len() - 256);
        }
        if self.audio_events.len() > 64 {
            self.audio_events.drain(..self.audio_events.len() - 64);
        }
    }

    fn advance_script(&mut self, dt: f32) -> Option<(WeatherParams, f32)> {
        let s = self.script.as_mut()?;
        s.elapsed += dt;
        let step = s.weight_speed * dt;
        if s.weight < s.weight_target {
            s.weight = (s.weight + step).min(s.weight_target);
        } else {
            s.weight = (s.weight - step).max(s.weight_target);
        }
        if s.weight_target <= 0.0 && s.weight <= 0.0 {
            self.script = None;
            return None;
        }
        Some((s.params(), s.weight))
    }

    /// Script values blended to now (the target while a `SetState` is running).
    fn script_now(&self) -> WeatherParams {
        match &self.script {
            Some(s) if s.weight >= 1.0 => s.params(),
            Some(s) => self.current.lerp(&s.params(), s.weight),
            None => self.current,
        }
    }

    /// Move the weather everywhere to `to` over `seconds` (a layer over every volume, until
    /// [`WeatherWorld::release`]).
    pub fn set_state(&mut self, to: WeatherParams, seconds: f32, name: &str) {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let from = self.script_now();
        self.script = Some(ScriptLayer {
            from,
            to: to.sanitized(),
            elapsed: 0.0,
            duration: seconds,
            weight: 1.0,
            weight_target: 1.0,
            weight_speed: 1.0,
        });
        self.events.push(WeatherEvent::Changed {
            state: name.to_owned(),
            transition: seconds,
        });
    }

    /// Change the script layer's target with `f` (starting from the weather now).
    pub fn modify(&mut self, seconds: f32, f: impl FnOnce(&mut WeatherParams)) {
        let mut to = match &self.script {
            Some(s) if s.weight_target > 0.0 => s.to,
            _ => self.current,
        };
        f(&mut to);
        let name = to.nearest_preset().key();
        self.set_state(to, seconds, name);
    }

    /// Hand the weather back to the volumes and their timelines over `seconds`.
    pub fn release(&mut self, seconds: f32) {
        if let Some(s) = self.script.as_mut() {
            s.weight_target = 0.0;
            s.weight_speed = if seconds > 0.0 { 1.0 / seconds } else { f32::MAX };
        }
    }

    /// Is a script holding the weather?
    pub fn scripted(&self) -> bool {
        self.script.as_ref().is_some_and(|s| s.weight_target > 0.0)
    }

    /// A strike now at `position` (scripts, the demo's key).
    pub fn strike(&mut self, position: Vec3, intensity: f32) {
        let d = (position - self.camera).length();
        let bolts = self.effects.bolts;
        let (position, distance, intensity) =
            (self.lightning).strike_at(position, d, intensity, bolts, self.camera);
        self.events.push(WeatherEvent::Lightning {
            position,
            distance,
            intensity,
        });
    }

    /// The events since the last call (scripts).
    pub fn take_events(&mut self) -> Vec<WeatherEvent> {
        std::mem::take(&mut self.events)
    }

    /// Thunder to play since the last call (audio).
    pub fn take_audio_events(&mut self) -> Vec<WeatherEvent> {
        std::mem::take(&mut self.audio_events)
    }

    /// The state of a volume (Inspector status line, tests).
    pub fn volume(&self, e: Entity) -> Option<&VolumeState> {
        self.volumes.get(&e)
    }

    /// Wetness at the camera (blended like the weather).
    pub fn wetness(&self) -> f32 {
        self.blend_at(self.camera, |l| l.wetness)
    }

    pub fn puddles(&self) -> f32 {
        self.blend_at(self.camera, |l| l.puddles)
    }

    pub fn snow_cover(&self) -> f32 {
        self.blend_at(self.camera, |l| l.snow_cover)
    }

    /// A per-layer value at `p`, blended by priority like the weather.
    pub fn blend_at(&self, p: Vec3, f: impl Fn(&Layer) -> f32) -> f32 {
        let Some(first) = self.layers.first() else {
            return 0.0;
        };
        let mut v = f(first);
        for l in &self.layers[1..] {
            let w = l.weight_at(p);
            v += (f(l) - v) * w;
        }
        v
    }

    /// Set the surfaces directly (tests, scripts' `Weather.SetWetness`): every layer.
    pub fn set_surfaces(&mut self, wetness: Option<f32>, snow: Option<f32>) {
        let apply = |s: &mut VolumeState| {
            if let Some(w) = wetness {
                s.wetness = w.clamp(0.0, 1.0);
                s.puddles = s.puddles.min(s.wetness).max(s.wetness * 0.8);
            }
            if let Some(c) = snow {
                s.snow_cover = c.clamp(0.0, 1.0);
            }
        };
        apply(&mut self.base);
        for s in self.volumes.values_mut() {
            apply(s);
        }
        for l in &mut self.layers {
            if let Some(w) = wetness {
                l.wetness = w.clamp(0.0, 1.0);
            }
            if let Some(c) = snow {
                l.snow_cover = c.clamp(0.0, 1.0);
            }
        }
    }

    /// Stop drawing (Edit mode without a preview).
    pub fn stop(&mut self) {
        self.live = false;
    }

    /// Back to the start (Play begins, the preview restarts).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Flash brightness at the camera and its direction.
    pub fn flash(&self) -> (f32, Vec3) {
        self.lightning.flash(self.camera)
    }
}

fn apply_script(p: WeatherParams, script: &Option<(WeatherParams, f32)>) -> WeatherParams {
    match script {
        Some((s, w)) if *w >= 1.0 => *s,
        Some((s, w)) => p.lerp(s, *w),
        None => p,
    }
}

fn wrap3(v: Vec3) -> Vec3 {
    Vec3::new(
        v.x.rem_euclid(WRAP),
        v.y.rem_euclid(WRAP),
        v.z.rem_euclid(WRAP),
    )
}

/// Smooth 1D value noise in -1..1.
pub fn noise1(x: f32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let h = |n: f32| {
        let s = (n * 127.1).sin() * 43758.547;
        (s - s.floor()) * 2.0 - 1.0
    };
    let u = f * f * (3.0 - 2.0 * f);
    h(i) + (h(i + 1.0) - h(i)) * u
}

/// Step a volume's timeline; returns the new entry's name when a blend to another entry starts.
fn advance_timeline(s: &mut VolumeState, w: &Weather, dt: f32) -> Option<(String, f32)> {
    let tl = &w.timeline;
    if !tl.is_on() {
        s.timeline = TimelineState::default();
        s.params = w.state.sanitized();
        return None;
    }
    let rng = s.rng.get_or_insert_with(|| Rng::new(u64::from(w.seed) ^ 0x7155));
    let n = tl.entries.len();
    let dt = dt * if tl.time_scale > 0.0 { tl.time_scale } else { 1.0 };
    let t = &mut s.timeline;
    let hold = |e: &super::TimelineEntry, rng: &mut Rng, random: bool| {
        let lo = e.duration.max(0.0);
        if random {
            rng.range(lo, e.max_duration.max(lo))
        } else {
            lo
        }
    };
    let random = tl.mode == TimelineMode::Random;
    if !t.started {
        t.started = true;
        t.index = 0;
        t.progress = 1.0;
        t.hold_left = hold(&tl.entries[0], rng, random);
        s.params = tl.entries[0].params().sanitized();
        return None;
    }
    t.index = t.index.min(n - 1);
    let target = tl.entries[t.index].params().sanitized();
    if t.progress < 1.0 {
        t.progress = if t.transition <= 0.0 {
            1.0
        } else {
            (t.progress + dt / t.transition).min(1.0)
        };
        s.params = t.from.lerp(&target, smooth(t.progress));
        return None;
    }
    s.params = target;
    t.hold_left -= dt;
    if t.hold_left > 0.0 {
        return None;
    }
    // Next entry.
    let next = match tl.mode {
        TimelineMode::Sequence => {
            if t.index + 1 < n {
                t.index + 1
            } else if tl.looping {
                0
            } else {
                // Stay on the last entry.
                t.hold_left = f32::INFINITY;
                return None;
            }
        }
        TimelineMode::Random => {
            let total: f32 = (0..n)
                .filter(|&i| i != t.index || n == 1)
                .map(|i| tl.entries[i].weight.max(0.0))
                .sum();
            let mut r = rng.f32() * total;
            let mut pick = (t.index + 1) % n;
            for i in (0..n).filter(|&i| i != t.index || n == 1) {
                let wgt = tl.entries[i].weight.max(0.0);
                if r < wgt {
                    pick = i;
                    break;
                }
                r -= wgt;
            }
            pick
        }
        TimelineMode::Off => return None,
    };
    let e = &tl.entries[next];
    t.from = s.params;
    t.index = next;
    t.transition = e.transition.max(0.0);
    t.progress = if t.transition <= 0.0 { 1.0 } else { 0.0 };
    t.hold_left = hold(e, rng, random);
    if t.progress >= 1.0 {
        s.params = e.params().sanitized();
    }
    let name = match e.custom {
        Some(_) => "Custom".to_owned(),
        None => e.preset.key().to_owned(),
    };
    Some((name, t.transition))
}

/// Wetting, drying, puddles, snow build-up and melting over `dt` seconds of `p`.
pub fn accumulate(s: &mut VolumeState, p: &WeatherParams, set: &SurfaceSettings, dt: f32) {
    let rain = if p.rain_density > 0.0 {
        p.rain_intensity
    } else {
        0.0
    };
    let snow = if p.snow_density > 0.0 {
        p.snow_intensity
    } else {
        0.0
    };
    let per = |seconds: f32| if seconds > 0.0 { dt / seconds } else { 1.0 };
    // Melting snow keeps the ground wet instead of letting it dry.
    let melting = set.snow_cover && snow <= 0.02 && s.snow_cover > 0.0;
    if set.wetness {
        if rain > 0.02 {
            s.wetness += per(set.wet_time) * (0.35 + 0.65 * rain);
            s.puddles += per(set.wet_time * 2.5) * rain;
        } else if !melting {
            s.wetness -= per(set.dry_time);
            s.puddles -= per(set.dry_time * 1.6);
        }
    } else {
        s.wetness = 0.0;
        s.puddles = 0.0;
    }
    if set.snow_cover {
        if snow > 0.02 {
            // Rain mixed in (sleet) settles less.
            s.snow_cover += per(set.snow_time) * (0.3 + 0.7 * snow) * (1.0 - 0.6 * rain);
        } else if s.snow_cover > 0.0 {
            let melt = per(set.melt_time) * (1.0 + 2.0 * rain);
            s.snow_cover -= melt;
            // Meltwater wets the ground.
            if set.wetness {
                s.wetness += melt * 0.8;
            }
        }
    } else {
        s.snow_cover = 0.0;
    }
    s.wetness = s.wetness.clamp(0.0, 1.0);
    s.puddles = s.puddles.clamp(0.0, 1.0);
    s.snow_cover = s.snow_cover.clamp(0.0, set.max_snow.clamp(0.0, 1.0));
}

/// Advance the scene's weather (Play mode, or an Edit-mode preview) with the camera at
/// `camera`.
pub fn update_scene(scene: &mut Scene, dt: f32, camera: Vec3) {
    let list = volumes(scene);
    let zone = crate::scene::wind_zone::scene_wind(scene).map(|w| (w.direction, w.main));
    scene.weather.update_with_wind(&list, dt, camera, zone);
}
