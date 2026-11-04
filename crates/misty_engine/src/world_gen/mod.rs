//! World generation (the editor's World Builder window): a base heightmap from shaped noise,
//! erosion, stamps and areas placed as scene objects, water, biome texturing and scatter.
//!
//! Scene layout: a "World Builder" object holds the settings ([`WorldBuilder`]); its children
//! with a [`WorldStamp`] are the stamps and areas; a "Generated" child holds what Generate made
//! (water planes and scatter). Settings, stamps and seeds are all saved with the scene, so
//! pressing Generate again reproduces the same terrain.

pub mod base;
pub mod biome;
pub mod classify;
pub mod erosion;
pub mod generate;
pub mod noise;
pub mod paint;
pub mod scatter;
pub mod scatter_set;
pub mod stamps;
#[cfg(test)]
mod tests;
pub mod textures;
pub mod water;

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use glam::{Mat4, Quat, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

pub use base::{BaseSettings, Generator};
pub use biome::{Biome, Feature, Preset, ScatterItem, ScatterShape};
pub use erosion::ErosionSettings;
pub use generate::{Output, Request, Stages};
pub use scatter_set::{ScatterPart, ScatterSet};
pub use stamps::{PlacedStamp, Placement, StampKind, WorldStamp};

use crate::scene::{MeshFilter, MeshRef, MeshRenderer, Scene, Terrain, Transform};

/// Name of the child that holds generated objects.
pub const GENERATED: &str = "Generated";
/// Bark material shared by every primitive tree trunk.
pub const BARK: &str = "Assets/WorldBuilder/Materials/Bark.mat";
/// Above this many instances, Auto storage packs scatter into Scatter components.
pub const OBJECT_LIMIT: usize = 20_000;

/// How generated scatter is stored in the scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScatterStorage {
    /// Objects, or Scatter components past [`OBJECT_LIMIT`] instances.
    #[default]
    Auto,
    /// One object per instance (editable one by one).
    Objects,
    /// One Scatter component per item (small files, very large counts).
    Compact,
}

impl ScatterStorage {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Objects, Self::Compact];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Objects => "Objects",
            Self::Compact => "Compact (Scatter component)",
        }
    }
}

/// World Builder settings (on the "World Builder" object).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldBuilder {
    /// Terrain data asset Generate writes.
    pub terrain_data: String,
    /// Terrain width, maximum height and length (metres).
    pub size: [f32; 3],
    pub heightmap_resolution: usize,
    pub alphamap_resolution: usize,
    pub base: BaseSettings,
    pub erosion: ErosionSettings,
    /// `.biome` used outside every Biome Zone.
    pub default_biome: String,
    /// Noise breakup of biome and area edges (metres).
    pub biome_breakup: f32,
    /// Regenerate a low-resolution preview while editing.
    pub live_preview: bool,
    pub scatter_storage: ScatterStorage,
}

impl Default for WorldBuilder {
    fn default() -> Self {
        Self {
            terrain_data: String::new(),
            size: [500.0, 150.0, 500.0],
            heightmap_resolution: 513,
            alphamap_resolution: 512,
            base: BaseSettings::default(),
            erosion: ErosionSettings::default(),
            default_biome: Preset::Forest.path(),
            biome_breakup: 12.0,
            live_preview: true,
            scatter_storage: ScatterStorage::Auto,
        }
    }
}

/// Progress of a running generation, shared with the UI thread; also carries Cancel.
#[derive(Default)]
pub struct Progress {
    fraction: AtomicU32,
    stage: Mutex<&'static str>,
    cancel: AtomicBool,
}

impl Progress {
    pub fn set(&self, stage: &'static str, fraction: f32) {
        *self.stage.lock().unwrap_or_else(|e| e.into_inner()) = stage;
        self.set_fraction(fraction);
    }

    pub fn set_fraction(&self, f: f32) {
        self.fraction.store(f.to_bits(), Ordering::Relaxed);
    }

    /// Current stage and its fraction (0..1).
    pub fn get(&self) -> (&'static str, f32) {
        let stage = *self.stage.lock().unwrap_or_else(|e| e.into_inner());
        (stage, f32::from_bits(self.fraction.load(Ordering::Relaxed)))
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Run `f(row_index, row)` over the rows of `out` on all cores. Rows are independent, so the
/// result doesn't depend on the thread count.
pub(crate) fn par_rows<T: Send>(out: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = out.len() / row_len.max(1);
    if rows == 0 {
        return;
    }
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(rows);
    let per = rows.div_ceil(threads);
    let f = &f;
    std::thread::scope(|s| {
        for (c, chunk) in out.chunks_mut(per * row_len).enumerate() {
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(row_len).enumerate() {
                    f(c * per + r, row);
                }
            });
        }
    });
}

// ---- scene integration --------------------------------------------------------------------

/// Objects with World Builder settings, in Hierarchy order.
pub fn builders(scene: &Scene) -> Vec<Entity> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.world.get::<&WorldBuilder>(*e).is_ok())
        .collect()
}

