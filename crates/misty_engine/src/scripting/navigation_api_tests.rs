use glam::{Vec2, Vec3};

use crate::navigation::NavMeshAgent;
use crate::navigation::tests::floor;
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{Camera, Scene, ScriptRef, Scripts, Transform};
use crate::scripting::{Input, LogKind, ScriptHost};

const SCRIPT: &str = r#"
local T = {}
function T:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    local ok = self.agent:SetDestination(Vector3.new(3, 0, 2))
    local hit = NavMesh.SamplePosition(Vector3.new(1, 3, 1), 5)
    local miss = NavMesh.SamplePosition(Vector3.new(50, 0, 0), 1)
    local path = NavMesh.CalculatePath(Vector3.new(-3, 0, -3), Vector3.new(3, 0, 3))
    Debug.Log(tostring(ok), tostring(self.agent.hasPath), tostring(self.agent.pathPending),
        tostring(self.agent.remainingDistance > 5), hit.position.y, tostring(miss == nil),
        #path, path.status, self.agent.speed)
    self.frames = 0
end
function T:Update()
    self.frames = self.frames + 1
    if self.frames == 240 then
        Debug.Log("arrived", tostring(self.agent.hasPath), self.agent.remainingDistance,
            math.floor(self.transform.position.x + 0.5), self.transform.position.y)
        self.agent:Warp(Vector3.new(-2, 0, -2))
        self.agent.isStopped = true
        self.agent:SetDestination(Vector3.new(2, 0, 2))
    elseif self.frames == 300 then
        local p = self.transform.position
        Debug.Log("stopped", tostring(self.agent.isStopped), p.x, p.z,
            tostring(self.agent.hasPath))
        self.agent:ResetPath()
        Debug.Log(tostring(self.agent.hasPath))
    end
end
return T
"#;

fn temp_root(tag: &str) -> std::path::PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    root
}

fn texts(host: &ScriptHost) -> Vec<String> {
    let logs = host.take_logs();
    assert!(logs.iter().all(|(k, _)| *k != LogKind::Error), "{logs:?}");
    logs.into_iter().map(|(_, t)| t).collect()
}

fn with_script(scene: &mut Scene, e: hecs::Entity) {
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/T.lua".into(),
                ..Default::default()
            }],
        },
    );
}

#[test]
fn scripts_drive_agents_and_query_the_nav_mesh() {
    let root = temp_root("nav_script");
    std::fs::write(root.join("Assets/Scripts/T.lua"), SCRIPT).unwrap();
    let mut scene = Scene::new("T");
    floor(&mut scene, 0.0, 0.0, 10.0, 10.0);
    let rel = "Assets/Scenes/T/NavMesh.navmesh";
    crate::navigation::bake::bake_scene(&scene)
        .save(&root.join(rel))
        .unwrap();
    scene.navigation.nav_mesh = Some(rel.into());
    let e = scene.spawn("Agent", None);
    scene.set_transform(e, Transform::from_position(Vec3::new(-3.0, 1.0, -3.0)));
    scene.add(
        e,
        NavMeshAgent {
            base_offset: 1.0,
            ..Default::default()
        },
    );
    with_script(&mut scene, e);
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = Time::default();
    let mut all = Vec::new();
    for _ in 0..300 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        all.extend(texts(&host));
    }
    assert_eq!(
        all,
        [
            "true true false true 0 true 2 PathComplete 3.5",
            "arrived false 0 3 1",
            "stopped true -2 -2 true",
            "false",
        ],
        "{all:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn camera_rays_from_the_mouse_position() {
    let root = temp_root("nav_camera");
    let src = r#"
local T = {}
function T:Update()
    local m = Input.mousePosition
    local ray = Camera.main:ScreenPointToRay(m)
    local s = Camera.main:WorldToScreenPoint(Vector3.new(0, 0, 0))
    Debug.Log(m.x, m.y, math.floor(ray.direction.y * 100 + 0.5) / 100, tostring(ray.origin.y > 9),
        math.floor(s.x + 0.5), math.floor(s.y + 0.5))
end
return T
"#;
    std::fs::write(root.join("Assets/Scripts/T.lua"), src).unwrap();
    let mut scene = Scene::new("T");
    let cam = scene.spawn("Main Camera", None);
    scene.game_object_mut(cam).tag = "MainCamera".into();
    scene.set_transform(
        cam,
        Transform::from_position(Vec3::new(0.0, 10.0, 0.0)).with_euler(Vec3::new(90.0, 0.0, 0.0)),
    );
    scene.add(cam, Camera::default());
    with_script(&mut scene, cam);
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    let input = Input {
        mouse_position: Some(Vec2::new(100.0, 30.0)),
        screen_size: Vec2::new(200.0, 100.0),
        ..Default::default()
    };
    host.update(&mut scene, &time, input);
    // Pixels from the bottom-left; the ray through (100, 70) tilts a little off straight down.
    let out = texts(&host);
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].starts_with("100 70 -0.9"), "{out:?}");
    assert!(out[0].ends_with("true 100 50"), "{out:?}");
    let _ = std::fs::remove_dir_all(root);
}

