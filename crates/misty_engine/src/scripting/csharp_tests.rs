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

#[test]
fn csharp_rename_class_follows_the_file_and_leaves_other_words_alone() {
    let src = "using Misty;\n// Old helper, not OldThing\npublic class Old : MonoBehaviour\n{\n    public Old() { }\n    string s = \"Old\";\n    OldThing t;\n}\n";
    let out = csharp::rename_class(src, "Old", "Player").unwrap();
    assert!(out.contains("public class Player : MonoBehaviour"), "{out}");
    assert!(out.contains("public Player() { }"), "{out}");
    assert!(out.contains("// Old helper, not OldThing"), "{out}");
    assert!(out.contains("OldThing t;"), "{out}");
    // No such class, or not a valid name: nothing to rewrite.
    assert!(csharp::rename_class(src, "Missing", "Player").is_none());
    assert!(csharp::rename_class(src, "Old", "Not Valid").is_none());
    // The template's fields are what the Properties panel shows.
    let t = csharp::template("Hero");
    let info = super::inspect_script(&t, "Assets/Hero.cs").unwrap();
    assert_eq!(
        info.fields,
        [("speed".to_owned(), ScriptValue::Number(1.0))]
    );
}

/// Run `frames` frames of `scene` with a host on `root`, feeding `input(frame)`; returns the
/// logs (errors fail the test unless `allow_errors`).
fn run(
    scene: &mut Scene,
    root: &std::path::Path,
    frames: usize,
    input: impl Fn(usize) -> Input,
) -> Vec<(LogKind, String)> {
    let physics = Physics::build(scene, Vec3::ZERO);
    let mut host = ScriptHost::start(scene, physics, root.to_path_buf());
    let mut time = Time::default();
    for f in 0..frames {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, input(f));
    }
    host.shutdown(scene);
    let logs = host.take_logs();
    let errors: Vec<_> = logs.iter().filter(|(k, _)| *k == LogKind::Error).collect();
    assert!(errors.is_empty(), "errors: {errors:#?}");
    logs
}

fn texts(logs: &[(LogKind, String)]) -> Vec<&str> {
    logs.iter().map(|(_, t)| t.as_str()).collect()
}

#[test]
fn csharp_inspector_values_override_the_field_initializers() {
    if skip() {
        return;
    }
    let src = r#"
using Misty;
public class Tuned : MonoBehaviour
{
    public float speed = 1;
    public int count = 1;
    public bool on;
    [SerializeField] string label = "default";
    public Vector3 offset;
    public float untouched = 7.5f;
    void Start() { Debug.Log($"{speed} {count} {on} {label} {offset.x},{offset.y},{offset.z} {untouched}"); }
}
"#;
    let root = project(&[("Tuned.cs", src)]);
    // The Properties panel reads the same fields from the source.
    let info = super::inspect_script(src, "Assets/Scripts/Tuned.cs").unwrap();
    let names: Vec<&str> = info.fields.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        ["speed", "count", "on", "label", "offset", "untouched"]
    );

    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    let mut script = ScriptRef {
        path: "Assets/Scripts/Tuned.cs".into(),
        ..Default::default()
    };
    script
        .fields
        .insert("speed".into(), ScriptValue::Number(42.5));
    script
        .fields
        .insert("count".into(), ScriptValue::Number(3.0));
    script.fields.insert("on".into(), ScriptValue::Bool(true));
    script
        .fields
        .insert("label".into(), ScriptValue::Text("Hero".into()));
    script
        .fields
        .insert("offset".into(), ScriptValue::Vector3([1.0, 2.0, 3.0]));
    // A field the class no longer has is ignored.
    script
        .fields
        .insert("gone".into(), ScriptValue::Number(1.0));
    scene.add(e, Scripts { list: vec![script] });
    let logs = run(&mut scene, &root, 2, |_| Input::default());
    assert!(
        texts(&logs).contains(&"42.5 3 True Hero 1,2,3 7.5"),
        "{:?}",
        texts(&logs)
    );
}

