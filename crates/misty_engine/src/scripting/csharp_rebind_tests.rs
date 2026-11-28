//! C# interactive rebinding (`action.PerformInteractiveRebinding()`), `action.bindings`,
//! overrides through PlayerPrefs as JSON, canceling through Escape; `Touch.tapCount`.
//! Compiles real `.cs` files with the .NET SDK; skipped (with a note) when none is installed.

use std::path::PathBuf;

use glam::{Vec2, Vec3};

use crate::input::actions::InputActionAsset;
use crate::input::{Touch, TouchPhase};
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{Scene, ScriptRef, Scripts};
use crate::scripting::{Input, LogKind, ScriptHost, csharp};

fn skip() -> bool {
    if csharp::dotnet_available() {
        return false;
    }
    eprintln!("no .NET runtime installed: C# test skipped");
    true
}

fn project(tag: &str, file: &str, src: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_cs_rebind_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts").join(file), src).unwrap();
    InputActionAsset::starter("Player")
        .save(&root.join("Assets/Player.inputactions"))
        .unwrap();
    root
}

/// Play a scene with one object running `file` for each input frame (0.1 s apart).
fn play(root: PathBuf, file: &str, frames: &[Input]) -> Vec<String> {
    let mut scene = Scene::new("t");
    let e = scene.spawn("Probe", None);
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{file}"),
                ..Default::default()
            }],
        },
    );
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut logs = Vec::new();
    for input in frames {
        time.advance(0.1);
        host.update(&mut scene, &time, input.clone());
        logs.extend(host.take_logs().into_iter().map(|(k, t)| match k {
            LogKind::Error => format!("ERROR {t}"),
            _ => t,
        }));
    }
    logs
}

fn key(prev: &[&str], held: &[&str]) -> Input {
    let mut i = Input::default();
    i.held.extend(held.iter().map(|k| (*k).to_owned()));
    i.pressed.extend(
        held.iter()
            .filter(|k| !prev.contains(k))
            .map(|k| (*k).to_owned()),
    );
    i.released.extend(
        prev.iter()
            .filter(|k| !held.contains(k))
            .map(|k| (*k).to_owned()),
    );
    i
}

const REBINDER: &str = r#"
using Misty;

public class Rebinder : MonoBehaviour
{
    InputActionAsset actions;
    InputAction jump;
    InputActionRebindingExtensions.RebindingOperation op;
    int n;

    void Start()
    {
        actions = InputActionAsset.Load("Assets/Player.inputactions");
        actions.Enable();
        jump = actions.FindAction("Jump");
        var b = jump.bindings;
        Debug.Log($"bindings {b.Length} {b[0].path} {b[0].groups} {b[1].effectivePath} override {(b[0].overridePath ?? "null")} pad {jump.GetBindingIndex(group: "Gamepad")} id {b[0].id.Length > 0}");
        var move = actions.FindAction("Move").bindings;
        Debug.Log($"move {move.Length} {move[0].isComposite} {move[0].name} {move[1].isPartOfComposite} {move[1].name} {move[1].path}");
    }

