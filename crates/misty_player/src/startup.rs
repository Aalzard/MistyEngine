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
