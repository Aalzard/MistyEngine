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
    assert!(
        angle < 1e-2,
        "elbow towards the hint: {elbow} ({angle} rad off)"
    );
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

const LUA_TWO_LAYERS: &str = r#"
local Reach = {}
function Reach:Start() self.anim = self.gameObject:GetComponent("Animator") end
function Reach:OnAnimatorIK(layer)
    self.calls = (self.calls or 0) + 1
    if self.calls <= 2 then
        Debug.Log("ik", layer, self.anim:GetIKPositionWeight(AvatarIKGoal.LeftHand))
    end
    if layer == 0 then
        self.anim:SetIKPosition(AvatarIKGoal.LeftHand, Vector3.new(-0.5, 1.8, -0.3))
    else
        self.anim:SetIKPosition(AvatarIKGoal.LeftHand, Vector3.new(-0.4, 1.2, 0.3))
    end
    self.anim:SetIKPositionWeight(AvatarIKGoal.LeftHand, 1)
end
return Reach
"#;

#[test]
fn lua_on_animator_ik_runs_per_layer_with_fresh_goals() {
    let root = project(&[("Reach.lua", LUA_TWO_LAYERS)]);
    let (mut scene, _) = ik_scene(&root, "Reach.lua", true);
    let logs = run(&mut scene, &root, Vec3::ZERO, 4);
    // Goals start cleared for each layer's call; the upper layer's goal wins.
    assert_eq!(logs, ["ik 0 0", "ik 1 0"]);
    assert!(
        hand(&scene).distance(TARGET) < 1e-3,
        "hand at {}",
        hand(&scene)
    );
}

// ---- 3D contacts ---------------------------------------------------------------------------

/// A 1×1×1 cube ("Player", script attached) dropping onto a wide static box whose top is at
/// y = -2.5.
fn drop_scene_3d(script: &str) -> Scene {
    use crate::scene::{BoxCollider, Rigidbody};
    let mut scene = Scene::new("t");
    let ground = scene.spawn("Ground", None);
    scene.set_transform(ground, Transform::from_position(Vec3::new(0.0, -3.0, 0.0)));
    scene.add(
        ground,
        BoxCollider {
            size: Vec3::new(40.0, 1.0, 40.0),
            ..Default::default()
        },
    );
    let player = scene.spawn("Player", None);
    scene.add(player, Rigidbody::default());
    scene.add(player, BoxCollider::default());
    attach(&mut scene, player, script);
    scene
}

const CS_CONTACTS_3D: &str = r#"
using System.Collections.Generic;
using Misty;
public class Lander3D : MonoBehaviour
{
    bool entered, stayed;
    List<ContactPoint> list = new List<ContactPoint>();
    void OnCollisionEnter(Collision c)
    {
        if (entered) return;
        entered = true;
        var p = c.GetContact(0);
        Debug.Log("enter " + c.contactCount + " " + (c.contactCount == c.contacts.Length)
            + " " + (c.GetContacts(list) == c.contactCount) + " " + (list.Count == c.contactCount)
            + " normal " + (p.normal.y > 0.9f) + " point " + (Mathf.Abs(p.point.y + 2.5f) < 0.1f)
            + " sep " + (p.separation < 0.05f) + " rel " + (c.relativeVelocity.y > 1f)
            + " this " + p.thisCollider.name + " other " + p.otherCollider.name + " " + c.gameObject.name);
        try { c.GetContact(c.contactCount); } catch (System.IndexOutOfRangeException) { Debug.Log("out of range"); }
    }
    void OnCollisionStay(Collision c)
    {
        if (stayed || Time.frameCount < 60) return;
        stayed = true;
        float impulse = 0;
        for (int i = 0; i < c.contactCount; i++) impulse += c.GetContact(i).impulse.y;
        Debug.Log("stay " + (c.contactCount >= 1) + " impulse " + (impulse > 0.005f && impulse < 1f)
            + " total " + (Mathf.Abs(c.impulse.y - impulse) < 1e-4f));
    }
}
"#;

