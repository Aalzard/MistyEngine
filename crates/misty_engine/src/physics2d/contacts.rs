//! Contact tracking: rapier's started/stopped events become Enter / Stay / Exit contacts between
//! objects, with a contact point, normal and relative velocity for collisions.

use std::collections::HashMap;

use glam::Vec2;
use hecs::Entity;
use rapier2d::prelude as rp;

use super::gv;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Enter,
    Stay,
    Exit,
}

/// A contact between the colliders of objects `a` and `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact2D {
    pub a: Entity,
    pub b: Entity,
    pub phase: Phase,
    /// One of the colliders is a trigger.
    pub trigger: bool,
    /// World contact point (collisions that touch; the midpoint of the objects otherwise).
    pub point: Vec2,
    /// World normal pointing from `a` towards `b`.
    pub normal: Vec2,
    /// Velocity of `b` minus velocity of `a` (before the step for new contacts).
    pub relative_velocity: Vec2,
}

/// Collects rapier collision events during a step.
#[derive(Default)]
pub(super) struct EventLog(std::sync::Mutex<Vec<rp::CollisionEvent>>);

impl EventLog {
    pub fn take(self) -> Vec<rp::CollisionEvent> {
        self.0.into_inner().unwrap_or_default()
    }
}

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

type Pair = (rp::ColliderHandle, rp::ColliderHandle);

/// Collider pairs touching now (value = trigger pair), plus events waiting for the next step.
#[derive(Default)]
pub(super) struct Tracker {
    touching: HashMap<Pair, bool>,
    deferred: Vec<rp::CollisionEvent>,
}

impl Tracker {
    /// Events found outside a real step (building), reported by the next one.
    pub fn defer(&mut self, events: Vec<rp::CollisionEvent>) {
        self.deferred.extend(events);
    }

    /// Drop pairs whose colliders were removed.
    pub fn forget(&mut self, alive: &HashMap<rp::ColliderHandle, Entity>) {
        self.touching
            .retain(|(a, b), _| alive.contains_key(a) && alive.contains_key(b));
    }

    /// Apply a step's events: Exit for ended pairs, Enter for new ones, Stay for the rest.
    pub fn update(
        &mut self,
        world: &rp::PhysicsWorld,
        owners: &HashMap<rp::ColliderHandle, Entity>,
        events: Vec<rp::CollisionEvent>,
        before: &HashMap<rp::RigidBodyHandle, Vec2>,
    ) -> Vec<Contact2D> {
        let mut out = Vec::new();
        let mut entered: Vec<Pair> = Vec::new();
        let mut all = std::mem::take(&mut self.deferred);
        all.extend(events);
        let mut stays: Vec<Pair> = self.touching.keys().copied().collect();
        for ev in all {
            match ev {
                rp::CollisionEvent::Started(a, b, _) => {
                    self.touching.insert((a, b), ev.sensor());
                    entered.push((a, b));
                }
                rp::CollisionEvent::Stopped(a, b, _) => {
                    let key = if self.touching.contains_key(&(a, b)) {
                        (a, b)
                    } else {
                        (b, a)
                    };
                    let trigger = self.touching.remove(&key).unwrap_or(ev.sensor());
                    stays.retain(|p| *p != key);
                    entered.retain(|p| *p != key);
                    if let Some(c) = contact(world, owners, key, Phase::Exit, trigger, before) {
                        out.push(c);
                    }
                }
            }
        }
        for key in entered {
            let trigger = self.touching.get(&key).copied().unwrap_or(false);
            out.extend(contact(world, owners, key, Phase::Enter, trigger, before));
        }
        for key in stays {
            if let Some(&trigger) = self.touching.get(&key) {
                out.extend(contact(
                    world,
                    owners,
                    key,
                    Phase::Stay,
                    trigger,
                    &HashMap::new(),
                ));
            }
        }
        out
    }
}

/// Linear velocity of a collider's body (`before` the step when known).
fn body_velocity(
    world: &rp::PhysicsWorld,
    c: &rp::Collider,
    before: &HashMap<rp::RigidBodyHandle, Vec2>,
) -> Vec2 {
    let Some(h) = c.parent() else {
        return Vec2::ZERO;
    };
    before
        .get(&h)
        .copied()
        .or_else(|| world.bodies.get(h).map(|b| gv(b.linvel())))
        .unwrap_or(Vec2::ZERO)
}

fn contact(
    world: &rp::PhysicsWorld,
    owners: &HashMap<rp::ColliderHandle, Entity>,
    (h1, h2): Pair,
    phase: Phase,
    trigger: bool,
    before: &HashMap<rp::RigidBodyHandle, Vec2>,
) -> Option<Contact2D> {
    let (a, b) = (*owners.get(&h1)?, *owners.get(&h2)?);
    let (c1, c2) = (world.colliders.get(h1), world.colliders.get(h2));
    let (p1, p2) = (
        c1.map_or(Vec2::ZERO, |c| gv(c.position().translation)),
        c2.map_or(Vec2::ZERO, |c| gv(c.position().translation)),
    );
    let mut point = (p1 + p2) * 0.5;
    let mut normal = (p2 - p1).normalize_or_zero();
    if !trigger
        && let Some(pair) = world.contact_pair(h1, h2)
        && let Some(m) = pair.manifolds().iter().find(|m| !m.points.is_empty())
        && let Some(c) = world.colliders.get(pair.collider1)
    {
        // Manifold data is expressed for the pair's own collider order.
        let flip = if pair.collider1 == h1 { 1.0 } else { -1.0 };
        let local = m.points[0].local_p1;
        point = gv(c.position().transform_point(local));
        let n = gv(c.position().transform_vector(m.local_n1));
        if n != Vec2::ZERO {
            normal = n * flip;
        }
    }
    let relative_velocity = match (c1, c2) {
        (Some(c1), Some(c2)) => body_velocity(world, c2, before) - body_velocity(world, c1, before),
        _ => Vec2::ZERO,
    };
    Some(Contact2D {
        a,
        b,
        phase,
        trigger,
        point,
        normal,
        relative_velocity,
    })
}
