//! Editing commands on [`TextEditor`]: typing with auto-close and auto-indent, line commands
//! (duplicate, delete, move, comment, indent), clipboard, formatting and navigation helpers.

use super::editor::{TextEditor, word_left, word_right};
use super::history::EditKind;
use super::lexer::{self, Block, LineState, Token, TokenKind};
use super::text::{Pos, Selection};

const PAIRS: [(char, char); 5] = [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];

/// Keywords that end a block; typing the last letter re-indents the line.
const DEDENT_WORDS: [&str; 5] = ["end", "else", "elseif", "until", "}"];

impl TextEditor {
    fn indent_unit(&self) -> String {
        " ".repeat(self.tab_size)
    }

    /// Merged line ranges (first, last) touched by the selections.
    pub fn line_blocks(&self) -> Vec<(usize, usize)> {
        let mut blocks: Vec<(usize, usize)> = Vec::new();
        for s in &self.sels {
            let r = s.lines();
            let (a, b) = (*r.start(), *r.end());
            match blocks.last_mut() {
                Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
                _ => blocks.push((a, b)),
            }
        }
        blocks
    }

    /// Type one character at every caret.
    pub fn type_char(&mut self, c: char, auto_close: bool) {
        if c == '\n' {
            return self.newline();
        }
        let closer = PAIRS.iter().find(|p| p.0 == c).map(|p| p.1);
        let is_closer = PAIRS.iter().any(|p| p.1 == c);
        // Typing a closer that's already next to every caret just steps over it.
        if auto_close
            && is_closer
            && self
                .sels
                .iter()
                .all(|s| s.is_empty() && self.buf.char_at(s.head) == Some(c))
        {
            self.move_carets(false, |b, s| b.next(s.head));
            return;
        }
        // (edit, caret offset back from the end, wrap selection)
        let mut edits = Vec::new();
        let mut plans = Vec::new();
        for s in &self.sels {
            let next = self.buf.char_at(s.head);
            let prev =
                (s.head.col > 0).then(|| self.buf.char_at(Pos::new(s.head.line, s.head.col - 1)));
            let prev = prev.flatten();
            let free_after = next.is_none_or(|n| n.is_whitespace() || ")]},;".contains(n));
            let quote = c == '"' || c == '\'';
            let (text, back, wrap) = match closer {
                Some(close) if auto_close && !s.is_empty() => {
                    let inner = self.buf.slice(s.start(), s.end());
                    (format!("{c}{inner}{close}"), 1, true)
                }
                Some(close)
                    if auto_close
                        && free_after
                        && !(quote && prev.is_some_and(|p| super::text::is_word(p) || p == c)) =>
                {
                    (format!("{c}{close}"), 1, false)
                }
                _ => (c.to_string(), 0, false),
            };
            edits.push((s.start(), s.end(), text));
            plans.push((back, wrap));
        }
        let ranges = self.apply(EditKind::Typing, edits);
        let sels = ranges
            .iter()
            .zip(plans)
            .map(|(&(start, end), (back, wrap))| {
                let inner_end = Pos::new(end.line, end.col - back);
                if wrap {
                    Selection::new(Pos::new(start.line, start.col + 1), inner_end)
                } else {
                    Selection::caret(inner_end)
                }
            })
            .collect();
        self.set_sels_keep_group(sels);
        if c.is_alphabetic() || c == '}' {
            self.auto_dedent();
        }
    }

    /// Type a string (IME / paste-like text input) without auto-closing.
    pub fn type_text(&mut self, text: &str) {
        if text.chars().count() == 1 {
            let c = text.chars().next().unwrap_or(' ');
            return self.type_char(c, true);
        }
        self.replace_selections(EditKind::Typing, |_| text.to_owned());
    }

    /// Replace selections without ending the typing burst.
    fn set_sels_keep_group(&mut self, sels: Vec<Selection>) {
        let primary = self.primary.min(sels.len().saturating_sub(1));
        self.sels = sels;
        self.primary = primary;
        self.normalize();
    }

    /// After typing `end` / `else` / `until` / `}` alone on a line, match the opener's indent.
    fn auto_dedent(&mut self) {
        let lines: Vec<usize> = self.sels.iter().map(|s| s.head.line).collect();
        let mut edits = Vec::new();
        let structure = lexer::structure(self.buf.lines(), &lexer::tokenize(self.buf.lines()));
        for line in lines {
            let text = self.buf.line(line);
            let word = text.trim();
            if !DEDENT_WORDS.contains(&word) {
                continue;
            }
            let want = structure.indent[line] * self.tab_size;
            let have = self.buf.indent_of(line).chars().count();
            if have > want {
                edits.push((Pos::new(line, 0), Pos::new(line, have), " ".repeat(want)));
            }
        }
        if !edits.is_empty() {
            self.apply(EditKind::Typing, edits);
        }
    }

