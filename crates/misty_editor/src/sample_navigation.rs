//! Procedural NavigationDemo scene in `SampleProject`: a walled 24 m yard with a small maze, a
//! raised platform reached by a ramp, a stepped terrace and a few crates, all Static and baked
//! into `Scenes/NavigationDemo/NavMesh.navmesh`. Two agents patrol waypoints (`Patrol.lua`), three
//! chase the marker (`Chaser.lua`), and clicking the Game view moves the marker to the clicked
//! point (`ClickMarker.lua`: `Camera.main:ScreenPointToRay`, `Physics.Raycast`,
//! `NavMesh.SamplePosition`). Regenerate with
//! `cargo test -p misty_editor -- --ignored write_navigation_demo`; the other tests check the
//! committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::navigation::{NavMesh, NavMeshAgent, bake};
use misty_engine::scene::{
    BoxCollider, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, ScriptValue, Scripts,
    Transform,
};

const SCENE: &str = "Assets/Scenes/NavigationDemo.scene";
const NAV_MESH: &str = "Assets/Scenes/NavigationDemo/NavMesh.navmesh";
const PATROL: &str = "Assets/Scripts/Patrol.lua";
const CHASER: &str = "Assets/Scripts/Chaser.lua";
const CLICK: &str = "Assets/Scripts/ClickMarker.lua";
const FLOOR: &str = "Assets/Materials/Checker.mat";
const WALL: &str = "Assets/Materials/Sandstone.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const RAISED: &str = "Assets/Materials/Bumpy Tiles.mat";
const STEEL: &str = "Assets/Materials/Blue Steel.mat";
const RED: &str = "Assets/Materials/Red.mat";
const LASER: &str = "Assets/Materials/Laser.mat";

const PATROL_LUA: &str = r#"-- Patrol: walks a NavMeshAgent from waypoint to waypoint (comma-separated object names),
-- waiting a moment at each one.
local Patrol = {
    waypoints = "Waypoint 1, Waypoint 2",
    wait = 0.5,
}

function Patrol:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    self.points = {}
    for name in string.gmatch(self.waypoints, "[^,]+") do
        local go = GameObject.Find(name:match("^%s*(.-)%s*$"))
        if go ~= nil then
            table.insert(self.points, go.transform.position)
        end
    end
    self.index = 1
    self.timer = 0
    self:GoNext()
end

function Patrol:GoNext()
    if #self.points == 0 then
        return
    end
    self.agent:SetDestination(self.points[self.index])
    self.index = self.index % #self.points + 1
end

function Patrol:Update()
    if not self.agent.pathPending and self.agent.remainingDistance < 0.3 then
        self.timer = self.timer + Time.deltaTime
        if self.timer >= self.wait then
            self.timer = 0
            self:GoNext()
        end
    end
end

return Patrol
"#;

const CHASER_LUA: &str = r#"-- Chaser: keeps sending its NavMeshAgent to the target object (re-planned a few times a second).
local Chaser = {
    target = "Click Marker",
    repath = 0.25,
}

function Chaser:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    self.goal = GameObject.Find(self.target)
    self.timer = 0
end

function Chaser:Update()
    self.timer = self.timer - Time.deltaTime
    if self.goal ~= nil and self.timer <= 0 then
        self.timer = self.repath
        self.agent:SetDestination(self.goal.transform.position)
    end
end

return Chaser
"#;

const CLICK_LUA: &str = r#"-- ClickMarker: left-click the Game view to move this marker onto the clicked spot of the
-- nav mesh (a ray from the camera through the mouse, then the nearest nav mesh point).
local ClickMarker = {
    hover = 0.25,
}

function ClickMarker:Update()
    if Input.GetMouseButtonDown(0) then
        local ray = Camera.main:ScreenPointToRay(Input.mousePosition)
        local hit = Physics.Raycast(ray.origin, ray.direction, 200)
        if hit ~= nil then
            local onMesh = NavMesh.SamplePosition(hit.point, 1.5)
            if onMesh ~= nil then
                self.transform.position = onMesh.position + Vector3.new(0, self.hover, 0)
            end
        end
    end
end

return ClickMarker
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn shape(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, material: &str) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

/// Static cube with a collider (walls, floors, ramps).
fn block(s: &mut Scene, name: &str, pos: Vec3, size: Vec3, euler: Vec3, material: &str) {
    let mut t = Transform::from_position(pos).with_euler(euler);
    t.scale = size;
    let e = shape(s, name, PrimitiveMesh::Cube, t, material);
    s.add(e, BoxCollider::default());
    s.game_object_mut(e).is_static = true;
}

fn script(path: &str, fields: &[(&str, ScriptValue)]) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            ..Default::default()
        }],
    }
}

/// A capsule agent standing on the floor at (x, z).
fn agent(s: &mut Scene, name: &str, at: Vec2, material: &str, a: NavMeshAgent) -> Entity {
    let pos = Vec3::new(at.x, 1.0, at.y);
    let e = shape(
        s,
        name,
        PrimitiveMesh::Capsule,
        Transform::from_position(pos),
        material,
    );
    s.add(
        e,
        NavMeshAgent {
            base_offset: 1.0,
            ..a
        },
    );
    e
}

