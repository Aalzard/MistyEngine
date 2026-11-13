//! Project lock: one editor at a time writes a project.
//!
//! Two editors on the same folder both autosaved layouts, scenes and terrains over each other,
//! and the second took the AI tools' info file over. An editor now holds an operating-system
//! lock on `Library/ProjectLock` while the project is open, and writes its process id next to
//! it (`Library/ProjectLock.json`) for the message the next one shows. A second editor sees
//! the lock held and asks: open the project read-only (nothing of the project's assets or
//! settings is saved, see `misty_engine::asset_db::set_read_only`) or cancel.
//!
//! The lock belongs to the open file handle, so the system drops it when the editor exits or
//! crashes: a lock left by a crash is free again and is simply taken over (no stale-pid
//! guessing; the info file is rewritten).

use std::fs::{File, TryLockError};

use crate::i18n::tr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file the lock is held on.
pub const LOCK_FILE: &str = "Library/ProjectLock";
/// Who holds it.
pub const INFO_FILE: &str = "Library/ProjectLock.json";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    /// Editor version that holds it.
    #[serde(default)]
    pub version: String,
}

/// A held project lock (released when dropped).
#[derive(Debug)]
pub struct ProjectLock {
    file: File,
    info: PathBuf,
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.info);
        let _ = self.file.unlock();
    }
}

/// What opening a project found.
#[derive(Debug)]
pub enum Acquired {
    /// This editor holds the lock.
    Mine(ProjectLock),
    /// Another editor does (its info, when it could be read).
    Held(Option<LockInfo>),
}

