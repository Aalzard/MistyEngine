//! Version control through the `git` command line: status badges on Project files and the
//! Hierarchy's scene rows, and the Window ▸ Version Control window (changed files, diffs, stage /
//! unstage, commit, discard, scene diffs and the scene merge helper). There are no push or pull
//! buttons; the window shows the commands to run instead.
//!
//! The editor app turns polling on ([`Vcs::start`]); tests drive temp repositories directly.

pub mod git;
pub mod scene_diff;
pub mod scene_doc;
pub mod scene_merge;
pub mod status;
pub mod window;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{Align2, FontId, Pos2, Rect, Ui, vec2};

pub use status::Badge;

/// How often the status is read again while the editor runs.
const POLL: Duration = Duration::from_secs(3);

type Found = Result<(git::Repo, status::Status), String>;

/// The project's repository, its status and the Version Control window.
#[derive(Default)]
pub struct Vcs {
    /// Status is polled (set by the editor app, off in tests).
    pub enabled: bool,
    pub repo: Option<git::Repo>,
    /// Why there is no repository ("not a git repository", git missing).
    pub error: Option<String>,
    pub status: status::Status,
    /// Bumped each time a new status arrives (diff caches follow it).
    pub generation: u64,
    pending: Option<mpsc::Receiver<Found>>,
    last_poll: Option<Instant>,
    pub window: window::WindowState,
}

impl Vcs {
    /// Start polling the repository holding `project`.
    pub fn start(&mut self, project: &Path) {
        self.enabled = true;
        self.refresh(project);
    }

    /// Read the status again on a worker thread.
    pub fn refresh(&mut self, project: &Path) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let project: PathBuf = project.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("misty git status".into())
            .spawn(move || {
                let _ = tx.send(read(&project));
            });
        if spawned.is_ok() {
            self.pending = Some(rx);
            self.last_poll = Some(Instant::now());
        }
    }

    /// Read the status now (after a stage, commit or discard).
    pub fn refresh_now(&mut self, project: &Path) {
        self.pending = None;
        self.last_poll = Some(Instant::now());
        self.apply(read(project));
    }

    /// Once per editor frame: take a finished status, start the next poll when due. True when
    /// the badges changed.
    pub fn frame(&mut self, project: &Path) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(found) => {
                    self.pending = None;
                    changed = self.apply(found);
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.enabled && self.last_poll.is_none_or(|t| t.elapsed() >= POLL) {
            self.refresh(project);
        }
        changed
    }

    fn apply(&mut self, found: Found) -> bool {
        let before = (self.repo.clone(), self.status.clone());
        match found {
            Ok((repo, status)) => {
                self.repo = Some(repo);
                self.status = status;
                self.error = None;
            }
            Err(e) => {
                self.repo = None;
                self.status = status::Status::default();
                self.error = Some(e);
            }
        }
        let changed = before != (self.repo.clone(), self.status.clone());
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// The badge of a project-relative file or folder ("Assets/Scenes/Main.scene").
    pub fn badge(&self, rel: &str) -> Option<Badge> {
        self.repo.as_ref()?;
        self.status.badge(rel)
    }

    /// The badge of an absolute path inside the project.
    pub fn badge_at(&self, root: &Path, path: &Path) -> Option<Badge> {
        let rel = path.strip_prefix(root).ok()?;
        self.badge(&rel.to_string_lossy().replace('\\', "/"))
    }
}

fn read(project: &Path) -> Found {
    let repo = git::discover(project)?;
    let status = repo.status()?;
    Ok((repo, status))
}

/// A small coloured letter badge (M, A, ?, !, D) centred on `at`.
pub fn paint_badge(ui: &Ui, at: Pos2, badge: Badge, size: f32) {
    let rect = Rect::from_center_size(at, vec2(size, size));
    let painter = ui.painter();
    painter.rect_filled(rect, size * 0.25, badge.color());
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        badge.letter(),
        FontId::proportional(size * 0.8),
        egui::Color32::from_rgb(20, 20, 20),
    );
}

/// Hover text for a badge.
pub fn badge_tip(badge: Badge) -> String {
    format!("Version control: {}", badge.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_reads_badges_from_a_temp_repo() {
        let (top, project) = git::tests::temp_repo("poll");
        std::fs::write(project.join("Assets/Readme.txt"), "changed\n").unwrap();
        let mut v = Vcs::default();
        v.start(&project);
        let t = Instant::now();
        while v.repo.is_none() && v.error.is_none() && t.elapsed() < Duration::from_secs(20) {
            v.frame(&project);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(v.repo.is_some(), "{:?}", v.error);
        assert_eq!(v.badge("Assets/Readme.txt"), Some(Badge::Modified));
        assert_eq!(v.badge("Assets"), Some(Badge::Modified));
        assert_eq!(
            v.badge_at(&project, &project.join("Assets").join("Readme.txt")),
            Some(Badge::Modified)
        );
        assert_eq!(v.badge("Assets/Scripts/Player.lua"), None);
        std::fs::remove_dir_all(top).ok();
    }

    #[test]
    fn no_repository_gives_an_error_and_no_badges() {
        let lone = std::env::temp_dir().join(format!("misty_vcs_norepo_{}", std::process::id()));
        std::fs::create_dir_all(lone.join("Assets")).unwrap();
        let mut v = Vcs::default();
        v.refresh_now(&lone);
        assert!(v.repo.is_none() && v.error.is_some());
        assert_eq!(v.badge("Assets"), None);
        std::fs::remove_dir_all(lone).ok();
    }
}
