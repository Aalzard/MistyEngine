//! Lua: `OnJointBreak(breakForce)` and the `Ragdoll` table.

use std::path::PathBuf;

use glam::Vec3;
use hecs::Entity;

use crate::physics::Physics;
use crate::physics::ragdoll::{RagdollOptions, auto_fill, create};
use crate::runtime::Time;
use crate::scene::{BoxCollider, FixedJoint, Rigidbody, Scene, ScriptRef, Scripts, Transform};
use crate::scripting::{Input, LogKind, ScriptHost};

fn project(files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_joint_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    for (name, source) in files {
        std::fs::write(root.join("Assets/Scripts").join(name), source).unwrap();
    }
    root
}

fn script(scene: &mut Scene, e: Entity, file: &str) {
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{file}"),
                ..Default::default()
            }],
        },
    );
}

fn run(scene: &mut Scene, root: PathBuf, frames: u32, input: impl Fn(u32) -> Input) -> Vec<String> {
    let physics = Physics::build(scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let mut host = ScriptHost::start(scene, physics, root);
    let mut time = Time::default();
    for f in 0..frames {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, input(f));
    }
    let logs = host.take_logs();
    for (kind, text) in &logs {
        assert_ne!(*kind, LogKind::Error, "{text}");
    }
    logs.into_iter().map(|(_, t)| t).collect()
}

const BREAKABLE: &str = r#"
local Breakable = { breaks = 0 }
function Breakable:OnJointBreak(force)
    self.breaks = self.breaks + 1
    Debug.Log("broke", force > 30, self.breaks)
end
return Breakable
"#;

#[test]
fn on_joint_break_runs_and_the_joint_is_removed() {
    let root = project(&[("Breakable.lua", BREAKABLE)]);
    let mut scene = Scene::new("t");
    let weight = scene.spawn("Weight", None);
    scene.set_transform(weight, Transform::from_position(Vec3::new(0.0, -1.0, 0.0)));
    scene.add(weight, BoxCollider::default());
    scene.add(
        weight,
        Rigidbody {
            mass: 5.0,
            ..Default::default()
        },
    );
    // 5 kg pulls with 49 N: a 30 N joint gives way.
    scene.add(
        weight,
        FixedJoint {
            break_force: 30.0,
            ..Default::default()
        },
    );
    script(&mut scene, weight, "Breakable.lua");
    let logs = run(&mut scene, root, 60, |_| Input::default());
    assert_eq!(
        logs.iter().filter(|l| l.starts_with("broke")).count(),
        1,
        "{logs:?}"
    );
    assert!(logs.contains(&"broke true 1".to_owned()), "{logs:?}");
    assert!(scene.world.get::<&FixedJoint>(weight).is_err(), "removed");
    assert!(
        !scene
            .component_names(weight)
            .iter()
            .any(|n| n == "Fixed Joint"),
        "gone from the Inspector list too"
    );
    assert!(scene.world_position(weight).y < -1.5, "fell");
}

const KNOCKOUT: &str = r#"
local Knockout = {}
function Knockout:Start()
    Debug.Log("start", Ragdoll.IsEnabled(self.gameObject))
end
function Knockout:Update()
    if Input.GetKeyDown("k") then
        Ragdoll.Enable(self.gameObject)
        Debug.Log("on", Ragdoll.IsEnabled(self.gameObject))
    end
    if Input.GetKeyDown("r") then
        Ragdoll.Disable(self.gameObject, 0.25)
        Debug.Log("off", Ragdoll.IsEnabled(self.gameObject))
    end
end
return Knockout
"#;

#[test]
fn ragdoll_enable_and_disable_from_lua() {
    let root = project(&[("Knockout.lua", KNOCKOUT)]);
    let mut scene = Scene::new("t");
    let floor = scene.spawn("Floor", None);
    let mut t = Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
    t.scale = Vec3::new(20.0, 1.0, 20.0);
    scene.set_transform(floor, t);
    scene.add(floor, BoxCollider::default());
    let character = crate::physics::ragdoll::test_skeleton(&mut scene);
    let bones = auto_fill(&scene, character);
    create(&mut scene, character, &bones, &RagdollOptions::default()).unwrap();
    script(&mut scene, character, "Knockout.lua");
    let head = scene.find_by_name("Head").unwrap();
    let key = |f: u32| {
        let mut i = Input::default();
        if f == 30 {
            i.pressed.insert("k".into());
        }
        if f == 200 {
            i.pressed.insert("r".into());
        }
        i
    };
    let logs = run(&mut scene, root.clone(), 180, key);
    assert!(logs.contains(&"start false".to_owned()), "{logs:?}");
    assert!(logs.contains(&"on true".to_owned()), "{logs:?}");
    assert!(
        scene.world_position(head).y < 1.2,
        "collapsed: {}",
        scene.world_position(head)
    );
    let logs = run(&mut scene, root, 240, key);
    assert!(logs.contains(&"off false".to_owned()), "{logs:?}");
}

/// `Ragdoll.Disable` after one of the ragdoll's bones was destroyed: no panic, the other
/// bones blend back.
#[test]
fn ragdoll_disable_after_a_bone_was_destroyed() {
    let src = r#"
local K = { frames = 0 }
function K:Update()
    self.frames = self.frames + 1
    if self.frames == 2 then
        Ragdoll.Enable(self.gameObject)
    elseif self.frames == 10 then
        Destroy(GameObject.Find("LeftLowerArm"))
    elseif self.frames == 14 then
        Debug.Log("off", Ragdoll.Disable(self.gameObject, 0.25), Ragdoll.IsEnabled(self.gameObject))
    end
end
return K
"#;
    let root = project(&[("K.lua", src)]);
    let mut scene = Scene::new("t");
    let character = crate::physics::ragdoll::test_skeleton(&mut scene);
    let bones = auto_fill(&scene, character);
    create(&mut scene, character, &bones, &RagdollOptions::default()).unwrap();
    script(&mut scene, character, "K.lua");
    let logs = run(&mut scene, root, 40, |_| Input::default());
    assert!(logs.contains(&"off true false".to_owned()), "{logs:?}");
    assert!(scene.find_by_name("LeftLowerArm").is_none());
}
