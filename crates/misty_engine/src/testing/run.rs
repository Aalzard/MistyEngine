//! Running a selection of tests a slice at a time.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use glam::Vec2;
use mlua::{
    Function, HookTriggers, IntoLuaMulti, Lua, MultiValue, Table, Thread, ThreadStatus, Value,
    VmState,
};

use super::{
    Failure, SimInput, Status, TestCase, TestFile, TestMode, TestResult, framework, load_file,
    lua_message, value_message,
};
use crate::audio::Audio;
use crate::build::{BuildSettings, SceneRef};
use crate::physics::Physics;
use crate::physics2d::Physics2D;
use crate::runtime::{Runtime, Time};
use crate::scene::Scene;
use crate::scripting::{LogKind, Request, World};
use crate::settings::ProjectSettings;

/// Game time per PlayMode test frame (tests are deterministic, whatever the machine).
pub const FRAME_DT: f32 = 1.0 / 60.0;
/// Longest game time a PlayMode test may run unless it calls `setTimeout`.
pub const DEFAULT_TIMEOUT: f64 = 30.0;
/// Longest wall-clock time Lua may run without yielding (infinite loops).
pub const RESUME_LIMIT: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });
/// Screen size PlayMode games see (UI layout).
const SCREEN: Vec2 = Vec2::new(1280.0, 720.0);

/// What tests run against.
pub struct RunContext {
    pub root: PathBuf,
    pub settings: ProjectSettings,
    pub build: BuildSettings,
    /// The active scene: every EditMode test gets a fresh copy.
    pub scene: Scene,
}

impl RunContext {
    /// The command line's context: settings from disk and the first enabled Build Settings scene
    /// (a new default scene when there is none).
    pub fn load(root: &Path) -> Result<Self, String> {
        let settings = ProjectSettings::load(root)?;
        let build = BuildSettings::load(root)?;
        let scene = match build.resolve(&SceneRef::Index(0)) {
            Ok((rel, _)) => {
                let (mut scene, _) = Scene::load(&root.join(&rel))?;
                scene.set_scene_path(0, &rel);
                scene
            }
            Err(_) => Scene::default_3d("Untitled"),
        };
        Ok(Self {
            root: root.to_path_buf(),
            settings,
            build,
            scene,
        })
    }

    /// Project-relative scene file for `loadScene(name)`: a path, a Build Settings scene, or any
    /// `<name>.scene` under `Assets`.
    pub fn resolve_scene(&self, name: &str) -> Result<String, String> {
        let direct = name.replace('\\', "/");
        if direct.ends_with(".scene") && self.root.join(&direct).is_file() {
            return Ok(direct);
        }
        if let Ok((rel, _)) = self.build.resolve(&SceneRef::Name(name.to_owned())) {
            return Ok(rel);
        }
        let stem = direct.trim_end_matches(".scene");
        let stem = stem.rsplit('/').next().unwrap_or(stem);
        let mut found = Vec::new();
        find_scenes(&self.root, &self.root.join("Assets"), stem, &mut found);
        found.sort();
        found.into_iter().next().ok_or_else(|| {
            format!("loadScene: no scene called '{name}' (looked in Build Settings and Assets)")
        })
    }
}

fn find_scenes(root: &Path, dir: &Path, stem: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_scenes(root, &path, stem, out);
        } else if path.extension().is_some_and(|e| e == "scene")
            && path.file_stem().is_some_and(|s| s == stem)
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Wall-clock limit for the Lua code running now (checked by an instruction-count hook).
struct Deadline(Cell<Option<Instant>>);

fn arm(lua: &Lua, limit: Option<Duration>) {
    if let Some(d) = lua.app_data_ref::<Deadline>() {
        d.0.set(limit.map(|l| Instant::now() + l));
    }
}

/// Stop runaway Lua (`while true do end`) instead of freezing the editor.
pub(super) fn install_deadline(lua: &Lua) {
    lua.set_app_data(Deadline(Cell::new(None)));
    let _ = lua.set_global_hook(
        HookTriggers::new().every_nth_instruction(10_000),
        |lua, _| {
            if let Some(d) = lua.app_data_ref::<Deadline>()
                && let Some(t) = d.0.get()
                && Instant::now() > t
            {
                // A little more time so afterEach hooks can still clean up.
                d.0.set(Some(Instant::now() + Duration::from_secs(1)));
                return Err(mlua::Error::runtime(format!(
                    "the test ran for over {} s without yielding (an infinite loop?)",
                    RESUME_LIMIT.as_secs()
                )));
            }
            Ok(VmState::Continue)
        },
    );
}

