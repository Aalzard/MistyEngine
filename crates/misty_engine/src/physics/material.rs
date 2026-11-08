//! Physics Material assets (`.physicmat`): dynamic and static friction, bounciness and how each
//! is combined with the other collider's value. A collider's Material slot replaces its own
//! friction / bounciness.
//!
//! The combine rule of a contact is the higher-priority one of the two colliders' rules:
//! Average < Minimum < Multiply < Maximum. Static friction holds bodies that are (nearly) at rest
//! against each other; once they slide, dynamic friction applies. rapier has one friction
//! coefficient, so a contact hook picks the combined static or dynamic value per contact from
//! the bodies' sliding speed.

use std::collections::HashMap;

use glam::Vec3;
use rapier3d::prelude as rp;
use serde::{Deserialize, Serialize};

use super::convert::gv;

pub const EXTENSION: &str = "physicmat";

pub fn is_physics_material(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// Sliding speed (m/s) under which two surfaces count as resting (static friction).
pub const STATIC_SPEED: f32 = 0.05;

/// How two colliders' values are combined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Combine {
    #[default]
    Average,
    Minimum,
    Multiply,
    Maximum,
}

impl Combine {
    pub const ALL: [Self; 4] = [Self::Average, Self::Minimum, Self::Multiply, Self::Maximum];

    pub fn name(self) -> &'static str {
        match self {
            Self::Average => "Average",
            Self::Minimum => "Minimum",
            Self::Multiply => "Multiply",
            Self::Maximum => "Maximum",
        }
    }

    /// Combine `a` (with rule `self`) and `b` (with rule `other`): the higher-priority rule wins.
    pub fn combine(self, other: Self, a: f32, b: f32) -> f32 {
        match self.max(other) {
            Self::Average => (a + b) * 0.5,
            Self::Minimum => a.min(b),
            Self::Multiply => a * b,
            Self::Maximum => a.max(b),
        }
    }

    /// The same rule in rapier (whose priority order matches).
    pub fn rapier(self) -> rp::CoefficientCombineRule {
        match self {
            Self::Average => rp::CoefficientCombineRule::Average,
            Self::Minimum => rp::CoefficientCombineRule::Min,
            Self::Multiply => rp::CoefficientCombineRule::Multiply,
            Self::Maximum => rp::CoefficientCombineRule::Max,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicsMaterial {
    /// Friction while sliding (0 = ice, 1 = rubber).
    pub dynamic_friction: f32,
    /// Friction that holds a resting body in place.
    pub static_friction: f32,
    /// 0 = no bounce, 1 = bounces back with all its speed.
    pub bounciness: f32,
    pub friction_combine: Combine,
    pub bounce_combine: Combine,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self {
            dynamic_friction: 0.6,
            static_friction: 0.6,
            bounciness: 0.0,
            friction_combine: Combine::Average,
            bounce_combine: Combine::Average,
        }
    }
}

impl PhysicsMaterial {
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(&crate::asset_db::resolve_json(text)).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The material at a project-relative path.
    pub fn load_asset(rel: &str) -> Result<Self, String> {
        Self::load(&crate::assets::absolute(rel))
    }

    /// Friction and bounciness of a collider without a material.
    pub fn from_surface(s: &crate::scene::PhysicsSurface) -> Self {
        Self {
            dynamic_friction: s.friction,
            static_friction: s.friction,
            bounciness: s.bounciness,
            ..Default::default()
        }
    }

    /// Friction of a contact with `other`: static while resting, dynamic while sliding.
    pub fn friction_with(&self, other: &Self, sliding_speed: f32) -> f32 {
        let (a, b) = if sliding_speed < STATIC_SPEED {
            (self.static_friction, other.static_friction)
        } else {
            (self.dynamic_friction, other.dynamic_friction)
        };
        self.friction_combine
            .combine(other.friction_combine, a.max(0.0), b.max(0.0))
    }

    pub fn bounciness_with(&self, other: &Self) -> f32 {
        self.bounce_combine
            .combine(other.bounce_combine, self.bounciness, other.bounciness)
            .clamp(0.0, 1.0)
    }
}

/// Picks static or dynamic friction per contact for colliders with a Physics Material.
pub(super) struct MaterialHooks<'a> {
    pub materials: &'a HashMap<rp::ColliderHandle, PhysicsMaterial>,
}

