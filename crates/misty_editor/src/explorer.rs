//! "Show in Explorer" / "Reveal in Finder": the OS file browser command for a path.
//!
//! A folder opens itself (`<Project>/Assets/Prefabs`), a file opens its folder with the file
//! selected. On Windows `explorer` parses `/select,"<path>"` itself: the standard library's
//! quoting (`"/select,E:\My Game\…"`, quotes around the whole argument once the path has a
//! space) made it ignore the argument and open Documents, so the command line is written raw.

use std::path::Path;

/// Whether a folder is opened or selected in its parent (files are always selected).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reveal {
    /// Project window, search palette, Assets menu.
    OpenFolders,
    /// The Hub's project list: the project folder selected in its parent.
    Select,
}

/// A file browser command: the program and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub program: &'static str,
    pub args: Vec<String>,
    /// Windows: `args` are passed as one raw command line, exactly as written.
    pub raw: bool,
    /// Linux: what to open when the file manager's D-Bus `ShowItems` call fails.
    pub fallback: Option<(&'static str, String)>,
}

/// Show `path` in the OS file browser.
pub fn show(path: &Path, reveal: Reveal) -> std::io::Result<()> {
    spawn(&command(path, path.is_dir(), reveal))
}

/// The command that shows `path` (`is_dir`: it is a folder) in this OS's file browser. A
/// relative path is made absolute first.
pub fn command(path: &Path, is_dir: bool, reveal: Reveal) -> Command {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let select = !is_dir || reveal == Reveal::Select;
    command_for(&abs.to_string_lossy(), select, std::env::consts::OS)
}

/// [`command`] for an absolute path on `os` ("windows", "macos", else Linux and the BSDs).
fn command_for(path: &str, select: bool, os: &str) -> Command {
    let plain = |program, args: Vec<String>| Command {
        program,
        args,
        raw: false,
        fallback: None,
    };
    match os {
        "windows" => {
            let mut p = path
                .strip_prefix(r"\\?\")
                .unwrap_or(path)
                .replace('/', "\\");
            // `"C:\a\"` would read as an escaped quote; a drive root keeps its backslash.
            while p.ends_with('\\') && p.len() > 3 {
                p.pop();
            }
            let arg = if select {
                format!("/select,\"{p}\"")
            } else {
                format!("\"{p}\"")
            };
            Command {
                program: "explorer",
                args: vec![arg],
                raw: true,
                fallback: None,
            }
        }
        "macos" if select => plain("open", vec!["-R".into(), path.into()]),
        "macos" => plain("open", vec![path.into()]),
        _ if select => {
            let parent = Path::new(path)
                .parent()
                .map_or_else(|| path.to_owned(), |p| p.to_string_lossy().into_owned());
            Command {
                program: "dbus-send",
                args: vec![
                    "--session".into(),
                    "--print-reply".into(),
                    "--dest=org.freedesktop.FileManager1".into(),
                    "--type=method_call".into(),
                    "/org/freedesktop/FileManager1".into(),
                    "org.freedesktop.FileManager1.ShowItems".into(),
                    format!("array:string:{}", file_uri(path)),
                    "string:".into(),
                ],
                raw: false,
                fallback: Some(("xdg-open", parent)),
            }
        }
        _ => plain("xdg-open", vec![path.into()]),
    }
}

/// `file://` URI of an absolute path, spaces and other reserved bytes percent-encoded.
fn file_uri(path: &str) -> String {
    let mut out = String::from("file://");
    for b in path.replace('\\', "/").bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn spawn(c: &Command) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(c.program);
    #[cfg(windows)]
    if c.raw {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(c.args.join(" "));
    } else {
        cmd.args(&c.args);
    }
    #[cfg(not(windows))]
    cmd.args(&c.args);
    let Some((program, arg)) = c.fallback.clone() else {
        return cmd.spawn().map(|_| ());
    };
    // D-Bus first (selects the file); without a file manager service, open the folder.
    let child = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    std::thread::spawn(move || {
        let ok = child
            .ok()
            .and_then(|mut c| c.wait().ok())
            .is_some_and(|s| s.success());
        if !ok {
            let _ = std::process::Command::new(program).arg(arg).spawn();
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_opens_folders_and_selects_files_with_spaces_quoted() {
        let folder = command_for(r"E:\My Game\Assets\Prefabs", false, "windows");
        assert_eq!(folder.program, "explorer");
        assert!(folder.raw);
        assert_eq!(folder.args, [r#""E:\My Game\Assets\Prefabs""#]);
        let file = command_for(r"E:\My Game\Assets\Red Metal.mat", true, "windows");
        assert_eq!(file.args, [r#"/select,"E:\My Game\Assets\Red Metal.mat""#]);
        // Forward slashes, a verbatim prefix and a trailing separator are tidied.
        let odd = command_for(r"\\?\C:\Projects/Space Game/Assets\", false, "windows");
        assert_eq!(odd.args, [r#""C:\Projects\Space Game\Assets""#]);
        assert_eq!(command_for(r"C:\", false, "windows").args, [r#""C:\""#]);
    }

    #[test]
    fn macos_and_linux_commands() {
        let mac = command_for("/Users/a/My Game/Assets/x.mat", true, "macos");
        assert_eq!(mac.program, "open");
        assert_eq!(mac.args, ["-R", "/Users/a/My Game/Assets/x.mat"]);
        let mac_dir = command_for("/Users/a/My Game/Assets", false, "macos");
        assert_eq!(mac_dir.args, ["/Users/a/My Game/Assets"]);

        let dir = command_for("/home/a/My Game/Assets/Prefabs", false, "linux");
        assert_eq!(dir.program, "xdg-open");
        assert_eq!(dir.args, ["/home/a/My Game/Assets/Prefabs"]);
        let file = command_for("/home/a/My Game/Assets/Red.mat", true, "linux");
        assert_eq!(file.program, "dbus-send");
        assert!(
            file.args
                .contains(&"array:string:file:///home/a/My%20Game/Assets/Red.mat".to_owned()),
            "{:?}",
            file.args
        );
        assert_eq!(
            file.fallback,
            Some(("xdg-open", "/home/a/My Game/Assets".to_owned()))
        );
    }

    #[test]
    fn a_folder_opens_itself_and_a_file_is_selected() {
        let dir = std::env::temp_dir().join(format!("misty explorer {}", std::process::id()));
        std::fs::create_dir_all(dir.join("Prefabs")).unwrap();
        std::fs::write(dir.join("Crate.prefab"), "{}").unwrap();
        let folder = dir.join("Prefabs");
        let open = command(&folder, folder.is_dir(), Reveal::OpenFolders);
        assert!(
            !open.args.iter().any(|a| a.contains("/select") || a == "-R"),
            "{open:?}"
        );
        assert!(open.args.iter().any(|a| a.contains("Prefabs")));
        let file = dir.join("Crate.prefab");
        let sel = command(&file, file.is_dir(), Reveal::OpenFolders);
        assert!(sel.args.iter().any(|a| a.contains("Crate.prefab")));
        if cfg!(windows) {
            assert!(sel.args[0].starts_with("/select,\""), "{:?}", sel.args);
            // The Hub selects a project folder in its parent.
            let hub = command(&folder, true, Reveal::Select);
            assert!(hub.args[0].starts_with("/select,\""), "{:?}", hub.args);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
