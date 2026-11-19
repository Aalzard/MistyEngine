//! C# support for `.cs` scripts: highlighting (keywords, types, the engine's classes, `//` and
//! `/* */` comments, verbatim and interpolated strings, `#` directives) and autocomplete read from
//! the engine's C# API (`MistyEngine.cs`): `transform.`, `Vector3.`, `rb.` for a `Rigidbody rb`
//! field, `GetComponent<Rigidbody>().`, type names inside `GetComponent<…>`.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use super::completion::{Context, Item, ItemKind, Target, rank};
use super::lexer::{LineState, Token, TokenKind};

/// Files the Code Editor treats as C#.
pub fn is_csharp(path: &str) -> bool {
    misty_engine::scripting::csharp::is_csharp_path(path)
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "base",
    "break",
    "case",
    "catch",
    "checked",
    "class",
    "const",
    "continue",
    "default",
    "delegate",
    "do",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "finally",
    "fixed",
    "for",
    "foreach",
    "get",
    "goto",
    "if",
    "implicit",
    "in",
    "init",
    "interface",
    "internal",
    "is",
    "lock",
    "nameof",
    "namespace",
    "new",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "record",
    "ref",
    "return",
    "sealed",
    "set",
    "sizeof",
    "stackalloc",
    "static",
    "struct",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "unchecked",
    "unsafe",
    "using",
    "value",
    "var",
    "virtual",
    "volatile",
    "when",
    "where",
    "while",
    "yield",
];

const TYPES: &[&str] = &[
    "bool", "byte", "char", "decimal", "double", "dynamic", "float", "int", "long", "object",
    "sbyte", "short", "string", "uint", "ulong", "ushort", "void",
];

/// Callbacks the engine calls on a MonoBehaviour, offered while typing a name.
const MESSAGES: &[(&str, &str)] = &[
    ("Awake", "void Awake()"),
    ("OnEnable", "void OnEnable()"),
    ("Start", "void Start()"),
    ("Update", "void Update()"),
    ("FixedUpdate", "void FixedUpdate()"),
    ("LateUpdate", "void LateUpdate()"),
    ("OnDisable", "void OnDisable()"),
    ("OnDestroy", "void OnDestroy()"),
    (
        "OnCollisionEnter",
        "void OnCollisionEnter(Collision collision)",
    ),
    (
        "OnCollisionStay",
        "void OnCollisionStay(Collision collision)",
    ),
    (
        "OnCollisionExit",
        "void OnCollisionExit(Collision collision)",
    ),
    ("OnTriggerEnter", "void OnTriggerEnter(Collider other)"),
    ("OnTriggerStay", "void OnTriggerStay(Collider other)"),
    ("OnTriggerExit", "void OnTriggerExit(Collider other)"),
];

// ---- highlighting -------------------------------------------------------------------------------

fn tok(kind: TokenKind, start: usize, end: usize) -> Token {
    Token { kind, start, end }
}

