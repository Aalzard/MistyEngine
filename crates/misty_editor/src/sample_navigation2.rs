//! Procedural NavigationDemo2 scene in `SampleProject`: a walled 28 × 20 m yard split by a wall
//! with a narrow gap (Humanoids only) and a wide one. Two NavMesh Surfaces bake one nav mesh per
//! agent type (Humanoid and a wide "Ogre"). Twelve Humanoids cross the yard both ways (crowds
//! avoiding each other with ORCA) and walk round an expensive Mud pool (a NavMesh Modifier with
//! the "Mud" area, cost 6); two Ogres patrol through the wide gap; a jumper takes a NavMesh Link
//! up onto a ledge; a rolling block (a carving Nav Mesh Obstacle moved by `PingPong.lua`) cuts its
//! moving hole into the mesh tile by tile. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_navigation_demo2`; the other tests check the
//! committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::navigation::{
    AgentSettings, AgentType, NavArea, NavMesh, NavMeshAgent, NavMeshLink, NavMeshModifier,
    NavMeshObstacle, NavMeshSurface, NavigationSettings, bake,
};
use misty_engine::scene::{
    BoxCollider, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, ScriptValue, Scripts,
    Transform,
};

const SCENE: &str = "Assets/Scenes/NavigationDemo2.scene";
const CROWD: &str = "Assets/Scripts/Crowd.lua";
const MOVER: &str = "Assets/Scripts/PingPong.lua";
/// Written by NavigationDemo's generator.
const PATROL: &str = "Assets/Scripts/Patrol.lua";
const FLOOR: &str = "Assets/Materials/Checker.mat";
const WALL: &str = "Assets/Materials/Sandstone.mat";
const RAISED: &str = "Assets/Materials/Bumpy Tiles.mat";
const STEEL: &str = "Assets/Materials/Blue Steel.mat";
const RED: &str = "Assets/Materials/Red.mat";
const MUD: &str = "Assets/Materials/Dirt.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";

/// The Ogre agent type's id.
const OGRE: u32 = 1;
/// The Mud area.
const MUD_AREA: u8 = 3;
/// Height of the ledge the jump link reaches.
const LEDGE_TOP: f32 = 1.2;
/// Mud pool centre and size (x/z).
const MUD_CENTER: Vec2 = Vec2::new(-7.0, -2.0);
const MUD_SIZE: Vec2 = Vec2::new(5.0, 6.0);
/// The narrow gap in the dividing wall (z range).
const NARROW: (f32, f32) = (5.2, 6.8);
/// The wide gap (z range).
const WIDE: (f32, f32) = (-4.0, 0.0);

const CROWD_LUA: &str = r#"-- Crowd: walks its NavMeshAgent from where it starts to a goal and back, again and again.
local Crowd = {
    goal = Vector3.new(0, 0, 0),
    wait = 1,
}

function Crowd:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    self.home = self.transform.position
    self.toGoal = true
    self.timer = 0
    self.agent:SetDestination(self.goal)
end

function Crowd:Update()
    if self.agent.pathPending or self.agent.remainingDistance > 0.5 then
        return
    end
    self.timer = self.timer + Time.deltaTime
    if self.timer < self.wait then
        return
    end
    self.timer = 0
    self.toGoal = not self.toGoal
    if self.toGoal then
        self.agent:SetDestination(self.goal)
    else
        self.agent:SetDestination(self.home)
    end
end

return Crowd
"#;

const MOVER_LUA: &str = r#"-- PingPong: slides the object back and forth along an axis, pausing at each end.
local PingPong = {
    axis = Vector3.new(0, 0, 1),
    distance = 12,
    speed = 2,
    pause = 1,
}

function PingPong:Start()
    self.origin = self.transform.position
    self.t = 0
    self.dir = 1
    self.wait = 0
end

