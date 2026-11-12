//! Test Runner: project tests written in Lua.
//!
//! Test files live under `Assets/Tests/`: `*.test.lua` are EditMode tests (run against a
//! scratch copy of the active scene, no game loop) and `*.playtest.lua` are PlayMode tests (each
//! test gets its own play session that frames are stepped through, torn down after it).
//!
//! ```lua
//! describe("Math", function()
//!     it("adds", function() assert.equal(1 + 1, 2) end)
//! end)
//! ```
//!
//! [`discover`] lists the tests, [`TestRun`] runs a selection of them a slice at a time (the
//! editor's Test Runner window steps it every frame; the command line runs it to the end) and
//! [`junit`] writes the results as JUnit XML for CI.

pub mod cli;
mod input;
pub mod junit;
mod run;

use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};

pub use input::SimInput;
pub use run::{RunContext, TestRun};

/// Where test files are looked for (and where Assets ▸ Create ▸ Testing puts new ones).
pub const TESTS_DIR: &str = "Assets/Tests";
/// File name ending of EditMode test files.
pub const EDIT_SUFFIX: &str = ".test.lua";
/// File name ending of PlayMode test files.
pub const PLAY_SUFFIX: &str = ".playtest.lua";
/// Chunk name of the framework (its frames are left out of tracebacks).
const FRAMEWORK_CHUNK: &str = "@misty/test_framework.lua";
const FRAMEWORK: &str = include_str!("framework.lua");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestMode {
    EditMode,
    PlayMode,
}

impl TestMode {
    pub const ALL: [TestMode; 2] = [TestMode::EditMode, TestMode::PlayMode];

    pub fn label(self) -> &'static str {
        match self {
            TestMode::EditMode => "EditMode",
            TestMode::PlayMode => "PlayMode",
        }
    }

    /// The mode a test file runs in, from its name (`None`: not a test file).
    pub fn of(rel: &str) -> Option<TestMode> {
        if rel.ends_with(PLAY_SUFFIX) {
            Some(TestMode::PlayMode)
        } else if rel.ends_with(EDIT_SUFFIX) {
            Some(TestMode::EditMode)
        } else {
            None
        }
    }
}

/// One test (`it`/`test`) of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct TestCase {
    /// `file::Suite > Inner > name`, unique within the project.
    pub id: String,
    /// Project-relative test file.
    pub file: String,
    pub mode: TestMode,
    /// Enclosing `describe` names, outermost first.
    pub suites: Vec<String>,
    pub name: String,
    /// Line of the `it(...)` call (1-based).
    pub line: u32,
    /// `it.skip` / `describe.skip`, a test without a function, or left out by `.only`.
    pub skipped: bool,
}

impl TestCase {
    /// Case-insensitive match of `filter` against the file, suites and name.
    pub fn matches(&self, filter: &str) -> bool {
        let filter = filter.trim();
        filter.is_empty() || self.id.to_lowercase().contains(&filter.to_lowercase())
    }
}

/// A test file and its tests.
#[derive(Clone, Debug)]
pub struct TestFile {
    pub rel: String,
    pub mode: TestMode,
    pub tests: Vec<TestCase>,
    /// The file failed to load (syntax error, error at the top level).
    pub error: Option<Failure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    NotRun,
    Running,
    Passed,
    Failed,
    Skipped,
}

/// Why a test failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Failure {
    /// The error message, without its `file:line:` prefix.
    pub message: String,
    /// Lua stack traceback (framework frames left out).
    pub traceback: String,
    /// Script and line of the failure (the test file line when it is on the stack).
    pub location: Option<(String, u32)>,
}

