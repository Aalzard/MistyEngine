//! `editor.*`: menu commands, Play mode, the Console, screenshots, tests, builds and undo.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use misty_engine::testing::{RunContext, Status, Summary, TestMode, TestRun};
use serde_json::{Value, json};

use super::scene::last_log;
use super::{
    Cx, Job, Kind, Outcome, Tool, ToolResult, object_schema, opt_bool, opt_str, opt_u64, str_arg,
    summary,
};
use crate::actions::Action;
use crate::console::LogLevel;
use crate::search::menu_items::{self, MenuRun};
use crate::state::{EditorState, PlayState};

const LIVE_ONLY: &str = "Play mode needs the running editor: connect through the live editor (misty --mcp-connect) instead of misty --mcp";

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "editor.get_state",
        title: "Editor state",
        description: "Project folder, open scenes and unsaved changes, Play mode, selection, undo/redo steps and Console counts.",
        kind: Kind::Read,
        idempotent: true,
        schema: empty_schema,
        run: get_state,
    },
    Tool {
        name: "editor.list_menu_items",
        title: "List menu items",
        description: "Menu bar commands (\"Object/3D Object/Cube\", \"Assets/Refresh\", editor script items…) that editor.run_menu_item runs.",
        kind: Kind::Read,
        idempotent: true,
        schema: list_menu_schema,
        run: list_menu_items,
    },
    Tool {
        name: "editor.run_menu_item",
        title: "Run menu item",
        description: "Run a menu bar command by its path (see editor.list_menu_items). Returns the Console lines it logged. Commands that close the editor are refused; File > Build asks the user first.",
        kind: Kind::Edit,
        idempotent: false,
        schema: run_menu_schema,
        run: run_menu_item,
    },
    Tool {
        name: "editor.play",
        title: "Enter Play mode",
        description: "Press Play (live editor only). Changes made while playing are reverted on Stop.",
        kind: Kind::Edit,
        idempotent: true,
        schema: empty_schema,
        run: play,
    },
    Tool {
        name: "editor.stop",
        title: "Stop Play mode",
        description: "Leave Play mode, reverting what changed while playing (live editor only).",
        kind: Kind::Edit,
        idempotent: true,
        schema: empty_schema,
        run: stop,
    },
    Tool {
        name: "editor.pause",
        title: "Pause Play mode",
        description: "Pause (or with \"paused\": false, resume) the game in Play mode (live editor only).",
        kind: Kind::Edit,
        idempotent: true,
        schema: pause_schema,
        run: pause,
    },
    Tool {
        name: "editor.read_console",
        title: "Read Console",
        description: "Console log lines (newest last) with their index. Pass \"since\" (the \"next\" of an earlier call) to read only new lines; \"level\" keeps that level and above.",
        kind: Kind::Read,
        idempotent: true,
        schema: console_schema,
        run: read_console,
    },
    Tool {
        name: "editor.screenshot",
        title: "Screenshot a view",
        description: "A PNG of the Editor Scene (the editor camera) or the Scene Viewer (the scene's main camera).",
        kind: Kind::Read,
        idempotent: true,
        schema: screenshot_schema,
        run: screenshot,
    },
    Tool {
        name: "editor.run_tests",
        title: "Run tests",
        description: "Run the project's Lua tests (EditMode and PlayMode, as the Test Runner does) and return the results with failure messages.",
        kind: Kind::Read,
        idempotent: true,
        schema: tests_schema,
        run: run_tests,
    },
    Tool {
        name: "editor.build",
        title: "Build player",
        description: "Build the game (the active Build Profile's scenes, target and options) into an output folder and return the executable's path. The live editor asks the user first.",
        kind: Kind::Destructive,
        idempotent: false,
        schema: build_schema,
        run: build,
    },
    Tool {
        name: "editor.undo",
        title: "Undo",
        description: "Undo the last edits (one step by default), whoever made them.",
        kind: Kind::Edit,
        idempotent: false,
        schema: steps_schema,
        run: undo,
    },
    Tool {
        name: "editor.redo",
        title: "Redo",
        description: "Redo undone edits (one step by default).",
        kind: Kind::Edit,
        idempotent: false,
        schema: steps_schema,
        run: redo,
    },
];

// ---- schemas ---------------------------------------------------------------------------------

fn empty_schema() -> Value {
    object_schema(json!({}), &[])
}

