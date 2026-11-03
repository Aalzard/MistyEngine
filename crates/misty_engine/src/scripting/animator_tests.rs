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
        states: vec![state("Idle"), state("Up")],
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
    };
    ctrl.save(&root.join("Assets/A.controller")).unwrap();
    root
}

fn run(source: &str, frames: usize) -> (Scene, Vec<String>) {
    let root = project(source);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Hero", None);
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
    local ok = pcall(function() a:GetCurrentAnimatorStateInfo(1) end)
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