function PingPong:Update()
    if self.wait > 0 then
        self.wait = self.wait - Time.deltaTime
        return
    end
    self.t = self.t + self.dir * self.speed * Time.deltaTime / self.distance
    if self.t >= 1 then
        self.t = 1
        self.dir = -1
        self.wait = self.pause
    elseif self.t <= 0 then
        self.t = 0
        self.dir = 1
        self.wait = self.pause
    end
    self.transform.position = self.origin + self.axis * (self.t * self.distance)
end

return PingPong
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

/// Static cube with a collider (walls, floors, ledges).
fn block(s: &mut Scene, name: &str, pos: Vec3, size: Vec3, material: &str) -> Entity {
    let mut t = Transform::from_position(pos);
    t.scale = size;
    let e = shape(s, name, PrimitiveMesh::Cube, t, material);
    s.add(e, BoxCollider::default());
    s.game_object_mut(e).is_static = true;
    e
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

/// A capsule agent standing on the floor at (x, z); `scale` widens it (Ogres).
fn agent(
    s: &mut Scene,
    name: &str,
    at: Vec2,
    material: &str,
    scale: f32,
    a: NavMeshAgent,
) -> Entity {
    let half = a.height * 0.5;
    let mut t = Transform::from_position(Vec3::new(at.x, half, at.y));
    t.scale = Vec3::new(scale, a.height * 0.5, scale);
    let e = shape(s, name, PrimitiveMesh::Capsule, t, material);
    s.add(
        e,
        NavMeshAgent {
            base_offset: half,
            ..a
        },
    );
    e
}

fn settings() -> NavigationSettings {
    let mut n = NavigationSettings {
        agent_types: vec![AgentType {
            id: OGRE,
            name: "Ogre".into(),
            settings: AgentSettings {
                radius: 0.9,
                height: 2.6,
                step_height: 0.4,
                max_slope: 30.0,
            },
        }],
        ..Default::default()
    };
    n.set_area(
        MUD_AREA as usize,
        NavArea {
            name: "Mud".into(),
            cost: 6.0,
        },
    );
    n
}

fn level(s: &mut Scene) {
    block(
        s,
        "Floor",
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(28.0, 1.0, 20.0),
        FLOOR,
    );
    let wall = |s: &mut Scene, name: &str, center: Vec2, size: Vec2| {
        block(
            s,
            name,
            Vec3::new(center.x, 0.75, center.y),
            Vec3::new(size.x, 1.5, size.y),
            WALL,
        );
    };
    wall(s, "Wall North", Vec2::new(0.0, 10.25), Vec2::new(29.0, 0.5));
    wall(
        s,
        "Wall South",
        Vec2::new(0.0, -10.25),
        Vec2::new(29.0, 0.5),
    );
    wall(s, "Wall East", Vec2::new(14.25, 0.0), Vec2::new(0.5, 20.0));
    wall(s, "Wall West", Vec2::new(-14.25, 0.0), Vec2::new(0.5, 20.0));
    // The dividing wall along x = 0: gaps at z NARROW (1.6 m) and WIDE (4 m).
    let segment = |s: &mut Scene, name: &str, z0: f32, z1: f32| {
        wall(
            s,
            name,
            Vec2::new(0.0, (z0 + z1) * 0.5),
            Vec2::new(0.5, z1 - z0),
        );
    };
    segment(s, "Divider South", -10.0, WIDE.0);
    segment(s, "Divider Middle", WIDE.1, NARROW.0);
    segment(s, "Divider North", NARROW.1, 10.0);
    // The mud pool: a thin slab baked with the Mud area.
    let mud = block(
        s,
        "Mud Pool",
        Vec3::new(MUD_CENTER.x, 0.02, MUD_CENTER.y),
        Vec3::new(MUD_SIZE.x, 0.04, MUD_SIZE.y),
        MUD,
    );
    s.add(
        mud,
        NavMeshModifier {
            override_area: true,
            area: MUD_AREA,
            ..Default::default()
        },
    );
    // A ledge in the north-east corner, above the step height: reached by the jump link only.
    block(
        s,
        "Ledge",
        Vec3::new(10.5, LEDGE_TOP * 0.5, 7.5),
        Vec3::new(6.0, LEDGE_TOP, 4.0),
        RAISED,
    );
    let link = s.spawn("Jump Link", None);
    s.set_transform(link, Transform::from_position(Vec3::new(10.5, 0.0, 5.5)));
    s.add(
        link,
        NavMeshLink {
            start: Vec3::new(0.0, 0.0, -1.2),
            end: Vec3::new(0.0, LEDGE_TOP, 1.2),
            width: 1.0,
            ..Default::default()
        },
    );
    // A few crates.
    for (i, p) in [
        Vec2::new(-4.0, 6.0),
        Vec2::new(4.0, -7.5),
        Vec2::new(-10.5, 8.5),
    ]
    .into_iter()
    .enumerate()
    {
        block(
            s,
            &format!("Crate {}", i + 1),
            Vec3::new(p.x, 0.5, p.y),
            Vec3::ONE,
            WOOD,
        );
    }
}

/// The two surfaces' data paths.
fn surface_data(name: &str) -> String {
    NavigationSettings::surface_asset_path_for(SCENE, name)
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("NavigationDemo2");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 22.0, -21.0))
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
    s.navigation = settings();
    level(&mut s);
    for (name, agent_type) in [("Humanoid Surface", 0), ("Ogre Surface", OGRE)] {
        let e = s.spawn(name, None);
        s.add(
            e,
            NavMeshSurface {
                agent_type,
                nav_mesh_data: Some(surface_data(name)),
                ..Default::default()
            },
        );
    }
    // The rolling block: carves as it moves (every 0.3 m), slid along z by PingPong.lua.
    let roller = shape(
        &mut s,
        "Rolling Block",
        PrimitiveMesh::Cube,
        Transform {
            scale: Vec3::new(1.6, 1.6, 1.6),
            ..Transform::from_position(Vec3::new(6.5, 0.8, -6.0))
        },
        STEEL,
    );
    s.add(
        roller,
        NavMeshObstacle {
            carve: true,
            carve_only_stationary: false,
            move_threshold: 0.3,
            ..Default::default()
        },
    );
    s.add(
        roller,
        script(
            MOVER,
            &[
                ("axis", ScriptValue::Vector3([0.0, 0.0, 1.0])),
                ("distance", ScriptValue::Number(12.0)),
                ("speed", ScriptValue::Number(1.5)),
                ("pause", ScriptValue::Number(2.0)),
            ],
        ),
    );
    // Crowds: six each side, each walking to the far side and back.
    for i in 0..6 {
        let z = -7.0 + i as f32 * 2.4;
        let w = agent(
            &mut s,
            &format!("Crowd West {}", i + 1),
            Vec2::new(-12.0, z),
            STEEL,
            0.8,
            NavMeshAgent {
                radius: 0.4,
                speed: 3.0 + (i % 3) as f32 * 0.3,
                priority: 40 + i,
                ..Default::default()
            },
        );
        s.add(
            w,
            script(
                CROWD,
                &[("goal", ScriptValue::Vector3([9.5, 0.0, -z * 0.6]))],
            ),
        );
        let e = agent(
            &mut s,
            &format!("Crowd East {}", i + 1),
            Vec2::new(12.0, z * 0.6 - 2.0),
            RED,
            0.8,
            NavMeshAgent {
                radius: 0.4,
                speed: 3.0 + (i % 2) as f32 * 0.4,
                priority: 46 + i,
                ..Default::default()
            },
        );
        s.add(
            e,
            script(
                CROWD,
                &[("goal", ScriptValue::Vector3([-12.0, 0.0, z + 1.2]))],
            ),
        );
    }
    // Waypoints (empty markers).
    let waypoints = [
        ("Ogre West", Vec3::new(-11.0, 0.0, 7.5)),
        ("Ogre East", Vec3::new(9.0, 0.0, -8.0)),
        ("Jump Ground", Vec3::new(9.0, 0.0, 2.0)),
        ("Jump Top", Vec3::new(11.5, LEDGE_TOP, 8.0)),
    ];
    for (name, p) in waypoints {
        let e = s.spawn(name, None);
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
    for (i, at) in [Vec2::new(-11.0, 4.0), Vec2::new(9.0, -4.0)]
        .into_iter()
        .enumerate()
    {
        let o = agent(
            &mut s,
            &format!("Ogre {}", i + 1),
            at,
            WOOD,
            1.8,
            NavMeshAgent {
                agent_type: OGRE,
                radius: 0.9,
                height: 2.6,
                speed: 2.2,
                priority: 20,
                ..Default::default()
            },
        );
        let list = if i == 0 {
            "Ogre East, Ogre West"
        } else {
            "Ogre West, Ogre East"
        };
        s.add(o, patrol(list));
    }
    let j = agent(
        &mut s,
        "Jumper",
        Vec2::new(8.0, 0.0),
        STEEL,
        0.8,
        NavMeshAgent {
            radius: 0.4,
            speed: 3.0,
            priority: 30,
            ..Default::default()
        },
    );
    s.add(j, patrol("Jump Top, Jump Ground"));
    s
}

