//! The editing core of one file: text, carets/selections (several for multi-caret editing),
//! undo history and caret movement. Editing commands live in `commands`.

use std::time::Instant;

use super::history::{EditKind, History, Snapshot};
use super::text::{Buffer, Pos, Selection, adjust, is_word};

pub struct TextEditor {
    pub buf: Buffer,
    /// Sorted, non-overlapping selections; `primary` is the one the view follows.
    pub sels: Vec<Selection>,
    pub primary: usize,
    history: History,
    /// Changes on every edit; undo brings back the version it had (for the dirty marker).
    pub version: u64,
    next_version: u64,
    /// Spaces per indent level.
    pub tab_size: usize,
    /// Fixed time for tests of undo grouping.
    pub clock: Option<Instant>,
    /// Column-selection anchor while Alt+Shift selecting.
    pub box_anchor: Option<Pos>,
    /// Positions that move with edits (the view keeps folded blocks here).
    pub markers: Vec<Pos>,
}

impl TextEditor {
    pub fn new(text: &str) -> Self {
        Self {
            buf: Buffer::from_text(text),
            sels: vec![Selection::default()],
            primary: 0,
            history: History::default(),
            version: 0,
            next_version: 1,
            tab_size: 4,
            clock: None,
            box_anchor: None,
            markers: Vec::new(),
        }
    }

    pub fn text(&self) -> String {
        self.buf.text()
    }

    pub fn primary(&self) -> Selection {
        self.sels[self.primary]
    }

    pub fn caret(&self) -> Pos {
        self.primary().head
    }

    pub fn set_caret(&mut self, p: Pos) {
        self.set_selection(Selection::caret(self.buf.clamp(p)));
    }

    /// A single selection.
    pub fn set_selection(&mut self, sel: Selection) {
        self.sels = vec![Selection {
            anchor: self.buf.clamp(sel.anchor),
            head: self.buf.clamp(sel.head),
            goal: sel.goal,
        }];
        self.primary = 0;
        self.history.break_group();
    }

    /// Add a selection (Alt+click, next occurrence); it becomes the primary one.
    pub fn add_selection(&mut self, sel: Selection) {
        self.sels.push(sel);
        self.primary = self.sels.len() - 1;
        self.normalize();
        self.history.break_group();
    }

    /// Several selections at once; the last one is primary.
    pub fn set_selections(&mut self, sels: Vec<Selection>) {
        if sels.is_empty() {
            return;
        }
        self.primary = sels.len() - 1;
        self.sels = sels;
        self.normalize();
        self.history.break_group();
    }

    /// Back to one caret (Esc).
    pub fn single_caret(&mut self) {
        let p = self.primary();
        self.sels = vec![p];
        self.primary = 0;
        self.box_anchor = None;
    }

    /// Sort selections and merge overlapping ones, keeping track of the primary.
    pub fn normalize(&mut self) {
        let primary = self.sels[self.primary.min(self.sels.len() - 1)];
        for s in &mut self.sels {
            s.anchor = self.buf.clamp(s.anchor);
            s.head = self.buf.clamp(s.head);
        }
        self.sels.sort_by_key(|s| s.start());
        let mut out: Vec<Selection> = Vec::with_capacity(self.sels.len());
        for s in self.sels.drain(..) {
            if let Some(last) = out.last_mut()
                && (s.start() < last.end() || s.start() == last.end() && s.is_empty())
            {
                let (start, end) = (last.start(), last.end().max(s.end()));
                let forward = last.head >= last.anchor;
                *last = if forward {
                    Selection::new(start, end)
                } else {
                    Selection::new(end, start)
                };
                continue;
            }
            out.push(s);
        }
        self.sels = out;
        self.primary = self
            .sels
            .iter()
            .position(|s| s.start() <= primary.head && primary.head <= s.end())
            .unwrap_or(self.sels.len() - 1);
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.buf.lines().to_vec(),
            sels: self.sels.clone(),
            version: self.version,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.buf.set_lines(s.lines);
        self.sels = s.sels;
        self.primary = self.sels.len() - 1;
        self.version = s.version;
        self.normalize();
    }

