//! Lua tokenizer for highlighting, bracket matching, folding and formatting.
//!
//! Works line by line; long strings and comments (`[[ ]]`, `--[==[ ]==]`) carry over to the next
//! line through [`LineState`].

/// What a token is, which decides its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    /// `true`, `false`, `nil`.
    Literal,
    /// `self`.
    SelfKw,
    Number,
    String,
    Comment,
    /// Misty API globals such as `Input`, `Time`, `Vector3`.
    EngineGlobal,
    /// Lua standard library (`print`, `pairs`, `math`, …).
    Builtin,
    /// A name followed by `(`, or declared after `function`.
    Function,
    Identifier,
    Operator,
    /// `( ) [ ] { }`.
    Bracket,
    /// `, ; . :`.
    Punct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Character columns `start..end` on the line.
    pub start: usize,
    pub end: usize,
}

/// Lexer state at the start of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineState {
    #[default]
    Normal,
    /// Inside `[==[` … with that many `=`.
    LongString(usize),
    LongComment(usize),
}

pub const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in", "local",
    "not", "or", "repeat", "return", "then", "until", "while",
];

pub const BUILTINS: &[&str] = &[
    "assert",
    "collectgarbage",
    "error",
    "ipairs",
    "math",
    "next",
    "pairs",
    "pcall",
    "print",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "select",
    "setmetatable",
    "getmetatable",
    "string",
    "table",
    "tonumber",
    "tostring",
    "type",
    "unpack",
    "utf8",
    "xpcall",
];

/// Tokens of one line and the state for the next line.
pub fn tokenize_line(line: &str, state: LineState) -> (Vec<Token>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let n = chars.len();

    // Continue a long string or comment from the previous line.
    match state {
        LineState::LongString(level) | LineState::LongComment(level) => {
            let kind = if matches!(state, LineState::LongString(_)) {
                TokenKind::String
            } else {
                TokenKind::Comment
            };
            match find_long_close(&chars, 0, level) {
                Some(end) => {
                    tokens.push(Token {
                        kind,
                        start: 0,
                        end,
                    });
                    i = end;
                }
                None => {
                    if n > 0 {
                        tokens.push(Token {
                            kind,
                            start: 0,
                            end: n,
                        });
                    }
                    return (tokens, state);
                }
            }
        }
        LineState::Normal => {}
    }

    let mut after_function = false;
    while i < n {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Comments: `--` to the end of the line, or a long comment.
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            if let Some(level) = long_open(&chars, i + 2) {
                let body = i + 2 + level + 2;
                match find_long_close(&chars, body, level) {
                    Some(end) => {
                        tokens.push(tok(TokenKind::Comment, start, end));
                        i = end;
                        continue;
                    }
                    None => {
                        tokens.push(tok(TokenKind::Comment, start, n));
                        return (tokens, LineState::LongComment(level));
                    }
                }
            }
            tokens.push(tok(TokenKind::Comment, start, n));
            return (tokens, LineState::Normal);
        }
        // Long strings.
        if c == '['
            && let Some(level) = long_open(&chars, i)
        {
            let body = i + level + 2;
            match find_long_close(&chars, body, level) {
                Some(end) => {
                    tokens.push(tok(TokenKind::String, start, end));
                    i = end;
                    continue;
                }
                None => {
                    tokens.push(tok(TokenKind::String, start, n));
                    return (tokens, LineState::LongString(level));
                }
            }
        }
        if c == '"' || c == '\'' {
            i += 1;
            while i < n && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            tokens.push(tok(TokenKind::String, start, i));
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            i = number_end(&chars, i);
            tokens.push(tok(TokenKind::Number, start, i));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let prev_is_member = tokens.last().is_some_and(|t: &Token| {
                t.kind == TokenKind::Punct && matches!(chars[t.start], '.' | ':')
            });
            let next = chars[i..].iter().find(|c| !c.is_whitespace());
            let kind = if KEYWORDS.contains(&word.as_str()) {
                TokenKind::Keyword
            } else if matches!(word.as_str(), "true" | "false" | "nil") {
                TokenKind::Literal
            } else if word == "self" {
                TokenKind::SelfKw
            } else if !prev_is_member && BUILTINS.contains(&word.as_str()) {
                TokenKind::Builtin
            } else if after_function || matches!(next, Some('(')) {
                TokenKind::Function
            } else if !prev_is_member && super::api::is_global(&word) {
                TokenKind::EngineGlobal
            } else {
                TokenKind::Identifier
            };
            // `function Class:Name` colours the last name; `Class` and `:` stay plain.
            if word == "function" {
                after_function = true;
            } else if after_function && !matches!(next, Some('.' | ':')) {
                after_function = false;
            }
            let kind = if after_function && kind == TokenKind::Function {
                TokenKind::Identifier
            } else {
                kind
            };
            tokens.push(tok(kind, start, i));
            continue;
        }
        i += 1;
        // `function Class.Name`: only `.` / `:` keep looking for the declared name.
        if !matches!(c, '.' | ':') {
            after_function = false;
        }
        let kind = match c {
            '(' | ')' | '[' | ']' | '{' | '}' => TokenKind::Bracket,
            ',' | ';' | '.' | ':' => {
                // `..` and `...` are operators.
                if c == '.' && chars.get(i) == Some(&'.') {
                    while i < n && chars[i] == '.' {
                        i += 1;
                    }
                    TokenKind::Operator
                } else {
                    TokenKind::Punct
                }
            }
            _ => {
                if matches!(c, '=' | '~' | '<' | '>' | '/') && chars.get(i) == Some(&'=')
                    || (c == '/' && chars.get(i) == Some(&'/'))
                    || (c == '<' && chars.get(i) == Some(&'<'))
                    || (c == '>' && chars.get(i) == Some(&'>'))
                {
                    i += 1;
                }
                TokenKind::Operator
            }
        };
        tokens.push(tok(kind, start, i));
    }
    (tokens, LineState::Normal)
}

