//! `misty --run-tests <project> [--filter X] [--mode EditMode|PlayMode] [--results out.xml]`:
//! run the project's tests without a window, print them, write JUnit XML and exit with
//! 0 (all passed), 1 (a test failed) or 2 (bad arguments or project).

use std::io::Write;
use std::path::{Path, PathBuf};

use super::{RunContext, Status, Summary, TestMode, TestRun, discover};

pub const USAGE: &str = "misty --run-tests <project folder> [--filter <text>] [--mode EditMode|PlayMode] [--results <file.xml>]";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub project: PathBuf,
    /// Case-insensitive substring of `file::Suite > test`.
    pub filter: Option<String>,
    pub mode: Option<TestMode>,
    /// JUnit XML output file.
    pub results: Option<PathBuf>,
}

/// Arguments after `--run-tests`.
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut project = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a.as_str() {
            "--filter" => o.filter = Some(value("--filter")?),
            "--results" => o.results = Some(PathBuf::from(value("--results")?)),
            "--mode" => {
                let m = value("--mode")?;
                o.mode = Some(match m.to_lowercase().as_str() {
                    "editmode" | "edit" => TestMode::EditMode,
                    "playmode" | "play" => TestMode::PlayMode,
                    _ => return Err(format!("unknown mode {m:?} (EditMode or PlayMode)")),
                });
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            p if project.is_none() => project = Some(PathBuf::from(p)),
            extra => return Err(format!("unexpected argument {extra:?}")),
        }
    }
    o.project = project.ok_or("the project folder is missing")?;
    Ok(o)
}

/// Run the tests for the command line; returns the process exit code.
pub fn run(args: &[String], out: &mut impl Write) -> i32 {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(out, "misty: {e}\nusage: {USAGE}");
            return 2;
        }
    };
    match run_options(&o, out) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(out, "misty: {e}");
            2
        }
    }
}

fn run_options(o: &Options, out: &mut impl Write) -> Result<i32, String> {
    let root = &o.project;
    if !root.join("Assets").is_dir() {
        return Err(format!(
            "{} is not a project (no Assets folder)",
            root.display()
        ));
    }
    crate::assets::set_project_root(root);
    let cx = RunContext::load(root)?;
    let files = discover(root);
    let filter = o.filter.clone().unwrap_or_default();
    let mode = o.mode;
    let mut run = TestRun::new(cx, &files, |c| {
        c.matches(&filter) && mode.is_none_or(|m| m == c.mode)
    });
    let (_, total) = run.progress();
    let _ = writeln!(out, "Running {total} tests in {}", root.display());
    let mut printed = 0;
    while !run.step(std::time::Duration::from_millis(100)) {
        printed = print_new(&run, printed, out);
    }
    print_new(&run, printed, out);
    let results = run.results();
    let s = Summary::of(results.iter().map(|(_, r)| r));
    let _ = writeln!(
        out,
        "{} passed, {} failed, {} skipped ({:.2} s)",
        s.passed,
        s.failed,
        s.skipped,
        run.elapsed()
    );
    if let Some(path) = &o.results {
        write_results(path, &super::junit::write(results))?;
        let _ = writeln!(out, "Results written to {}", path.display());
    }
    Ok(if s.failed > 0 { 1 } else { 0 })
}

fn print_new(run: &TestRun, from: usize, out: &mut impl Write) -> usize {
    let results = run.results();
    for (c, r) in &results[from..] {
        let tag = match r.status {
            Status::Passed => "PASS",
            Status::Failed => "FAIL",
            Status::Skipped => "SKIP",
            Status::NotRun | Status::Running => "----",
        };
        let name = c.id.replacen("::", " > ", 1);
        let _ = writeln!(out, "  {tag} {name} ({:.3} s)", r.duration);
        if let Some(f) = &r.failure {
            for line in f.full_message().lines() {
                let _ = writeln!(out, "       {line}");
            }
        }
    }
    results.len()
}

fn write_results(path: &Path, xml: &str) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, xml).map_err(|e| format!("{}: {e}", path.display()))
}