/// Run Lua with the deadline armed.
pub(super) fn guarded<R>(lua: &Lua, f: impl FnOnce() -> R) -> R {
    arm(lua, Some(RESUME_LIMIT));
    let r = f();
    arm(lua, None);
    r
}

/// A play session: the scene and the game running on it.
struct Session {
    scene: Scene,
    runtime: Runtime,
    time: Time,
}

impl Session {
    fn start(cx: &RunContext, mut scene: Scene) -> Self {
        let (audio, _) = Audio::null(cx.root.clone());
        let runtime = Runtime::start_with_audio(&mut scene, cx.root.clone(), &cx.settings, audio);
        Self {
            scene,
            runtime,
            time: Time::new(&cx.settings.time),
        }
    }
}

/// The game world an EditMode test sees: `scene` with physics for queries, nothing playing.
fn edit_world(cx: &RunContext, scene: Scene) -> World {
    let physics = Physics::build_with(&scene, &cx.settings.physics);
    let physics2d = Physics2D::build(&scene, &cx.settings.physics2d, &cx.settings.physics);
    let (audio, _) = Audio::null(cx.root.clone());
    World::new(
        scene,
        physics,
        physics2d,
        cx.root.clone(),
        audio,
        &cx.settings,
    )
}

struct FilePlan {
    rel: String,
    mode: TestMode,
    tests: Vec<TestCase>,
}

/// What a test's coroutine did when resumed.
enum Yielded {
    Frames(u32),
    Load(String),
    Done(Table),
    Error(String),
}

struct ActiveTest {
    case: TestCase,
    lua_index: usize,
    next_index: Option<usize>,
    thread: Thread,
    session: Option<Session>,
    /// Frames left before the coroutine resumes.
    wait: u32,
    frames: u64,
    timeout: f64,
    started: Instant,
    logs: Vec<String>,
    errors: Vec<String>,
}

/// A test file being run: its Lua state (shared by its tests, like a test class) and position.
struct OpenFile {
    plan: FilePlan,
    lua: Lua,
    /// Test id → Lua index.
    index: HashMap<String, usize>,
    pos: usize,
    test: Option<ActiveTest>,
}

/// A run of selected tests. Call [`TestRun::step`] until it reports done.
pub struct TestRun {
    cx: RunContext,
    plan: VecDeque<FilePlan>,
    open: Option<OpenFile>,
    results: Vec<(TestCase, TestResult)>,
    total: usize,
    started: Instant,
}

impl TestRun {
    /// Run the tests of `files` that `select` picks (EditMode files first, then PlayMode).
    pub fn new(cx: RunContext, files: &[TestFile], select: impl Fn(&TestCase) -> bool) -> Self {
        let mut plan = VecDeque::new();
        let mut total = 0;
        for mode in TestMode::ALL {
            for f in files.iter().filter(|f| f.mode == mode) {
                let tests: Vec<TestCase> = f.tests.iter().filter(|c| select(c)).cloned().collect();
                if tests.is_empty() {
                    continue;
                }
                total += tests.len();
                plan.push_back(FilePlan {
                    rel: f.rel.clone(),
                    mode,
                    tests,
                });
            }
        }
        Self {
            cx,
            plan,
            open: None,
            results: Vec::new(),
            total,
            started: Instant::now(),
        }
    }

    /// Work for about `budget` (at least one unit: open a file, start a test or one game
    /// frame); true once every test has a result.
    pub fn step(&mut self, budget: Duration) -> bool {
        let until = Instant::now() + budget;
        loop {
            if self.is_finished() {
                return true;
            }
            self.advance();
            if Instant::now() >= until {
                return self.is_finished();
            }
        }
    }

    /// Run everything (the command line).
    pub fn run_to_end(&mut self) {
        while !self.step(Duration::from_millis(250)) {}
    }

    pub fn is_finished(&self) -> bool {
        self.open.is_none() && self.plan.is_empty()
    }

    /// Finished tests in the order they ran.
    pub fn results(&self) -> &[(TestCase, TestResult)] {
        &self.results
    }