const LINK_SCRIPT: &str = r#"
local T = {}
function T:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    self.agent.autoTraverseOffMeshLink = false
    local mud = NavMesh.GetAreaFromName("Mud")
    local blocked, hit = NavMesh.Raycast(Vector3.new(-6, 0, 2), Vector3.new(6, 0, 2))
    local edge = NavMesh.FindClosestEdge(Vector3.new(-5, 0, 2.2))
    local path = NavMesh.CalculatePath(Vector3.new(-6, 0, -2), Vector3.new(6, 0, -1))
    local inMud = NavMesh.SamplePosition(Vector3.new(-5, 0, 0), 0.3)
    local dry = NavMesh.SamplePosition(Vector3.new(-5, 0, 0), 0.3, ~(1 << mud))
    NavMesh.SetAreaCost(mud, 4)
    self.agent:SetAreaCost(mud, 7)
    Debug.Log(mud, NavMesh.GetAreaFromName("Nope"), tostring(blocked),
        tostring(hit.position.x > -2.9 and hit.position.x < -2.1), tostring(edge.distance < 0.5),
        path.status, tostring(#path >= 3), inMud.mask, tostring(dry == nil),
        NavMesh.GetAreaCost(mud), self.agent:GetAreaCost(mud), self.agent.areaMask)
    local ok = self.agent:SetDestination(Vector3.new(6, 0, -1))
    Debug.Log("go", tostring(ok), tostring(self.agent.isOnNavMesh), self.agent.pathStatus)
end
function T:Update()
    if self.agent.isOnOffMeshLink and not self.done then
        local d = self.agent.currentOffMeshLinkData
        Debug.Log("on link", tostring(d.valid), tostring(d.startPos.x < -2 and d.endPos.x > 2))
        self.agent:CompleteOffMeshLink()
        self.done = true
    end
end
function T:OnOffMeshLinkStart(link)
    Debug.Log("start", link.area)
end
function T:OnOffMeshLinkEnd(link)
    Debug.Log("end", tostring(self.agent.isOnOffMeshLink))
end
return T
"#;

#[test]
fn scripts_use_links_areas_and_queries() {
    use crate::navigation::{NavArea, NavMeshLink, NavMeshModifierVolume};
    let root = temp_root("nav_script_links");
    std::fs::write(root.join("Assets/Scripts/T.lua"), LINK_SCRIPT).unwrap();
    let mut scene = Scene::new("T");
    floor(&mut scene, -5.0, 0.0, 6.0, 6.0);
    floor(&mut scene, 5.0, 0.0, 6.0, 6.0);
    scene.navigation.set_area(
        3,
        NavArea {
            name: "Mud".into(),
            cost: 2.0,
        },
    );
    let mud = scene.spawn("Mud", None);
    scene.set_transform(mud, Transform::from_position(Vec3::new(-5.0, 0.0, 0.0)));
    scene.add(
        mud,
        NavMeshModifierVolume {
            size: Vec3::new(2.0, 2.0, 2.0),
            area: 3,
            ..Default::default()
        },
    );
    let link = scene.spawn("Link", None);
    scene.add(
        link,
        NavMeshLink {
            start: Vec3::new(-2.6, 0.0, 0.0),
            end: Vec3::new(2.6, 0.0, 0.0),
            ..Default::default()
        },
    );
    let rel = "Assets/Scenes/T/NavMesh.navmesh";
    crate::navigation::bake::bake_scene(&scene)
        .save(&root.join(rel))
        .unwrap();
    scene.navigation.nav_mesh = Some(rel.into());
    let e = scene.spawn("Agent", None);
    scene.set_transform(e, Transform::from_position(Vec3::new(-6.0, 0.0, -2.0)));
    scene.add(e, NavMeshAgent::default());
    with_script(&mut scene, e);
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = Time::default();
    let mut all = Vec::new();
    for _ in 0..360 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        all.extend(texts(&host));
    }
    assert_eq!(
        all,
        [
            "3 -1 true true true PathComplete true 8 true 4 7 -1",
            "go true true PathComplete",
            "start 2",
            "on link true true",
            "end false",
        ],
        "{all:?}"
    );
    let p = scene.world_position(e);
    assert!(p.distance(Vec3::new(6.0, 0.0, -1.0)) < 0.3, "{p}");
    let _ = std::fs::remove_dir_all(root);
}