/// Height of the raised platform and the ramp's angle.
const PLATFORM_TOP: f32 = 1.5;
const RAMP_DEG: f32 = 20.0;

fn level(s: &mut Scene) {
    block(
        s,
        "Floor",
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(24.0, 1.0, 24.0),
        Vec3::ZERO,
        FLOOR,
    );
    let wall = |s: &mut Scene, name: &str, pos: Vec2, size: Vec2| {
        let at = Vec3::new(pos.x, 0.75, pos.y);
        block(
            s,
            name,
            at,
            Vec3::new(size.x, 1.5, size.y),
            Vec3::ZERO,
            WALL,
        );
    };
    // Border.
    wall(s, "Wall North", Vec2::new(0.0, 12.25), Vec2::new(25.0, 0.5));
    wall(
        s,
        "Wall South",
        Vec2::new(0.0, -12.25),
        Vec2::new(25.0, 0.5),
    );
    wall(s, "Wall East", Vec2::new(12.25, 0.0), Vec2::new(0.5, 24.0));
    wall(s, "Wall West", Vec2::new(-12.25, 0.0), Vec2::new(0.5, 24.0));
    // Maze.
    wall(
        s,
        "Maze Wall 1",
        Vec2::new(-5.0, -3.0),
        Vec2::new(14.0, 0.5),
    );
    wall(s, "Maze Wall 2", Vec2::new(5.0, 3.0), Vec2::new(14.0, 0.5));
    wall(s, "Maze Wall 3", Vec2::new(-3.0, 7.5), Vec2::new(0.5, 9.0));
    // Raised platform (south-east) and its ramp from the west.
    let top = PLATFORM_TOP;
    block(
        s,
        "Platform",
        Vec3::new(8.0, top * 0.5, -8.0),
        Vec3::new(6.0, top, 6.0),
        Vec3::ZERO,
        RAISED,
    );
    let len = top / RAMP_DEG.to_radians().sin();
    let run = len * RAMP_DEG.to_radians().cos();
    block(
        s,
        "Ramp",
        Vec3::new(5.0 - run * 0.5, top * 0.5 - 0.1, -8.0),
        Vec3::new(len + 0.3, 0.2, 3.0),
        Vec3::new(0.0, 0.0, RAMP_DEG),
        WOOD,
    );
    // Stepped terrace (north-west): two 0.25 m steps up to 0.75 m.
    block(
        s,
        "Terrace",
        Vec3::new(-8.0, 0.375, 9.0),
        Vec3::new(6.0, 0.75, 5.0),
        Vec3::ZERO,
        RAISED,
    );
    block(
        s,
        "Step 1",
        Vec3::new(-8.0, 0.125, 5.375),
        Vec3::new(3.0, 0.25, 0.75),
        Vec3::ZERO,
        WOOD,
    );
    block(
        s,
        "Step 2",
        Vec3::new(-8.0, 0.25, 6.125),
        Vec3::new(3.0, 0.5, 0.75),
        Vec3::ZERO,
        WOOD,
    );
    // Crates.
    for (i, (p, size)) in [
        (Vec2::new(-7.0, -8.0), 1.2),
        (Vec2::new(2.0, 8.5), 1.5),
        (Vec2::new(-1.0, 0.0), 1.0),
        (Vec2::new(6.0, -1.0), 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        let at = Vec3::new(p.x, size * 0.5, p.y);
        let name = format!("Crate {}", i + 1);
        block(
            s,
            &name,
            at,
            Vec3::splat(size),
            Vec3::new(0.0, 15.0 * i as f32, 0.0),
            WOOD,
        );
    }
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("NavigationDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 21.0, -19.0))
                .with_euler(Vec3::new(50.0, 0.0, 0.0)),
        );
    }
    if let Some(light) = s.find_by_name("Directional Light") {
        s.set_transform(
            light,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(55.0, -35.0, 0.0)),
        );
    }
    level(&mut s);
    s.navigation.nav_mesh = Some(NAV_MESH.into());
    // Waypoints: empty markers the patrols walk between.
    let waypoints = [
        Vec3::new(-9.5, 0.0, -9.5),
        Vec3::new(9.5, 0.0, 0.0),
        Vec3::new(-8.0, 0.75, 10.0),
        Vec3::new(9.5, 0.0, 9.5),
        Vec3::new(8.0, PLATFORM_TOP, -8.0),
    ];
    for (i, p) in waypoints.into_iter().enumerate() {
        let e = s.spawn(&format!("Waypoint {}", i + 1), None);
        s.set_transform(e, Transform::from_position(p));
    }
    let patrol = |list: &str| {
        script(
            PATROL,
            &[
                ("waypoints", ScriptValue::Text(list.into())),
                ("wait", ScriptValue::Number(0.5)),
            ],
        )
    };
    let a = agent(
        &mut s,
        "Patrol A",
        Vec2::new(-9.0, -6.0),
        STEEL,
        NavMeshAgent::default(),
    );
    s.add(a, patrol("Waypoint 1, Waypoint 2, Waypoint 4, Waypoint 3"));
    let b = agent(
        &mut s,
        "Patrol B",
        Vec2::new(9.0, 6.0),
        STEEL,
        NavMeshAgent::default(),
    );
    s.add(b, patrol("Waypoint 4, Waypoint 5, Waypoint 1"));
    // Chasers follow the marker (it starts on the raised platform).
    let marker = shape(
        &mut s,
        "Click Marker",
        PrimitiveMesh::Sphere,
        Transform {
            scale: Vec3::splat(0.4),
            ..Transform::from_position(Vec3::new(9.0, PLATFORM_TOP + 0.25, -9.0))
        },
        LASER,
    );
    s.add(
        marker,
        script(CLICK, &[("hover", ScriptValue::Number(0.25))]),
    );
    for (i, x) in [-6.0, -4.0, -2.0].into_iter().enumerate() {
        let c = agent(
            &mut s,
            &format!("Chaser {}", i + 1),
            Vec2::new(x, -10.0),
            RED,
            NavMeshAgent {
                speed: 3.0 + i as f32 * 0.5,
                stopping_distance: 1.0,
                priority: 40 + i as i32 * 10,
                ..Default::default()
            },
        );
        s.add(
            c,
            script(
                CHASER,
                &[
                    ("target", ScriptValue::Text("Click Marker".into())),
                    ("repath", ScriptValue::Number(0.25)),
                ],
            ),
        );
    }
    s
}

