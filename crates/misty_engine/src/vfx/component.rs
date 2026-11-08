//! The Visual Effect component (saved) and its runtime instance (spawn state, step counter,
//! property data the renderer uploads).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::codegen::{CompiledEffect, property_data, ramp_data};
use super::graph::PropertyValue;
use super::library;
use super::spawn::{self, SpawnState};

/// A per-instance value for an exposed property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyOverride {
    pub name: String,
    pub value: PropertyValue,
}

/// Plays an Effect Graph asset on the GPU.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualEffect {
    /// Project path of the `.effect` asset.
    pub effect: Option<String>,
    /// Start spawning when the effect first runs.
    pub play_on_awake: bool,
    /// Random seed of the simulation.
    pub seed: u32,
    /// Pick a new seed on every Play / Reinit.
    pub reseed_on_play: bool,
    /// Exposed property values that differ from the graph's defaults.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<PropertyOverride>,
}

impl Default for VisualEffect {
    fn default() -> Self {
        Self {
            effect: None,
            play_on_awake: true,
            seed: 0,
            reseed_on_play: true,
            overrides: Vec::new(),
        }
    }
}

impl VisualEffect {
    pub fn with_effect(path: &str) -> Self {
        Self {
            effect: Some(path.to_owned()),
            ..Default::default()
        }
    }

    pub fn override_value(&self, name: &str) -> Option<&PropertyValue> {
        self.overrides
            .iter()
            .find(|o| o.name == name)
            .map(|o| &o.value)
    }

    /// Set (or replace) an override.
    pub fn set_override(&mut self, name: &str, value: PropertyValue) {
        match self.overrides.iter_mut().find(|o| o.name == name) {
            Some(o) => o.value = value,
            None => self.overrides.push(PropertyOverride {
                name: name.to_owned(),
                value,
            }),
        }
    }

    pub fn clear_override(&mut self, name: &str) {
        self.overrides.retain(|o| o.name != name);
    }

    fn override_list(&self) -> Vec<(String, PropertyValue)> {
        self.overrides
            .iter()
            .map(|o| (o.name.clone(), o.value.clone()))
            .collect()
    }
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Runtime state of a Visual Effect (not saved; a fresh one starts with every Play).
#[derive(Clone, Debug)]
pub struct EffectInstance {
    /// Unique id (the renderer keeps the GPU particles under it).
    pub id: u64,
    pub effect: Option<Arc<CompiledEffect>>,
    /// Compile errors of the asset, when it doesn't compile.
    pub error: Option<String>,
    path: Option<String>,
    generation: u64,
    /// Spawn blocks run (Play) or not (Stop: live particles finish their lives).
    pub playing: bool,
    pub spawn: SpawnState,
    /// Seconds simulated since the last Reinit.
    pub total_time: f32,
    /// Step counter: the renderer simulates once per new value.
    pub serial: u64,
    pub dt: f32,
    pub spawn_count: u32,
    /// Bumped by Reinit (and effect changes): the renderer clears the particles.
    pub reset_gen: u64,
    pub seed: u32,
    /// Properties uniform and baked ramps for the current overrides.
    pub props: Arc<Vec<[f32; 4]>>,
    pub ramps: Arc<Vec<[f32; 4]>>,
    /// Content key of `props` + `ramps`.
    pub data_key: u64,
    /// Total spawned since Reinit, as counted on the CPU (capped by capacity on the GPU).
    pub requested: u64,
}

impl EffectInstance {
    pub fn new(component: &VisualEffect) -> Self {
        let mut i = Self {
            id: next_id(),
            effect: None,
            error: None,
            path: None,
            generation: 0,
            playing: false,
            spawn: SpawnState::default(),
            total_time: 0.0,
            serial: 0,
            dt: 0.0,
            spawn_count: 0,
            reset_gen: 1,
            seed: component.seed,
            props: Arc::new(Vec::new()),
            ramps: Arc::new(Vec::new()),
            data_key: 0,
            requested: 0,
        };
        i.refresh(component);
        i
    }

    /// An instance of an already compiled effect (no asset file; tests and tools).
    pub fn with_effect(effect: Arc<CompiledEffect>, component: &VisualEffect) -> Self {
        let mut i = Self::new(&VisualEffect {
            effect: None,
            ..component.clone()
        });
        i.effect = Some(effect);
        i.refresh(component);
        i
    }