/// The end (after the closing quote) of a verbatim string from `from`, `""` being a quote.
fn verbatim_end(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '"' {
            if chars.get(i + 1) == Some(&'"') {
                i += 2;
                continue;
            }
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Tokens of one line, and the state the next line starts in (inside a `/* */` comment:
/// `LongComment`, inside a verbatim `@"…"` string: `LongString`).
pub fn tokenize_line(line: &str, state: LineState) -> (Vec<Token>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    match state {
        LineState::LongComment(_) => match find_close(&chars, 0) {
            Some(end) => {
                tokens.push(tok(TokenKind::Comment, 0, end));
                i = end;
            }
            None => {
                if n > 0 {
                    tokens.push(tok(TokenKind::Comment, 0, n));
                }
                return (tokens, state);
            }
        },
        LineState::LongString(_) => match verbatim_end(&chars, 0) {
            Some(end) => {
                tokens.push(tok(TokenKind::String, 0, end));
                i = end;
            }
            None => {
                if n > 0 {
                    tokens.push(tok(TokenKind::String, 0, n));
                }
                return (tokens, state);
            }
        },
        LineState::Normal => {}
    }
    // `#if`, `#region` …: the whole line.
    if line.trim_start().starts_with('#') && i == 0 {
        let start = n - line.trim_start().chars().count();
        tokens.push(tok(TokenKind::Keyword, start, n));
        return (tokens, LineState::Normal);
    }
    while i < n {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            tokens.push(tok(TokenKind::Comment, start, n));
            return (tokens, LineState::Normal);
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            match find_close(&chars, i + 2) {
                Some(end) => {
                    tokens.push(tok(TokenKind::Comment, start, end));
                    i = end;
                    continue;
                }
                None => {
                    tokens.push(tok(TokenKind::Comment, start, n));
                    return (tokens, LineState::LongComment(0));
                }
            }
        }
        // Strings: "…", $"…", @"…", $@"…", @$"…".
        let prefix: String = chars[i..n.min(i + 3)].iter().collect();
        let verbatim =
            prefix.starts_with("@\"") || prefix.starts_with("$@\"") || prefix.starts_with("@$\"");
        if verbatim {
            let open = i + prefix.find('"').unwrap_or(0) + 1;
            match verbatim_end(&chars, open) {
                Some(end) => {
                    tokens.push(tok(TokenKind::String, start, end));
                    i = end;
                    continue;
                }
                None => {
                    tokens.push(tok(TokenKind::String, start, n));
                    return (tokens, LineState::LongString(0));
                }
            }
        }
        if c == '"' || (c == '$' && chars.get(i + 1) == Some(&'"')) || c == '\'' {
            let quote = if c == '\'' { '\'' } else { '"' };
            i += if c == '$' { 2 } else { 1 };
            while i < n && chars[i] != quote {
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
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                if matches!(chars[i], 'e' | 'E') && matches!(chars.get(i + 1), Some('+' | '-')) {
                    i += 1;
                }
                i += 1;
            }
            tokens.push(tok(TokenKind::Number, start, i));
            continue;
        }
        if c.is_alphabetic()
            || c == '_'
            || (c == '@' && chars.get(i + 1).is_some_and(|c| c.is_alphabetic()))
        {
            i += 1;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let next = chars[i..].iter().find(|c| !c.is_whitespace());
            let kind = if KEYWORDS.contains(&word.as_str()) {
                TokenKind::Keyword
            } else if matches!(word.as_str(), "true" | "false" | "null") {
                TokenKind::Literal
            } else if TYPES.contains(&word.as_str()) {
                TokenKind::SelfKw
            } else if api().types.contains_key(&word) {
                TokenKind::EngineGlobal
            } else if matches!(next, Some('(')) {
                TokenKind::Function
            } else {
                TokenKind::Identifier
            };
            tokens.push(tok(kind, start, i));
            continue;
        }
        i += 1;
        let kind = match c {
            '(' | ')' | '[' | ']' | '{' | '}' => TokenKind::Bracket,
            ',' | ';' | '.' | ':' => TokenKind::Punct,
            _ => {
                if chars
                    .get(i)
                    .is_some_and(|n| matches!(n, '=' | '>' | '<' | '&' | '|' | '+' | '-'))
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

fn find_close(chars: &[char], from: usize) -> Option<usize> {
    (from..chars.len().saturating_sub(1))
        .find(|&i| chars[i] == '*' && chars[i + 1] == '/')
        .map(|i| i + 2)
}

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

// ---- the engine's C# API ------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemberKind {
    Method,
    Property,
    EnumValue,
}

#[derive(Clone, Debug)]
struct Member {
    name: String,
    kind: MemberKind,
    is_static: bool,
    /// Declared or returned type ("Vector3", "T", "void").
    ty: String,
    /// The declaration without its body ("public void Rotate(Vector3 eulers)").
    signature: String,
    doc: String,
}

#[derive(Clone, Debug, Default)]
struct ApiType {
    base: Option<String>,
    members: Vec<Member>,
    doc: String,
}

struct Api {
    types: HashMap<String, ApiType>,
}

/// Public types and members of namespace `Misty` in the engine's C# API, read once.
fn api() -> &'static Api {
    static API: OnceLock<Api> = OnceLock::new();
    API.get_or_init(|| parse_api(misty_engine::scripting::csharp::API_SOURCE))
}

/// Text of the line without strings and comments (for counting braces).
fn code_only(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == '\\' {
                    chars.next();
                } else if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '/' && chars.peek() == Some(&'/') {
                    break;
                }
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn summary(doc_lines: &[String]) -> String {
    let text = doc_lines.join(" ");
    let text = text.replace("<summary>", "").replace("</summary>", "");
    let tag = Regex::new(r"<[^>]+>").expect("valid regex");
    tag.replace_all(&text, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_api(source: &str) -> Api {
    let type_re = Regex::new(
        r"^public\s+(?:(?:sealed|static|abstract|partial|readonly|unsafe)\s+)*(class|struct|enum)\s+(\w+)(?:<[^>]*>)?(?:\s*:\s*([\w.]+))?",
    )
    .expect("valid regex");
    let method_re = Regex::new(
        r"^(?P<ty>[\w.<>\[\],?]+(?:\s*\[\])?)\s+(?P<name>\w+)\s*(?:<[^>]*>)?\s*\((?P<params>[^)]*)\)?",
    )
    .expect("valid regex");
    let field_re =
        Regex::new(r"^(?P<ty>[\w.<>\[\],?]+)\s+(?P<names>\w+(?:\s*,\s*\w+)*)\s*(?:=>|\{|=|;|$)")
            .expect("valid regex");
    let mut types: HashMap<String, ApiType> = HashMap::new();
    let mut depth = 0i32;
    // The type being read and the depth its body sits at.
    let mut current: Option<(String, bool, i32)> = None;
    let mut doc: Vec<String> = Vec::new();
    for raw in source.lines() {
        let line = raw.trim();
        if line.starts_with("namespace Misty.Internal") {
            break;
        }
        if let Some(d) = line.strip_prefix("///") {
            doc.push(d.trim().to_owned());
            continue;
        }
        let code = code_only(line);
        let opens = code.matches('{').count() as i32;
        let closes = code.matches('}').count() as i32;
        if let Some(c) = type_re.captures(line).filter(|_| depth == 1) {
            let name = c[2].to_owned();
            let is_enum = &c[1] == "enum";
            types.insert(
                name.clone(),
                ApiType {
                    base: c.get(3).map(|b| b.as_str().to_owned()),
                    members: Vec::new(),
                    doc: summary(&doc),
                },
            );
            // An enum on one line: `public enum ForceMode { Force, Impulse }`.
            if is_enum && let (Some(a), Some(b)) = (line.find('{'), line.rfind('}')) {
                let values = &line[a + 1..b];
                if let Some(t) = types.get_mut(&name) {
                    for v in values.split(',') {
                        let v = v.split('=').next().unwrap_or("").trim();
                        if !v.is_empty() {
                            t.members.push(enum_value(v, &name));
                        }
                    }
                }
            } else {
                current = Some((name, is_enum, depth + 1));
            }
            doc.clear();
            depth += opens - closes;
            continue;
        }
        if let Some((name, is_enum, body)) = current.clone()
            && depth == body
        {
            let t = types.get_mut(&name).expect("type just added");
            if is_enum {
                for v in code.split(',') {
                    let v = v.split('=').next().unwrap_or("").trim();
                    if !v.is_empty() && v.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        t.members.push(enum_value(v, &name));
                    }
                }
            } else if let Some(rest) = line.strip_prefix("public ") {
                let mut rest = rest.trim();
                let mut is_static = false;
                loop {
                    let word = rest.split_whitespace().next().unwrap_or("");
                    match word {
                        "static" | "const" => is_static = true,
                        "override" | "virtual" | "abstract" | "readonly" | "new" | "unsafe"
                        | "extern" | "sealed" => {}
                        _ => break,
                    }
                    rest = rest[word.len()..].trim_start();
                }
                let decl = rest.split("=>").next().unwrap_or(rest);
                let decl = decl
                    .split('{')
                    .next()
                    .unwrap_or(decl)
                    .trim()
                    .trim_end_matches(';');
                let signature = format!("{}{}", if is_static { "static " } else { "" }, decl);
                let doc_text = summary(&doc);
                if rest.contains("operator") || rest.starts_with(&format!("{name}(")) {
                    // Operators and constructors aren't members to complete.
                } else if let Some(m) = method_re.captures(rest) {
                    t.members.push(Member {
                        name: m["name"].to_owned(),
                        kind: MemberKind::Method,
                        is_static,
                        ty: m["ty"].to_owned(),
                        signature: signature.clone(),
                        doc: doc_text,
                    });
                } else if let Some(f) = field_re.captures(rest) {
                    for n in f["names"].split(',') {
                        t.members.push(Member {
                            name: n.trim().to_owned(),
                            kind: MemberKind::Property,
                            is_static,
                            ty: f["ty"].to_owned(),
                            signature: signature.clone(),
                            doc: doc_text.clone(),
                        });
                    }
                }
            }
        }
        if !line.is_empty() {
            doc.clear();
        }
        depth += opens - closes;
        if let Some((_, _, body)) = &current
            && depth < *body
        {
            current = None;
        }
    }
    Api { types }
}

fn enum_value(name: &str, ty: &str) -> Member {
    Member {
        name: name.to_owned(),
        kind: MemberKind::EnumValue,
        is_static: true,
        ty: ty.to_owned(),
        signature: format!("{ty}.{name}"),
        doc: String::new(),
    }
}

/// Members of `ty` and the types it derives from (the nearest first).
fn members_of(ty: &str) -> Vec<&'static Member> {
    let api = api();
    let mut out = Vec::new();
    let mut next = Some(ty.to_owned());
    let mut guard = 0;
    while let Some(name) = next.take() {
        guard += 1;
        let Some(t) = api.types.get(&name).filter(|_| guard < 16) else {
            break;
        };
        out.extend(t.members.iter());
        next = t.base.clone();
    }
    out
}

fn derives_from(ty: &str, base: &str) -> bool {
    let mut next = Some(ty.to_owned());
    let mut guard = 0;
    while let Some(name) = next.take() {
        guard += 1;
        if name == base {
            return true;
        }
        next = api()
            .types
            .get(&name)
            .and_then(|t| t.base.clone())
            .filter(|_| guard < 16);
    }
    false
}

fn member_item(m: &Member) -> Item {
    Item {
        label: m.name.clone(),
        kind: match m.kind {
            MemberKind::Method => ItemKind::Method,
            MemberKind::Property | MemberKind::EnumValue => ItemKind::Property,
        },
        signature: m.signature.clone(),
        doc: m.doc.clone(),
    }
}

// ---- completion ---------------------------------------------------------------------------------

/// Calls that take a component type: `GetComponent<` offers the types.
const TYPE_ARG_CALLS: &[&str] = &[
    "GetComponent<",
    "GetComponents<",
    "GetComponentInChildren<",
    "GetComponentInParent<",
    "TryGetComponent<",
    "AddComponent<",
    "FindObjectOfType<",
    "FindObjectsOfType<",
];

/// Completion context from the text before the caret, or `None` in comments and strings.
pub fn context(before: &str) -> Option<Context> {
    let chars: Vec<char> = before.chars().collect();
    let mut start = chars.len();
    while start > 0 && super::text::is_word(chars[start - 1]) {
        start -= 1;
    }
    let prefix: String = chars[start..].iter().collect();
    let head: String = chars[..start].iter().collect();
    let (tokens, state) = tokenize_line(before, LineState::Normal);
    if state != LineState::Normal
        || tokens.last().is_some_and(|t| {
            t.end >= start
                && t.start < start.max(1)
                && matches!(t.kind, TokenKind::Comment | TokenKind::String)
        })
    {
        return None;
    }
    if prefix.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    if TYPE_ARG_CALLS.iter().any(|c| head.ends_with(c)) {
        return Some(Context {
            prefix,
            start,
            target: Target::Component,
        });
    }
    let target = match chars[..start].last() {
        Some('.') => {
            // Back over `a.b(…).c<T>()` to where the expression starts.
            let mut s = start - 1;
            let (mut paren, mut angle) = (0, 0);
            while s > 0 {
                let c = chars[s - 1];
                match c {
                    ')' => paren += 1,
                    '(' if paren > 0 => paren -= 1,
                    '>' => angle += 1,
                    '<' if angle > 0 => angle -= 1,
                    _ if paren > 0 => {}
                    _ if super::text::is_word(c) || c == '.' => {}
                    _ => break,
                }
                s -= 1;
            }
            let expr: String = chars[s..start - 1].iter().collect();
            if expr.is_empty() {
                return None;
            }
            Target::Member { expr, colon: false }
        }
        _ => Target::Name,
    };
    Some(Context {
        prefix,
        start,
        target,
    })
}

/// Candidates for a context, ranked against its prefix (best first).
pub fn complete(ctx: &Context, lines: &[String]) -> Vec<Item> {
    let items = match &ctx.target {
        Target::Component => component_types(),
        Target::Name => names(lines),
        Target::Member { expr, .. } => match resolve(expr, lines) {
            Some((ty, statics)) => {
                let mut seen = std::collections::HashSet::new();
                members_of(&ty)
                    .into_iter()
                    .filter(|m| m.is_static == statics)
                    .filter(|m| seen.insert(m.name.clone()))
                    .map(member_item)
                    .collect()
            }
            None => Vec::new(),
        },
    };
    rank(items, &ctx.prefix)
}

/// Types `GetComponent<…>` takes: the engine's components and the project's scripts.
fn component_types() -> Vec<Item> {
    let mut items: Vec<Item> = api()
        .types
        .iter()
        .filter(|(n, _)| n.as_str() != "Component" && derives_from(n, "Component"))
        .map(|(n, t)| Item {
            label: n.clone(),
            kind: ItemKind::Component,
            signature: format!("GetComponent<{n}>()"),
            doc: t.doc.clone(),
        })
        .collect();
    for name in misty_engine::scripting::csharp_fields::script_classes() {
        items.push(Item {
            signature: format!("GetComponent<{name}>()"),
            label: name,
            kind: ItemKind::Component,
            doc: "Script".into(),
        });
    }
    items
}

/// Keywords, types, the engine's classes, what a MonoBehaviour has, the callbacks and the
/// names in the file.
fn names(lines: &[String]) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for k in KEYWORDS
        .iter()
        .chain(TYPES)
        .chain(["true", "false", "null"].iter())
    {
        items.push(Item {
            label: (*k).to_owned(),
            kind: ItemKind::Keyword,
            signature: String::new(),
            doc: String::new(),
        });
    }
    for (n, t) in &api().types {
        items.push(Item {
            label: n.clone(),
            kind: ItemKind::Global,
            signature: format!("class {n}"),
            doc: t.doc.clone(),
        });
    }
    // Inside a script: its own (inherited) members without `this.`.
    let mut seen = std::collections::HashSet::new();
    for m in members_of("MonoBehaviour") {
        if seen.insert(m.name.clone()) {
            items.push(member_item(m));
        }
    }
    for (n, sig) in MESSAGES {
        items.push(Item {
            label: (*n).to_owned(),
            kind: ItemKind::Method,
            signature: (*sig).to_owned(),
            doc: "Called by the engine".into(),
        });
    }
    let word = Regex::new(r"[A-Za-z_]\w*").expect("valid regex");
    let mut state = LineState::Normal;
    for line in lines {
        let (tokens, s) = tokenize_line(line, state);
        state = s;
        let chars: Vec<char> = line.chars().collect();
        for t in tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Identifier || t.kind == TokenKind::Function)
        {
            let w: String = chars[t.start..t.end].iter().collect();
            if word.is_match(&w) && w.len() > 1 {
                items.push(Item {
                    label: w,
                    kind: ItemKind::Local,
                    signature: String::new(),
                    doc: String::new(),
                });
            }
        }
    }
    items
}

/// Split `a.b(x).c<T>()` at the dots outside brackets.
fn segments(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut paren, mut angle) = (0, 0);
    for c in expr.chars() {
        match c {
            '(' => paren += 1,
            ')' => paren -= 1,
            '<' => angle += 1,
            '>' => angle -= 1,
            '.' if paren == 0 && angle == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// The type a segment's call or name gives on `ty` (`None`: unknown).
fn member_type(ty: &str, segment: &str) -> Option<String> {
    let name: String = segment
        .chars()
        .take_while(|c| super::text::is_word(*c))
        .collect();
    // A generic call answers its type argument: GetComponent<Rigidbody>().
    if let (Some(a), Some(b)) = (segment.find('<'), segment.find('>'))
        && a < b
    {
        let arg = segment[a + 1..b].trim();
        if !arg.is_empty() {
            return Some(arg.to_owned());
        }
    }
    let m = members_of(ty).into_iter().find(|m| m.name == name)?;
    let t = m.ty.trim_end_matches("[]").to_owned();
    (t != "void").then_some(t)
}

/// The type of `expr` and whether its static members are meant (`Vector3.` vs `transform.`).
fn resolve(expr: &str, lines: &[String]) -> Option<(String, bool)> {
    let segs = segments(expr);
    let first = segs.first()?;
    let (mut ty, mut statics) = if first == "this" || first == "base" {
        ("MonoBehaviour".to_owned(), false)
    } else if api().types.contains_key(first.as_str()) {
        (first.clone(), true)
    } else if let Some(t) = member_type("MonoBehaviour", first) {
        // `transform`, `gameObject`, `GetComponent<Rigidbody>()` inside a script.
        (t, false)
    } else {
        (declared_type(first.trim_end_matches("()"), lines)?, false)
    };
    for seg in &segs[1..] {
        ty = member_type(&ty, seg)?;
        statics = false;
    }
    Some((ty, statics))
}

/// The declared type of a field, local or parameter `name` in the file: `Rigidbody rb`,
/// `var rb = GetComponent<Rigidbody>()`, `var v = new Vector3(…)`.
fn declared_type(name: &str, lines: &[String]) -> Option<String> {
    if name.is_empty() || !name.chars().all(super::text::is_word) {
        return None;
    }
    let typed = Regex::new(&format!(
        r"\b([A-Z]\w*)(?:<\w+>)?(?:\[\])?\s+{name}\s*(?:=|;|,|\)|\bin\b)"
    ))
    .ok()?;
    let var_new = Regex::new(&format!(r"\bvar\s+{name}\s*=\s*new\s+(\w+)")).ok()?;
    let var_generic = Regex::new(&format!(r"\bvar\s+{name}\s*=\s*[\w.]*<(\w+)>\s*\(")).ok()?;
    for line in lines {
        let code = code_only(line);
        for re in [&var_new, &var_generic, &typed] {
            if let Some(c) = re.captures(&code) {
                return Some(c[1].to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.split('\n').map(str::to_owned).collect()
    }

    fn labels_for(before: &str, file: &str) -> Vec<String> {
        let ctx = context(before).expect("a completion context");
        complete(&ctx, &lines(file))
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn highlights_csharp() {
        let l = lines(
            "using Misty; // the API\npublic class A : MonoBehaviour { /* note\nstill */ string s = @\"C:\\a\"\"b\"; int n = 3; }\n#if DEBUG",
        );
        let t = tokenize(&l);
        let kinds = |i: usize| t[i].iter().map(|t| t.kind).collect::<Vec<_>>();
        assert_eq!(kinds(0)[0], TokenKind::Keyword, "using");
        assert_eq!(*kinds(0).last().unwrap(), TokenKind::Comment, "// comment");
        assert!(kinds(1).contains(&TokenKind::EngineGlobal), "MonoBehaviour");
        assert_eq!(
            *kinds(1).last().unwrap(),
            TokenKind::Comment,
            "block comment opens"
        );
        assert_eq!(kinds(2)[0], TokenKind::Comment, "and carries over");
        assert!(kinds(2).contains(&TokenKind::String), "verbatim string");
        assert!(kinds(2).contains(&TokenKind::SelfKw), "built-in types");
        assert_eq!(kinds(3), [TokenKind::Keyword], "directive");
        // `i--` is not a Lua comment here.
        let (t, _) = tokenize_line("i--; // done", LineState::Normal);
        assert_eq!(t[0].kind, TokenKind::Identifier);
    }

    #[test]
    fn the_api_is_read_from_the_engine_source() {
        let transform = members_of("Transform");
        let find = |n: &str| transform.iter().find(|m| m.name == n).copied();
        assert_eq!(find("position").unwrap().ty, "Vector3");
        assert_eq!(find("Rotate").unwrap().kind, MemberKind::Method);
        // Inherited from Component.
        assert_eq!(find("gameObject").unwrap().ty, "GameObject");
        let force = members_of("ForceMode");
        assert!(
            force
                .iter()
                .any(|m| m.name == "Impulse" && m.kind == MemberKind::EnumValue)
        );
        assert!(members_of("KeyCode").iter().any(|m| m.name == "Space"));
        assert!(
            members_of("Vector3")
                .iter()
                .any(|m| m.name == "up" && m.is_static)
        );
    }

    #[test]
    fn members_follow_types_through_the_expression() {
        let file = "using Misty;\npublic class P : MonoBehaviour\n{\n    Rigidbody rb;\n    void Update()\n    {\n        var v = new Vector3(1, 2, 3);\n    }\n}";
        let t = labels_for("        transform.", file);
        assert!(
            t.contains(&"position".into()) && t.contains(&"Rotate".into()),
            "{t:?}"
        );
        assert!(!t.contains(&"zero".into()), "no statics on an instance");
        let s = labels_for("        Vector3.", file);
        assert!(
            s.contains(&"up".into()) && s.contains(&"Lerp".into()),
            "{s:?}"
        );
        assert!(labels_for("        rb.", file).contains(&"AddForce".into()));
        assert!(
            labels_for("        GetComponent<Rigidbody>().", file).contains(&"velocity".into())
        );
        assert!(labels_for("        transform.position.", file).contains(&"normalized".into()));
        assert!(labels_for("        v.", file).contains(&"magnitude".into()));
        assert!(labels_for("        Input.GetK", file).contains(&"GetKeyDown".into()));
        assert!(
            labels_for("        rb.AddForce(Vector3.up, ForceMode.", file)
                .contains(&"Impulse".into())
        );
        let types = labels_for("        GetComponent<", file);
        assert!(types.contains(&"Rigidbody".into()) && !types.contains(&"Vector3".into()));
        let names = labels_for("    void OnTrig", file);
        assert!(names.contains(&"OnTriggerEnter".into()), "{names:?}");
        assert!(context("        // transform.").is_none());
        assert!(context("        Debug.Log(\"transform.").is_none());
    }

    #[test]
    fn toggle_comment_uses_slashes_in_csharp() {
        let mut e = super::super::editor::TextEditor::new(
            "int a;
    int b;",
        );
        e.comment = "//";
        e.select_all();
        e.toggle_comment();
        assert_eq!(
            e.text(),
            "// int a;
//     int b;"
        );
        e.select_all();
        e.toggle_comment();
        assert_eq!(
            e.text(),
            "int a;
    int b;"
        );
    }
}
