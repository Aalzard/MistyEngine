//! Lua PlayableDirector API: signals reach scripts once, Play / Pause / Stop, `time`,
//! `duration`, `state` and the `stopped` event.

use std::path::PathBuf;

use glam::Vec3;

use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{DirectorWrapMode, PlayableDirector, Scene, ScriptRef, Scripts};
use crate::scripting::{Input, ScriptHost};
use crate::timeline::{Marker, TimelineAsset, TimelineClip, TrackKind};

/// A project with `T.lua` and `T.timeline`: 2 s long, a Signal track calling `OnCue("hi")` at
/// 0.5 s and a timeline marker calling `OnEnd` at 2 s.
fn project(source: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_timeline_script_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    std::fs::write(root.join("Assets/T.lua"), source).unwrap();
    let mut t = TimelineAsset::default();
    let act = t.add_track(TrackKind::Activation, None);
    t.track_mut(&act).unwrap().clips.push(TimelineClip {
        duration: 2.0,
        ..Default::default()
    });
    let sig = t.add_track(TrackKind::Signal, None);
    t.track_mut(&sig).unwrap().markers.push(Marker {
        time: 0.5,
        method: "OnCue".into(),
        parameter: Some("hi".into()),
        retroactive: false,
    });
    t.markers.push(Marker {
        time: 2.0,
        method: "OnEnd".into(),
        ..Default::default()
    });
    t.save(&root.join("Assets/T.timeline")).unwrap();
    root
}

fn run(source: &str, play_on_awake: bool, frames: usize) -> Vec<String> {
    let root = project(source);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Director", None);
    scene.add(
        e,
        PlayableDirector {
            playable: Some("Assets/T.timeline".into()),
            play_on_awake,
            wrap_mode: DirectorWrapMode::None,
            ..Default::default()
        },
    );
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/T.lua".into(),
                ..Default::default()
            }],
        },
    );
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..frames {
        time.advance(0.1);
        h.update(&mut scene, &time, Input::default());
    }
    h.take_logs().into_iter().map(|(_, t)| t).collect()
}

#[test]
fn signals_fire_once_and_stopped_runs_listeners() {
    let src = r#"
local T = {}
function T:Start()
    local d = self.gameObject:GetComponent("PlayableDirector")
    print("start", d.state, d.duration)
    d.stopped:AddListener(function(dir) print("stopped", dir.state, dir.time) end)
end
function T:OnCue(text) print("cue", text) end
function T:OnEnd() print("end") end
return T
"#;
    let logs = run(src, true, 30);
    assert_eq!(
        logs,
        ["start Playing 2", "cue hi", "end", "stopped Stopped 0"]
    );
}

#[test]
fn generic_bindings_played_and_signal_receivers_from_lua() {
    let src = r#"
local T = {}
function T:Start()
    local d = self.gameObject:GetComponent("PlayableDirector")
    d.played:AddListener(function(dir) print("played", dir.state) end)
    d.signalReceived:AddListener(function(dir, method, param)
        print("signal", method, tostring(param), dir.gameObject.name)
    end)
    d.stopped:AddListener(function() print("stopped") end)
    print("before", tostring(d:GetGenericBinding("Activation Track")))
    d:SetGenericBinding("Activation Track", self.gameObject)
    print("bound", d:GetGenericBinding("Activation Track").name)
    local ok = pcall(function() d:SetGenericBinding("No Such Track", nil) end)
    print("missing track errors", tostring(not ok))
    d:Play()
end
function T:OnCue(text) print("cue", text) end
function T:OnEnd() print("end") end
return T
"#;
    let logs = run(src, false, 30);
    assert_eq!(
        logs,
        [
            "before nil",
            "bound Director",
            "missing track errors true",
            "played Playing",
            "cue hi",
            "signal OnCue hi Director",
            "end",
            "signal OnEnd nil Director",
            "stopped"
        ],
        "{logs:?}"
    );
}

#[test]
fn play_pause_stop_and_time_from_lua() {
    let src = r#"
local T = { frames = 0 }
function T:Start()
    self.d = self.gameObject:GetComponent("PlayableDirector")
    print(self.d.state, tostring(self.d.playOnAwake))
    self.d.time = 1.0
    self.d:Play()
end
function T:Update()
    self.frames = self.frames + 1
    if self.frames == 3 then
        self.d:Pause()
        print(self.d.state, string.format("%.1f", self.d.time))
    elseif self.frames == 5 then
        print(string.format("%.1f", self.d.time))
        self.d:Play()
    elseif self.frames == 6 then
        self.d:Stop()
        print(self.d.state, self.d.time)
    end
end
function T:OnCue() print("cue") end
return T
"#;
    let logs = run(src, false, 8);
    // Started at 1.0 s: the cue at 0.5 s is behind it and never fires.
    assert_eq!(
        logs,
        ["Stopped false", "Paused 1.1", "1.1", "Stopped 0"],
        "{logs:?}"
    );
}
