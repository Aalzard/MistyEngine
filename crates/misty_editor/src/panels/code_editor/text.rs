//! Text model: a buffer of lines, positions in characters and selections.
//!
//! Columns count `char`s (not bytes) so they match the laid-out galleys of the view.

/// A place in the text: line and character column (both 0-based).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// One selection; empty when `anchor == head` (a plain caret at `head`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub head: Pos,
    /// Column kept while moving up/down through shorter lines.
    pub goal: Option<usize>,
}

impl Selection {
    pub fn caret(p: Pos) -> Self {
        Self {
            anchor: p,
            head: p,
            goal: None,
        }
    }

    pub fn new(anchor: Pos, head: Pos) -> Self {
        Self {
            anchor,
            head,
            goal: None,
        }
    }

    pub fn start(&self) -> Pos {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> Pos {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Lines touched by the selection; a selection ending at column 0 doesn't take that line.
    pub fn lines(&self) -> std::ops::RangeInclusive<usize> {
        let (s, e) = (self.start(), self.end());
        let last = if e.col == 0 && e.line > s.line {
            e.line - 1
        } else {
            e.line
        };
        s.line..=last
    }
}

/// The document text as lines without their line breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Buffer {
    lines: Vec<String>,
    /// The file used `\r\n`; saving keeps it.
    pub crlf: bool,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            crlf: false,
        }
    }
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        let crlf = text.contains("\r\n");
        let lines = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
            .collect();
        Self { lines, crlf }
    }

    /// Text with the file's line endings.
    pub fn text(&self) -> String {
        self.lines.join(if self.crlf { "\r\n" } else { "\n" })
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Length of line `i` in characters.
    pub fn line_len(&self, i: usize) -> usize {
        self.line(i).chars().count()
    }

    pub fn end(&self) -> Pos {
        let last = self.lines.len() - 1;
        Pos::new(last, self.line_len(last))
    }

    /// The nearest valid position.
    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos::new(line, p.col.min(self.line_len(line)))
    }

    pub fn char_at(&self, p: Pos) -> Option<char> {
        self.line(p.line).chars().nth(p.col)
    }

    /// Text between two positions (any order), lines joined with `\n`.
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        let (s, e) = (self.clamp(a.min(b)), self.clamp(a.max(b)));
        if s.line == e.line {
            let l = self.line(s.line);
            return l[byte_of(l, s.col)..byte_of(l, e.col)].to_owned();
        }
        let first = self.line(s.line);
        let mut out = first[byte_of(first, s.col)..].to_owned();
        for l in &self.lines[s.line + 1..e.line] {
            out.push('\n');
            out.push_str(l);
        }
        out.push('\n');
        let last = self.line(e.line);
        out.push_str(&last[..byte_of(last, e.col)]);
        out
    }

    /// Replace the text between `a` and `b` with `text` (`\n` / `\r\n` start new lines).
    /// Returns the position just after the inserted text.
    pub fn replace(&mut self, a: Pos, b: Pos, text: &str) -> Pos {
        let (s, e) = (self.clamp(a.min(b)), self.clamp(a.max(b)));
        let head = {
            let l = self.line(s.line);
            l[..byte_of(l, s.col)].to_owned()
        };
        let tail = {
            let l = self.line(e.line);
            l[byte_of(l, e.col)..].to_owned()
        };
        let text = text.replace("\r\n", "\n");
        let mut parts: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let last = parts.len() - 1;
        let end = Pos::new(
            s.line + last,
            if last == 0 {
                s.col + parts[0].chars().count()
            } else {
                parts[last].chars().count()
            },
        );
        parts[0] = head + &parts[0];
        parts[last].push_str(&tail);
        self.lines.splice(s.line..=e.line, parts);
        end
    }

    /// Position one character before `p` (crossing line starts), or `p` at the start.
    pub fn prev(&self, p: Pos) -> Pos {
        if p.col > 0 {
            Pos::new(p.line, p.col - 1)
        } else if p.line > 0 {
            Pos::new(p.line - 1, self.line_len(p.line - 1))
        } else {
            p
        }
    }

    /// Position one character after `p` (crossing line ends), or `p` at the end.
    pub fn next(&self, p: Pos) -> Pos {
        if p.col < self.line_len(p.line) {
            Pos::new(p.line, p.col + 1)
        } else if p.line + 1 < self.lines.len() {
            Pos::new(p.line + 1, 0)
        } else {
            p
        }
    }

    /// The identifier-like word around `p` (start, end), if `p` touches one.
    pub fn word_at(&self, p: Pos) -> Option<(Pos, Pos)> {
        let chars: Vec<char> = self.line(p.line).chars().collect();
        let mut s = p.col.min(chars.len());
        let mut e = s;
        while s > 0 && is_word(chars[s - 1]) {
            s -= 1;
        }
        while e < chars.len() && is_word(chars[e]) {
            e += 1;
        }
        (s < e).then(|| (Pos::new(p.line, s), Pos::new(p.line, e)))
    }

    /// Leading whitespace of line `i`.
    pub fn indent_of(&self, i: usize) -> &str {
        let l = self.line(i);
        &l[..l.len() - l.trim_start().len()]
    }
}

