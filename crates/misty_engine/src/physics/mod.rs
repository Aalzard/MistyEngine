//! Physics while playing (rapier). Built from the scene when Play starts, stepped at a fixed rate.
//!
//! Objects with a Rigidbody become dynamic (or kinematic) bodies; colliders attach to the nearest
//! Rigidbody on themselves or a parent, otherwise they are static. Collider sizes include the
//! object's world scale. Character Controllers are kinematic capsules moved by scripts; joints
//! connect bodies to each other or to the world. Submodules: `colliders` (shapes and layers),
//! `joints` (with break forces), `material` (Physics Material assets), `character`, `queries`
//! (raycasts and overlaps), `layers`, `vehicles` (Wheel Colliders), `ragdoll` (the Ragdoll
//! Wizard and switching ragdolls on and off), `cloth` (XPBD cloth on the background pool) and
//! `debug` (the Scene view's Physics Debug lines).

mod character;
pub mod cloth;
mod colliders;
mod convert;
pub mod debug;
mod joints;
pub mod layers;
pub mod material;
mod queries;
pub mod ragdoll;
pub mod shape_cache;
mod vehicles;

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use crate::scene::{CharacterController, Interpolation, Rigidbody, Scene};
use crate::settings::PhysicsSettings;
use convert::{gv, parts, pose, rv};

pub use character::ControllerHit;
pub use joints::{JointBreak, joint_basis};
pub use layers::{ALL_LAYERS, DEFAULT_RAYCAST_LAYERS, LAYERS};
pub use queries::RaycastHit;
pub use vehicles::world_pose_of as wheel_world_pose;

struct Body {
    handle: rp::RigidBodyHandle,
    kinematic: bool,
    /// Rigidbody ▸ Interpolate: draw between `prev` and the current pose.
    interpolate: bool,
    prev: (Vec3, Quat),
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
    /// Upper bound of fixed steps per frame, so a slow frame can't spiral.
    pub max_steps: u32,
    /// Problems found while building (shown in the Console).
    pub warnings: Vec<String>,
    settings: PhysicsSettings,
    /// Fixed body for joints without a connected body (created on first use).
    world_anchor: Option<rp::RigidBodyHandle>,
    springs: Vec<joints::Spring>,
    controllers: HashMap<Entity, character::Controller>,
    controller_hits: Vec<ControllerHit>,
    /// Contacts found while building, reported by the next step.
    pending: Vec<rp::CollisionEvent>,
    wheels: Vec<vehicles::Wheel>,
    /// Rapier joints with their break limits, projection and soft limits.
    joints: Vec<joints::JointRecord>,
    /// Joints broken since the script host last asked.
    joint_breaks: Vec<JointBreak>,
    /// Colliders with a Physics Material (static / dynamic friction picked per contact).
    materials: HashMap<rp::ColliderHandle, material::PhysicsMaterial>,
    /// Loaded Physics Material assets by path (`None` = missing).
    material_assets: HashMap<String, Option<material::PhysicsMaterial>>,
    ragdolls: ragdoll::Ragdolls,
    cloth: cloth::ClothSystem,
}

/// Rigid (unscaled) world pose and world scale of an object.
fn world_pose(scene: &Scene, e: Entity) -> (Vec3, Quat, Vec3) {
    let (scale, rot, pos) = scene.world_matrix(e).to_scale_rotation_translation();
    (pos, rot, scale.abs())
}

impl Physics {
    /// Build with default settings and the given gravity.
    pub fn build(scene: &Scene, gravity: Vec3) -> Self {
        let settings = PhysicsSettings {
            gravity,
            ..Default::default()
        };
        Self::build_with(scene, &settings)
    }

    /// Build from the scene using Project Settings ▸ Physics.
    pub fn build_with(scene: &Scene, settings: &PhysicsSettings) -> Self {
        let mut world = rp::PhysicsWorld::new();
        world.gravity = rv(settings.gravity);
        world.integration_parameters.dt = settings.fixed_timestep;
        let mut physics = Self {
            world,
            bodies: HashMap::new(),
            colliders: HashMap::new(),
            accumulator: 0.0,
            fixed_dt: settings.fixed_timestep,
            max_steps: settings.max_steps_per_frame,
            warnings: Vec::new(),
            settings: settings.clone(),
            world_anchor: None,
            springs: Vec::new(),
            controllers: HashMap::new(),
            controller_hits: Vec::new(),
            pending: Vec::new(),
            wheels: Vec::new(),
            joints: Vec::new(),
            joint_breaks: Vec::new(),
            materials: HashMap::new(),
            material_assets: HashMap::new(),
            ragdolls: ragdoll::Ragdolls::default(),
            cloth: cloth::ClothSystem::default(),
        };
        let objects: Vec<Entity> = scene.walk().into_iter().map(|(e, _)| e).collect();
        physics.add_objects(scene, &objects);
        physics
    }

