//! Scene queries: Physics.Raycast / RaycastAll / SphereCast / OverlapSphere, and the box and
//! capsule shapes (OverlapBox, BoxCast, CheckCapsule, …) through [`QueryShape`].
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

/// The shape of an overlap or sweep query (sizes in metres, world space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QueryShape {
    Sphere {
        radius: f32,
    },
    /// A box of these half sizes, turned by the query's rotation.
    Box {
        half_extents: Vec3,
    },
    /// The capsule round the segment `a`–`b` (offsets from the query's position).
    Capsule {
        a: Vec3,
        b: Vec3,
        radius: f32,
    },
}

/// Most hits a sweep reports (`SphereCastAll`, `BoxCastAll`, …).
pub const MAX_CAST_HITS: usize = 64;

impl QueryShape {
    /// The capsule round world points `p1`–`p2`, placed at their midpoint.
    pub fn capsule(p1: Vec3, p2: Vec3, radius: f32) -> (Self, Vec3) {
        let c = (p1 + p2) * 0.5;
        (
            Self::Capsule {
                a: p1 - c,
                b: p2 - c,
                radius,
            },
            c,
        )
    }

    /// The parry shape, `None` for sizes that are negative, not finite or too large.
    fn shape(&self) -> Option<rp::SharedShape> {
        let ok = |v: f32| (0.0..=super::MAX_EXTENT).contains(&v);
        match *self {
            Self::Sphere { radius } if ok(radius) => Some(rp::SharedShape::ball(radius.max(1e-4))),
            Self::Box { half_extents: h } if ok(h.x) && ok(h.y) && ok(h.z) => {
                let h = h.max(Vec3::splat(1e-4));
                Some(rp::SharedShape::cuboid(h.x, h.y, h.z))
            }
            Self::Capsule { a, b, radius } if ok(radius) && a.is_finite() && b.is_finite() => {
                Some(rp::SharedShape::capsule(rv(a), rv(b), radius.max(1e-4)))
            }
            _ => None,
        }
    }
}