/// The World Builder that owns `e` (itself or an ancestor).
pub fn builder_of(scene: &Scene, e: Entity) -> Option<Entity> {
    let mut cur = Some(e);
    while let Some(c) = cur {
        if scene.world.get::<&WorldBuilder>(c).is_ok() {
            return Some(c);
        }
        cur = scene.parent(c);
    }
    None
}

/// Stamp objects under a builder, in Hierarchy order (generated objects excluded).
pub fn stamps_of(scene: &Scene, builder: Entity) -> Vec<Entity> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| *e != builder && scene.is_ancestor(builder, *e))
        .filter(|e| scene.world.get::<&WorldStamp>(*e).is_ok())
        .collect()
}

/// The terrain object drawing the builder's terrain data.
pub fn terrain_of(scene: &Scene, builder: Entity) -> Option<Entity> {
    let path = scene
        .world
        .get::<&WorldBuilder>(builder)
        .ok()?
        .terrain_data
        .clone();
    scene.walk().into_iter().map(|(e, _)| e).find(|e| {
        scene
            .world
            .get::<&Terrain>(*e)
            .is_ok_and(|t| !path.is_empty() && t.terrain_data == path)
    })
}

/// Gather a generation request from the scene. `load` reads a `.biome` asset by project path.
/// Returns the request and warnings (missing biomes fall back to defaults).
pub fn request(
    scene: &Scene,
    builder: Entity,
    stages: Stages,
    preview: bool,
    current: Option<crate::terrain::TerrainData>,
    load: &dyn Fn(&str) -> Result<Biome, String>,
) -> Result<(Request, Vec<String>), String> {
    let settings = (*scene
        .world
        .get::<&WorldBuilder>(builder)
        .map_err(|_| "Not a World Builder".to_string())?)
    .clone();
    let terrain = terrain_of(scene, builder).ok_or("The World Builder has no terrain")?;
    let corner = scene.world_position(terrain);
    let mut warnings = Vec::new();
    let mut paths = vec![settings.default_biome.clone()];
    let mut stamps = Vec::new();
    let mut zone_biome = Vec::new();
    for (i, e) in stamps_of(scene, builder).into_iter().enumerate() {
        if !scene.active_in_hierarchy(e) {
            continue;
        }
        let stamp = (*scene.world.get::<&WorldStamp>(e).expect("listed")).clone();
        let b = match &stamp.kind {
            StampKind::BiomeZone(z) if !z.biome.is_empty() => {
                paths.iter().position(|p| *p == z.biome).unwrap_or_else(|| {
                    paths.push(z.biome.clone());
                    paths.len() - 1
                })
            }
            _ => 0,
        };
        zone_biome.push(b);
        stamps.push(PlacedStamp {
            stamp,
            at: Placement::from_matrix(&scene.world_matrix(e), corner),
            seed: settings.base.seed.wrapping_add(i as u32 * 7919),
        });
    }
    let biomes = paths
        .iter()
        .map(|p| {
            load(p).unwrap_or_else(|e| {
                warnings.push(format!("Biome {p}: {e} (using defaults)"));
                Biome::default()
            })
        })
        .collect();
    Ok((
        Request {
            settings,
            stamps,
            biomes,
            zone_biome,
            stages,
            current,
            preview,
        },
        warnings,
    ))
}

/// Material asset of a biome scatter item's main part.
pub fn material_path(biome: &Biome, item: &ScatterItem) -> String {
    let clean = |s: &str| s.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    format!(
        "{}/Materials/{} {}.mat",
        biome::FOLDER,
        clean(&biome.name),
        clean(&item.name)
    )
}

/// Colour of an item tinted by its biome tone.
pub fn item_color(biome: &Biome, item: &ScatterItem) -> [f32; 3] {
    let t = 0.15;
    [0, 1, 2].map(|i| item.color[i] * (1.0 - t) + biome.tone[i] * t)
}