fn list_menu_schema() -> Value {
    object_schema(
        json!({
            "filter": { "type": "string", "description": "Substring of the path or title" },
            "limit": { "type": "integer", "minimum": 1, "description": "Most results (default 300)" },
        }),
        &[],
    )
}

fn run_menu_schema() -> Value {
    object_schema(
        json!({ "path": { "type": "string", "description": "Menu path, like \"Object/3D Object/Cube\" (the old \"GameObject/…\" and \"Component/…\" paths still work)" } }),
        &["path"],
    )
}

fn pause_schema() -> Value {
    object_schema(
        json!({ "paused": { "type": "boolean", "description": "Pause (true, default) or resume (false)" } }),
        &[],
    )
}

fn console_schema() -> Value {
    object_schema(
        json!({
            "since": { "type": "integer", "minimum": 0, "description": "First line index to return" },
            "level": { "type": "string", "enum": ["info", "warning", "error"], "description": "Lowest level to return (default info)" },
            "search": { "type": "string", "description": "Only lines containing this text" },
            "limit": { "type": "integer", "minimum": 1, "description": "Most lines (default 100)" },
        }),
        &[],
    )
}

fn screenshot_schema() -> Value {
    object_schema(
        json!({
            "view": { "type": "string", "enum": ["scene", "game"], "description": "Which view (default scene)" },
            "width": { "type": "integer", "minimum": 16, "maximum": 2048, "description": "Width in pixels (default 960, or the view's size in the live editor)" },
            "height": { "type": "integer", "minimum": 16, "maximum": 2048, "description": "Height in pixels (default 540)" },
        }),
        &[],
    )
}

fn tests_schema() -> Value {
    object_schema(
        json!({
            "filter": { "type": "string", "description": "Case-insensitive substring of \"file::Suite > test\"" },
            "mode": { "type": "string", "enum": ["EditMode", "PlayMode"], "description": "Only this kind of test" },
        }),
        &[],
    )
}

fn build_schema() -> Value {
    object_schema(
        json!({ "output": { "type": "string", "description": "Absolute path of the folder to build into" } }),
        &["output"],
    )
}

fn steps_schema() -> Value {
    object_schema(
        json!({ "steps": { "type": "integer", "minimum": 1, "maximum": 100, "description": "How many steps (default 1)" } }),
        &[],
    )
}

// ---- state ---------------------------------------------------------------------------------------

fn play_state(s: &EditorState) -> &'static str {
    match s.play {
        PlayState::Edit => "edit",
        PlayState::Playing => "playing",
        PlayState::Paused => "paused",
    }
}

fn console_counts(s: &EditorState) -> Value {
    json!({
        "info": s.console.count(LogLevel::Info),
        "warning": s.console.count(LogLevel::Warning),
        "error": s.console.count(LogLevel::Error),
        "total": s.console.entries.len(),
    })
}

fn get_state(cx: &mut Cx<'_>, _args: &Value) -> Result<Outcome, String> {
    let s = &*cx.s;
    let dirty = crate::scene_files::dirty_now(s);
    let active = s.scene.active_scene();
    let scenes: Vec<Value> = s
        .scene
        .loaded_scenes()
        .into_iter()
        .map(|id| {
            json!({
                "name": s.scene.scene_name(id),
                "path": s.scene.scene_path(id),
                "active": id == active,
                "unsaved_changes": dirty.contains(&id),
            })
        })
        .collect();
    let selection: Vec<Value> = s.selection.list().iter().map(|e| summary(s, *e)).collect();
    Ok(Outcome::ok(json!({
        "project": s.project.root.display().to_string(),
        "live_editor": cx.live,
        "play_state": play_state(s),
        "prefab_mode": crate::prefab_mode::open_path(s),
        "scenes": scenes,
        "selection": selection,
        "undo": {
            "undo": s.undo.undo_label(),
            "redo": s.undo.redo_label(),
            "steps": s.undo.steps().len(),
        },
        "console": console_counts(s),
    })))
}

// ---- menus ---------------------------------------------------------------------------------------