#[test]
fn csharp_scripts_follow_the_lifecycle_through_instantiate_setactive_and_destroy() {
    if skip() {
        return;
    }
    let counter = r#"
using Misty;
public class Counter : MonoBehaviour
{
    public int updates;
    void Awake() { Debug.Log("Awake " + name); }
    void OnEnable() { Debug.Log("OnEnable " + name); }
    void Start() { Debug.Log("Start " + name); }
    void Update() { updates++; }
    void OnDisable() { Debug.Log("OnDisable " + name); }
    void OnDestroy() { Debug.Log("OnDestroy " + name); }
    public string Report() => name + " " + updates;
}
"#;
    let director = r#"
using Misty;
public class Director : MonoBehaviour
{
    GameObject copy;
    int frame;
    void Update()
    {
        frame++;
        var template = GameObject.Find("Template");
        switch (frame)
        {
            case 1:
                copy = Instantiate(template, new Vector3(5, 0, 0), Quaternion.identity);
                Debug.Log("copy " + copy.name + " at " + copy.transform.position.x);
                break;
            case 3:
                Debug.Log("found " + template.GetComponent<Counter>().Report());
                Debug.Log("copy has Counter " + (copy.GetComponent<Counter>() != null));
                copy.SetActive(false);
                break;
            case 4: copy.SetActive(true); break;
            case 5:
                template.GetComponent<Counter>().enabled = false;
                Destroy(copy);
                break;
            case 6:
                Debug.Log("copy alive " + (copy != null));
                Debug.Log("FindObjectsOfType " + FindObjectsOfType<Counter>().Length);
                break;
            case 9:
                Debug.Log("after disable " + template.GetComponent<Counter>().Report());
                break;
        }
    }
}
"#;
    let root = project(&[("Counter.cs", counter), ("Director.cs", director)]);
    let mut scene = Scene::new("t");
    let template = scene.spawn("Template", None);
    attach(&mut scene, template, "Counter.cs");
    let d = scene.spawn("Director", None);
    attach(&mut scene, d, "Director.cs");
    let logs = run(&mut scene, &root, 10, |_| Input::default());
    let t = texts(&logs);
    let order = |items: &[&str]| {
        let mut at = 0;
        for item in items {
            match t[at..].iter().position(|x| x == item) {
                Some(i) => at += i + 1,
                None => panic!("{item:?} missing (or out of order) in {t:#?}"),
            }
        }
    };
    order(&[
        "Awake Template",
        "OnEnable Template",
        "Start Template",
        "copy Template(Clone) at 5",
        "Awake Template(Clone)",
        "OnEnable Template(Clone)",
        "Start Template(Clone)",
        "found Template 3",
        "copy has Counter True",
        "OnDisable Template(Clone)",
        "OnEnable Template(Clone)",
        // Frame 5: the checkbox switches off after the callback, the copy goes at frame end.
        "OnDisable Template",
        "OnDisable Template(Clone)",
        "OnDestroy Template(Clone)",
        "copy alive False",
        "FindObjectsOfType 1",
        // Its 5th Update ran before the Director switched it off; none since.
        "after disable Template 5",
        "OnDestroy Template",
    ]);
}

#[test]
fn csharp_input_and_the_hierarchy() {
    if skip() {
        return;
    }
    let src = r#"
using Misty;
public class Mover : MonoBehaviour
{
    void Start()
    {
        var child = transform.Find("Arm/Hand");
        Debug.Log("hand " + (child != null) + " children " + transform.childCount);
        child.SetParent(null, true);
        Debug.Log("hand world x " + child.position.x + " parent null " + (child.parent == null));
        transform.position = new Vector3(10, 0, 0);
        Debug.Log("arm world x " + transform.GetChild(0).position.x);
    }
    void Update()
    {
        if (Input.GetKeyDown(KeyCode.Space)) Debug.Log("jump at frame " + Time.frameCount);
        if (Input.GetKey("left shift")) Debug.Log("shift held");
        if (Input.GetAxisRaw("Horizontal") > 0.5f) Debug.Log("right");
    }
}
"#;
    let root = project(&[("Mover.cs", src)]);
    let mut scene = Scene::new("t");
    let body = scene.spawn("Body", None);
    let arm = scene.spawn("Arm", Some(body));
    let hand = scene.spawn("Hand", Some(arm));
    let mut t = scene.transform(arm);
    t.position = Vec3::new(1.0, 0.0, 0.0);
    scene.set_transform(arm, t);
    let mut t = scene.transform(hand);
    t.position = Vec3::new(2.0, 0.0, 0.0);
    scene.set_transform(hand, t);
    attach(&mut scene, body, "Mover.cs");
    let logs = run(&mut scene, &root, 3, |f| {
        let mut i = Input::default();
        if f == 1 {
            i.pressed.insert("space".into());
            i.held.insert("space".into());
            i.held.insert("left shift".into());
            i.held.insert("right".into());
        }
        i
    });
    let t = texts(&logs);
    for want in [
        "hand True children 1",
        "hand world x 3 parent null True",
        "arm world x 11",
        "jump at frame 2",
        "shift held",
        "right",
    ] {
        assert!(t.contains(&want), "{want:?} missing in {t:#?}");
    }
    assert_eq!(scene.parent(hand), None);
}

