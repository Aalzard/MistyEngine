//! Finding the game to play: a built game's `<Product>_Data/data.pak` next to the executable
//! (mounted as the asset file system), a loose project folder, or the nearest `SampleProject`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use misty_engine::build::{BUILD_INFO_FILE, BuildInfo, PAK_FILE};
use misty_engine::pak::Pak;

/// Where the game comes from.
pub struct Launch {
    /// Project root (a data folder for packed builds).
    pub root: PathBuf,
    /// Assets come from a mounted pak.
    pub packed: bool,
    pub info: BuildInfo,
}

/// The game for command-line argument `arg` (a project or data folder), or the one next to the
/// executable.
pub fn locate(arg: Option<PathBuf>) -> Result<Launch, String> {
    let exe = std::env::current_exe().ok();
    let root = match arg {
        Some(dir) => dir,
        None => exe
            .as_deref()
            .and_then(data_folder)
            .unwrap_or_else(default_project),
    };
    let packed = root.join(PAK_FILE).is_file();
    if packed {
        mount(&root)?;
    }
    let info = misty_engine::vfs::read(root.join(BUILD_INFO_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        // Loose project folders are development runs.
        .unwrap_or(BuildInfo {
            development: !packed,
            ..Default::default()
        });
    Ok(Launch { root, packed, info })
}

fn mount(root: &Path) -> Result<(), String> {
    let path = root.join(PAK_FILE);
    let pak = Pak::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stamp = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    misty_engine::vfs::mount(root, pak, stamp);
    Ok(())
}

/// `<exe stem>_Data` next to the executable (or in an app bundle's Resources), else any
/// `*_Data` folder there holding a pak.
pub fn data_folder(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let stem = exe.file_stem()?.to_string_lossy().into_owned();
    let name = misty_engine::build::data_folder(&stem);
    let candidates = [dir.join(&name), dir.join("../Resources").join(&name)];
    if let Some(found) = candidates.iter().find(|d| d.join(PAK_FILE).is_file()) {
        return Some(found.clone());
    }
    let mut others: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("_Data"))
                && p.join(PAK_FILE).is_file()
        })
        .collect();
    others.sort();
    others.into_iter().next()
}

/// The folder the executable sits in when it is a loose build, else the nearest `SampleProject`.
fn default_project() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    if let Some(dir) = &exe_dir
        && dir.join("Assets").is_dir()
    {
        return dir.clone();
    }
    [std::env::current_dir().ok(), exe_dir]
        .into_iter()
        .flatten()
        .flat_map(|start| {
            start
                .ancestors()
                .map(|a| a.join("SampleProject"))
                .collect::<Vec<_>>()
        })
        .find(|p| p.join("Assets").is_dir())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Window overrides from the command line (the editor's Multiplayer Play Mode places its
/// player windows with them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowOptions {
    pub title: Option<String>,
    pub size: Option<(u32, u32)>,
    pub position: Option<(i32, i32)>,
}

/// Everything the command line asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    /// The project or data folder (the first plain argument).
    pub project: Option<PathBuf>,
    pub launch: crate::game::LaunchOptions,
    pub window: WindowOptions,
}

/// Parse `misty_player [Project] [--scene Assets/…scene] [--scene-file file] [--net-host |
/// --net-server | --net-client host[:port]] [--title text] [--size 800x450] [--pos 40,40]`.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    use misty_engine::network::AutoStart;
    let mut out = Args::default();
    let mut it = args.into_iter();
    fn value(it: &mut dyn Iterator<Item = String>, flag: &str) -> Result<String, String> {
        it.next().ok_or_else(|| format!("{flag} needs a value"))
    }
    fn pair(text: &str, sep: char, flag: &str) -> Result<(i64, i64), String> {
        let (a, b) = text
            .split_once(sep)
            .ok_or_else(|| format!("{flag} expects A{sep}B, got '{text}'"))?;
        let n = |s: &str| s.trim().parse::<i64>().map_err(|e| format!("{flag}: {e}"));
        Ok((n(a)?, n(b)?))
    }
    while let Some(a) = it.next() {
        match a.as_str() {
            "--scene" => out.launch.scene = Some(value(&mut it, &a)?),
            "--scene-file" => out.launch.scene_file = Some(PathBuf::from(value(&mut it, &a)?)),
            "--net-host" => out.launch.network = Some(AutoStart::Host),
            "--net-server" => out.launch.network = Some(AutoStart::Server),
            "--net-client" => out.launch.network = Some(AutoStart::Client(value(&mut it, &a)?)),
            "--title" => out.window.title = Some(value(&mut it, &a)?),
            "--size" => {
                let (w, h) = pair(&value(&mut it, &a)?, 'x', "--size")?;
                out.window.size = Some((w.clamp(160, 16384) as u32, h.clamp(120, 16384) as u32));
            }
            "--pos" => {
                let (x, y) = pair(&value(&mut it, &a)?, ',', "--pos")?;
                out.window.position = Some((x as i32, y as i32));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ if out.project.is_none() => out.project = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument '{a}'")),
        }
    }
    Ok(out)
}

/// Development builds get a console window for the log (release builds have none).
pub fn open_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AllocConsole() -> i32;
        }
        // SAFETY: no arguments; failure (a console already exists) is harmless.
        unsafe {
            AllocConsole();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_options() {
        use misty_engine::network::AutoStart;
        let args = |v: &[&str]| parse_args(v.iter().map(|s| s.to_string()));
        let a = args(&[
            "C:/Game",
            "--scene",
            "Assets/Scenes/Net.scene",
            "--net-client",
            "127.0.0.1:7777",
            "--title",
            "Player 2",
            "--size",
            "800x450",
            "--pos",
            "40,-10",
        ])
        .unwrap();
        assert_eq!(a.project, Some(PathBuf::from("C:/Game")));
        assert_eq!(a.launch.scene.as_deref(), Some("Assets/Scenes/Net.scene"));
        assert_eq!(
            a.launch.network,
            Some(AutoStart::Client("127.0.0.1:7777".into()))
        );
        assert_eq!(a.window.title.as_deref(), Some("Player 2"));
        assert_eq!(a.window.size, Some((800, 450)));
        assert_eq!(a.window.position, Some((40, -10)));
        assert_eq!(args(&[]).unwrap(), Args::default());
        assert_eq!(
            args(&["--net-host"]).unwrap().launch.network,
            Some(AutoStart::Host)
        );
        assert!(args(&["--size", "big"]).is_err());
        assert!(args(&["--what"]).is_err());
        assert!(args(&["--scene"]).is_err());
    }

    #[test]
    fn finds_the_data_folder_named_after_the_executable() {
        let dir = std::env::temp_dir().join(format!("misty_locate_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Ball_Data")).unwrap();
        std::fs::create_dir_all(dir.join("Other_Data")).unwrap();
        std::fs::write(dir.join("Ball_Data").join(PAK_FILE), b"x").unwrap();
        let exe = dir.join("Ball.exe");
        assert_eq!(data_folder(&exe), Some(dir.join("Ball_Data")));
        // A renamed executable still finds the only data folder with a pak.
        assert_eq!(
            data_folder(&dir.join("Renamed.exe")),
            Some(dir.join("Ball_Data"))
        );
        std::fs::remove_file(dir.join("Ball_Data").join(PAK_FILE)).unwrap();
        assert_eq!(data_folder(&exe), None);
    }
}
