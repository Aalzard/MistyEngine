//! Procedural BehaviourTreeDemo scene in `SampleProject` (the Behaviour Trees package): a walled
//! 28 m yard with a hiding wall and a few crates, baked into
//! `Scenes/BehaviourTreeDemo/NavMesh.navmesh`. A guard runs `AI/Guard.btree`: it patrols four
//! waypoints (a custom Lua leaf, `AI/NextWaypoint.lua`, picks the next one), chases the player
//! when it sees them (Can See Target with a field of view and line of sight, a Conditional that
//! aborts lower-priority branches), attacks in range (`GuardBrain.lua`'s `Attack`, behind a
//! Cooldown), searches for a moment when the player gets away, and goes back to its patrol. The
//! blackboard key `State` says which of Patrol, Chase, Attack and Search it is in. Move the
//! player with WASD / the arrow keys (`AI/PlayerMove.lua`).
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_behaviour_tree_demo`; the
//! other tests check the committed files still match and that the guard goes through its states.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::behaviour_tree::asset::{AbortMode, LogLevel, NodeKind, ParallelPolicy};
use misty_engine::behaviour_tree::blackboard::{BbValue, CompareOp, KeyDef, KeyType};
use misty_engine::behaviour_tree::component::BlackboardOverride;
use misty_engine::behaviour_tree::{BehaviourTree, BehaviourTreeAgent, BtNode};
use misty_engine::hecs::Entity;
use misty_engine::navigation::{NavMesh, NavMeshAgent, bake};
use misty_engine::scene::{
    BoxCollider, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, ScriptValue, Scripts,
    Transform,
};

const SCENE: &str = "Assets/Scenes/BehaviourTreeDemo.scene";
const NAV_MESH: &str = "Assets/Scenes/BehaviourTreeDemo/NavMesh.navmesh";
const TREE: &str = "Assets/AI/Guard.btree";
const NEXT_WAYPOINT: &str = "Assets/AI/NextWaypoint.lua";
const GUARD_BRAIN: &str = "Assets/AI/GuardBrain.lua";
const PLAYER_MOVE: &str = "Assets/AI/PlayerMove.lua";
const FLOOR: &str = "Assets/Materials/Checker.mat";
const WALL: &str = "Assets/Materials/Sandstone.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const STEEL: &str = "Assets/Materials/Blue Steel.mat";
const RED: &str = "Assets/Materials/Red.mat";

/// Where the player starts: behind the hiding wall, out of the guard's sight.
const HIDDEN: Vec3 = Vec3::new(0.0, 1.0, 12.5);
/// Patrol corners.
const WAYPOINTS: [Vec3; 4] = [
    Vec3::new(-8.0, 0.0, -8.0),
    Vec3::new(8.0, 0.0, -8.0),
    Vec3::new(8.0, 0.0, 4.0),
    Vec3::new(-8.0, 0.0, 4.0),
];
/// Attack range (m).
const ATTACK_RANGE: f32 = 2.2;

const NEXT_WAYPOINT_LUA: &str = r#"-- @behaviour-leaf Next Waypoint
-- @param waypoints Waypoint 1, Waypoint 2, Waypoint 3, Waypoint 4
-- @param key PatrolPoint
-- A custom Behaviour Tree leaf: picks the next waypoint of a comma-separated list of object
-- names (the place it reached is kept in the blackboard's PatrolIndex key) and writes its
-- position to a Vector key, ready for a Move To node.
local NextWaypoint = {}

function NextWaypoint:OnTick(dt)
    local names = {}
    for name in string.gmatch(tostring(self.params.waypoints or ""), "[^,]+") do
        names[#names + 1] = name:match("^%s*(.-)%s*$")
    end
    if #names == 0 then
        return false
    end
    local me = self.gameObject
    local index = (BehaviourTree.GetValue(me, "PatrolIndex") or 0) % #names + 1
    BehaviourTree.SetValue(me, "PatrolIndex", index)
    local waypoint = GameObject.Find(names[index])
    if waypoint == nil then
        return false
    end
    BehaviourTree.SetValue(me, self.params.key or "PatrolPoint", waypoint.transform.position)
    return true
end

return NextWaypoint
"#;

const GUARD_BRAIN_LUA: &str = r#"-- GuardBrain: what the guard's Behaviour Tree calls with Run Lua nodes.
local GuardBrain = {
    damage = 10,
}

function GuardBrain:Awake()
    self.attacks = 0
end

-- Run Lua "Attack": one swing (the tree's Cooldown spaces them out).
function GuardBrain:Attack()
    self.attacks = self.attacks + 1
    Debug.Log(self.gameObject.name .. " attacks the player for " .. self.damage .. " damage")
    return true
end

return GuardBrain
"#;

const PLAYER_MOVE_LUA: &str = r#"-- PlayerMove: walk with WASD or the arrow keys.
local PlayerMove = {
    speed = 5,
}

function PlayerMove:Update()
    local h = Input.GetAxis("Horizontal")
    local v = Input.GetAxis("Vertical")
    if h ~= 0 or v ~= 0 then
        local step = Vector3.new(h, 0, v) * (self.speed * Time.deltaTime)
        self.transform.position = self.transform.position + step
    end
end

return PlayerMove
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Adds nodes to a tree; children are listed in run order.
struct Tree(BehaviourTree);

impl Tree {
    fn add(&mut self, kind: NodeKind, name: &str, children: &[u32]) -> u32 {
        let id = self.0.next_id();
        let mut n = BtNode::new(id, kind, [0.0, 0.0]);
        n.name = name.into();
        n.children = children.to_vec();
        self.0.nodes.push(n);
        id
    }
}

fn set_state(t: &mut Tree, state: &str) -> u32 {
    t.add(
        NodeKind::SetBlackboard {
            key: "State".into(),
            value: BbValue::String(state.into()),
            from_key: String::new(),
        },
        "",
        &[],
    )
}

/// The guard: eyes in parallel with a brain that picks Chase / Attack, Search or Patrol.
pub fn guard_tree() -> BehaviourTree {
    let mut t = Tree(BehaviourTree::default());
    t.0.keys = vec![
        KeyDef {
            description: "The player (set on the guard's agent)".into(),
            ..KeyDef::new("Player", KeyType::Object)
        },
        KeyDef {
            description: "Can See Target writes it".into(),
            ..KeyDef::new("SeesPlayer", KeyType::Bool)
        },
        KeyDef {
            description: "Set while chasing; the guard searches once it loses the player".into(),
            ..KeyDef::new("WasChasing", KeyType::Bool)
        },
        KeyDef::new("PatrolPoint", KeyType::Vector),
        KeyDef::new("PatrolIndex", KeyType::Int),
        KeyDef {
            default: BbValue::String("Patrol".into()),
            description: "Patrol, Chase, Attack or Search".into(),
            ..KeyDef::new("State", KeyType::String)
        },
    ];
    // Eyes: look for the player ten times a second.
    let see = t.add(
        NodeKind::CanSeeTarget {
            target: "Player".into(),
            range: 14.0,
            fov: 120.0,
            eye_height: 0.6,
            store: "SeesPlayer".into(),
        },
        "Look for the player",
        &[],
    );
    let see_ok = t.add(NodeKind::ForceSuccess, "", &[see]);
    let blink = t.add(
        NodeKind::Wait {
            seconds: 0.1,
            random: 0.0,
        },
        "",
        &[],
    );
    let look = t.add(NodeKind::Sequence, "", &[see_ok, blink]);
    let eyes = t.add(
        NodeKind::Repeat {
            count: 0,
            until_fail: false,
        },
        "Eyes",
        &[look],
    );
    // Attack when in range.
    let in_range = t.add(
        NodeKind::IsInRange {
            target: "Player".into(),
            range: ATTACK_RANGE,
        },
        "",
        &[],
    );
    let attack_state = set_state(&mut t, "Attack");
    let face = t.add(
        NodeKind::LookAt {
            target: "Player".into(),
            turn_speed: 540.0,
        },
        "",
        &[],
    );
    let swing = t.add(
        NodeKind::RunLua {
            function: "Attack".into(),
        },
        "Swing",
        &[],
    );
    let cooldown = t.add(NodeKind::Cooldown { seconds: 1.0 }, "", &[swing]);
    let pause = t.add(
        NodeKind::Wait {
            seconds: 0.2,
            random: 0.0,
        },
        "",
        &[],
    );
    let swing_or_wait = t.add(NodeKind::Selector, "", &[cooldown, pause]);
    let attack = t.add(
        NodeKind::Sequence,
        "Attack",
        &[in_range, attack_state, face, swing_or_wait],
    );
    // Chase otherwise.
    let chase_state = set_state(&mut t, "Chase");
    let chasing = t.add(
        NodeKind::SetBlackboard {
            key: "WasChasing".into(),
            value: BbValue::Bool(true),
            from_key: String::new(),
        },
        "",
        &[],
    );
    let run = t.add(
        NodeKind::MoveTo {
            target: "Player".into(),
            acceptance: 1.6,
        },
        "Run to the player",
        &[],
    );
    let chase = t.add(NodeKind::Sequence, "Chase", &[chase_state, chasing, run]);
    let fight = t.add(NodeKind::Selector, "", &[attack, chase]);
    let seen = t.add(
        NodeKind::Conditional {
            key: "SeesPlayer".into(),
            op: CompareOp::IsSet,
            value: BbValue::Bool(true),
            abort: AbortMode::Both,
        },
        "Player seen",
        &[fight],
    );
    // Search for a moment after losing the player.
    let search_state = set_state(&mut t, "Search");
    let lost = t.add(
        NodeKind::Log {
            message: "Lost the player, searching".into(),
            level: LogLevel::Info,
        },
        "",
        &[],
    );
    let search_wait = t.add(
        NodeKind::Wait {
            seconds: 2.0,
            random: 0.0,
        },
        "Look around",
        &[],
    );
    let calm = t.add(
        NodeKind::SetBlackboard {
            key: "WasChasing".into(),
            value: BbValue::Bool(false),
            from_key: String::new(),
        },
        "",
        &[],
    );
    let search_seq = t.add(
        NodeKind::Sequence,
        "Search",
        &[search_state, lost, search_wait, calm],
    );
    let after_chase = t.add(
        NodeKind::Conditional {
            key: "WasChasing".into(),
            op: CompareOp::IsSet,
            value: BbValue::Bool(true),
            abort: AbortMode::None,
        },
        "After a chase",
        &[search_seq],
    );
    // Patrol.
    let patrol_state = set_state(&mut t, "Patrol");
    let next = t.add(
        NodeKind::CustomLua {
            script: NEXT_WAYPOINT.into(),
            params: [
                (
                    "waypoints".to_owned(),
                    "Waypoint 1, Waypoint 2, Waypoint 3, Waypoint 4".to_owned(),
                ),
                ("key".to_owned(), "PatrolPoint".to_owned()),
            ]
            .into(),
        },
        "Next Waypoint",
        &[],
    );
    let walk = t.add(
        NodeKind::MoveTo {
            target: "PatrolPoint".into(),
            acceptance: 1.3,
        },
        "",
        &[],
    );
    let rest = t.add(
        NodeKind::Wait {
            seconds: 1.0,
            random: 0.5,
        },
        "",
        &[],
    );
    let patrol = t.add(
        NodeKind::Sequence,
        "Patrol",
        &[patrol_state, next, walk, rest],
    );
    let brain_sel = t.add(NodeKind::Selector, "", &[seen, after_chase, patrol]);
    let brain = t.add(
        NodeKind::Repeat {
            count: 0,
            until_fail: false,
        },
        "Brain",
        &[brain_sel],
    );
    let guard = t.add(
        NodeKind::Parallel {
            policy: ParallelPolicy::RequireOne,
        },
        "Guard",
        &[eyes, brain],
    );
    t.add(NodeKind::Root, "", &[guard]);
    t.0.comments.push(misty_engine::behaviour_tree::asset::Comment {
        text: "The eyes update SeesPlayer ten times a second. \"Player seen\" aborts the patrol or the search as soon as it turns true, and gives up the chase when it turns false.".into(),
        position: [-40.0, -260.0],
        size: [420.0, 70.0],
    });
    t.0.arrange();
    let attack_nodes = vec![
        attack,
        in_range,
        attack_state,
        face,
        swing_or_wait,
        cooldown,
        swing,
        pause,
    ];
    t.0.groups.push(misty_engine::behaviour_tree::asset::Group {
        title: "Attack".into(),
        nodes: attack_nodes,
    });
    t.0.groups.push(misty_engine::behaviour_tree::asset::Group {
        title: "Patrol".into(),
        nodes: vec![patrol, patrol_state, next, walk, rest],
    });
    t.0
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

/// Static cube with a collider.
fn block(s: &mut Scene, name: &str, pos: Vec3, size: Vec3, material: &str) {
    let mut t = Transform::from_position(pos);
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

fn level(s: &mut Scene) {
    block(
        s,
        "Floor",
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(28.0, 1.0, 28.0),
        FLOOR,
    );
    let wall = |s: &mut Scene, name: &str, pos: Vec2, size: Vec2| {
        block(
            s,
            name,
            Vec3::new(pos.x, 1.0, pos.y),
            Vec3::new(size.x, 2.0, size.y),
            WALL,
        );
    };
    wall(s, "Wall North", Vec2::new(0.0, 14.25), Vec2::new(29.0, 0.5));
    wall(
        s,
        "Wall South",
        Vec2::new(0.0, -14.25),
        Vec2::new(29.0, 0.5),
    );
    wall(s, "Wall East", Vec2::new(14.25, 0.0), Vec2::new(0.5, 28.0));
    wall(s, "Wall West", Vec2::new(-14.25, 0.0), Vec2::new(0.5, 28.0));
    // The player hides behind it.
    wall(s, "Hiding Wall", Vec2::new(0.0, 10.0), Vec2::new(12.0, 0.5));
    for (i, (p, size)) in [
        (Vec2::new(-11.0, -11.0), 1.4),
        (Vec2::new(11.5, -2.0), 1.2),
        (Vec2::new(-11.5, 8.0), 1.2),
        (Vec2::new(0.0, -2.0), 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        block(
            s,
            &format!("Crate {}", i + 1),
            Vec3::new(p.x, size * 0.5, p.y),
            Vec3::splat(size),
            WOOD,
        );
    }
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("BehaviourTreeDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 24.0, -21.0))
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
    for (i, p) in WAYPOINTS.into_iter().enumerate() {
        let e = s.spawn(&format!("Waypoint {}", i + 1), None);
        s.set_transform(e, Transform::from_position(p));
    }
    let player = shape(
        &mut s,
        "Player",
        PrimitiveMesh::Capsule,
        Transform::from_position(HIDDEN),
        STEEL,
    );
    s.add(
        player,
        script(PLAYER_MOVE, &[("speed", ScriptValue::Number(5.0))]),
    );
    let guard = shape(
        &mut s,
        "Guard",
        PrimitiveMesh::Capsule,
        Transform::from_position(Vec3::new(-8.0, 1.0, -10.0)),
        RED,
    );
    s.add(
        guard,
        NavMeshAgent {
            base_offset: 1.0,
            speed: 4.0,
            angular_speed: 360.0,
            acceleration: 16.0,
            stopping_distance: 1.2,
            ..Default::default()
        },
    );
    s.add(
        guard,
        BehaviourTreeAgent {
            tree: Some(TREE.into()),
            overrides: vec![BlackboardOverride {
                key: "Player".into(),
                object: Some(player),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    s.add(
        guard,
        script(GUARD_BRAIN, &[("damage", ScriptValue::Number(10.0))]),
    );
    s
}

fn nav_mesh() -> NavMesh {
    bake::bake_scene(&demo_scene())
}

fn scripts() -> [(&'static str, &'static str); 3] {
    [
        (NEXT_WAYPOINT, NEXT_WAYPOINT_LUA),
        (GUARD_BRAIN, GUARD_BRAIN_LUA),
        (PLAYER_MOVE, PLAYER_MOVE_LUA),
    ]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_behaviour_tree_demo() {
    let root = project();
    std::fs::create_dir_all(root.join("Assets/AI")).unwrap();
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
    }
    guard_tree().save(&root.join(TREE)).unwrap();
    nav_mesh().save(&root.join(NAV_MESH)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn behaviour_tree_demo_matches_its_generator() {
    let root = project();
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}");
    }
    let tree = BehaviourTree::load(&root.join(TREE)).unwrap();
    assert_eq!(tree, guard_tree(), "{TREE}");
    let compiled = misty_engine::behaviour_tree::compile::compile(&tree);
    assert!(compiled.errors.is_empty(), "{:?}", compiled.errors);
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(&scene.to_json(), &demo_scene().to_json(), SCENE);
    let on_disk = NavMesh::load(&root.join(NAV_MESH)).unwrap();
    assert_eq!(on_disk.poly_count(), nav_mesh().poly_count());
    // The custom leaf shows in the Behaviour Tree window's palette.
    let leaves = crate::panels::behaviour_tree::palette_scan(&root);
    assert!(
        leaves
            .iter()
            .any(|i| i.custom.as_deref() == Some("Next Waypoint")),
        "{leaves:?}"
    );
}

/// Frames of the demo with the guard's states and the logs gathered.
struct Sim {
    host: misty_engine::scripting::ScriptHost,
    time: misty_engine::runtime::Time,
    guard: Entity,
    logs: Vec<(misty_engine::scripting::LogKind, String)>,
    states: Vec<String>,
}

impl Sim {
    fn state(&self) -> String {
        let guard = self.guard;
        self.host.with_behaviour(|b| {
            b.agent(guard)
                .and_then(|a| a.bb.get_named(&b.shared, "State").map(|v| v.to_string()))
                .unwrap_or_default()
        })
    }

    /// Play up to `seconds`; true as soon as `until` holds for the guard's state.
    fn run(&mut self, scene: &mut Scene, seconds: f32, until: impl Fn(&str) -> bool) -> bool {
        for _ in 0..(seconds * 60.0) as usize {
            self.time.advance(1.0 / 60.0);
            self.host
                .update(scene, &self.time, misty_engine::scripting::Input::default());
            self.logs.extend(self.host.take_logs());
            let s = self.state();
            if self.states.last() != Some(&s) {
                self.states.push(s.clone());
            }
            if until(&s) {
                return true;
            }
        }
        false
    }
}

/// Playing: the guard patrols, chases the player once it sees them, attacks in range, searches
/// when the player hides again, then goes back to its patrol.
#[test]
fn behaviour_tree_demo_plays() {
    use misty_engine::scripting::{LogKind, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let guard = scene.find_by_name("Guard").unwrap();
    let player = scene.find_by_name("Player").unwrap();
    let start = scene.world_position(guard);
    let host = ScriptHost::start(&mut scene, physics, root);
    let mut sim = Sim {
        host,
        time: Default::default(),
        guard,
        logs: Vec::new(),
        states: Vec::new(),
    };
    // Patrol for a while: the player is hidden.
    sim.run(&mut scene, 6.0, |_| false);
    assert_eq!(
        sim.state(),
        "Patrol",
        "states {:?} logs {:?}",
        sim.states,
        sim.logs
    );
    let moved = scene.world_position(guard);
    assert!(
        moved.distance(start) > 3.0,
        "the guard walks its patrol: {moved}"
    );
    // Step out in front of the guard: it chases, then attacks.
    let fwd = scene.world_rotation(guard) * Vec3::Z;
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
    let mut spot = moved + flat * 6.0;
    spot.x = spot.x.clamp(-12.0, 12.0);
    spot.z = spot.z.clamp(-12.0, 8.0);
    spot.y = 1.0;
    scene.set_world_position(player, spot);
    assert!(
        sim.run(&mut scene, 2.0, |s| s == "Chase" || s == "Attack"),
        "the guard notices the player: {:?}",
        sim.states
    );
    let reached = sim.run(&mut scene, 8.0, |s| s == "Attack");
    assert!(
        reached,
        "the guard reaches the player: {:?} at {} vs {spot}",
        sim.states,
        scene.world_position(guard)
    );
    sim.run(&mut scene, 1.5, |_| false);
    let swings = sim
        .logs
        .iter()
        .filter(|(_, l)| l.contains("attacks the player"))
        .count();
    assert!(swings >= 1, "{:?}", sim.logs);
    // Hide again: the guard searches, then patrols.
    scene.set_world_position(player, HIDDEN);
    assert!(
        sim.run(&mut scene, 3.0, |s| s == "Search"),
        "the guard searches: {:?}",
        sim.states
    );
    assert!(
        sim.run(&mut scene, 4.0, |s| s == "Patrol"),
        "the guard goes back to its patrol: {:?}",
        sim.states
    );
    let order: Vec<&str> = sim.states.iter().map(String::as_str).collect();
    let pos = |s: &str| order.iter().position(|x| *x == s).unwrap_or(usize::MAX);
    assert!(pos("Patrol") < pos("Attack"), "{order:?}");
    assert!(pos("Attack") < pos("Search"), "{order:?}");
    assert_eq!(order.last(), Some(&"Patrol"), "{order:?}");
    let problems: Vec<_> = sim
        .logs
        .iter()
        .filter(|(k, _)| *k != LogKind::Info)
        .collect();
    assert!(problems.is_empty(), "{problems:?}");
}