/// Tokens for every line.
pub fn tokenize(lines: &[String]) -> Vec<Vec<Token>> {
    let mut state = LineState::Normal;
    lines
        .iter()
        .map(|l| {
            let (t, s) = tokenize_line(l, state);
            state = s;
            t
        })
        .collect()
}

fn tok(kind: TokenKind, start: usize, end: usize) -> Token {
    Token { kind, start, end }
}

/// `[` `=`* `[` at `i`: the level (number of `=`).
fn long_open(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) != Some(&'[') {
        return None;
    }
    let mut j = i + 1;
    while chars.get(j) == Some(&'=') {
        j += 1;
    }
    (chars.get(j) == Some(&'[')).then_some(j - i - 1)
}

/// End (exclusive) of the `]` `=`*level `]` closing at or after `from`.
fn find_long_close(chars: &[char], from: usize, level: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == ']' {
            let mut j = i + 1;
            while chars.get(j) == Some(&'=') {
                j += 1;
            }
            if j - i - 1 == level && chars.get(j) == Some(&']') {
                return Some(j + 1);
            }
        }
        i += 1;
    }
    None
}

fn number_end(chars: &[char], mut i: usize) -> usize {
    let hex = chars[i] == '0' && matches!(chars.get(i + 1), Some('x' | 'X'));
    if hex {
        i += 2;
    }
    while i < chars.len() {
        let c = chars[i];
        let exp = if hex {
            matches!(c, 'p' | 'P')
        } else {
            matches!(c, 'e' | 'E')
        };
        if exp && matches!(chars.get(i + 1), Some('+' | '-')) {
            i += 2;
        } else if c.is_ascii_alphanumeric() || c == '.' {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// A block opener/closer seen while walking the tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Open,
    /// `else` / `elseif`: closes and reopens (dedents one line).
    Middle,
    Close,
}

/// Block structure of a token: `function`, `if`, `while`, `for`, `repeat`, `do`, `{` open;
/// `end`, `until`, `}` close. `do` after `while` / `for` doesn't open a second block, so the
/// caller passes whether a loop header is waiting for its `do`.
pub fn block_of(word: &str, awaiting_do: bool) -> Option<Block> {
    match word {
        "function" | "if" | "while" | "for" | "repeat" | "{" | "(" => Some(Block::Open),
        "do" if !awaiting_do => Some(Block::Open),
        "end" | "until" | "}" | ")" => Some(Block::Close),
        "else" | "elseif" => Some(Block::Middle),
        _ => None,
    }
}

/// Opening/closing block words of a line in order, with their columns.
pub fn block_words(line: &str, tokens: &[Token]) -> Vec<(usize, &'static str)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    for t in tokens {
        if !matches!(t.kind, TokenKind::Keyword | TokenKind::Bracket) {
            continue;
        }
        let w: String = chars[t.start..t.end].iter().collect();
        let word: &'static str = match w.as_str() {
            "function" => "function",
            "if" => "if",
            "while" => "while",
            "for" => "for",
            "repeat" => "repeat",
            "do" => "do",
            "end" => "end",
            "until" => "until",
            "else" => "else",
            "elseif" => "elseif",
            "{" => "{",
            "}" => "}",
            "(" => "(",
            ")" => ")",
            _ => continue,
        };
        out.push((t.start, word));
    }
    out
}