impl Failure {
    /// A failure from a Lua error message and traceback; `file` is preferred for the location.
    pub fn from_lua(message: &str, traceback: &str, file: &str) -> Self {
        let traceback = clean_traceback(traceback);
        let location = locate(message, file)
            .or_else(|| locate_in_file(&traceback, file))
            .or_else(|| locate(&traceback, file));
        let mut message = message.trim().to_owned();
        if let Some((rel, line)) = &location {
            let prefix = format!("{rel}:{line}: ");
            if let Some(rest) = message.strip_prefix(&prefix) {
                message = rest.to_owned();
            }
        }
        Self {
            message,
            traceback,
            location,
        }
    }

    /// `file:line: message` (JUnit, the command line).
    pub fn full_message(&self) -> String {
        match &self.location {
            Some((rel, line)) => format!("{rel}:{line}: {}", self.message),
            None => self.message.clone(),
        }
    }
}

/// The outcome of one test.
#[derive(Clone, Debug, PartialEq)]
pub struct TestResult {
    pub status: Status,
    /// Wall-clock seconds.
    pub duration: f64,
    pub failure: Option<Failure>,
    /// What the test and the game logged while it ran.
    pub logs: Vec<String>,
    /// Game frames a PlayMode test stepped through.
    pub frames: u64,
}

impl TestResult {
    pub fn skipped() -> Self {
        Self {
            status: Status::Skipped,
            duration: 0.0,
            failure: None,
            logs: Vec::new(),
            frames: 0,
        }
    }

    pub fn failed(failure: Failure) -> Self {
        Self {
            status: Status::Failed,
            failure: Some(failure),
            ..Self::skipped()
        }
    }
}

/// Counts for the summary bar and the command line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    /// Seconds.
    pub duration: f64,
}

impl Summary {
    pub fn of<'a>(results: impl IntoIterator<Item = &'a TestResult>) -> Self {
        let mut s = Self::default();
        for r in results {
            match r.status {
                Status::Passed => s.passed += 1,
                Status::Failed => s.failed += 1,
                Status::Skipped => s.skipped += 1,
                Status::NotRun | Status::Running => {}
            }
            s.duration += r.duration;
        }
        s
    }

    pub fn total(&self) -> usize {
        self.passed + self.failed + self.skipped
    }
}

/// Every test file under `Assets/Tests` (both modes), sorted by path, with its tests listed.
pub fn discover(root: &Path) -> Vec<TestFile> {
    discover_paths(root)
        .iter()
        .map(|rel| collect(root, rel))
        .collect()
}

/// Project-relative paths of the test files under `Assets/Tests`, sorted.
pub fn discover_paths(root: &Path) -> Vec<String> {
    let mut rels = Vec::new();
    walk(root, &root.join(TESTS_DIR), &mut rels);
    rels.sort();
    rels
}

/// The `Assets/….lua:<line>` a traceback line names.
pub fn line_location(line: &str) -> Option<(String, u32)> {
    let at = line.find("Assets/")?;
    parse_location(&line[at..])
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if TestMode::of(&rel).is_some() {
                out.push(rel);
            }
        }
    }
}

/// List the tests of one file (evaluates it without a running game).
pub fn collect(root: &Path, rel: &str) -> TestFile {
    let mode = TestMode::of(rel).unwrap_or(TestMode::EditMode);
    let mut file = TestFile {
        rel: rel.to_owned(),
        mode,
        tests: Vec::new(),
        error: None,
    };
    match load_file(root, rel, mode) {
        Ok((_, tests)) => file.tests = tests,
        Err(f) => {
            // A file that fails to load shows (and fails) as one test, so runs report it.
            file.tests.push(TestCase {
                id: format!("{rel}::{LOAD_ERROR}"),
                file: rel.to_owned(),
                mode,
                suites: Vec::new(),
                name: LOAD_ERROR.to_owned(),
                line: f.location.as_ref().map_or(1, |(_, l)| *l),
                skipped: false,
            });
            file.error = Some(f);
        }
    }
    file
}

/// Name of the stand-in test of a file that fails to load.
pub const LOAD_ERROR: &str = "(file failed to load)";

