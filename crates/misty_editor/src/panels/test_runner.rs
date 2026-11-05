//! Window ▸ General ▸ Test Runner: the project's Lua tests (`Assets/Tests/**/*.test.lua` and
//! `*.playtest.lua`) in a tree per mode, run without freezing the editor (a slice of tests or
//! game frames per editor frame), with failure details that jump to the failing line.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use egui::{Color32, RichText, Ui};
use misty_engine::testing::{
    self, RunContext, Status, Summary, TestCase, TestFile, TestMode, TestResult, TestRun,
};

use crate::actions::Action;
use crate::layout::Tab;
use crate::state::EditorState;

pub const ICON: &str = "☑";
/// Work done per editor frame while tests run.
const SLICE: Duration = Duration::from_millis(12);

const PASSED: Color32 = Color32::from_rgb(90, 190, 90);
const FAILED: Color32 = Color32::from_rgb(225, 80, 70);
const SKIPPED: Color32 = Color32::from_rgb(200, 170, 70);
const RUNNING: Color32 = Color32::from_rgb(90, 160, 230);

/// What a Run button runs.
#[derive(Clone, Debug, PartialEq)]
pub enum RunWhich {
    All,
    /// Tests under the selected row (a file, a describe block or one test).
    Selected,
    Failed,
}

/// Window state (kept while the tab is closed).
pub struct TestRunnerWindow {
    pub mode: TestMode,
    pub files: Vec<TestFile>,
    scanned: bool,
    /// Latest result of each test id.
    pub results: HashMap<String, TestResult>,
    run: Option<TestRun>,
    /// Results of the current run already copied into `results`.
    taken: usize,
    /// Selected row: a file (`rel`), a describe block (`rel::A > B`) or a test id.
    pub selected: Option<String>,
    pub filter: String,
    pub collapsed: HashSet<String>,
    pub rerun_on_save: bool,
    mtimes: HashMap<String, Option<SystemTime>>,
    last_poll: Option<Instant>,
}

impl Default for TestRunnerWindow {
    fn default() -> Self {
        Self {
            mode: TestMode::EditMode,
            files: Vec::new(),
            scanned: false,
            results: HashMap::new(),
            run: None,
            taken: 0,
            selected: None,
            filter: String::new(),
            collapsed: HashSet::new(),
            rerun_on_save: false,
            mtimes: HashMap::new(),
            last_poll: None,
        }
    }
}

impl TestRunnerWindow {
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Every test of `mode`.
    fn cases(&self, mode: TestMode) -> impl Iterator<Item = &TestCase> {
        self.files
            .iter()
            .filter(move |f| f.mode == mode)
            .flat_map(|f| f.tests.iter())
    }

    pub fn status(&self, id: &str) -> Status {
        self.results.get(id).map_or(Status::NotRun, |r| r.status)
    }

    /// Counts of the tests of `mode` (for the summary bar).
    pub fn summary(&self, mode: TestMode) -> (Summary, usize) {
        let cases: Vec<&TestCase> = self.cases(mode).collect();
        let s = Summary::of(cases.iter().filter_map(|c| self.results.get(&c.id)));
        (s, cases.len())
    }
}

/// Look for test files again (and remember their modification times).
pub fn scan(s: &mut EditorState) {
    let root = s.project.root.clone();
    let w = &mut s.test_runner;
    w.files = testing::discover(&root);
    w.mtimes = mtimes(&root);
    w.scanned = true;
    w.last_poll = Some(Instant::now());
}

fn mtimes(root: &Path) -> HashMap<String, Option<SystemTime>> {
    testing::discover_paths(root)
        .into_iter()
        .map(|rel| {
            let m = std::fs::metadata(root.join(&rel))
                .and_then(|m| m.modified())
                .ok();
            (rel, m)
        })
        .collect()
}

/// `key` selects `case`: the test itself, its file, or a describe block around it.
pub fn under(case: &TestCase, key: &str) -> bool {
    case.id == key
        || key == case.file
        || (case.id.starts_with(key) && case.id[key.len()..].starts_with(" > "))
}