    void Update()
    {
        n++;
        if (n == 1)
        {
            op = jump.PerformInteractiveRebinding(0)
                .WithControlsExcluding("Mouse")
                .WithCancelingThrough("<Keyboard>/escape")
                .OnComplete(o =>
                {
                    var b = jump.bindings[0];
                    Debug.Log($"complete {o.selectedControl.path} {o.completed} {o.bindingIndex} {b.effectivePath} {b.hasOverrides} {jump.GetBindingDisplayString(0)}");
                    PlayerPrefs.SetString("cs_rebinds", actions.SaveBindingOverridesAsJson());
                    o.Dispose();
                })
                .OnCancel(o => Debug.Log("canceled early"))
                .Start();
            Debug.Log($"listening {op.started} enabled {jump.enabled}");
        }
        if (n == 4)
        {
            string json = PlayerPrefs.GetString("cs_rebinds");
            actions.RemoveAllBindingOverrides();
            Debug.Log("removed " + jump.GetBindingDisplayString(0));
            actions.LoadBindingOverridesFromJson(json);
            Debug.Log("loaded " + jump.GetBindingDisplayString(0) + " json " + json.Contains("<Keyboard>/j"));
        }
        if (n == 5)
        {
            op = jump.PerformInteractiveRebinding()
                .OnComplete(o => Debug.Log("completed?"))
                .OnCancel(o => Debug.Log($"canceled {o.canceled} {o.completed} {o.bindingIndex} {(o.selectedControl == null ? "none" : "?")}"))
                .Start();
        }
        if (n == 7)
        {
            Debug.Log("still " + jump.GetBindingDisplayString(0) + " enabled " + jump.enabled);
            var pad = jump.PerformInteractiveRebinding(0).WithBindingGroup("Gamepad");
            Debug.Log("group target " + pad.bindingIndex);
            pad.Start();
            pad.Cancel();
            Debug.Log("manual " + pad.canceled);
            try { pad.WithTargetBinding(0); }
            catch (System.InvalidOperationException) { Debug.Log("over"); }
            try { jump.PerformInteractiveRebinding(99); }
            catch (System.InvalidOperationException) { Debug.Log("bad index"); }
            try { actions.FindAction("Move").PerformInteractiveRebinding(0); }
            catch (System.InvalidOperationException) { Debug.Log("composite"); }
        }
        if (jump.WasPressedThisFrame()) Debug.Log("jump " + n);
    }
}
"#;

#[test]
fn csharp_interactive_rebinding_full_flow() {
    if skip() {
        return;
    }
    let _guard = crate::prefs::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = project("flow", "Rebinder.cs", REBINDER);
    crate::prefs::open(&root.join("prefs.json"));
    let mut click = Input::default();
    click.mouse_pressed[0] = true;
    click.mouse_held[0] = true;
    let frames = [
        Input::default(),
        click,
        key(&[], &["j"]),
        key(&["j"], &["j"]),
        key(&["j"], &[]),
        key(&[], &["escape"]),
        key(&["escape"], &[]),
        Input::default(),
        key(&[], &["j"]),
    ];
    let logs = play(root, "Rebinder.cs", &frames);
    assert_eq!(
        logs,
        [
            "bindings 2 <Keyboard>/space Keyboard&Mouse <Gamepad>/buttonSouth override null pad 1 id True",
            "move 11 True WASD True up <Keyboard>/w",
            "listening True enabled False",
            "complete <Keyboard>/j True 0 <Keyboard>/j True J",
            "removed Space",
            "loaded J json True",
            "canceled True False 0 none",
            "still J enabled True",
            "group target 1",
            "manual True",
            "over",
            "bad index",
            "composite",
            "jump 9",
        ]
    );
}

const TAPS: &str = r#"
using Misty;

public class Taps : MonoBehaviour
{
    void Update()
    {
        if (Input.touchCount == 0) return;
        var t = Input.GetTouch(0);
        Debug.Log($"{t.phase} {t.tapCount} {Input.touches[0].tapCount} {Touchscreen.current.primaryTouch.tapCount.ReadValue()}");
    }
}
"#;

fn touch(id: u64, x: f32, phase: TouchPhase) -> Input {
    Input {
        screen_size: Vec2::new(800.0, 600.0),
        touches: vec![Touch {
            finger_id: id,
            position: Vec2::new(x, 300.0),
            phase,
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn csharp_touch_tap_count() {
    if skip() {
        return;
    }
    let root = project("taps", "Taps.cs", TAPS);
    let mut frames = vec![
        touch(1, 100.0, TouchPhase::Began),
        touch(1, 100.0, TouchPhase::Ended),
        touch(2, 103.0, TouchPhase::Began),
        touch(2, 103.0, TouchPhase::Ended),
        // Down and up between frames: only the lift is seen.
        touch(3, 101.0, TouchPhase::Ended),
        // Far away: a new series.
        touch(4, 400.0, TouchPhase::Ended),
    ];
    frames.extend(std::iter::repeat_n(Input::default(), 8));
    frames.push(touch(5, 400.0, TouchPhase::Began));
    let logs = play(root, "Taps.cs", &frames);
    assert_eq!(
        logs,
        [
            "Began 1 1 1",
            "Ended 1 1 1",
            "Began 2 2 2",
            "Ended 2 2 2",
            "Ended 3 3 3",
            "Ended 1 1 1",
            "Began 1 1 1",
        ]
    );
}
