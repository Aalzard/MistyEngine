//! Recompile & Restart for editors run from a source checkout.
//!
//! The editor knows where it was compiled (`CARGO_MANIFEST_DIR`); when that checkout still
//! exists it watches `<repo>/target/<profile>/misty.exe` (the profile of the running build).
//! Once a build newer than the running exe appears and has stopped changing, the toolbar shows
//! a green "New build — Restart" pill. Help ▸ Developer ▸ Rebuild & Restart runs
//! `cargo build -p misty_editor -p misty_player` (same profile) in the background, with its
//! progress in the status bar and its errors in the Console, then restarts.
//!
//! Restarting writes a restore file (`restore`), starts the new exe with `--restore <file>` and
//! closes this one. When the running exe is a copy outside `target` (e.g.
//! `E:\MistyTest\grass\misty-grass.exe`), the new build is copied over it with the updater's
//! rename trick (the running copy becomes `<name>.old.exe`, deleted on the next start) and the
//! copy is started: the user keeps their shortcut, and `target` stays free of a running exe so
//! the next cargo build can replace it. When the running exe is the one in `target`, cargo
//! couldn't overwrite it while it runs, so Rebuild & Restart first renames it aside the same way.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Settle, Stamp, stamp};
use crate::console::LogLevel;
use crate::scene_files::{self, AfterPrompt};
use crate::state::EditorState;
use crate::updater::install::old_exe_path;

/// The cargo profile this editor was built with.
pub const PROFILE: &str = if cfg!(debug_assertions) {
    "debug"
} else {
    "release"
};
/// How often the built exe is checked.
const POLL: Duration = Duration::from_secs(1);
/// A new exe must stop changing this long (the linker may still be writing it).
const SETTLE: Duration = Duration::from_millis(1500);

