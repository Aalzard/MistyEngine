//! Physics while playing (rapier). Built from the scene when Play starts, stepped at a fixed rate.
//!
//! Objects with a Rigidbody become dynamic (or kinematic) bodies; colliders attach to the nearest
//! Rigidbody on themselves or a parent, otherwise they are static. Collider sizes include the
//! object's world scale.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use crate::render::mesh;
use crate::scene::{
    BoxCollider, CapsuleCollider, MeshCollider, MeshFilter, PhysicsSurface, Rigidbody, Scene,
    SphereCollider,
};

/// Default fixed timestep (seconds).
pub const FIXED_DT: f32 = 0.02;
/// Upper bound of fixed steps per frame, so a slow frame can't spiral.
const MAX_STEPS: u32 = 8;

// rapier uses its own glam version: convert through arrays.
fn rv(v: Vec3) -> rp::Vector {
    rp::Vector::new(v.x, v.y, v.z)
}
fn gv(v: rp::Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}
fn rq(q: Quat) -> rp::Rotation {
    rp::Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}
fn gq(q: rp::Rotation) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}
fn pose(t: Vec3, r: Quat) -> rp::Pose {
    rp::Pose::from_parts(rv(t), rq(r))
}

struct Body {
    handle: rp::RigidBodyHandle,
    kinematic: bool,
}

/// A collision or trigger contact between two objects' colliders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub a: Entity,
    pub b: Entity,
    /// true = began touching, false = stopped.
    pub started: bool,
    /// One of the colliders is a trigger.
    pub trigger: bool,
}

/// Collects rapier collision events during a step.
#[derive(Default)]
struct EventLog(std::sync::Mutex<Vec<rp::CollisionEvent>>);

impl rp::EventHandler for EventLog {
    fn handle_collision_event(
        &self,
        _bodies: &rp::RigidBodySet,
        _colliders: &rp::ColliderSet,
        event: rp::CollisionEvent,
        _contact_pair: Option<&rp::ContactPair>,
    ) {
        if let Ok(mut v) = self.0.lock() {
            v.push(event);
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: rp::Real,
        _bodies: &rp::RigidBodySet,
        _colliders: &rp::ColliderSet,
        _contact_pair: &rp::ContactPair,
        _total_force_magnitude: rp::Real,
    ) {
    }

    fn handle_soft_body_tear_event(
        &self,
        _bodies: &rp::SoftBodySet,
        _event: &rp::SoftBodyTearEvent,
    ) {
    }
}

pub struct Physics {
    world: rp::PhysicsWorld,
    bodies: HashMap<Entity, Body>,
    /// Which object each collider belongs to.
    colliders: HashMap<rp::ColliderHandle, Entity>,
    accumulator: f32,
    pub fixed_dt: f32,
    /// Problems found while building (shown in the Console).
    pub warnings: Vec<String>,
}

/// Rigid (unscaled) world pose and world scale of an object.
fn world_pose(scene: &Scene, e: Entity) -> (Vec3, Quat, Vec3) {
    let (scale, rot, pos) = scene.world_matrix(e).to_scale_rotation_translation();
    (pos, rot, scale.abs())
}

impl Physics {
    pub fn build(scene: &Scene, gravity: Vec3) -> Self {
        let mut world = rp::PhysicsWorld::new();
        world.gravity = rv(gravity);
        world.integration_parameters.dt = FIXED_DT;
        let mut physics = Self {
            world,
            bodies: HashMap::new(),
            colliders: HashMap::new(),
            accumulator: 0.0,
            fixed_dt: FIXED_DT,
            warnings: Vec::new(),
        };
        let objects: Vec<Entity> = scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| scene.active_in_hierarchy(*e))
            .collect();

