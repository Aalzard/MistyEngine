//! `SceneManager.LoadSceneAsync` / `AsyncOperation` and `DontDestroyOnLoad` from Lua and C#,
//! with scene requests carried out between frames like the editor and the player do.

use std::path::{Path, PathBuf};

use crate::audio::Audio;
use crate::build::{BuildScene, BuildSettings};
use crate::runtime::{Runtime, Time};
use crate::scene::{Scene, ScriptRef, Scripts};
use crate::scripting::{Input, LogKind, Request};

fn project(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_scene_async_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    for (name, src) in files {
        std::fs::write(root.join("Assets/Scripts").join(name), src).unwrap();
    }
    for name in ["Extra", "Next"] {
        let mut s = Scene::new(name);
        s.spawn(&format!("{name}Thing"), None);
        s.save(&root.join(format!("Assets/Scenes/{name}.scene")))
            .unwrap();
    }
    root
}

fn build() -> BuildSettings {
    BuildSettings {
        scenes: ["Extra", "Next"]
            .iter()
            .map(|n| BuildScene {
                path: format!("Assets/Scenes/{n}.scene"),
                enabled: true,
            })
            .collect(),
        ..Default::default()
    }
}

/// Run `frames` frames of a game whose first object runs `script`, carrying out scene requests
/// between frames. Returns the logged lines (no errors allowed) and the final scene.
fn play(root: &Path, script: &str, frames: usize) -> (Vec<String>, Scene) {
    let build = build();
    let mut scene = Scene::new("First");
    let e = scene.spawn("Loader", None);
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{script}"),
                ..Default::default()
            }],
        },
    );
    let (audio, _log) = Audio::null(root.to_path_buf());
    let mut rt =
        Runtime::start_with_audio(&mut scene, root.to_path_buf(), &Default::default(), audio);
    let mut time = Time::default();
    let mut lines = Vec::new();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        for r in rt.take_requests() {
            let result = match &r {
                Request::LoadScene(which) => {
                    assert!(rt.has_persistent_objects(&scene), "the loader is kept");
                    let (rel, _) = build.resolve(which).unwrap();
                    let text = std::fs::read_to_string(root.join(&rel)).unwrap();
                    rt.load_scene_single(&mut scene, &rel, &text)
                }
                other => rt.apply_scene_request(&mut scene, &build, root, other),
            };
            result.unwrap();
        }
        for (k, t) in rt.take_logs() {
            assert_ne!(k, LogKind::Error, "{t}");
            lines.push(t);
        }
    }
    (lines, scene)
}

#[test]
fn lua_load_scene_async_waits_for_activation_and_completes() {
    let src = r#"
local L = { f = 0 }
function L:Start()
    DontDestroyOnLoad(self.gameObject)
    self.add = SceneManager.LoadSceneAsync("Extra", LoadSceneMode.Additive)
    self.add.allowSceneActivation = false
    self.add:OnCompleted(function(op)
        Debug.Log("extra done", op.progress, SceneManager.GetSceneCount())
    end)
    Debug.Log("start", self.add.progress, tostring(self.add.isDone),
        tostring(self.add.allowSceneActivation))
end
function L:Update()
    self.f = self.f + 1
    if self.f == 3 then
        Debug.Log("waiting", tostring(self.add.isDone), SceneManager.GetSceneCount())
        self.add.allowSceneActivation = true
    elseif self.f == 6 then
        local op = SceneManager.LoadSceneAsync("Next")
        op.completed = function(o)
            Debug.Log("next done", tostring(o.isDone), SceneManager.GetActiveScene().name,
                SceneManager.GetSceneCount(), self.f)
        end
    end
end
return L
"#;
    let root = project("lua", &[("Loader.lua", src)]);
    let (lines, scene) = play(&root, "Loader.lua", 10);
    assert_eq!(
        lines,
        [
            "start 0.9 false false",
            "waiting false 1",
            "extra done 1 2",
            "next done true Next 2 6",
        ],
        "{lines:?}"
    );
    assert!(scene.find_by_name("Loader").is_some());
    assert!(scene.find_by_name("NextThing").is_some());
    assert!(scene.find_by_name("ExtraThing").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn csharp_load_scene_async_yields_and_keeps_dont_destroy_objects() {
    if !super::csharp::dotnet_available() {
        eprintln!("no .NET runtime installed: C# test skipped");
        return;
    }
    let src = r#"
using System.Collections;
using Misty;
public class Loader : MonoBehaviour
{
    int frames;
    void Update() { frames++; }
    IEnumerator Start()
    {
        DontDestroyOnLoad(gameObject);
        var op = SceneManager.LoadSceneAsync("Extra", LoadSceneMode.Additive);
        op.allowSceneActivation = false;
        op.completed += o => Debug.Log("completed " + o.progress);
        Debug.Log("start " + op.progress + " " + op.isDone + " " + op.allowSceneActivation);
        yield return null;
        yield return null;
        op.allowSceneActivation = true;
        yield return op;
        Debug.Log("extra " + op.isDone + " " + (GameObject.Find("ExtraThing") != null));
        var next = SceneManager.LoadSceneAsync(1);
        yield return next;
        Debug.Log("next " + next.isDone + " " + (GameObject.Find("NextThing") != null)
            + " " + (GameObject.Find("ExtraThing") == null) + " " + (frames > 3));
    }
}
"#;
    let root = project("cs", &[("Loader.cs", src)]);
    let (lines, scene) = play(&root, "Loader.cs", 12);
    assert_eq!(
        lines,
        [
            "start 0.9 False False",
            "completed 1",
            "extra True True",
            "next True True True True",
        ],
        "{lines:?}"
    );
    assert!(scene.find_by_name("Loader").is_some());
    let _ = std::fs::remove_dir_all(&root);
}