/// A Lua state with the game API, the framework and the input helpers, with `rel` evaluated;
/// returns it and the tests the file declared (index `i` is Lua test `i + 1`).
fn load_file(root: &Path, rel: &str, mode: TestMode) -> Result<(Lua, Vec<TestCase>), Failure> {
    // The framework needs `debug` (test lines, tracebacks), which `Lua::new` leaves out; it keeps
    // the functions it uses and removes the global again, so tests see the game's environment.
    // SAFETY: only the framework (our code) touches `debug`, before any test code runs.
    let lua = unsafe {
        Lua::unsafe_new_with(
            crate::scripting::game_libs() | mlua::StdLib::DEBUG,
            mlua::LuaOptions::default(),
        )
    };
    let fail = |e: String| Failure::from_lua(&e, "", rel);
    crate::scripting::sandbox(&lua).map_err(|e| fail(e.to_string()))?;
    crate::scripting::api::register(&lua).map_err(|e| fail(e.to_string()))?;
    lua.load(FRAMEWORK)
        .set_name(FRAMEWORK_CHUNK)
        .exec()
        .map_err(|e| fail(e.to_string()))?;
    input::register(&lua).map_err(|e| fail(e.to_string()))?;
    let source =
        crate::vfs::read_to_string(root.join(rel)).map_err(|e| fail(format!("{rel}: {e}")))?;
    let mode_name = if mode == TestMode::PlayMode {
        "play"
    } else {
        "edit"
    };
    framework(&lua)
        .and_then(|m| m.set("mode", mode_name))
        .map_err(|e| fail(e.to_string()))?;
    run::install_deadline(&lua);
    run::guarded(&lua, || {
        lua.load(&source).set_name(format!("@{rel}")).exec()
    })
    .map_err(|e| fail(lua_message(&e)))?;
    let tests = list(&lua, rel, mode).map_err(|e| fail(e.to_string()))?;
    Ok((lua, tests))
}

/// The framework's `__misty` table.
fn framework(lua: &Lua) -> mlua::Result<Table> {
    lua.globals().get("__misty")
}

fn list(lua: &Lua, rel: &str, mode: TestMode) -> mlua::Result<Vec<TestCase>> {
    let listed: Table = framework(lua)?.get::<mlua::Function>("list")?.call(())?;
    let mut out: Vec<TestCase> = Vec::new();
    for entry in listed.sequence_values::<Table>() {
        let t = entry?;
        let name: String = t.get("name")?;
        let suites: Vec<String> = t
            .get::<Table>("path")?
            .sequence_values()
            .collect::<mlua::Result<_>>()?;
        let mut path = suites.clone();
        path.push(name.clone());
        let base = format!("{rel}::{}", path.join(" > "));
        let mut id = base.clone();
        let mut n = 2;
        while out.iter().any(|c| c.id == id) {
            id = format!("{base} #{n}");
            n += 1;
        }
        out.push(TestCase {
            id,
            file: rel.to_owned(),
            mode,
            suites,
            name,
            line: t.get::<Option<u32>>("line")?.unwrap_or(0),
            skipped: t.get::<Option<bool>>("skip")?.unwrap_or(false),
        });
    }
    Ok(out)
}

/// An mlua error's message (callback errors carry the Lua-side message inside).
fn lua_message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::CallbackError { cause, .. } => lua_message(cause),
        mlua::Error::RuntimeError(m) | mlua::Error::SyntaxError { message: m, .. } => m.clone(),
        other => other.to_string(),
    }
}

/// A Lua error value as text (strings, tables with `message`, mlua errors).
fn value_message(lua: &Lua, v: &Value) -> String {
    match v {
        Value::String(s) => s.to_string_lossy(),
        Value::Error(e) => lua_message(e),
        other => framework(lua)
            .and_then(|m| m.get::<mlua::Function>("errmsg"))
            .and_then(|f| f.call::<String>(other.clone()))
            .unwrap_or_else(|_| format!("{other:?}")),
    }
}

