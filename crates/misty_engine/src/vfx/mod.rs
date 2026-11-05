//! Visual Effects: GPU particle systems authored as Effect Graphs (`.effect`, JSON).
//!
//! A graph stacks four contexts like the reference editor's particle graphs: **Spawn** (how
//! many particles per step, on the CPU), **Initialize Particle**, **Update Particle** and one or
//! more **Output Particle** contexts, each a list of blocks whose inputs can be wired to operator
//! nodes and Blackboard properties. [`codegen`] turns a graph into WGSL compute kernels and
//! render shaders; the renderer (`render/vfx`) keeps every instance's particles, dead list and
//! alive lists in GPU buffers, simulates with indirect dispatches and draws indirectly.
//!
//! The saved [`VisualEffect`] component names the asset and holds per-instance property
//! overrides; the runtime [`EffectInstance`] (not in the registry, like particle emitters) holds
//! the spawn state and the step counter the renderer follows.

pub mod blocks;
pub mod codegen;
pub mod component;
pub mod graph;
pub mod library;
pub mod operators;
pub mod spawn;
pub mod types;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::{Mat4, Vec3};
use hecs::Entity;

pub use blocks::{Attribute, BlockKind, Composition, ContextType, OrientMode, ShapeMode};
pub use codegen::{CompiledEffect, GraphError, compile};
pub use component::{EffectInstance, PropertyOverride, VisualEffect};
pub use graph::{
    BlendMode, ContextKind, EffectGraph, EffectProperty, OutputKind, OutputSettings, PropertyValue,
    SimulationSpace,
};
pub use operators::{AttributeRead, OperatorKind, RandomMode};
pub use types::{Curve, Gradient, Value, ValueType};

use crate::render::culling::Aabb;
use crate::scene::Scene;

/// File extension of Effect Graph assets.
pub const EXTENSION: &str = "effect";

pub fn is_effect_path(path: &str) -> bool {
    crate::assets::has_extension(path, &[EXTENSION])
}

impl crate::scene::SceneComponent for VisualEffect {
    const NAME: &'static str = "Visual Effect";
    const CATEGORY: &'static str = "Effects";
}

/// What the renderer needs to simulate and draw one instance this frame.
#[derive(Clone, Debug)]
pub struct EffectDraw {
    pub id: u64,
    pub effect: Arc<CompiledEffect>,
    pub world: Mat4,
    /// World-space culling bounds.
    pub bounds: Aabb,
    pub serial: u64,
    pub dt: f32,
    pub time: f32,
    pub spawn_count: u32,
    pub seed: u32,
    pub reset_gen: u64,
    pub props: Arc<Vec<[f32; 4]>>,
    pub ramps: Arc<Vec<[f32; 4]>>,
    pub data_key: u64,
    /// Texture per output (project path; None = the soft dot).
    pub textures: Vec<Option<String>>,
}

/// Counters read back from the GPU (a frame or two late).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectStats {
    pub alive: u32,
    pub spawned: u32,
    pub reaped: u32,
}

fn stats_map() -> &'static Mutex<HashMap<u64, EffectStats>> {
    static MAP: OnceLock<Mutex<HashMap<u64, EffectStats>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// The renderer reports an instance's counters.
pub fn set_stats(id: u64, s: EffectStats) {
    stats_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, s);
}

/// Latest counters of an instance (None before the GPU reported any).
pub fn stats(id: u64) -> Option<EffectStats> {
    stats_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
        .copied()
}

/// Forget counters of instances that are gone.
pub fn forget_stats(id: u64) {
    stats_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
}

/// Effects on active objects.
fn active(scene: &Scene) -> Vec<Entity> {
    let mut list: Vec<Entity> = scene
        .world
        .query::<(Entity, &VisualEffect)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    list.retain(|e| scene.active_in_hierarchy(*e));
    list
}