/// Each surface's bake: (data path, nav mesh).
fn nav_meshes(scene: &Scene) -> Vec<(String, NavMesh)> {
    scene
        .walk()
        .into_iter()
        .filter_map(|(e, _)| {
            let path = scene
                .world
                .get::<&NavMeshSurface>(e)
                .ok()?
                .nav_mesh_data
                .clone()?;
            Some((path, bake::bake_surface(scene, e)?))
        })
        .collect()
}

fn scripts() -> [(&'static str, &'static str); 2] {
    [(CROWD, CROWD_LUA), (MOVER, MOVER_LUA)]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_navigation_demo2() {
    let root = project();
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
    }
    let scene = demo_scene();
    for (rel, mesh) in nav_meshes(&scene) {
        mesh.save(&root.join(rel)).unwrap();
    }
    scene.save(&root.join(SCENE)).unwrap();
}

#[test]
fn navigation_demo2_matches_its_generator() {
    let root = project();
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}");
    }
    assert!(root.join(PATROL).is_file(), "NavigationDemo's Patrol.lua");
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let generated = demo_scene();
    crate::test_util::assert_json_close(&scene.to_json(), &generated.to_json(), SCENE);
    let meshes = nav_meshes(&generated);
    assert_eq!(meshes.len(), 2);
    for (rel, baked) in meshes {
        let on_disk = NavMesh::load(&root.join(&rel)).unwrap();
        assert_eq!(on_disk.settings, baked.settings, "{rel}");
        assert_eq!(on_disk.poly_count(), baked.poly_count(), "{rel}");
        for ((_, a), (_, b)) in on_disk.polys().zip(baked.polys()) {
            assert_eq!(a.area, b.area);
            assert_eq!(a.links.len(), b.links.len());
            for (p, q) in a.vertices.iter().zip(&b.vertices) {
                assert!(p.distance(*q) < 1e-4, "{rel}: {p} {q}");
            }
        }
    }
}