    pub fn undo(&mut self) -> bool {
        let current = self.snapshot();
        match self.history.undo(current) {
            Some(s) => {
                self.restore(s);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        let current = self.snapshot();
        match self.history.redo(current) {
            Some(s) => {
                self.restore(s);
                true
            }
            None => false,
        }
    }

    pub fn break_undo_group(&mut self) {
        self.history.break_group();
    }

    /// Replace the whole text (reload from disk) as one undoable step, keeping carets in range.
    pub fn reload(&mut self, text: &str) {
        let new = Buffer::from_text(text);
        if new.lines() == self.buf.lines() {
            return;
        }
        let end = self.buf.end();
        self.apply(
            EditKind::Other,
            vec![(Pos::default(), end, new.lines().join("\n"))],
        );
        self.buf.crlf = new.crlf;
    }

    /// Apply non-overlapping edits `(start, end, new text)`, moving every selection along.
    /// Returns where each edit's new text ends up, in the input order.
    pub fn apply(&mut self, kind: EditKind, edits: Vec<(Pos, Pos, String)>) -> Vec<(Pos, Pos)> {
        if edits.is_empty() {
            return Vec::new();
        }
        let now = self.clock.unwrap_or_else(Instant::now);
        if self.history.begin(kind, now) {
            let before = self.snapshot();
            self.history.push(before);
        }
        let mut order: Vec<usize> = (0..edits.len()).collect();
        // Bottom-up, so earlier edits' positions stay valid.
        order.sort_by_key(|&i| std::cmp::Reverse(edits[i].0.min(edits[i].1)));
        let mut done: Vec<Option<(Pos, Pos)>> = vec![None; edits.len()];
        for i in order {
            let (a, b, text) = &edits[i];
            let (s, e) = (self.buf.clamp(*a.min(b)), self.buf.clamp(*a.max(b)));
            let new_end = self.buf.replace(s, e, text);
            let move_pos = |p: Pos| adjust(p, s, e, new_end);
            for sel in &mut self.sels {
                sel.anchor = move_pos(sel.anchor);
                sel.head = move_pos(sel.head);
            }
            for m in &mut self.markers {
                *m = move_pos(*m);
            }
            for r in done.iter_mut().flatten() {
                *r = (move_pos(r.0), move_pos(r.1));
            }
            done[i] = Some((s, new_end));
        }
        self.version = self.next_version;
        self.next_version += 1;
        self.box_anchor = None;
        self.normalize();
        done.into_iter().flatten().collect()
    }

    /// Replace each selection with `f(selected text)`; carets go after the new text.
    pub fn replace_selections(&mut self, kind: EditKind, f: impl Fn(&str) -> String) {
        let edits: Vec<_> = self
            .sels
            .iter()
            .map(|s| (s.start(), s.end(), f(&self.buf.slice(s.start(), s.end()))))
            .collect();
        let ranges = self.apply(kind, edits);
        let sels = ranges
            .into_iter()
            .map(|(_, e)| Selection::caret(e))
            .collect();
        let primary = self.primary;
        self.sels = sels;
        self.primary = primary.min(self.sels.len() - 1);
        self.normalize();
    }

    /// Move every caret with `f`; `extend` keeps the anchors (Shift).
    pub fn move_carets(&mut self, extend: bool, f: impl Fn(&Buffer, &Selection) -> Pos) {
        for s in &mut self.sels {
            let p = self.buf.clamp(f(&self.buf, s));
            s.head = p;
            if !extend {
                s.anchor = p;
            }
            s.goal = None;
        }
        self.box_anchor = None;
        self.history.break_group();
        self.normalize();
    }

    /// Left/Right: an unextended move with a selection collapses it to that side.
    pub fn move_horizontal(&mut self, right: bool, word: bool, extend: bool) {
        self.move_carets(extend, |b, s| {
            if !extend && !s.is_empty() && !word {
                return if right { s.end() } else { s.start() };
            }
            match (right, word) {
                (true, false) => b.next(s.head),
                (false, false) => b.prev(s.head),
                (true, true) => word_right(b, s.head),
                (false, true) => word_left(b, s.head),
            }
        });
    }

    /// Up/Down by `lines`, keeping the goal column. `visible` maps a line to the next one shown
    /// (folded lines are skipped).
    pub fn move_vertical(
        &mut self,
        lines: isize,
        extend: bool,
        visible: &dyn Fn(usize, isize) -> usize,
    ) {
        for s in &mut self.sels {
            let goal = s.goal.unwrap_or(s.head.col);
            let p = if !extend && !s.is_empty() && lines.abs() == 1 {
                if lines < 0 { s.start() } else { s.end() }
            } else {
                s.head
            };
            let target = visible(p.line, lines);
            let head = if target == p.line && lines < 0 {
                Pos::new(p.line, 0)
            } else if target == p.line && lines > 0 {
                Pos::new(p.line, self.buf.line_len(p.line))
            } else {
                Pos::new(target, goal.min(self.buf.line_len(target)))
            };
            s.head = head;
            if !extend {
                s.anchor = head;
            }
            s.goal = Some(goal);
        }
        self.box_anchor = None;
        self.history.break_group();
        self.normalize();
    }

    /// Home: first non-blank character, or column 0 when already there (smart home).
    pub fn home(&mut self, extend: bool) {
        self.move_carets(extend, |b, s| {
            let indent = b.indent_of(s.head.line).chars().count();
            let col = if s.head.col == indent { 0 } else { indent };
            Pos::new(s.head.line, col)
        });
    }

    pub fn end(&mut self, extend: bool) {
        self.move_carets(extend, |b, s| {
            Pos::new(s.head.line, b.line_len(s.head.line))
        });
    }

    pub fn doc_start(&mut self, extend: bool) {
        self.move_carets(extend, |_, _| Pos::default());
        self.collapse_to_primary();
    }

    pub fn doc_end(&mut self, extend: bool) {
        self.move_carets(extend, |b, _| b.end());
        self.collapse_to_primary();
    }

    fn collapse_to_primary(&mut self) {
        if self.sels.len() > 1 {
            self.single_caret();
        }
    }

    pub fn select_all(&mut self) {
        self.set_selection(Selection::new(Pos::default(), self.buf.end()));
    }

    /// Select the word at `p` (double-click).
    pub fn select_word_at(&mut self, p: Pos) -> Selection {
        match self.buf.word_at(p) {
            Some((s, e)) => Selection::new(s, e),
            None => Selection::caret(p),
        }
    }

    /// Select line `i` including its line break (triple-click, gutter click).
    pub fn line_selection(&self, i: usize) -> Selection {
        let end = if i + 1 < self.buf.line_count() {
            Pos::new(i + 1, 0)
        } else {
            Pos::new(i, self.buf.line_len(i))
        };
        Selection::new(Pos::new(i, 0), end)
    }

    /// Column (box) selection from `anchor` to `head`: one selection per line.
    pub fn box_select(&mut self, anchor: Pos, head: Pos) {
        let (l0, l1) = (anchor.line.min(head.line), anchor.line.max(head.line));
        let sels: Vec<Selection> = (l0..=l1)
            .map(|l| {
                let len = self.buf.line_len(l);
                Selection::new(
                    Pos::new(l, anchor.col.min(len)),
                    Pos::new(l, head.col.min(len)),
                )
            })
            .collect();
        let primary_line = head.line;
        self.sels = sels;
        self.primary = primary_line - l0;
        self.box_anchor = Some(anchor);
        self.history.break_group();
    }

    /// Alt+Shift+arrows: grow the column selection from the primary caret.
    pub fn box_extend(&mut self, dl: isize, dc: isize) {
        let anchor = self.box_anchor.unwrap_or(self.primary().anchor);
        let head = self.primary().head;
        let line = (head.line as isize + dl).clamp(0, self.buf.line_count() as isize - 1) as usize;
        let col = (head.col as isize + dc).max(0) as usize;
        self.box_select(anchor, Pos::new(line, col));
    }
}

/// Ctrl+Right: to the start of the next word (or the end of the line first).
pub fn word_right(b: &Buffer, p: Pos) -> Pos {
    let chars: Vec<char> = b.line(p.line).chars().collect();
    if p.col >= chars.len() {
        return b.next(p);
    }
    let class = |c: char| {
        if is_word(c) {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let mut i = p.col;
    let k = class(chars[i]);
    while i < chars.len() && class(chars[i]) == k && k != 1 {
        i += 1;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    Pos::new(p.line, i)
}

/// Ctrl+Left: to the start of the previous word.
pub fn word_left(b: &Buffer, p: Pos) -> Pos {
    if p.col == 0 {
        return b.prev(p);
    }
    let chars: Vec<char> = b.line(p.line).chars().collect();
    let mut i = p.col.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i == 0 {
        return Pos::new(p.line, 0);
    }
    let word = is_word(chars[i - 1]);
    while i > 0 && !chars[i - 1].is_whitespace() && is_word(chars[i - 1]) == word {
        i -= 1;
    }
    Pos::new(p.line, i)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn typed(ed: &mut TextEditor, s: &str) {
        for c in s.chars() {
            ed.replace_selections(EditKind::Typing, |_| c.to_string());
        }
    }

    #[test]
    fn undo_groups_typing_bursts() {
        let t0 = Instant::now();
        let mut ed = TextEditor::new("");
        ed.clock = Some(t0);
        typed(&mut ed, "local");
        ed.clock = Some(t0 + Duration::from_millis(300));
        typed(&mut ed, " x");
        // A pause starts a new step.
        ed.clock = Some(t0 + Duration::from_secs(5));
        typed(&mut ed, " = 1");
        assert_eq!(ed.text(), "local x = 1");
        assert!(ed.undo());
        assert_eq!(ed.text(), "local x");
        // Moving the caret also ends a burst.
        ed.move_horizontal(false, false, false);
        typed(&mut ed, "y");
        assert_eq!(ed.text(), "local yx");
        assert!(ed.undo());
        assert_eq!(ed.text(), "local x");
        assert!(ed.undo());
        assert_eq!(ed.text(), "");
        assert!(!ed.undo());
        assert!(ed.redo() && ed.redo());
        assert_eq!(ed.text(), "local yx");
    }

    #[test]
    fn undo_restores_the_saved_version() {
        let mut ed = TextEditor::new("a");
        let saved = ed.version;
        ed.set_caret(Pos::new(0, 1));
        typed(&mut ed, "b");
        assert_ne!(ed.version, saved);
        ed.undo();
        assert_eq!(ed.version, saved);
    }

    #[test]
    fn multi_caret_typing_shifts_later_carets() {
        let mut ed = TextEditor::new("ab ab\nab");
        ed.set_caret(Pos::new(0, 1));
        ed.add_selection(Selection::caret(Pos::new(0, 4)));
        ed.add_selection(Selection::caret(Pos::new(1, 1)));
        typed(&mut ed, "XY");
        assert_eq!(ed.text(), "aXYb aXYb\naXYb");
        assert_eq!(ed.sels.len(), 3);
        assert_eq!(ed.sels[1].head, Pos::new(0, 8));
    }

    #[test]
    fn word_moves_and_smart_home() {
        let b = Buffer::from_text("    self.speed = 10");
        assert_eq!(word_right(&b, Pos::new(0, 0)), Pos::new(0, 4));
        assert_eq!(word_right(&b, Pos::new(0, 4)), Pos::new(0, 8));
        assert_eq!(word_right(&b, Pos::new(0, 8)), Pos::new(0, 9));
        assert_eq!(word_left(&b, Pos::new(0, 14)), Pos::new(0, 9));
        assert_eq!(word_left(&b, Pos::new(0, 9)), Pos::new(0, 8));
        let mut ed = TextEditor::new("    x = 1");
        ed.set_caret(Pos::new(0, 7));
        ed.home(false);
        assert_eq!(ed.caret(), Pos::new(0, 4));
        ed.home(false);
        assert_eq!(ed.caret(), Pos::new(0, 0));
        ed.home(true);
        assert_eq!(ed.primary(), Selection::new(Pos::new(0, 0), Pos::new(0, 4)));
    }

    #[test]
    fn box_selection_makes_one_selection_per_line() {
        let mut ed = TextEditor::new("abcd\nab\nabcd");
        ed.box_select(Pos::new(0, 1), Pos::new(2, 3));
        assert_eq!(ed.sels.len(), 3);
        assert_eq!(ed.sels[1], Selection::new(Pos::new(1, 1), Pos::new(1, 2)));
        ed.replace_selections(EditKind::Typing, |_| "-".into());
        assert_eq!(ed.text(), "a-d\na-\na-d");
    }
}