/// Start a run of the current mode's tests (rescans first). Not while playing: the game clock
/// and audio belong to Play mode then.
pub fn start(s: &mut EditorState, which: RunWhich) {
    if s.play.in_play_mode() {
        s.console
            .warn("Test Runner: exit Play mode to run tests".to_owned());
        return;
    }
    if s.test_runner.running() {
        return;
    }
    scan(s);
    let w = &s.test_runner;
    let mode = w.mode;
    let filter = w.filter.clone();
    let ids: Vec<String> = w
        .cases(mode)
        .filter(|c| c.matches(&filter))
        .filter(|c| match &which {
            RunWhich::All => true,
            RunWhich::Selected => w.selected.as_deref().is_some_and(|k| under(c, k)),
            RunWhich::Failed => w.status(&c.id) == Status::Failed,
        })
        .map(|c| c.id.clone())
        .collect();
    if ids.is_empty() {
        return;
    }
    let cx = RunContext {
        root: s.project.root.clone(),
        settings: s.project_settings.clone(),
        build: s.build_settings.clone(),
        scene: s.scene.clone(),
    };
    let w = &mut s.test_runner;
    for id in &ids {
        if let Some(r) = w.results.get_mut(id) {
            r.status = Status::Running;
        } else {
            w.results.insert(
                id.clone(),
                TestResult {
                    status: Status::Running,
                    ..TestResult::skipped()
                },
            );
        }
    }
    w.run = Some(TestRun::new(cx, &w.files, |c| ids.contains(&c.id)));
    w.taken = 0;
}

/// Stop the run; tests that did not get to run go back to "not run".
pub fn cancel(s: &mut EditorState) {
    let w = &mut s.test_runner;
    if w.run.take().is_some() {
        w.results.retain(|_, r| r.status != Status::Running);
    }
}

/// Every editor frame: step the run (and rerun on save). True while tests are running.
pub fn tick(s: &mut EditorState) -> bool {
    poll_saves(s);
    let w = &mut s.test_runner;
    let Some(run) = w.run.as_mut() else {
        return false;
    };
    let done = run.step(SLICE);
    for (case, result) in &run.results()[w.taken..] {
        w.results.insert(case.id.clone(), result.clone());
    }
    w.taken = run.results().len();
    if let Some(case) = run.current() {
        let id = case.id.clone();
        if let Some(r) = w.results.get_mut(&id) {
            r.status = Status::Running;
        }
    }
    if !done {
        return true;
    }
    let run = w.run.take().expect("running");
    let summary = Summary::of(run.results().iter().map(|(_, r)| r));
    let line = format!(
        "Test Runner: {} passed, {} failed, {} skipped ({:.2} s)",
        summary.passed,
        summary.failed,
        summary.skipped,
        run.elapsed()
    );
    if summary.failed > 0 {
        s.console.warn(line);
    } else {
        s.console.info(line);
    }
    false
}

/// Run every test to the end now (screenshots, `MISTY_RUN_TESTS`), then show `mode`'s tab
/// with the test named `select` selected.
pub fn run_from_env(s: &mut EditorState, mode: &str, select: Option<&str>) {
    let shown = if mode.eq_ignore_ascii_case("PlayMode") {
        TestMode::PlayMode
    } else {
        TestMode::EditMode
    };
    for m in TestMode::ALL {
        s.test_runner.mode = m;
        start(s, RunWhich::All);
        while s.test_runner.running() {
            tick(s);
        }
    }
    s.test_runner.mode = shown;
    if let Some(name) = select {
        let w = &mut s.test_runner;
        let id = w
            .cases(shown)
            .find(|c| c.name == name)
            .map(|c| c.id.clone());
        w.selected = id;
    }
}