    /// (tests with a result, tests in the run).
    pub fn progress(&self) -> (usize, usize) {
        (self.results.len(), self.total)
    }

    /// The test running now.
    pub fn current(&self) -> Option<&TestCase> {
        self.open.as_ref()?.test.as_ref().map(|t| &t.case)
    }

    /// Wall-clock seconds since the run started.
    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn advance(&mut self) {
        let Some(open) = self.open.as_mut() else {
            if let Some(plan) = self.plan.pop_front() {
                self.open = OpenFile::open(&self.cx, plan, &mut self.results);
            }
            return;
        };
        if open.advance(&self.cx, &mut self.results) {
            self.open = None;
        }
    }
}

impl OpenFile {
    /// Load the file; when it fails every planned test fails with its error.
    fn open(
        cx: &RunContext,
        plan: FilePlan,
        results: &mut Vec<(TestCase, TestResult)>,
    ) -> Option<Self> {
        match load_file(&cx.root, &plan.rel, plan.mode) {
            Ok((lua, tests)) => {
                let index = tests
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.id.clone(), i + 1))
                    .collect();
                Some(Self {
                    plan,
                    lua,
                    index,
                    pos: 0,
                    test: None,
                })
            }
            Err(f) => {
                for case in plan.tests {
                    results.push((case, TestResult::failed(f.clone())));
                }
                None
            }
        }
    }

    /// One unit of work; true when the file is done.
    fn advance(&mut self, cx: &RunContext, results: &mut Vec<(TestCase, TestResult)>) -> bool {
        if self.test.is_none() {
            if self.pos >= self.plan.tests.len() {
                return true;
            }
            self.start_next(cx, results);
            return false;
        }
        self.frame(cx, results);
        false
    }

    fn start_next(&mut self, cx: &RunContext, results: &mut Vec<(TestCase, TestResult)>) {
        let case = self.plan.tests[self.pos].clone();
        self.pos += 1;
        if case.skipped {
            results.push((case, TestResult::skipped()));
            return;
        }
        let Some(&lua_index) = self.index.get(&case.id) else {
            let failure = Failure {
                message: "the test is no longer in its file (refresh the test list)".into(),
                location: Some((case.file.clone(), case.line)),
                ..Default::default()
            };
            results.push((case, TestResult::failed(failure)));
            return;
        };
        let next_index = self.plan.tests[self.pos..]
            .iter()
            .filter(|c| !c.skipped)
            .find_map(|c| self.index.get(&c.id).copied());
        let thread = framework(&self.lua)
            .and_then(|m| m.get::<Function>("run"))
            .and_then(|f| self.lua.create_thread(f));
        let thread = match thread {
            Ok(t) => t,
            Err(e) => {
                let failure = Failure::from_lua(&e.to_string(), "", &case.file);
                results.push((case, TestResult::failed(failure)));
                return;
            }
        };
        self.lua.set_app_data(SimInput::default());
        let session = match self.plan.mode {
            TestMode::PlayMode => Some(Session::start(cx, Scene::new("Test Scene"))),
            TestMode::EditMode => {
                self.lua.set_app_data(edit_world(cx, cx.scene.clone()));
                None
            }
        };
        self.test = Some(ActiveTest {
            case,
            lua_index,
            next_index,
            thread,
            session,
            wait: 0,
            frames: 0,
            timeout: DEFAULT_TIMEOUT,
            started: Instant::now(),
            logs: Vec::new(),
            errors: Vec::new(),
        });
        let args = (lua_index, next_index)
            .into_lua_multi(&self.lua)
            .unwrap_or_default();
        self.drive(cx, results, args);
    }

    /// Resume the test's coroutine once (with the play session's world lent to it).
    fn resume(&mut self, args: MultiValue) -> Yielded {
        let lua = &self.lua;
        let Some(t) = self.test.as_mut() else {
            return Yielded::Error("no test is running".into());
        };
        let thread = t.thread.clone();
        let call = || guarded(lua, || thread.resume::<Value>(args));
        let r = match t.session.as_mut() {
            Some(s) => s.runtime.lend_world(&mut s.scene, lua, call),
            None => call(),
        };
        match r {
            Err(e) => Yielded::Error(lua_message(&e)),
            Ok(v) if thread.status() == ThreadStatus::Finished => match v {
                Value::Table(t) => Yielded::Done(t),
                other => Yielded::Error(format!(
                    "the test coroutine ended unexpectedly ({})",
                    value_message(lua, &other)
                )),
            },
            Ok(Value::Integer(n)) => Yielded::Frames(n.clamp(1, u32::MAX as i64) as u32),
            Ok(Value::Number(n)) => Yielded::Frames(n.clamp(1.0, u32::MAX as f64) as u32),
            Ok(Value::Table(t)) => match t.get::<Option<String>>("loadScene") {
                Ok(Some(name)) => Yielded::Load(name),
                _ => Yielded::Frames(1),
            },
            Ok(_) => Yielded::Frames(1),
        }
    }

    /// Resume until the test waits for frames or finishes.
    fn drive(
        &mut self,
        cx: &RunContext,
        results: &mut Vec<(TestCase, TestResult)>,
        args: MultiValue,
    ) {
        let mut args = Some(args);
        loop {
            let y = self.resume(args.take().unwrap_or_default());
            self.collect_logs();
            self.read_timeout();
            match y {
                Yielded::Frames(n) => {
                    let Some(t) = self.test.as_mut() else { return };
                    if t.session.is_some() {
                        t.wait = n;
                        return;
                    }
                    // EditMode code never waits for frames (the framework refuses); keep going.
                }
                Yielded::Load(name) => match self.load_scene(cx, &name) {
                    Ok(()) => {
                        let Some(t) = self.test.as_mut() else { return };
                        if t.session.is_some() {
                            // The new scene's Start runs on the next frame, then the test goes on.
                            t.wait = 1;
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = framework(&self.lua).and_then(|m| m.set("loadError", e));
                    }
                },
                Yielded::Done(table) => {
                    self.finish(results, Some(table), None);
                    return;
                }
                Yielded::Error(e) => {
                    self.finish(results, None, Some(e));
                    return;
                }
            }
        }
    }

    fn read_timeout(&mut self) {
        let timeout = framework(&self.lua)
            .and_then(|m| m.get::<Option<f64>>("timeout"))
            .ok()
            .flatten();
        if let Some(t) = self.test.as_mut() {
            t.timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
        }
    }

    /// `loadScene`: PlayMode restarts the session on the scene; EditMode swaps the scratch scene.
    fn load_scene(&mut self, cx: &RunContext, name: &str) -> Result<(), String> {
        let rel = cx.resolve_scene(name)?;
        let (mut scene, _) = Scene::load(&cx.root.join(&rel))?;
        scene.set_scene_path(0, &rel);
        let Some(t) = self.test.as_mut() else {
            return Ok(());
        };
        if t.session.is_some() {
            self.collect_logs();
            if let Some(t) = self.test.as_mut() {
                t.session = Some(Session::start(cx, scene));
            }
            self.collect_logs();
        } else {
            self.lua.set_app_data(edit_world(cx, scene));
        }
        Ok(())
    }

    /// One game frame of the running PlayMode test, then resume it when its wait is over.
    fn frame(&mut self, cx: &RunContext, results: &mut Vec<(TestCase, TestResult)>) {
        let input = self
            .lua
            .app_data_mut::<SimInput>()
            .map(|mut sim| sim.frame(SCREEN))
            .unwrap_or_default();
        let Some(t) = self.test.as_mut() else { return };
        let Some(s) = t.session.as_mut() else {
            // An EditMode test only gets here if it was left waiting; finish it.
            self.finish(
                results,
                None,
                Some("an EditMode test waited for a frame".into()),
            );
            return;
        };
        s.time.advance(FRAME_DT);
        s.runtime.update(&mut s.scene, &s.time, input);
        t.frames += 1;
        let requests = s.runtime.take_requests();
        self.collect_logs();
        for r in requests {
            self.game_request(cx, r);
        }
        let Some(t) = self.test.as_mut() else { return };
        let game_time = t.session.as_ref().map_or(0.0, |s| s.time.time);
        if game_time > t.timeout {
            let message = format!(
                "timed out after {:.1} s of game time (setTimeout(seconds) allows longer)",
                t.timeout
            );
            self.abort();
            self.finish(results, None, Some(message));
            return;
        }
        t.wait = t.wait.saturating_sub(1);
        if t.wait == 0 {
            self.drive(cx, results, MultiValue::new());
        }
    }

    /// `SceneManager.LoadScene` and friends called by the game's scripts during a test.
    fn game_request(&mut self, cx: &RunContext, r: Request) {
        let Some(t) = self.test.as_mut() else { return };
        let Some(s) = t.session.as_mut() else { return };
        let outcome = match &r {
            Request::LoadScene(which) => cx
                .build
                .resolve(which)
                .and_then(|(rel, _)| {
                    let (mut scene, _) = Scene::load(&cx.root.join(&rel))?;
                    scene.set_scene_path(0, &rel);
                    Ok(scene)
                })
                .map(|scene| *s = Session::start(cx, scene)),
            Request::Quit => {
                t.logs.push("Application.Quit() ignored in tests".into());
                Ok(())
            }
            other => s
                .runtime
                .apply_scene_request(&mut s.scene, &cx.build, &cx.root, other),
        };
        if let Err(e) = outcome {
            t.logs.push(format!("Error: {e}"));
            t.errors.push(e);
        }
        self.collect_logs();
    }

    /// Console output of the game and the test since the last call.
    fn collect_logs(&mut self) {
        let Some(t) = self.test.as_mut() else { return };
        let logs = match t.session.as_mut() {
            Some(s) => s.runtime.take_logs(),
            None => self
                .lua
                .app_data_mut::<World>()
                .map(|mut w| std::mem::take(&mut w.logs))
                .unwrap_or_default(),
        };
        for (kind, text) in logs {
            let line = match kind {
                LogKind::Info => text.clone(),
                LogKind::Warning => format!("Warning: {text}"),
                LogKind::Error => format!("Error: {text}"),
            };
            if kind == LogKind::Error {
                t.errors.push(text);
            }
            t.logs.push(line);
        }
    }

    /// Run the abandoned test's afterEach / afterAll hooks outside its coroutine.
    fn abort(&mut self) {
        let lua = &self.lua;
        let Some(t) = self.test.as_mut() else { return };
        let (i, next) = (t.lua_index, t.next_index);
        let call = || {
            guarded(lua, || {
                framework(lua)
                    .and_then(|m| m.get::<Function>("abort"))
                    .and_then(|f| f.call::<String>((i, next)))
            })
        };
        let r = match t.session.as_mut() {
            Some(s) => s.runtime.lend_world(&mut s.scene, lua, call),
            None => call(),
        };
        match r {
            Ok(m) if !m.is_empty() => t.logs.push(format!("Error: {m}")),
            Ok(_) => {}
            Err(e) => t.logs.push(format!("Error: {e}")),
        }
        self.collect_logs();
    }

    /// Record the result and tear the session (or scratch world) down.
    fn finish(
        &mut self,
        results: &mut Vec<(TestCase, TestResult)>,
        table: Option<Table>,
        error: Option<String>,
    ) {
        self.collect_logs();
        let Some(t) = self.test.take() else { return };
        let file = t.case.file.clone();
        let mut failure = error.map(|e| Failure::from_lua(&e, "", &file));
        if let Some(table) = table
            && table.get::<String>("status").is_ok_and(|s| s == "failed")
        {
            let message: String = table
                .get::<Option<String>>("message")
                .ok()
                .flatten()
                .unwrap_or_default();
            let traceback: String = table
                .get::<Option<String>>("traceback")
                .ok()
                .flatten()
                .unwrap_or_default();
            failure = Some(Failure::from_lua(&message, &traceback, &file));
        }
        let ignore = framework(&self.lua)
            .and_then(|m| m.get::<Option<bool>>("ignoreErrors"))
            .ok()
            .flatten()
            .unwrap_or(false);
        if failure.is_none()
            && !ignore
            && let Some(first) = t.errors.first()
        {
            let more = match t.errors.len() {
                1 => String::new(),
                n => format!(" (and {} more)", n - 1),
            };
            let mut f = Failure::from_lua(first, "", &file);
            f.message = format!(
                "error logged during the test{more}: {}\n(call ignoreErrorLogs() if it is expected)",
                f.message
            );
            failure = Some(f);
        }
        let status = if failure.is_some() {
            Status::Failed
        } else {
            Status::Passed
        };
        results.push((
            t.case,
            TestResult {
                status,
                duration: t.started.elapsed().as_secs_f64(),
                failure,
                logs: t.logs,
                frames: t.frames,
            },
        ));
        // The play session ends with the test; EditMode's scratch scene is thrown away.
        drop(t.session);
        self.lua.remove_app_data::<World>();
    }
}