    /// Add bodies and colliders for `root` and its children (objects created while playing).
    pub fn add_subtree(&mut self, scene: &Scene, root: Entity) {
        self.add_subtrees(scene, &[root]);
    }

    /// [`Physics::add_subtree`] for several roots at once (one broad-phase update for all).
    pub fn add_subtrees(&mut self, scene: &Scene, roots: &[Entity]) {
        let objects: Vec<Entity> = roots.iter().flat_map(|r| scene.subtree(*r)).collect();
        if !objects.is_empty() {
            self.add_objects(scene, &objects);
        }
    }

    /// [`Physics::add_subtrees`] without the zero-length step: the new bodies join the broad
    /// phase in the next real step (World Partition objects, which arrive away from the player;
    /// the streaming budget then doesn't pay a whole-world broad-phase pass per frame).
    pub fn add_subtrees_deferred(&mut self, scene: &Scene, roots: &[Entity]) {
        let objects: Vec<Entity> = roots.iter().flat_map(|r| scene.subtree(*r)).collect();
        if !objects.is_empty() {
            self.add_objects_with(scene, &objects, false);
        }
    }

    /// Bodies, colliders and joints for the active objects in `objects` (Hierarchy order).
    fn add_objects(&mut self, scene: &Scene, objects: &[Entity]) {
        self.add_objects_with(scene, objects, true);
    }

    fn add_objects_with(&mut self, scene: &Scene, objects: &[Entity], register: bool) {
        let objects: Vec<Entity> = objects
            .iter()
            .copied()
            .filter(|e| scene.active_in_hierarchy(*e))
            .collect();
        for &e in &objects {
            self.add_body(scene, e);
        }
        self.add_colliders(scene, &objects);
        self.add_joints(scene, &objects);
        self.add_wheels(scene, &objects);
        self.add_ragdolls(scene, &objects);
        self.cloth.add(scene, &objects);
        if !register {
            return;
        }
        // A zero-length step registers the new bodies and fills the broad phase, so queries
        // work before the first real step; contacts found now are reported by that step.
        let dt = self.world.integration_parameters.dt;
        self.world.integration_parameters.dt = 0.0;
        let events = EventLog::default();
        self.world.step_with_events(&(), &events);
        self.world.integration_parameters.dt = dt;
        self.pending
            .extend(events.0.into_inner().unwrap_or_default());
    }

    /// A body for an object with a Rigidbody or a Character Controller.
    fn add_body(&mut self, scene: &Scene, e: Entity) {
        let w = &scene.world;
        let rb = w.get::<&Rigidbody>(e).ok().map(|r| (*r).clone());
        if let Ok(cc) = w.get::<&CharacterController>(e).map(|c| (*c).clone()) {
            if rb.is_some() {
                self.warnings.push(format!(
                    "'{}': a Character Controller ignores the object's Rigidbody.",
                    scene.name(e)
                ));
            }
            self.add_controller(scene, e, cc);
            return;
        }
        let Some(rb) = rb else { return };
        let (pos, rot, _) = world_pose(scene, e);
        let builder = if rb.is_kinematic {
            rp::RigidBodyBuilder::kinematic_position_based()
        } else {
            rp::RigidBodyBuilder::dynamic()
        };
        let body = builder
            .pose(pose(pos, rot))
            .linear_damping(rb.drag)
            .angular_damping(rb.angular_drag)
            .gravity_scale(if rb.use_gravity { 1.0 } else { 0.0 })
            .locked_axes(locked_axes(&rb))
            .ccd_enabled(rb.continuous)
            .additional_mass(rb.mass.max(1e-4))
            .build();
        let handle = self.world.insert_body(body);
        self.bodies.insert(
            e,
            Body {
                handle,
                kinematic: rb.is_kinematic,
                interpolate: rb.interpolation == Interpolation::Interpolate,
                prev: (pos, rot),
            },
        );
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
        if steps > self.max_steps {
            self.accumulator = 0.0;
        }
        steps.min(self.max_steps)
    }

