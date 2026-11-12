//! First open of a project (round 44): the built editor opens a fresh copy of SampleProject
//! with no `Library/` folder (no shader cache, no texture cache) and reports when its first
//! frame drew and its longest frame after that, so a load step that blocks the main thread
//! shows up as a slow frame.
//!
//! `cargo build --release -p misty_editor --bin misty`, then
//! `cargo test --release -p misty_editor first_open -- --ignored --nocapture`. A small window
//! (`MISTY_WINDOW_SIZE`) opens at the top left for the run. `MISTY_EDITOR_EXE` picks another
//! editor build, `MISTY_FIRST_OPEN_SCENE=Assets/Scenes/MeadowDemo.scene` another scene.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The editor next to this test binary (`target/<profile>/misty(.exe)`).
fn editor_exe() -> PathBuf {
    if let Some(p) = std::env::var_os("MISTY_EDITOR_EXE") {
        return p.into();
    }
    let test = std::env::current_exe().expect("test exe");
    let profile = test.parent().and_then(Path::parent).expect("target dir");
    profile.join(format!("misty{}", std::env::consts::EXE_SUFFIX))
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create dir");
    for entry in std::fs::read_dir(from).expect("read dir").flatten() {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).expect("copy file");
        }
    }
}

/// What the editor's capture run printed (see `capture::FrameClock::report`).
#[derive(Debug, Default)]
pub(crate) struct FirstOpen {
    pub first_frame_ms: f32,
    pub worst_frame_ms: f32,
    pub slow_frames: u32,
    pub complete_ms: Option<f32>,
}

/// The numbers in the capture report lines.
pub(crate) fn parse_report(log: &str) -> Option<FirstOpen> {
    let numbers = |line: &str| -> Vec<f32> {
        line.split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .filter_map(|w| w.trim_matches('.').parse().ok())
            .collect()
    };
    let line = log.lines().find(|l| l.starts_with("first editor frame"))?;
    // "first editor frame A ms after start; worst frame N at B ms; C frames over 100 ms; …"
    let n = numbers(line);
    let mut out = FirstOpen {
        first_frame_ms: *n.first()?,
        worst_frame_ms: *n.get(2)?,
        slow_frames: *n.get(3)? as u32,
        complete_ms: None,
    };
    if let Some(l) = log.lines().find(|l| l.starts_with("scene complete")) {
        out.complete_ms = numbers(l).get(1).copied();
    }
    Some(out)
}

#[test]
fn the_capture_report_parses() {
    let log = "renderer: X (Vulkan)\nfirst editor frame 812 ms after start; worst frame 37 at \
               64 ms; 0 frames over 100 ms; interactive 812 ms after start\nscene complete \
               (nothing streaming in) after frame 410, 9120 ms after start\n";
    let r = parse_report(log).expect("parsed");
    assert_eq!(r.first_frame_ms, 812.0);
    assert_eq!(r.worst_frame_ms, 64.0);
    assert_eq!(r.slow_frames, 0);
    assert_eq!(r.complete_ms, Some(9120.0));
}

#[test]
#[ignore = "runs the built editor on a copy of SampleProject (a window opens)"]
fn first_open_of_sample_project_keeps_drawing() {
    let exe = editor_exe();
    assert!(exe.is_file(), "build the editor first: {}", exe.display());
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let dir = std::env::temp_dir().join(format!("misty_first_open_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("SampleProject");
    copy_dir(&source, &project);
    let _ = std::fs::remove_dir_all(project.join("Library"));
    let mut cmd = Command::new(&exe);
    cmd.arg(&project)
        .env("MISTY_WINDOW_SIZE", "1100x650")
        .env("MISTY_SCREENSHOT", dir.join("shot.png"))
        .env("MISTY_CAPTURE_FRAME", "300")
        .env("MISTY_PROFILE", dir.join("profile.jsonl"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(scene) = std::env::var_os("MISTY_FIRST_OPEN_SCENE") {
        cmd.env("MISTY_SCENE", scene);
    }
    let started = Instant::now();
    let child = cmd.spawn().expect("start the editor");
    // A hung editor fails the test instead of holding the run (the window is left open).
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = match rx.recv_timeout(Duration::from_secs(900)) {
        Ok(out) => out.expect("editor output"),
        Err(_) => panic!("the editor did not finish within 15 minutes"),
    };
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    std::fs::write(dir.join("editor.log"), &log).ok();
    let report = parse_report(&log).unwrap_or_else(|| panic!("no capture report in:\n{log}"));
    eprintln!(
        "first open of SampleProject (empty Library): first frame {:.0} ms after start, worst \
         frame after it {:.0} ms, {} frames over 100 ms, scene complete {} (run {:.1} s; \
         logs in {})",
        report.first_frame_ms,
        report.worst_frame_ms,
        report.slow_frames,
        report
            .complete_ms
            .map_or("n/a".to_owned(), |t| format!("{t:.0} ms after start")),
        started.elapsed().as_secs_f32(),
        dir.display()
    );
    // Before round 44 the first frame came after every base pipeline compiled (8-12 s) and
    // the scene's first render compiled its variants in one frame (2-3 s). Loose bounds: a
    // build machine may be busy.
    assert!(
        report.first_frame_ms < 6000.0,
        "first frame after {:.0} ms",
        report.first_frame_ms
    );
    assert!(
        report.worst_frame_ms < 1500.0,
        "a frame took {:.0} ms",
        report.worst_frame_ms
    );
}