        // Rigidbodies first.
        for &e in &objects {
            let Ok(rb) = scene.world.get::<&Rigidbody>(e).map(|r| (*r).clone()) else {
                continue;
            };
            let (pos, rot, _) = world_pose(scene, e);
            let builder = if rb.is_kinematic {
                rp::RigidBodyBuilder::kinematic_position_based()
            } else {
                rp::RigidBodyBuilder::dynamic()
            };
            let mut locked = rp::LockedAxes::empty();
            let flags = [
                rp::LockedAxes::TRANSLATION_LOCKED_X,
                rp::LockedAxes::TRANSLATION_LOCKED_Y,
                rp::LockedAxes::TRANSLATION_LOCKED_Z,
                rp::LockedAxes::ROTATION_LOCKED_X,
                rp::LockedAxes::ROTATION_LOCKED_Y,
                rp::LockedAxes::ROTATION_LOCKED_Z,
            ];
            for (i, f) in flags.into_iter().enumerate() {
                let frozen = if i < 3 {
                    rb.freeze_position[i]
                } else {
                    rb.freeze_rotation[i - 3]
                };
                if frozen {
                    locked |= f;
                }
            }
            let body = builder
                .pose(pose(pos, rot))
                .linear_damping(rb.drag)
                .angular_damping(rb.angular_drag)
                .gravity_scale(if rb.use_gravity { 1.0 } else { 0.0 })
                .locked_axes(locked)
                .ccd_enabled(rb.continuous)
                .additional_mass(rb.mass.max(1e-4))
                .build();
            let handle = physics.world.insert_body(body);
            physics.bodies.insert(
                e,
                Body {
                    handle,
                    kinematic: rb.is_kinematic,
                },
            );
        }

        // Colliders: attach to the closest Rigidbody up the hierarchy, else static.
        for &e in &objects {
            let owner = {
                let mut cur = Some(e);
                let mut found = None;
                while let Some(c) = cur {
                    if physics.bodies.contains_key(&c) {
                        found = Some(c);
                        break;
                    }
                    cur = scene.parent(c);
                }
                found
            };
            for collider in physics.colliders_for(scene, e) {
                // Mass comes from the Rigidbody, not collider density.
                let collider = collider
                    .density(0.0)
                    .active_events(rp::ActiveEvents::COLLISION_EVENTS);
                match owner {
                    Some(o) => {
                        // Pose relative to the owning body.
                        let (bp, br, _) = world_pose(scene, o);
                        let (cp, cr, _) = world_pose(scene, e);
                        let rel_rot = br.inverse() * cr;
                        let rel_pos = br.inverse() * (cp - bp);
                        let c = collider.position(pose(rel_pos, rel_rot)).build();
                        let parent = physics.bodies[&o].handle;
                        let h = physics.world.insert_collider(c, Some(parent));
                        physics.colliders.insert(h, e);
                    }
                    None => {
                        let (cp, cr, _) = world_pose(scene, e);
                        let c = collider.position(pose(cp, cr)).build();
                        let h = physics.world.insert_collider(c, None);
                        physics.colliders.insert(h, e);
                    }
                }
            }
        }
        physics
    }

