//! Lua WheelCollider: `motorTorque`, `brakeTorque`, `steerAngle`, `forwardStiffness` and
//! `sidewaysStiffness` (the friction curves' grip multipliers; settable), `rpm`, `isGrounded`, `radius`, `suspensionDistance`, `GetWorldPose()` (position and Euler angles,
//! there is no Quaternion type) and `GetGroundHit()` (a WheelHit table or nil).

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, LTransform, LVec3, with_world};
use crate::camera::quat_to_euler;
use crate::scene::WheelCollider;

#[derive(Clone, Copy)]
pub struct LWheelCollider(pub Entity);

fn read<R>(lua: &Lua, e: Entity, f: impl FnOnce(&WheelCollider) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&WheelCollider>(e)
            .map(|c| f(&c))
            .map_err(|_| mlua::Error::runtime("object was destroyed"))
    })?
}

fn write(lua: &Lua, e: Entity, f: impl FnOnce(&mut WheelCollider)) -> Result<()> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&mut WheelCollider>(e)
            .map(|mut c| f(&mut c))
            .map_err(|_| mlua::Error::runtime("object was destroyed"))
    })?
}

impl UserData for LWheelCollider {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("motorTorque", |lua, c| read(lua, c.0, |w| w.motor_torque));
        f.add_field_method_set("motorTorque", |lua, c, v: f32| {
            write(lua, c.0, |w| w.motor_torque = v)
        });
        f.add_field_method_get("brakeTorque", |lua, c| read(lua, c.0, |w| w.brake_torque));
        f.add_field_method_set("brakeTorque", |lua, c, v: f32| {
            write(lua, c.0, |w| w.brake_torque = v.max(0.0))
        });
        f.add_field_method_get("steerAngle", |lua, c| read(lua, c.0, |w| w.steer_angle));
        f.add_field_method_set("steerAngle", |lua, c, v: f32| {
            write(lua, c.0, |w| w.steer_angle = v)
        });
        f.add_field_method_get("forwardStiffness", |lua, c| {
            read(lua, c.0, |w| w.forward_friction.stiffness)
        });
        f.add_field_method_set("forwardStiffness", |lua, c, v: f32| {
            write(lua, c.0, |w| w.forward_friction.stiffness = v.max(0.0))
        });
        f.add_field_method_get("sidewaysStiffness", |lua, c| {
            read(lua, c.0, |w| w.sideways_friction.stiffness)
        });
        f.add_field_method_set("sidewaysStiffness", |lua, c, v: f32| {
            write(lua, c.0, |w| w.sideways_friction.stiffness = v.max(0.0))
        });
        f.add_field_method_get("rpm", |lua, c| read(lua, c.0, |w| w.state.rpm));
        f.add_field_method_get("isGrounded", |lua, c| {
            read(lua, c.0, |w| w.state.is_grounded)
        });
        f.add_field_method_get("radius", |lua, c| read(lua, c.0, |w| w.radius));
        f.add_field_method_get("suspensionDistance", |lua, c| {
            read(lua, c.0, |w| w.suspension_distance)
        });
        f.add_field_method_get("enabled", |lua, c| read(lua, c.0, |w| w.enabled));
        f.add_field_method_set("enabled", |lua, c, v: bool| {
            write(lua, c.0, |w| w.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // GetWorldPose(): wheel centre and rotation (Euler degrees), with steering and rolling.
        m.add_method("GetWorldPose", |lua, c, ()| {
            let pose = with_world(lua, |w| crate::physics::wheel_world_pose(&w.scene, c.0))?;
            let (p, r) = pose.ok_or_else(|| mlua::Error::runtime("object was destroyed"))?;
            Ok((LVec3(p), LVec3(quat_to_euler(r))))
        });
        // GetGroundHit(): what the wheel touches, or nil in the air.
        m.add_method("GetGroundHit", |lua, c, ()| {
            let Some(hit) = read(lua, c.0, |w| w.state.hit)? else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.set("point", LVec3(hit.point))?;
            t.set("normal", LVec3(hit.normal))?;
            t.set("force", hit.force)?;
            t.set("forwardDir", LVec3(hit.forward_dir))?;
            t.set("sidewaysDir", LVec3(hit.sideways_dir))?;
            t.set("forwardSlip", hit.forward_slip)?;
            t.set("sidewaysSlip", hit.sideways_slip)?;
            t.set("collider", LGameObject(hit.collider))?;
            t.set("gameObject", LGameObject(hit.collider))?;
            Ok(Value::Table(t))
        });
    }
}

/// `GetComponent("WheelCollider")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "WheelCollider" {
        return Ok(None);
    }
    let has = with_world(lua, |w| w.scene.world.get::<&WheelCollider>(e).is_ok())?;
    if !has {
        return Ok(None);
    }
    Ok(Some(Value::UserData(
        lua.create_userdata(LWheelCollider(e))?,
    )))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use glam::Vec3;

    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{BoxCollider, Rigidbody, Scene, ScriptRef, Scripts, Transform};
    use crate::scripting::{Input, LogKind, ScriptHost};