/// Rerun on save: a changed, added or removed test file reruns the current mode's tests.
fn poll_saves(s: &mut EditorState) {
    let w = &s.test_runner;
    if !w.rerun_on_save || w.running() || s.play.in_play_mode() {
        return;
    }
    if w.last_poll
        .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
    {
        return;
    }
    let now = mtimes(&s.project.root);
    let changed = now != s.test_runner.mtimes;
    s.test_runner.last_poll = Some(Instant::now());
    if changed {
        start(s, RunWhich::All);
    }
}

/// Assets ▸ Create ▸ Testing: a test file from the template, in `Assets/Tests` (or the current
/// folder when it is inside it).
pub fn create_test_file(
    root: &Path,
    dir: &Path,
    mode: TestMode,
) -> Result<std::path::PathBuf, String> {
    let dir = testing::create_dir(root, dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (stem, ext) = match mode {
        TestMode::EditMode => ("NewEditModeTest", "test.lua"),
        TestMode::PlayMode => ("NewPlayModeTest", "playtest.lua"),
    };
    let path = crate::asset_ops::unique_path(&dir, stem, ext);
    let name = path
        .file_name()
        .map(|n| {
            n.to_string_lossy()
                .split('.')
                .next()
                .unwrap_or(stem)
                .replace(' ', "")
        })
        .unwrap_or_else(|| stem.to_owned());
    std::fs::write(&path, testing::template(mode, &name)).map_err(|e| e.to_string())?;
    Ok(path)
}

// ---- Tree -------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    File,
    Suite,
    Test,
}

/// One visible line of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub depth: usize,
    pub kind: RowKind,
    pub expanded: bool,
    pub status: Status,
    /// Seconds (tests that ran, summed for groups).
    pub duration: Option<f64>,
    pub file: String,
    pub line: u32,
}

