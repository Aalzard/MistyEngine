//! Which ticks of capture time become frames. Every tick of a recording advances the game by
//! exactly one frame of the recorder's frame rate; the schedule says whether that frame is
//! written (inside the range), skipped (before it) or whether the recording is over.

use crate::RecorderSettings;

/// What to do with the frame the game just advanced to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Before the range: run the frame, don't keep it.
    Skip,
    /// Capture it as frame `n`.
    Capture(u64),
    /// Past the range: the recording is over.
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// `first..=last`, `None` = until stopped.
    range: Option<(u64, u64)>,
    next: u64,
}

impl Schedule {
    pub fn new(range: Option<(u64, u64)>) -> Self {
        Self { range, next: 0 }
    }

    pub fn for_settings(s: &RecorderSettings) -> Self {
        Self::new(s.frame_range())
    }

    /// The next frame of capture time.
    pub fn step(&mut self) -> Step {
        let f = self.next;
        match self.range {
            Some((_, last)) if f > last => Step::Done,
            Some((first, _)) if f < first => {
                self.next += 1;
                Step::Skip
            }
            _ => {
                self.next += 1;
                Step::Capture(f)
            }
        }
    }

    /// Every frame of the range has been handed out.
    pub fn complete(&self) -> bool {
        self.range.is_some_and(|(_, last)| self.next > last)
    }

    /// Frames stepped through so far (skipped ones included).
    pub fn elapsed(&self) -> u64 {
        self.next
    }

    /// Frames the range holds (`None` = open-ended).
    pub fn total(&self) -> Option<u64> {
        self.range.map(|(a, b)| b - a + 1)
    }
}
