//! `SetActive` while playing reaches the physics worlds: a switched-off object's colliders and
//! body leave them (3D and 2D), a switched-on object's join them.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use hecs::Entity;

use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    BoxCollider, BoxCollider2D, Rigidbody, Rigidbody2D, Scene, ScriptRef, Scripts, Transform,
};
use crate::scripting::{Input, LogKind, ScriptHost};

const SWITCH: &str = r#"
local Switch = { frames = 0 }
function Switch:Update()
    self.frames = self.frames + 1
    if self.frames == 60 then
        GameObject.Find("Floor3D"):SetActive(false)
        GameObject.Find("Floor2D"):SetActive(false)
        local holder = GameObject.Find("Holder").transform
        holder:Find("Ball3D").gameObject:SetActive(true)
        holder:Find("Ball2D").gameObject:SetActive(true)
    end
end
return Switch
"#;

fn project() -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_set_active_physics_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Switch.lua"), SWITCH).unwrap();
    root
}

fn object(scene: &mut Scene, name: &str, pos: Vec3, parent: Option<Entity>) -> Entity {
    let e = scene.spawn(name, parent);
    scene.set_transform(e, Transform::from_position(pos));
    e
}

/// A floor a script switched off kept holding the box up (its collider stayed in the physics
/// world), and a ball a script switched on hung in the air (it never got a body); in 3D and 2D.
#[test]
fn set_active_while_playing_reaches_3d_and_2d_physics() {
    let root = project();
    let mut scene = Scene::new("t");
    let floor = object(&mut scene, "Floor3D", Vec3::ZERO, None);
    scene.add(floor, BoxCollider::default());
    let box3 = object(&mut scene, "Box3D", Vec3::new(0.0, 1.5, 0.0), None);
    scene.add(box3, BoxCollider::default());
    scene.add(box3, Rigidbody::default());
    let floor2 = object(&mut scene, "Floor2D", Vec3::new(20.0, 0.0, 0.0), None);
    scene.add(floor2, BoxCollider2D::default());
    let box2 = object(&mut scene, "Box2D", Vec3::new(20.0, 1.5, 0.0), None);
    scene.add(box2, BoxCollider2D::default());
    scene.add(box2, Rigidbody2D::default());
    let holder = object(&mut scene, "Holder", Vec3::ZERO, None);
    let ball3 = object(&mut scene, "Ball3D", Vec3::new(40.0, 4.0, 0.0), Some(holder));
    scene.add(ball3, BoxCollider::default());
    scene.add(ball3, Rigidbody::default());
    let ball2 = object(&mut scene, "Ball2D", Vec3::new(60.0, 4.0, 0.0), Some(holder));
    scene.add(ball2, BoxCollider2D::default());
    scene.add(ball2, Rigidbody2D::default());
    for b in [ball3, ball2] {
        scene.game_object_mut(b).active = false;
    }
    let switch = object(&mut scene, "Switch", Vec3::new(0.0, 50.0, 0.0), None);
    scene.add(
        switch,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Switch.lua".into(),
                ..Default::default()
            }],
        },
    );
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = Time::default();
    let mut frame = |scene: &mut Scene, host: &mut ScriptHost, n: u32| {
        for _ in 0..n {
            time.advance(1.0 / 60.0);
            host.update(scene, &time, Input::default());
        }
    };
    frame(&mut scene, &mut host, 59);
    for (e, y) in [(box3, 1.0), (box2, 1.0), (ball3, 4.0), (ball2, 4.0)] {
        let p = scene.world_position(e).y;
        assert!((p - y).abs() < 0.05, "{} at {p} before the switch", scene.name(e));
    }
    frame(&mut scene, &mut host, 60);
    for (text, e, below) in [
        ("the box falls through the switched-off 3D floor", box3, 0.0),
        ("the box falls through the switched-off 2D floor", box2, 0.0),
        ("the switched-on 3D ball falls", ball3, 3.0),
        ("the switched-on 2D ball falls", ball2, 3.0),
    ] {
        let y = scene.world_position(e).y;
        assert!(y < below, "{text}: y = {y}");
    }
    for (kind, text) in host.take_logs() {
        assert_ne!(kind, LogKind::Error, "{text}");
    }
    let _ = std::fs::remove_dir_all(&root);
}