/// Take the lock of the project at `root`. `Err` when the Library can't be written (a
/// read-only folder): the project then opens as before, unlocked.
pub fn acquire(root: &Path) -> std::io::Result<Acquired> {
    let path = root.join(LOCK_FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(Acquired::Held(read_info(root))),
        Err(TryLockError::Error(e)) => return Err(e),
    }
    let info = root.join(INFO_FILE);
    let me = LockInfo {
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    // Rewritten every time: after a crash it still names the dead editor.
    let _ = std::fs::write(&info, serde_json::to_string_pretty(&me).unwrap_or_default());
    Ok(Acquired::Mine(ProjectLock { file, info }))
}

/// Who holds the lock of the project at `root`, as it wrote it.
pub fn read_info(root: &Path) -> Option<LockInfo> {
    let text = std::fs::read_to_string(root.join(INFO_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The editor's side of the lock.
#[derive(Debug, Default)]
pub enum LockState {
    /// Not taken (tests, or a project whose Library can't be written).
    #[default]
    None,
    /// Held for its `Drop` (which releases the lock).
    Mine(#[allow(dead_code)] ProjectLock),
    /// Another editor has the project: open read-only. `asking` while the choice is on screen.
    ReadOnly {
        holder: Option<LockInfo>,
        asking: bool,
        _guard: ReadOnlyGuard,
    },
}

/// Keeps the project read-only while it lives (saves are allowed again once it is closed:
/// the next project, or the same one opened later, decides afresh).
#[derive(Debug)]
pub struct ReadOnlyGuard(PathBuf);

impl Drop for ReadOnlyGuard {
    fn drop(&mut self) {
        misty_engine::asset_db::set_read_only(&self.0, false);
    }
}

impl LockState {
    pub fn read_only(&self) -> bool {
        matches!(self, Self::ReadOnly { .. })
    }
}

/// Take the lock for an editor opening `root`; when another editor holds it, the project is
/// set read-only (before anything can save) and the choice is asked.
pub fn open(root: &Path) -> LockState {
    match acquire(root) {
        Ok(Acquired::Mine(lock)) => {
            misty_engine::asset_db::set_read_only(root, false);
            LockState::Mine(lock)
        }
        Ok(Acquired::Held(holder)) => {
            misty_engine::asset_db::set_read_only(root, true);
            LockState::ReadOnly {
                holder,
                asking: true,
                _guard: ReadOnlyGuard(root.to_path_buf()),
            }
        }
        Err(e) => {
            eprintln!("project lock not taken: {e}");
            LockState::None
        }
    }
}

/// The project is being closed: its lock is released and its saves no longer refused.
pub fn close(state: &mut LockState) {
    *state = LockState::None;
}

/// What the read-only question answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    ReadOnly,
    Cancel,
}

/// The modal question while another editor has the project. `Some` once answered.
pub fn prompt_ui(ctx: &egui::Context, state: &mut LockState) -> Option<Choice> {
    let LockState::ReadOnly { holder, asking, .. } = state else {
        return None;
    };
    if !*asking {
        return None;
    }
    let mut choice = None;
    egui::Modal::new(egui::Id::new("project_lock_prompt")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(tr!("Project Already Open"));
        ui.add_space(6.0);
        ui.label(match holder {
            Some(h) => tr!(
                "This project is open in another Misty editor (process {pid}).",
                pid = h.pid
            ),
            None => tr!("This project is open in another Misty editor."),
        });
        ui.label(
            egui::RichText::new(tr!(
                "Open it read-only to look around: scenes, assets and settings can't be saved."
            ))
            .weak(),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(tr!("Open Read-Only")).clicked() {
                choice = Some(Choice::ReadOnly);
            }
            if ui.button(tr!("Cancel")).clicked() {
                choice = Some(Choice::Cancel);
            }
        });
    });
    if choice.is_some() {
        *asking = false;
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("misty_lock_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        root
    }

    /// The first editor holds the lock and names itself; a second one finds it held, with
    /// the first one's process id; once the first closes, the next one takes it.
    #[test]
    fn a_second_editor_finds_the_project_locked() {
        let root = project("second");
        let Acquired::Mine(first) = acquire(&root).unwrap() else {
            panic!("the first editor gets the lock");
        };
        let info = read_info(&root).expect("lock info written");
        assert_eq!(info.pid, std::process::id());
        match acquire(&root).unwrap() {
            Acquired::Held(Some(h)) => assert_eq!(h.pid, std::process::id()),
            other => panic!("a second editor must find the lock held: {other:?}"),
        }
        drop(first);
        assert!(read_info(&root).is_none(), "info removed on close");
        assert!(matches!(acquire(&root).unwrap(), Acquired::Mine(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A crash leaves the lock file and its info behind, but no lock: the next editor takes
    /// it over and names itself.
    #[test]
    fn a_lock_left_by_a_crash_is_taken_over() {
        let root = project("crash");
        std::fs::create_dir_all(root.join("Library")).unwrap();
        std::fs::write(root.join(LOCK_FILE), "").unwrap();
        std::fs::write(
            root.join(INFO_FILE),
            r#"{"pid": 4000000000, "version": "0.3.0"}"#,
        )
        .unwrap();
        let Acquired::Mine(lock) = acquire(&root).unwrap() else {
            panic!("a dead editor's lock is free");
        };
        assert_eq!(read_info(&root).unwrap().pid, std::process::id());
        drop(lock);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The second editor's project is read-only until it closes, and asks first.
    #[test]
    fn opening_a_locked_project_makes_it_read_only() {
        let root = project("read_only");
        let mut first = open(&root);
        assert!(matches!(first, LockState::Mine(_)));
        let mut second = open(&root);
        assert!(second.read_only());
        assert!(matches!(second, LockState::ReadOnly { asking: true, .. }));
        assert!(misty_engine::asset_db::is_read_only(&root));
        assert!(misty_engine::asset_db::write_atomic(&root.join("Assets/A.mat"), "{}").is_err());
        close(&mut second);
        assert!(!misty_engine::asset_db::is_read_only(&root));
        close(&mut first);
        assert!(matches!(open(&root), LockState::Mine(_)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