    /// Enter: break the line, keep the indent and indent one more level after a block opener.
    /// Between brackets (`{|}`) the closer moves to its own line.
    pub fn newline(&mut self) {
        let unit = self.indent_unit();
        let mut edits = Vec::new();
        let mut middle = Vec::new();
        for s in &self.sels {
            let line = self.buf.line(s.start().line);
            let before: String = line.chars().take(s.start().col).collect();
            let indent = self.buf.indent_of(s.start().line).to_owned();
            let indent = if before.trim().is_empty() {
                before.clone()
            } else {
                indent
            };
            let opens = opens_block(&before);
            let next = self.buf.char_at(s.end());
            let prev = before.chars().last();
            let between = matches!(
                (prev, next),
                (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']'))
            );
            // A whitespace-only line loses its spaces; they move to the new line.
            let start = if before.trim().is_empty() {
                Pos::new(s.start().line, 0)
            } else {
                s.start()
            };
            let inner = if opens {
                format!("{indent}{unit}")
            } else {
                indent.clone()
            };
            if between {
                edits.push((start, s.end(), format!("\n{inner}\n{indent}")));
                middle.push(Some(inner.chars().count()));
            } else {
                edits.push((start, s.end(), format!("\n{inner}")));
                middle.push(None);
            }
        }
        let ranges = self.apply(EditKind::Typing, edits);
        let sels = ranges
            .iter()
            .zip(middle)
            .map(|(&(_, end), mid)| match mid {
                Some(col) => Selection::caret(Pos::new(end.line - 1, col)),
                None => Selection::caret(end),
            })
            .collect();
        self.set_sels_keep_group(sels);
        self.break_undo_group();
    }

    /// Backspace (Ctrl = whole word). In leading spaces it removes back to the previous indent
    /// stop; between an empty pair `()` it removes both.
    pub fn backspace(&mut self, word: bool) {
        let mut edits = Vec::new();
        for s in &self.sels {
            if !s.is_empty() {
                edits.push((s.start(), s.end(), String::new()));
                continue;
            }
            let p = s.head;
            if word {
                edits.push((word_left(&self.buf, p), p, String::new()));
                continue;
            }
            let line = self.buf.line(p.line);
            let before: String = line.chars().take(p.col).collect();
            if p.col > 0 && before.chars().all(|c| c == ' ') {
                let stop = (p.col - 1) / self.tab_size * self.tab_size;
                edits.push((Pos::new(p.line, stop), p, String::new()));
                continue;
            }
            let prev = before.chars().last();
            let next = self.buf.char_at(p);
            let pair = PAIRS
                .iter()
                .any(|&(o, c)| prev == Some(o) && next == Some(c));
            let end = if pair { Pos::new(p.line, p.col + 1) } else { p };
            edits.push((self.buf.prev(p), end, String::new()));
        }
        self.apply(EditKind::Erase, edits);
    }

    /// Delete (Ctrl = to the next word start).
    pub fn delete(&mut self, word: bool) {
        let edits = self
            .sels
            .iter()
            .map(|s| {
                let (a, b) = if !s.is_empty() {
                    (s.start(), s.end())
                } else if word {
                    (s.head, word_right(&self.buf, s.head))
                } else {
                    (s.head, self.buf.next(s.head))
                };
                (a, b, String::new())
            })
            .collect();
        self.apply(EditKind::Erase, edits);
    }

    /// Tab: indent selected lines, or insert spaces up to the next indent stop.
    pub fn tab(&mut self) {
        if self.sels.iter().any(|s| s.start().line != s.end().line) {
            return self.indent_lines(false);
        }
        let tab = self.tab_size;
        let edits = self
            .sels
            .iter()
            .map(|s| {
                let col = s.start().col;
                (s.start(), s.end(), " ".repeat(tab - col % tab))
            })
            .collect();
        let ranges = self.apply(EditKind::Typing, edits);
        let sels = ranges
            .into_iter()
            .map(|(_, e)| Selection::caret(e))
            .collect();
        self.set_sels_keep_group(sels);
    }

    /// Indent (or outdent) every selected line by one level; blank lines are left alone.
    pub fn indent_lines(&mut self, outdent: bool) {
        let unit = self.indent_unit();
        let mut edits = Vec::new();
        for (a, b) in self.line_blocks() {
            for l in a..=b {
                let line = self.buf.line(l);
                if outdent {
                    let n = if line.starts_with('\t') {
                        1
                    } else {
                        line.chars()
                            .take(self.tab_size)
                            .take_while(|&c| c == ' ')
                            .count()
                    };
                    if n > 0 {
                        edits.push((Pos::new(l, 0), Pos::new(l, n), String::new()));
                    }
                } else if !line.trim().is_empty() {
                    edits.push((Pos::new(l, 0), Pos::new(l, 0), unit.clone()));
                }
            }
        }
        // Whole-line selections keep starting at column 0.
        let at_zero: Vec<(bool, bool)> = self
            .sels
            .iter()
            .map(|s| {
                (
                    s.anchor.col == 0 && !s.is_empty(),
                    s.head.col == 0 && !s.is_empty(),
                )
            })
            .collect();
        self.apply(EditKind::Other, edits);
        if at_zero.len() == self.sels.len() {
            for (s, (a, h)) in self.sels.iter_mut().zip(at_zero) {
                if a {
                    s.anchor.col = 0;
                }
                if h {
                    s.head.col = 0;
                }
            }
        }
    }

    /// Ctrl+D: duplicate the selected text, or the caret lines below themselves.
    pub fn duplicate(&mut self) {
        if self.sels.iter().any(|s| !s.is_empty()) {
            let edits: Vec<_> = self
                .sels
                .iter()
                .map(|s| (s.end(), s.end(), self.buf.slice(s.start(), s.end())))
                .collect();
            let ranges = self.apply(EditKind::Other, edits);
            let sels = ranges
                .into_iter()
                .map(|(s, e)| Selection::new(s, e))
                .collect();
            self.set_sels_keep_group(sels);
            return;
        }
        let blocks = self.line_blocks();
        let edits: Vec<_> = blocks
            .iter()
            .map(|&(a, b)| {
                let end = Pos::new(b, self.buf.line_len(b));
                let copy = self.buf.lines()[a..=b].join("\n");
                (end, end, format!("\n{copy}"))
            })
            .collect();
        let old = self.sels.clone();
        self.apply(EditKind::Other, edits);
        // Carets move onto the copies; every earlier block pushes later lines down too.
        let sels = old
            .iter()
            .map(|s| {
                let shift: usize = blocks
                    .iter()
                    .filter(|(a, _)| *a <= s.head.line)
                    .map(|(a, b)| b - a + 1)
                    .sum();
                Selection::caret(Pos::new(s.head.line + shift, s.head.col))
            })
            .collect();
        self.set_sels_keep_group(sels);
    }

    /// Ctrl+Shift+L: delete the selected lines.
    pub fn delete_lines(&mut self) {
        let blocks = self.line_blocks();
        let last = self.buf.line_count() - 1;
        let cols: Vec<usize> = self.sels.iter().map(|s| s.head.col).collect();
        let edits = blocks
            .iter()
            .map(|&(a, b)| {
                if b < last {
                    (Pos::new(a, 0), Pos::new(b + 1, 0), String::new())
                } else if a > 0 {
                    let prev = Pos::new(a - 1, self.buf.line_len(a - 1));
                    (prev, Pos::new(b, self.buf.line_len(b)), String::new())
                } else {
                    (
                        Pos::new(a, 0),
                        Pos::new(b, self.buf.line_len(b)),
                        String::new(),
                    )
                }
            })
            .collect();
        let ranges = self.apply(EditKind::Other, edits);
        let sels = ranges
            .iter()
            .zip(cols)
            .map(|(&(s, _), col)| {
                let line = if s.col > 0 { s.line + 1 } else { s.line };
                let line = line.min(self.buf.line_count() - 1);
                Selection::caret(self.buf.clamp(Pos::new(line, col)))
            })
            .collect();
        self.set_sels_keep_group(sels);
    }

    /// Text of the selected lines, each with its line break (line-mode clipboard).
    pub fn lines_text(&self) -> String {
        self.line_blocks()
            .iter()
            .map(|&(a, b)| self.buf.lines()[a..=b].join("\n") + "\n")
            .collect()
    }

    /// Alt+Up / Alt+Down: move the selected lines past their neighbour.
    pub fn move_lines(&mut self, up: bool) -> bool {
        let blocks = self.line_blocks();
        let last = self.buf.line_count() - 1;
        if blocks
            .iter()
            .any(|&(a, b)| (up && a == 0) || (!up && b == last))
        {
            return false;
        }
        let edits = blocks
            .iter()
            .map(|&(a, b)| {
                let lines = self.buf.lines();
                if up {
                    let text = format!("{}\n{}", lines[a..=b].join("\n"), lines[a - 1]);
                    (Pos::new(a - 1, 0), Pos::new(b, self.buf.line_len(b)), text)
                } else {
                    let text = format!("{}\n{}", lines[b + 1], lines[a..=b].join("\n"));
                    (
                        Pos::new(a, 0),
                        Pos::new(b + 1, self.buf.line_len(b + 1)),
                        text,
                    )
                }
            })
            .collect();
        let old = self.sels.clone();
        self.apply(EditKind::Other, edits);
        let shift = |p: Pos| Pos::new(if up { p.line - 1 } else { p.line + 1 }, p.col);
        let sels = old
            .iter()
            .map(|s| Selection::new(shift(s.anchor), shift(s.head)))
            .collect();
        self.set_sels_keep_group(sels);
        true
    }

    /// Ctrl+/: comment the lines unless all of them already are, then uncomment.
    pub fn toggle_comment(&mut self) {
        let all_commented = self.line_blocks().iter().all(|&(a, b)| {
            (a..=b).all(|l| {
                let t = self.buf.line(l).trim_start();
                t.is_empty() || t.starts_with("--")
            })
        });
        self.comment(!all_commented);
    }

    /// Ctrl+K,C / Ctrl+K,U: add `-- ` at the block's smallest indent, or remove `--`.
    pub fn comment(&mut self, add: bool) {
        let mut edits = Vec::new();
        for (a, b) in self.line_blocks() {
            let lines: Vec<usize> = (a..=b)
                .filter(|&l| !self.buf.line(l).trim().is_empty())
                .collect();
            let min_indent = lines
                .iter()
                .map(|&l| self.buf.indent_of(l).chars().count())
                .min()
                .unwrap_or(0);
            for l in lines {
                let indent = self.buf.indent_of(l).chars().count();
                let rest = self.buf.line(l).trim_start();
                if add {
                    edits.push((
                        Pos::new(l, min_indent),
                        Pos::new(l, min_indent),
                        "-- ".into(),
                    ));
                } else if let Some(after) = rest.strip_prefix("--") {
                    let n = if after.starts_with(' ') { 3 } else { 2 };
                    edits.push((Pos::new(l, indent), Pos::new(l, indent + n), String::new()));
                }
            }
        }
        self.apply(EditKind::Other, edits);
    }

    /// Ctrl+Enter / Ctrl+Shift+Enter: open a new line below/above the caret's line.
    pub fn insert_line(&mut self, above: bool) {
        let edits: Vec<_> = self
            .sels
            .iter()
            .map(|s| {
                let l = s.head.line;
                let indent = self.buf.indent_of(l).to_owned();
                if above {
                    (Pos::new(l, 0), Pos::new(l, 0), format!("{indent}\n"))
                } else {
                    let end = Pos::new(l, self.buf.line_len(l));
                    let extra = if opens_block(self.buf.line(l)) {
                        self.indent_unit()
                    } else {
                        String::new()
                    };
                    (end, end, format!("\n{indent}{extra}"))
                }
            })
            .collect();
        let ranges = self.apply(EditKind::Other, edits);
        let sels = ranges
            .iter()
            .map(|&(_, e)| {
                if above {
                    Selection::caret(self.buf.clamp(Pos::new(e.line - 1, usize::MAX)))
                } else {
                    Selection::caret(e)
                }
            })
            .collect();
        self.set_sels_keep_group(sels);
    }

    /// Text for Copy, and whether it is whole lines (nothing was selected).
    pub fn copy_text(&self) -> (String, bool) {
        if self.sels.iter().all(Selection::is_empty) {
            return (self.lines_text(), true);
        }
        let parts: Vec<String> = self
            .sels
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| self.buf.slice(s.start(), s.end()))
            .collect();
        (parts.join("\n"), false)
    }

    /// Cut: the selection, or whole lines when nothing is selected.
    pub fn cut(&mut self) -> (String, bool) {
        let (text, lines) = self.copy_text();
        if lines {
            self.delete_lines();
        } else {
            self.replace_selections(EditKind::Other, |_| String::new());
        }
        (text, lines)
    }

    /// Paste: whole lines go above the caret's line; with one line per caret each caret gets its
    /// own line; otherwise the text replaces every selection.
    pub fn paste(&mut self, text: &str, line_mode: bool) {
        let text = text.replace("\r\n", "\n");
        if line_mode && self.sels.iter().all(Selection::is_empty) {
            let mut seen = Vec::new();
            let edits: Vec<_> = self
                .sels
                .iter()
                .filter(|s| {
                    let new = !seen.contains(&s.head.line);
                    seen.push(s.head.line);
                    new
                })
                .map(|s| {
                    let p = Pos::new(s.head.line, 0);
                    (p, p, text.clone())
                })
                .collect();
            let lines: Vec<usize> = edits
                .iter()
                .map(|e: &(Pos, Pos, String)| e.0.line)
                .collect();
            let old = self.sels.clone();
            self.apply(EditKind::Other, edits);
            // Each paste at or above a caret's line pushes it down.
            let n = text.matches('\n').count();
            let sels = old
                .iter()
                .map(|s| {
                    let above = lines.iter().filter(|&&l| l <= s.head.line).count();
                    Selection::caret(Pos::new(s.head.line + n * above, s.head.col))
                })
                .collect();
            self.set_sels_keep_group(sels);
            return;
        }
        let parts: Vec<&str> = text.split('\n').collect();
        if self.sels.len() > 1 && parts.len() == self.sels.len() {
            let edits = self
                .sels
                .iter()
                .zip(&parts)
                .map(|(s, p)| (s.start(), s.end(), p.to_string()))
                .collect();
            let ranges = self.apply(EditKind::Other, edits);
            let sels = ranges
                .into_iter()
                .map(|(_, e)| Selection::caret(e))
                .collect();
            return self.set_sels_keep_group(sels);
        }
        self.replace_selections(EditKind::Other, |_| text.clone());
    }

    /// Ctrl+K,D: re-indent every line by block depth and strip trailing spaces.
    pub fn format_document(&mut self) {
        let lines = self.buf.lines().to_vec();
        let tokens = lexer::tokenize(&lines);
        let structure = lexer::structure(&lines, &tokens);
        let mut state = LineState::Normal;
        let mut edits = Vec::new();
        for (l, text) in lines.iter().enumerate() {
            let starts_in_long = state != LineState::Normal;
            state = lexer::tokenize_line(text, state).1;
            if starts_in_long {
                continue;
            }
            let len = text.chars().count();
            let trimmed_end = text.trim_end().chars().count();
            if trimmed_end < len && state == LineState::Normal {
                edits.push((Pos::new(l, trimmed_end), Pos::new(l, len), String::new()));
            }
            if text.trim().is_empty() {
                continue;
            }
            let have = text.chars().count() - text.trim_start().chars().count();
            let want = " ".repeat(structure.indent[l] * self.tab_size);
            if text[..text.len() - text.trim_start().len()] != want {
                edits.push((Pos::new(l, 0), Pos::new(l, have), want));
            }
        }
        self.apply(EditKind::Other, edits);
    }

    /// Ctrl+G: caret to the start of 1-based `line`.
    pub fn go_to_line(&mut self, line: usize) {
        let l = line.max(1) - 1;
        let col = self
            .buf
            .indent_of(l.min(self.buf.line_count() - 1))
            .chars()
            .count();
        self.set_caret(Pos::new(l, col));
    }

    /// The bracket next to the primary caret and its partner.
    pub fn matching_bracket(&self, tokens: &[Vec<Token>]) -> Option<(Pos, Pos)> {
        let p = self.caret();
        let at = |pos: Pos| {
            tokens
                .get(pos.line)?
                .iter()
                .find(|t| t.kind == TokenKind::Bracket && t.start == pos.col)?;
            self.buf.char_at(pos)
        };
        let candidates = [Some(p), (p.col > 0).then(|| Pos::new(p.line, p.col - 1))];
        for pos in candidates.into_iter().flatten() {
            let Some(c) = at(pos) else { continue };
            let (open, close, forward) = match c {
                '(' => ('(', ')', true),
                '[' => ('[', ']', true),
                '{' => ('{', '}', true),
                ')' => ('(', ')', false),
                ']' => ('[', ']', false),
                '}' => ('{', '}', false),
                _ => continue,
            };
            let mut depth = 0i32;
            let mut brackets = tokens.iter().enumerate().flat_map(|(l, ts)| {
                ts.iter()
                    .filter(|t| t.kind == TokenKind::Bracket)
                    .map(move |t| Pos::new(l, t.start))
            });
            let list: Vec<Pos> = if forward {
                brackets.by_ref().filter(|b| *b >= pos).collect()
            } else {
                let mut v: Vec<Pos> = brackets.filter(|b| *b <= pos).collect();
                v.reverse();
                v
            };
            for b in list {
                match self.buf.char_at(b) {
                    Some(ch) if ch == open => depth += if forward { 1 } else { -1 },
                    Some(ch) if ch == close => depth += if forward { -1 } else { 1 },
                    _ => {}
                }
                if depth == 0 {
                    return Some((pos, b));
                }
            }
        }
        None
    }

    /// F12: where `name` is declared in this file (`function X:name`, `local name`, …).
    pub fn definition_of(&self, name: &str) -> Option<Pos> {
        let n = regex::escape(name);
        let patterns = [
            format!(r"function\s+(?:[\w.]+[.:])?({n})\s*\("),
            format!(r"local\s+function\s+({n})\b"),
            format!(r"local\s+({n})\b"),
            format!(r"(?:^|[^.\w])({n})\s*=\s*function\b"),
            format!(r"(?:^|\s)({n})\s*=[^=]"),
        ];
        for pat in patterns {
            let re = regex::Regex::new(&pat).ok()?;
            for (l, line) in self.buf.lines().iter().enumerate() {
                if let Some(m) = re.captures(line).and_then(|c| c.get(1)) {
                    return Some(Pos::new(l, super::text::col_of(line, m.start())));
                }
            }
        }
        None
    }

    /// Shift+Alt+. : select the word, then add the next occurrence of the selection.
    pub fn select_next_occurrence(&mut self) {
        let p = self.primary();
        if p.is_empty() {
            let sel = self.select_word_at(p.head);
            self.sels[self.primary] = sel;
            return;
        }
        let needle = self.buf.slice(p.start(), p.end());
        let from = self.sels.iter().map(|s| s.end()).max().unwrap_or_default();
        let found = self
            .find_from(&needle, from)
            .or_else(|| self.find_from(&needle, Pos::default()));
        if let Some((s, e)) = found
            && !self.sels.iter().any(|x| x.start() == s)
        {
            self.add_selection(Selection::new(s, e));
        }
    }

    /// Shift+Alt+; : select every occurrence of the word or selection.
    pub fn select_all_occurrences(&mut self) {
        let p = self.primary();
        let p = if p.is_empty() {
            self.select_word_at(p.head)
        } else {
            p
        };
        if p.is_empty() {
            return;
        }
        let needle = self.buf.slice(p.start(), p.end());
        let mut sels = Vec::new();
        let mut from = Pos::default();
        while let Some((s, e)) = self.find_from(&needle, from) {
            sels.push(Selection::new(s, e));
            from = e;
        }
        self.set_selections(sels);
    }

    /// Next exact match of a single-line `needle` at or after `from`.
    fn find_from(&self, needle: &str, from: Pos) -> Option<(Pos, Pos)> {
        if needle.is_empty() || needle.contains('\n') {
            return None;
        }
        let len = needle.chars().count();
        for l in from.line..self.buf.line_count() {
            let line = self.buf.line(l);
            let start_b = if l == from.line {
                super::text::byte_of(line, from.col)
            } else {
                0
            };
            if let Some(b) = line[start_b..].find(needle) {
                let col = super::text::col_of(line, start_b + b);
                return Some((Pos::new(l, col), Pos::new(l, col + len)));
            }
        }
        None
    }
}