/// Give `e` an instance if it has none (playing when Play On Awake is set). False without a
/// Visual Effect.
pub fn ensure_instance(scene: &mut Scene, e: Entity) -> bool {
    if scene.world.get::<&EffectInstance>(e).is_ok() {
        return true;
    }
    let Ok(c) = scene.world.get::<&VisualEffect>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let mut inst = EffectInstance::new(&c);
    if c.play_on_awake {
        inst.play(&c);
    }
    scene.world.insert_one(e, inst).is_ok()
}

/// Run `f` on the instance and component of `e` (the instance is created first if needed).
pub fn with_instance<R>(
    scene: &mut Scene,
    e: Entity,
    f: impl FnOnce(&mut EffectInstance, &mut VisualEffect) -> R,
) -> Option<R> {
    if !ensure_instance(scene, e) {
        return None;
    }
    let (c, i) = scene
        .world
        .query_one_mut::<(&mut VisualEffect, &mut EffectInstance)>(e)
        .ok()?;
    Some(f(i, c))
}

/// Step every active effect by `dt` (Play mode, and the Scene view in Edit mode).
pub fn update(scene: &mut Scene, dt: f32) {
    for e in active(scene) {
        step_one(scene, e, dt);
    }
}

/// Step one effect.
pub fn step_one(scene: &mut Scene, e: Entity, dt: f32) {
    ensure_instance(scene, e);
    if let Ok((c, i)) = scene
        .world
        .query_one_mut::<(&VisualEffect, &mut EffectInstance)>(e)
    {
        i.step(c, dt);
    }
}

/// Remove every instance (all particles), e.g. when Play starts.
pub fn clear_all(scene: &mut Scene) {
    let list: Vec<(Entity, u64)> = scene
        .world
        .query::<(Entity, &EffectInstance)>()
        .iter()
        .map(|(e, i)| (e, i.id))
        .collect();
    for (e, id) in list {
        forget_stats(id);
        let _ = scene.world.remove_one::<EffectInstance>(e);
    }
}

/// World-space bounds of an effect on an object.
pub fn world_bounds(effect: &CompiledEffect, world: &Mat4) -> Aabb {
    let half = effect.bounds_size.abs() * 0.5;
    Aabb::new(effect.bounds_center - half, effect.bounds_center + half).transformed(world)
}

/// Texture of an output for an instance (the texture property's value when it has one).
pub(crate) fn output_texture(
    effect: &CompiledEffect,
    o: &codegen::CompiledOutput,
    c: &VisualEffect,
) -> Option<String> {
    if let Some(name) = &o.settings.texture_property {
        if let Some(PropertyValue::Texture(t)) = c.override_value(name) {
            return t.clone();
        }
        if let Some(p) = effect.properties.iter().find(|p| &p.name == name)
            && let PropertyValue::Texture(t) = &p.value
        {
            return t.clone();
        }
    }
    o.settings.texture.clone()
}

/// Draw data of every effect with an instance.
pub fn draws(scene: &Scene) -> Vec<EffectDraw> {
    draws_where(scene, |_| true)
}

/// [`draws`] of the effects `keep` accepts (a camera's culling mask).
pub fn draws_where(scene: &Scene, keep: impl Fn(Entity) -> bool) -> Vec<EffectDraw> {
    let mut out = Vec::new();
    for e in active(scene) {
        if !keep(e) {
            continue;
        }
        let (Ok(c), Ok(i)) = (
            scene.world.get::<&VisualEffect>(e),
            scene.world.get::<&EffectInstance>(e),
        ) else {
            continue;
        };
        if let Some(d) = i.draw(scene.world_matrix(e), &c) {
            out.push(d);
        }
    }
    out
}

/// Bounds of the effect on `e` in world space (gizmos, framing), when it compiles.
pub fn bounds_of(scene: &Scene, e: Entity) -> Option<(Vec3, Vec3)> {
    let c = scene.world.get::<&VisualEffect>(e).ok()?;
    let path = c.effect.as_ref()?;
    let effect = library::get(path).1.ok()?;
    let b = world_bounds(&effect, &scene.world_matrix(e));
    Some((b.min, b.max))
}