/// The source checkout this editor was compiled from, if it is still there.
pub fn source_repo() -> Option<PathBuf> {
    if std::env::var_os("MISTY_NO_DEV_RELOAD").is_some() {
        return None;
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent()?.parent()?;
    let is_checkout =
        repo.join("Cargo.toml").is_file() && repo.join("crates/misty_editor/Cargo.toml").is_file();
    is_checkout.then(|| repo.to_path_buf())
}

/// `candidate` is a newer build than `running`: written later, or written at the same moment
/// but a different size. Copies that keep the file time (Explorer, `std::fs::copy`) count as
/// the same build; copies stamped with the copy time count as newer than that build.
pub fn is_newer(running: Option<Stamp>, candidate: Option<Stamp>) -> bool {
    let (Some(r), Some(c)) = (running, candidate) else {
        return false;
    };
    match (r.modified, c.modified) {
        (Some(rm), Some(cm)) => cm > rm || (cm == rm && c.len != r.len),
        _ => false,
    }
}

/// A cargo build running in the background.
struct Job {
    shared: Arc<Mutex<JobState>>,
    started: Instant,
}

#[derive(Default)]
struct JobState {
    /// Latest progress line ("Compiling misty_engine v0.1.0 (…)").
    status: String,
    /// Compiler errors (one entry per message, with its source lines).
    errors: Vec<String>,
    warnings: usize,
    done: Option<Result<(), String>>,
}

#[derive(Default)]
pub struct DevBuild {
    /// The checkout (None = not a dev build: nothing is watched and the menu is hidden).
    pub repo: Option<PathBuf>,
    /// The running exe and its stamp when the editor started.
    running: Option<(PathBuf, Option<Stamp>)>,
    /// `<repo>/target/<profile>/misty.exe`.
    pub candidate: Option<PathBuf>,
    profile: String,
    settle: Settle,
    last_poll: Option<Instant>,
    /// A newer build is ready: the pill shows.
    pub newer: bool,
    /// `MISTY_FAKE_NEW_BUILD`: the pill shows (screenshots) but restarting does nothing.
    fake: bool,
    job: Option<Job>,
    /// The running exe renamed out of cargo's way (moved back if the build fails).
    moved_aside: Option<(PathBuf, PathBuf)>,
}

impl DevBuild {
    /// For the running editor: watching when it runs from a source checkout.
    pub fn detect() -> Self {
        let mut dev = match (source_repo(), std::env::current_exe().ok()) {
            (Some(repo), Some(exe)) => Self::with(repo, exe, PROFILE),
            _ => Self::default(),
        };
        if std::env::var_os("MISTY_FAKE_NEW_BUILD").is_some() {
            dev.fake = true;
            dev.newer = true;
        }
        dev
    }

    /// Watch `<repo>/target/<profile>/misty.exe` against `running_exe`.
    pub fn with(repo: PathBuf, running_exe: PathBuf, profile: &str) -> Self {
        let candidate = repo
            .join("target")
            .join(profile)
            .join(format!("misty{}", std::env::consts::EXE_SUFFIX));
        let running_stamp = stamp(&running_exe);
        Self {
            repo: Some(repo),
            running: Some((running_exe, running_stamp)),
            candidate: Some(candidate),
            profile: profile.to_owned(),
            ..Default::default()
        }
    }

    pub fn enabled(&self) -> bool {
        self.repo.is_some() || self.fake
    }

    pub fn building(&self) -> bool {
        self.job.is_some()
    }

    /// Look at the built exe (at most once a second). Returns true when the pill appears.
    pub fn poll(&mut self) -> bool {
        self.poll_with(POLL, SETTLE)
    }

    /// [`Self::poll`] with explicit intervals (tests use zero).
    pub fn poll_with(&mut self, every: Duration, settle: Duration) -> bool {
        if self.fake || self.job.is_some() {
            return false;
        }
        let (Some(candidate), Some((_, running))) = (&self.candidate, &self.running) else {
            return false;
        };
        if self.last_poll.is_some_and(|t| t.elapsed() < every) {
            return false;
        }
        self.last_poll = Some(Instant::now());
        let now = stamp(candidate);
        if !is_newer(*running, now) {
            self.newer = false;
            return false;
        }
        if self.newer || !self.settle.ready("exe", now, settle) {
            return false;
        }
        self.newer = true;
        true
    }

    /// Whether `exe` lives in the checkout's `target` folder.
    fn in_target(&self, exe: &Path) -> bool {
        let (Some(repo), Some(candidate)) = (&self.repo, &self.candidate) else {
            return false;
        };
        let norm = |p: &Path| {
            let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
            let s = p.to_string_lossy().replace('\\', "/");
            if cfg!(windows) { s.to_lowercase() } else { s }
        };
        let exe = norm(exe);
        exe == norm(candidate) || exe.starts_with(&(norm(&repo.join("target")) + "/"))
    }

    /// Put the new build where it will be started from; returns that exe and a note for the
    /// Console. A copy outside `target` is replaced (rename trick); otherwise the build in
    /// `target` is started directly.
    pub fn prepare_exe(&self) -> Result<(PathBuf, Option<String>), String> {
        let candidate = self
            .candidate
            .clone()
            .ok_or_else(|| "this editor was not built from a source checkout".to_owned())?;
        if !candidate.is_file() {
            return Err(format!("there is no build at {}", candidate.display()));
        }
        let Some((running, _)) = &self.running else {
            return Ok((candidate, None));
        };
        if self.in_target(running) {
            return Ok((candidate, None));
        }
        match copy_over(&candidate, running) {
            Ok(()) => Ok((
                running.clone(),
                Some(format!(
                    "Copied the new build over {} (the old one is kept as {} until the next start).",
                    running.display(),
                    old_exe_path(running).display()
                )),
            )),
            Err(e) => Ok((
                candidate.clone(),
                Some(format!(
                    "Could not replace {} ({e}); starting the build in {} instead.",
                    running.display(),
                    candidate.display()
                )),
            )),
        }
    }

    /// Start the new build with `--restore <file>`; returns the exe started.
    pub fn launch(&self, restore_file: &Path) -> Result<(PathBuf, Option<String>), String> {
        if self.fake {
            return Err("this is a simulated new build (MISTY_FAKE_NEW_BUILD)".into());
        }
        let (exe, note) = self.prepare_exe()?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--restore").arg(restore_file);
        if let Ok(dir) = std::env::current_dir() {
            cmd.current_dir(dir);
        }
        cmd.spawn()
            .map_err(|e| format!("could not start {}: {e}", exe.display()))?;
        Ok((exe, note))
    }
}

/// Free name for moving `exe` aside: `<name>.old.exe`, or `<name>.<pid>.old.exe` when an older
/// one can't be deleted yet (still running). Both are cleaned up on the next start.
fn aside_path(exe: &Path) -> PathBuf {
    let old = old_exe_path(exe);
    if !old.exists() || std::fs::remove_file(&old).is_ok() {
        return old;
    }
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    exe.with_file_name(format!("{stem}.{}.old.exe", std::process::id()))
}

/// Replace the running exe `dest` with `src`: a running exe can't be overwritten on Windows but
/// can be renamed, so it is moved aside first (and put back if the copy fails). A player exe
/// next to `dest` is refreshed too (best effort).
pub fn copy_over(src: &Path, dest: &Path) -> Result<(), String> {
    let aside = aside_path(dest);
    std::fs::rename(dest, &aside).map_err(|e| format!("rename: {e}"))?;
    if let Err(e) = std::fs::copy(src, dest) {
        let _ = std::fs::rename(&aside, dest);
        return Err(format!("copy: {e}"));
    }
    let player = format!("misty_player{}", std::env::consts::EXE_SUFFIX);
    let (player_src, player_dest) = (src.with_file_name(&player), dest.with_file_name(&player));
    if player_src.is_file() && player_dest.is_file() {
        let _ = std::fs::copy(&player_src, &player_dest);
    }
    Ok(())
}

/// cargo: the one that built us when run through `cargo run`, else from PATH or ~/.cargo/bin.
fn cargo_exe() -> PathBuf {
    if let Some(c) = std::env::var_os("CARGO") {
        return c.into();
    }
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| Path::new(&h).join(".cargo")))
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cargo")));
    let in_home = home.map(|h| {
        h.join("bin")
            .join(format!("cargo{}", std::env::consts::EXE_SUFFIX))
    });
    match in_home {
        Some(p) if p.is_file() => p,
        _ => PathBuf::from("cargo"),
    }
}