#[test]
fn csharp_coroutines_waits_and_invoke() {
    if skip() {
        return;
    }
    let src = r#"
using Misty;
using System.Collections;
public class Timer : MonoBehaviour
{
    int frames, ticks, started;
    IEnumerator Start()
    {
        started = Time.frameCount;
        yield return null;
        Debug.Log("next frame " + (Time.frameCount == started + 1));
        yield return new WaitForSeconds(0.25f);
        Debug.Log("waited " + (Time.time >= 0.25f) + " " + (Time.time < 0.3f));
        yield return StartCoroutine(Inner());
        Debug.Log("inner done");
        yield return new WaitUntil(() => frames >= 40);
        Debug.Log("until " + frames);
        Invoke("Ping", 0.1f);
        InvokeRepeating("Count", 0f, 0.05f);
        var forever = StartCoroutine(Forever());
        yield return new WaitForSeconds(0.2f);
        StopCoroutine(forever);
        CancelInvoke();
        int seen = ticks;
        yield return new WaitForSeconds(0.2f);
        Debug.Log("ticks " + (seen >= 3) + " stopped " + (ticks == seen));
    }
    IEnumerator Inner() { yield return null; Debug.Log("inner"); }
    IEnumerator Forever() { while (true) yield return null; }
    void Count() { ticks++; }
    void Ping() { Debug.Log("ping"); }
    void Update() { frames++; }
}
"#;
    let blink = r#"
using Misty;
using System.Collections;
public class Blink : MonoBehaviour
{
    public int beats;
    void Start() { StartCoroutine(Beat()); }
    IEnumerator Beat() { while (true) { beats++; yield return null; } }
    void OnDisable() { Debug.Log("beats when switched off " + beats); }
}
"#;
    let switcher = r#"
using Misty;
public class Switcher : MonoBehaviour
{
    int frame;
    void Update()
    {
        frame++;
        var b = GameObject.Find("Blinker");
        if (frame == 5) b.SetActive(false);
        if (frame == 7) b.SetActive(true);
        if (frame == 10) Debug.Log("beats later " + b.GetComponent<Blink>().beats);
    }
}
"#;
    let root = project(&[
        ("Timer.cs", src),
        ("Blink.cs", blink),
        ("Switcher.cs", switcher),
    ]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Clock", None);
    attach(&mut scene, e, "Timer.cs");
    let b = scene.spawn("Blinker", None);
    attach(&mut scene, b, "Blink.cs");
    let s = scene.spawn("Switcher", None);
    attach(&mut scene, s, "Switcher.cs");
    let logs = run(&mut scene, &root, 90, |_| Input::default());
    let t = texts(&logs);
    for want in [
        "next frame True",
        "waited True True",
        "inner",
        "inner done",
        "until 40",
        "ping",
        "ticks True stopped True",
    ] {
        assert!(t.contains(&want), "{want:?} missing in {t:#?}");
    }
    // Switching the object off stops its coroutines (switching it on again doesn't restart
    // them): the beat count froze at frame 5.
    let off = t
        .iter()
        .find_map(|l| l.strip_prefix("beats when switched off "))
        .unwrap();
    assert!(t.contains(&format!("beats later {off}").as_str()), "{t:#?}");
}

#[test]
fn csharp_rigidbody_collisions_triggers_and_raycasts() {
    if skip() {
        return;
    }
    let src = r#"
using Misty;
public class Faller : MonoBehaviour
{
    Rigidbody rb;
    bool bounced;
    void Start()
    {
        rb = GetComponent<Rigidbody>();
        Debug.Log("rb " + (rb != null) + " collider " + (GetComponent<Collider>() != null)
            + " no audio " + (GetComponent<AudioSource>() == null) + " gravity " + Physics.gravity.y.ToString("F2"));
        rb.mass = 2.5f;
        Debug.Log("mass " + rb.mass);
        bool hit = Physics.Raycast(new Vector3(5, 3, 0), Vector3.down, out RaycastHit h, 100f);
        Debug.Log("ray " + hit + " " + h.collider.name + " " + h.distance.ToString("F2") + " " + h.normal.y.ToString("F0"));
        Debug.Log("miss " + Physics.Raycast(new Vector3(5, 3, 0), Vector3.up, 100f));
        Debug.Log("all " + Physics.RaycastAll(new Vector3(0, 10, 0), Vector3.down, 100f).Length);
        Debug.Log("overlap " + Physics.OverlapSphere(new Vector3(5, -0.5f, 5), 0.2f).Length);
        Debug.Log("layer " + LayerMask.NameToLayer("Default") + " " + LayerMask.LayerToName(0));
    }
    void OnTriggerEnter(Collider other) { Debug.Log("trigger " + other.name); }
    void OnCollisionEnter(Collision c)
    {
        Debug.Log("hit " + c.gameObject.name + " falling " + (rb.velocity.y < -1f));
        rb.AddForce(Vector3.up * 6f, ForceMode.VelocityChange);
    }
    void FixedUpdate()
    {
        if (!bounced && rb.velocity.y > 2f) { bounced = true; Debug.Log("bounced"); }
    }
}
"#;
    let root = project(&[("Faller.cs", src)]);
    let mut scene = Scene::new("t");
    let ground = scene.spawn("Ground", None);
    let mut t = crate::scene::Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
    t.scale = Vec3::new(20.0, 1.0, 20.0);
    scene.set_transform(ground, t);
    scene.add(ground, crate::scene::BoxCollider::default());
    let zone = scene.spawn("Zone", None);
    scene.set_transform(
        zone,
        crate::scene::Transform::from_position(Vec3::new(0.0, 1.5, 0.0)),
    );
    scene.add(
        zone,
        crate::scene::BoxCollider {
            is_trigger: true,
            ..Default::default()
        },
    );
    let ball = scene.spawn("Ball", None);
    scene.set_transform(
        ball,
        crate::scene::Transform::from_position(Vec3::new(0.0, 3.0, 0.0)),
    );
    scene.add(ball, crate::scene::BoxCollider::default());
    scene.add(ball, crate::scene::Rigidbody::default());
    attach(&mut scene, ball, "Faller.cs");

    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..120 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    host.shutdown(&mut scene);
    let logs = host.take_logs();
    let errors: Vec<_> = logs.iter().filter(|(k, _)| *k == LogKind::Error).collect();
    assert!(errors.is_empty(), "errors: {errors:#?}");
    let t = texts(&logs);
    for want in [
        "rb True collider True no audio True gravity -9.81",
        "mass 2.5",
        "ray True Ground 3.00 1",
        "miss False",
        "all 3",
        "overlap 1",
        "layer 0 Default",
        "trigger Zone",
        "hit Ground falling True",
        "bounced",
    ] {
        assert!(t.contains(&want), "{want:?} missing in {t:#?}");
    }
}

#[test]
fn csharp_scene_requests_and_missing_components() {
    if skip() {
        return;
    }
    let src = r#"
using Misty;
public class Loader : MonoBehaviour
{
    void Start()
    {
        Debug.Log("animator " + (GetComponent<Animator>() == null));
        try { new GameObject_Probe().Check(gameObject); } catch (System.Exception e) { Debug.Log(e.Message); }
        SceneManager.LoadScene("Level2");
        SceneManager.LoadScene(3, LoadSceneMode.Additive);
    }
}
public class GameObject_Probe
{
    public void Check(GameObject g)
    {
        // A destroyed object compares equal to null and throws on use.
        Debug.Log("alive " + (g != null));
    }
}
"#;
    let root = project(&[("Loader.cs", src)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, "Loader.cs");
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    host.update(&mut scene, &time, Input::default());
    let requests = host.take_requests();
    host.shutdown(&mut scene);
    let logs = host.take_logs();
    let t = texts(&logs);
    assert!(t.contains(&"animator True"), "{t:#?}");
    assert!(t.contains(&"alive True"), "{t:#?}");
    let names: Vec<String> = requests.iter().map(|r| format!("{r:?}")).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(
        names[0].contains("LoadScene(Name(\"Level2\"))"),
        "{names:?}"
    );
    assert!(
        names[1].contains("LoadSceneAdditive(Index(3))"),
        "{names:?}"
    );
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

#[test]
fn csharp_object_fields_get_objects_components_and_scripts() {
    if skip() {
        return;
    }
    let seeker_src = r#"
using Misty;
public class Seeker : MonoBehaviour
{
    public GameObject thing;
    public Transform target;
    public Rigidbody body;
    public Mover mover;
    public Transform empty;
    void Start()
    {
        Debug.Log($"thing={thing.name} x={target.position.x} body={body != null} speed={mover.speed} empty={empty == null}");
    }
}
"#;
    let mover_src = r#"
using Misty;
public class Mover : MonoBehaviour { public float speed = 4; }
"#;
    let root = project(&[("Seeker.cs", seeker_src), ("Mover.cs", mover_src)]);
    // Properties: fields typed with engine types or the project's script classes are slots.
    super::csharp_fields::set_script_classes(vec!["Seeker".into(), "Mover".into()]);
    let info = super::inspect_script(seeker_src, "Assets/Scripts/Seeker.cs").unwrap();
    let kinds: Vec<(String, String)> = info
        .fields
        .iter()
        .map(|(n, v)| match v {
            ScriptValue::Object(o) => (n.clone(), o.kind.clone()),
            other => (n.clone(), format!("{other:?}")),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("thing".into(), "GameObject".into()),
            ("target".into(), "Transform".into()),
            ("body".into(), "Rigidbody".into()),
            ("mover".into(), "Mover".into()),
            ("empty".into(), "Transform".into()),
        ]
    );

    let mut scene = Scene::new("t");
    // The seeker's script is made first: the Mover on the target comes later.
    let seeker = scene.spawn("Seeker", None);
    let target = scene.spawn("Target", None);
    let mut t = scene.transform(target);
    t.position = Vec3::new(5.0, 0.0, 0.0);
    scene.set_transform(target, t);
    scene.add(target, crate::scene::Rigidbody::default());
    attach(&mut scene, target, "Mover.cs");
    let obj = |e: Entity, kind: &str| {
        ScriptValue::Object(crate::scene::ObjectValue {
            object: Some(e.to_bits().get()),
            kind: kind.into(),
        })
    };
    let mut script = ScriptRef {
        path: "Assets/Scripts/Seeker.cs".into(),
        ..Default::default()
    };
    script
        .fields
        .insert("thing".into(), obj(target, "GameObject"));
    script
        .fields
        .insert("target".into(), obj(target, "Transform"));
    script
        .fields
        .insert("body".into(), obj(target, "Rigidbody"));
    script.fields.insert("mover".into(), obj(target, "Mover"));
    script.fields.insert(
        "empty".into(),
        ScriptValue::Object(crate::scene::ObjectValue::none("Transform")),
    );
    scene.add(seeker, Scripts { list: vec![script] });
    let logs = run(&mut scene, &root, 2, |_| Input::default());
    assert!(
        texts(&logs).contains(&"thing=Target x=5 body=True speed=4 empty=True"),
        "{:?}",
        texts(&logs)
    );
}

/// OnCollisionStay and OnTriggerStay run every physics step while two objects touch, in C#
/// and Lua alike, and stop when they part.
#[test]
fn csharp_and_lua_stay_callbacks_run_while_touching() {
    if skip() {
        return;
    }
    let cs = r#"
using Misty;
public class Rest : MonoBehaviour
{
    int stays, triggerStays;
    void OnCollisionStay(Collision c) { stays++; }
    void OnTriggerStay(Collider other) { triggerStays++; }
    void OnDestroy() { Debug.Log($"cs stays {(stays > 20)} {(triggerStays > 20)} {c(stays)}"); }
    static string c(int n) => n < 1000 ? "sane" : "runaway";
}
"#;
    let lua = r#"
local R = { stays = 0 }
function R:OnCollisionStay(other) self.stays = self.stays + 1 end
function R:OnDestroy() Debug.Log("lua stays " .. tostring(self.stays > 20)) end
return R
"#;
    let root = project(&[("Rest.cs", cs), ("Rest.lua", lua)]);
    let mut scene = Scene::new("t");
    let ground = scene.spawn("Ground", None);
    let mut t = crate::scene::Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
    t.scale = Vec3::new(20.0, 1.0, 20.0);
    scene.set_transform(ground, t);
    scene.add(ground, crate::scene::BoxCollider::default());
    // A trigger the box sits inside the whole time.
    let zone = scene.spawn("Zone", None);
    let mut z = crate::scene::Transform::from_position(Vec3::new(0.0, 0.5, 0.0));
    z.scale = Vec3::new(4.0, 4.0, 4.0);
    scene.set_transform(zone, z);
    scene.add(
        zone,
        crate::scene::BoxCollider {
            is_trigger: true,
            ..Default::default()
        },
    );
    let boxy = scene.spawn("Box", None);
    scene.set_transform(
        boxy,
        crate::scene::Transform::from_position(Vec3::new(0.0, 0.55, 0.0)),
    );
    scene.add(boxy, crate::scene::BoxCollider::default());
    scene.add(boxy, crate::scene::Rigidbody::default());
    attach(&mut scene, boxy, "Rest.cs");
    attach(&mut scene, boxy, "Rest.lua");
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..90 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    host.shutdown(&mut scene);
    let logs = host.take_logs();
    let t = texts(&logs);
    assert!(t.contains(&"cs stays True True sane"), "{t:?}");
    assert!(t.contains(&"lua stays true"), "{t:?}");
}

#[test]
fn csharp_camera_light_renderer_material_screen_and_cursor() {
    if skip() {
        return;
    }
    let src = r#"
using Misty;
public class Look : MonoBehaviour
{
    static string V(Vector3 v) => v.x.ToString("F1") + " " + v.y.ToString("F1") + " " + v.z.ToString("F1");
    void Start()
    {
        var cam = Camera.main;
        Debug.Log("main " + cam.name + " own " + (GetComponent<Camera>() == null));
        cam.fieldOfView = 50f;
        cam.farClipPlane = 500f;
        cam.cullingMask = ~(1 << 4);
        cam.backgroundColor = new Color(0.1f, 0.2f, 0.3f);
        Debug.Log("cam " + cam.fieldOfView + " " + cam.farClipPlane + " " + cam.orthographic + " "
            + (cam.cullingMask == ~(1 << 4)) + " " + cam.backgroundColor.b.ToString("F1")
            + " " + cam.pixelWidth + "x" + cam.pixelHeight + " " + cam.aspect.ToString("F2"));
        Debug.Log("screen " + Screen.width + "x" + Screen.height);
        var ray = cam.ScreenPointToRay(new Vector3(400, 300, 0));
        Debug.Log("ray " + V(ray.direction) + " from " + ray.origin.z.ToString("F1"));
        Debug.Log("to screen " + V(cam.WorldToScreenPoint(Vector3.zero)));
        Debug.Log("to viewport " + V(cam.WorldToViewportPoint(Vector3.zero)));
        Debug.Log("to world " + V(cam.ScreenToWorldPoint(new Vector3(400, 300, 10))));
        Debug.Log("corner " + (cam.ScreenToWorldPoint(new Vector3(800, 600, 10)).x > 1f)
            + " " + (cam.ScreenToWorldPoint(new Vector3(800, 600, 10)).y > 1f));
        Debug.Log("viewport world " + V(cam.ViewportToWorldPoint(new Vector3(0.5f, 0.5f, 5))));
        Debug.Log("behind " + (cam.WorldToScreenPoint(new Vector3(0, 0, -20)).z < 0));

        var lamp = GameObject.Find("Lamp").GetComponent<Light>();
        lamp.intensity = 3f;
        lamp.color = Color.red;
        lamp.type = LightType.Spot;
        lamp.spotAngle = 40f;
        lamp.shadows = LightShadows.Hard;
        Debug.Log("light " + lamp.intensity + " " + lamp.color + " " + lamp.type + " " + lamp.spotAngle + " " + lamp.shadows + " " + lamp.enabled);

        var r = GetComponent<Renderer>();
        Debug.Log("renderer " + (r != null) + " " + (GetComponent<MeshRenderer>() != null) + " " + r.enabled);
        r.material.color = Color.Lerp(Color.black, Color.blue, 0.5f);
        r.material.SetFloat("_Metallic", 0.75f);
        r.material.SetColor("_EmissionColor", new Color(2, 0, 0));
        r.material.EnableKeyword("_EMISSION");
        Debug.Log("material " + r.material.color + " " + r.material.GetFloat("_Metallic") + " "
            + r.material.GetColor("_EmissionColor").r + " " + r.material.IsKeywordEnabled("_EMISSION")
            + " " + r.material.HasProperty("_Color") + " " + r.material.HasProperty("_Nope"));
        r.material.mainTextureScale = new Vector2(2, 3);
        Debug.Log("tiling " + r.material.mainTextureScale.y);
        try { r.sharedMaterial.color = Color.red; }
        catch (System.InvalidOperationException e) { Debug.Log("shared: " + e.Message); }
        r.receiveShadows = false;
        r.enabled = false;

        Cursor.lockState = CursorLockMode.Locked;
        Debug.Log("cursor " + Cursor.lockState + " " + Cursor.visible);
        Debug.Log("hsv " + Color.HSVToRGB(0f, 1f, 1f));
    }
}
"#;
    let root = project(&[("Look.cs", src)]);
    let mut scene = Scene::new("t");
    let cam = scene.spawn("Cam", None);
    scene.game_object_mut(cam).tag = "MainCamera".into();
    scene.set_transform(
        cam,
        crate::scene::Transform::from_position(Vec3::new(0.0, 0.0, -10.0)),
    );
    scene.add(cam, crate::scene::Camera::default());
    let lamp = scene.spawn("Lamp", None);
    scene.add(lamp, crate::scene::Light::default());
    let cube = scene.spawn("Cube", None);
    scene.add(cube, crate::scene::MeshRenderer::default());
    attach(&mut scene, cube, "Look.cs");

    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    let input = Input {
        screen_size: glam::Vec2::new(800.0, 600.0),
        ..Default::default()
    };
    host.update(&mut scene, &time, input);
    let cursor = host.cursor();
    host.shutdown(&mut scene);
    let logs = host.take_logs();
    let errors: Vec<_> = logs.iter().filter(|(k, _)| *k == LogKind::Error).collect();
    assert!(errors.is_empty(), "errors: {errors:#?}");
    let t = texts(&logs);
    for want in [
        "main Cam own True",
        "cam 50 500 False True 0.3 800x600 1.33",
        "screen 800x600",
        "to screen 400.0 300.0 10.0",
        "to viewport 0.5 0.5 10.0",
        "to world 0.0 0.0 0.0",
        "corner True True",
        "viewport world 0.0 0.0 -5.0",
        "behind True",
        "light 3 RGBA(1.000, 0.000, 0.000, 1.000) Spot 40 Hard True",
        "renderer True True True",
        "material RGBA(0.000, 0.000, 0.500, 1.000) 0.75 2 True True False",
        "tiling 3",
        "shared: Default-Material can't be changed; assign a material first.",
        "cursor Locked False",
        "hsv RGBA(1.000, 0.000, 0.000, 1.000)",
    ] {
        assert!(t.contains(&want), "{want:?} missing in {t:#?}");
    }
    assert!(
        t.iter().any(|l| l.starts_with("ray 0.0 0.0 1.0 from -9.")),
        "{t:#?}"
    );
    let c = scene.world.get::<&crate::scene::Camera>(cam).unwrap();
    assert_eq!((c.fov, c.far, c.culling_mask), (50.0, 500.0, !(1 << 4)));
    let l = scene.world.get::<&crate::scene::Light>(lamp).unwrap();
    assert_eq!(l.intensity, 3.0);
    assert_eq!(l.kind, crate::scene::LightType::Spot);
    let r = scene
        .world
        .get::<&crate::scene::MeshRenderer>(cube)
        .unwrap();
    assert!(!r.enabled && !r.receive_shadows && r.cast_shadows);
    assert_eq!(r.instance(0).unwrap().metallic, 0.75);
    assert_eq!(cursor.lock, CursorLockMode::Locked);
}
