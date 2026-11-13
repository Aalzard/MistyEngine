//! Collider shapes from Box/Sphere/Capsule/Mesh Collider components, attached to the closest
//! body up the hierarchy (static otherwise), with collision groups from the object's layer.

use glam::{Quat, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use super::convert::{pose, rv};
use super::material::PhysicsMaterial;
use super::{MAX_EXTENT, Physics, layers, world_mirror, world_pose};
use crate::scene::{
    BoxCollider, CapsuleCollider, MeshCollider, MeshFilter, PhysicsSurface, Scene, SphereCollider,
};

impl Physics {
    /// Colliders of `objects`: attached to the closest body up the hierarchy, else static.
    pub(super) fn add_colliders(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let owner = self.owner(scene, e);
            let groups = layers::groups(&self.settings, scene.game_object(e).layer);
            for (collider, material) in self.colliders_for(scene, e) {
                // Mass comes from the Rigidbody, not collider density.
                let mut collider = collider
                    .density(0.0)
                    .collision_groups(groups)
                    .active_events(rp::ActiveEvents::COLLISION_EVENTS);
                if collider.is_sensor {
                    // Triggers also fire for kinematic bodies and controllers.
                    collider = collider.active_collision_types(rp::ActiveCollisionTypes::all());
                }
                if material.is_some() {
                    collider = collider.active_hooks(rp::ActiveHooks::MODIFY_SOLVER_CONTACTS);
                }
                let h = self.insert_collider(scene, e, owner, collider);
                if let Some(m) = material {
                    self.materials.insert(h, m);
                }
            }
            // Terrain Collider: a static heightfield placed in world space (ignores rotation).
            if let Some(c) = crate::terrain::collider(scene, e) {
                let c = c
                    .collision_groups(groups)
                    .active_events(rp::ActiveEvents::COLLISION_EVENTS);
                let h = self.world.insert_collider(c.build(), None);
                self.colliders.insert(h, e);
            }
            // Its trees' trunks (static capsules).
            for c in crate::terrain::tree_colliders(scene, e) {
                let c = c.collision_groups(groups);
                let h = self.world.insert_collider(c.build(), None);
                self.colliders.insert(h, e);
            }
        }
    }

    /// Insert a collider of object `e`, relative to `owner`'s body or in world space.
    pub(super) fn insert_collider(
        &mut self,
        scene: &Scene,
        e: Entity,
        owner: Option<Entity>,
        collider: rp::ColliderBuilder,
    ) -> rp::ColliderHandle {
        let (cp, cr, _) = world_pose(scene, e);
        let h = match owner.and_then(|o| self.bodies.get(&o).map(|b| (o, b.handle))) {
            Some((o, parent)) => {
                let (bp, br, _) = world_pose(scene, o);
                let rel = pose(br.inverse() * (cp - bp), br.inverse() * cr);
                let c = collider.position(rel).build();
                self.world.insert_collider(c, Some(parent))
            }
            None => {
                let c = collider.position(pose(cp, cr)).build();
                self.world.insert_collider(c, None)
            }
        };
        self.colliders.insert(h, e);
        h
    }

    /// The object whose body `e`'s colliders attach to: itself or the closest parent with one.
    pub(super) fn owner(&self, scene: &Scene, e: Entity) -> Option<Entity> {
        let mut cur = Some(e);
        while let Some(c) = cur {
            if self.bodies.contains_key(&c) {
                return Some(c);
            }
            cur = scene.parent(c);
        }
        None
    }

    /// Move `e`'s colliders to another layer (a script changed `gameObject.layer`).
    pub fn set_layer(&mut self, e: Entity, layer: u8) {
        self.set_controller_layer(e, layer);
        let groups = layers::groups(&self.settings, layer);
        for (h, owner) in &self.colliders {
            if *owner == e
                && let Some(c) = self.world.colliders.get_mut(*h)
            {
                c.set_collision_groups(groups);
            }
        }
    }

    /// The Physics Material asset at `path` (loaded once per build; a warning when missing).
    fn physics_material(
        &mut self,
        scene: &Scene,
        e: Entity,
        path: &str,
    ) -> Option<PhysicsMaterial> {
        if let Some(m) = self.material_assets.get(path) {
            return m.clone();
        }
        let loaded = match PhysicsMaterial::load_asset(path) {
            Ok(m) => Some(m),
            Err(err) => {
                self.warnings.push(format!(
                    "'{}': Physics Material {path} could not be loaded ({err}).",
                    scene.name(e)
                ));
                None
            }
        };
        self.material_assets.insert(path.to_owned(), loaded.clone());
        loaded
    }

    /// Collider builders for `e`'s collider components (in the object's local frame, scaled),
    /// with the Physics Material each one uses.
    fn colliders_for(
        &mut self,
        scene: &Scene,
        e: Entity,
    ) -> Vec<(rp::ColliderBuilder, Option<PhysicsMaterial>)> {
        let w = &scene.world;
        let (_, _, s) = world_pose(scene, e);
        // Mirrored objects (a negative scale) mirror their colliders' centres and meshes too:
        // they sat on the other side, an asymmetric mesh unmirrored.
        let mirror = world_mirror(scene, e);
        let name = scene.name(e);
        let paths = [
            w.get::<&BoxCollider>(e)
                .ok()
                .and_then(|c| c.material.clone()),
            w.get::<&SphereCollider>(e)
                .ok()
                .and_then(|c| c.material.clone()),
            w.get::<&CapsuleCollider>(e)
                .ok()
                .and_then(|c| c.material.clone()),
            w.get::<&MeshCollider>(e)
                .ok()
                .and_then(|c| c.material.clone()),
        ];
        let mats: Vec<Option<PhysicsMaterial>> = paths
            .iter()
            .map(|p| {
                p.as_deref()
                    .and_then(|p| self.physics_material(scene, e, p))
            })
            .collect();
        let w = &scene.world;
        let finish = |b: rp::ColliderBuilder,
                      center: Vec3,
                      surface: PhysicsSurface,
                      trigger: bool,
                      material: &Option<PhysicsMaterial>| {
            let b = match material {
                Some(m) => b
                    .friction(m.dynamic_friction.max(0.0))
                    .friction_combine_rule(m.friction_combine.rapier())
                    .restitution(m.bounciness.clamp(0.0, 1.0))
                    .restitution_combine_rule(m.bounce_combine.rapier()),
                None => b.friction(surface.friction).restitution(surface.bounciness),
            };
            let center =
                (center * s * mirror).clamp(Vec3::splat(-MAX_EXTENT), Vec3::splat(MAX_EXTENT));
            (b.sensor(trigger), center, material.clone())
        };
        let mut shaped: Vec<(rp::ColliderBuilder, Vec3, Option<PhysicsMaterial>)> = Vec::new();
        if let Ok(c) = w.get::<&BoxCollider>(e)
            && c.enabled
        {
            let h = (c.size * s * 0.5).clamp(Vec3::splat(1e-3), Vec3::splat(MAX_EXTENT));
            shaped.push(finish(
                rp::ColliderBuilder::cuboid(h.x, h.y, h.z),
                c.center,
                c.surface,
                c.is_trigger,
                &mats[0],
            ));
        }
        if let Ok(c) = w.get::<&SphereCollider>(e)
            && c.enabled
        {
            let r = (c.radius * s.max_element()).clamp(1e-3, MAX_EXTENT);
            shaped.push(finish(
                rp::ColliderBuilder::ball(r),
                c.center,
                c.surface,
                c.is_trigger,
                &mats[1],
            ));
        }
        if let Ok(c) = w.get::<&CapsuleCollider>(e)
            && c.enabled
        {
            let axis = c.direction.min(2) as usize;
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let r = (c.radius * s[a].max(s[b])).clamp(1e-3, MAX_EXTENT);
            let half = (c.height * s[axis] * 0.5 - r).clamp(0.0, MAX_EXTENT);
            let builder = match axis {
                0 => rp::ColliderBuilder::capsule_x(half, r),
                1 => rp::ColliderBuilder::capsule_y(half, r),
                _ => rp::ColliderBuilder::capsule_z(half, r),
            };
            shaped.push(finish(builder, c.center, c.surface, c.is_trigger, &mats[2]));
        }
        if let Ok(c) = w.get::<&MeshCollider>(e)
            && c.enabled
        {
            let mesh_ref = w.get::<&MeshFilter>(e).ok().and_then(|f| f.mesh.clone());
            match mesh_ref.as_ref().and_then(crate::models::mesh) {
                Some(data) => {
                    let verts: Vec<rp::Vector> = data
                        .vertices
                        .iter()
                        .map(|v| rv(Vec3::from(v.position) * s * mirror))
                        .collect();
                    let dynamic = self.bodies.get(&e).is_some_and(|b| !b.kinematic);
                    let builder = if c.convex || dynamic {
                        if dynamic && !c.convex {
                            self.warnings.push(format!(
                                "'{name}': non-convex Mesh Collider on a dynamic Rigidbody is treated as convex."
                            ));
                        }
                        rp::ColliderBuilder::convex_hull(&verts)
                    } else if let Some(shape) = mesh_ref
                        .as_ref()
                        .filter(|_| mirror == Vec3::ONE)
                        .and_then(|r| super::shape_cache::take(r, s))
                    {
                        // Built ahead on a background thread (a streamed cell).
                        Some(rp::ColliderBuilder::new(shape))
                    } else {
                        // Mirroring turns the triangles inside out: keep them facing out.
                        let flip = mirror.x < 0.0;
                        let tris = data
                            .indices
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .map(|t| {
                                if flip {
                                    [t[0], t[2], t[1]]
                                } else {
                                    [t[0], t[1], t[2]]
                                }
                            })
                            .collect();
                        rp::ColliderBuilder::trimesh(verts, tris).ok()
                    };
                    match builder {
                        Some(b) => {
                            shaped.push(finish(b, Vec3::ZERO, c.surface, c.is_trigger, &mats[3]))
                        }
                        None => self
                            .warnings
                            .push(format!("'{name}': could not build Mesh Collider.")),
                    }
                }
                None => self.warnings.push(match &mesh_ref {
                    Some(r) => format!(
                        "'{name}': Mesh Collider mesh {} could not be loaded.",
                        crate::models::display_name(r)
                    ),
                    None => format!("'{name}': Mesh Collider needs a Mesh Filter with a mesh."),
                }),
            }
        }
        shaped
            .into_iter()
            .map(|(b, center, m)| (offset(b, center), m))
            .collect()
    }
}

/// Wrap a shape in a compound so its centre offset survives the outer position.
pub(super) fn offset(b: rp::ColliderBuilder, center: Vec3) -> rp::ColliderBuilder {
    // A composite shape (a Mesh Collider's triangles) can't go inside a compound (parry
    // panics); a non-finite centre (a damaged scene) has nowhere to go.
    if center == Vec3::ZERO || !center.is_finite() || b.shape.as_composite_shape().is_some() {
        return b;
    }
    let shape = b.shape.clone();
    rp::ColliderBuilder::compound(vec![(pose(center, Quat::IDENTITY), shape)])
        .friction(b.friction)
        .friction_combine_rule(b.friction_combine_rule)
        .restitution(b.restitution)
        .restitution_combine_rule(b.restitution_combine_rule)
        .sensor(b.is_sensor)
}