fn list_menu_items(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let filter = opt_str(args, "filter").unwrap_or("").to_lowercase();
    let limit = opt_u64(args, "limit").unwrap_or(300) as usize;
    let items: Vec<Value> = menu_items::all(cx.s)
        .into_iter()
        .filter(|m| {
            filter.is_empty()
                || m.path.to_lowercase().contains(&filter)
                || m.title.to_lowercase().contains(&filter)
        })
        .take(limit)
        .map(|m| {
            let mut v = json!({ "path": m.path, "title": m.title });
            if !m.shortcut.is_empty() {
                v["shortcut"] = json!(m.shortcut);
            }
            v
        })
        .collect();
    Ok(Outcome::ok(json!({ "items": items })))
}

/// Whether a menu command may run here, and whether it needs the user's OK.
enum Gate {
    Run,
    Confirm(String),
    Refuse(String),
}

fn gate(s: &EditorState, action: &Action, live: bool) -> Gate {
    let unsaved = !crate::scene_files::dirty_now(s).is_empty();
    match action {
        Action::Quit
        | Action::OpenHub { .. }
        | Action::RebuildAndRestart
        | Action::RestartIntoNewBuild => {
            Gate::Refuse("this command closes or restarts the editor".into())
        }
        Action::BuildAndRun if live => {
            Gate::Confirm("run File > Build (pick a folder and build the game)".into())
        }
        Action::BuildAndRun => {
            Gate::Refuse("builds need a folder: use editor.build with \"output\"".into())
        }
        Action::OpenScene | Action::SaveSceneAs if !live => {
            Gate::Refuse("this opens a file dialog: use scene.open / scene.save with a path".into())
        }
        Action::TogglePlay | Action::TogglePause | Action::Step if !live => {
            Gate::Refuse(LIVE_ONLY.into())
        }
        Action::NewScene | Action::OpenScenePath(_) | Action::ReloadScene if unsaved && !live => {
            Gate::Refuse("there are unsaved changes: save them first (scene.save)".into())
        }
        Action::SaveScene if !live => {
            let untitled = crate::scene_files::dirty_now(s)
                .into_iter()
                .any(|id| s.scene.scene_path(id).is_none());
            if untitled {
                Gate::Refuse("a scene has no file yet: use scene.save with a path".into())
            } else {
                Gate::Run
            }
        }
        Action::DeleteAsset(p) => Gate::Confirm(format!("delete {}", p.display())),
        _ => Gate::Run,
    }
}

/// Window, layout and Play commands have no effect without a window; scene file commands run
/// as their menu items do (the unsaved-changes case was refused before).
fn headless_apply(s: &mut EditorState, action: Action) -> Option<String> {
    use crate::scene_files::{AfterPrompt, perform};
    match action {
        Action::NewScene => {
            perform(s, AfterPrompt::NewScene);
        }
        Action::OpenScenePath(rel) => {
            perform(s, AfterPrompt::Open(rel));
        }
        Action::ReloadScene => {
            perform(s, AfterPrompt::ReloadScenes);
        }
        Action::SaveScene => {
            crate::scene_files::save(s);
        }
        Action::NotImplemented(what) => s.console.warn(format!("'{what}' is not implemented yet.")),
        _ => {
            return Some(
                "this command only changes the editor window, which misty --mcp has not".into(),
            );
        }
    }
    None
}

fn run_menu_item(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let path = str_arg(args, "path")?.trim().trim_matches('/');
    let norm = |p: &str| p.replace(" > ", "/").replace('>', "/").to_lowercase();
    let wanted = norm(path);
    // Paths with the names from before round 31 still run ("GameObject/3D Object/Cube").
    let renamed = norm(&crate::renames::menu_path(&norm(path)));
    let items = menu_items::all(cx.s);
    let Some(item) = items
        .iter()
        .find(|m| norm(&m.path) == wanted)
        .or_else(|| items.iter().find(|m| norm(&m.path) == renamed))
    else {
        let last = wanted.rsplit('/').next().unwrap_or(&wanted).to_owned();
        let close: Vec<&str> = items
            .iter()
            .filter(|m| m.path.to_lowercase().contains(&last))
            .map(|m| m.path.as_str())
            .take(8)
            .collect();
        return Err(if close.is_empty() {
            format!("no menu item \"{path}\" (see editor.list_menu_items)")
        } else {
            format!(
                "no menu item \"{path}\"; did you mean {}?",
                close.join(", ")
            )
        });
    };
    let item = item.clone();
    let name = item.path.clone();
    let gate = match &item.run {
        MenuRun::Action(a) => gate(cx.s, a, cx.live),
        _ => Gate::Run,
    };
    let run = move |cx: &mut Cx<'_>| -> Outcome {
        let from = cx.s.console.entries.len();
        let mut note = None;
        match item.run.clone() {
            MenuRun::Action(a) => {
                if let Some(rest) = crate::commands::apply(cx.s, a) {
                    if cx.live {
                        // Window and Play commands: applied by the editor later this frame.
                        cx.s.push(rest);
                    } else {
                        note = headless_apply(cx.s, rest);
                    }
                }
            }
            other => menu_items::run(cx.s, &other),
        }
        if cx.live {
            // Report after the frame applied it.
            Outcome::Wait(Box::new(MenuJob {
                path: item.path.clone(),
                from,
                frames: 0,
            }))
        } else {
            Outcome::Done(menu_result(cx.s, &item.path, from, note))
        }
    };
    match gate {
        Gate::Refuse(why) => Err(format!("{name}: {why}")),
        Gate::Confirm(what) => Ok(Outcome::Confirm {
            what,
            then: Box::new(run),
        }),
        Gate::Run => Ok(run(cx)),
    }
}

