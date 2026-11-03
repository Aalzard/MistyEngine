//! Scene queries: Physics.Raycast / RaycastAll / SphereCast / OverlapSphere.
//!
//! Like the reference engine, rays ignore colliders they start inside, masks select layers by
//! bit, and triggers are hit unless Project Settings ▸ Physics ▸ Queries Hit Triggers is off.

use glam::{Quat, Vec3};
use hecs::Entity;
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude as rp;

use super::Physics;
use super::convert::{gv, pose, rv};

/// One query hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit {
    /// Object of the collider that was hit.
    pub entity: Entity,
    /// Object whose body the collider belongs to (itself or a parent), if any.
    pub body: Option<Entity>,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

impl Physics {
    /// Colliders that a query with `mask` may report.
    fn query_filter<'a, F>(&self, test: &'a F) -> rp::QueryFilter<'a>
    where
        F: Fn(rp::ColliderHandle, &rp::Collider) -> bool,
    {
        let filter = rp::QueryFilter::default().predicate(test);
        if self.settings.queries_hit_triggers {
            filter
        } else {
            filter.exclude_sensors()
        }
    }

    fn hit_from(
        &self,
        h: rp::ColliderHandle,
        point: Vec3,
        normal: Vec3,
        distance: f32,
    ) -> Option<RaycastHit> {
        let entity = *self.colliders.get(&h)?;
        let body = self
            .world
            .colliders
            .get(h)
            .and_then(|c| c.parent())
            .and_then(|p| self.bodies.iter().find(|(_, b)| b.handle == p))
            .map(|(e, _)| *e);
        Some(RaycastHit {
            entity,
            body,
            point,
            normal,
            distance,
        })
    }

    /// Every collider along the ray within `max_distance`, nearest first.
    pub fn raycast_all(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        mask: u32,
    ) -> Vec<RaycastHit> {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO || max_distance.is_nan() || max_distance < 0.0 {
            return Vec::new();
        }
        let test = in_mask(mask);
        let ray = rp::Ray::new(rv(origin), rv(dir));
        let mut hits: Vec<RaycastHit> = self
            .world
            .intersect_ray(
                ray,
                max_distance.min(f32::MAX),
                true,
                self.query_filter(&test),
            )
            // A ray starting inside a collider doesn't hit it.
            .filter(|(_, _, i)| i.time_of_impact > 0.0)
            .filter_map(|(h, _, i)| {
                let t = i.time_of_impact;
                self.hit_from(h, origin + dir * t, gv(i.normal), t)
            })
            .collect();
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        hits
    }

    /// The nearest collider along the ray.
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        mask: u32,
    ) -> Option<RaycastHit> {
        self.raycast_all(origin, direction, max_distance, mask)
            .into_iter()
            .next()
    }

    /// Sweep a sphere along the ray; colliders it starts inside are skipped.
    pub fn sphere_cast(
        &self,
        origin: Vec3,
        radius: f32,
        direction: Vec3,
        max_distance: f32,
        mask: u32,
    ) -> Option<RaycastHit> {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO
            || radius.is_nan()
            || radius <= 0.0
            || max_distance.is_nan()
            || max_distance < 0.0
        {
            return None;
        }
        let shape = rp::Ball::new(radius);
        let options = ShapeCastOptions {
            max_time_of_impact: max_distance.min(f32::MAX),
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let test = in_mask(mask);
        let (h, hit) = self.world.cast_shape(
            &pose(origin, Quat::IDENTITY),
            rv(dir),
            &shape,
            options,
            self.query_filter(&test),
        )?;
        if hit.time_of_impact <= 0.0 {
            return None;
        }
        let t = hit.time_of_impact;
        self.hit_from(h, gv(hit.witness1), gv(hit.normal1), t)
    }

    /// Objects of the colliders touching or inside the sphere (one entry per collider).
    pub fn overlap_sphere(&self, center: Vec3, radius: f32, mask: u32) -> Vec<Entity> {
        if radius.is_nan() || radius < 0.0 {
            return Vec::new();
        }
        let shape = rp::Ball::new(radius.max(1e-4));
        let test = in_mask(mask);
        self.world
            .intersect_shape(
                pose(center, Quat::IDENTITY),
                &shape,
                self.query_filter(&test),
            )
            .filter_map(|(h, _)| self.colliders.get(&h).copied())
            .collect()
    }
}

/// Predicate: the collider's layer is in `mask`.
fn in_mask(mask: u32) -> impl Fn(rp::ColliderHandle, &rp::Collider) -> bool {
    move |_, c| c.collision_groups().memberships.bits() & mask != 0
}