/// The rows of `mode`'s tree: files ▸ describe blocks ▸ tests, filtered and folded.
pub fn rows(w: &TestRunnerWindow) -> Vec<Row> {
    let mut out = Vec::new();
    for f in w.files.iter().filter(|f| f.mode == w.mode) {
        let tests: Vec<&TestCase> = f.tests.iter().filter(|c| c.matches(&w.filter)).collect();
        if tests.is_empty() {
            continue;
        }
        let name = f.rel.strip_prefix("Assets/Tests/").unwrap_or(&f.rel);
        group_row(
            w,
            &mut out,
            &f.rel,
            name,
            0,
            RowKind::File,
            &f.rel,
            1,
            &tests,
        );
        if w.collapsed.contains(&f.rel) {
            continue;
        }
        add_children(w, &mut out, &f.rel, &[], 1, &tests);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn group_row(
    w: &TestRunnerWindow,
    out: &mut Vec<Row>,
    key: &str,
    label: &str,
    depth: usize,
    kind: RowKind,
    file: &str,
    line: u32,
    tests: &[&TestCase],
) {
    let statuses: Vec<Status> = tests.iter().map(|c| w.status(&c.id)).collect();
    let durations: Vec<f64> = tests
        .iter()
        .filter_map(|c| w.results.get(&c.id))
        .filter(|r| matches!(r.status, Status::Passed | Status::Failed))
        .map(|r| r.duration)
        .collect();
    out.push(Row {
        key: key.to_owned(),
        label: label.to_owned(),
        depth,
        kind,
        expanded: !w.collapsed.contains(key),
        status: aggregate(&statuses),
        duration: (!durations.is_empty()).then(|| durations.iter().sum()),
        file: file.to_owned(),
        line,
    });
}

/// Children of the describe path `path` (tests in declaration order, suites where they start).
fn add_children(
    w: &TestRunnerWindow,
    out: &mut Vec<Row>,
    file: &str,
    path: &[String],
    depth: usize,
    tests: &[&TestCase],
) {
    let mut seen: Vec<&str> = Vec::new();
    for c in tests {
        if c.suites.len() == path.len() {
            let r = w.results.get(&c.id);
            out.push(Row {
                key: c.id.clone(),
                label: c.name.clone(),
                depth,
                kind: RowKind::Test,
                expanded: false,
                status: w.status(&c.id),
                duration: r
                    .filter(|r| matches!(r.status, Status::Passed | Status::Failed))
                    .map(|r| r.duration),
                file: file.to_owned(),
                line: c.line,
            });
            continue;
        }
        let name = c.suites[path.len()].as_str();
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        let mut sub = path.to_vec();
        sub.push(name.to_owned());
        let inner: Vec<&TestCase> = tests
            .iter()
            .filter(|t| t.suites.len() > path.len() && t.suites[..=path.len()] == sub[..])
            .copied()
            .collect();
        let key = format!("{file}::{}", sub.join(" > "));
        let line = inner.first().map_or(1, |t| t.line);
        group_row(
            w,
            out,
            &key,
            name,
            depth,
            RowKind::Suite,
            file,
            line,
            &inner,
        );
        if !w.collapsed.contains(&key) {
            add_children(w, out, file, &sub, depth + 1, &inner);
        }
    }
}

/// A group's status: any failure, else running, else passed when every test that should run
/// did, else not run (all skipped: skipped).
pub fn aggregate(statuses: &[Status]) -> Status {
    if statuses.contains(&Status::Failed) {
        Status::Failed
    } else if statuses.contains(&Status::Running) {
        Status::Running
    } else if !statuses.is_empty() && statuses.iter().all(|s| *s == Status::Skipped) {
        Status::Skipped
    } else if statuses.contains(&Status::Passed) && !statuses.contains(&Status::NotRun) {
        Status::Passed
    } else {
        Status::NotRun
    }
}

pub fn status_icon(status: Status) -> (&'static str, Color32) {
    match status {
        Status::Passed => ("✔", PASSED),
        Status::Failed => ("✖", FAILED),
        Status::Skipped => ("⊘", SKIPPED),
        Status::Running => ("⟳", RUNNING),
        Status::NotRun => ("○", Color32::GRAY),
    }
}

fn duration_text(secs: f64) -> String {
    if secs < 0.0005 {
        "<1 ms".to_owned()
    } else if secs < 1.0 {
        format!("{:.0} ms", secs * 1000.0)
    } else {
        format!("{secs:.2} s")
    }
}

// ---- UI ---------------------------------------------------------------------------------------

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    if !s.test_runner.scanned {
        scan(s);
    }
    toolbar(ui, s);
    summary_bar(ui, s);
    ui.separator();
    let avail = ui.available_height();
    let tree_h = (avail * 0.58).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("test_runner_tree")
        .max_height(tree_h)
        .min_scrolled_height(tree_h)
        .auto_shrink([false, false])
        .show(ui, |ui| tree(ui, s));
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("test_runner_details")
        .auto_shrink([false, false])
        .show(ui, |ui| details(ui, s));
}

fn toolbar(ui: &mut Ui, s: &mut EditorState) {
    ui.horizontal(|ui| {
        for mode in TestMode::ALL {
            let on = s.test_runner.mode == mode;
            if ui.selectable_label(on, mode.label()).clicked() && !on {
                s.test_runner.mode = mode;
                s.test_runner.selected = None;
            }
        }
        ui.separator();
        let running = s.test_runner.running();
        let playing = s.play.in_play_mode();
        let can_run = !running && !playing;
        let hint = if playing {
            "Exit Play mode to run tests"
        } else {
            ""
        };
        let failed_any = {
            let w = &s.test_runner;
            w.cases(w.mode).any(|c| w.status(&c.id) == Status::Failed)
        };
        if ui
            .add_enabled(can_run, egui::Button::new("Run All"))
            .on_disabled_hover_text(hint)
            .clicked()
        {
            start(s, RunWhich::All);
        }
        let has_sel = s.test_runner.selected.is_some();
        if ui
            .add_enabled(can_run && has_sel, egui::Button::new("Run Selected"))
            .clicked()
        {
            start(s, RunWhich::Selected);
        }
        if ui
            .add_enabled(can_run && failed_any, egui::Button::new("Run Failed"))
            .clicked()
        {
            start(s, RunWhich::Failed);
        }
        if running && ui.button("Cancel").clicked() {
            cancel(s);
        }
        ui.checkbox(&mut s.test_runner.rerun_on_save, "Rerun on save")
            .on_hover_text("Run the tests again when a test file changes");
        if ui
            .add_enabled(!running, egui::Button::new("⟳"))
            .on_hover_text("Refresh the test list")
            .clicked()
        {
            scan(s);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut s.test_runner.filter)
                    .hint_text("🔍 Search tests")
                    .desired_width(180.0),
            );
        });
    });
}

