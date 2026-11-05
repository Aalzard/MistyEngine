//! Cached, incremental scene → mesh draws: the same list as
//! [`crate::render_scene::mesh_draws_culled`], but only objects whose inputs changed are
//! rebuilt.
//!
//! There is no change tracking in the scene (scripts, physics and the editor all write
//! components directly), so every input is compared with the copy kept from the last call:
//! local transforms down the hierarchy (world matrices are only recomputed below a change),
//! the Mesh Filter, the Mesh Renderer, the selection outline, LOD levels and each distinct
//! material's resolved values (checked once per call per material, not per object). An
//! unchanged static scene costs one walk of the hierarchy with component views.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::light_probes::LightProbeUsage;
use crate::render::lod::LodRange;
use crate::render::{MaterialParams, MeshDraw};
use crate::render_scene::MaterialSource;
use crate::scene::{
    GameObject, LodGroup, MeshFilter, MeshRef, MeshRenderer, Node, Scene, SpriteRenderer, Transform,
};

/// What the last [`DrawCache::update`] did (tests, Profiler).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Renderers whose draws were rebuilt / kept.
    pub rebuilt: u32,
    pub kept: u32,
    /// The draw list was rebuilt from scratch (objects added, removed, hidden or reordered).
    pub relaid: bool,
}

#[derive(Clone, Default)]
struct NodeEntry {
    entity: Option<Entity>,
    parent: Option<Entity>,
    local: Transform,
    world: Mat4,
}

struct RendererEntry {
    entity: Entity,
    mesh: MeshRef,
    renderer: MeshRenderer,
    outline: bool,
    lod: Option<LodRange>,
    /// Per Materials element: (material table index, version) — `None` for script instances.
    materials: Vec<Option<(u32, u64)>>,
    /// Draws in the output list.
    first: u32,
    count: u32,
    /// The call that last listed it (its range is only valid in the following call).
    call: u64,
    /// Merged into a static batch (Static object, no LOD Group, not outlined).
    batched: bool,
    /// Static objects don't use light probes.
    is_static: bool,
}

/// A distinct material path's resolved values.
struct MaterialEntry {
    path: Option<String>,
    params: MaterialParams,
    version: u64,
    checked: u64,
}

/// One object to draw this call.
struct Renderable {
    entity: Entity,
    world_changed: bool,
    outline: bool,
    is_static: bool,
}

/// Static batching state (see [`crate::render::static_batch`]).
#[derive(Default)]
struct Batching {
    on: bool,
    /// Runtime mesh name prefix of this cache's batches.
    name: String,
    /// Objects merged by the last build, and the merged draws.
    members: Vec<Entity>,
    merged: Vec<MeshDraw>,
    /// A member was rebuilt this call (moved, new material, …).
    dirty: bool,
    /// Unbatched draws + merged draws.
    out: Arc<Vec<MeshDraw>>,
    meshes: usize,
}

/// Persistent draw list of one view (keep one per view: Scene view, Game view, player).
#[derive(Default)]
pub struct DrawCache {
    nodes: Vec<NodeEntry>,
    renderers: Vec<Option<RendererEntry>>,
    /// Renderers in output order (the previous call's).
    order: Vec<Entity>,
    materials: Vec<MaterialEntry>,
    material_index: HashMap<Option<String>, u32>,
    out: Arc<Vec<MeshDraw>>,
    /// Draws before the sprites (which are rebuilt every call).
    mesh_draws: usize,
    calls: u64,
    stats: CacheStats,
    batching: Batching,
    /// Key namespace of this view's skinned mesh poses (see [`crate::skinning`]).
    skin_view: u64,
}

impl Drop for DrawCache {
    fn drop(&mut self) {
        if !self.batching.name.is_empty() {
            crate::models::runtime::unregister(&format!("{}/", self.batching.name));
        }
    }
}

