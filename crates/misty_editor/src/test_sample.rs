//! One temp copy of the repo's `SampleProject` per test process, for tests that open the
//! sample in the editor (`EditorState::new`, `AssetDatabase::open`) but only read it.
//!
//! Opening a project writes into it: new assets get `.meta` files, models get their materials
//! extracted next to them (`Crate.fbx` → `Crate_Metal.mat`, `Crate_Metal_Normal.png`,
//! `Crate_Wood.mat`), the Library is filled. Tests that opened the real folder left those
//! files in the repo after every full test run. Tests that change the project make their own
//! copies instead.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The repo's sample project (read it, never open it in the editor: see the module docs).
pub fn repo_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

const PREFIX: &str = "misty_sample_copy_";

/// The shared copy (Assets, Packages and ProjectSettings; no Library), made on first use.
/// Copies left by earlier test processes are removed.
pub fn shared_copy() -> PathBuf {
    static COPY: OnceLock<PathBuf> = OnceLock::new();
    COPY.get_or_init(|| {
        let temp = std::env::temp_dir();
        remove_stale_copies(&temp);
        let root = temp.join(format!("{PREFIX}{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = repo_sample();
        for dir in ["Assets", "Packages", "ProjectSettings"] {
            if src.join(dir).is_dir() {
                crate::asset_ops::copy_all(&src.join(dir), &root.join(dir))
                    .unwrap_or_else(|e| panic!("copying the sample project's {dir}: {e}"));
            }
        }
        // Canonical, as the editor's own project paths are.
        std::fs::canonicalize(&root).unwrap_or(root)
    })
    .clone()
}

/// Removes `misty_sample_copy_<pid>` folders of processes that are gone (an hour old: a test
/// process runs for minutes).
fn remove_stale_copies(temp: &Path) {
    let own = format!("{PREFIX}{}", std::process::id());
    for entry in std::fs::read_dir(temp).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(PREFIX) || name == own {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() > 3600);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
