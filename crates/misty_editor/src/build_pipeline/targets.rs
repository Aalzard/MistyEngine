//! Build targets: is the Rust target installed, can this machine link for it, and the exact
//! cargo command that builds the player for it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use misty_engine::build::{BuildTarget, TargetOs};

/// What the machine can do for a target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetStatus {
    /// `rustup target list --installed` lists it (true when rustup isn't there to ask).
    pub installed: bool,
    /// Why it can't be built here even when installed (missing linker / SDK), if known.
    pub blocker: Option<String>,
    /// Extra tools the build may need.
    pub note: Option<String>,
}

impl TargetStatus {
    pub fn ready(&self) -> bool {
        self.installed && self.blocker.is_none()
    }
}

/// `rustup target add …`: what to run when a target is missing.
pub fn install_command(t: BuildTarget) -> String {
    format!("rustup target add {}", t.triple())
}

/// Status of `t` on a machine of type `host`, given the installed target list (`None` =
/// rustup not found) and whether `cargo zigbuild` is available.
pub fn status(
    t: BuildTarget,
    host: Option<BuildTarget>,
    installed: Option<&[String]>,
    zigbuild: bool,
) -> TargetStatus {
    let installed = installed.is_none_or(|list| list.iter().any(|x| x == t.triple()));
    let host_os = host.map(|h| h.os());
    let other_os = host_os.is_some_and(|h| h != t.os());
    let blocker = match t.os() {
        TargetOs::MacOs if other_os => Some(
            "macOS builds must be made on a Mac: Apple's SDK and code signing only exist there."
                .to_owned(),
        ),
        TargetOs::Linux if other_os && !zigbuild => Some(
            "Linking Linux executables needs a Linux linker. Build on Linux (or WSL), or \
             install zig and cargo-zigbuild (`cargo install cargo-zigbuild`) and build again."
                .to_owned(),
        ),
        _ => None,
    };
    // Same OS, other CPU: the platform's C tools for that CPU are needed (Lua is C code).
    let note = (Some(t) != host && host_os == Some(t.os())).then(|| match t.os() {
        TargetOs::Windows => "Also needs the Visual Studio component \"MSVC build tools\" for \
                              this CPU (ARM64 or x64), from the Visual Studio Installer."
            .to_owned(),
        TargetOs::Linux => "Also needs a C cross-compiler for this CPU (for example \
                            gcc-aarch64-linux-gnu), or cargo-zigbuild."
            .to_owned(),
        TargetOs::MacOs => "Xcode builds both Mac CPUs; nothing else is needed.".to_owned(),
    });
    TargetStatus {
        installed,
        blocker,
        note,
    }
}

/// Program and arguments that build the player for `t` (`zigbuild` = link with zig).
pub fn cargo_command(t: BuildTarget, zigbuild: bool) -> (String, Vec<String>) {
    let sub = if zigbuild && t.os() == TargetOs::Linux {
        "zigbuild"
    } else {
        "build"
    };
    let args = [
        sub,
        "-p",
        "misty_player",
        "--release",
        "--target",
        t.triple(),
        "--message-format",
        "human",
    ];
    ("cargo".into(), args.iter().map(|s| s.to_string()).collect())
}

/// Where cargo leaves the player for `t` under `target_dir`.
pub fn player_output(target_dir: &Path, t: BuildTarget) -> PathBuf {
    target_dir
        .join(t.triple())
        .join("release")
        .join(format!("misty_player{}", t.exe_suffix()))
}