fn summary_bar(ui: &mut Ui, s: &mut EditorState) {
    let w = &s.test_runner;
    let (sum, total) = w.summary(w.mode);
    let not_run = total.saturating_sub(sum.total());
    let (fill, text) = if let Some(run) = w.run.as_ref() {
        let (done, all) = run.progress();
        let name = run.current().map(|c| c.name.as_str()).unwrap_or("…");
        (
            RUNNING.gamma_multiply(0.55),
            format!("Running {}/{all}: {name}", done + 1),
        )
    } else {
        let fill = if sum.failed > 0 {
            FAILED.gamma_multiply(0.6)
        } else if sum.passed > 0 {
            PASSED.gamma_multiply(0.55)
        } else {
            Color32::from_gray(70)
        };
        let mut text = format!(
            "✔ {} passed    ✖ {} failed    ⊘ {} skipped",
            sum.passed, sum.failed, sum.skipped
        );
        if not_run > 0 {
            text.push_str(&format!("    ○ {not_run} not run"));
        }
        if sum.total() > 0 {
            text.push_str(&format!("    {}", duration_text(sum.duration)));
        }
        (fill, text)
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(Color32::WHITE));
        });
}

fn tree(ui: &mut Ui, s: &mut EditorState) {
    let rows = rows(&s.test_runner);
    if rows.is_empty() {
        let w = &s.test_runner;
        let msg = if w.files.iter().any(|f| f.mode == w.mode) {
            "No tests match the search.".to_owned()
        } else {
            let suffix = match w.mode {
                TestMode::EditMode => testing::EDIT_SUFFIX,
                TestMode::PlayMode => testing::PLAY_SUFFIX,
            };
            format!(
                "No {} tests. Add *{suffix} files under {} (Assets ▸ Create ▸ Testing).",
                w.mode.label(),
                testing::TESTS_DIR
            )
        };
        ui.weak(msg);
        return;
    }
    for row in rows {
        ui.horizontal(|ui| {
            ui.add_space(row.depth as f32 * 16.0);
            if row.kind != RowKind::Test {
                let arrow = if row.expanded { "⏷" } else { "⏵" };
                if ui
                    .add(egui::Button::new(arrow).frame(false).small())
                    .clicked()
                {
                    let c = &mut s.test_runner.collapsed;
                    if !c.remove(&row.key) {
                        c.insert(row.key.clone());
                    }
                }
            } else {
                ui.add_space(18.0);
            }
            let (icon, color) = status_icon(row.status);
            ui.label(RichText::new(icon).color(color));
            let selected = s.test_runner.selected.as_deref() == Some(row.key.as_str());
            let label = if row.kind == RowKind::File {
                RichText::new(&row.label).strong()
            } else {
                RichText::new(&row.label)
            };
            let r = ui.selectable_label(selected, label);
            if r.clicked() {
                s.test_runner.selected = Some(row.key.clone());
            }
            if r.double_clicked() {
                open_at(s, &row.file, row.line);
            }
            r.context_menu(|ui| {
                if ui.button("Run").clicked() {
                    s.test_runner.selected = Some(row.key.clone());
                    start(s, RunWhich::Selected);
                    ui.close();
                }
                if ui.button("Open in Code Editor").clicked() {
                    open_at(s, &row.file, row.line);
                    ui.close();
                }
            });
            if let Some(d) = row.duration {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(duration_text(d));
                });
            }
        });
    }
}