    /// Collider builders for `e`'s collider components (in the object's local frame, scaled).
    fn colliders_for(&mut self, scene: &Scene, e: Entity) -> Vec<rp::ColliderBuilder> {
        let w = &scene.world;
        let (_, _, s) = world_pose(scene, e);
        let name = scene.name(e);
        let mut out = Vec::new();
        let finish =
            |b: rp::ColliderBuilder, center: Vec3, surface: PhysicsSurface, trigger: bool| {
                // Shapes are offset by their centre inside the object's frame.
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
            match w.get::<&MeshFilter>(e).ok().and_then(|f| f.mesh) {
                Some(m) => {
                    let data = mesh::primitive(m);
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
                None => self.warnings.push(format!(
                    "'{name}': Mesh Collider needs a Mesh Filter with a mesh."
                )),
            }
        }
        // Fold the centre offset into each shape by pre-translating.
        for (b, center) in shaped {
            out.push(if center == Vec3::ZERO {
                b
            } else {
                // Wrap in a compound so the centre offset survives the outer position.
                let shape = b.shape.clone();
                rp::ColliderBuilder::compound(vec![(pose(center, Quat::IDENTITY), shape)])
                    .friction(b.friction)
                    .restitution(b.restitution)
                    .sensor(b.is_sensor)
            });
        }
        out
    }

    /// Advance by `dt` seconds of game time in fixed steps, then write dynamic poses back.
    pub fn update(&mut self, scene: &mut Scene, dt: f32) {
        for _ in 0..self.steps_due(dt) {
            self.step(scene);
        }
        self.sync_out(scene);
    }

    /// Add `dt` to the accumulator and return how many fixed steps to run now.
    pub fn steps_due(&mut self, dt: f32) -> u32 {
        self.accumulator += dt;
        let steps = (self.accumulator / self.fixed_dt).floor() as u32;
        self.accumulator -= steps as f32 * self.fixed_dt;
        if steps > MAX_STEPS {
            self.accumulator = 0.0;
        }
        steps.min(MAX_STEPS)
    }

    /// One fixed step; returns the contacts that began or ended.
    pub fn step(&mut self, scene: &Scene) -> Vec<Contact> {
        self.push_kinematic(scene);
        let events = EventLog::default();
        self.world.step_with_events(&(), &events);
        let raw = events.0.into_inner().unwrap_or_default();
        raw.into_iter()
            .filter_map(|ev| {
                let (h1, h2, started) = match ev {
                    rp::CollisionEvent::Started(a, b, _) => (a, b, true),
                    rp::CollisionEvent::Stopped(a, b, _) => (a, b, false),
                };
                Some(Contact {
                    a: *self.colliders.get(&h1)?,
                    b: *self.colliders.get(&h2)?,
                    started,
                    trigger: ev.sensor(),
                })
            })
            .collect()
    }

    /// Dynamic bodies drive their Transform.
    pub fn sync_out(&self, scene: &mut Scene) {
        self.pull_dynamic(scene);
    }

    fn body_mut(&mut self, e: Entity) -> Option<&mut rp::RigidBody> {
        let h = self.bodies.get(&e)?.handle;
        self.world.bodies.get_mut(h)
    }

    fn body(&self, e: Entity) -> Option<&rp::RigidBody> {
        self.world.bodies.get(self.bodies.get(&e)?.handle)
    }

    pub fn has_body(&self, e: Entity) -> bool {
        self.bodies.contains_key(&e)
    }

    pub fn velocity(&self, e: Entity) -> Option<Vec3> {
        self.body(e).map(|b| gv(b.linvel()))
    }

    pub fn set_velocity(&mut self, e: Entity, v: Vec3) {
        if let Some(b) = self.body_mut(e) {
            b.set_linvel(rv(v), true);
        }
    }

    pub fn angular_velocity(&self, e: Entity) -> Option<Vec3> {
        self.body(e).map(|b| gv(b.angvel()))
    }

    pub fn set_angular_velocity(&mut self, e: Entity, v: Vec3) {
        if let Some(b) = self.body_mut(e) {
            b.set_angvel(rv(v), true);
        }
    }

    pub fn mass(&self, e: Entity) -> Option<f32> {
        self.body(e).map(|b| b.mass())
    }

    pub fn apply_impulse(&mut self, e: Entity, impulse: Vec3) {
        if let Some(b) = self.body_mut(e) {
            b.apply_impulse(rv(impulse), true);
        }
    }

    pub fn apply_torque_impulse(&mut self, e: Entity, impulse: Vec3) {
        if let Some(b) = self.body_mut(e) {
            b.apply_torque_impulse(rv(impulse), true);
        }
    }

    /// Move a body to a pose immediately (script set its Transform).
    pub fn teleport(&mut self, e: Entity, position: Vec3, rotation: Quat) {
        let kinematic = self.bodies.get(&e).is_some_and(|b| b.kinematic);
        if let Some(b) = self.body_mut(e) {
            if kinematic {
                b.set_next_kinematic_position(pose(position, rotation));
            } else {
                b.set_position(pose(position, rotation), true);
            }
        }
    }

    /// Remove an object's body and colliders (Destroy).
    pub fn remove(&mut self, e: Entity) {
        if let Some(b) = self.bodies.remove(&e) {
            self.world.remove_body_with_colliders(b.handle, true);
        }
        let handles: Vec<_> = self
            .colliders
            .iter()
            .filter(|(_, o)| **o == e)
            .map(|(h, _)| *h)
            .collect();
        for h in handles {
            self.world.remove_collider(h);
            self.colliders.remove(&h);
        }
        self.colliders
            .retain(|h, _| self.world.colliders.get(*h).is_some());
    }

    /// Kinematic bodies follow their Transform.
    fn push_kinematic(&mut self, scene: &Scene) {
        for (e, b) in &self.bodies {
            if b.kinematic && scene.contains(*e) {
                let (p, r, _) = world_pose(scene, *e);
                if let Some(body) = self.world.bodies.get_mut(b.handle) {
                    body.set_next_kinematic_position(pose(p, r));
                }
            }
        }
    }

    /// Dynamic bodies drive their Transform.
    fn pull_dynamic(&self, scene: &mut Scene) {
        for (e, b) in &self.bodies {
            if b.kinematic || !scene.contains(*e) {
                continue;
            }
            if let Some(body) = self.world.bodies.get(b.handle) {
                let p = body.position();
                scene.set_world_position(*e, gv(p.translation));
                scene.set_world_rotation(*e, gq(p.rotation));
            }
        }
    }

    pub fn body_count(&self) -> usize {
        self.world.bodies.len()
    }

    pub fn collider_count(&self) -> usize {
        self.world.colliders.len()
    }
}

/// World matrix helper used by gizmos: rigid pose of a collider centre.
pub fn collider_matrix(scene: &Scene, e: Entity, center: Vec3) -> Mat4 {
    let (p, r, s) = world_pose(scene, e);
    Mat4::from_rotation_translation(r, p + r * (center * s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{PrimitiveMesh, Transform};

    fn cube(scene: &mut Scene, name: &str, pos: Vec3, rigidbody: bool) -> Entity {
        let e = scene.spawn(name, None);
        scene.set_transform(e, Transform::from_position(pos));
        scene.add(
            e,
            MeshFilter {
                mesh: Some(PrimitiveMesh::Cube),
            },
        );
        scene.add(e, BoxCollider::default());
        if rigidbody {
            scene.add(e, Rigidbody::default());
        }
        e
    }

    #[test]
    fn falling_cube_lands_on_static_ground() {
        let mut scene = Scene::new("t");
        let ground = scene.spawn("Ground", None);
        scene.add(
            ground,
            MeshFilter {
                mesh: Some(PrimitiveMesh::Plane),
            },
        );
        scene.add(ground, MeshCollider::default());
        let c = cube(&mut scene, "Cube", Vec3::new(0.0, 3.0, 0.0), true);
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        assert_eq!(
            physics.body_count(),
            1,
            "only the cube has a body; the ground is a static collider"
        );
        assert_eq!(physics.collider_count(), 2);
        for _ in 0..200 {
            physics.update(&mut scene, 1.0 / 60.0);
        }
        let y = scene.world_position(c).y;
        assert!((y - 0.5).abs() < 0.05, "cube rests on the plane, y = {y}");
        assert_eq!(
            scene.world_position(ground),
            Vec3::ZERO,
            "static colliders don't move"
        );
    }

    #[test]
    fn gravity_off_and_frozen_axes() {
        let mut scene = Scene::new("t");
        let floating = cube(&mut scene, "Float", Vec3::new(0.0, 5.0, 0.0), true);
        scene
            .world
            .get::<&mut Rigidbody>(floating)
            .unwrap()
            .use_gravity = false;
        let frozen = cube(&mut scene, "Frozen", Vec3::new(3.0, 5.0, 0.0), true);
        scene
            .world
            .get::<&mut Rigidbody>(frozen)
            .unwrap()
            .freeze_position = [false, true, false];
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        for _ in 0..60 {
            physics.update(&mut scene, 1.0 / 60.0);
        }
        assert!((scene.world_position(floating).y - 5.0).abs() < 1e-3);
        assert!((scene.world_position(frozen).y - 5.0).abs() < 1e-3);
    }

    #[test]
    fn scaled_box_collider_matches_object_size() {
        let mut scene = Scene::new("t");
        let ground = cube(&mut scene, "Ground", Vec3::ZERO, false);
        let mut t = scene.transform(ground);
        t.scale = Vec3::new(10.0, 1.0, 10.0);
        scene.set_transform(ground, t);
        let c = cube(&mut scene, "Cube", Vec3::new(4.0, 3.0, 4.0), true);
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        for _ in 0..200 {
            physics.update(&mut scene, 1.0 / 60.0);
        }
        // Ground top is at y = 0.5, cube half-height 0.5.
        assert!(
            (scene.world_position(c).y - 1.0).abs() < 0.05,
            "{}",
            scene.world_position(c).y
        );
    }

    #[test]
    fn landing_reports_a_collision_and_triggers_are_flagged() {
        let mut scene = Scene::new("t");
        let ground = cube(&mut scene, "Ground", Vec3::ZERO, false);
        let c = cube(&mut scene, "Cube", Vec3::new(0.0, 2.0, 0.0), true);
        let zone = cube(&mut scene, "Zone", Vec3::new(0.0, 1.2, 0.0), false);
        scene
            .world
            .get::<&mut BoxCollider>(zone)
            .unwrap()
            .is_trigger = true;
        let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        let mut contacts = Vec::new();
        for _ in 0..120 {
            contacts.extend(physics.step(&scene));
            physics.sync_out(&mut scene);
        }
        let hit = |x: Entity, y: Entity| {
            contacts
                .iter()
                .find(|k| (k.a == x && k.b == y) || (k.a == y && k.b == x))
        };
        let landing = hit(c, ground).expect("cube hits the ground");
        assert!(landing.started && !landing.trigger);
        assert!(hit(c, zone).expect("cube passes the trigger").trigger);
    }
}
