//! Lua Animator API: parameters, triggers, Play / CrossFade and the state info table.

use std::path::PathBuf;

use glam::Vec3;

use crate::animation::AnimationClip;
use crate::animation::controller::*;
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{Animator, Scene, ScriptRef, Scripts};
use crate::scripting::{Input, ScriptHost};

/// A project with `T.lua`, two clips (Idle: y = 0, Up: y = 2) and `A.controller`.
fn project(source: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_animator_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/T.lua"), source).unwrap();
    for (name, y) in [("Idle", 0.0), ("Up", 2.0)] {
        let mut c = AnimationClip::default();
        c.set_key("", crate::animation::Property::Position(1), 0.0, y);
        c.set_key("", crate::animation::Property::Position(1), 1.0, y);
        c.save(&root.join(format!("Assets/{name}.anim"))).unwrap();
    }
    // Wave: the object's x scale is 3. Walk: 2 m forwards per second (linear), for root motion.
    let mut wave = AnimationClip::default();
    wave.set_key("", crate::animation::Property::Scale(0), 0.0, 3.0);
    wave.set_key("", crate::animation::Property::Scale(0), 1.0, 3.0);
    wave.save(&root.join("Assets/Wave.anim")).unwrap();
    let mut walk = AnimationClip::default();
    walk.set_key("", crate::animation::Property::Position(2), 0.0, 0.0);
    walk.set_key("", crate::animation::Property::Position(2), 1.0, 2.0);
    for i in 0..2 {
        walk.curves[0]
            .keys
            .set_tangent(i, crate::animation::TangentMode::Linear);
    }
    walk.save(&root.join("Assets/Walk.anim")).unwrap();
    let state = |n: &str| State {
        name: n.into(),
        motion: Some(format!("Assets/{n}.anim")),
        ..Default::default()
    };
    let ctrl = AnimatorController {
        parameters: ["Jump:Trigger", "On:Bool", "Speed:Float", "Mode:Int"]
            .iter()
            .map(|s| {
                let (n, k) = s.split_once(':').unwrap();
                Parameter {
                    name: n.into(),
                    kind: ParamKind::ALL.into_iter().find(|x| x.label() == k).unwrap(),
                    default: 0.0,
                }
            })
            .collect(),
        base: StateMachine {
            states: vec![state("Idle"), state("Up"), state("Walk")],
            default_state: Some("Idle".into()),
            transitions: vec![Transition {
                has_exit_time: false,
                duration: 0.0,
                conditions: vec![Condition {
                    parameter: "Jump".into(),
                    mode: ConditionMode::If,
                    threshold: 0.0,
                }],
                ..Transition::new(Node::state("Idle"), Node::state("Up"))
            }],
            ..Default::default()
        },
        // An upper layer (weight 0) whose Wave state plays the Up clip on the object's scale.
        layers: vec![Layer {
            name: "Upper".into(),
            weight: 0.0,
            machine: StateMachine {
                states: vec![State {
                    tag: "Greeting".into(),
                    ..state("Wave")
                }],
                default_state: Some("Wave".into()),
                ..Default::default()
            },
            ..Default::default()
        }],
    };
    ctrl.save(&root.join("Assets/A.controller")).unwrap();
    root
}

fn run(source: &str, frames: usize) -> (Scene, Vec<String>) {
    run_with(source, frames, |_, _| {})
}

/// [`run`], letting `setup` change the scene (the Hero object) first.
fn run_with(
    source: &str,
    frames: usize,
    setup: impl FnOnce(&mut Scene, hecs::Entity),
) -> (Scene, Vec<String>) {
    let root = project(source);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Hero", None);
    setup(&mut scene, e);
    scene.add(
        e,
        Animator {
            controller: Some("Assets/A.controller".into()),
            ..Default::default()
        },
    );
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/T.lua".into(),
                ..Default::default()
            }],
        },
    );
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        h.update(&mut scene, &time, Input::default());
    }
    let logs = h.take_logs().into_iter().map(|(_, t)| t).collect();
    (scene, logs)
}

#[test]
fn parameters_round_trip_through_lua() {
    let src = r#"
local T = {}
function T:Start()
    local a = self.gameObject:GetComponent("Animator")
    a:SetFloat("Speed", 1.5)
    a:SetInteger("Mode", 3)
    a:SetBool("On", true)
    a:SetTrigger("Jump")
    a:ResetTrigger("Jump")
    print(a:GetFloat("Speed"), a:GetInteger("Mode"), tostring(a:GetBool("On")), a.speed)
    a.speed = 2
    print(a.speed, tostring(a.enabled), a.gameObject.name)
end
return T
"#;
    let (_, logs) = run(src, 1);
    assert_eq!(logs, ["1.5 3 true 1", "2 true Hero"]);
}

#[test]
fn trigger_switches_state_and_state_info_reports_it() {
    let src = r#"
local T = {}
function T:Start()
    self.a = self.gameObject:GetComponent("Animator")
    self.n = 0
end
function T:Update()
    self.n = self.n + 1
    if self.n == 2 then self.a:SetTrigger("Jump") end
    if self.n == 4 then
        local info = self.a:GetCurrentAnimatorStateInfo(0)
        print(tostring(info:IsName("Up")), tostring(info.IsName("Base Layer.Up")),
            tostring(info:IsName("Idle")), tostring(self.a:IsInTransition(0)))
        print(string.format("%.3f", info.normalizedTime))
    end
end
return T
"#;
    let (scene, logs) = run(src, 4);
    assert_eq!(logs[0], "true true false false");
    // Entered Up on frame 2 at 0, then advanced one frame (1/60 of a 1 s clip).
    assert_eq!(logs[1], "0.017");
    let e = scene.find_by_name("Hero").unwrap();
    assert_eq!(scene.transform(e).position.y, 2.0);
}

