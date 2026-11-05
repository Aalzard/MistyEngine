//! 2D lights (like a 2D renderer's Light 2D): Global, Point and Spot lights that light Sprite and
//! Tilemap Renderers on their target Sorting Layers, and **Shadow Caster 2D** shapes that block
//! point and spot lights.
//!
//! Lighting is per pixel in the sprite shader: each lit sprite multiplies its colour by the sum
//! of the lights reaching it (global lights add their colour everywhere). Nothing changes while
//! a scene has no 2D light: sprites stay unlit at full brightness.
//!
//! Shadows are a polar depth map per shadowed light, built on the CPU each frame: for
//! [`SHADOW_BINS`] directions around the light, the distance at which the nearest caster's
//! shadow starts. A caster without Self Shadows shades only what lies behind it (its far side),
//! one with Self Shadows also itself (its near side). The shader samples neighbouring directions
//! for soft edges.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::scene::{Scene, SceneComponent};

/// Most 2D lights drawn at once (the rest are dropped, brightest kept).
pub const MAX_LIGHTS: usize = 16;
/// Directions per shadow map.
pub const SHADOW_BINS: usize = 256;
/// Sorting Layer values (relative to Default) a light can target: −16..=15.
pub const LAYER_OFFSET: i32 = 16;
/// Shadow map distance of a direction no caster blocks.
pub const NO_SHADOW: f32 = 1e30;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Light2DType {
    /// Lights its target layers evenly.
    Global,
    /// A circle with an inner (full) and outer (zero) radius.
    #[default]
    Point,
    /// A point light limited to a cone.
    Spot,
}

impl Light2DType {
    pub const ALL: [Self; 3] = [Self::Global, Self::Point, Self::Spot];

    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Point => "Point",
            Self::Spot => "Spot",
        }
    }
}

/// Light 2D component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light2D {
    pub enabled: bool,
    pub kind: Light2DType,
    /// sRGB colour.
    pub color: [f32; 3],
    pub intensity: f32,
    /// Full brightness inside this radius.
    pub inner_radius: f32,
    /// No light past this radius.
    pub outer_radius: f32,
    /// Spot: full brightness inside this cone (degrees, full angle).
    pub inner_angle: f32,
    /// Spot: no light outside this cone (degrees, full angle).
    pub outer_angle: f32,
    /// Shape of the fade from inner to outer radius: 0 = linear, 1 = a soft glow.
    pub falloff_strength: f32,
    /// Sorting Layers lit; None = every layer.
    pub target_sorting_layers: Option<Vec<String>>,
    pub shadows_enabled: bool,
    /// How dark shadows are (0..1).
    pub shadow_strength: f32,
    /// Penumbra width (0 = hard edges).
    pub shadow_softness: f32,
}

impl Default for Light2D {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: Light2DType::Point,
            color: [1.0; 3],
            intensity: 1.0,
            inner_radius: 0.0,
            outer_radius: 5.0,
            inner_angle: 360.0,
            outer_angle: 360.0,
            falloff_strength: 0.5,
            target_sorting_layers: None,
            shadows_enabled: false,
            shadow_strength: 1.0,
            shadow_softness: 0.3,
        }
    }
}

impl Light2D {
    pub fn global(color: [f32; 3], intensity: f32) -> Self {
        Self {
            kind: Light2DType::Global,
            color,
            intensity,
            ..Default::default()
        }
    }

    /// Does the light reach a Sorting Layer?
    pub fn targets(&self, layer: &str) -> bool {
        self.target_sorting_layers
            .as_ref()
            .is_none_or(|l| l.iter().any(|n| n == layer))
    }
}

/// Shadow Caster 2D: the object's sprite rectangle (or its Tilemap's collider cells) blocks
/// shadowed 2D lights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowCaster2D {
    pub enabled: bool,
    /// The caster darkens itself too (else only what lies behind it).
    pub self_shadows: bool,
}

impl Default for ShadowCaster2D {
    fn default() -> Self {
        Self {
            enabled: true,
            self_shadows: false,
        }
    }
}

