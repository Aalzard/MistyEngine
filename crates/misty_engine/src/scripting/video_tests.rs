//! Lua VideoPlayer API: events reach listeners once, Play / Pause / Stop, `time`, `frame`,
//! `length`, `isPlaying`, `isLooping`; rich text links call `OnLinkClicked` and listeners.

use std::path::PathBuf;

use glam::{Vec2, Vec3};

use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    Canvas, CanvasScaler, RectTransform, Scene, ScriptRef, Scripts, UiScaleMode, UiText,
};
use crate::scripting::{Input, LogKind, ScriptHost};
use crate::video::VideoPlayer;
use crate::video::generate::TestPattern;

fn project(name: &str, source: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_video_script_{name}_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    std::fs::write(root.join("Assets/T.lua"), source).unwrap();
    let clip = TestPattern {
        width: 64,
        height: 48,
        fps: 10,
        seconds: 1,
        sample_rate: 8000,
        ..Default::default()
    };
    std::fs::write(root.join("Assets/clip.avi"), clip.encode()).unwrap();
    root
}

fn scripts() -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: "Assets/T.lua".into(),
            ..Default::default()
        }],
    }
}

fn logs(h: &ScriptHost) -> Vec<String> {
    h.take_logs()
        .into_iter()
        .filter(|(k, _)| *k == LogKind::Info)
        .map(|(_, t)| t)
        .collect()
}

#[test]
fn video_player_events_and_controls_from_lua() {
    let src = r#"
local T = { frames = 0 }
function T:Awake()
    local vp = self.gameObject:GetComponent("VideoPlayer")
    self.vp = vp
    vp.prepareCompleted:AddListener(function(v) print("prepared", v.frameCount, v.length) end)
    vp.started:AddListener(function(v) print("started") end)
    vp.loopPointReached:AddListener(function(v) print("loop", v.isLooping) end)
end
function T:Update()
    self.frames = self.frames + 1
    local vp = self.vp
    if self.frames == 3 then
        print("playing", vp.isPlaying, vp.width, vp.height, vp.texture ~= nil)
    elseif self.frames == 8 then
        vp:Pause()
        print("paused", vp.isPlaying, vp.isPaused)
        vp.frame = 5
        print("seek", string.format("%.1f", vp.time))
        vp:Play()
    elseif self.frames == 30 then
        vp:Stop()
        print("stopped", vp.isPlaying, vp.time, vp.frame)
    end
end
return T
"#;
    let root = project("events", src);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Screen", None);
    scene.add(
        e,
        VideoPlayer {
            clip: Some("Assets/clip.avi".into()),
            looping: true,
            ..Default::default()
        },
    );
    scene.add(e, scripts());
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for i in 0..32 {
        if i == 1 {
            // Give the decoder thread time for the first frame.
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        time.advance(0.1);
        h.update(&mut scene, &time, Input::default());
    }
    let out = logs(&h);
    assert_eq!(
        out.iter().filter(|l| l.starts_with("prepared")).count(),
        1,
        "{out:?}"
    );
    assert_eq!(out[0], "prepared 10 1", "{out:?}");
    assert_eq!(out[1], "started", "{out:?}");
    assert!(
        out.contains(&"playing true 64 48 true".to_owned()),
        "{out:?}"
    );
    assert!(out.contains(&"paused false true".to_owned()), "{out:?}");
    assert!(out.contains(&"seek 0.5".to_owned()), "{out:?}");
    // 1 s clip, looping, seeked to 0.5 at frame 8 and stopped at frame 30: two loops.
    let loops = out.iter().filter(|l| *l == "loop true").count();
    assert_eq!(loops, 2, "{out:?}");
    assert_eq!(out.last().unwrap(), "stopped false 0 -1");
}

#[test]
fn clicking_a_link_reaches_scripts_and_listeners() {
    let src = r#"
local T = {}
function T:Start()
    self.gameObject:GetComponent("Text").onLinkClicked:AddListener(function(id) print("listener", id) end)
end
function T:OnLinkClicked(id) print("method", id) end
return T
"#;
    let root = project("link", src);
    let mut scene = Scene::new("UI");
    let canvas = scene.spawn("Canvas", None);
    scene.add(canvas, Canvas::default());
    scene.add(
        canvas,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    let label = scene.spawn("Label", Some(canvas));
    scene.add(label, RectTransform::sized(400.0, 60.0));
    scene.add(
        label,
        UiText {
            text: "<link=\"shop\">SHOP SHOP SHOP</link>".into(),
            font_size: 30.0,
            alignment: crate::scene::TextAnchor::MiddleCenter,
            ..Default::default()
        },
    );
    scene.add(label, scripts());
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let screen = Vec2::new(800.0, 600.0);
    let mut frame = |scene: &mut Scene, pos: Vec2, down: bool, up: bool| {
        time.advance(1.0 / 60.0);
        let input = Input {
            mouse_position: Some(pos),
            screen_size: screen,
            mouse_pressed: [down, false, false],
            mouse_released: [up, false, false],
            ..Default::default()
        };
        h.update(scene, &time, input);
    };
    let centre = Vec2::new(400.0, 300.0);
    frame(&mut scene, centre, false, false);
    frame(&mut scene, centre, true, false);
    frame(&mut scene, centre, false, true);
    // A press beside the text (inside the rect, outside the link) does nothing.
    frame(&mut scene, Vec2::new(205.0, 275.0), true, false);
    frame(&mut scene, Vec2::new(205.0, 275.0), false, true);
    let out = logs(&h);
    assert_eq!(out, ["method shop", "listener shop"], "{out:?}");
}
