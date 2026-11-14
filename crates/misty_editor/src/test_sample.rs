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

#[cfg(test)]
mod tests {
    /// No test opens the repo's sample in the editor (`EditorState::new`, `AssetDatabase::open`)
    /// or makes it the global project root, except the ignored generators that write it on
    /// purpose: that imports the sample (extracting `Crate.fbx`'s materials next to it) and
    /// resolves its packages (rewriting `packages-lock.json`), leaving files in the repo after a
    /// full test run. The theme panel tests did, after the round 60 cleanup.
    #[test]
    fn tests_never_open_the_repo_sample() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        let mut stack = vec![src];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    files.push(p);
                }
            }
        }
        let opens = [
            "EditorState::new(",
            "AssetDatabase::open(",
            "set_project_root(",
        ];
        let mut bad = Vec::new();
        for f in files {
            if f.ends_with("test_sample.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                let path = concat!("\"../../Sample", "Project\")");
                // `project()` helpers of the sample generators return the repo's folder.
                let repo = l.contains(path)
                    || (l.trim() == "let root = project();"
                        && text.contains(&format!(
                            "fn project() -> PathBuf {{\n    PathBuf::from(env!(\"CARGO_MANIFEST_DIR\")).join({path})"
                        )));
                if !repo {
                    continue;
                }
                let window = &lines[i..(i + 8).min(lines.len())];
                let opened = window.iter().any(|w| opens.iter().any(|o| w.contains(o)));
                let copied = window.iter().any(|w| {
                    w.contains("copy_all") || w.contains("temp_dir") || w.contains("on purpose")
                });
                // The enclosing function's attributes: ignored generators may write the sample.
                let start = (0..=i).rev().find(|&j| {
                    lines[j].trim_start().starts_with("fn ")
                        || lines[j].trim_start().starts_with("pub fn ")
                });
                let ignored = start.is_some_and(|j| {
                    lines[j.saturating_sub(3)..j]
                        .iter()
                        .any(|a| a.contains("#[ignore"))
                });
                if opened && !copied && !ignored {
                    bad.push(format!("{}:{}", f.display(), i + 1));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "tests open the repo's SampleProject: {bad:?}"
        );
    }
}
