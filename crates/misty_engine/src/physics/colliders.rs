//! Collider shapes from Box/Sphere/Capsule/Mesh Collider components, attached to the closest
//! body up the hierarchy (static otherwise), with collision groups from the object's layer.

use glam::{Quat, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use super::convert::{pose, rv};
use super::{Physics, layers, world_pose};
use crate::scene::{
    BoxCollider, CapsuleCollider, MeshCollider, MeshFilter, PhysicsSurface, Scene, SphereCollider,
};

impl Physics {
    /// Colliders of `objects`: attached to the closest body up the hierarchy, else static.
    pub(super) fn add_colliders(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let owner = self.owner(scene, e);
            let groups = layers::groups(&self.settings, scene.game_object(e).layer);
            for collider in self.colliders_for(scene, e) {
                // Mass comes from the Rigidbody, not collider density.
                let mut collider = collider
                    .density(0.0)
                    .collision_groups(groups)
                    .active_events(rp::ActiveEvents::COLLISION_EVENTS);
                if collider.is_sensor {
                    // Triggers also fire for kinematic bodies and controllers.
                    collider = collider.active_collision_types(rp::ActiveCollisionTypes::all());
                }
                self.insert_collider(scene, e, owner, collider);
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

    /// Collider builders for `e`'s collider components (in the object's local frame, scaled).
    fn colliders_for(&mut self, scene: &Scene, e: Entity) -> Vec<rp::ColliderBuilder> {
        let w = &scene.world;
        let (_, _, s) = world_pose(scene, e);
        let name = scene.name(e);
        let finish =
            |b: rp::ColliderBuilder, center: Vec3, surface: PhysicsSurface, trigger: bool| {
                let b = b
                    .friction(surface.friction)
                    .restitution(surface.bounciness)
                    .sensor(trigger);
                (b, center * s)
            };
        let mut shaped: Vec<(rp::ColliderBuilder, Vec3)> = Vec::new();
        if let Ok(c) = w.get::<&BoxCollider>(e)
            && c.enabled
        {
            let h = (c.size * s * 0.5).max(Vec3::splat(1e-3));
            shaped.push(finish(
                rp::ColliderBuilder::cuboid(h.x, h.y, h.z),
                c.center,
                c.surface,
                c.is_trigger,
            ));
        }
        if let Ok(c) = w.get::<&SphereCollider>(e)
            && c.enabled
        {
            let r = (c.radius * s.max_element()).max(1e-3);
            shaped.push(finish(
                rp::ColliderBuilder::ball(r),
                c.center,
                c.surface,
                c.is_trigger,
            ));
        }
        if let Ok(c) = w.get::<&CapsuleCollider>(e)
            && c.enabled
        {
            let axis = c.direction.min(2) as usize;
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let r = (c.radius * s[a].max(s[b])).max(1e-3);
            let half = (c.height * s[axis] * 0.5 - r).max(0.0);
            let builder = match axis {
                0 => rp::ColliderBuilder::capsule_x(half, r),
                1 => rp::ColliderBuilder::capsule_y(half, r),
                _ => rp::ColliderBuilder::capsule_z(half, r),
            };
            shaped.push(finish(builder, c.center, c.surface, c.is_trigger));
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
                        .map(|v| rv(Vec3::from(v.position) * s))
                        .collect();
                    let dynamic = self.bodies.get(&e).is_some_and(|b| !b.kinematic);
                    let builder = if c.convex || dynamic {
                        if dynamic && !c.convex {
                            self.warnings.push(format!(
                                "'{name}': non-convex Mesh Collider on a dynamic Rigidbody is treated as convex."
                            ));
                        }
                        rp::ColliderBuilder::convex_hull(&verts)
                    } else {
                        let tris = data.indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
                        rp::ColliderBuilder::trimesh(verts, tris).ok()
                    };
                    match builder {
                        Some(b) => shaped.push(finish(b, Vec3::ZERO, c.surface, c.is_trigger)),
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
            .map(|(b, center)| offset(b, center))
            .collect()
    }
}

/// Wrap a shape in a compound so its centre offset survives the outer position.
pub(super) fn offset(b: rp::ColliderBuilder, center: Vec3) -> rp::ColliderBuilder {
    if center == Vec3::ZERO {
        return b;
    }
    let shape = b.shape.clone();
    rp::ColliderBuilder::compound(vec![(pose(center, Quat::IDENTITY), shape)])
        .friction(b.friction)
        .restitution(b.restitution)
        .sensor(b.is_sensor)
}
