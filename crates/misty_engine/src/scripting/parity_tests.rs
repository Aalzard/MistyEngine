//! 2D contact points (`Collision2D.contacts` in C# and Lua) and the Animator IK pass
//! (`OnAnimatorIK` in C# and Lua), through the real script host. The C# tests are skipped (with a
//! note) when no .NET SDK is installed.

use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};

use super::*;
use crate::animation::controller::{AnimatorController, Layer, State, StateMachine};
use crate::physics::Physics;
use crate::scene::{Animator, BoxCollider2D, Rigidbody2D, ScriptRef, Transform};

fn project(files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_parity_test_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    for (name, src) in files {
        std::fs::write(root.join("Assets/Scripts").join(name), src).unwrap();
    }
    root
}

fn attach(scene: &mut Scene, e: Entity, file: &str) {
    let mut list = scene
        .world
        .get::<&Scripts>(e)
        .map(|s| s.list.clone())
        .unwrap_or_default();
    list.push(ScriptRef {
        path: format!("Assets/Scripts/{file}"),
        ..Default::default()
    });
    scene.add(e, Scripts { list });
}

fn no_dotnet() -> bool {
    if csharp::dotnet_available() {
        return false;
    }
    eprintln!("no .NET runtime installed: C# test skipped");
    true
}

/// Run `frames` frames; every log line, failing on errors.
fn run(scene: &mut Scene, root: &Path, gravity: Vec3, frames: usize) -> Vec<String> {
    let physics = Physics::build(scene, gravity);
    let mut host = ScriptHost::start(scene, physics, root.to_path_buf());
    let mut time = Time::default();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, Input::default());
    }
    host.shutdown(scene);
    let logs = host.take_logs();
    let errors: Vec<_> = logs.iter().filter(|(k, _)| *k == LogKind::Error).collect();
    assert!(errors.is_empty(), "errors: {errors:#?}");
    logs.into_iter().map(|(_, t)| t).collect()
}

// ---- 2D contacts ---------------------------------------------------------------------------

/// A 1×1 box ("Player", script attached) dropping onto a wide static ground whose top is at
/// y = -2.5.
fn drop_scene(script: &str) -> Scene {
    let mut scene = Scene::new("t");
    let ground = scene.spawn("Ground", None);
    scene.set_transform(ground, Transform::from_position(Vec3::new(0.0, -3.0, 0.0)));
    scene.add(
        ground,
        BoxCollider2D {
            size: Vec2::new(40.0, 1.0),
            ..Default::default()
        },
    );
    let player = scene.spawn("Player", None);
    scene.add(player, Rigidbody2D::default());
    scene.add(player, BoxCollider2D::default());
    attach(&mut scene, player, script);
    scene
}

const CS_CONTACTS: &str = r#"
using System.Collections.Generic;
using Misty;
public class Lander : MonoBehaviour
{
    bool entered, stayed;
    List<ContactPoint2D> list = new List<ContactPoint2D>();
    void OnCollisionEnter2D(Collision2D c)
    {
        if (entered) return;
        entered = true;
        var p = c.GetContact(0);
        Debug.Log("enter " + c.contactCount + " " + (c.contactCount == c.contacts.Length)
            + " " + (c.GetContacts(list) == c.contactCount) + " " + (list.Count == c.contactCount)
            + " normal " + (p.normal.y > 0.9f) + " point " + (Mathf.Abs(p.point.y + 2.5f) < 0.1f)
            + " sep " + (p.separation < 0.05f) + " rel " + (c.relativeVelocity.y > 1f)
            + " " + (p.relativeVelocity.y == c.relativeVelocity.y)
            + " col " + p.collider.name + " other " + p.otherCollider.name + " " + c.otherCollider.name
            + " rb " + (p.rigidbody == null) + " " + (p.otherRigidbody != null) + " " + (c.otherRigidbody != null));
        try { c.GetContact(c.contactCount); } catch (System.IndexOutOfRangeException) { Debug.Log("out of range"); }
    }
    void OnCollisionStay2D(Collision2D c)
    {
        if (stayed || Time.frameCount < 60) return;
        stayed = true;
        float impulse = 0;
        for (int i = 0; i < c.contactCount; i++) impulse += c.GetContact(i).normalImpulse;
        // Resting: about m·g·dt over the step, spread over the touching points.
        Debug.Log("stay " + c.contactCount + " impulse " + (impulse > 0.005f && impulse < 1f));
    }
}
"#;

