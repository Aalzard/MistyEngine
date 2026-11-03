//! 2D physics while playing (rapier2d), a world of its own beside the 3D one.
//!
//! Objects with a Rigidbody 2D become bodies in the XY plane (position = world X/Y, rotation =
//! the angle about Z); 2D colliders attach to the closest Rigidbody 2D on themselves or a parent,
//! otherwise they are static. Collider sizes include the object's world X/Y scale. Layers use the
//! same Layer Collision Matrix as 3D physics. Submodules: `colliders` (shapes), `queries`
//! (raycasts and overlaps), `contacts` (enter/stay/exit tracking).

mod colliders;
mod contacts;
mod queries;

use std::collections::HashMap;

use glam::{Quat, Vec2, Vec3};
use hecs::Entity;
use rapier2d::prelude as rp;

use crate::scene::{BodyType2D, CollisionDetection2D, Rigidbody2D, Scene};
use crate::settings::{Physics2DSettings, PhysicsSettings};

pub use colliders::polygon_points;
pub use contacts::{Contact2D, Phase};
pub use queries::RaycastHit2D;

/// glam (ours) → rapier's vector type.
pub(crate) fn rv(v: Vec2) -> rp::Vector {
    rp::Vector::new(v.x, v.y)
}

/// rapier's vector type → glam (ours).
pub(crate) fn gv(v: rp::Vector) -> Vec2 {
    Vec2::new(v.x, v.y)
}

pub(crate) fn pose(p: Vec2, angle: f32) -> rp::Pose {
    rp::Pose::new(rv(p), angle)
}

/// Angle (radians) of a rotation about Z, measured from +X towards +Y.
pub fn z_angle(q: Quat) -> f32 {
    let x = q * Vec3::X;
    x.y.atan2(x.x)
}

/// The frame 2D colliders live in (world position and Z angle, no scale) and the world X/Y
/// scale their sizes are multiplied by; for gizmos and editor handles.
pub fn collider_frame(scene: &Scene, e: Entity) -> (glam::Mat4, Vec2) {
    let (p, a, s, z) = world_pose(scene, e);
    let m = glam::Mat4::from_rotation_translation(Quat::from_rotation_z(a), p.extend(z));
    (m, s)
}

struct Body {
    handle: rp::RigidBodyHandle,
    kind: BodyType2D,
    /// MovePosition / MoveRotation target for the next step (kinematic bodies sweep there).
    target: Option<(Vec2, f32)>,
}

pub struct Physics2D {
    world: rp::PhysicsWorld,
    bodies: HashMap<Entity, Body>,
    /// Which object each collider belongs to.
    colliders: HashMap<rp::ColliderHandle, Entity>,
    settings: Physics2DSettings,
    /// Layer Collision Matrix (shared with 3D).
    layers: PhysicsSettings,
    pub fixed_dt: f32,
    /// Problems found while building (shown in the Console).
    pub warnings: Vec<String>,
    contacts: contacts::Tracker,
}

/// World X/Y position, Z angle, world scale (absolute) and depth of an object.
fn world_pose(scene: &Scene, e: Entity) -> (Vec2, f32, Vec2, f32) {
    let (scale, rot, pos) = scene.world_matrix(e).to_scale_rotation_translation();
    (pos.truncate(), z_angle(rot), scale.truncate().abs(), pos.z)
}

impl Physics2D {
    /// Empty world with default settings.
    pub fn empty() -> Self {
        Self::build(
            &Scene::default(),
            &Physics2DSettings::default(),
            &Default::default(),
        )
    }

    /// Build from the scene with Project Settings ▸ Physics 2D (and the 3D layer matrix).
    pub fn build(scene: &Scene, settings: &Physics2DSettings, layers: &PhysicsSettings) -> Self {
        let mut world = rp::PhysicsWorld::new();
        world.gravity = rv(settings.gravity);
        world.integration_parameters.dt = layers.fixed_timestep;
        let mut p = Self {
            world,
            bodies: HashMap::new(),
            colliders: HashMap::new(),
            settings: settings.clone(),
            layers: layers.clone(),
            fixed_dt: layers.fixed_timestep,
            warnings: Vec::new(),
            contacts: Default::default(),
        };
        let objects: Vec<Entity> = scene.walk().into_iter().map(|(e, _)| e).collect();
        p.add_objects(scene, &objects);
        p
    }