fn rotation_or_identity(r: Quat) -> Quat {
    glam::Vec4::from(r)
        .try_normalize()
        .map_or(Quat::IDENTITY, Quat::from_vec4)
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
            .and_then(|p| {
                let e = self.world.bodies.get(p).and_then(super::body_object)?;
                self.bodies
                    .get(&e)
                    .is_some_and(|b| b.handle == p)
                    .then_some(e)
            });
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

    /// The nearest solid collider (triggers never count) between `from` and `to` that `keep`
    /// accepts, without allocating (line-of-sight checks run every AI tick).
    pub fn line_hit(
        &self,
        from: Vec3,
        to: Vec3,
        mask: u32,
        keep: impl Fn(&RaycastHit) -> bool,
    ) -> Option<RaycastHit> {
        let delta = to - from;
        let len = delta.length();
        let dir = delta.normalize_or_zero();
        if dir == Vec3::ZERO || !len.is_finite() {
            return None;
        }
        let test = in_mask(mask);
        let filter = rp::QueryFilter::default()
            .predicate(&test)
            .exclude_sensors();
        let ray = rp::Ray::new(rv(from), rv(dir));
        let mut best: Option<RaycastHit> = None;
        for (h, _, i) in self.world.intersect_ray(ray, len, true, filter) {
            let t = i.time_of_impact;
            if t <= 0.0 || best.is_some_and(|b| b.distance <= t) {
                continue;
            }
            if let Some(hit) = self.hit_from(h, from + dir * t, gv(i.normal), t)
                && keep(&hit)
            {
                best = Some(hit);
            }
        }
        best
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

impl Physics {
    /// Objects of the colliders touching or inside `shape` at `center` turned by `rotation`
    /// (one entry per collider).
    pub fn overlap(
        &self,
        shape: QueryShape,
        center: Vec3,
        rotation: Quat,
        mask: u32,
    ) -> Vec<Entity> {
        let Some(s) = shape.shape().filter(|_| center.is_finite()) else {
            return Vec::new();
        };
        let test = in_mask(mask);
        self.world
            .intersect_shape(
                pose(center, rotation_or_identity(rotation)),
                s.as_ref(),
                self.query_filter(&test),
            )
            .filter_map(|(h, _)| self.colliders.get(&h).copied())
            .collect()
    }

    /// Whether any collider touches `shape` (`Physics.CheckSphere` / `CheckBox` /
    /// `CheckCapsule`); stops at the first one.
    pub fn check(&self, shape: QueryShape, center: Vec3, rotation: Quat, mask: u32) -> bool {
        let Some(s) = shape.shape().filter(|_| center.is_finite()) else {
            return false;
        };
        let test = in_mask(mask);
        self.world
            .intersect_shape(
                pose(center, rotation_or_identity(rotation)),
                s.as_ref(),
                self.query_filter(&test),
            )
            .next()
            .is_some()
    }

    /// Sweep `shape` from `origin` along `direction`: the nearest collider hit within
    /// `max_distance` (ones it starts inside are skipped, like `sphere_cast`).
    pub fn cast(
        &self,
        shape: QueryShape,
        origin: Vec3,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        mask: u32,
    ) -> Option<RaycastHit> {
        self.cast_skipping(shape, origin, rotation, direction, max_distance, mask, &[])
    }

    /// Every collider the sweep hits within `max_distance`, nearest first (at most
    /// [`MAX_CAST_HITS`]).
    pub fn cast_all(
        &self,
        shape: QueryShape,
        origin: Vec3,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        mask: u32,
    ) -> Vec<RaycastHit> {
        let mut skip: Vec<rp::ColliderHandle> = Vec::new();
        let mut hits = Vec::new();
        while hits.len() < MAX_CAST_HITS {
            let Some((h, hit)) = self.cast_one(
                shape,
                origin,
                rotation,
                direction,
                max_distance,
                mask,
                &skip,
            ) else {
                break;
            };
            skip.push(h);
            if let Some(hit) = hit {
                hits.push(hit);
            }
        }
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        hits
    }

    #[allow(clippy::too_many_arguments)]
    fn cast_skipping(
        &self,
        shape: QueryShape,
        origin: Vec3,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        mask: u32,
        skip: &[rp::ColliderHandle],
    ) -> Option<RaycastHit> {
        let mut skip = skip.to_vec();
        // Colliders the shape starts inside are passed over (and the sweep goes on).
        loop {
            let (h, hit) = self.cast_one(
                shape,
                origin,
                rotation,
                direction,
                max_distance,
                mask,
                &skip,
            )?;
            if hit.is_some() {
                return hit;
            }
            skip.push(h);
        }
    }

    /// One sweep ignoring `skip`: the collider met and its hit (`None` when the shape starts
    /// inside it).
    #[allow(clippy::too_many_arguments)]
    fn cast_one(
        &self,
        shape: QueryShape,
        origin: Vec3,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        mask: u32,
        skip: &[rp::ColliderHandle],
    ) -> Option<(rp::ColliderHandle, Option<RaycastHit>)> {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO || !origin.is_finite() || max_distance.is_nan() || max_distance < 0.0 {
            return None;
        }
        let s = shape.shape()?;
        let options = ShapeCastOptions {
            max_time_of_impact: max_distance.min(f32::MAX),
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let test = |h: rp::ColliderHandle, c: &rp::Collider| {
            !skip.contains(&h) && c.collision_groups().memberships.bits() & mask != 0
        };
        let (h, hit) = self.world.cast_shape(
            &pose(origin, rotation_or_identity(rotation)),
            rv(dir),
            s.as_ref(),
            options,
            self.query_filter(&test),
        )?;
        if hit.time_of_impact <= 0.0 {
            return Some((h, None));
        }
        let t = hit.time_of_impact;
        Some((h, self.hit_from(h, gv(hit.witness1), gv(hit.normal1), t)))
    }
}

/// Predicate: the collider's layer is in `mask`.
fn in_mask(mask: u32) -> impl Fn(rp::ColliderHandle, &rp::Collider) -> bool {
    move |_, c| c.collision_groups().memberships.bits() & mask != 0
}
