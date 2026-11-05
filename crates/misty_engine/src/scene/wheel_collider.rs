//! Wheel Collider: a raycast wheel with suspension and slip-based tyre friction, on a child of
//! a Rigidbody (see `physics::vehicles` for how it drives the body).
//!
//! The object's position is the top of the suspension (fully compressed); the wheel centre
//! hangs `suspension_distance` below it at full extension. Like the reference engine the
//! wheel ignores the object's scale and rolls around its local X axis, driving along local Z.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use super::components::entity_bits;
use super::physics_components::JointSpring;

/// Tyre friction as a function of slip: rises to the extremum, falls to the asymptote, then
/// stays flat. Values are multiples of the wheel's load, scaled by `stiffness`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelFrictionCurve {
    pub extremum_slip: f32,
    pub extremum_value: f32,
    pub asymptote_slip: f32,
    pub asymptote_value: f32,
    pub stiffness: f32,
}

impl Default for WheelFrictionCurve {
    fn default() -> Self {
        Self::FORWARD
    }
}

impl WheelFrictionCurve {
    /// Default forward (rolling direction) curve.
    pub const FORWARD: Self = Self {
        extremum_slip: 0.4,
        extremum_value: 1.0,
        asymptote_slip: 0.8,
        asymptote_value: 0.5,
        stiffness: 1.0,
    };
    /// Default sideways curve.
    pub const SIDEWAYS: Self = Self {
        extremum_slip: 0.2,
        extremum_value: 1.0,
        asymptote_slip: 0.5,
        asymptote_value: 0.75,
        stiffness: 1.0,
    };

    /// Friction coefficient at `slip` (sign ignored).
    pub fn evaluate(&self, slip: f32) -> f32 {
        let x = slip.abs();
        let ext = self.extremum_slip.max(1e-4);
        let asym = self.asymptote_slip.max(ext + 1e-4);
        let y = if x <= ext {
            // Steep start, flat at the peak.
            let t = x / ext;
            self.extremum_value * t * (2.0 - t)
        } else if x <= asym {
            let t = (x - ext) / (asym - ext);
            let s = t * t * (3.0 - 2.0 * t);
            self.extremum_value + (self.asymptote_value - self.extremum_value) * s
        } else {
            self.asymptote_value
        };
        y.max(0.0) * self.stiffness.max(0.0)
    }

    /// Highest coefficient the curve reaches.
    pub fn peak(&self) -> f32 {
        self.extremum_value.max(self.asymptote_value).max(0.0) * self.stiffness.max(0.0)
    }
}

/// What the wheel touched during the last physics step (WheelHit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelHit {
    /// Object of the collider under the wheel.
    pub collider: hecs::Entity,
    pub point: Vec3,
    pub normal: Vec3,
    /// Rolling and sideways directions on the ground.
    pub forward_dir: Vec3,
    pub sideways_dir: Vec3,
    /// Suspension force pushing the body up (newtons).
    pub force: f32,
    /// Slip in the rolling direction (negative while accelerating, positive while braking).
    pub forward_slip: f32,
    /// Sideways slip (roughly the slip angle in radians).
    pub sideways_slip: f32,
}

/// Values the physics step writes back each frame while playing (not saved).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelState {
    pub is_grounded: bool,
    pub rpm: f32,
    /// Wheel centre below the object's position, along its down axis.
    pub extension: f32,
    pub hit: Option<WheelHit>,
    /// Rolling angle (radians) for the wheel model.
    pub spin: f32,
    /// Written by a running simulation (otherwise the wheel shows at its rest position).
    pub simulated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelCollider {
    pub enabled: bool,
    /// Wheel mass (kg): sets how quickly torque spins it up.
    pub mass: f32,
    pub radius: f32,
    /// Torque that slows the free-spinning wheel per unit of angular speed.
    pub wheel_damping_rate: f32,
    /// Longest the suspension extends (metres).
    pub suspension_distance: f32,
    /// Forces act this far above the resting wheel's base (0 = at the ground contact).
    pub force_app_point_distance: f32,
    /// Wheel attach point in the object's local space.
    pub center: Vec3,
    /// Spring (N/m), damper (N·s/m) and target position (rest point: 0 = fully compressed,
    /// 1 = fully extended) of the suspension.
    pub suspension_spring: JointSpring,
    pub forward_friction: WheelFrictionCurve,
    pub sideways_friction: WheelFrictionCurve,
    /// Drive torque on the wheel axle (N·m); negative reverses.
    pub motor_torque: f32,
    /// Braking torque (N·m, always slows the wheel).
    pub brake_torque: f32,
    /// Steering angle around the object's up axis (degrees).
    pub steer_angle: f32,
    /// Moved to the wheel's pose every frame while playing (the visible tyre).
    #[serde(with = "entity_bits::option")]
    pub wheel_model: Option<hecs::Entity>,
    #[serde(skip)]
    pub state: WheelState,
}

impl Default for WheelCollider {
    fn default() -> Self {
        Self {
            enabled: true,
            mass: 20.0,
            radius: 0.5,
            wheel_damping_rate: 0.25,
            suspension_distance: 0.3,
            force_app_point_distance: 0.0,
            center: Vec3::ZERO,
            suspension_spring: JointSpring {
                spring: 35000.0,
                damper: 4500.0,
                target_position: 0.5,
            },
            forward_friction: WheelFrictionCurve::FORWARD,
            sideways_friction: WheelFrictionCurve::SIDEWAYS,
            motor_torque: 0.0,
            brake_torque: 0.0,
            steer_angle: 0.0,
            wheel_model: None,
            state: WheelState::default(),
        }
    }
}

impl WheelCollider {
    /// Suspension length and rest extension, clamped to sane values.
    pub fn travel(&self) -> (f32, f32) {
        let dist = self.suspension_distance.max(0.0);
        let rest = self.suspension_spring.target_position.clamp(0.0, 1.0) * dist;
        (dist, rest)
    }

    /// World pose of the wheel (centre and rotation, with steering and rolling) given the
    /// object's rigid world pose and the extension below its attach point.
    pub fn wheel_pose(&self, pos: Vec3, rot: Quat, extension: f32, spin: f32) -> (Vec3, Quat) {
        let attach = pos + rot * self.center;
        let centre = attach - rot * Vec3::Y * extension;
        let steer = Quat::from_rotation_y(self.steer_angle.to_radians());
        (centre, rot * steer * Quat::from_rotation_x(spin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friction_curve_peaks_then_settles() {
        let c = WheelFrictionCurve::FORWARD;
        assert_eq!(c.evaluate(0.0), 0.0);
        assert!((c.evaluate(0.4) - 1.0).abs() < 1e-5);
        assert!(c.evaluate(0.2) > 0.5 && c.evaluate(0.2) < 1.0);
        assert!((c.evaluate(2.0) - 0.5).abs() < 1e-5);
        assert_eq!(c.evaluate(-0.4), c.evaluate(0.4));
        let soft = WheelFrictionCurve {
            stiffness: 0.5,
            ..c
        };
        assert!((soft.evaluate(0.4) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn wheel_pose_steers_and_hangs_below_the_attach_point() {
        let w = WheelCollider {
            steer_angle: 90.0,
            ..Default::default()
        };
        let (p, r) = w.wheel_pose(Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY, 0.2, 0.0);
        assert!((p - Vec3::new(0.0, 0.8, 0.0)).length() < 1e-5);
        assert!((r * Vec3::Z - Vec3::X).length() < 1e-5);
    }
}