    pub fn gravity(&self) -> Vec3 {
        gv(self.world.gravity)
    }

    /// Project Settings ▸ Physics this world was built with.
    pub fn settings(&self) -> &PhysicsSettings {
        &self.settings
    }

    /// The Profiler's Physics module counters for this frame (`steps` fixed steps ran).
    pub fn report_profiler(&self, steps: u32) {
        use crate::profiler::{Counter, set_counter};
        let w = &self.world;
        let (mut active, mut sleeping, mut kinematic, mut fixed) = (0, 0, 0, 0);
        for (_, b) in w.bodies.iter() {
            if b.is_kinematic() {
                kinematic += 1;
            } else if !b.is_dynamic() {
                fixed += 1;
            } else if b.is_sleeping() {
                sleeping += 1;
            } else {
                active += 1;
            }
        }
        let triggers = w.colliders.iter().filter(|(_, c)| c.is_sensor()).count();
        let contacts = w
            .narrow_phase
            .contact_pairs()
            .filter(|p| p.has_any_active_contact())
            .count();
        let joints = w.impulse_joints.len() + w.multibody_joints.iter().count();
        for (c, v) in [
            (Counter::PhysicsSteps, steps as usize),
            (Counter::PhysicsBodies, w.bodies.len()),
            (Counter::PhysicsActiveBodies, active),
            (Counter::PhysicsSleepingBodies, sleeping),
            (Counter::PhysicsKinematicBodies, kinematic),
            (Counter::PhysicsStaticBodies, fixed),
            (Counter::PhysicsColliders, w.colliders.len()),
            (Counter::PhysicsTriggers, triggers),
            (Counter::PhysicsContacts, contacts),
            (Counter::PhysicsJoints, joints),
            (Counter::PhysicsCharacterControllers, self.controllers.len()),
        ] {
            set_counter(c, v as f32);
        }
    }