/// The engine's source workspace (where `cargo build -p misty_player` works), if present.
/// `MISTY_ENGINE_SOURCE` overrides the folder the editor was compiled from.
pub fn engine_workspace() -> Option<PathBuf> {
    let dir = std::env::var_os("MISTY_ENGINE_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let dir = std::path::absolute(dir).ok()?;
    dir.join("crates/misty_player/Cargo.toml")
        .is_file()
        .then_some(dir)
}

/// Installed targets and whether cargo-zigbuild exists, found once in the background.
#[derive(Clone, Debug, Default)]
pub struct Toolchain {
    /// `None` = rustup isn't available.
    pub installed: Option<Vec<String>>,
    pub zigbuild: bool,
}

static TOOLCHAIN: Mutex<Option<Option<Toolchain>>> = Mutex::new(None);

/// The toolchain check result; starts it on the first call (`None` while it runs).
pub fn toolchain() -> Option<Toolchain> {
    let mut t = TOOLCHAIN.lock().unwrap_or_else(|e| e.into_inner());
    match &*t {
        Some(result) => result.clone(),
        None => {
            *t = Some(None);
            std::thread::spawn(|| {
                let found = probe();
                *TOOLCHAIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Some(found));
            });
            None
        }
    }
}

/// Ask again (after the user installed a target).
pub fn refresh_toolchain() {
    *TOOLCHAIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Check right now (the build thread) and remember the answer for the window.
pub fn probe_now() -> Toolchain {
    let found = probe();
    *TOOLCHAIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Some(found.clone()));
    found
}

fn probe() -> Toolchain {
    let installed = quiet(Command::new("rustup").args(["target", "list", "--installed"]))
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_target_list(&String::from_utf8_lossy(&o.stdout)));
    let zigbuild = quiet(Command::new("cargo").args(["zigbuild", "--version"]))
        .is_ok_and(|o| o.status.success());
    Toolchain {
        installed,
        zigbuild,
    }
}

/// Run without flashing a console window on Windows.
pub fn quiet(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    no_window(cmd).output()
}

pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn parse_target_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_commands_name_the_target() {
        let (prog, args) = cargo_command(BuildTarget::WindowsArm64, false);
        assert_eq!(prog, "cargo");
        assert_eq!(
            args.join(" "),
            "build -p misty_player --release --target aarch64-pc-windows-msvc --message-format human"
        );
        let (_, zig) = cargo_command(BuildTarget::LinuxArm64, true);
        assert_eq!(zig[0], "zigbuild");
        assert!(zig.contains(&"aarch64-unknown-linux-gnu".to_owned()));
        // zig only links Linux; Windows keeps the normal build.
        assert_eq!(cargo_command(BuildTarget::WindowsX64, true).1[0], "build");
        assert_eq!(
            install_command(BuildTarget::MacArm64),
            "rustup target add aarch64-apple-darwin"
        );
        let out = player_output(Path::new("t"), BuildTarget::LinuxX64);
        assert_eq!(
            out,
            Path::new("t/x86_64-unknown-linux-gnu/release/misty_player")
        );
    }

    #[test]
    fn status_reports_missing_targets_and_cross_os_blockers() {
        let list = parse_target_list("x86_64-pc-windows-msvc\n aarch64-unknown-linux-gnu \n\n");
        let host = Some(BuildTarget::WindowsX64);
        let s = |t, zig| status(t, host, Some(&list), zig);
        assert!(s(BuildTarget::WindowsX64, false).ready());
        let arm = s(BuildTarget::WindowsArm64, false);
        assert!(!arm.installed && arm.blocker.is_none());
        assert!(arm.note.unwrap().contains("MSVC"));
        let linux = s(BuildTarget::LinuxArm64, false);
        assert!(linux.installed && linux.blocker.unwrap().contains("cargo-zigbuild"));
        assert!(s(BuildTarget::LinuxArm64, true).ready(), "zig links Linux");
        let mac = s(BuildTarget::MacArm64, true);
        assert!(!mac.installed && mac.blocker.unwrap().contains("Mac"));
        // Without rustup we can't tell, so we try.
        assert!(status(BuildTarget::WindowsArm64, host, None, false).installed);
        // On a Mac, both Mac CPUs build.
        let mac_host = Some(BuildTarget::MacArm64);
        assert!(status(BuildTarget::MacX64, mac_host, None, false).ready());
    }
}