fn menu_result(s: &EditorState, path: &str, from: usize, note: Option<String>) -> ToolResult {
    let logged: Vec<String> = s.console.entries[from.min(s.console.entries.len())..]
        .iter()
        .map(|e| e.message.clone())
        .collect();
    ToolResult::json(json!({ "ran": path, "console": logged, "note": note }))
}

struct MenuJob {
    path: String,
    from: usize,
    frames: u32,
}

impl Job for MenuJob {
    fn poll(&mut self, cx: &mut Cx<'_>) -> Option<ToolResult> {
        self.frames += 1;
        (self.frames >= 2).then(|| menu_result(cx.s, &self.path, self.from, None))
    }
}

// ---- Play mode -------------------------------------------------------------------------------------

/// Waits until Play mode is entered (or left).
struct PlayJob {
    want: bool,
    until: Instant,
}

impl Job for PlayJob {
    fn poll(&mut self, cx: &mut Cx<'_>) -> Option<ToolResult> {
        if cx.s.play.in_play_mode() == self.want {
            return Some(ToolResult::json(json!({ "play_state": play_state(cx.s) })));
        }
        (Instant::now() > self.until).then(|| ToolResult::error(last_log(cx.s)))
    }
}

fn toggle_play(cx: &mut Cx<'_>, want: bool) -> Result<Outcome, String> {
    if !cx.live {
        return Err(LIVE_ONLY.into());
    }
    if cx.s.play.in_play_mode() == want {
        return Ok(Outcome::ok(json!({ "play_state": play_state(cx.s) })));
    }
    if want && crate::prefab_mode::active(cx.s) {
        return Err("the editor is in Prefab Mode; leave it before playing".into());
    }
    cx.s.push(Action::TogglePlay);
    Ok(Outcome::Wait(Box::new(PlayJob {
        want,
        until: Instant::now() + Duration::from_secs(20),
    })))
}

fn play(cx: &mut Cx<'_>, _args: &Value) -> Result<Outcome, String> {
    toggle_play(cx, true)
}

fn stop(cx: &mut Cx<'_>, _args: &Value) -> Result<Outcome, String> {
    toggle_play(cx, false)
}

fn pause(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    if !cx.live {
        return Err(LIVE_ONLY.into());
    }
    if !cx.s.play.in_play_mode() {
        return Err("not in Play mode (editor.play first)".into());
    }
    cx.s.play = if opt_bool(args, "paused").unwrap_or(true) {
        PlayState::Paused
    } else {
        PlayState::Playing
    };
    Ok(Outcome::ok(json!({ "play_state": play_state(cx.s) })))
}

// ---- Console ---------------------------------------------------------------------------------------

fn rank(level: LogLevel) -> u8 {
    match level {
        LogLevel::Info => 0,
        LogLevel::Warning => 1,
        LogLevel::Error => 2,
    }
}

