//! Find / Replace in one file (match case, whole word, regex) and Find in Files.
//! Matches never span lines.

use std::path::Path;

use regex::{Regex, RegexBuilder};

use super::editor::TextEditor;
use super::history::EditKind;
use super::text::{Pos, Selection, col_of, is_word};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// A compiled query.
pub struct Query {
    re: Regex,
    whole_word: bool,
    regex: bool,
}

impl Query {
    pub fn new(text: &str, opts: FindOptions) -> Result<Self, String> {
        if text.is_empty() {
            return Err("empty".into());
        }
        let pattern = if opts.regex {
            text.to_owned()
        } else {
            regex::escape(text)
        };
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(!opts.match_case)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            re,
            whole_word: opts.whole_word,
            regex: opts.regex,
        })
    }

    /// Matches in one line as character ranges.
    pub fn find_in_line(&self, line: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for m in self.re.find_iter(line) {
            if m.start() == m.end() {
                continue;
            }
            if self.whole_word {
                let before = line[..m.start()].chars().last();
                let after = line[m.end()..].chars().next();
                if before.is_some_and(is_word) || after.is_some_and(is_word) {
                    continue;
                }
            }
            out.push((col_of(line, m.start()), col_of(line, m.end())));
        }
        out
    }

    /// Every match in the text, in order.
    pub fn find_all(&self, lines: &[String]) -> Vec<(Pos, Pos)> {
        lines
            .iter()
            .enumerate()
            .flat_map(|(l, line)| {
                self.find_in_line(line)
                    .into_iter()
                    .map(move |(a, b)| (Pos::new(l, a), Pos::new(l, b)))
            })
            .collect()
    }

    /// The replacement for `matched`: `$1` groups expand in regex mode.
    pub fn expand(&self, matched: &str, replacement: &str) -> String {
        if !self.regex {
            return replacement.to_owned();
        }
        match self.re.captures(matched) {
            Some(c) => {
                let mut out = String::new();
                c.expand(replacement, &mut out);
                out
            }
            None => replacement.to_owned(),
        }
    }
}

/// The match after (or before, going back) the primary selection, wrapping around.
pub fn next_match(matches: &[(Pos, Pos)], sel: Selection, forward: bool) -> Option<(Pos, Pos)> {
    if forward {
        let from = sel.end();
        matches
            .iter()
            .find(|m| m.0 >= from && (m.0, m.1) != (sel.start(), sel.end()))
            .or_else(|| matches.first())
            .copied()
    } else {
        let from = sel.start();
        matches
            .iter()
            .rev()
            .find(|m| m.0 < from)
            .or_else(|| matches.last())
            .copied()
    }
}

/// Replace: if the selection is a match it's replaced, then the next match is selected.
/// Returns whether something was replaced.
pub fn replace_next(ed: &mut TextEditor, q: &Query, replacement: &str) -> bool {
    let sel = ed.primary();
    let matches = q.find_all(ed.buf.lines());
    let replaced = if matches.contains(&(sel.start(), sel.end())) && !sel.is_empty() {
        let text = q.expand(&ed.buf.slice(sel.start(), sel.end()), replacement);
        ed.set_selection(sel);
        ed.replace_selections(EditKind::Other, |_| text.clone());
        true
    } else {
        false
    };
    let matches = q.find_all(ed.buf.lines());
    if let Some((a, b)) = next_match(&matches, ed.primary(), true) {
        ed.set_selection(Selection::new(a, b));
    }
    replaced
}

/// Replace every match as one undo step; returns how many.
pub fn replace_all(ed: &mut TextEditor, q: &Query, replacement: &str) -> usize {
    let matches = q.find_all(ed.buf.lines());
    let edits: Vec<_> = matches
        .iter()
        .map(|&(a, b)| (a, b, q.expand(&ed.buf.slice(a, b), replacement)))
        .collect();
    let n = edits.len();
    ed.apply(EditKind::Other, edits);
    n
}

/// One Find in Files result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatch {
    /// Project-relative path ("Assets/Scripts/Rotator.lua").
    pub path: String,
    pub line: usize,
    pub col: usize,
    pub len: usize,
    pub text: String,
}