/// Read cargo's stderr: progress lines become the status, error messages are collected.
fn read_output(out: impl std::io::Read, shared: &Mutex<JobState>, ctx: &Option<egui::Context>) {
    let mut block: Option<(bool, Vec<String>)> = None;
    let lock = || shared.lock().unwrap_or_else(|e| e.into_inner());
    let finish = |block: &mut Option<(bool, Vec<String>)>| {
        if let Some((is_error, lines)) = block.take()
            && is_error
        {
            lock().errors.push(lines.join("\n"));
        }
    };
    for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
        let trimmed = line.trim_start();
        let progress = [
            "Compiling ",
            "Checking ",
            "Building ",
            "Finished ",
            "Blocking ",
        ]
        .iter()
        .any(|p| trimmed.starts_with(p));
        if progress {
            finish(&mut block);
            lock().status = trimmed.to_owned();
        } else if line.starts_with("error") || line.starts_with("warning") {
            finish(&mut block);
            let is_error = line.starts_with("error");
            if !is_error {
                lock().warnings += 1;
            }
            // "error: could not compile …" closes the build; keep it with the others.
            block = Some((is_error, vec![line.clone()]));
        } else if line.trim().is_empty() {
            finish(&mut block);
        } else if let Some((_, lines)) = &mut block {
            lines.push(line.clone());
        }
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    }
    finish(&mut block);
}

