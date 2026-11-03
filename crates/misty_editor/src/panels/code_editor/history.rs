//! Per-file undo/redo of whole-text snapshots, grouped by typing bursts.

use std::time::{Duration, Instant};

use super::text::Selection;

/// What kind of edit was made; consecutive edits of the same burst kind undo together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Typing characters (a word and the spaces around it undo as one).
    Typing,
    /// Backspace / Delete.
    Erase,
    /// Anything else (paste, line commands, replace…): always its own step.
    Other,
}

/// Text and carets before an edit.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub lines: Vec<String>,
    pub sels: Vec<Selection>,
    pub version: u64,
}

/// Typing pauses longer than this start a new undo step.
const BURST_GAP: Duration = Duration::from_millis(1200);
const LIMIT: usize = 1000;

#[derive(Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Kind and time of the last edit while its burst can still grow.
    open: Option<(EditKind, Instant)>,
}

impl History {
    /// Called before an edit: whether it starts a new undo step (then [`Self::push`] the state
    /// from before it) rather than continuing the open burst.
    pub fn begin(&mut self, kind: EditKind, now: Instant) -> bool {
        self.redo.clear();
        let continues = kind != EditKind::Other
            && self
                .open
                .is_some_and(|(k, t)| k == kind && now.duration_since(t) < BURST_GAP);
        self.open = (kind != EditKind::Other).then_some((kind, now));
        !continues
    }

    pub fn push(&mut self, before: Snapshot) {
        self.undo.push(before);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }

    /// End the current burst (caret moved, new line typed, …).
    pub fn break_group(&mut self) {
        self.open = None;
    }

    /// Swap `current` for the previous snapshot.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        self.open = None;
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        self.open = None;
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }
}