    /// One fixed step; returns the contacts that began or ended.
    pub fn step(&mut self, scene: &Scene) -> Vec<Contact> {
        self.push_kinematic(scene);
        self.apply_springs();
        let dt = self.world.integration_parameters.dt;
        self.apply_wheels(scene, dt);
        // Floaters on water surfaces.
        crate::water::buoyancy::apply(self, scene, dt);
        for b in self.bodies.values_mut().filter(|b| b.interpolate) {
            if let Some(body) = self.world.bodies.get(b.handle) {
                b.prev = parts(body.position());
            }
        }
        let events = EventLog::default();
        let hooks = material::MaterialHooks {
            materials: &self.materials,
        };
        self.world.step_with_events(&hooks, &events);
        self.check_breaks();
        self.project_joints();
        let mut raw = std::mem::take(&mut self.pending);
        raw.extend(events.0.into_inner().unwrap_or_default());
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

    /// Dynamic bodies drive their Transform (interpolated ones are blended between steps).
    pub fn sync_out(&self, scene: &mut Scene) {
        let alpha = (self.accumulator / self.fixed_dt).clamp(0.0, 1.0);
        for (e, b) in &self.bodies {
            if b.kinematic || !scene.contains(*e) || self.ragdolls.is_bone(*e) {
                continue;
            }
            let Some(body) = self.world.bodies.get(b.handle) else {
                continue;
            };
            let (mut p, mut r) = parts(body.position());
            if b.interpolate {
                p = b.prev.0.lerp(p, alpha);
                r = b.prev.1.slerp(r, alpha);
            }
            scene.set_world_position(*e, p);
            scene.set_world_rotation(*e, r);
        }
        self.sync_wheels(scene);
        // Ragdoll bones parent each other: written parents first.
        self.pose_ragdolls(scene);
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

    /// The body's pose in the physics world (Rigidbody.position / rotation).
    pub fn body_pose(&self, e: Entity) -> Option<(Vec3, Quat)> {
        self.body(e).map(|b| parts(b.position()))
    }

    pub fn apply_impulse(&mut self, e: Entity, impulse: Vec3) {
        if let Some(b) = self.body_mut(e) {
            b.apply_impulse(rv(impulse), true);
        }
    }

    /// Impulse at a world point (also turns the body).
    pub fn apply_impulse_at_point(&mut self, e: Entity, impulse: Vec3, point: Vec3) {
        if let Some(b) = self.body_mut(e) {
            b.apply_impulse_at_point(rv(impulse), rv(point), true);
        }
    }

    pub fn apply_torque_impulse(&mut self, e: Entity, impulse: Vec3) {
        if let Some(b) = self.body_mut(e) {
            b.apply_torque_impulse(rv(impulse), true);
        }
    }

    /// Move a body to a pose immediately (script set its Transform).
    pub fn teleport(&mut self, e: Entity, position: Vec3, rotation: Quat) {
        let Some(b) = self.bodies.get_mut(&e) else {
            return;
        };
        b.prev = (position, rotation);
        let kinematic = b.kinematic;
        if let Some(body) = self.world.bodies.get_mut(b.handle) {
            if kinematic {
                body.set_next_kinematic_position(pose(position, rotation));
            } else {
                body.set_position(pose(position, rotation), true);
            }
        }
    }

    /// Rigidbody.MovePosition / MoveRotation: kinematic bodies sweep there during the next
    /// step (pushing what they meet), dynamic ones are placed there. The Transform follows.
    pub fn move_body(&mut self, scene: &mut Scene, e: Entity, position: Vec3, rotation: Quat) {
        if !self.has_body(e) || !scene.contains(e) {
            return;
        }
        scene.set_world_position(e, position);
        scene.set_world_rotation(e, rotation);
        self.teleport(e, position, rotation);
    }

    /// Rigidbody.isKinematic changed by a script.
    pub fn set_kinematic(&mut self, e: Entity, kinematic: bool) {
        let Some(b) = self.bodies.get_mut(&e) else {
            return;
        };
        b.kinematic = kinematic;
        if let Some(body) = self.world.bodies.get_mut(b.handle) {
            body.set_body_type(
                if kinematic {
                    rp::RigidBodyType::KinematicPositionBased
                } else {
                    rp::RigidBodyType::Dynamic
                },
                true,
            );
        }
    }

    /// Rigidbody.useGravity changed by a script.
    pub fn set_use_gravity(&mut self, e: Entity, on: bool) {
        if let Some(b) = self.body_mut(e) {
            b.set_gravity_scale(if on { 1.0 } else { 0.0 }, true);
        }
    }

    /// Remove an object's body and colliders (Destroy).
    pub fn remove(&mut self, e: Entity) {
        if let Some(b) = self.bodies.remove(&e) {
            self.world.remove_body_with_colliders(b.handle, true);
        }
        self.controllers.remove(&e);
        self.cloth.remove(e);
        self.remove_springs(e);
        let colliders = &self.world.colliders;
        self.materials.retain(|h, _| colliders.get(*h).is_some());
        self.remove_wheels(e);
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

    /// Kinematic bodies (and Character Controllers) follow their Transform.
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

    pub fn body_count(&self) -> usize {
        self.world.bodies.len()
    }

    pub fn collider_count(&self) -> usize {
        self.world.colliders.len()
    }
}

/// Rigidbody ▸ Constraints as rapier locked axes.
fn locked_axes(rb: &Rigidbody) -> rp::LockedAxes {
    let flags = [
        rp::LockedAxes::TRANSLATION_LOCKED_X,
        rp::LockedAxes::TRANSLATION_LOCKED_Y,
        rp::LockedAxes::TRANSLATION_LOCKED_Z,
        rp::LockedAxes::ROTATION_LOCKED_X,
        rp::LockedAxes::ROTATION_LOCKED_Y,
        rp::LockedAxes::ROTATION_LOCKED_Z,
    ];
    let frozen = rb.freeze_position.iter().chain(&rb.freeze_rotation);
    flags
        .into_iter()
        .zip(frozen)
        .filter(|(_, on)| **on)
        .fold(rp::LockedAxes::empty(), |m, (f, _)| m | f)
}

/// World matrix helper used by gizmos: rigid pose of a collider centre.
pub fn collider_matrix(scene: &Scene, e: Entity, center: Vec3) -> Mat4 {
    let (p, r, s) = world_pose(scene, e);
    Mat4::from_rotation_translation(r, p + r * (center * s))
}

#[cfg(test)]
mod character_tests;
#[cfg(test)]
mod joint_config_tests;
#[cfg(test)]
mod joint_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vehicle_tests;