fn read_console(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &*cx.s;
    let min = match opt_str(args, "level") {
        Some("warning") => 1,
        Some("error") => 2,
        _ => 0,
    };
    let search = opt_str(args, "search").unwrap_or("").to_lowercase();
    let limit = opt_u64(args, "limit").unwrap_or(100).max(1) as usize;
    let since = opt_u64(args, "since").map(|n| n as usize);
    let total = s.console.entries.len();
    let picked: Vec<(usize, &crate::console::LogEntry)> = s
        .console
        .entries
        .iter()
        .enumerate()
        .skip(since.unwrap_or(0).min(total))
        .filter(|(_, e)| rank(e.level) >= min)
        .filter(|(_, e)| search.is_empty() || e.message.to_lowercase().contains(&search))
        .collect();
    // From `since`: the oldest lines first; otherwise the newest `limit`.
    let shown: Vec<_> = match since {
        Some(_) => picked.into_iter().take(limit).collect(),
        None => {
            let skip = picked.len().saturating_sub(limit);
            picked.into_iter().skip(skip).collect()
        }
    };
    let next = match (since, shown.last()) {
        (Some(_), Some((i, _))) => i + 1,
        _ => total,
    };
    let entries: Vec<Value> = shown
        .iter()
        .map(|(i, e)| {
            json!({
                "index": i,
                "level": match e.level {
                    LogLevel::Info => "info",
                    LogLevel::Warning => "warning",
                    LogLevel::Error => "error",
                },
                "time": e.time,
                "message": e.message,
            })
        })
        .collect();
    Ok(Outcome::ok(json!({
        "entries": entries,
        "next": next,
        "counts": console_counts(s),
    })))
}

// ---- screenshots -------------------------------------------------------------------------------

fn screenshot(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let view = match opt_str(args, "view") {
        Some("game") => super::super::shot::View::Game,
        _ => super::super::shot::View::Scene,
    };
    let size = match (opt_u64(args, "width"), opt_u64(args, "height")) {
        (None, None) => None,
        (w, h) => Some((
            w.unwrap_or(960).clamp(16, 2048) as u32,
            h.unwrap_or(540).clamp(16, 2048) as u32,
        )),
    };
    let (w, h, rgba) = cx.shots.capture(cx.s, view, size)?;
    let png = super::super::png(w, h, &rgba)?;
    let name = match view {
        super::super::shot::View::Scene => "Editor Scene",
        super::super::shot::View::Game => "Scene Viewer",
    };
    Ok(Outcome::Done(ToolResult::image(
        &png,
        format!("{name}, {w}x{h}"),
    )))
}

// ---- tests ---------------------------------------------------------------------------------------

struct TestJob {
    run: TestRun,
    slice: Duration,
}

impl Job for TestJob {
    fn poll(&mut self, _cx: &mut Cx<'_>) -> Option<ToolResult> {
        if !self.run.step(self.slice) {
            return None;
        }
        let results = self.run.results();
        let sum = Summary::of(results.iter().map(|(_, r)| r));
        let tests: Vec<Value> = results
            .iter()
            .map(|(c, r)| {
                let status = match r.status {
                    Status::Passed => "passed",
                    Status::Failed => "failed",
                    Status::Skipped => "skipped",
                    Status::NotRun | Status::Running => "not run",
                };
                let mut v = json!({
                    "id": c.id,
                    "mode": c.mode.label(),
                    "status": status,
                    "seconds": r.duration,
                });
                if let Some(f) = &r.failure {
                    v["failure"] = json!(f.full_message());
                }
                v
            })
            .collect();
        let mut out = ToolResult::json(json!({
            "passed": sum.passed,
            "failed": sum.failed,
            "skipped": sum.skipped,
            "total": sum.total(),
            "seconds": self.run.elapsed(),
            "tests": tests,
        }));
        // A failing run is still a successful tool call: the results say what failed.
        out.is_error = false;
        Some(out)
    }
}

fn run_tests(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let root = cx.s.project.root.clone();
    let rc = RunContext::load(&root)?;
    let files = misty_engine::testing::discover(&root);
    let filter = opt_str(args, "filter").unwrap_or("").to_owned();
    let mode = match opt_str(args, "mode") {
        Some("EditMode") => Some(TestMode::EditMode),
        Some("PlayMode") => Some(TestMode::PlayMode),
        _ => None,
    };
    let run = TestRun::new(rc, &files, |c| {
        c.matches(&filter) && mode.is_none_or(|m| m == c.mode)
    });
    if run.progress().1 == 0 {
        return Ok(Outcome::ok(json!({
            "passed": 0, "failed": 0, "skipped": 0, "total": 0,
            "note": "no tests matched (tests live in Assets/Tests, see Assets > Create > Testing)",
        })));
    }
    // The live editor keeps drawing between slices.
    let slice = Duration::from_millis(if cx.live { 25 } else { 250 });
    Ok(Outcome::Wait(Box::new(TestJob { run, slice })))
}