/// Byte index of character column `col` in `s` (clamped to the end).
pub fn byte_of(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(b, _)| b).unwrap_or(s.len())
}

/// Character column of byte index `b` in `s`.
pub fn col_of(s: &str, b: usize) -> usize {
    s[..b.min(s.len())].chars().count()
}

pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `p` ends up after the text between `start` and `old_end` was replaced by text ending at
/// `new_end`. Positions inside the replaced text move to its end.
pub fn adjust(p: Pos, start: Pos, old_end: Pos, new_end: Pos) -> Pos {
    if p < start || (p == start && start != old_end) {
        p
    } else if p >= old_end {
        if p.line == old_end.line {
            Pos::new(new_end.line, new_end.col + (p.col - old_end.col))
        } else {
            Pos::new(p.line + new_end.line - old_end.line, p.col)
        }
    } else {
        new_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_and_slice_across_lines() {
        let mut b = Buffer::from_text("local a = 1\r\nlocal b = 2\r\n");
        assert!(b.crlf);
        assert_eq!(b.line_count(), 3);
        let end = b.replace(Pos::new(0, 6), Pos::new(1, 7), "x\ny");
        assert_eq!(end, Pos::new(1, 1));
        assert_eq!(b.lines(), &["local x", "y = 2", ""]);
        assert_eq!(b.slice(Pos::new(0, 6), Pos::new(1, 1)), "x\ny");
        assert_eq!(b.text(), "local x\r\ny = 2\r\n");
    }

    #[test]
    fn unicode_columns_are_characters() {
        let mut b = Buffer::from_text("-- héllo");
        assert_eq!(b.line_len(0), 8);
        b.replace(Pos::new(0, 4), Pos::new(0, 5), "e");
        assert_eq!(b.line(0), "-- hello");
        assert_eq!(
            b.word_at(Pos::new(0, 5)),
            Some((Pos::new(0, 3), Pos::new(0, 8)))
        );
    }

    #[test]
    fn positions_follow_edits() {
        let start = Pos::new(1, 2);
        // Insert "ab\nc" at (1,2): later text on that line moves to line 2.
        let new_end = Pos::new(2, 1);
        assert_eq!(
            adjust(Pos::new(1, 5), start, start, new_end),
            Pos::new(2, 4)
        );
        assert_eq!(
            adjust(Pos::new(3, 0), start, start, new_end),
            Pos::new(4, 0)
        );
        assert_eq!(
            adjust(Pos::new(0, 9), start, start, new_end),
            Pos::new(0, 9)
        );
        // Delete (1,2)..(2,0): positions inside collapse to the start.
        let (s, e) = (Pos::new(1, 2), Pos::new(2, 0));
        assert_eq!(adjust(Pos::new(1, 4), s, e, s), s);
        assert_eq!(adjust(Pos::new(2, 3), s, e, s), Pos::new(1, 5));
    }
}
