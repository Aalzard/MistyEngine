//! The node game in `SampleProject/Assets/NodeGame` ("Lumo's Leap"): a short 2D platformer whose
//! behaviour is all Script Graphs, no Lua and no engine-side gameplay code. A menu, three
//! stages and a win screen:
//!
//! - **Player** (`Player.vgraph`, fed by `Player Input.vgraph`): run, jump with coyote time, a
//!   jump buffer and variable height, and a dash attack on E that glows white and leaves a
//!   particle trail. Hearts; falling or touching an enemy costs one.
//! - **Enemies**: `Patrol.vgraph` walks between ledges and walls, `Enemy Touch.vgraph` tells a
//!   dash hit from a hurt, `Brute.vgraph` takes two hits (flash, knock back), `Puff.vgraph` is
//!   pushed away, fades and is destroyed.
//! - **Stage** (`Goal.vgraph`, `Stage.vgraph`, `Camera Follow.vgraph`): the flag loads the next
//!   scene; `Menu.vgraph` and `Win.vgraph` run the two screens.
//!
//! The files are written by `python tools/node_game/node_game.py SampleProject`. The tests here
//! check that the game is only Script Graphs that compile, and play stage 1 headlessly with
//! simulated keys.

use std::path::{Path, PathBuf};

use misty_engine::audio::Audio;
use misty_engine::build::{BuildSettings, SceneRef};
use misty_engine::hecs::Entity;
use misty_engine::runtime::{Runtime, Time};
use misty_engine::scene::{Scene, Scripts, SpriteRenderer, UiText};
use misty_engine::scripting::{Input, LogKind, Request};
use misty_engine::settings::ProjectSettings;
use misty_engine::visual_script::{self, ScriptGraph, Value};

const FOLDER: &str = "Assets/NodeGame";
const SCENES: [&str; 5] = [
    "NodeGameMenu",
    "NodeGameStage1",
    "NodeGameStage2",
    "NodeGameStage3",
    "NodeGameWin",
];

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn scene_path(name: &str) -> String {
    format!("{FOLDER}/Scenes/{name}.scene")
}

