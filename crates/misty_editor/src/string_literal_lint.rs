//! Guard against string literals that lost their `\` line continuation.
//!
//! A literal written as `"... word \` + newline + indented `next ..."` reads "word next". When an
//! edit drops the backslash (shell heredocs strip a backslash-newline pair), the indentation ends up
//! inside the string and tooltips show the two words with a long run of spaces between them. This
//! test scans every `.rs` file under `crates/*/src` for ordinary (non-raw) string literals with:
//! - a source newline followed by 8 or more spaces (the backslash lost, the newline kept), or
//! - a run of 6 or more spaces between two non-space characters on one line of text (both lost).
//!
//! Raw strings are skipped: they hold code samples and fixtures, and cannot use `\` continuations.

use std::path::{Path, PathBuf};

/// Literals whose space runs are intentional: (path from the workspace root, text in the literal).
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "crates/misty_editor/src/sample_network.rs",
        "J  join",
        "aligned key help",
    ),
    (
        "crates/misty_engine/src/render/upscale_gpu_tests.rs",
        "GPU ms (rounds)",
        "table header",
    ),
    (
        "crates/misty_engine/src/render/grass_study/gpu.rs",
        "fn count_quad",
        "WGSL source",
    ),
    (
        "crates/misty_engine/src/testing/tests.rs",
        "local Mover",
        "Lua fixture",
    ),
];

/// An ordinary string literal: the line it starts on and its source text between the quotes.
struct Literal {
    line: usize,
    body: String,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The ordinary (non-raw) string literals in Rust source, skipping comments and char literals.
fn literals(src: &str) -> Vec<Literal> {
    let s: Vec<char> = src.chars().collect();
    let n = s.len();
    let at = |i: usize| s.get(i).copied().unwrap_or('\0');
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < n {
        let c = s[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c == '/' && at(i + 1) == '/' {
            while i < n && s[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == '*' {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if s[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if s[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    line += usize::from(s[i] == '\n');
                    i += 1;
                }
            }
        } else if c == 'r'
            && (i == 0 || !is_ident(s[i - 1]) || matches!(s[i - 1], 'b' | 'c'))
            && matches!(at(i + 1), '#' | '"')
        {
            // A raw string r"..." / r#"..."# (also br and cr), or a raw identifier r#name.
            let mut j = i + 1;
            while at(j) == '#' {
                j += 1;
            }
            if at(j) != '"' {
                i = j;
                continue;
            }
            let hashes = j - i - 1;
            j += 1;
            while j < n && !(s[j] == '"' && (1..=hashes).all(|k| at(j + k) == '#')) {
                line += usize::from(s[j] == '\n');
                j += 1;
            }
            i = j + 1 + hashes;
        } else if c == '\'' {
            if at(i + 1) == '\\' {
                // An escaped char literal: '\n', '\'', '\u{..}'.
                let mut j = i + 3;
                while j < n && s[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
            } else if at(i + 2) == '\'' {
                i += 3;
            } else {
                i += 1; // a lifetime or loop label
            }
        } else if c == '"' {
            let start = line;
            let mut j = i + 1;
            while j < n && s[j] != '"' {
                if s[j] == '\\' {
                    line += usize::from(at(j + 1) == '\n');
                    j += 2;
                } else {
                    line += usize::from(s[j] == '\n');
                    j += 1;
                }
            }
            out.push(Literal {
                line: start,
                body: s[i + 1..j.min(n)].iter().collect(),
            });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// What is wrong with a literal's source text, if anything.
fn problem(body: &str) -> Option<&'static str> {
    let s: Vec<char> = body.chars().filter(|&c| c != '\r').collect();
    let mut lines = vec![String::new()];
    let mut i = 0;
    while i < s.len() {
        match (s[i], s.get(i + 1).copied()) {
            ('\\', Some('\n')) => {
                // A proper continuation: the newline and the next line's indentation are skipped.
                i += 2;
                while i < s.len() && s[i].is_whitespace() {
                    i += 1;
                }
            }
            ('\\', Some('n')) => {
                lines.push(String::new());
                i += 2;
            }
            ('\\', Some(e)) => {
                lines.last_mut().unwrap().extend(['\\', e]);
                i += 2;
            }
            ('\n', _) => {
                let indent = s[i + 1..].iter().take_while(|&&c| c == ' ').count();
                if indent >= 8 {
                    return Some("a newline and indentation inside the string (lost `\\`?)");
                }
                lines.push(String::new());
                i += 1;
            }
            (c, _) => {
                lines.last_mut().unwrap().push(c);
                i += 1;
            }
        }
    }
    let gap = |l: &String| {
        let t = l.trim();
        let mut run = 0;
        t.chars().any(|c| {
            run = if c == ' ' { run + 1 } else { 0 };
            run >= 6
        })
    };
    lines
        .iter()
        .any(gap)
        .then_some("a run of 6+ spaces between words (lost `\\` continuation?)")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_string_literal_has_a_lost_line_continuation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for krate in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        let src = krate.path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(files.len() > 100, "found only {} source files", files.len());
    let mut used = vec![false; ALLOWED.len()];
    let mut found = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(file).unwrap();
        for lit in literals(&src) {
            let Some(why) = problem(&lit.body) else {
                continue;
            };
            let allowed = ALLOWED
                .iter()
                .position(|(p, text, _)| rel == *p && lit.body.contains(text));
            match allowed {
                Some(k) => used[k] = true,
                None => {
                    let head: String = lit.body.chars().take(80).collect();
                    found.push(format!("{rel}:{}: {why}: {head:?}", lit.line));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "string literals with stray indentation (restore the trailing `\\`, or add a deliberate \
        case to ALLOWED):\n{}",
        found.join("\n")
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(a, _)| a)
        .collect();
    assert!(
        stale.is_empty(),
        "ALLOWED entries that match nothing any more: {stale:?}"
    );
}

#[test]
fn the_checker_tells_continuations_from_lost_ones() {
    let gap = " ".repeat(10);
    // Proper continuation, and code-like text after an escaped newline: fine.
    assert_eq!(problem(&format!("word \\\n{gap}next")), None);
    assert_eq!(problem(&format!("fn f() {{\\n{gap}x\\n}}")), None);
    assert_eq!(problem("a   b"), None);
    // The backslash lost but the newline kept, or both lost.
    assert!(problem(&format!("word \n{gap}next")).is_some());
    assert!(problem(&format!("word{gap}next")).is_some());
}

#[test]
fn the_scanner_skips_comments_chars_and_raw_strings() {
    let src = "// \"not\" a string\n/* \"nor\" /* nested */ this */\nlet q = '\"';\n\
        let e = '\\'';\nfn f<'a>(x: &'a str) {}\nlet r = r#\"raw \"inner\"\"#;\n\
        let s = \"real\";\n";
    let found = literals(src);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].line, found[0].body.as_str()), (7, "real"));
}
