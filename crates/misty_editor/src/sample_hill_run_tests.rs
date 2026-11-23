//! Hill Run played headlessly through the real runtime: the C# scripts compiled with the .NET
//! SDK, keys and a fake gamepad injected frame by frame. Skipped (with a note) without .NET.

use super::*;
use misty_engine::audio::Audio;
use misty_engine::build::SceneRef;
use misty_engine::input::Joystick;
use misty_engine::runtime::{Runtime, Time};
use misty_engine::scripting::{Input, LogKind, Request, csharp};
use misty_engine::settings::ProjectSettings;

const SOUTH: usize = 0;
const DPAD_UP: usize = 11;
const DPAD_DOWN: usize = 12;

/// The C# host keeps the scripts in the assembly's statics: games of one project run one at a
/// time in a process (as in the editor).
static ONE_GAME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A running Hill Run with the keys and the gamepad buttons held last frame.
struct Game {
    scene: Scene,
    rt: Runtime,
    time: Time,
    keys: Vec<String>,
    pad: Vec<usize>,
    stick: Vec2,
    logs: Vec<String>,
    /// Last, so it's let go after the runtime stopped.
    _one: std::sync::MutexGuard<'static, ()>,
}

impl Game {
    fn start() -> Option<Self> {
        if !csharp::dotnet_available() {
            eprintln!("no .NET runtime installed: Hill Run test skipped");
            return None;
        }
        let one = ONE_GAME.lock().unwrap_or_else(|e| e.into_inner());
        let root = project();
        let (mut scene, report) = Scene::load(&root.join(SCENE)).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let settings = ProjectSettings::load(&root).unwrap();
        let (audio, _) = Audio::null(root.clone());
        let rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
        let mut g = Self {
            scene,
            rt,
            time: Time::new(&settings.time),
            keys: Vec::new(),
            pad: Vec::new(),
            stick: Vec2::ZERO,
            logs: Vec::new(),
            _one: one,
        };
        g.idle(3);
        Some(g)
    }

    /// One frame holding `keys` and gamepad buttons `pad`, the left stick at `self.stick`.
    fn frame_with(&mut self, keys: &[&str], pad: &[usize]) {
        let mut input = Input {
            screen_size: Vec2::new(1280.0, 720.0),
            ..Default::default()
        };
        for k in keys {
            input.held.insert((*k).to_owned());
            if !self.keys.iter().any(|p| p == k) {
                input.pressed.insert((*k).to_owned());
            }
        }
        for k in &self.keys {
            if !keys.contains(&k.as_str()) {
                input.released.insert(k.clone());
            }
        }
        let mut j = Joystick {
            name: "Test Pad".into(),
            ..Default::default()
        };
        for &b in pad {
            j.held[b] = true;
            j.pressed[b] = !self.pad.contains(&b);
        }
        for &b in &self.pad {
            j.released[b] = !pad.contains(&b);
        }
        j.axes[0] = self.stick.x;
        j.axes[1] = -self.stick.y;
        j.axes[6] = j.held[DPAD_UP] as i32 as f32 - j.held[DPAD_DOWN] as i32 as f32;
        input.joysticks = vec![j];
        self.keys = keys.iter().map(|k| (*k).to_owned()).collect();
        self.pad = pad.to_vec();
        self.time.advance(1.0 / 60.0);
        self.rt.update(&mut self.scene, &self.time, input);
        for (kind, text) in self.rt.take_logs() {
            assert!(kind != LogKind::Error, "error: {text}");
            self.logs.push(text);
        }
    }

    fn frames(&mut self, n: usize, keys: &[&str], pad: &[usize]) {
        for _ in 0..n {
            self.frame_with(keys, pad);
        }
    }

    fn idle(&mut self, n: usize) {
        self.frames(n, &[], &[]);
    }

    /// Press and release a key (two frames).
    fn tap_key(&mut self, key: &str) {
        self.frame_with(&[key], &[]);
        self.idle(1);
    }

    /// Press and release a gamepad button (two frames).
    fn tap_pad(&mut self, button: usize) {
        self.frame_with(&[], &[button]);
        self.idle(1);
    }

