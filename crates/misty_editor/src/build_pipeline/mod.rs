//! The build pipeline behind File ▸ Build Profiles: compile the player for the chosen platform
//! with cargo, then pack the project into a ready-to-ship folder. It all runs on a background
//! thread; the window shows progress and the log from [`BuildJob::poll`].

pub mod icon;
pub mod package;
pub mod steam;
pub mod targets;
pub mod zip;

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use misty_engine::build::BuildTarget;

pub use package::{Built, PackageRequest};

/// Messages from the build thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Log(String),
    Warning(String),
    /// Fraction done (0–1) and what is happening.
    Progress(f32, String),
    Done(Result<Built, String>),
}

/// A log line of the build window.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub warning: bool,
    pub text: String,
}

/// A running (or finished) build.
pub struct BuildJob {
    rx: Receiver<Msg>,
    pub progress: f32,
    pub stage: String,
    pub log: Vec<LogLine>,
    pub result: Option<Result<Built, String>>,
    /// Start the game when the build succeeds.
    pub run: bool,
}

impl BuildJob {
    /// Start building on a background thread.
    pub fn start(req: PackageRequest, run: bool) -> Self {
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("misty build".into())
            .spawn(move || {
                let mut send = |m: Msg| {
                    let _ = tx.send(m);
                };
                let result = build(&req, &mut send);
                let _ = tx.send(Msg::Done(result));
            })
            .expect("spawn the build thread");
        Self {
            rx,
            progress: 0.0,
            stage: "Starting".into(),
            log: Vec::new(),
            result: None,
            run,
        }
    }

    pub fn running(&self) -> bool {
        self.result.is_none()
    }

    /// Take the thread's messages; returns the result once, when the build ends.
    pub fn poll(&mut self) -> Option<Result<Built, String>> {
        let mut finished = None;
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Log(text) => self.log.push(LogLine {
                    warning: false,
                    text,
                }),
                Msg::Warning(text) => self.log.push(LogLine {
                    warning: true,
                    text,
                }),
                Msg::Progress(p, stage) => {
                    self.progress = p.max(self.progress);
                    self.stage = stage;
                }
                Msg::Done(r) => {
                    self.progress = 1.0;
                    self.stage = if r.is_ok() { "Done" } else { "Failed" }.into();
                    if let Err(e) = &r {
                        self.log.push(LogLine {
                            warning: true,
                            text: format!("Build failed: {e}"),
                        });
                    }
                    self.result = Some(r.clone());
                    finished = Some(r);
                }
            }
        }
        finished
    }
}

/// Compile (or find) the player, then package.
fn build(req: &PackageRequest, send: &mut dyn FnMut(Msg)) -> Result<Built, String> {
    let target = req.build.target;
    send(Msg::Log(format!(
        "Building {} with the {} profile ({}{}) into {}",
        req.product(),
        req.profile_name(),
        target.label(),
        if req.server() {
            ", dedicated server"
        } else {
            ""
        },
        req.out.display()
    )));
    package::check_output(&req.project, &req.out)?;
    // An editor installed outside the engine source (a release package) uses the player shipped
    // next to it, even if the source it was compiled from is still on this machine.
    let player = match targets::engine_workspace() {
        Some(ws) if !runs_from(&ws) && prebuilt(target).is_ok() => {
            let p = prebuilt(target)?;
            send(Msg::Log(format!(
                "Using the player next to the editor: {}",
                p.display()
            )));
            p
        }
        Some(ws) => compile(&ws, target, send)?,
        None => prebuilt(target)?,
    };
    let built = package::package(req, &player, send)?;
    send(Msg::Log(format!(
        "Build succeeded: {}",
        built.exe.display()
    )));
    Ok(built)
}

/// `cargo build` the player for `target` in the engine workspace; returns the executable.
fn compile(ws: &Path, target: BuildTarget, send: &mut dyn FnMut(Msg)) -> Result<PathBuf, String> {
    send(Msg::Progress(0.02, "Checking the Rust toolchain".into()));
    let tools = targets::probe_now();
    let status = targets::status(
        target,
        BuildTarget::host(),
        tools.installed.as_deref(),
        tools.zigbuild,
    );
    if !status.installed {
        return Err(format!(
            "the Rust target {} isn't installed. Run `{}` in a terminal, then build again.",
            target.triple(),
            targets::install_command(target)
        ));
    }
    if let Some(b) = status.blocker {
        return Err(b);
    }
    let (program, args) = targets::cargo_command(target, tools.zigbuild);
    send(Msg::Log(format!("> {program} {}", args.join(" "))));
    let expected = std::fs::read_to_string(ws.join("Cargo.lock"))
        .map_or(300, |t| t.matches("[[package]]").count())
        .max(1);
    let mut child = targets::no_window(Command::new(&program).args(&args))
        .current_dir(ws)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start cargo ({e}). Is Rust installed (rustup.rs)?"))?;
    let mut compiled = 0usize;
    let mut errors = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            let trimmed = line.trim_start();
            if trimmed.starts_with("Compiling ") {
                compiled += 1;
                let f = 0.05 + 0.65 * (compiled as f32 / expected as f32).min(1.0);
                send(Msg::Progress(f, trimmed.to_owned()));
            }
            if trimmed.starts_with("error") {
                errors.push(trimmed.to_owned());
            }
            send(Msg::Log(line));
        }
    }
    let exit = child.wait().map_err(|e| e.to_string())?;
    if !exit.success() {
        let first = errors.first().cloned().unwrap_or_default();
        return Err(format!(
            "cargo couldn't build the player for {} ({first}). See the log above.",
            target.label()
        ));
    }
    send(Msg::Progress(0.72, "Player compiled".into()));
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| ws.join("target"));
    let exe = targets::player_output(&target_dir, target);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!("cargo finished but {} is missing", exe.display()))
    }
}