#[test]
fn csharp_collision2d_has_the_contact_points_of_the_step() {
    if no_dotnet() {
        return;
    }
    let root = project(&[("Lander.cs", CS_CONTACTS)]);
    let mut scene = drop_scene("Lander.cs");
    let logs = run(&mut scene, &root, Vec3::new(0.0, -9.81, 0.0), 90);
    let enter = logs
        .iter()
        .find(|l| l.starts_with("enter"))
        .expect("entered");
    assert!(
        enter.ends_with(
            "True True True normal True point True sep True rel True True col Ground other Player Player rb True True True"
        ),
        "{enter}"
    );
    let count: usize = enter.split(' ').nth(1).unwrap().parse().unwrap();
    assert!((1..=2).contains(&count), "{enter}");
    assert!(logs.iter().any(|l| l == "out of range"), "{logs:?}");
    assert!(logs.iter().any(|l| l == "stay 2 impulse True"), "{logs:?}");
}

const LUA_CONTACTS: &str = r#"
local Lander = {}
function Lander:OnCollisionEnter2D(c)
    if self.entered then return end
    self.entered = true
    local p = c:GetContact(0)
    local list = {}
    Debug.Log("enter", c.contactCount, #c.contacts == c.contactCount,
        c:GetContacts(list) == c.contactCount, p.normal.y > 0.9, math.abs(p.point.y + 2.5) < 0.1,
        p.separation < 0.05, p.collider.name, p.otherCollider.name, c.otherCollider.name,
        c.otherRigidbody ~= nil, p.relativeVelocity.y > 1)
end
function Lander:OnCollisionStay2D(c)
    if self.stayed or Time.frameCount < 60 then return end
    self.stayed = true
    local impulse = 0
    for i = 1, c.contactCount do impulse = impulse + c.contacts[i].normalImpulse end
    Debug.Log("stay", c.contactCount, impulse > 0.005 and impulse < 1)
end
return Lander
"#;

#[test]
fn lua_collision2d_lists_every_contact_point() {
    let root = project(&[("Lander.lua", LUA_CONTACTS)]);
    let mut scene = drop_scene("Lander.lua");
    let logs = run(&mut scene, &root, Vec3::new(0.0, -9.81, 0.0), 90);
    let enter = logs
        .iter()
        .find(|l| l.starts_with("enter"))
        .expect("entered");
    assert!(
        enter.ends_with("true true true true true Ground Player Player true true"),
        "{enter}"
    );
    assert!(logs.iter().any(|l| l == "stay 2 true"), "{logs:?}");
}

// ---- OnAnimatorIK --------------------------------------------------------------------------

fn add(scene: &mut Scene, parent: Entity, name: &str, pos: Vec3) -> Entity {
    let e = scene.spawn(name, Some(parent));
    scene.set_transform(e, Transform::from_position(pos));
    e
}