    fn find(&self, name: &str) -> Entity {
        self.scene
            .find_by_name(name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    fn active(&self, name: &str) -> bool {
        self.scene.active_in_hierarchy(self.find(name))
    }

    fn text(&self, name: &str) -> String {
        self.scene
            .world
            .get::<&UiText>(self.find(name))
            .unwrap()
            .text
            .clone()
    }

    fn selected(&self) -> Option<String> {
        self.rt
            .ui_interaction()
            .selected
            .map(|e| self.scene.name(e).to_owned())
    }

    fn pos(&self, name: &str) -> Vec3 {
        self.scene.world_position(self.find(name))
    }

    fn players(&self) -> Vec<Entity> {
        self.scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| self.scene.world.get::<&PlayerInput>(*e).is_ok())
            .collect()
    }

    fn viewport(&self, player: &str) -> [f32; 4] {
        let cam = self.scene.children(self.find(player))[1];
        assert_eq!(self.scene.name(cam), "Camera");
        self.scene.world.get::<&Camera>(cam).unwrap().viewport
    }

    /// The Base Layer's state on a player's body.
    fn anim_state(&self, player: &str) -> String {
        let body = self.scene.children(self.find(player))[0];
        self.rt
            .animator_status(&self.scene, body)
            .unwrap()
            .current
            .unwrap()
            .name
    }

    /// Walk "Player 1" with WASD towards `to` (x, z) until within `near`, at most `max` frames;
    /// returns the frames taken.
    fn keys_to(&mut self, to: Vec2, near: f32, max: usize) -> usize {
        for i in 0..max {
            let p = self.pos("Player 1");
            let d = to - Vec2::new(p.x, p.z);
            if d.length() < near {
                self.idle(1);
                return i;
            }
            let mut keys = Vec::new();
            if d.x > near * 0.5 {
                keys.push("d");
            } else if d.x < -near * 0.5 {
                keys.push("a");
            }
            if d.y > near * 0.5 {
                keys.push("w");
            } else if d.y < -near * 0.5 {
                keys.push("s");
            }
            self.frame_with(&keys, &[]);
        }
        panic!(
            "Player 1 didn't reach {to} (at {}, logs {:?})",
            self.pos("Player 1"),
            self.logs
        );
    }

    /// Steer "Player 2" with the left stick towards `to` (x, z).
    fn stick_to(&mut self, to: Vec2, near: f32, max: usize) -> usize {
        for i in 0..max {
            let p = self.pos("Player 2");
            let d = to - Vec2::new(p.x, p.z);
            if d.length() < near {
                self.stick = Vec2::ZERO;
                self.idle(1);
                return i;
            }
            self.stick = d.normalize();
            self.frame_with(&[], &[]);
        }
        panic!("Player 2 didn't reach {to} (at {})", self.pos("Player 2"));
    }

    /// From the menu: Play with the gamepad, then the keyboard player joins with Space.
    fn play_one(&mut self) {
        self.tap_pad(SOUTH);
        assert!(!self.active("Menu Panel"));
        self.tap_key("space");
        assert_eq!(self.players().len(), 1);
        self.idle(30);
    }

    fn pickups_left(&self) -> usize {
        (1..=PICKUPS.len())
            .filter(|i| self.active(&format!("Pickup {i}")))
            .count()
    }

    fn feet_above_ground(&self, ground: impl Fn(Vec3) -> f32) -> [f32; 2] {
        ["LeftFoot", "RightFoot"].map(|f| {
            // The sole is 0.03 below the ankle.
            let p = self.scene.world_position(self.find(f));
            p.y - 0.03 - ground(p)
        })
    }
}

#[test]
fn hill_run_menu_joins_two_players_with_split_screen_and_races() {
    let Some(mut g) = Game::start() else {
        return;
    };
    // The start menu, Play selected and highlighted.
    assert!(g.active("Menu Panel"));
    assert!(!g.active("HUD"));
    assert_eq!(g.selected().as_deref(), Some("Play Button"));
    let play = g.find("Play Button");
    let quit = g.find("Quit Button");
    assert!((g.scene.transform(play).scale.x - 1.1).abs() < 1e-4);
    // The d-pad moves the selection and the highlight.
    g.tap_pad(DPAD_DOWN);
    assert_eq!(g.selected().as_deref(), Some("Quit Button"));
    assert!((g.scene.transform(quit).scale.x - 1.1).abs() < 1e-4);
    assert!((g.scene.transform(play).scale.x - 1.0).abs() < 1e-4);
    let quit_label = g.scene.children(quit)[0];
    assert_eq!(
        g.scene.world.get::<&UiText>(quit_label).unwrap().color,
        [1.0, 0.92, 0.016, 1.0]
    );
    g.tap_pad(DPAD_UP);
    assert_eq!(g.selected().as_deref(), Some("Play Button"));
    // A presses Play: the game starts and waits for players.
    g.tap_pad(SOUTH);
    assert!(!g.active("Menu Panel"));
    assert!(g.active("HUD"));
    assert_eq!(g.text("Status"), "6 pickups left");
    assert_eq!(g.text("P1 Score"), "P1: press a key");
    assert!(
        g.players().is_empty(),
        "the A that pressed Play doesn't join"
    );
    let lobby = g.find("Lobby Camera");
    assert!(g.scene.world.get::<&Camera>(lobby).unwrap().enabled);

    // Space joins the keyboard player at the first spawn, with the whole screen.
    g.tap_key("space");
    assert_eq!(g.players().len(), 1);
    assert_eq!(g.viewport("Player 1"), [0.0, 0.0, 1.0, 1.0]);
    assert!(!g.scene.world.get::<&Camera>(lobby).unwrap().enabled);
    assert_eq!(g.text("P1 Score"), "P1  0");
    // A joins the gamepad player: side by side.
    g.tap_pad(SOUTH);
    assert_eq!(g.players().len(), 2);
    assert_eq!(g.viewport("Player 1"), [0.0, 0.0, 0.5, 1.0]);
    assert_eq!(g.viewport("Player 2"), [0.5, 0.0, 0.5, 1.0]);
    assert_eq!(g.text("P2 Score"), "P2  0");
    g.idle(30);
    for (i, spawn) in SPAWNS.iter().enumerate() {
        let p = g.pos(&format!("Player {}", i + 1));
        assert!(
            (Vec2::new(p.x, p.z) - Vec2::new(spawn.x, spawn.z)).length() < 0.05 && p.y.abs() < 0.15,
            "player {} at its spawn: {p}",
            i + 1
        );
    }

    // Player 1 walks up the hill: the first pickup, the ramp, the top.
    g.keys_to(Vec2::new(-1.5, 5.0), 0.3, 120);
    assert_eq!(g.text("P1 Score"), "P1  1");
    assert_eq!(g.text("Status"), "5 pickups left");
    g.keys_to(Vec2::new(-1.5, 22.0), 0.3, 600);
    let top = g.pos("Player 1");
    assert!((top.y - top_height()).abs() < 0.15, "on the top: {top}");
    assert_eq!(g.text("P1 Score"), "P1  3");

    // Player 2: walking under the high pickup misses it; jumping takes it.
    g.stick_to(Vec2::new(4.0, 0.0), 0.15, 120);
    assert!(g.active("Pickup 2"), "out of reach without jumping");
    let mut highest = 0.0f32;
    g.frame_with(&[], &[SOUTH]);
    for _ in 0..40 {
        g.idle(1);
        highest = highest.max(g.pos("Player 2").y);
    }
    assert!(highest > 1.0 && highest < 1.4, "jumped {highest}");
    assert!(!g.active("Pickup 2"));
    assert_eq!(g.text("P2 Score"), "P2  1");
    // The pickup by the steep ramp, then the ramp itself: too steep to walk up.
    g.stick_to(Vec2::new(6.0, 6.0), 0.2, 200);
    assert_eq!(g.text("P2 Score"), "P2  2");
    g.stick = Vec2::new(0.0, 1.0);
    g.frames(120, &[], &[]);
    g.stick = Vec2::ZERO;
    let p = g.pos("Player 2");
    assert!(p.y < 0.6 && p.z < 9.6, "the steep ramp blocks: {p}");
    // Round the hill to the steps and up to the landing.
    g.stick_to(Vec2::new(6.0, 5.0), 0.2, 200);
    g.stick_to(Vec2::new(-7.0, 5.0), 0.2, 400);
    // The landing's pickup is the last one: touching it ends the race.
    g.stick = Vec2::new(0.0, 1.0);
    for _ in 0..300 {
        if g.pickups_left() == 0 {
            break;
        }
        g.idle(1);
    }
    g.stick = Vec2::ZERO;
    g.idle(1);
    assert_eq!(g.pickups_left(), 0);
    let p = g.pos("Player 2");
    assert!((p.y - 1.0).abs() < 0.12, "climbed the steps: {p}");
    // Nobody moves once it's over; a draw.
    g.stick = Vec2::new(0.0, 1.0);
    g.frames(20, &[], &[]);
    g.stick = Vec2::ZERO;
    assert!((g.pos("Player 2") - p).length() < 0.01);
    assert!(g.active("Win Panel"));
    assert_eq!(g.text("Win Text"), "Draw!  3 - 3");
    assert_eq!(g.selected().as_deref(), Some("Again Button"));
    // Main Menu loads the scene again.
    g.tap_pad(SOUTH);
    let requests = g.rt.take_requests();
    assert!(
        requests
            .iter()
            .any(|r| matches!(r, Request::LoadScene(SceneRef::Name(n)) if n == "HillRun")),
        "{requests:?}"
    );
}

#[test]
fn hill_run_pause_stops_time_and_resumes_from_the_menu() {
    let Some(mut g) = Game::start() else {
        return;
    };
    g.play_one();
    let start = g.pos("Player 1");
    // Esc pauses: Time.timeScale is 0, so holding W doesn't walk and the pickups stop spinning.
    g.tap_key("escape");
    assert!(g.active("Pause Panel"));
    assert_eq!(g.selected().as_deref(), Some("Resume Button"));
    let spin = g.scene.transform(g.find("Pickup 1")).rotation;
    let body = g.scene.children(g.find("Player 1"))[0];
    let anim_time = |g: &Game| {
        g.rt.animator_status(&g.scene, body)
            .unwrap()
            .current
            .unwrap()
            .normalized_time
    };
    let t0 = anim_time(&g);
    g.frames(30, &["w"], &[]);
    let still = g.pos("Player 1");
    assert!((still - start).length() < 0.01, "paused: {start} → {still}");
    assert_eq!(g.scene.transform(g.find("Pickup 1")).rotation, spin);
    assert!((anim_time(&g) - t0).abs() < 1e-5, "the animator waits");
    g.idle(1);
    // The pause menu works with the keyboard: Enter on Resume.
    g.tap_key("return");
    assert!(!g.active("Pause Panel"));
    g.frames(30, &["w"], &[]);
    assert!(g.pos("Player 1").z - start.z > 1.0, "walks again");
    // The gamepad's Start pauses as well (it's the keyboard player's game, but the pause
    // menu is anyone's); Down + A on Main Menu asks for the scene again.
    g.idle(1);
    g.tap_key("escape");
    assert!(g.active("Pause Panel"));
    g.tap_pad(DPAD_DOWN);
    assert_eq!(g.selected().as_deref(), Some("Menu Button"));
    g.tap_pad(SOUTH);
    let requests = g.rt.take_requests();
    assert!(
        requests
            .iter()
            .any(|r| matches!(r, Request::LoadScene(SceneRef::Name(n)) if n == "HillRun")),
        "{requests:?}"
    );
}

#[test]
fn hill_run_animator_follows_speed_and_feet_stay_on_the_ground() {
    let Some(mut g) = Game::start() else {
        return;
    };
    g.play_one();
    assert_eq!(g.anim_state("Player 1"), "Idle");
    // Standing on the flat: both soles on the ground.
    for h in g.feet_above_ground(|_| 0.0) {
        assert!(h.abs() < 0.03, "sole {h} above the ground");
    }
    // Running on the flat (4 m/s): Run (through Walk; the fades take a moment).
    g.frames(40, &["w"], &[]);
    assert_eq!(g.anim_state("Player 1"), "Run");
    // Every frame walks the whole step (Move pressing into the ground used to stall now and
    // then, dropping the speed to 0 for a frame).
    for _ in 0..10 {
        let a = g.pos("Player 1");
        g.frames(1, &["w"], &[]);
        let step = g.pos("Player 1").z - a.z;
        assert!((step - 4.0 / 60.0).abs() < 2e-3, "step {step}");
    }
    let speed_flat = {
        let a = g.pos("Player 1");
        g.frames(10, &["w"], &[]);
        (g.pos("Player 1") - a).length() / (10.0 / 60.0)
    };
    assert!((speed_flat - 4.0).abs() < 0.3, "flat speed {speed_flat}");
    // Up the ramp: slower (Slope Speed), still grounded, and Walk.
    g.keys_to(Vec2::new(-1.5, 10.0), 0.3, 300);
    g.frames(10, &["w"], &[]);
    let a = g.pos("Player 1");
    g.frames(20, &["w"], &[]);
    let b = g.pos("Player 1");
    let horizontal = Vec2::new(b.x - a.x, b.z - a.z).length() / (20.0 / 60.0);
    assert!(
        horizontal < speed_flat * 0.8 && horizontal > speed_flat * 0.3,
        "uphill {horizontal} vs flat {speed_flat}"
    );
    assert!(b.y > a.y, "climbing");
    assert_eq!(g.anim_state("Player 1"), "Walk");
    // Standing still on the ramp: Idle, and foot IK puts each sole on the slope under it.
    g.idle(40);
    assert_eq!(g.anim_state("Player 1"), "Idle");
    let (foot, _) = ramp_span();
    let tan = RAMP_DEG.to_radians().tan();
    for h in g.feet_above_ground(|p| (p.z - foot) * tan) {
        assert!(h.abs() < 0.04, "sole {h} above the slope");
    }
}
