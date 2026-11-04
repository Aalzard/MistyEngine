//! `done_when` conditions: small strings checked every frame while a step is shown.
//!
//! - `selected:<name>` the active object has this name;
//! - `has:<Component>` some object has this component;
//! - `undo:<step name>` a new undo step with this name was made during the step;
//! - `open:<Tab title>` that tab is drawn; `play` Play mode is on;
//! - `ppv:…`, `sun:…`, `terrain:…`, `wb:…` named checks of the Post-process Volume, the
//!   sun, the selected terrain's data and the World Builder (see [`named`]).

use misty_engine::hecs::Entity;
use misty_engine::scene::{
    CloudPreset, CloudsOverride, FogOverride, Light, LightType, PostProcessVolume, Shadows,
    Terrain, registry,
};
use misty_engine::terrain::TerrainData;
use misty_engine::world_gen::stamps::WaterMode;
use misty_engine::world_gen::{self, StampKind, WorldBuilder, WorldStamp};

use crate::state::EditorState;

/// What a condition can look at.
pub struct Check<'a> {
    pub state: &'a EditorState,
    pub ctx: &'a egui::Context,
    /// Undo step names when the step started.
    pub undo_before: &'a [String],
}

/// Whether `cond` holds; `None` when the condition is not known.
pub fn eval(cond: &str, c: &Check) -> Option<bool> {
    let s = c.state;
    let (kind, arg) = cond.split_once(':').unwrap_or((cond, ""));
    Some(match kind {
        "selected" => s.selection.active().is_some_and(|e| s.scene.name(e) == arg),
        "has" => {
            let info = registry::find(arg)?;
            s.scene
                .walk()
                .iter()
                .any(|(e, _)| (info.has)(&s.scene.world, *e))
        }
        "undo" => count(s.undo.steps(), arg) > count(c.undo_before.iter().map(String::as_str), arg),
        "open" => super::targets::rect(c.ctx, &format!("panel.{arg}")).is_some(),
        "play" => s.play.in_play_mode(),
        _ => named(kind, arg, s)?,
    })
}

fn count<'a>(steps: impl IntoIterator<Item = &'a str>, label: &str) -> usize {
    steps.into_iter().filter(|l| *l == label).count()
}

/// The checks behind `ppv:`, `sun:`, `terrain:` and `wb:`.
fn named(kind: &str, arg: &str, s: &EditorState) -> Option<bool> {
    let missing = Some(false);
    match kind {
        "ppv" => {
            let Some(v) = volume(s) else { return missing };
            ppv(arg, &v)
        }
        "sun" => {
            let Some(l) = sun(s) else { return missing };
            Some(match arg {
                "shadows" => l.shadows != Shadows::None,
                "volumetric" => l.volumetric_multiplier >= 1.5,
                _ => return None,
            })
        }
        "terrain" => {
            let Some(d) = terrain(s) else {
                return terrain_check(arg, &TerrainData::default()).map(|_| false);
            };
            terrain_check(arg, &d)
        }
        "wb" => wb(arg, s),
        _ => None,
    }
}

fn ppv(arg: &str, v: &PostProcessVolume) -> Option<bool> {
    let (f, c): (&FogOverride, &CloudsOverride) = (&v.fog, &v.clouds);
    let fog_on = f.active && f.enabled;
    let base = FogOverride::default();
    Some(match arg {
        "tonemapping" => v.tonemapping.active,
        "bloom_color" => v.bloom.active && v.color_adjustments.active,
        "volumetric" => fog_on && f.volumetric,
        "anisotropy" => fog_on && f.anisotropy >= 0.6,
        "heights" => {
            fog_on && (f.base_height != base.base_height || f.maximum_height != base.maximum_height)
        }
        "ambient_low" => fog_on && f.ambient_dimmer <= 0.5,
        "sun_shafts" => v.sun_shafts.active,
        "clouds" => c.is_on() && CloudPreset::matching(c) != Some(CloudPreset::Cloudy),
        _ => return None,
    })
}

fn terrain_check(arg: &str, d: &TerrainData) -> Option<bool> {
    let textured = |i: usize| d.layers.get(i).is_some_and(|l| l.diffuse.is_some());
    let rules = d.layers.iter().filter(|l| l.rules.enabled);
    Some(match arg {
        "layers1" => !d.layers.is_empty(),
        "texture1" => textured(0),
        "layers2" => textured(1),
        "rule_slope" => d
            .layers
            .iter()
            .any(|l| l.rules.enabled && l.rules.slope[0] >= 20.0),
        "rules2" => rules.count() >= 2,
        _ => return None,
    })
}

fn wb(arg: &str, s: &EditorState) -> Option<bool> {
    let scene = &s.scene;
    let builder = world_gen::builders(scene).first().copied();
    let settings = builder.and_then(|b| {
        scene
            .world
            .get::<&WorldBuilder>(b)
            .ok()
            .map(|w| (*w).clone())
    });
    let stamps: Vec<WorldStamp> = builder
        .map(|b| world_gen::stamps_of(scene, b))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| scene.world.get::<&WorldStamp>(e).ok().map(|w| (*w).clone()))
        .collect();
    let any = |f: fn(&StampKind) -> bool| stamps.iter().any(|w| f(&w.kind));
    Some(match arg {
        "generator" => {
            settings.is_some_and(|w| w.base.generator != WorldBuilder::default().base.generator)
        }
        "mountain" => any(|k| matches!(k, StampKind::Mountain(_))),
        "sea" => any(|k| matches!(k, StampKind::Water(w) if w.mode == WaterMode::Sea)),
        "zone" => any(|k| matches!(k, StampKind::BiomeZone(z) if !z.biome.is_empty())),
        "biome_open" => s.world_builder.biome.is_some(),
        _ => return None,
    })
}

// ---- what the checks look at -------------------------------------------------------------------

/// The first Post-process Volume in the scene.
pub fn volume_entity(s: &EditorState) -> Option<Entity> {
    s.scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .find(|e| s.scene.world.get::<&PostProcessVolume>(*e).is_ok())
}

fn volume(s: &EditorState) -> Option<PostProcessVolume> {
    let e = volume_entity(s)?;
    s.scene
        .world
        .get::<&PostProcessVolume>(e)
        .ok()
        .map(|v| (*v).clone())
}

/// The first directional light.
pub fn sun_entity(s: &EditorState) -> Option<Entity> {
    s.scene.walk().into_iter().map(|(e, _)| e).find(|e| {
        s.scene
            .world
            .get::<&Light>(*e)
            .is_ok_and(|l| l.kind == LightType::Directional)
    })
}

fn sun(s: &EditorState) -> Option<Light> {
    let e = sun_entity(s)?;
    s.scene.world.get::<&Light>(e).ok().map(|l| (*l).clone())
}

/// The selected terrain, else the first one.
pub fn terrain_entity(s: &EditorState) -> Option<Entity> {
    let is_terrain = |e: &Entity| s.scene.world.get::<&Terrain>(*e).is_ok();
    s.selection
        .active()
        .filter(is_terrain)
        .or_else(|| s.scene.walk().into_iter().map(|(e, _)| e).find(is_terrain))
}

fn terrain(s: &EditorState) -> Option<std::sync::Arc<TerrainData>> {
    misty_engine::terrain::data_of(&s.scene, terrain_entity(s)?)
}