impl DrawCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Merge Static objects sharing a material into chunked meshes (Play mode and the player;
    /// the Scene view keeps objects separate for picking and outlines).
    pub fn set_static_batching(&mut self, on: bool) {
        if on && self.batching.name.is_empty() {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.batching.name = format!("static-batch-{n}");
        }
        if on != self.batching.on {
            self.batching.on = on;
            self.batching.members.clear();
            self.batching.dirty = true;
        }
    }

    /// Static batches the last call drew.
    pub fn static_batches(&self) -> usize {
        if self.batching.on {
            self.batching.merged.len()
        } else {
            0
        }
    }

    /// The visible draws of objects whose layer is in `culling_mask`, then Sprite Renderers.
    /// `selected(e)` = draw the selection outline around `e` and its children.
    pub fn update(
        &mut self,
        scene: &Scene,
        materials: &dyn MaterialSource,
        culling_mask: u32,
        selected: impl Fn(Entity) -> bool,
    ) -> Arc<Vec<MeshDraw>> {
        crate::profile_scope!("DrawCache.Update", crate::profiler::Category::Rendering);
        self.calls += 1;
        self.stats = CacheStats::default();
        crate::mesh_builder::sync(scene);
        let (renderables, sprites, groups) = self.walk(scene, culling_mask, &selected);
        let lods = self.lod_ranges(scene, &groups);
        self.update_meshes(scene, materials, &renderables, &lods);
        if self.batching.on {
            self.update_batches(&renderables);
        }

        if self.skin_view == 0 {
            self.skin_view = crate::skinning::next_view_id();
        }
        // Sprites are few and cheap: rebuilt every call.
        let views = (scene.world.view::<&SpriteRenderer>(), &self.nodes);
        let (target, meshes) = match self.batching.on {
            true => (&mut self.batching.out, self.batching.meshes),
            false => (&mut self.out, self.mesh_draws),
        };
        // World Builder scatter (stored compactly, instanced by the renderer): rebuilt too.
        let mut scatter = Vec::new();
        let keep = |e| crate::render_scene::in_mask(scene, e, culling_mask);
        crate::world_gen::scatter_set::draws(scene, materials, keep, &mut scatter);
        // Skinned Mesh Renderers: posed every call (bones move without touching the renderer).
        let world = |e: Entity| {
            views
                .1
                .get(e.id() as usize)
                .filter(|n| n.entity == Some(e))
                .map_or_else(|| scene.world_matrix(e), |n| n.world)
        };
        let outline = |e: Entity| {
            let mut cur = Some(e);
            while let Some(x) = cur {
                if selected(x) {
                    return true;
                }
                cur = scene.parent(x);
            }
            false
        };
        let view = self.skin_view;
        crate::skinning::draws(scene, materials, view, keep, &world, outline, &mut scatter);
        // Tilemap Renderers: chunk meshes cached by content, animated tiles posed every call.
        crate::tilemap::chunks::draws(scene, &keep, &world, &outline, &mut scatter);
        if !sprites.is_empty() || !scatter.is_empty() || target.len() > meshes {
            let out = Arc::make_mut(target);
            out.truncate(meshes);
            for (e, outline) in sprites {
                if let Some(r) = views.0.get(e) {
                    let world = views.1[e.id() as usize].world;
                    out.extend(crate::sprites::draw_of(r, world, outline));
                }
            }
            out.extend(scatter);
        }
        target.clone()
    }

    /// Re-merge the static batches when a member changed, and rebuild the batched list when
    /// anything did.
    fn update_batches(&mut self, list: &[Renderable]) {
        let b = &mut self.batching;
        let members: Vec<Entity> = list
            .iter()
            .filter(|r| {
                self.renderers[r.entity.id() as usize]
                    .as_ref()
                    .is_some_and(|e| e.batched)
            })
            .map(|r| r.entity)
            .collect();
        let remerge = b.dirty || members != b.members;
        let range = |e: &RendererEntry| e.first as usize..(e.first + e.count) as usize;
        if remerge {
            let draws: Vec<MeshDraw> = members
                .iter()
                .filter_map(|e| self.renderers[e.id() as usize].as_ref())
                .flat_map(|e| self.out[range(e)].iter().cloned())
                .collect();
            b.merged = crate::render::static_batch::combine(&b.name, &draws);
            b.members = members;
        }
        if remerge || self.stats.rebuilt > 0 || self.stats.relaid || b.out.is_empty() {
            let mut out: Vec<MeshDraw> = Vec::new();
            for r in list {
                if let Some(e) = self.renderers[r.entity.id() as usize].as_ref()
                    && !e.batched
                {
                    out.extend_from_slice(&self.out[range(e)]);
                }
            }
            out.extend(b.merged.iter().cloned());
            b.meshes = out.len();
            b.out = Arc::new(out);
        }
        b.dirty = false;
    }

    /// Walk the hierarchy: refresh world matrices and list what draws, in Hierarchy order.
    #[allow(clippy::type_complexity)]
    fn walk(
        &mut self,
        scene: &Scene,
        mask: u32,
        selected: &impl Fn(Entity) -> bool,
    ) -> (Vec<Renderable>, Vec<(Entity, bool)>, Vec<Entity>) {
        let w = &scene.world;
        let (nodes, objects, transforms) = (
            w.view::<&Node>(),
            w.view::<&GameObject>(),
            w.view::<&Transform>(),
        );
        let (filters, renderers, sprites, lods) = (
            w.view::<&MeshFilter>(),
            w.view::<&MeshRenderer>(),
            w.view::<&SpriteRenderer>(),
            w.view::<&LodGroup>(),
        );
        let mut out = (Vec::new(), Vec::new(), Vec::new());
        // (entity, parent, parent world, parent world changed, parent active, parent outlined)
        let mut stack: Vec<(Entity, Option<Entity>, Mat4, bool, bool, bool)> = scene
            .roots()
            .iter()
            .rev()
            .map(|r| (*r, None, Mat4::IDENTITY, false, true, false))
            .collect();
        while let Some((e, parent, parent_world, parent_changed, parent_active, parent_outline)) =
            stack.pop()
        {
            let (Some(go), Some(t)) = (objects.get(e), transforms.get(e)) else {
                continue;
            };
            let id = e.id() as usize;
            if self.nodes.len() <= id {
                self.nodes.resize(id + 1, NodeEntry::default());
            }
            let n = &mut self.nodes[id];
            let changed =
                parent_changed || n.entity != Some(e) || n.parent != parent || n.local != *t;
            if changed {
                // Same operation order as `Scene::world_matrix`, so the result is identical.
                n.world = match parent {
                    Some(_) => parent_world * t.local_matrix(),
                    None => t.local_matrix(),
                };
                n.entity = Some(e);
                n.parent = parent;
                n.local = *t;
            }
            let world = n.world;
            let active = parent_active && go.active;
            let outline = parent_outline || selected(e);
            if active && mask & (1u32 << go.layer.min(31)) != 0 {
                let has_mesh = filters.get(e).is_some_and(|f| f.mesh.is_some());
                if has_mesh && renderers.get(e).is_some_and(|r| r.enabled) {
                    out.0.push(Renderable {
                        entity: e,
                        world_changed: changed,
                        outline,
                        is_static: go.is_static,
                    });
                }
            }
            if active {
                if sprites.get(e).is_some_and(|s| s.enabled) {
                    out.1.push((e, outline));
                }
                if lods.get(e).is_some_and(|g| g.enabled) {
                    out.2.push(e);
                }
            }
            if let Some(node) = nodes.get(e) {
                for c in node.children.iter().rev() {
                    stack.push((*c, Some(e), world, changed, active, outline));
                }
            }
        }
        out
    }

    /// LOD level of every renderer listed by an active LOD Group (first listing wins).
    fn lod_ranges(&self, scene: &Scene, groups: &[Entity]) -> HashMap<Entity, LodRange> {
        let mut out = HashMap::new();
        for &g in groups {
            let Ok(group) = scene.world.get::<&LodGroup>(g) else {
                continue;
            };
            // Bounds of every level's renderers, in world space.
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for e in group.lods.iter().flat_map(|l| &l.renderers) {
                let Some((a, b)) = self.world_bounds(scene, *e) else {
                    continue;
                };
                lo = lo.min(a);
                hi = hi.max(b);
            }
            if lo.x > hi.x {
                continue;
            }
            let (center, size) = ((lo + hi) * 0.5, (hi - lo).max_element());
            for (i, level) in group.lods.iter().enumerate() {
                let (min, max) = group.range(i);
                let range = LodRange {
                    center,
                    size,
                    min,
                    max,
                    fade: group.fade(),
                };
                for e in &level.renderers {
                    out.entry(*e).or_insert(range);
                }
            }
        }
        out
    }

    /// World box of an object's mesh, from the matrices of this walk.
    fn world_bounds(&self, scene: &Scene, e: Entity) -> Option<(Vec3, Vec3)> {
        let n = self.nodes.get(e.id() as usize)?;
        if n.entity != Some(e) {
            return None;
        }
        let mesh = scene.world.get::<&MeshFilter>(e).ok()?.mesh.clone()?;
        let (lo, hi) = crate::models::bounds(&mesh)?;
        let b = crate::render::culling::Aabb::new(lo, hi).transformed(&n.world);
        Some((b.min, b.max))
    }

    /// Bring the mesh section of the output up to date.
    fn update_meshes(
        &mut self,
        scene: &Scene,
        materials: &dyn MaterialSource,
        list: &[Renderable],
        lods: &HashMap<Entity, LodRange>,
    ) {
        let (filters, renderers) = (
            scene.world.view::<&MeshFilter>(),
            scene.world.view::<&MeshRenderer>(),
        );
        // Same objects in the same order with the same draw counts: patch in place.
        let mut in_place = list.len() == self.order.len()
            && list.iter().zip(&self.order).all(|(r, e)| r.entity == *e);
        let mut fresh: Vec<Option<Vec<MeshDraw>>> = Vec::with_capacity(list.len());
        for item in list {
            let e = item.entity;
            let (Some(f), Some(r)) = (filters.get(e), renderers.get(e)) else {
                fresh.push(Some(Vec::new()));
                continue;
            };
            let Some(mesh) = f.mesh.as_ref() else {
                fresh.push(Some(Vec::new()));
                continue;
            };
            let lod = lods.get(&e).copied();
            let id = e.id() as usize;
            if self.renderers.len() <= id {
                self.renderers.resize_with(id + 1, || None);
            }
            let calls = self.calls;
            let old = self.renderers[id]
                .take()
                .filter(|o| o.entity == e && o.call + 1 == calls);
            let batched = item.is_static && lod.is_none() && !item.outline;
            let same_inputs = old.as_ref().is_some_and(|o| {
                !item.world_changed
                    && o.mesh == *mesh
                    && o.renderer == *r
                    && o.outline == item.outline
                    && o.lod == lod
                    && o.is_static == item.is_static
            });
            // Material slots: keep the table indices while the renderer is unchanged.
            let slots: Vec<Option<(u32, u64)>> = (0..r.material_count())
                .map(|i| {
                    if r.instance(i).is_some() {
                        return None;
                    }
                    let known = old
                        .as_ref()
                        .filter(|_| same_inputs)
                        .and_then(|o| o.materials.get(i).copied().flatten());
                    let index = match known {
                        Some((index, _)) => index,
                        None => self.material(r.material_at(i)),
                    };
                    Some((index, self.check(index, scene, materials)))
                })
                .collect();
            let dirty = !same_inputs || old.as_ref().is_some_and(|o| o.materials != slots);
            let (first, count) = old.as_ref().map_or((0, 0), |o| (o.first, o.count));
            if dirty && (batched || old.as_ref().is_some_and(|o| o.batched)) {
                self.batching.dirty = true;
            }
            if dirty {
                let world = self.nodes[id].world;
                let probes = r.light_probes == LightProbeUsage::BlendProbes && !item.is_static;
                let draws = self.build(mesh, r, world, item.outline, lod, &slots, probes);
                in_place &= old.is_some() && draws.len() as u32 == count;
                fresh.push(Some(draws));
                self.stats.rebuilt += 1;
            } else {
                fresh.push(None);
                self.stats.kept += 1;
            }
            self.renderers[id] = Some(RendererEntry {
                entity: e,
                mesh: mesh.clone(),
                renderer: r.clone(),
                outline: item.outline,
                lod,
                materials: slots,
                first,
                count,
                call: calls,
                batched,
                is_static: item.is_static,
            });
        }
        if in_place {
            if self.stats.rebuilt > 0 {
                let out = Arc::make_mut(&mut self.out);
                for (item, draws) in list.iter().zip(fresh) {
                    let Some(draws) = draws else { continue };
                    let entry = self.renderers[item.entity.id() as usize].as_ref();
                    let first = entry.map_or(0, |e| e.first as usize);
                    for (k, d) in draws.into_iter().enumerate() {
                        out[first + k] = d;
                    }
                }
            }
            return;
        }
        // Relayout: new draws where rebuilt, copies of the old ones elsewhere.
        self.stats.relaid = true;
        let mut out = Vec::with_capacity(self.out.len());
        for (item, draws) in list.iter().zip(fresh) {
            let first = out.len() as u32;
            let Some(entry) = self.renderers[item.entity.id() as usize].as_mut() else {
                continue;
            };
            match draws {
                Some(d) => out.extend(d),
                None => {
                    let range = entry.first as usize..(entry.first + entry.count) as usize;
                    out.extend_from_slice(&self.out[range]);
                }
            }
            entry.first = first;
            entry.count = out.len() as u32 - first;
        }
        self.order = list.iter().map(|r| r.entity).collect();
        self.mesh_draws = out.len();
        self.out = Arc::new(out);
    }

    /// One draw per Materials element, with the object's overrides applied; `light_probes` =
    /// lit by the baked light probes.
    #[allow(clippy::too_many_arguments)]
    fn build(
        &self,
        mesh: &MeshRef,
        r: &MeshRenderer,
        world: Mat4,
        outline: bool,
        lod: Option<LodRange>,
        slots: &[Option<(u32, u64)>],
        light_probes: bool,
    ) -> Vec<MeshDraw> {
        // A baked Editable Mesh draws each element's faces only (its submeshes).
        let split = crate::mesh_builder::is_baked_key(mesh);
        (0..r.material_count())
            .map(|i| {
                let mut material = match (r.instance(i), slots[i]) {
                    (Some(m), _) => m.params(),
                    (None, Some((index, _))) => self.materials[index as usize].params.clone(),
                    (None, None) => MaterialParams::default(),
                };
                if let Some(o) = r.overrides_at(i) {
                    o.apply(&mut material);
                }
                // Unlit materials ignore probes (the probe gizmos use that pairing).
                material.light_probes = light_probes && !material.unlit;
                MeshDraw {
                    mesh: match split {
                        true => crate::mesh_builder::submesh(mesh, i),
                        false => mesh.clone(),
                    },
                    model: world,
                    material,
                    cast_shadows: r.cast_shadows,
                    receive_shadows: r.receive_shadows,
                    // The outline mask needs each mesh once (each part of a split mesh).
                    outline: outline && (i == 0 || split),
                    sorting: Default::default(),
                    lod,
                }
            })
            .collect()
    }

    /// Table index of a material path.
    fn material(&mut self, path: Option<&str>) -> u32 {
        let key = path.map(str::to_owned);
        if let Some(i) = self.material_index.get(&key) {
            return *i;
        }
        let i = self.materials.len() as u32;
        self.materials.push(MaterialEntry {
            path: key.clone(),
            params: MaterialParams::default(),
            version: 0,
            checked: 0,
        });
        self.material_index.insert(key, i);
        i
    }

    /// Re-resolve a material once per call; its version changes when its values do.
    fn check(&mut self, index: u32, scene: &Scene, materials: &dyn MaterialSource) -> u64 {
        let m = &mut self.materials[index as usize];
        if m.checked != self.calls {
            let path = m.path.as_deref();
            let params = match path.and_then(|p| scene.shared_materials.get(p)) {
                Some(shared) => shared.params(),
                None => materials.params(path),
            };
            if m.checked == 0 || params != m.params {
                m.params = params;
                m.version += 1;
            }
            m.checked = self.calls;
        }
        m.version
    }
}

#[cfg(test)]
mod tests;
