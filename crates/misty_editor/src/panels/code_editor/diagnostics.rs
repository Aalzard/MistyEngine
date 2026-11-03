//! Syntax errors as squiggles, and script locations in Console messages.

use std::sync::OnceLock;

use regex::Regex;

use super::text::col_of;

/// A syntax error placed in the text (0-based line, character columns).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

/// `Assets/….lua` followed by `:line:` (with or without Lua's `[string "…"]` wrapper).
fn location_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(Assets/[^"\]\n]*?\.lua)"?\]?:(\d+):\s*(.*)"#).expect("valid regex")
    })
}

/// Compile the buffer (without running it); `None` when it parses.
pub fn check(source: &str, path: &str, lines: &[String]) -> Option<Diagnostic> {
    let err = misty_engine::scripting::syntax::check(source, path).err()?;
    Some(parse(&err, lines).unwrap_or(Diagnostic {
        line: 0,
        start: 0,
        end: lines.first().map_or(0, |l| l.chars().count()),
        message: err,
    }))
}

/// Place a Lua error message: its line, and the token it names (`near 'x'`) or the whole line.
pub fn parse(message: &str, lines: &[String]) -> Option<Diagnostic> {
    let c = location_re().captures(message)?;
    let line: usize = c[2].parse().ok()?;
    let text = c[3].trim().to_owned();
    let mut l = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let src = lines.get(l).map(String::as_str).unwrap_or("");
    let (start, end) = if text.ends_with("near <eof>") {
        // End of file: mark the last non-blank character.
        while l > 0 && lines[l].trim().is_empty() {
            l -= 1;
        }
        let n = lines.get(l).map_or(0, |s| s.trim_end().chars().count());
        (n.saturating_sub(1), n.max(1))
    } else if let Some(tok) = text
        .split("near '")
        .nth(1)
        .and_then(|t| t.strip_suffix('\''))
    {
        // The offending token is usually the last one of its kind on the line.
        match src.rfind(tok) {
            Some(b) => {
                let s = col_of(src, b);
                (s, s + tok.chars().count())
            }
            None => whole_line(src),
        }
    } else {
        whole_line(src)
    };
    Some(Diagnostic {
        line: l,
        start,
        end,
        message: text,
    })
}

fn whole_line(src: &str) -> (usize, usize) {
    let indent = src.chars().count() - src.trim_start().chars().count();
    (indent, src.trim_end().chars().count().max(indent + 1))
}

/// Script and 1-based line named in a Console message (`Assets/Scripts/A.lua"]:12: …`).
pub fn error_location(message: &str) -> Option<(String, usize)> {
    let c = location_re().captures(message)?;
    Some((c[1].to_owned(), c[2].parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.split('\n').map(str::to_owned).collect()
    }

    #[test]
    fn syntax_errors_are_placed_on_their_token() {
        let src = "local A = {}\nfunction A:Update()\n    local x = = 2\nend\nreturn A";
        let d = check(src, "Assets/Scripts/A.lua", &lines(src)).unwrap();
        assert_eq!(d.line, 2);
        assert_eq!((d.start, d.end), (14, 15));
        assert!(d.message.contains("near '='"), "{}", d.message);
        assert!(check("return {}", "Assets/A.lua", &lines("return {}")).is_none());
    }

    #[test]
    fn missing_end_points_at_the_last_line() {
        let src = "function f()\n    x = 1\n\n";
        let d = check(src, "Assets/A.lua", &lines(src)).unwrap();
        assert_eq!(d.line, 1);
        assert_eq!((d.start, d.end), (8, 9));
        assert!(d.message.contains("'end' expected"), "{}", d.message);
    }

    #[test]
    fn console_messages_name_script_lines() {
        let runtime = r#"Rotator (Update): [string "Assets/Scripts/Rotator.lua"]:12: attempt to index a nil value"#;
        assert_eq!(
            error_location(runtime),
            Some(("Assets/Scripts/Rotator.lua".into(), 12))
        );
        let plain = "Assets/Scripts/Sub Dir/Foo.lua:3: oops";
        assert_eq!(
            error_location(plain),
            Some(("Assets/Scripts/Sub Dir/Foo.lua".into(), 3))
        );
        assert_eq!(error_location("Renderer: Vulkan"), None);
    }
}
