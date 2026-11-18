//! C# scripts on the .NET runtime (proof): compile `.cs` files from a temp project, run frames
//! through the real script host, and check what they did to the scene. Skipped (with a note)
//! when no .NET SDK is installed.

use std::path::PathBuf;
use std::time::Instant;

use glam::Vec3;

use super::*;
use crate::physics::Physics;
use crate::scene::ScriptRef;

fn project(files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_csharp_test_{n}"));
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

fn skip() -> bool {
    if csharp::dotnet_available() {
        return false;
    }
    eprintln!("no .NET runtime installed: C# test skipped");
    true
}

const ROTATOR: &str = r#"
using Misty;

public class Rotator : MonoBehaviour
{
    public float speed = 180f;
    int updates;

    void Awake() { Debug.Log("awake " + name); }
    void Start() { Debug.Log("start " + gameObject.name); }

    void Update()
    {
        updates++;
        transform.Rotate(0f, speed * Time.deltaTime, 0f);
        transform.Translate(Vector3.forward * Time.deltaTime);
    }

    void OnDestroy() { Debug.Log("destroyed after " + updates + " updates"); }
}
"#;

const LUA_ROTATOR: &str = r#"
local LuaRotator = {}
function LuaRotator:Update()
    self.transform:Rotate(Vector3.new(0, 90 * Time.deltaTime, 0))
end
return LuaRotator
"#;

#[test]
fn csharp_rotator_turns_and_moves_the_cube_next_to_a_lua_script() {
    if skip() {
        return;
    }
    let root = project(&[("Rotator.cs", ROTATOR), ("LuaRotator.lua", LUA_ROTATOR)]);
    let mut scene = Scene::new("t");
    let cube = scene.spawn("Cube", None);
    attach(&mut scene, cube, "Rotator.cs");
    let other = scene.spawn("Other", None);
    attach(&mut scene, other, "LuaRotator.lua");

    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let timings = host.csharp_timings();
    let mut time = Time::default();
    for _ in 0..30 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    host.shutdown(&mut scene);
    let logs = host.take_logs();
    let errors: Vec<_> = logs.iter().filter(|(k, _)| *k == LogKind::Error).collect();
    assert!(errors.is_empty(), "errors: {errors:#?}");

    // 180°/s for 0.5 s around Y, and 1 unit/s forward along the turning heading: a quarter
    // circle 0.5 long, whose chord is 2 · (0.5 / (π/2)) · sin 45° ≈ 0.450.
    let yaw = scene.transform(cube).euler().y;
    assert!((yaw - 90.0).abs() < 1.0, "C# yaw {yaw}");
    let pos = scene.transform(cube).position;
    assert!(
        (pos.length() - 0.450).abs() < 0.02,
        "moved along a quarter arc: {pos}"
    );
    // The Lua script on the other object ran in the same frames.
    let lua_yaw = scene.transform(other).euler().y;
    assert!((lua_yaw - 45.0).abs() < 1.0, "Lua yaw {lua_yaw}");

    let texts: Vec<&str> = logs.iter().map(|(_, t)| t.as_str()).collect();
    assert!(texts.contains(&"awake Cube"), "{texts:?}");
    assert!(texts.contains(&"start Cube"), "{texts:?}");
    assert!(texts.contains(&"destroyed after 30 updates"), "{texts:?}");
    eprintln!(
        "C# compile {:?} (cached: {}), load {:?}",
        timings.compile, timings.cached, timings.load
    );
}

#[test]
fn csharp_compile_errors_and_exceptions_reach_the_console() {
    if skip() {
        return;
    }
    // A compile error: nothing runs, the error names the file and line.
    let root = project(&[(
        "Broken.cs",
        "using Misty;\npublic class Broken : MonoBehaviour {\n  void Update() { int x = \"no\"; }\n}\n",
    )]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, "Broken.cs");
    let physics = Physics::build(&scene, Vec3::ZERO);
    let host = ScriptHost::start(&mut scene, physics, root);
    let logs = host.take_logs();
    let error = logs
        .iter()
        .find(|(k, t)| *k == LogKind::Error && t.contains("did not compile"))
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    assert!(error.contains("Broken.cs(3"), "{logs:#?}");
    assert!(error.contains("CS0029"), "{error}");

    // An exception in Update: logged with the class and method, the game keeps running.
    let root = project(&[(
        "Thrower.cs",
        "using Misty;\npublic class Thrower : MonoBehaviour {\n  void Update() { throw new System.Exception(\"boom\"); }\n}\n",
    )]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, "Thrower.cs");
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..3 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    let logs = host.take_logs();
    let thrown = logs
        .iter()
        .filter(|(k, t)| *k == LogKind::Error && t.starts_with("Thrower (Update): Exception: boom"))
        .count();
    assert_eq!(thrown, 3, "{logs:#?}");
}

/// The proof's numbers: compile time (cold and cached), first load, and the per-frame cost of
/// many C# Update calls next to the same Lua script. `cargo test --release -p misty_engine
/// --lib csharp_cost -- --ignored --nocapture`
#[test]
#[ignore]
fn csharp_cost() {
    if skip() {
        return;
    }
    const SPIN_CS: &str = "using Misty;\npublic class Spin : MonoBehaviour {\n  void Update() { transform.Rotate(0f, 90f * Time.deltaTime, 0f); }\n}\n";
    const SPIN_LUA: &str = "local Spin = {}\nfunction Spin:Update() self.transform:Rotate(Vector3.new(0, 90 * Time.deltaTime, 0)) end\nreturn Spin\n";
    const EMPTY_CS: &str =
        "using Misty;\npublic class Idle : MonoBehaviour {\n  void Update() { }\n}\n";
    let root = project(&[
        ("Spin.cs", SPIN_CS),
        ("Spin.lua", SPIN_LUA),
        ("Idle.cs", EMPTY_CS),
    ]);
    for (label, file, count) in [
        ("C# Rotate", "Spin.cs", 1000),
        ("Lua Rotate", "Spin.lua", 1000),
        ("C# empty Update", "Idle.cs", 1000),
    ] {
        let mut scene = Scene::new("t");
        for i in 0..count {
            let e = scene.spawn(&format!("Obj{i}"), None);
            attach(&mut scene, e, file);
        }
        let physics = Physics::build(&scene, Vec3::ZERO);
        let started = Instant::now();
        let mut host = ScriptHost::start(&mut scene, physics, root.clone());
        let start_time = started.elapsed();
        let t = host.csharp_timings();
        let mut time = Time::default();
        // Warm up (JIT, caches), then time 300 frames.
        for _ in 0..30 {
            time.advance(1.0 / 60.0);
            host.update(&mut scene, &time, Input::default());
        }
        let frames = 300;
        let started = Instant::now();
        for _ in 0..frames {
            time.advance(1.0 / 60.0);
            host.update(&mut scene, &time, Input::default());
        }
        let per_frame = started.elapsed() / frames;
        let errors: Vec<_> = host
            .take_logs()
            .into_iter()
            .filter(|(k, _)| *k == LogKind::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        eprintln!(
            "{label:>16} x{count}: frame {per_frame:?} | Play start {start_time:?} \
             (compile {:?}, cached {}, load {:?})",
            t.compile, t.cached, t.load
        );
    }
}