#[test]
fn csharp_collision_has_the_contact_points_of_the_step() {
    if no_dotnet() {
        return;
    }
    let root = project(&[("Lander3D.cs", CS_CONTACTS_3D)]);
    let mut scene = drop_scene_3d("Lander3D.cs");
    let logs = run(&mut scene, &root, Vec3::new(0.0, -9.81, 0.0), 90);
    let enter = logs
        .iter()
        .find(|l| l.starts_with("enter"))
        .expect("entered");
    assert!(
        enter.ends_with(
            "True True True normal True point True sep True rel True this Player other Ground Ground"
        ),
        "{enter}"
    );
    let count: usize = enter.split(' ').nth(1).unwrap().parse().unwrap();
    assert!((1..=16).contains(&count), "{enter}");
    assert!(logs.iter().any(|l| l == "out of range"), "{logs:?}");
    assert!(
        logs.iter()
            .any(|l| l == "stay True impulse True total True"),
        "{logs:?}"
    );
}

// ---- ScreenCapture / Application.persistentDataPath -----------------------------------------

/// Requests of the screenshot queue under `root` (other tests may queue their own).
fn captures_under(root: &Path) -> Vec<crate::screen_capture::Request> {
    let mut mine = Vec::new();
    for r in crate::screen_capture::take() {
        if r.path.starts_with(root) {
            mine.push(r);
        } else {
            crate::screen_capture::request(Path::new("/"), &r.path.to_string_lossy(), 1);
        }
    }
    mine
}