    /// What the renderer needs this frame (None without a compiled effect).
    pub fn draw(&self, world: glam::Mat4, component: &VisualEffect) -> Option<super::EffectDraw> {
        let effect = self.effect.clone()?;
        Some(super::EffectDraw {
            id: self.id,
            bounds: super::world_bounds(&effect, &world),
            textures: effect
                .all_outputs()
                .map(|o| super::output_texture(&effect, o, component))
                .collect(),
            effect,
            world,
            serial: self.serial,
            dt: self.dt,
            time: self.total_time,
            spawn_count: self.spawn_count,
            seed: self.seed,
            reset_gen: self.reset_gen,
            props: self.props.clone(),
            ramps: self.ramps.clone(),
            data_key: self.data_key,
        })
    }

    fn new_seed(&mut self, component: &VisualEffect) {
        self.seed = if component.reseed_on_play {
            (self.id as u32)
                .wrapping_mul(2_654_435_761)
                .wrapping_add(self.reset_gen as u32)
                ^ component.seed
        } else {
            component.seed
        };
    }

    /// Start spawning (from the start of the spawn timeline).
    pub fn play(&mut self, component: &VisualEffect) {
        if !self.playing {
            self.spawn = SpawnState::default();
        }
        self.playing = true;
        if component.reseed_on_play {
            self.new_seed(component);
        }
    }

    /// Stop spawning; live particles finish.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Clear every particle and start over (playing).
    pub fn reinit(&mut self, component: &VisualEffect) {
        self.reset_gen += 1;
        self.spawn = SpawnState::default();
        self.total_time = 0.0;
        self.requested = 0;
        self.playing = true;
        self.new_seed(component);
    }

    /// Follow the component's asset (reloads, working copies) and property overrides.
    pub fn refresh(&mut self, component: &VisualEffect) {
        let path = component.effect.clone();
        if path.is_none() && self.path.is_none() && self.generation == 0 && self.effect.is_some() {
            // Built with `with_effect`: only the property data follows the component.
            return self.refresh_data(component);
        }
        let (generation, compiled) = match &path {
            Some(p) => {
                let (g, c) = library::get(p);
                (g, Some(c))
            }
            None => (0, None),
        };
        if path != self.path || generation != self.generation {
            let old_hash = self.effect.as_ref().map(|e| e.hash);
            self.path = path;
            self.generation = generation;
            match compiled {
                Some(Ok(e)) => {
                    self.error = None;
                    if old_hash != Some(e.hash) {
                        // New shaders or capacity: start over.
                        self.reset_gen += 1;
                        self.spawn = SpawnState::default();
                        self.total_time = 0.0;
                        self.requested = 0;
                    }
                    self.effect = Some(e);
                }
                Some(Err(errors)) => {
                    self.error = Some(
                        errors
                            .iter()
                            .map(|e| e.message.clone())
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                    self.effect = None;
                }
                None => {
                    self.error = None;
                    self.effect = None;
                }
            }
            self.data_key = 0;
        }
        self.refresh_data(component);
    }

    fn refresh_data(&mut self, component: &VisualEffect) {
        let Some(effect) = &self.effect else {
            return;
        };
        let overrides = component.override_list();
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            effect.hash.hash(&mut h);
            format!("{overrides:?}{:?}", effect.properties).hash(&mut h);
            h.finish() | 1
        };
        if key != self.data_key {
            self.data_key = key;
            self.props = Arc::new(property_data(effect, &overrides));
            self.ramps = Arc::new(ramp_data(effect, &overrides));
        }
    }

    /// One simulation step of `dt` seconds.
    pub fn step(&mut self, component: &VisualEffect, dt: f32) {
        self.refresh(component);
        let dt = dt.max(0.0);
        self.dt = dt;
        self.total_time += dt;
        self.serial += 1;
        self.spawn_count = 0;
        let Some(effect) = &self.effect else {
            return;
        };
        if self.playing {
            let overrides = component.override_list();
            let props = spawn::Props {
                defaults: &effect.properties,
                overrides: &overrides,
            };
            let n = spawn::step(&effect.spawn, &mut self.spawn, &props, dt);
            self.spawn_count = n.min(effect.capacity());
            self.requested += self.spawn_count as u64;
        }
    }
}