    use super::WheelCollider;

    const DRIVER: &str = r#"
local Driver = {}
function Driver:Start()
    self.wheel = GameObject.Find("Wheel"):GetComponent("WheelCollider")
end
function Driver:FixedUpdate()
    self.wheel.motorTorque = 300
    self.wheel.steerAngle = 5
    self.wheel.brakeTorque = -1
end
function Driver:Update()
    local pos, euler = self.wheel:GetWorldPose()
    local hit = self.wheel:GetGroundHit()
    Debug.Log(string.format("grounded=%s rpm=%.1f y=%.2f yaw=%.1f hit=%s brake=%g",
        tostring(self.wheel.isGrounded), self.wheel.rpm, pos.y, euler.y,
        tostring(hit ~= nil), self.wheel.brakeTorque))
end
return Driver
"#;

    fn project() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_wheel_script_{n}"));
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        std::fs::write(root.join("Assets/Scripts/Driver.lua"), DRIVER).unwrap();
        root
    }

    #[test]
    fn scripts_drive_wheels_and_read_their_pose() {
        let mut scene = Scene::new("t");
        let ground = scene.spawn("Ground", None);
        let mut t = Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
        t.scale = Vec3::new(50.0, 1.0, 50.0);
        scene.set_transform(ground, t);
        scene.add(ground, BoxCollider::default());
        let cart = scene.spawn("Cart", None);
        scene.set_transform(cart, Transform::from_position(Vec3::new(0.0, 0.5, 0.0)));
        scene.add(
            cart,
            Rigidbody {
                mass: 400.0,
                freeze_rotation: [true; 3],
                ..Default::default()
            },
        );
        let wheel = scene.spawn("Wheel", Some(cart));
        scene.add(
            wheel,
            WheelCollider {
                radius: 0.35,
                ..Default::default()
            },
        );
        scene.add(
            cart,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Driver.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        let mut host = ScriptHost::start(&mut scene, physics, project());
        let mut time = Time::default();
        for _ in 0..30 {
            time.advance(1.0 / 60.0);
            host.update(&mut scene, &time, Input::default());
        }
        let logs = host.take_logs();
        for (kind, text) in &logs {
            assert_ne!(*kind, LogKind::Error, "{text}");
        }
        let last = &logs.last().unwrap().1;
        assert!(last.contains("grounded=true"), "{last}");
        assert!(last.contains("hit=true"), "{last}");
        assert!(last.contains("yaw=5.0"), "{last}");
        assert!(
            last.contains("brake=0"),
            "brake torque can't go negative: {last}"
        );
        assert!(
            !last.contains("rpm=0.0"),
            "the motor spins the wheel: {last}"
        );
        let c = scene.world.get::<&WheelCollider>(wheel).unwrap();
        assert_eq!(c.motor_torque, 300.0);
    }

    const TUNER: &str = r#"
local Tuner = {}
function Tuner:Start()
    self.wheel = GameObject.Find("Wheel"):GetComponent("WheelCollider")
    self.body = self.gameObject:GetComponent("Rigidbody")
    self.wheel.forwardStiffness = 1.6
    self.wheel.sidewaysStiffness = -2
    self.body.mass = 900
    Physics.gravity = Vector3.new(0, -3.5, 0)
    Debug.Log(string.format("fwd=%.1f side=%.1f mass=%.0f g=%.1f", self.wheel.forwardStiffness,
        self.wheel.sidewaysStiffness, self.body.mass, Physics.gravity.y))
end
return Tuner
"#;

    /// Grip, mass and gravity set from a script (the Racing template's settings panel).
    #[test]
    fn scripts_tune_grip_mass_and_gravity() {
        let root = project();
        std::fs::write(root.join("Assets/Scripts/Tuner.lua"), TUNER).unwrap();
        let mut scene = Scene::new("t");
        let cart = scene.spawn("Cart", None);
        scene.add(
            cart,
            Rigidbody {
                mass: 400.0,
                ..Default::default()
            },
        );
        let wheel = scene.spawn("Wheel", Some(cart));
        scene.add(wheel, WheelCollider::default());
        scene.add(
            cart,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Tuner.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        let mut host = ScriptHost::start(&mut scene, physics, root);
        let mut time = Time::default();
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        for (kind, text) in &logs {
            assert_ne!(*kind, LogKind::Error, "{text}");
        }
        assert!(
            logs.iter()
                .any(|(_, t)| t == "fwd=1.6 side=0.0 mass=900 g=-3.5"),
            "{logs:?}"
        );
        let c = scene.world.get::<&WheelCollider>(wheel).unwrap();
        assert_eq!(c.forward_friction.stiffness, 1.6);
        assert_eq!(
            scene.world.get::<&Rigidbody>(cart).unwrap().mass,
            900.0,
            "the component follows"
        );
    }
}