#[test]
fn lua_screen_capture_queues_a_png_under_the_data_folder() {
    let root = project(&[(
        "Shot.lua",
        r#"
local Shot = {}
function Shot:Start()
    ScreenCapture.CaptureScreenshot("shots/first", 2)
    Debug.Log("data " .. Application.persistentDataPath)
end
return Shot
"#,
    )]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Camera", None);
    attach(&mut scene, e, "Shot.lua");
    let logs = run(&mut scene, &root, Vec3::ZERO, 2);
    assert!(
        logs.iter()
            .any(|l| l == &format!("data {}", root.display())),
        "{logs:?}"
    );
    let shots = captures_under(&root);
    assert_eq!(shots.len(), 1, "{shots:?}");
    assert_eq!(shots[0].path, root.join("shots/first.png"));
    assert_eq!(shots[0].super_size, 2);
}

#[test]
fn csharp_screen_capture_and_application_paths() {
    if no_dotnet() {
        return;
    }
    let root = project(&[(
        "Shot.cs",
        r#"
using Misty;
public class Shot : MonoBehaviour
{
    void Start()
    {
        ScreenCapture.CaptureScreenshot("cs_shot.png");
        Debug.Log("data " + Application.persistentDataPath + " editor " + Application.isEditor
            + " define " + Application.HasDefine("NOT_A_DEFINE_123"));
        try { ScreenCapture.CaptureScreenshot(""); } catch (System.ArgumentException) { Debug.Log("empty refused"); }
    }
}
"#,
    )]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Camera", None);
    attach(&mut scene, e, "Shot.cs");
    let logs = run(&mut scene, &root, Vec3::ZERO, 2);
    assert!(
        logs.iter().any(
            |l| l.starts_with(&format!("data {} editor ", root.display()))
                && l.ends_with(" define False")
        ),
        "{logs:?}"
    );
    assert!(logs.iter().any(|l| l == "empty refused"), "{logs:?}");
    let shots = captures_under(&root);
    assert_eq!(shots.len(), 1, "{shots:?}");
    assert_eq!(shots[0].path, root.join("cs_shot.png"));
    assert_eq!(shots[0].super_size, 1);
}

// ---- Box / capsule queries -------------------------------------------------------------------

#[test]
fn csharp_box_and_capsule_queries() {
    use crate::scene::BoxCollider;
    if no_dotnet() {
        return;
    }
    let root = project(&[(
        "Shapes.cs",
        r#"
using Misty;
public class Shapes : MonoBehaviour
{
    void Start()
    {
        RaycastHit h;
        bool hit = Physics.BoxCast(Vector3.zero, new Vector3(0.2f, 0.2f, 0.2f), Vector3.forward, out h);
        Debug.Log("box " + hit + " " + h.collider.name + " " + h.distance.ToString("F2"));
        Debug.Log("check " + Physics.CheckSphere(new Vector3(0, 0, 4.6f), 0.2f)
            + " " + Physics.CheckBox(new Vector3(0, 0, 3.6f), new Vector3(0.1f, 0.1f, 1f), Quaternion.Euler(0, 90, 0))
            + " " + Physics.CheckCapsule(new Vector3(0, 0, 3), new Vector3(0, 0, 4.4f), 0.2f));
        Debug.Log("overlap " + Physics.OverlapBox(new Vector3(0, 0, 7), new Vector3(1, 1, 3)).Length
            + " " + Physics.OverlapCapsule(new Vector3(0, 0, 3), new Vector3(0, 0, 4.4f), 0.2f).Length);
        Physics.CapsuleCast(new Vector3(0, 0, 3), new Vector3(0, 0, 4), 0.2f, Vector3.forward, out h);
        Debug.Log("capsule " + h.distance.ToString("F2"));
        Debug.Log("all " + Physics.SphereCastAll(Vector3.zero, 0.2f, Vector3.forward).Length
            + " " + Physics.BoxCastAll(Vector3.zero, new Vector3(0.2f, 0.2f, 0.2f), Vector3.forward, Quaternion.identity, 6f).Length
            + " " + Physics.CapsuleCastAll(new Vector3(0, 0, 3), new Vector3(0, 0, 4), 0.2f, Vector3.forward).Length);
    }
}
"#,
    )]);
    let mut scene = Scene::new("t");
    for (name, z) in [("A", 5.0), ("B", 9.0)] {
        let e = scene.spawn(name, None);
        scene.set_transform(e, Transform::from_position(Vec3::new(0.0, 0.0, z)));
        scene.add(e, BoxCollider::default());
    }
    let e = scene.spawn("Prober", None);
    attach(&mut scene, e, "Shapes.cs");
    let logs = run(&mut scene, &root, Vec3::ZERO, 1);
    for want in [
        "box True A 4.30",
        "check True False True",
        "overlap 2 1",
        "capsule 0.30",
        "all 2 1 2",
    ] {
        assert!(logs.iter().any(|l| l == want), "{want}: {logs:?}");
    }
}

// ---- OnControllerColliderHit ----------------------------------------------------------------

#[test]
fn csharp_controller_collider_hit() {
    use crate::scene::{BoxCollider, CharacterController};
    if no_dotnet() {
        return;
    }
    let root = project(&[(
        "Walker.cs",
        r#"
using Misty;
public class Walker : MonoBehaviour
{
    CharacterController cc;
    bool reported;
    void Start() { cc = GetComponent<CharacterController>(); }
    void Update() { cc.SimpleMove(new Vector3(2, 0, 0)); }
    void OnControllerColliderHit(ControllerColliderHit hit)
    {
        if (reported || hit.gameObject.name != "Wall") return;
        reported = true;
        Debug.Log("cc hit " + hit.collider.name + " " + (hit.normal.x < -0.5f)
            + " " + (hit.controller.gameObject.name) + " " + (hit.moveDirection.x > 0.5f)
            + " " + (Mathf.Abs(hit.point.x - 12.5f) < 0.2f) + " " + (hit.rigidbody == null));
    }
}
"#,
    )]);
    let mut scene = Scene::new("t");
    for (name, pos, size) in [
        (
            "Ground",
            Vec3::new(0.0, -0.5, 0.0),
            Vec3::new(60.0, 1.0, 60.0),
        ),
        (
            "Wall",
            Vec3::new(13.0, 1.5, -10.0),
            Vec3::new(1.0, 3.0, 2.0),
        ),
    ] {
        let e = scene.spawn(name, None);
        let mut t = Transform::from_position(pos);
        t.scale = size;
        scene.set_transform(e, t);
        scene.add(e, BoxCollider::default());
    }
    let player = scene.spawn("Player", None);
    scene.set_transform(
        player,
        Transform::from_position(Vec3::new(10.0, 1.2, -10.0)),
    );
    scene.add(player, CharacterController::default());
    attach(&mut scene, player, "Walker.cs");
    let logs = run(&mut scene, &root, Vec3::new(0.0, -9.81, 0.0), 120);
    assert!(
        logs.iter()
            .any(|l| l == "cc hit Wall True Player True True True"),
        "{logs:?}"
    );
}