/// Drop the framework's own frames and the C plumbing from a traceback.
fn clean_traceback(tb: &str) -> String {
    let framework = FRAMEWORK_CHUNK.trim_start_matches('@');
    tb.lines()
        .map(str::trim_end)
        .filter(|l| {
            !l.is_empty()
                && !l.contains(framework)
                && l.trim() != "stack traceback:"
                && !l.contains("[C]: in function 'xpcall'")
                && !l.contains("[C]: in function 'error'")
                && !l.contains("(...tail calls...)")
                && !l.contains("[C]: in ?")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// First `Assets/….lua:<line>` in `text` (`file` wins when present anywhere).
fn locate(text: &str, file: &str) -> Option<(String, u32)> {
    locate_in_file(text, file).or_else(|| {
        let mut rest = text;
        while let Some(at) = rest.find("Assets/") {
            let tail = &rest[at..];
            if let Some(found) = parse_location(tail) {
                return Some(found);
            }
            rest = &rest[at + 7..];
        }
        None
    })
}

/// First `file:<line>` in `text`.
fn locate_in_file(text: &str, file: &str) -> Option<(String, u32)> {
    let mut rest = text;
    while let Some(at) = rest.find(file) {
        let tail = &rest[at + file.len()..];
        if let Some(digits) = tail.strip_prefix(':') {
            let n: String = digits.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(line) = n.parse() {
                return Some((file.to_owned(), line));
            }
        }
        rest = &rest[at + file.len()..];
    }
    None
}

/// `Assets/x/y.lua:12` at the start of `s`.
fn parse_location(s: &str) -> Option<(String, u32)> {
    let end = s.find(".lua:")? + 4;
    let path = &s[..end];
    if path.contains(char::is_whitespace) || path.contains('"') {
        return None;
    }
    let n: String = s[end + 1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    Some((path.to_owned(), n.parse().ok()?))
}

/// Starter file for Assets ▸ Create ▸ Testing ▸ EditMode Test / PlayMode Test.
pub fn template(mode: TestMode, name: &str) -> String {
    match mode {
        TestMode::EditMode => format!(
            r#"-- {name}: EditMode tests. They run against a scratch copy of the open scene, without
-- playing it. Run them from Window ▸ General ▸ Test Runner.
describe("{name}", function()
    beforeEach(function()
        -- Runs before every test of this describe block.
    end)

    it("adds numbers", function()
        assert.equal(1 + 1, 2)
    end)

    it("finds the camera", function()
        local camera = GameObject.Find("Main Camera")
        expect(camera).never.toBeNil()
    end)
end)
"#
        ),
        TestMode::PlayMode => format!(
            r#"-- {name}: PlayMode tests. Each test runs in its own play session; yield(seconds),
-- yieldFrames(n) and waitUntil(fn, timeout) let the game run. Run them from
-- Window ▸ General ▸ Test Runner.
describe("{name}", function()
    it("runs the game for a second", function()
        -- loadScene("SampleScene")
        local start = Time.frameCount
        yield(1)
        assert.greater(Time.frameCount, start)
    end)

    it("simulates input", function()
        Input.simulateKeyDown("d")
        yieldFrames(1)
        assert.isTrue(Input.GetKey("d"))
        Input.simulateKeyUp("d")
    end)
end)
"#
        ),
    }
}

/// `true` when `dir` (absolute) is inside the project's `Assets/Tests` folder.
pub fn in_tests_dir(root: &Path, dir: &Path) -> bool {
    dir.starts_with(root.join(TESTS_DIR))
}

/// Folder new test files go to: `dir` when inside `Assets/Tests`, else `Assets/Tests`.
pub fn create_dir(root: &Path, dir: &Path) -> PathBuf {
    if in_tests_dir(root, dir) {
        dir.to_path_buf()
    } else {
        root.join(TESTS_DIR)
    }
}

#[cfg(test)]
mod tests;