/// Nesting depth at the start of each line (after closers that begin the line), and fold ranges
/// (first line, last line) of every block spanning several lines.
pub struct Structure {
    pub indent: Vec<usize>,
    pub folds: Vec<(usize, usize)>,
}

pub fn structure(lines: &[String], tokens: &[Vec<Token>]) -> Structure {
    // Stack of (line where the block opened, waiting for `do`).
    let mut stack: Vec<(usize, bool)> = Vec::new();
    let mut indent = Vec::with_capacity(lines.len());
    let mut folds = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let words = block_words(line, &tokens[i]);
        // Closers at the start of the line dedent it.
        let mut depth = stack.len();
        for (_, w) in &words {
            match block_of(w, false) {
                Some(Block::Close | Block::Middle) => depth = depth.saturating_sub(1),
                _ => break,
            }
        }
        indent.push(depth);
        for (_, w) in words {
            let awaiting = stack.last().is_some_and(|s| s.1);
            match block_of(w, awaiting) {
                Some(Block::Open) => stack.push((i, matches!(w, "while" | "for"))),
                Some(Block::Close) => {
                    if let Some((start, _)) = stack.pop()
                        && i > start
                    {
                        folds.push((start, i));
                    }
                }
                Some(Block::Middle) => {}
                None if w == "do" => {
                    if let Some(top) = stack.last_mut() {
                        top.1 = false;
                    }
                }
                None => {}
            }
        }
    }
    folds.sort();
    Structure { indent, folds }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(TokenKind, String)> {
        let chars: Vec<char> = line.chars().collect();
        tokenize_line(line, LineState::Normal)
            .0
            .into_iter()
            .map(|t| (t.kind, chars[t.start..t.end].iter().collect()))
            .collect()
    }

    #[test]
    fn keywords_strings_numbers_and_globals() {
        use TokenKind::*;
        let k = kinds(r#"local x = Input.GetKey("w") and 0x1F or 1.5e-3 -- hi"#);
        assert_eq!(k[0], (Keyword, "local".into()));
        assert_eq!(k[3], (EngineGlobal, "Input".into()));
        assert_eq!(k[5], (Function, "GetKey".into()));
        assert_eq!(k[7], (String, "\"w\"".into()));
        assert!(k.contains(&(Number, "0x1F".into())));
        assert!(k.contains(&(Number, "1.5e-3".into())));
        assert_eq!(k.last().unwrap(), &(Comment, "-- hi".into()));
        let k = kinds("function Rotator:Update() return self end");
        assert_eq!(k[1], (Identifier, "Rotator".into()));
        assert_eq!(k[3], (Function, "Update".into()));
        assert!(k.contains(&(SelfKw, "self".into())));
        let k = kinds(r#"print('it\'s', a .. b, nil)"#);
        assert_eq!(k[0], (Builtin, "print".into()));
        assert_eq!(k[2], (String, r"'it\'s'".into()));
        assert!(k.contains(&(Operator, "..".into())));
        assert!(k.contains(&(Literal, "nil".into())));
    }

    #[test]
    fn long_comments_and_strings_span_lines() {
        let lines: Vec<String> = ["a = 1 --[[ start", "middle", "end ]] b = [==[x", "]==] c"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let toks = tokenize(&lines);
        assert_eq!(toks[0].last().unwrap().kind, TokenKind::Comment);
        assert_eq!(toks[1], vec![tok(TokenKind::Comment, 0, 6)]);
        assert_eq!(toks[2][0], tok(TokenKind::Comment, 0, 6));
        assert_eq!(toks[2].last().unwrap().kind, TokenKind::String);
        assert_eq!(toks[3][0], tok(TokenKind::String, 0, 4));
        assert_eq!(toks[3][1].kind, TokenKind::Identifier);
    }

    #[test]
    fn structure_gives_indents_and_folds() {
        let src =
            "function A:Update()\nif x then\nfor i = 1, 3 do\nend\nelse\nt = {\n1,\n}\nend\nend";
        let lines: Vec<String> = src.lines().map(str::to_owned).collect();
        let s = structure(&lines, &tokenize(&lines));
        assert_eq!(s.indent, vec![0, 1, 2, 2, 1, 2, 3, 2, 1, 0]);
        assert_eq!(s.folds, vec![(0, 9), (1, 8), (2, 3), (5, 7)]);
    }
}