/// Write the materials generated objects use (water, bark, one per scatter item), replacing
/// stale ones. `root` = project folder.
pub fn write_materials(root: &Path, biomes: &[Biome]) -> Result<(), String> {
    use crate::assets::Material;
    let mut list: Vec<(String, Material)> = vec![
        (water::MATERIAL.into(), water::material()),
        (
            BARK.into(),
            Material {
                color: [0.33, 0.24, 0.16, 1.0],
                smoothness: 0.1,
                ..Default::default()
            },
        ),
    ];
    for b in biomes {
        for item in b.scatter.iter().filter(|i| i.prefab.is_none()) {
            let c = item_color(b, item);
            let rock = item.shape.is_rock();
            list.push((
                material_path(b, item),
                Material {
                    color: [c[0], c[1], c[2], 1.0],
                    smoothness: if rock { 0.2 } else { 0.08 },
                    ..Default::default()
                },
            ));
        }
    }
    for (rel, m) in list {
        let path = root.join(&rel);
        if Material::load(&path).is_ok_and(|old| old == m) {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        m.save(&path)?;
    }
    Ok(())
}

/// Give `e` a world matrix (whatever its parent).
pub fn set_world_matrix(scene: &mut Scene, e: Entity, m: Mat4) {
    let parent = scene
        .parent(e)
        .map(|p| scene.world_matrix(p))
        .unwrap_or(Mat4::IDENTITY);
    let (scale, rotation, position) = (parent.inverse() * m).to_scale_rotation_translation();
    let mut t = Transform::from_position(position);
    t.set_rotation(rotation);
    t.scale = scale;
    scene.set_transform(e, t);
}

/// Child of `parent` named `name`, created when missing.
fn child(scene: &mut Scene, parent: Entity, name: &str) -> Entity {
    scene
        .children(parent)
        .into_iter()
        .find(|c| scene.name(*c) == name)
        .unwrap_or_else(|| scene.spawn(name, Some(parent)))
}

/// Replace the child `name` of `parent` with an empty one.
fn fresh_child(scene: &mut Scene, parent: Entity, name: &str) -> Entity {
    if let Some(old) = scene
        .children(parent)
        .into_iter()
        .find(|c| scene.name(*c) == name)
    {
        scene.despawn(old);
    }
    scene.spawn(name, Some(parent))
}

/// Put a generation's water and scatter into the scene under the builder's "Generated" child.
/// `prefab` loads a prefab asset by project path. Returns the number of scatter instances.
pub fn apply(
    scene: &mut Scene,
    builder: Entity,
    req: &Request,
    out: &Output,
    prefab: &dyn Fn(&str) -> Option<crate::scene::prefab::Prefab>,
) -> usize {
    let Some(terrain) = terrain_of(scene, builder) else {
        return 0;
    };
    let corner = scene.world_position(terrain);
    let generated = child(scene, builder, GENERATED);
    if !req.preview {
        let water_root = fresh_child(scene, generated, "Water");
        for body in &out.water {
            water::spawn_water(scene, Some(water_root), body, corner);
        }
    }
    let Some(batches) = &out.scatter else {
        return 0;
    };
    let scatter_root = fresh_child(scene, generated, "Scatter");
    let total: usize = batches.iter().map(|b| b.instances.len()).sum();
    let compact = match req.settings.scatter_storage {
        ScatterStorage::Auto => total > OBJECT_LIMIT,
        ScatterStorage::Objects => false,
        ScatterStorage::Compact => true,
    };
    for batch in batches {
        let (Some(b), true) = (req.biomes.get(batch.biome), !batch.instances.is_empty()) else {
            continue;
        };
        let item = &b.scatter[batch.item];
        let name = format!("{} {}", b.name, item.name);
        let parts = primitive_parts(b, item);
        let holder = scene.spawn(&name, Some(scatter_root));
        set_world_matrix(scene, holder, Mat4::from_translation(corner));
        let asset = item.prefab.as_deref().and_then(prefab);
        if compact && asset.is_none() {
            scene.add(holder, ScatterSet::new(parts, &batch.instances));
            continue;
        }
        for inst in &batch.instances {
            let m = Mat4::from_translation(corner)
                * Mat4::from_scale_rotation_translation(
                    Vec3::splat(inst.scale),
                    scatter_set::instance_rotation(inst.yaw, inst.up),
                    inst.position,
                );
            let e = match (&asset, item.prefab.as_deref()) {
                (Some(p), path) => scene.instantiate_prefab(p, path, Some(holder)),
                _ => {
                    let e = scene.spawn(&item.name, Some(holder));
                    spawn_parts(scene, e, &parts);
                    e
                }
            };
            set_world_matrix(scene, e, m);
        }
    }
    total
}

/// Parts of a primitive scatter item, with their materials.
fn primitive_parts(b: &Biome, item: &ScatterItem) -> Vec<ScatterPart> {
    scatter_set::shape_parts(item.shape)
        .into_iter()
        .map(|(mesh, offset, scale, role)| ScatterPart {
            mesh: MeshRef::Primitive(mesh),
            material: Some(match role {
                scatter_set::PartRole::Trunk => BARK.into(),
                scatter_set::PartRole::Main => material_path(b, item),
            }),
            offset,
            scale,
        })
        .collect()
}

/// One object per instance: a single part sits on the instance object, several get children.
fn spawn_parts(scene: &mut Scene, e: Entity, parts: &[ScatterPart]) {
    let renderer = |p: &ScatterPart| MeshRenderer {
        material: p.material.clone(),
        ..Default::default()
    };
    let filter = |p: &ScatterPart| MeshFilter {
        mesh: Some(p.mesh.clone()),
    };
    for (i, p) in parts.iter().enumerate() {
        let c = scene.spawn(if i == 0 { "Trunk" } else { "Crown" }, Some(e));
        let mut t = Transform::from_position(Vec3::from(p.offset));
        t.rotation = Quat::IDENTITY;
        t.scale = Vec3::from(p.scale);
        scene.set_transform(c, t);
        scene.add(c, filter(p));
        scene.add(c, renderer(p));
        if parts.len() == 1 {
            scene.game_object_mut(c).name = "Mesh".into();
        }
    }
}