impl SceneComponent for Light2D {
    const NAME: &'static str = "Light 2D";
    const CATEGORY: &'static str = "Rendering";
}
impl SceneComponent for ShadowCaster2D {
    const NAME: &'static str = "Shadow Caster 2D";
    const CATEGORY: &'static str = "Rendering";
}

/// Attenuation of a point light at `dist`: 1 inside the inner radius, 0 past the outer one,
/// eased by the falloff strength between them (the shader does the same).
pub fn falloff(dist: f32, inner: f32, outer: f32, strength: f32) -> f32 {
    let t = ((outer - dist) / (outer - inner).max(1e-4)).clamp(0.0, 1.0);
    t.powf(falloff_exponent(strength))
}

/// Exponent of the radial fade for a falloff strength.
pub fn falloff_exponent(strength: f32) -> f32 {
    0.5 + strength.clamp(0.0, 1.0) * 2.5
}

/// Spot cone factor of a direction (`cos_angle` = cosine between the light's forward axis and
/// the direction to the point).
pub fn cone(cos_angle: f32, inner_deg: f32, outer_deg: f32) -> f32 {
    let co = (outer_deg.clamp(0.0, 360.0) * 0.5).to_radians().cos();
    let ci = (inner_deg.clamp(0.0, outer_deg.max(0.0)) * 0.5)
        .to_radians()
        .cos();
    if outer_deg >= 360.0 {
        return 1.0;
    }
    let t = ((cos_angle - co) / (ci - co).max(1e-4)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One light ready for the GPU (world XY, linear colour × intensity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightItem {
    pub kind: Light2DType,
    pub position: Vec2,
    /// The light's forward direction in the XY plane (spots).
    pub direction: Vec2,
    pub color: Vec3,
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub inner_angle: f32,
    pub outer_angle: f32,
    pub falloff_strength: f32,
    /// Bit `value + LAYER_OFFSET` set for every targeted Sorting Layer value.
    pub layer_mask: u32,
    /// Row in the shadow maps (None = no shadows).
    pub shadow_row: Option<usize>,
    pub shadow_strength: f32,
    pub shadow_softness: f32,
}

/// The scene's 2D lights for one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lights2D {
    pub items: Vec<LightItem>,
    /// `SHADOW_BINS` distances per shadowed light (row = `shadow_row`).
    pub shadows: Vec<f32>,
}

impl Lights2D {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Light reaching a point on Sorting Layer value `layer` (linear RGB; CPU copy of the
    /// shader, for tests and probes).
    pub fn sample(&self, p: Vec2, layer: i32) -> Vec3 {
        let bit = (layer + LAYER_OFFSET).clamp(0, 31) as u32;
        let mut sum = Vec3::ZERO;
        for l in &self.items {
            if l.layer_mask & (1 << bit) == 0 {
                continue;
            }
            if l.kind == Light2DType::Global {
                sum += l.color;
                continue;
            }
            let d = p - l.position;
            let dist = d.length();
            if dist >= l.outer_radius {
                continue;
            }
            let mut a = falloff(dist, l.inner_radius, l.outer_radius, l.falloff_strength);
            if l.kind == Light2DType::Spot && dist > 1e-5 {
                a *= cone(d.dot(l.direction) / dist, l.inner_angle, l.outer_angle);
            }
            if let Some(row) = l.shadow_row {
                let lit = shadow_lit(&self.shadows[row * SHADOW_BINS..][..SHADOW_BINS], d);
                a *= 1.0 - l.shadow_strength * (1.0 - lit);
            }
            sum += l.color * a;
        }
        sum
    }
}

/// Bin of a direction from the light.
pub fn bin_of(d: Vec2) -> usize {
    let a = d.y.atan2(d.x);
    let t = (a + std::f32::consts::PI) / std::f32::consts::TAU;
    ((t * SHADOW_BINS as f32) as usize).min(SHADOW_BINS - 1)
}

/// Direction at the centre of a bin.
pub fn bin_direction(b: usize) -> Vec2 {
    let a = (b as f32 + 0.5) / SHADOW_BINS as f32 * std::f32::consts::TAU - std::f32::consts::PI;
    Vec2::new(a.cos(), a.sin())
}

/// Hard shadow test against one map row: 1 = lit.
pub fn shadow_lit(row: &[f32], d: Vec2) -> f32 {
    let occ = row[bin_of(d)];
    if d.length() > occ { 0.0 } else { 1.0 }
}

/// Where rays from `origin` along `dir` enter and leave a convex polygon (None = miss).
fn ray_polygon(origin: Vec2, dir: Vec2, poly: &[Vec2]) -> Option<(f32, f32)> {
    let mut enter = 0.0f32;
    let mut exit = f32::MAX;
    let n = poly.len();
    // Winding sign so outward normals point away from the centre.
    let area: f32 = (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum();
    let s = if area >= 0.0 { 1.0 } else { -1.0 };
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let e = b - a;
        // Outward normal of a counter-clockwise edge.
        let normal = Vec2::new(e.y, -e.x) * s;
        let denom = normal.dot(dir);
        let dist = normal.dot(a - origin);
        if denom.abs() < 1e-9 {
            if dist < 0.0 {
                return None;
            }
            continue;
        }
        let t = dist / denom;
        if denom < 0.0 {
            enter = enter.max(t);
        } else {
            exit = exit.min(t);
        }
        if enter > exit {
            return None;
        }
    }
    (exit > 0.0 && exit < f32::MAX).then_some((enter, exit))
}

/// A shadow caster outline in world XY.
#[derive(Clone, Debug, PartialEq)]
pub struct Caster {
    pub points: Vec<Vec2>,
    pub self_shadows: bool,
    /// Outlines of one caster object (a tilemap's boxes) share a group: a ray crossing
    /// touching boxes of the group counts them as one solid.
    pub group: usize,
}

/// One light's shadow map row: per direction, where the shadow of the nearest caster starts.
pub fn shadow_row(light: Vec2, radius: f32, casters: &[Caster]) -> Vec<f32> {
    // Per direction: (enter, exit, group, self shadows) of every caster hit.
    let mut hits: Vec<Vec<(f32, f32, usize, bool)>> = vec![Vec::new(); SHADOW_BINS];
    for c in casters {
        if c.points.len() < 3 {
            continue;
        }
        // Skip casters out of reach or holding the light.
        let (lo, hi) = c.points.iter().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        if light.clamp(lo, hi).distance(light) > radius {
            continue;
        }
        if light.cmpge(lo).all() && light.cmple(hi).all() {
            continue;
        }
        // Angular span: bins between the extreme vertex directions (convex, light outside).
        let center = c.points.iter().copied().sum::<Vec2>() / c.points.len() as f32;
        let base = (center - light).to_angle();
        let (mut amin, mut amax) = (0.0f32, 0.0f32);
        for p in &c.points {
            let mut a = (*p - light).to_angle() - base;
            a = (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            amin = amin.min(a);
            amax = amax.max(a);
        }
        let step = std::f32::consts::TAU / SHADOW_BINS as f32;
        let first = ((base + amin + std::f32::consts::PI) / step).floor() as i64 - 1;
        let last = ((base + amax + std::f32::consts::PI) / step).ceil() as i64 + 1;
        for k in first..=last {
            let b = k.rem_euclid(SHADOW_BINS as i64) as usize;
            let Some((enter, exit)) = ray_polygon(light, bin_direction(b), &c.points) else {
                continue;
            };
            hits[b].push((enter, exit, c.group, c.self_shadows));
        }
    }
    hits.into_iter()
        .map(|mut h| {
            h.sort_by(|a, b| a.0.total_cmp(&b.0));
            let Some(&(enter, mut exit, group, selfish)) = h.first() else {
                return NO_SHADOW;
            };
            if selfish {
                return enter;
            }
            // Walk through touching outlines of the same object to its far side.
            for &(e, x, g, _) in &h[1..] {
                if g == group && e <= exit + 1e-3 {
                    exit = exit.max(x);
                }
            }
            exit
        })
        .collect()
}

/// Shadow caster outlines of the scene's enabled Shadow Caster 2D objects.
pub fn casters(scene: &Scene) -> Vec<Caster> {
    let mut out = Vec::new();
    for (group, e) in scene
        .active_with::<ShadowCaster2D>()
        .into_iter()
        .enumerate()
    {
        let Ok(c) = scene.world.get::<&ShadowCaster2D>(e).map(|c| (*c).clone()) else {
            continue;
        };
        if !c.enabled {
            continue;
        }
        let m = scene.world_matrix(e);
        let to_world = |p: Vec2| m.transform_point3(p.extend(0.0)).truncate();
        if let Ok(map) = scene.world.get::<&crate::tilemap::Tilemap>(e) {
            let grid = crate::tilemap::grid_of(scene, e);
            for s in crate::tilemap::collider::shapes(&map, &grid, true) {
                out.push(Caster {
                    points: s.points().into_iter().map(to_world).collect(),
                    self_shadows: c.self_shadows,
                    group,
                });
            }
            continue;
        }
        let (lo, hi) =
            crate::sprites::local_bounds(scene, e).unwrap_or((Vec2::splat(-0.5), Vec2::splat(0.5)));
        let pts = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
        out.push(Caster {
            points: pts.into_iter().map(to_world).collect(),
            self_shadows: c.self_shadows,
            group,
        });
    }
    out
}

/// The enabled, active 2D lights of a scene with their shadow maps (empty when there are none).
pub fn collect(scene: &Scene) -> Lights2D {
    let lights = scene.active_with::<Light2D>();
    if lights.is_empty() {
        return Lights2D::default();
    }
    let layers = crate::sprites::sorting::layers();
    let mut items: Vec<(Light2D, LightItem)> = Vec::new();
    for e in lights {
        let Ok(l) = scene.world.get::<&Light2D>(e).map(|l| (*l).clone()) else {
            continue;
        };
        if !l.enabled || l.intensity <= 0.0 {
            continue;
        }
        let m = scene.world_matrix(e);
        let position = m.transform_point3(Vec3::ZERO).truncate();
        // Spots point along the object's up axis, like a 2D light.
        let direction = m
            .transform_vector3(Vec3::Y)
            .truncate()
            .normalize_or(Vec2::Y);
        let mut mask = 0u32;
        for name in &layers {
            if l.targets(name) {
                let v = crate::sprites::sorting::value_in(&layers, name);
                mask |= 1 << (v + LAYER_OFFSET).clamp(0, 31);
            }
        }
        let color = Vec3::from(crate::render::srgb_to_linear(l.color)) * l.intensity;
        items.push((
            l.clone(),
            LightItem {
                kind: l.kind,
                position,
                direction,
                color,
                inner_radius: l.inner_radius.max(0.0).min(l.outer_radius),
                outer_radius: l.outer_radius.max(1e-3),
                inner_angle: l.inner_angle,
                outer_angle: l.outer_angle,
                falloff_strength: l.falloff_strength,
                layer_mask: mask,
                shadow_row: None,
                shadow_strength: l.shadow_strength.clamp(0.0, 1.0),
                shadow_softness: l.shadow_softness.max(0.0),
            },
        ));
    }
    // Globals first, then the brightest point lights.
    items.sort_by(|a, b| {
        let g = |l: &LightItem| l.kind != Light2DType::Global;
        g(&a.1)
            .cmp(&g(&b.1))
            .then(b.1.color.length().total_cmp(&a.1.color.length()))
    });
    items.truncate(MAX_LIGHTS);
    let mut out = Lights2D::default();
    let wants_shadows = items
        .iter()
        .any(|(l, i)| l.shadows_enabled && i.kind != Light2DType::Global);
    let casters = if wants_shadows {
        casters(scene)
    } else {
        Vec::new()
    };
    for (l, mut item) in items {
        if l.shadows_enabled && item.kind != Light2DType::Global && !casters.is_empty() {
            item.shadow_row = Some(out.shadows.len() / SHADOW_BINS);
            out.shadows
                .extend(shadow_row(item.position, item.outer_radius, &casters));
        }
        out.items.push(item);
    }
    out
}

#[cfg(test)]
mod tests;