/// Search every `.lua` file under `root/Assets` (sorted by path).
pub fn find_in_files(root: &Path, q: &Query) -> Vec<FileMatch> {
    let mut files = Vec::new();
    collect_scripts(&root.join("Assets"), &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let rel = file
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        for (l, line) in text.lines().enumerate() {
            for (a, b) in q.find_in_line(line) {
                out.push(FileMatch {
                    path: rel.clone(),
                    line: l,
                    col: a,
                    len: b - a,
                    text: line.trim().to_owned(),
                });
            }
        }
    }
    out
}

fn collect_scripts(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_scripts(&p, out);
        } else if p.extension().is_some_and(|x| x == "lua") {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_owned).collect()
    }

    #[test]
    fn options_change_what_matches() {
        let text = lines("Speed speed speedy\nself.speed = 1");
        let q = |t, match_case, whole_word| {
            Query::new(
                t,
                FindOptions {
                    match_case,
                    whole_word,
                    regex: false,
                },
            )
            .unwrap()
            .find_all(&text)
        };
        assert_eq!(q("speed", false, false).len(), 4);
        assert_eq!(q("speed", true, false).len(), 3);
        assert_eq!(q("speed", false, true).len(), 3);
        assert_eq!(
            q("speed", true, true),
            vec![
                (Pos::new(0, 6), Pos::new(0, 11)),
                (Pos::new(1, 5), Pos::new(1, 10)),
            ]
        );
        assert!(Query::new("", FindOptions::default()).is_err());
        let bad = FindOptions {
            regex: true,
            ..Default::default()
        };
        assert!(Query::new("(", bad).is_err());
        // Special characters are literal without regex.
        assert_eq!(
            Query::new("self.", FindOptions::default())
                .unwrap()
                .find_all(&text)
                .len(),
            1
        );
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let m = vec![
            (Pos::new(0, 0), Pos::new(0, 1)),
            (Pos::new(2, 0), Pos::new(2, 1)),
        ];
        let at = |l, c| Selection::caret(Pos::new(l, c));
        assert_eq!(next_match(&m, at(1, 0), true), Some(m[1]));
        assert_eq!(
            next_match(&m, Selection::new(m[1].0, m[1].1), true),
            Some(m[0])
        );
        assert_eq!(next_match(&m, at(1, 0), false), Some(m[0]));
        assert_eq!(next_match(&m, at(0, 0), false), Some(m[1]));
    }

    #[test]
    fn replace_next_and_all_with_regex_groups() {
        let mut ed = TextEditor::new("a1 b2 a3");
        let opts = FindOptions {
            regex: true,
            match_case: true,
            ..Default::default()
        };
        let q = Query::new(r"a(\d)", opts).unwrap();
        // First call selects the first match, the second replaces it.
        assert!(!replace_next(&mut ed, &q, "x$1"));
        assert_eq!(ed.primary(), Selection::new(Pos::new(0, 0), Pos::new(0, 2)));
        assert!(replace_next(&mut ed, &q, "x$1"));
        assert_eq!(ed.text(), "x1 b2 a3");
        assert_eq!(ed.primary(), Selection::new(Pos::new(0, 6), Pos::new(0, 8)));
        ed.undo();
        assert_eq!(replace_all(&mut ed, &q, "<$1>"), 2);
        assert_eq!(ed.text(), "<1> b2 <3>");
        ed.undo();
        assert_eq!(ed.text(), "a1 b2 a3", "one undo step");
    }

    #[test]
    fn find_in_files_searches_project_scripts() {
        let root = std::env::temp_dir().join(format!("misty_find_files_{}", std::process::id()));
        let dir = root.join("Assets/Scripts/Sub");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            root.join("Assets/Scripts/A.lua"),
            "x = 1\n  Input.GetKey('w')",
        )
        .unwrap();
        std::fs::write(dir.join("B.lua"), "Input.GetAxis('Horizontal')").unwrap();
        std::fs::write(dir.join("notes.txt"), "Input").unwrap();
        let q = Query::new("Input", FindOptions::default()).unwrap();
        let found = find_in_files(&root, &q);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].path, "Assets/Scripts/A.lua");
        assert_eq!((found[0].line, found[0].col, found[0].len), (1, 2, 5));
        assert_eq!(found[0].text, "Input.GetKey('w')");
        assert_eq!(found[1].path, "Assets/Scripts/Sub/B.lua");
        std::fs::remove_dir_all(root).ok();
    }
}