/// `misty --build <project> <output folder> [--profile <name>]`: File ▸ Build Profiles ▸ Build
/// without the window (scripts, CI, release smoke tests). Without `--profile` the active
/// profile builds; without an output folder, the profile's. Prints the log and the build
/// report; returns the built executable.
pub fn build_from_cli(
    project: &Path,
    out: Option<&Path>,
    profile: Option<&str>,
) -> Result<PathBuf, String> {
    let build = misty_engine::build::BuildSettings::load(project)?;
    let settings = misty_engine::settings::ProjectSettings::load(project)?;
    // Profile assets hold scene paths saved as GUIDs.
    misty_engine::asset_db::open(project);
    let profiles = misty_engine::build_profile::BuildProfiles::load(project, &build);
    for w in &profiles.warnings {
        println!("warning: {w}");
    }
    let chosen = match profile {
        Some(name) => profiles.find(name).ok_or_else(|| {
            let names: Vec<&str> = profiles.profiles.iter().map(|p| p.name.as_str()).collect();
            format!(
                "no Build Profile named {name:?} (one of {})",
                names.join(", ")
            )
        })?,
        None => profiles.active(&build),
    };
    let out = match out {
        Some(o) => o.to_path_buf(),
        None => chosen.output_path(project).ok_or_else(|| {
            format!(
                "the profile {:?} has no output folder: pass one after the project",
                chosen.name
            )
        })?,
    };
    println!("Build Profile: {}", chosen.title());
    let req = PackageRequest::with_profile(project, &out, &build, &settings, chosen);
    let mut job = BuildJob::start(req, false);
    let mut printed = 0;
    loop {
        let result = job.poll();
        for l in &job.log[printed..] {
            println!("{}{}", if l.warning { "warning: " } else { "" }, l.text);
        }
        printed = job.log.len();
        if let Some(r) = result {
            return r.map(|b| {
                println!("{}", b.report.to_text());
                b.exe
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The running editor is inside `dir` (a development build in the workspace's `target`).
fn runs_from(dir: &Path) -> bool {
    let norm = |p: &Path| {
        let s = std::path::absolute(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .replace('\\', "/");
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    std::env::current_exe().is_ok_and(|exe| norm(&exe).starts_with(&(norm(dir) + "/")))
}

/// Without the engine source, use the player shipped next to the editor (this platform only).
fn prebuilt(target: BuildTarget) -> Result<PathBuf, String> {
    if Some(target) != BuildTarget::host() {
        return Err(format!(
            "building for {} needs the Misty Engine source (set MISTY_ENGINE_SOURCE to its \
             folder) so cargo can compile the player",
            target.label()
        ));
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let exe = dir.join(format!("misty_player{}", target.exe_suffix()));
    exe.is_file().then_some(exe.clone()).ok_or_else(|| {
        format!(
            "Player not found at {}: build `misty_player` first: \
             `cargo build -p misty_player --release`",
            exe.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end build of a real project (compiles the player, minutes):
    /// `MISTY_BUILD_PROJECT=<project> MISTY_BUILD_OUT=<folder> cargo test -p misty_editor
    /// real_build -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_build() {
        let project = PathBuf::from(std::env::var_os("MISTY_BUILD_PROJECT").expect("project"));
        let out = PathBuf::from(std::env::var_os("MISTY_BUILD_OUT").expect("output folder"));
        let build = misty_engine::build::BuildSettings::load(&project).unwrap();
        let settings = misty_engine::settings::ProjectSettings::load(&project).unwrap();
        let req = PackageRequest {
            project,
            out,
            build,
            settings,
            profile: None,
        };
        let mut job = BuildJob::start(req, false);
        let result = loop {
            if let Some(r) = job.poll() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        for l in &job.log {
            if l.warning || !l.text.starts_with("   Compiling") {
                eprintln!("{}{}", if l.warning { "WARN " } else { "" }, l.text);
            }
        }
        let built = result.unwrap();
        eprintln!("built {:?}", built);
        assert!(built.exe.is_file());
    }

    /// A development build runs from inside the workspace (and compiles the player); a package
    /// elsewhere uses the player next to it.
    #[test]
    fn runs_from_tells_a_workspace_build_from_a_package() {
        let exe = std::env::current_exe().unwrap();
        let above = exe.parent().and_then(Path::parent).unwrap();
        assert!(runs_from(above));
        assert!(!runs_from(&std::env::temp_dir().join("misty_not_here")));
        // A folder whose name only starts the same is not the same folder.
        let dir = exe.parent().unwrap();
        let sibling = PathBuf::from(format!("{}x", dir.display()));
        assert!(!runs_from(&sibling));
    }

    #[test]
    fn jobs_report_failures_through_poll() {
        let root = std::env::temp_dir().join(format!("misty_job_{}", std::process::id()));
        let req = PackageRequest {
            project: root.clone(),
            // Inside the project's Assets: refused before anything is compiled.
            out: root.join("Assets/Out"),
            build: Default::default(),
            settings: Default::default(),
            profile: None,
        };
        let mut job = BuildJob::start(req, false);
        let result = loop {
            if let Some(r) = job.poll() {
                break r;
            }
            std::thread::yield_now();
        };
        assert!(result.unwrap_err().contains("outside the project"));
        assert!(!job.running());
        assert!(
            job.log
                .iter()
                .any(|l| l.warning && l.text.contains("Build failed"))
        );
    }
}