/// Open a script at a line in the built-in Code Editor.
fn open_at(s: &mut EditorState, rel: &str, line: u32) {
    match s.code_editor.open(rel, Some(line.max(1) as usize)) {
        Ok(()) => s.push(Action::FocusTab(Tab::CodeEditor)),
        Err(e) => s.console.warn(format!("Could not open {rel}: {e}")),
    }
}

/// `file.lua:12` link that opens the Code Editor there.
fn location_link(ui: &mut Ui, s: &mut EditorState, rel: &str, line: u32) {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if ui
        .link(format!("{name}:{line}"))
        .on_hover_text(format!("Open {rel} at line {line}"))
        .clicked()
    {
        open_at(s, rel, line);
    }
}

fn details(ui: &mut Ui, s: &mut EditorState) {
    let Some(key) = s.test_runner.selected.clone() else {
        ui.weak("Select a test to see its result.");
        return;
    };
    let w = &s.test_runner;
    // A file that failed to load.
    if let Some(f) = w.files.iter().find(|f| f.rel == key) {
        ui.label(RichText::new(&f.rel).strong());
        if let Some(err) = f.error.clone() {
            failure_ui(ui, s, &err);
        } else {
            let n = f.tests.len();
            ui.weak(format!("{n} tests"));
        }
        return;
    }
    let Some(case) = w.cases(w.mode).find(|c| c.id == key).cloned() else {
        let n = w.cases(w.mode).filter(|c| under(c, &key)).count();
        ui.label(RichText::new(key.rsplit(" > ").next().unwrap_or(&key)).strong());
        ui.weak(format!("{n} tests"));
        return;
    };
    let result = w.results.get(&case.id).cloned();
    ui.horizontal(|ui| {
        let (icon, color) = status_icon(result.as_ref().map_or(Status::NotRun, |r| r.status));
        ui.label(RichText::new(icon).color(color));
        ui.label(RichText::new(&case.name).strong());
        location_link(ui, s, &case.file, case.line);
    });
    if !case.suites.is_empty() {
        ui.weak(case.suites.join(" ▸ "));
    }
    let Some(r) = result else {
        ui.weak(if case.skipped {
            "Skipped (it.skip, describe.skip, no function, or left out by .only)"
        } else {
            "Not run yet."
        });
        return;
    };
    let mut info = match r.status {
        Status::Passed => format!("Passed in {}", duration_text(r.duration)),
        Status::Failed => format!("Failed after {}", duration_text(r.duration)),
        Status::Skipped => "Skipped".into(),
        Status::Running => "Running…".into(),
        Status::NotRun => "Not run".into(),
    };
    if r.frames > 0 {
        info.push_str(&format!(" ({} frames)", r.frames));
    }
    ui.label(info);
    if let Some(f) = &r.failure {
        failure_ui(ui, s, f);
    }
    if !r.logs.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new("Output").strong());
        for l in &r.logs {
            ui.add(egui::Label::new(RichText::new(l).monospace()).selectable(true));
        }
    }
}

fn failure_ui(ui: &mut Ui, s: &mut EditorState, f: &testing::Failure) {
    ui.add_space(4.0);
    ui.add(egui::Label::new(RichText::new(&f.message).color(FAILED)).selectable(true));
    if let Some((rel, line)) = &f.location {
        ui.horizontal(|ui| {
            ui.label("at");
            location_link(ui, s, rel, *line);
        });
    }
    if !f.traceback.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Traceback").strong());
        for line in f.traceback.lines() {
            match testing::line_location(line) {
                Some((rel, n)) => {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        location_link(ui, s, &rel, n);
                        let rest = line.trim().splitn(3, ':').nth(2).unwrap_or("").trim();
                        ui.weak(rest);
                    });
                }
                None => {
                    ui.weak(line.trim());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