/// Whether text ends inside an unclosed block (`then`, `do`, `function(…)`, `{`, `else`, …).
pub fn opens_block(text: &str) -> bool {
    let (tokens, _) = lexer::tokenize_line(text, LineState::Normal);
    let words = lexer::block_words(text, &tokens);
    let mut net = 0i32;
    let mut awaiting = false;
    for (_, w) in &words {
        match lexer::block_of(w, awaiting) {
            Some(Block::Open) => {
                net += 1;
                awaiting = matches!(*w, "while" | "for");
            }
            Some(Block::Close) => net -= 1,
            Some(Block::Middle) => {}
            None => awaiting = false,
        }
    }
    let starts_middle = matches!(words.first(), Some((_, "else" | "elseif")));
    net > 0 || (starts_middle && net == 0 && !text.trim_end().ends_with("end"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(text: &str, caret: (usize, usize)) -> TextEditor {
        let mut e = TextEditor::new(text);
        e.set_caret(Pos::new(caret.0, caret.1));
        e
    }

    fn sel(e: &mut TextEditor, a: (usize, usize), b: (usize, usize)) {
        e.set_selection(Selection::new(Pos::new(a.0, a.1), Pos::new(b.0, b.1)));
    }

    #[test]
    fn duplicate_line_and_selection() {
        let mut e = ed("a\nb\nc", (1, 1));
        e.duplicate();
        assert_eq!(e.text(), "a\nb\nb\nc");
        assert_eq!(e.caret(), Pos::new(2, 1));
        sel(&mut e, (0, 0), (0, 1));
        e.duplicate();
        assert_eq!(e.text(), "aa\nb\nb\nc");
        assert_eq!(e.primary(), Selection::new(Pos::new(0, 1), Pos::new(0, 2)));
        // A selection over two lines duplicates both, below.
        let mut e = ed("x\ny\nz", (0, 0));
        e.set_caret(Pos::new(0, 0));
        e.add_selection(Selection::caret(Pos::new(1, 0)));
        e.duplicate();
        assert_eq!(e.text(), "x\ny\nx\ny\nz");
    }

    #[test]
    fn delete_and_move_lines() {
        let mut e = ed("a\nb\nc", (1, 0));
        e.delete_lines();
        assert_eq!(e.text(), "a\nc");
        assert_eq!(e.caret(), Pos::new(1, 0));
        e.delete_lines();
        assert_eq!(e.text(), "a");
        let mut e = ed("1\n2\n3\n4", (2, 1));
        assert!(e.move_lines(true));
        assert_eq!(e.text(), "1\n3\n2\n4");
        assert_eq!(e.caret(), Pos::new(1, 1));
        sel(&mut e, (1, 0), (2, 1));
        assert!(e.move_lines(false));
        assert_eq!(e.text(), "1\n4\n3\n2");
        assert_eq!(e.primary(), Selection::new(Pos::new(2, 0), Pos::new(3, 1)));
        assert!(!e.move_lines(false), "already at the bottom");
        let (text, lines) = ed("x\ny", (1, 0)).copy_text();
        assert_eq!((text.as_str(), lines), ("y\n", true));
    }

    #[test]
    fn comment_uncomment_and_toggle() {
        let mut e = ed("    a()\n\n  b()", (0, 0));
        sel(&mut e, (0, 0), (2, 3));
        e.comment(true);
        assert_eq!(e.text(), "  --   a()\n\n  -- b()");
        e.toggle_comment();
        assert_eq!(e.text(), "    a()\n\n  b()");
        e.toggle_comment();
        e.comment(false);
        assert_eq!(e.text(), "    a()\n\n  b()");
    }

    #[test]
    fn tab_and_shift_tab_indent_selections() {
        let mut e = ed("a\n  b\n\nc", (0, 0));
        sel(&mut e, (0, 0), (3, 1));
        e.tab();
        assert_eq!(e.text(), "    a\n      b\n\n    c");
        assert_eq!(e.primary().start(), Pos::new(0, 0));
        e.indent_lines(true);
        e.indent_lines(true);
        assert_eq!(e.text(), "a\nb\n\nc");
        let mut e = ed("ab", (0, 1));
        e.tab();
        assert_eq!(e.text(), "a   b");
    }

    #[test]
    fn auto_close_overtype_and_wrap() {
        let mut e = ed("", (0, 0));
        e.type_char('(', true);
        e.type_char('"', true);
        assert_eq!(e.text(), "(\"\")");
        e.type_char('w', true);
        e.type_char('"', true);
        e.type_char(')', true);
        assert_eq!(e.text(), "(\"w\")");
        assert_eq!(e.caret(), Pos::new(0, 5));
        // Backspace inside an empty pair removes both.
        let mut e = ed("f()", (0, 2));
        e.backspace(false);
        assert_eq!(e.text(), "f");
        // Typing a quote around a selection wraps it.
        let mut e = ed("say hi", (0, 0));
        sel(&mut e, (0, 4), (0, 6));
        e.type_char('"', true);
        assert_eq!(e.text(), "say \"hi\"");
        // No pair after a word character (an apostrophe).
        let mut e = ed("it", (0, 2));
        e.type_char('\'', true);
        assert_eq!(e.text(), "it'");
    }

    #[test]
    fn enter_indents_after_openers_and_end_dedents() {
        let mut e = ed("function A:Update()", (0, 19));
        e.newline();
        assert_eq!(e.text(), "function A:Update()\n    ");
        for c in "if x then".chars() {
            e.type_char(c, true);
        }
        e.newline();
        assert_eq!(e.caret(), Pos::new(2, 8));
        for c in "y()\nend".chars() {
            e.type_char(c, true);
        }
        assert_eq!(e.buf.line(3), "    end");
        // Enter between braces puts the closer on its own line.
        let mut e = ed("t = {}", (0, 5));
        e.newline();
        assert_eq!(e.text(), "t = {\n    \n}");
        assert_eq!(e.caret(), Pos::new(1, 4));
        assert!(opens_block("for i = 1, 3 do"));
        assert!(!opens_block("if a then b() end"));
        assert!(opens_block("else"));
    }

    #[test]
    fn insert_line_above_and_below() {
        let mut e = ed("  a\nb", (0, 1));
        e.insert_line(false);
        assert_eq!(e.text(), "  a\n  \nb");
        assert_eq!(e.caret(), Pos::new(1, 2));
        e.set_caret(Pos::new(2, 1));
        e.insert_line(true);
        assert_eq!(e.text(), "  a\n  \n\nb");
        assert_eq!(e.caret(), Pos::new(2, 0));
    }

    #[test]
    fn paste_line_mode_and_per_caret() {
        let mut e = ed("a\nb", (1, 1));
        e.paste("x\n", true);
        assert_eq!(e.text(), "a\nx\nb");
        assert_eq!(e.caret(), Pos::new(2, 1));
        let mut e = ed("1\n2", (0, 1));
        e.add_selection(Selection::caret(Pos::new(1, 1)));
        e.paste("a\nb", false);
        assert_eq!(e.text(), "1a\n2b");
        let mut e = ed("abc", (0, 0));
        e.select_all();
        let (t, lines) = e.cut();
        assert_eq!((t.as_str(), lines, e.text()), ("abc", false, String::new()));
    }

    #[test]
    fn format_reindents_blocks() {
        let src = "local A = {}\nfunction A:Update()   \nif x then\ny()\n  else\n      z()\nend\nend\nlocal s = [[\n  keep\n]]";
        let mut e = ed(src, (0, 0));
        e.format_document();
        assert_eq!(
            e.text(),
            "local A = {}\nfunction A:Update()\n    if x then\n        y()\n    else\n        z()\n    end\nend\nlocal s = [[\n  keep\n]]"
        );
    }

    #[test]
    fn brackets_definitions_and_occurrences() {
        let mut e = ed(
            "f(a[1], {b})\nlocal function g() end\nfunction T:Run() g() end",
            (0, 1),
        );
        let tokens = lexer::tokenize(e.buf.lines());
        assert_eq!(
            e.matching_bracket(&tokens),
            Some((Pos::new(0, 1), Pos::new(0, 11)))
        );
        e.set_caret(Pos::new(0, 6));
        assert_eq!(
            e.matching_bracket(&tokens),
            Some((Pos::new(0, 5), Pos::new(0, 3)))
        );
        assert_eq!(e.definition_of("g"), Some(Pos::new(1, 15)));
        assert_eq!(e.definition_of("Run"), Some(Pos::new(2, 11)));
        assert_eq!(e.definition_of("nope"), None);
        let mut e = ed("x y x x", (0, 0));
        e.select_next_occurrence();
        e.select_next_occurrence();
        assert_eq!(e.sels.len(), 2);
        e.select_all_occurrences();
        assert_eq!(e.sels.len(), 3);
        e.replace_selections(EditKind::Typing, |_| "z".into());
        assert_eq!(e.text(), "z y z z");
    }

    #[test]
    fn backspace_in_indent_and_word_delete() {
        let mut e = ed("      x", (0, 6));
        e.backspace(false);
        assert_eq!(e.text(), "    x");
        let mut e = ed("local speed", (0, 11));
        e.backspace(true);
        assert_eq!(e.text(), "local ");
        let mut e = ed("a bc d", (0, 2));
        e.delete(true);
        assert_eq!(e.text(), "a d");
    }

    #[test]
    fn go_to_line_lands_on_the_first_character() {
        let mut e = ed("a\n   b\nc", (0, 0));
        e.go_to_line(2);
        assert_eq!(e.caret(), Pos::new(1, 3));
        e.go_to_line(99);
        assert_eq!(e.caret(), Pos::new(2, 0));
    }
}