// ---- builds ----------------------------------------------------------------------------------------

struct BuildJob {
    job: crate::build_pipeline::BuildJob,
    out: PathBuf,
}

impl Job for BuildJob {
    fn poll(&mut self, cx: &mut Cx<'_>) -> Option<ToolResult> {
        let result = self.job.poll()?;
        let warnings: Vec<&str> = self
            .job
            .log
            .iter()
            .filter(|l| l.warning)
            .map(|l| l.text.as_str())
            .collect();
        Some(match result {
            Ok(built) => {
                cx.s.console.info(format!(
                    "Build succeeded: {} ({} assets, {} warnings)",
                    built.exe.display(),
                    built.files,
                    warnings.len()
                ));
                ToolResult::json(json!({
                    "exe": built.exe.display().to_string(),
                    "folder": self.out.display().to_string(),
                    "files": built.files,
                    "warnings": warnings,
                }))
            }
            Err(e) => {
                cx.s.console
                    .log(LogLevel::Error, format!("Build failed: {e}"));
                let tail: Vec<&str> = self
                    .job
                    .log
                    .iter()
                    .rev()
                    .take(20)
                    .map(|l| l.text.as_str())
                    .collect();
                ToolResult::error(format!(
                    "build failed: {e}\n{}",
                    tail.into_iter().rev().collect::<Vec<_>>().join("\n")
                ))
            }
        })
    }
}

fn build(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let out = PathBuf::from(str_arg(args, "output")?);
    if !out.is_absolute() {
        return Err("\"output\" must be an absolute folder path".into());
    }
    if out.starts_with(cx.s.project.root.join("Assets")) {
        return Err("build into a folder outside Assets/".into());
    }
    if cx.s.play.in_play_mode() {
        return Err("stop Play mode before building".into());
    }
    let what = format!("build the game into {}", out.display());
    Ok(Outcome::Confirm {
        what,
        then: Box::new(move |cx| match start_build(cx.s, out) {
            Ok(job) => Outcome::Wait(Box::new(job)),
            Err(e) => Outcome::Done(ToolResult::error(e)),
        }),
    })
}

fn start_build(s: &mut EditorState, out: PathBuf) -> Result<BuildJob, String> {
    if s.build_window
        .job
        .as_ref()
        .is_some_and(crate::build_pipeline::BuildJob::running)
    {
        return Err("a build is already running".into());
    }
    if s.build_settings.enabled().next().is_none() {
        crate::build_settings::add_open_scene(s);
    }
    if s.build_settings.enabled().next().is_none() {
        return Err("no scenes in Build Settings (save the scene first)".into());
    }
    if s.scene_dirty {
        s.console
            .warn("An open scene has unsaved changes; the build uses the saved files.");
    }
    s.console.info(format!(
        "Building for {} into {}…",
        s.build_settings.target.label(),
        out.display()
    ));
    let req = crate::build_settings::request(s, &out);
    Ok(BuildJob {
        job: crate::build_pipeline::BuildJob::start(req, false),
        out,
    })
}

// ---- undo ----------------------------------------------------------------------------------------

fn undo_redo(cx: &mut Cx<'_>, args: &Value, redo: bool) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let steps = opt_u64(args, "steps").unwrap_or(1).clamp(1, 100);
    let mut done = Vec::new();
    for _ in 0..steps {
        // Undone values are not new edits for the Animation window's Record mode.
        s.animation.skip_record = true;
        let step = if redo { s.redo_step() } else { s.undo_step() };
        match step {
            Some(label) => {
                s.console
                    .info(format!("{} {label}", if redo { "Redo" } else { "Undo" }));
                done.push(label);
            }
            None => break,
        }
    }
    if done.is_empty() {
        return Err(format!("nothing to {}", if redo { "redo" } else { "undo" }));
    }
    let mut out = json!({
        "next_undo": s.undo.undo_label(),
        "next_redo": s.undo.redo_label(),
    });
    out[if redo { "redone" } else { "undone" }] = json!(done);
    Ok(Outcome::ok(out))
}

fn undo(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    undo_redo(cx, args, false)
}

fn redo(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    undo_redo(cx, args, true)
}