/// "Hero" (Animator on `A.controller`, the script) over a small humanoid rig found by bone
/// names; layer 1 "Arms" has IK Pass on when `ik` (the base layer always does).
fn ik_scene(root: &Path, script: &str, ik: bool) -> (Scene, Entity) {
    let empty = |name: &str| StateMachine {
        states: vec![State {
            name: name.into(),
            ..Default::default()
        }],
        default_state: Some(name.into()),
        ..Default::default()
    };
    let ctrl = AnimatorController {
        base: empty("Idle"),
        base_ik_pass: ik,
        layers: vec![Layer {
            name: "Arms".into(),
            ik_pass: ik,
            machine: empty("Hold"),
            ..Default::default()
        }],
        ..Default::default()
    };
    ctrl.save(&root.join("Assets/A.controller")).unwrap();
    let mut scene = Scene::new("t");
    let hero = scene.spawn("Hero", None);
    let hips = add(&mut scene, hero, "Hips", Vec3::new(0.0, 1.0, 0.0));
    let spine = add(&mut scene, hips, "Spine", Vec3::new(0.0, 0.2, 0.0));
    let chest = add(&mut scene, spine, "Chest", Vec3::new(0.0, 0.2, 0.0));
    let neck = add(&mut scene, chest, "Neck", Vec3::new(0.0, 0.2, 0.0));
    add(&mut scene, neck, "Head", Vec3::new(0.0, 0.1, 0.0));
    for (side, x) in [("Left", -1.0f32), ("Right", 1.0)] {
        let arm = add(
            &mut scene,
            chest,
            &format!("{side}Arm"),
            Vec3::new(0.2 * x, 0.15, 0.0),
        );
        let fore = add(
            &mut scene,
            arm,
            &format!("{side}ForeArm"),
            Vec3::new(0.3 * x, 0.0, -0.02),
        );
        add(
            &mut scene,
            fore,
            &format!("{side}Hand"),
            Vec3::new(0.3 * x, 0.0, 0.02),
        );
        let up = add(
            &mut scene,
            hips,
            &format!("{side}UpLeg"),
            Vec3::new(0.1 * x, 0.0, 0.0),
        );
        let leg = add(
            &mut scene,
            up,
            &format!("{side}Leg"),
            Vec3::new(0.0, -0.45, 0.02),
        );
        add(
            &mut scene,
            leg,
            &format!("{side}Foot"),
            Vec3::new(0.0, -0.45, -0.02),
        );
    }
    scene.add(
        hero,
        Animator {
            controller: Some("Assets/A.controller".into()),
            ..Default::default()
        },
    );
    attach(&mut scene, hero, script);
    (scene, hero)
}

const TARGET: Vec3 = Vec3::new(-0.4, 1.2, 0.3);

fn hand(scene: &Scene) -> Vec3 {
    scene.world_position(scene.find_by_name("LeftHand").unwrap())
}

const CS_REACH: &str = r#"
using Misty;
public class Reach : MonoBehaviour
{
    Animator anim;
    int calls;
    void Start() { anim = GetComponent<Animator>(); }
    void OnAnimatorIK(int layerIndex)
    {
        if (calls++ < 2)
            Debug.Log("ik " + layerIndex + " weight " + anim.GetIKPositionWeight(AvatarIKGoal.LeftHand)
                + " bone " + anim.GetBoneTransform(HumanBodyBones.LeftHand).name
                + " animated " + (Vector3.Distance(anim.GetIKPosition(AvatarIKGoal.LeftHand), new Vector3(-0.8f, 1.55f, 0f)) < 0.001f));
        if (layerIndex != 1) return;
        anim.SetIKPosition(AvatarIKGoal.LeftHand, new Vector3(-0.4f, 1.2f, 0.3f));
        anim.SetIKPositionWeight(AvatarIKGoal.LeftHand, 1f);
        anim.SetIKHintPosition(AvatarIKHint.LeftElbow, new Vector3(-0.6f, 1.2f, -1f));
        anim.SetIKHintPositionWeight(AvatarIKHint.LeftElbow, 1f);
        anim.SetLookAtPosition(new Vector3(1f, 1.65f, 1f));
        anim.SetLookAtWeight(1f, 0f, 1f, 0f, 0f);
        if (calls == 2)
            Debug.Log("set " + (anim.GetIKPosition(AvatarIKGoal.LeftHand) == new Vector3(-0.4f, 1.2f, 0.3f))
                + " " + anim.GetIKPositionWeight(AvatarIKGoal.LeftHand) + " " + anim.GetIKHintPositionWeight(AvatarIKHint.LeftElbow));
    }
}
"#;