/// Help ▸ Developer ▸ Rebuild & Restart: build in the background, then restart.
pub fn start_rebuild(s: &mut EditorState, ctx: Option<egui::Context>) {
    let dev = &mut s.live.dev;
    if dev.job.is_some() {
        s.console.warn("A rebuild is already running.");
        return;
    }
    let Some(repo) = dev.repo.clone() else {
        s.console
            .warn("Rebuild & Restart needs an editor run from a Misty Engine source checkout.");
        return;
    };
    // cargo can't replace a running exe: move ours out of its way first.
    if let Some((running, _)) = dev.running.clone()
        && dev.candidate.as_deref() == Some(running.as_path())
        && running.is_file()
    {
        let aside = aside_path(&running);
        match std::fs::rename(&running, &aside) {
            Ok(()) => dev.moved_aside = Some((running, aside)),
            Err(e) => s.console.warn(format!(
                "Could not move the running editor aside ({e}); cargo may fail to replace it."
            )),
        }
    }
    let shared = Arc::new(Mutex::new(JobState {
        status: "Starting cargo…".into(),
        ..Default::default()
    }));
    let mut cmd = std::process::Command::new(cargo_exe());
    cmd.args([
        "build",
        "-p",
        "misty_editor",
        "-p",
        "misty_player",
        "--color",
        "never",
    ]);
    if dev.profile == "release" {
        cmd.arg("--release");
    }
    cmd.current_dir(&repo)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window pops up for cargo.
        cmd.creation_flags(0x0800_0000);
    }
    let thread_shared = shared.clone();
    std::thread::spawn(move || {
        let result = match cmd.spawn() {
            Ok(mut child) => {
                if let Some(err) = child.stderr.take() {
                    read_output(err, &thread_shared, &ctx);
                }
                match child.wait() {
                    Ok(status) if status.success() => Ok(()),
                    Ok(status) => Err(format!("cargo exited with {status}")),
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(format!("could not start cargo: {e}")),
        };
        thread_shared.lock().unwrap_or_else(|e| e.into_inner()).done = Some(result);
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
    let profile = dev.profile.clone();
    dev.job = Some(Job {
        shared,
        started: Instant::now(),
    });
    s.console.info(format!(
        "Rebuilding the editor ({profile}) in {}…",
        repo.display()
    ));
}

/// Once per frame: watch the built exe and pick up a finished rebuild. Returns true while a
/// rebuild runs (the caller keeps repainting).
pub fn frame(s: &mut EditorState) -> bool {
    if s.live.dev.poll() {
        s.console
            .info("A newer build of the editor is ready: click \"New build — Restart\".");
    }
    let Some(job) = &s.live.dev.job else {
        return false;
    };
    let finished = {
        let mut st = job.shared.lock().unwrap_or_else(|e| e.into_inner());
        st.done.take().map(|r| {
            (
                r,
                std::mem::take(&mut st.errors),
                st.warnings,
                job.started.elapsed(),
            )
        })
    };
    let Some((result, errors, warnings, took)) = finished else {
        return true;
    };
    s.live.dev.job = None;
    for e in &errors {
        s.console.log(LogLevel::Error, e.clone());
    }
    match result {
        Ok(()) => {
            s.console.info(format!(
                "Rebuilt in {:.0} s ({warnings} warning(s)); restarting.",
                took.as_secs_f32()
            ));
            s.live.dev.moved_aside = None;
            s.live.dev.newer = true;
            request_restart(s);
        }
        Err(e) => {
            // Put the running exe back where it was.
            if let Some((running, aside)) = s.live.dev.moved_aside.take()
                && !running.exists()
            {
                let _ = std::fs::rename(aside, running);
            }
            s.console.log(
                LogLevel::Error,
                format!(
                    "Rebuild failed: {e} ({} error(s), see above).",
                    errors.len()
                ),
            );
        }
    }
    false
}

/// The "New build — Restart" pill / the end of a rebuild: save prompt, then restart.
pub fn request_restart(s: &mut EditorState) {
    if s.code_editor.any_dirty() {
        s.console
            .warn("Save the scripts with unsaved changes in the Code Editor, then restart again.");
        return;
    }
    if s.play.in_play_mode() {
        crate::play::stop(s);
    }
    if let Some(a) = scene_files::guard(s, AfterPrompt::Restart) {
        scene_files::perform(s, a);
    }
}

/// The pill's green.
pub const GREEN: egui::Color32 = crate::updater::ui::GREEN;

/// Green "⟳ New build — Restart" pill while a newer build is ready. Returns true when clicked.
pub fn pill(ui: &mut egui::Ui, s: &EditorState) -> bool {
    let dev = &s.live.dev;
    if !dev.newer || dev.building() {
        return false;
    }
    let button = egui::Button::new(
        egui::RichText::new("⟳ New build — Restart")
            .color(egui::Color32::WHITE)
            .strong(),
    )
    .fill(GREEN)
    .corner_radius(egui::CornerRadius::same(10))
    .min_size(egui::vec2(0.0, 20.0));
    let path = dev
        .candidate
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    ui.add(button)
        .on_hover_text(format!(
            "A newer build of the editor is ready ({path}).\nClick to save your session \
             (scenes, camera, selection, layout) and restart into it."
        ))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Status bar: rebuild progress.
pub fn status(ui: &mut egui::Ui, s: &EditorState) {
    let Some(job) = &s.live.dev.job else {
        return;
    };
    let status = job
        .shared
        .lock()
        .map(|st| st.status.clone())
        .unwrap_or_default();
    ui.label(
        egui::RichText::new(format!(
            "Rebuilding ({:.0} s): {}",
            job.started.elapsed().as_secs_f32(),
            first_words(&status, 60)
        ))
        .color(egui::Color32::from_rgb(150, 200, 255)),
    );
    ui.add(egui::Spinner::new().size(12.0));
    ui.ctx().request_repaint_after(Duration::from_millis(250));
}

fn first_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

/// Help ▸ Developer.
pub fn menu(ui: &mut egui::Ui, s: &mut EditorState) {
    let dev = &s.live.dev;
    if !dev.enabled() {
        return;
    }
    let building = dev.building();
    let newer = dev.newer;
    ui.menu_button("Developer", |ui| {
        let rebuild = ui
            .add_enabled(!building, egui::Button::new("Rebuild & Restart"))
            .on_hover_text(format!(
                "cargo build -p misty_editor -p misty_player ({PROFILE}), then restart into it"
            ));
        if rebuild.clicked() {
            s.push(crate::actions::Action::RebuildAndRestart);
            ui.close();
        }
        if ui
            .add_enabled(
                newer && !building,
                egui::Button::new("Restart Into New Build"),
            )
            .clicked()
        {
            s.push(crate::actions::Action::RestartIntoNewBuild);
            ui.close();
        }
    });
}