fn nav_mesh() -> NavMesh {
    bake::bake_scene(&demo_scene())
}

fn scripts() -> [(&'static str, &'static str); 3] {
    [
        (PATROL, PATROL_LUA),
        (CHASER, CHASER_LUA),
        (CLICK, CLICK_LUA),
    ]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_navigation_demo() {
    let root = project();
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
    }
    nav_mesh().save(&root.join(NAV_MESH)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn navigation_demo_matches_its_generator() {
    let root = project();
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}");
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(&scene.to_json(), &demo_scene().to_json(), SCENE);
    let on_disk = NavMesh::load(&root.join(NAV_MESH)).unwrap();
    let baked = nav_mesh();
    assert_eq!(on_disk.poly_count(), baked.poly_count());
    for ((_, a), (_, b)) in on_disk.polys().zip(baked.polys()) {
        assert_eq!(a.links.len(), b.links.len());
        for (p, q) in a.vertices.iter().zip(&b.vertices) {
            assert!(p.distance(*q) < 1e-4, "{p} {q}");
        }
    }
}

/// Playing: patrols move between waypoints, chasers climb the ramp to the marker, and a click
/// moves the marker (and then the chasers) elsewhere.
#[test]
fn navigation_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, LogKind, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::ZERO);
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let find = |s: &Scene, n: &str| s.find_by_name(n).unwrap();
    let patrol_start = scene.world_position(find(&scene, "Patrol A"));
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut run = |scene: &mut Scene, seconds: f32, input: &Input| {
        for i in 0..(seconds * 60.0) as usize {
            time.advance(1.0 / 60.0);
            let mut frame = input.clone();
            if i > 0 {
                frame.mouse_pressed = [false; 3];
            }
            host.update(scene, &time, frame);
            let logs = host.take_logs();
            assert!(logs.is_empty(), "{logs:?}");
        }
    };
    let screen = Vec2::new(1280.0, 720.0);
    let idle = Input {
        screen_size: screen,
        ..Default::default()
    };
    run(&mut scene, 14.0, &idle);
    for i in 1..=3 {
        let p = scene.world_position(find(&scene, &format!("Chaser {i}")));
        assert!(p.y > PLATFORM_TOP + 0.9, "Chaser {i} climbed the ramp: {p}");
        assert!(
            p.x > 5.0 && p.z < -5.0,
            "Chaser {i} is on the platform: {p}"
        );
    }
    let patrol_now = scene.world_position(find(&scene, "Patrol A"));
    assert!(patrol_now.distance(patrol_start) > 2.0, "the patrol moved");
    // Click the middle of the view: the marker jumps to the floor there.
    let click = Input {
        mouse_position: Some(screen * 0.5),
        mouse_pressed: [true, false, false],
        mouse_held: [true, false, false],
        ..idle.clone()
    };
    run(&mut scene, 1.0 / 60.0, &click);
    let marker = scene.world_position(find(&scene, "Click Marker"));
    assert!(marker.y < 0.5 && marker.x.abs() < 6.0, "{marker}");
    run(&mut scene, 12.0, &idle);
    for i in 1..=3 {
        let p = scene.world_position(find(&scene, &format!("Chaser {i}")));
        assert!(
            p.distance(marker) < 3.0,
            "Chaser {i} followed the click: {p} vs {marker}"
        );
    }
    let errors: Vec<_> = host
        .take_logs()
        .into_iter()
        .filter(|(k, _)| *k == LogKind::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}