#[test]
fn csharp_on_animator_ik_moves_the_hand_to_its_goal() {
    if no_dotnet() {
        return;
    }
    let root = project(&[("Reach.cs", CS_REACH)]);
    let (mut scene, _) = ik_scene(&root, "Reach.cs", true);
    let logs = run(&mut scene, &root, Vec3::ZERO, 5);
    assert_eq!(
        logs,
        [
            "ik 0 weight 0 bone LeftHand animated True",
            "ik 1 weight 0 bone LeftHand animated True",
            "set True 1 1",
        ]
    );
    assert!(
        hand(&scene).distance(TARGET) < 1e-3,
        "hand at {}",
        hand(&scene)
    );
    let elbow = scene.world_position(scene.find_by_name("LeftForeArm").unwrap());
    // The elbow sits on the hint's side of the shoulder → hand line.
    let shoulder = scene.world_position(scene.find_by_name("LeftArm").unwrap());
    let n = (TARGET - shoulder).normalize();
    let side = |p: Vec3| {
        let v = p - shoulder;
        (v - n * v.dot(n)).normalize()
    };
    let angle = side(elbow).angle_between(side(Vec3::new(-0.6, 1.2, -1.0)));
    assert!(angle < 1e-2, "elbow towards the hint: {elbow} ({angle} rad off)");
    let head = scene.find_by_name("Head").unwrap();
    let fwd = scene.world_rotation(head) * Vec3::Z;
    let want = (Vec3::new(1.0, 1.65, 1.0) - scene.world_position(head)).normalize();
    assert!(fwd.angle_between(want) < 1e-2, "head looks at the point");
}

#[test]
fn csharp_on_animator_ik_needs_an_ik_pass_layer() {
    if no_dotnet() {
        return;
    }
    let root = project(&[("Reach.cs", CS_REACH)]);
    let (mut scene, _) = ik_scene(&root, "Reach.cs", false);
    let logs = run(&mut scene, &root, Vec3::ZERO, 5);
    assert!(logs.is_empty(), "{logs:?}");
    assert!(hand(&scene).distance(Vec3::new(-0.8, 1.55, 0.0)) < 1e-4);
}

const LUA_REACH: &str = r#"
local Reach = {}
function Reach:Start() self.anim = self.gameObject:GetComponent("Animator") end
function Reach:OnAnimatorIK(layer)
    self.calls = (self.calls or 0) + 1
    if self.calls <= 2 then
        Debug.Log("ik", layer, self.anim:GetBoneTransform(HumanBodyBones.LeftHand).gameObject.name)
    end
    self.anim:SetIKPosition(AvatarIKGoal.LeftHand, Vector3.new(-0.4, 1.2, 0.3))
    self.anim:SetIKPositionWeight("LeftHand", 1)
    self.anim:SetIKRotation(AvatarIKGoal.RightHand, Vector3.new(0, 40, 0))
    self.anim:SetIKRotationWeight(AvatarIKGoal.RightHand, 1)
end
return Reach
"#;

#[test]
fn lua_on_animator_ik_moves_the_hand_to_its_goal() {
    let root = project(&[("Reach.lua", LUA_REACH)]);
    let (mut scene, _) = ik_scene(&root, "Reach.lua", true);
    let logs = run(&mut scene, &root, Vec3::ZERO, 4);
    assert_eq!(logs, ["ik 0 LeftHand", "ik 1 LeftHand"]);
    assert!(
        hand(&scene).distance(TARGET) < 1e-3,
        "hand at {}",
        hand(&scene)
    );
    let right = scene.find_by_name("RightHand").unwrap();
    let want = crate::camera::euler_to_quat(Vec3::new(0.0, 40.0, 0.0));
    assert!(scene.world_rotation(right).angle_between(want) < 1e-3);
}