/// Every file under `dir` (project-relative paths).
fn files(root: &Path, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(root).unwrap();
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// The Script Graphs on the objects of a scene or prefab file.
fn graphs_of(root: &Path, rel: &str) -> Vec<String> {
    let (scene, report) = Scene::load(&root.join(rel)).unwrap();
    assert!(report.warnings.is_empty(), "{rel}: {:?}", report.warnings);
    let mut out = Vec::new();
    for (e, _) in scene.walk() {
        if let Ok(s) = scene.world.get::<&Scripts>(e) {
            out.extend(s.list.iter().map(|r| r.path.clone()));
        }
    }
    out
}

/// No Lua anywhere in the game: every script on every object is a Script Graph, every graph
/// compiles without a problem, and the scenes are in Build Settings (the graphs load them by
/// name).
#[test]
fn node_game_is_only_script_graphs() {
    let root = project();
    let all = files(&root, FOLDER);
    let lua: Vec<&String> = all.iter().filter(|f| f.ends_with(".lua")).collect();
    assert!(lua.is_empty(), "the node game has Lua files: {lua:?}");
    let resolver = visual_script::FileResolver(root.clone());
    let graphs: Vec<&String> = all
        .iter()
        .filter(|f| visual_script::is_graph_path(f))
        .collect();
    assert!(graphs.len() >= 12, "{graphs:?}");
    let mut nodes = 0;
    for rel in &graphs {
        let g = ScriptGraph::load(&root.join(rel)).unwrap();
        let errors = visual_script::compile::check(&g, &resolver);
        assert!(errors.is_empty(), "{rel}: {errors:?}");
        // Graphs never call into Lua scripts.
        assert!(
            g.nodes.iter().all(|n| n.kind != "CallLua"),
            "{rel} calls a Lua script"
        );
        nodes += g.nodes.len();
    }
    assert!(nodes > 300, "{nodes} nodes");
    let build = BuildSettings::load(&root).unwrap();
    let mut used = std::collections::BTreeSet::new();
    let mut containers: Vec<String> = SCENES.iter().map(|s| scene_path(s)).collect();
    containers.extend(all.iter().filter(|f| f.ends_with(".prefab")).cloned());
    for rel in &containers {
        if rel.ends_with(".scene") {
            assert!(
                build.scenes.iter().any(|s| s.path == *rel && s.enabled),
                "{rel} is not in Build Settings"
            );
        }
        for path in graphs_of(&root, rel) {
            assert!(visual_script::is_graph_path(&path), "{rel}: {path}");
            assert!(root.join(&path).is_file(), "{rel}: {path} is missing");
            used.insert(path);
        }
    }
    // Every graph is used by a scene or the prefab, but Autopilot (the demo driver).
    for rel in &graphs {
        assert!(
            used.contains(*rel) || rel.ends_with("Autopilot.vgraph"),
            "{rel} is not used"
        );
    }
}

/// A play session on one scene of the game.
struct Session {
    root: PathBuf,
    settings: ProjectSettings,
    scene: Scene,
    rt: Runtime,
    time: Time,
    /// Scenes the game asked to load.
    loads: Vec<String>,
    frames: u32,
}

impl Session {
    fn start(name: &str) -> Self {
        let root = project();
        // Tile assets (the ground's collider) load from the project root.
        misty_engine::assets::set_project_root(&root);
        let settings = ProjectSettings::load(&root).unwrap();
        let rel = scene_path(name);
        let (mut scene, _) = Scene::load(&root.join(&rel)).unwrap();
        scene.set_scene_path(0, &rel);
        let (audio, _) = Audio::null(root.clone());
        let rt = Runtime::start_with_audio(&mut scene, root.clone(), &settings, audio);
        let time = Time::new(&settings.time);
        Self {
            root,
            settings,
            scene,
            rt,
            time,
            loads: Vec::new(),
            frames: 0,
        }
    }

    /// One frame at 60 FPS with these keys held / pressed. Script errors fail the test.
    fn frame(&mut self, held: &[&str], pressed: &[&str]) {
        let mut input = Input::default();
        input.held.extend(held.iter().map(|k| (*k).to_owned()));
        input
            .pressed
            .extend(pressed.iter().map(|k| (*k).to_owned()));
        self.time.advance(1.0 / 60.0);
        self.rt.update(&mut self.scene, &self.time, input);
        self.frames += 1;
        for (kind, text) in self.rt.take_logs() {
            assert!(
                kind == LogKind::Info,
                "frame {}: {kind:?}: {text}",
                self.frames
            );
        }
        for r in self.rt.take_requests() {
            if let Request::LoadScene(SceneRef::Name(n)) = r {
                self.loads.push(n);
            }
        }
    }

    fn find(&self, name: &str) -> Option<Entity> {
        self.scene.find_by_name(name)
    }

    fn pos(&self, e: Entity) -> glam::Vec3 {
        self.scene.world_position(e)
    }

    fn text(&self, name: &str) -> String {
        let e = self.find(name).unwrap();
        self.scene.world.get::<&UiText>(e).unwrap().text.clone()
    }

    fn variable(&self, e: Entity, name: &str) -> Option<Value> {
        self.rt
            .object_variables(e)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    fn dashing(&self, player: Entity) -> bool {
        matches!(self.variable(player, "Dashing"), Some(Value::Bool(true)))
    }

    /// Follow the scene load the game asked for, like the editor and the player do.
    fn follow_load(&mut self) -> String {
        let name = self.loads.remove(0);
        let build = BuildSettings::load(&self.root).unwrap();
        let (rel, _) = build.resolve(&SceneRef::Name(name)).unwrap();
        let (mut scene, _) = Scene::load(&self.root.join(&rel)).unwrap();
        scene.set_scene_path(0, &rel);
        let (audio, _) = Audio::null(self.root.clone());
        self.rt = Runtime::start_with_audio(&mut scene, self.root.clone(), &self.settings, audio);
        self.scene = scene;
        self.time = Time::new(&self.settings.time);
        rel
    }
}

/// Stage 1 played headlessly with simulated keys: the player runs right (D), a dash (E) hits
/// the first Puff, which is pushed away, fades and is destroyed; the Brute takes two dashes;
/// Space jumps the two gaps; reaching the flag loads stage 2, which then starts.
#[test]
fn node_game_plays_stage_one() {
    let mut s = Session::start("NodeGameStage1");
    for _ in 0..30 {
        s.frame(&[], &[]);
    }
    let player = s.find("Player").unwrap();
    let start = s.pos(player);
    assert!(
        (0.3..0.7).contains(&start.y),
        "the player stands on the ground: {start}"
    );
    assert_eq!(s.text("Hearts Text"), "Hearts: 3");
    assert_eq!(s.text("Stage Text"), "Stage 1");
    assert_eq!(s.text("Dash Text"), "Dash: ready");
    // The Puff patrols.
    let puff = s.find("Puff 1").unwrap();
    let puff_x = s.pos(puff).x;
    assert!(
        (puff_x - 11.5).abs() > 0.2,
        "the Puff walked from its place: {puff_x}"
    );

    // The player moves.
    s.frame(&["d"], &["d"]);
    for _ in 0..29 {
        s.frame(&["d"], &[]);
    }
    let x = s.pos(player).x;
    assert!(x > start.x + 2.0, "ran right: {} -> {x}", start.x);

    // Run on; dash when an enemy is close ahead, jump at the two gaps (tiles 15-16 and 29-30).
    let enemies = ["Puff 1", "Brute 1", "Puff 2"];
    let mut dashes = 0;
    let mut glowed = false;
    let mut recharging = false;
    let mut brute_health = Vec::new();
    let mut jumped_from = Vec::new();
    let mut was_dashing = false;
    for frame in 0..60 * 40 {
        if !s.loads.is_empty() {
            break;
        }
        let p = s.pos(player);
        let mut pressed = Vec::new();
        let close = enemies.iter().filter_map(|n| s.find(n)).any(|e| {
            let d = s.pos(e) - p;
            (0.0..1.9).contains(&d.x) && d.y.abs() < 1.0
        });
        if close && !s.dashing(player) {
            pressed.push("e");
        }
        let at_gap = [(13.8, 14.8), (27.8, 28.8)]
            .iter()
            .any(|(a, b)| (*a..*b).contains(&p.x));
        let mut held = vec!["d"];
        if at_gap && p.y < 0.6 {
            pressed.push("space");
            jumped_from.push(p.x);
        }
        // Hold the jump key while over a gap: a full jump.
        if at_gap || p.y > 0.6 {
            held.push("space");
        }
        s.frame(&held, &pressed);
        let dashing = s.dashing(player);
        if dashing && !was_dashing {
            dashes += 1;
        }
        if dashing {
            let c = s.scene.world.get::<&SpriteRenderer>(player).unwrap().color;
            glowed |= c[0] > 2.0 && c[1] > 2.0 && c[2] > 2.0;
        }
        was_dashing = dashing;
        recharging |= s.text("Dash Text") == "Dash: ...";
        if let Some(Value::Int(h)) = s.find("Brute 1").and_then(|b| s.variable(b, "Health"))
            && brute_health.last() != Some(&h)
        {
            brute_health.push(h);
        }
        assert!(frame < 60 * 40 - 1, "stage 1 was not cleared: at {p}");
    }
    assert!(dashes >= 3, "dashed {dashes} times");
    assert!(glowed, "the dashing player glows white");
    assert!(recharging, "the HUD shows the dash recharging");
    assert!(s.find("Puff 1").is_none(), "the Puff was destroyed");
    assert!(s.find("Brute 1").is_none(), "the Brute was destroyed");
    assert_eq!(brute_health, [2, 1], "the Brute took two hits");
    assert!(jumped_from.len() >= 2, "jumped at {jumped_from:?}");
    assert_eq!(s.text("Banner Text"), "Stage clear!");
    assert_eq!(s.loads, ["NodeGameStage2"]);

    // Stage 2 starts.
    let rel = s.follow_load();
    assert_eq!(rel, scene_path("NodeGameStage2"));
    for _ in 0..30 {
        s.frame(&[], &[]);
    }
    assert_eq!(s.text("Stage Text"), "Stage 2");
    assert_eq!(s.text("Hearts Text"), "Hearts: 3");
    assert!(s.find("Player").is_some());
}

/// The menu starts stage 1 on Enter, and the win screen goes back to the menu on a key.
#[test]
fn node_game_menu_and_win_screen() {
    let mut s = Session::start("NodeGameMenu");
    for _ in 0..10 {
        s.frame(&[], &[]);
    }
    assert_eq!(s.text("Title"), "Lumo's Leap");
    assert!(s.loads.is_empty());
    s.frame(&["return"], &["return"]);
    for _ in 0..30 {
        s.frame(&[], &[]);
    }
    assert_eq!(s.loads, ["NodeGameStage1"]);

    let mut s = Session::start("NodeGameWin");
    for _ in 0..10 {
        s.frame(&[], &[]);
    }
    assert_eq!(s.text("Title"), "You Won!");
    assert!(s.loads.is_empty());
    s.frame(&["space"], &["space"]);
    s.frame(&[], &[]);
    assert_eq!(s.loads, ["NodeGameMenu"]);
    // Left alone, it goes back by itself after a few seconds.
    let mut s = Session::start("NodeGameWin");
    for _ in 0..60 * 7 {
        s.frame(&[], &[]);
    }
    assert_eq!(s.loads, ["NodeGameMenu"]);
}