impl rp::PhysicsHooks for MaterialHooks<'_> {
    fn filter_contact_pair(&self, _context: &rp::PairFilterContext) -> Option<rp::SolverFlags> {
        Some(rp::SolverFlags::default())
    }

    fn filter_intersection_pair(&self, _context: &rp::PairFilterContext) -> bool {
        true
    }

    fn modify_solver_contacts(&self, cx: &mut rp::ContactModificationContext) {
        let material = |h: rp::ColliderHandle| {
            self.materials.get(&h).cloned().unwrap_or_else(|| {
                let c = cx.colliders.get(h);
                PhysicsMaterial {
                    dynamic_friction: c.map_or(0.6, |c| c.friction()),
                    static_friction: c.map_or(0.6, |c| c.friction()),
                    bounciness: c.map_or(0.0, |c| c.restitution()),
                    ..Default::default()
                }
            })
        };
        let (m1, m2) = (material(cx.collider1), material(cx.collider2));
        let velocity_at = |body: Option<rp::RigidBodyHandle>, p: Vec3| {
            body.and_then(|b| cx.bodies.get(b)).map_or(Vec3::ZERO, |b| {
                gv(b.velocity_at_point(super::convert::rv(p)))
            })
        };
        let (b1, b2) = (cx.rigid_body1, cx.rigid_body2);
        let Some(m) = cx.rigid_mut() else {
            return;
        };
        let normal = gv(*m.normal);
        let sliding = m
            .solver_contacts
            .iter()
            .map(|c| {
                let p = gv(c.anchor1);
                let rel = velocity_at(b1, p) - velocity_at(b2, p);
                (rel - normal * rel.dot(normal)).length()
            })
            .fold(0.0, f32::max);
        *m.friction = m1.friction_with(&m2, sliding);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_priority_rule_wins() {
        use Combine::*;
        assert_eq!(Average.combine(Average, 0.2, 0.6), 0.4);
        assert_eq!(Average.combine(Minimum, 0.2, 0.6), 0.2);
        assert_eq!(Maximum.combine(Minimum, 0.2, 0.6), 0.6);
        assert!((Multiply.combine(Average, 0.5, 0.6) - 0.3).abs() < 1e-6);
        assert!((Minimum.combine(Multiply, 0.5, 0.6) - 0.3).abs() < 1e-6);
        assert_eq!(Maximum.combine(Multiply, 0.5, 0.6), 0.6);
        // rapier's own order agrees.
        for (a, b) in [(Average, Minimum), (Minimum, Multiply), (Multiply, Maximum)] {
            assert!((a.rapier() as u32) < (b.rapier() as u32));
        }
    }

    #[test]
    fn static_friction_while_resting_dynamic_while_sliding() {
        let rubber = PhysicsMaterial {
            dynamic_friction: 0.8,
            static_friction: 1.0,
            ..Default::default()
        };
        let ice = PhysicsMaterial {
            dynamic_friction: 0.02,
            static_friction: 0.1,
            friction_combine: Combine::Minimum,
            ..Default::default()
        };
        assert_eq!(rubber.friction_with(&rubber, 0.0), 1.0);
        assert_eq!(rubber.friction_with(&rubber, 1.0), 0.8);
        assert_eq!(rubber.friction_with(&ice, 0.0), 0.1);
        assert_eq!(ice.friction_with(&rubber, 2.0), 0.02);
        let bouncy = PhysicsMaterial {
            bounciness: 0.9,
            bounce_combine: Combine::Maximum,
            ..Default::default()
        };
        assert_eq!(bouncy.bounciness_with(&rubber), 0.9);
        assert_eq!(rubber.bounciness_with(&ice), 0.0);
    }

    #[test]
    fn file_round_trip_and_defaults() {
        let m = PhysicsMaterial {
            dynamic_friction: 0.1,
            bounce_combine: Combine::Maximum,
            ..Default::default()
        };
        assert_eq!(PhysicsMaterial::from_json(&m.to_json()).unwrap(), m);
        let partial = PhysicsMaterial::from_json(r#"{ "bounciness": 0.5 }"#).unwrap();
        assert_eq!(partial.bounciness, 0.5);
        assert_eq!(partial.static_friction, 0.6);
        assert!(is_physics_material("Assets/Ice.physicmat"));
        assert!(!is_physics_material("Assets/Ice.mat"));
    }
}