/// Playing: crowds cross without overlapping and keep out of the mud, Ogres only use the wide
/// gap, the jumper reaches the ledge over the link, and the rolling block's hole moves.
#[test]
fn navigation_demo2_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::ZERO);
    let names = |prefix: &str, s: &Scene| -> Vec<Entity> {
        s.walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| s.name(*e).starts_with(prefix))
            .collect()
    };
    let crowd = names("Crowd", &scene);
    let ogres = vec![
        scene.find_by_name("Ogre 1").unwrap(),
        scene.find_by_name("Ogre 2").unwrap(),
    ];
    let jumper = scene.find_by_name("Jumper").unwrap();
    assert_eq!(crowd.len(), 12);
    assert_eq!(ogres.len(), 2);
    let starts: Vec<Vec3> = crowd.iter().map(|e| scene.world_position(*e)).collect();
    let mut host = ScriptHost::start(&mut scene, physics, root);
    // The rolling block's carving rebuilds on the background pool; on a busy machine they
    // landed frames later, the crowd re-planned at other moments and walked through the mud
    // more often (a flaky 240 > 180). Rebuilt in step, the run is the same every time.
    host.with_navigation_mut(|n| n.sync_rebuilds = true);
    let mut time = Time::default();
    let mut min_gap = f32::MAX;
    let mut jumper_top: f32 = 0.0;
    let mut in_mud = 0;
    let mut farthest = vec![0.0f32; crowd.len()];
    let mut path_in_mud = 0;
    let half = MUD_SIZE * 0.5 - Vec2::splat(1.0);
    for _ in 0..(24 * 60) {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
        let pos: Vec<Vec3> = crowd.iter().map(|e| scene.world_position(*e)).collect();
        for (f, (p, s)) in farthest.iter_mut().zip(pos.iter().zip(&starts)) {
            *f = f.max(p.distance(*s));
        }
        for i in 0..pos.len() {
            let d = Vec2::new(pos[i].x, pos[i].z) - MUD_CENTER;
            if d.x.abs() < half.x && d.y.abs() < half.y {
                in_mud += 1;
            }
            for j in i + 1..pos.len() {
                let gap = Vec2::new(pos[i].x - pos[j].x, pos[i].z - pos[j].z).length() - 0.8;
                min_gap = min_gap.min(gap);
            }
        }
        for o in &ogres {
            let p = scene.world_position(*o);
            if p.x.abs() < 0.4 {
                assert!(
                    p.z > WIDE.0 && p.z < WIDE.1,
                    "Ogres fit only through the wide gap: {p}"
                );
            }
        }
        jumper_top = jumper_top.max(scene.world_position(jumper).y);
        for e in &crowd {
            let Some(path) = host.with_navigation(|n| n.path_of(*e)) else {
                continue;
            };
            for w in path.windows(2).skip(1) {
                for k in 0..=10 {
                    let p = w[0].lerp(w[1], k as f32 / 10.0);
                    let d = Vec2::new(p.x, p.z) - MUD_CENTER;
                    if d.x.abs() < MUD_SIZE.x * 0.5 - 0.6 && d.y.abs() < MUD_SIZE.y * 0.5 - 0.6 {
                        path_in_mud += 1;
                    }
                }
            }
        }
    }
    assert!(min_gap > -0.15, "crowd agents overlapped by {}", -min_gap);
    // Avoidance may jostle an agent a little way in; paths themselves never cross the mud.
    assert!(
        in_mud < 180,
        "crowds walk round the mud: {in_mud} agent-frames inside"
    );
    assert_eq!(path_in_mud, 0, "planned paths keep out of the mud");
    let moved = farthest.iter().filter(|d| **d > 12.0).count();
    assert!(
        moved >= 10,
        "{moved} of 12 crowd agents crossed the yard: {farthest:?}"
    );
    // Base offset 1 m: standing on the 1.2 m ledge puts the pivot above 2.1 m.
    assert!(
        jumper_top > LEDGE_TOP + 0.9,
        "the jumper reached the ledge: {jumper_top}"
    );
    // The rolling block's tile rebuilds run on the background pool (below normal priority) and
    // the loop above plays far faster than real time: on a busy machine they can all still be
    // in flight. Keep playing, at real-time pace, until they come in (a game gets 24 s of wall
    // time for 24 s of play).
    let waited = std::time::Instant::now();
    while host.with_navigation(|n| n.stats.tiles_rebuilt) <= 4 && waited.elapsed().as_secs() < 600 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let (rebuilt, live) = host.with_navigation(|n| (n.stats.tiles_rebuilt, n.meshes().count()));
    assert_eq!(live, 2);
    assert!(
        rebuilt > 4,
        "the rolling block's tiles were rebuilt: {rebuilt}"
    );
}