#[test]
fn play_cross_fade_and_warnings() {
    let src = r#"
local T = {}
function T:Start()
    local a = self.gameObject:GetComponent("Animator")
    a:Play("Up")
    print(tostring(a:Play("Nope")))
    a:SetFloat("Missing", 1)
    a:CrossFadeInFixedTime("Idle", 1.0)
    print(tostring(a:IsInTransition(0)), tostring(a:GetNextAnimatorStateInfo(0):IsName("Idle")))
    local ok = pcall(function() a:GetCurrentAnimatorStateInfo(2) end)
    print(tostring(ok), tostring(self.gameObject:GetComponent("Animation")))
end
return T
"#;
    let (scene, logs) = run(src, 30);
    assert!(logs.iter().any(|l| l == "false"), "{logs:?}");
    assert!(logs.iter().any(|l| l.contains("'Nope' could not be found")));
    assert!(logs.iter().any(|l| l.contains("'Missing' does not exist")));
    assert!(logs.iter().any(|l| l == "true true"), "{logs:?}");
    assert!(logs.iter().any(|l| l == "false nil"), "{logs:?}");
    // Halfway through a 1 s fade from Up (2) to Idle (0).
    let e = scene.find_by_name("Hero").unwrap();
    let y = scene.transform(e).position.y;
    assert!((y - 1.0).abs() < 0.1, "{y}");
}

#[test]
fn layers_weights_and_state_info_through_lua() {
    let src = r#"
local T = {}
function T:Start()
    local a = self.gameObject:GetComponent("Animator")
    print(a.layerCount, a:GetLayerName(1), a:GetLayerIndex("Upper"), a:GetLayerIndex("Nope"),
        a:GetLayerWeight(1))
    local info = a:GetCurrentAnimatorStateInfo(1)
    print(info.name, info.fullPath, tostring(info:IsName("Upper.Wave")),
        tostring(info:IsTag("Greeting")), tostring(info:IsTag("Other")))
    a:SetLayerWeight(1, 0.5)
    a:SetLayerWeight(0, 0.2)
    print(a:GetLayerWeight(1), a:GetLayerWeight(0))
end
return T
"#;
    let (scene, logs) = run(src, 2);
    assert_eq!(logs[0], "2 Upper 1 -1 0");
    assert_eq!(logs[1], "Wave Upper.Wave true true false");
    assert_eq!(logs[2], "0.5 1", "the base layer stays at weight 1");
    // Half of the Upper layer's x scale of 3 over the rest value 1.
    let e = scene.find_by_name("Hero").unwrap();
    let sx = scene.transform(e).scale.x;
    assert!((sx - 2.0).abs() < 1e-4, "{sx}");
}

#[test]
fn damped_set_float_and_cross_fade_on_a_layer() {
    let src = r#"
local T = {}
function T:Start()
    local a = self.gameObject:GetComponent("Animator")
    a:SetFloat("Speed", 1, 0.1, 0.1)
    print(a:GetFloat("Speed"))
    a:SetFloat("Speed", 1, 0.1, 0.1)
    print(a:GetFloat("Speed"))
    a:SetFloat("Speed", 0)
    print(a:GetFloat("Speed"))
    a:Play("Up", 0)
    a:CrossFadeInFixedTime("Wave", 0.5, 1, 0.25)
    print(tostring(a:IsInTransition(1)), tostring(a:IsInTransition(0)),
        tostring(a:GetNextAnimatorStateInfo(1):IsName("Wave")))
    print(tostring(pcall(function() a:Play("Up", 5) end)))
end
return T
"#;
    let (_, logs) = run(src, 1);
    assert_eq!(logs[..3], ["0.5", "0.75", "0"]);
    assert_eq!(logs[3], "true false true");
    assert_eq!(logs[4], "false", "layer out of range");
}

#[test]
fn root_motion_moves_the_object_along_its_facing() {
    let src = r#"
local T = {}
function T:Start()
    local a = self.gameObject:GetComponent("Animator")
    a.applyRootMotion = true
    a:Play("Walk")
    self.a = a
end
function T:LateUpdate()
    self.v = self.a.velocity
end
return T
"#;
    // Turned to face +x: walking forwards (clip +z) moves it along +x.
    let (scene, logs) = run_with(src, 31, |s, e| {
        s.set_transform(
            e,
            crate::scene::Transform::default().with_euler(Vec3::new(0.0, 90.0, 0.0)),
        );
    });
    assert!(logs.is_empty(), "{logs:?}");
    let e = scene.find_by_name("Hero").unwrap();
    let p = scene.transform(e).position;
    // 30 frames of 1/60 s at 2 m/s (the first frame only poses).
    assert!((p.x - 1.0).abs() < 1e-3, "{p}");
    assert!(p.z.abs() < 1e-3, "the clip's own z is not written: {p}");
}

#[test]
fn on_animator_move_takes_over_root_motion() {
    let src = r#"
local T = {}
function T:Start()
    self.a = self.gameObject:GetComponent("Animator")
    self.a:Play("Walk")
    self.sum = 0
end
function T:OnAnimatorMove()
    self.sum = self.sum + self.a.deltaPosition.z
end
function T:Update()
    if self.sum > 0.49 and not self.done then
        self.done = true
        print(string.format("%.2f", self.sum), tostring(self.a.applyRootMotion))
    end
end
return T
"#;
    let (scene, logs) = run(src, 40);
    assert_eq!(logs, ["0.50 false"]);
    let e = scene.find_by_name("Hero").unwrap();
    assert!(
        scene.transform(e).position.z.abs() < 1e-4,
        "the script did not move it"
    );
}