    /// Add bodies and colliders for `root` and its children (objects created while playing).
    pub fn add_subtree(&mut self, scene: &Scene, root: Entity) {
        let objects: Vec<Entity> = scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| scene.is_ancestor(root, *e))
            .collect();
        self.add_objects(scene, &objects);
    }

    fn add_objects(&mut self, scene: &Scene, objects: &[Entity]) {
        let objects: Vec<Entity> = objects
            .iter()
            .copied()
            .filter(|e| scene.active_in_hierarchy(*e))
            .collect();
        for &e in &objects {
            self.add_body(scene, e);
        }
        self.add_colliders(scene, &objects);
        // A zero-length step fills the broad phase so queries work before the first step.
        let dt = self.world.integration_parameters.dt;
        self.world.integration_parameters.dt = 0.0;
        let events = contacts::EventLog::default();
        self.world.step_with_events(&(), &events);
        self.world.integration_parameters.dt = dt;
        self.contacts.defer(events.take());
    }

    fn add_body(&mut self, scene: &Scene, e: Entity) {
        let Ok(rb) = scene.world.get::<&Rigidbody2D>(e).map(|r| (*r).clone()) else {
            return;
        };
        let (p, a, _, _) = world_pose(scene, e);
        let builder = match rb.body_type {
            BodyType2D::Dynamic => rp::RigidBodyBuilder::dynamic(),
            BodyType2D::Kinematic => rp::RigidBodyBuilder::kinematic_velocity_based(),
            BodyType2D::Static => rp::RigidBodyBuilder::fixed(),
        };
        let mut locked = rp::LockedAxes::empty();
        if rb.freeze_position[0] {
            locked |= rp::LockedAxes::TRANSLATION_LOCKED_X;
        }
        if rb.freeze_position[1] {
            locked |= rp::LockedAxes::TRANSLATION_LOCKED_Y;
        }
        if rb.freeze_rotation {
            locked |= rp::LockedAxes::ROTATION_LOCKED;
        }
        let body = builder
            .pose(pose(p, a))
            .linear_damping(rb.linear_drag.max(0.0))
            .angular_damping(rb.angular_drag.max(0.0))
            .gravity_scale(rb.gravity_scale)
            .locked_axes(locked)
            .ccd_enabled(rb.collision_detection == CollisionDetection2D::Continuous)
            .additional_mass(rb.mass.max(1e-4))
            .build();
        let handle = self.world.insert_body(body);
        self.bodies.insert(
            e,
            Body {
                handle,
                kind: rb.body_type,
                target: None,
            },
        );
    }

    /// One fixed step; returns the contacts that began, continue or ended.
    pub fn step(&mut self) -> Vec<Contact2D> {
        let dt = self.fixed_dt;
        // Kinematic bodies: sweep to their MovePosition target (velocity for one step).
        let mut restore = Vec::new();
        for b in self.bodies.values_mut() {
            let Some(body) = self.world.bodies.get_mut(b.handle) else {
                continue;
            };
            if let Some((p, a)) = b.target.take() {
                if b.kind == BodyType2D::Kinematic && dt > 0.0 {
                    // The rotation is set directly; only the move is swept.
                    restore.push((b.handle, body.linvel(), body.angvel()));
                    let cur = *body.position();
                    body.set_position(pose(gv(cur.translation), a), true);
                    body.set_linvel((rv(p) - cur.translation) / dt, true);
                    body.set_angvel(0.0, true);
                } else {
                    body.set_position(pose(p, a), true);
                }
            }
        }
        let before = self.velocities();
        let events = contacts::EventLog::default();
        self.world.step_with_events(&(), &events);
        for (h, v, w) in restore {
            if let Some(body) = self.world.bodies.get_mut(h) {
                body.set_linvel(v, true);
                body.set_angvel(w, true);
            }
        }
        self.contacts
            .update(&self.world, &self.colliders, events.take(), &before)
    }

    /// Linear velocities of the bodies before a step (for relative velocities of new contacts).
    fn velocities(&self) -> HashMap<rp::RigidBodyHandle, Vec2> {
        self.bodies
            .values()
            .filter_map(|b| Some((b.handle, gv(self.world.bodies.get(b.handle)?.linvel()))))
            .collect()
    }

    /// Bodies drive their Transform's X/Y position and Z rotation (depth is kept).
    pub fn sync_out(&self, scene: &mut Scene) {
        for (e, b) in &self.bodies {
            if b.kind == BodyType2D::Static || !scene.contains(*e) {
                continue;
            }
            let Some(body) = self.world.bodies.get(b.handle) else {
                continue;
            };
            let p = body.position();
            let z = scene.world_position(*e).z;
            scene.set_world_position(*e, gv(p.translation).extend(z));
            scene.set_world_rotation(*e, Quat::from_rotation_z(p.rotation.angle()));
        }
    }

    fn body(&self, e: Entity) -> Option<&rp::RigidBody> {
        self.world.bodies.get(self.bodies.get(&e)?.handle)
    }

    fn body_mut(&mut self, e: Entity) -> Option<&mut rp::RigidBody> {
        let h = self.bodies.get(&e)?.handle;
        self.world.bodies.get_mut(h)
    }

    pub fn has_body(&self, e: Entity) -> bool {
        self.bodies.contains_key(&e)
    }

    pub fn velocity(&self, e: Entity) -> Option<Vec2> {
        self.body(e).map(|b| gv(b.linvel()))
    }

    pub fn set_velocity(&mut self, e: Entity, v: Vec2) {
        if let Some(b) = self.body_mut(e) {
            b.set_linvel(rv(v), true);
        }
    }

    /// Degrees per second, counter-clockwise.
    pub fn angular_velocity(&self, e: Entity) -> Option<f32> {
        self.body(e).map(|b| b.angvel().to_degrees())
    }

    pub fn set_angular_velocity(&mut self, e: Entity, degrees: f32) {
        if let Some(b) = self.body_mut(e) {
            b.set_angvel(degrees.to_radians(), true);
        }
    }

    pub fn mass(&self, e: Entity) -> Option<f32> {
        self.body(e).map(|b| b.mass())
    }

    /// Body position and Z angle in degrees (Rigidbody2D.position / rotation).
    pub fn body_pose(&self, e: Entity) -> Option<(Vec2, f32)> {
        self.body(e).map(|b| {
            let p = b.position();
            (gv(p.translation), p.rotation.angle().to_degrees())
        })
    }

    pub fn apply_impulse(&mut self, e: Entity, impulse: Vec2) {
        if let Some(b) = self.body_mut(e) {
            b.apply_impulse(rv(impulse), true);
        }
    }

    pub fn apply_torque_impulse(&mut self, e: Entity, impulse: f32) {
        if let Some(b) = self.body_mut(e) {
            b.apply_torque_impulse(impulse, true);
        }
    }

    pub fn gravity_scale(&self, e: Entity) -> Option<f32> {
        self.body(e).map(|b| b.gravity_scale())
    }

    pub fn set_gravity_scale(&mut self, e: Entity, s: f32) {
        if let Some(b) = self.body_mut(e) {
            b.set_gravity_scale(s, true);
        }
    }

    pub fn body_type(&self, e: Entity) -> Option<BodyType2D> {
        self.bodies.get(&e).map(|b| b.kind)
    }

    /// Rigidbody2D.bodyType changed by a script.
    pub fn set_body_type(&mut self, e: Entity, kind: BodyType2D) {
        let Some(b) = self.bodies.get_mut(&e) else {
            return;
        };
        b.kind = kind;
        if let Some(body) = self.world.bodies.get_mut(b.handle) {
            body.set_body_type(
                match kind {
                    BodyType2D::Dynamic => rp::RigidBodyType::Dynamic,
                    BodyType2D::Kinematic => rp::RigidBodyType::KinematicVelocityBased,
                    BodyType2D::Static => rp::RigidBodyType::Fixed,
                },
                true,
            );
        }
    }

    /// Rigidbody2D.MovePosition / MoveRotation (degrees): kinematic bodies sweep there during
    /// the next step, pushing what they meet; other bodies are placed there.
    pub fn move_body(&mut self, e: Entity, position: Option<Vec2>, degrees: Option<f32>) {
        let Some((p, a)) = self.body_pose(e) else {
            return;
        };
        let Some(b) = self.bodies.get_mut(&e) else {
            return;
        };
        let (tp, ta) = b.target.unwrap_or((p, a.to_radians()));
        b.target = Some((
            position.unwrap_or(tp),
            degrees.map(f32::to_radians).unwrap_or(ta),
        ));
    }

    /// Move a body to a pose immediately (a script set its Transform).
    pub fn teleport(&mut self, e: Entity, position: Vec3, rotation: Quat) {
        if let Some(b) = self.body_mut(e) {
            b.set_position(pose(position.truncate(), z_angle(rotation)), true);
        }
    }

    pub fn gravity(&self) -> Vec2 {
        gv(self.world.gravity)
    }

    /// Physics2D.gravity changed by a script.
    pub fn set_gravity(&mut self, g: Vec2) {
        self.world.gravity = rv(g);
        for b in self.world.bodies.iter_mut().map(|(_, b)| b) {
            b.wake_up(true);
        }
    }

    pub fn settings(&self) -> &Physics2DSettings {
        &self.settings
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
        }
        let world = &self.world;
        self.colliders
            .retain(|h, _| world.colliders.get(*h).is_some());
        self.contacts.forget(&self.colliders);
    }

    pub fn body_count(&self) -> usize {
        self.world.bodies.len()
    }

    pub fn collider_count(&self) -> usize {
        self.world.colliders.len()
    }
}

#[cfg(test)]
mod tests;
