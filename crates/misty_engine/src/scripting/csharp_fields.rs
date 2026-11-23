//! The Properties panel's view of a C# script: its serialized fields, read from the source.
//!
//! Reading the text (instead of compiling and reflecting) keeps the panel instant and works
//! without .NET, in `misty --mcp` and while the code doesn't compile. The rules follow Unity's:
//! a field of the script's class (named like the file, else the first class deriving from
//! MonoBehaviour) shows when it is `public` or has `[SerializeField]`, and is not `static`,
//! `const`, `readonly` or `[NonSerialized]`. Supported types: `float`, `double`, `int`,
//! `long`, `short`, `byte`, `uint` (numbers), `bool`, `string` and `Vector3`. The default is
//! the initializer when it is a literal (`90f`, `-2.5`, `true`, `"Player"`,
//! `new Vector3(0, 1, 0)`, `Vector3.up`), else the type's zero.
//!
//! `[Header("…")]`, `[Tooltip("…")]`, `[Space]` / `[Space(12)]`, `[Range(a, b)]` and
//! `[HideInInspector]` set the same [`FieldAttributes`] as the Lua script attributes.

use std::collections::HashMap;

use super::ScriptInfo;
use super::attributes::FieldAttributes;
use crate::scene::ScriptValue;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Num(String),
    Str(String),
    Punct(char),
    /// `=>`
    Arrow,
}

/// Split C# source into tokens, without comments, preprocessor lines and char literals.
fn tokenize(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if line_start && c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        line_start = false;
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            continue;
        }
        // Strings: "…", @"…" (verbatim), $"…" / $@"…" (interpolated; braces skipped).
        let (verbatim, interpolated, skip) = match (c, chars.get(i + 1), chars.get(i + 2)) {
            ('"', _, _) => (false, false, 1),
            ('@', Some('"'), _) => (true, false, 2),
            ('$', Some('"'), _) => (false, true, 2),
            ('$', Some('@'), Some('"')) | ('@', Some('$'), Some('"')) => (true, true, 3),
            _ => (false, false, 0),
        };
        if skip > 0 {
            i += skip;
            let mut s = String::new();
            let mut depth = 0;
            while i < chars.len() {
                let ch = chars[i];
                if interpolated
                    && depth == 0
                    && (ch == '{' || ch == '}')
                    && chars.get(i + 1) == Some(&ch)
                {
                    // `{{` / `}}`: a literal brace.
                    s.push(ch);
                    i += 2;
                    continue;
                }
                if interpolated && ch == '{' {
                    depth += 1;
                } else if interpolated && ch == '}' && depth > 0 {
                    depth -= 1;
                } else if depth == 0 && ch == '"' {
                    if verbatim && chars.get(i + 1) == Some(&'"') {
                        s.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                } else if !verbatim && ch == '\\' {
                    if let Some(&next) = chars.get(i + 1) {
                        s.push(match next {
                            'n' => '\n',
                            't' => '\t',
                            '0' => '\0',
                            other => other,
                        });
                    }
                    i += 2;
                    continue;
                }
                s.push(ch);
                i += 1;
            }
            out.push(Tok::Str(s));
            continue;
        }
        if c == '\'' {
            // A char literal: skip it.
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() {
                let ch = chars[i];
                let exponent_sign = (ch == '+' || ch == '-')
                    && matches!(chars[i - 1], 'e' | 'E')
                    && !chars[start..i].iter().any(|c| *c == 'x' || *c == 'X');
                if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || exponent_sign {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '@' {
            let start = if c == '@' { i + 1 } else { i };
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
            continue;
        }
        if c == '=' && chars.get(i + 1) == Some(&'>') {
            out.push(Tok::Arrow);
            i += 2;
            continue;
        }
        out.push(Tok::Punct(c));
        i += 1;
    }
    out
}

fn is_punct(t: &Tok, c: char) -> bool {
    matches!(t, Tok::Punct(p) if *p == c)
}

fn ident(t: &Tok) -> Option<&str> {
    match t {
        Tok::Ident(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Index just past the bracket matching the opener at `open` (`{`, `(`, `[` or `<`).
fn skip_group(toks: &[Tok], open: usize) -> usize {
    let (o, c) = match toks.get(open) {
        Some(Tok::Punct('{')) => ('{', '}'),
        Some(Tok::Punct('(')) => ('(', ')'),
        Some(Tok::Punct('[')) => ('[', ']'),
        Some(Tok::Punct('<')) => ('<', '>'),
        _ => return open + 1,
    };
    let mut depth = 0;
    let mut i = open;
    while i < toks.len() {
        if is_punct(&toks[i], o) {
            depth += 1;
        } else if is_punct(&toks[i], c) {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    toks.len()
}

/// The token range of the body of the script's class.
fn class_body(toks: &[Tok], stem: &str) -> Option<(usize, usize)> {
    let mut first_behaviour = None;
    let mut first_class = None;
    let mut i = 0;
    while i + 1 < toks.len() {
        if ident(&toks[i]) == Some("class")
            && let Some(name) = ident(&toks[i + 1])
        {
            let Some(open) = (i + 2..toks.len()).find(|&j| is_punct(&toks[j], '{')) else {
                break;
            };
            let close = skip_group(toks, open);
            let range = (open + 1, close.saturating_sub(1));
            if name == stem {
                return Some(range);
            }
            if first_behaviour.is_none()
                && toks[i + 2..open].iter().any(|t| {
                    ident(t).is_some_and(|s| s == "MonoBehaviour" || s.ends_with("Behaviour"))
                })
            {
                first_behaviour = Some(range);
            }
            first_class.get_or_insert(range);
            // Classes nested in this one are looked at too (the walk goes on inside).
            i += 2;
            continue;
        }
        i += 1;
    }
    first_behaviour.or(first_class)
}

/// The member declarations ending in `;` at the top level of a class body (methods,
/// properties and nested types are left out).
fn statements(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if is_punct(t, '(') || is_punct(t, '[') {
            i = skip_group(toks, i);
            continue;
        }
        if is_punct(t, '{') {
            let in_initializer = toks[start..i].iter().any(|t| is_punct(t, '='));
            i = skip_group(toks, i);
            if in_initializer {
                continue;
            }
            // A method, property or nested type; `{ get; set; } = 5;` has a tail to drop.
            if toks.get(i).is_some_and(|t| is_punct(t, '=')) {
                while i < toks.len() && !is_punct(&toks[i], ';') {
                    i += 1;
                }
                i += 1;
            }
            start = i;
            continue;
        }
        if is_punct(t, ';') {
            out.push(&toks[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out
}

/// One `[Name(args)]` attribute.
struct Attr {
    name: String,
    args: Vec<Tok>,
}

/// Attributes in the `[...]` group starting at `open`.
fn attributes(toks: &[Tok], open: usize, end: usize) -> Vec<Attr> {
    let mut out = Vec::new();
    let mut i = open + 1;
    let close = end - 1;
    while i < close {
        let mut name = String::new();
        while i < close && !is_punct(&toks[i], ',') && !is_punct(&toks[i], '(') {
            if let Some(s) = ident(&toks[i]) {
                name = s.to_owned();
            }
            i += 1;
        }
        let mut args = Vec::new();
        if i < close && is_punct(&toks[i], '(') {
            let after = skip_group(toks, i);
            args = toks[i + 1..after - 1].to_vec();
            i = after;
        }
        if i < close && is_punct(&toks[i], ',') {
            i += 1;
        }
        let name = name.strip_suffix("Attribute").unwrap_or(&name).to_owned();
        if !name.is_empty() {
            out.push(Attr { name, args });
        }
    }
    out
}

/// A number literal with an optional sign at `toks[i..]`; returns it and the tokens used.
fn number(toks: &[Tok]) -> Option<(f64, usize)> {
    let (sign, i) = match toks.first() {
        Some(Tok::Punct('-')) => (-1.0, 1),
        Some(Tok::Punct('+')) => (1.0, 1),
        _ => (1.0, 0),
    };
    let Some(Tok::Num(n)) = toks.get(i) else {
        return None;
    };
    let n = n.replace('_', "");
    let v = if let Some(hex) = n.strip_prefix("0x").or_else(|| n.strip_prefix("0X")) {
        let digits = hex.trim_end_matches(['u', 'U', 'l', 'L']);
        i64::from_str_radix(digits, 16).ok()? as f64
    } else {
        n.trim_end_matches(['f', 'F', 'd', 'D', 'm', 'M', 'u', 'U', 'l', 'L'])
            .parse::<f64>()
            .ok()?
    };
    Some((sign * v, i + 1))
}

/// Comma-separated number literals (the arguments of `new Vector3(...)` or `Range(...)`).
fn numbers(toks: &[Tok]) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let (v, used) = number(&toks[i..])?;
        out.push(v);
        i += used;
        match toks.get(i) {
            None => break,
            Some(Tok::Punct(',')) => i += 1,
            Some(_) => return None,
        }
    }
    Some(out)
}

#[derive(Clone, PartialEq)]
enum Kind {
    Number,
    Bool,
    Text,
    Vector3,
    /// A scene object, one of its components or a script on it (the type's name).
    Object(String),
}

/// Engine types an object field can hold.
const OBJECT_TYPES: &[&str] = &[
    "GameObject",
    "Transform",
    "Component",
    "MonoBehaviour",
    "Rigidbody",
    "Collider",
    "AudioSource",
    "Animator",
    "Camera",
    "Light",
    "Renderer",
    "MeshRenderer",
    "Text",
    "Image",
    "Button",
    "Toggle",
    "Slider",
    "InputField",
    "Rigidbody2D",
    "Collider2D",
    "NavMeshAgent",
    "ParticleSystem",
    "CharacterController",
];

/// Script classes of the project (a C# script's class is named like its file): fields of
/// these types are object fields too. Set by the editor when it lists the scripts.
static SCRIPT_CLASSES: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

/// The project's C# script classes (file names without `.cs`), for object fields that hold
/// another script.
pub fn set_script_classes(mut names: Vec<String>) -> ClassesSet {
    names.sort_unstable();
    names.dedup();
    let mut known = SCRIPT_CLASSES.write().unwrap_or_else(|e| e.into_inner());
    let changed = *known != names;
    *known = names;
    ClassesSet { changed }
}

/// What [`set_script_classes`] did.
pub struct ClassesSet {
    /// The list differs from the last one: scripts read before may have other fields now.
    pub changed: bool,
}

/// The project's C# script classes as last set (for the Code Editor's completion).
pub fn script_classes() -> Vec<String> {
    SCRIPT_CLASSES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn is_script_class(name: &str) -> bool {
    SCRIPT_CLASSES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .binary_search_by(|n| n.as_str().cmp(name))
        .is_ok()
}

fn kind_of(type_name: &str) -> Option<Kind> {
    let t = type_name.rsplit('.').next().unwrap_or(type_name);
    if OBJECT_TYPES.contains(&t) || is_script_class(t) {
        return Some(Kind::Object(t.to_owned()));
    }
    Some(match t {
        "float" | "double" | "int" | "long" | "short" | "byte" | "uint" | "ushort" | "ulong"
        | "sbyte" | "Single" | "Double" | "Int32" | "Int64" | "Int16" | "Byte" | "UInt32" => {
            Kind::Number
        }
        "bool" | "Boolean" => Kind::Bool,
        "string" | "String" => Kind::Text,
        "Vector3" => Kind::Vector3,
        _ => return None,
    })
}

/// The value of initializer `init` for a field of `kind` (the type's zero when it isn't a
/// literal).
fn initial(kind: Kind, init: &[Tok]) -> ScriptValue {
    match kind {
        Kind::Number => ScriptValue::Number(
            number(init)
                .filter(|(_, used)| *used == init.len())
                .map_or(0.0, |(v, _)| v),
        ),
        Kind::Bool => ScriptValue::Bool(matches!(init, [Tok::Ident(s)] if s == "true")),
        Kind::Text => ScriptValue::Text(match init {
            [Tok::Str(s)] => s.clone(),
            _ => String::new(),
        }),
        Kind::Vector3 => ScriptValue::Vector3(vector(init).unwrap_or([0.0; 3])),
        // Always empty at first: set in Properties.
        Kind::Object(t) => ScriptValue::Object(crate::scene::ObjectValue::none(t)),
    }
}

/// `new Vector3(x, y[, z])`, `new(x, y, z)` or `Vector3.up` and friends.
fn vector(init: &[Tok]) -> Option<[f32; 3]> {
    let named = |name: &str| -> Option<[f32; 3]> {
        Some(match name {
            "zero" => [0.0, 0.0, 0.0],
            "one" => [1.0, 1.0, 1.0],
            "up" => [0.0, 1.0, 0.0],
            "down" => [0.0, -1.0, 0.0],
            "right" => [1.0, 0.0, 0.0],
            "left" => [-1.0, 0.0, 0.0],
            "forward" => [0.0, 0.0, 1.0],
            "back" => [0.0, 0.0, -1.0],
            _ => return None,
        })
    };
    match init {
        [Tok::Ident(v), Tok::Punct('.'), Tok::Ident(n)] if v == "Vector3" => named(n),
        [Tok::Ident(new), rest @ ..] if new == "new" => {
            let open = rest.iter().position(|t| is_punct(t, '('))?;
            if rest.last().is_some_and(|t| is_punct(t, ')')) {
                let args = numbers(&rest[open + 1..rest.len() - 1])?;
                match args.as_slice() {
                    [] => Some([0.0; 3]),
                    [x, y] => Some([*x as f32, *y as f32, 0.0]),
                    [x, y, z] => Some([*x as f32, *y as f32, *z as f32]),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

const MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "readonly",
    "const",
    "volatile",
    "new",
    "unsafe",
    "fixed",
    "extern",
    "override",
    "virtual",
    "abstract",
    "sealed",
    "partial",
    "required",
    "event",
];

/// Fields of one declaration statement (attributes, modifiers, type, declarators).
fn declaration(
    stmt: &[Tok],
    fields: &mut Vec<(String, ScriptValue)>,
    attrs: &mut HashMap<String, FieldAttributes>,
) {
    let mut i = 0;
    let mut found = Vec::new();
    while i < stmt.len() && is_punct(&stmt[i], '[') {
        let end = skip_group(stmt, i);
        found.extend(attributes(stmt, i, end));
        i = end;
    }
    let mut mods = Vec::new();
    while let Some(m) = stmt
        .get(i)
        .and_then(ident)
        .filter(|m| MODIFIERS.contains(m))
    {
        mods.push(m);
        i += 1;
    }
    let has = |name: &str| found.iter().any(|a| a.name == name);
    let serialized = (mods.contains(&"public") || has("SerializeField"))
        && !["static", "const", "readonly", "event"]
            .iter()
            .any(|m| mods.contains(m))
        && !has("NonSerialized");
    if !serialized {
        return;
    }
    // The type: a dotted name, then generic arguments, `[]` or `?` (which make it unsupported).
    let mut type_name = String::new();
    while let Some(part) = stmt.get(i).and_then(ident) {
        type_name.push_str(part);
        i += 1;
        if stmt.get(i).is_some_and(|t| is_punct(t, '.')) {
            type_name.push('.');
            i += 1;
        } else {
            break;
        }
    }
    if matches!(
        type_name.as_str(),
        "class" | "struct" | "enum" | "interface" | "delegate" | "record"
    ) {
        return;
    }
    let plain = stmt.get(i).is_some_and(|t| matches!(t, Tok::Ident(_)));
    let Some(kind) = kind_of(&type_name).filter(|_| plain) else {
        return;
    };
    // Declarators: `a = 1, b, c = 2`.
    let rest = &stmt[i..];
    let mut parts: Vec<&[Tok]> = Vec::new();
    let mut start = 0;
    let mut j = 0;
    while j < rest.len() {
        if is_punct(&rest[j], '(') || is_punct(&rest[j], '{') || is_punct(&rest[j], '[') {
            j = skip_group(rest, j);
            continue;
        }
        if is_punct(&rest[j], ',') {
            parts.push(&rest[start..j]);
            start = j + 1;
        }
        j += 1;
    }
    parts.push(&rest[start..]);
    for (n, part) in parts.into_iter().enumerate() {
        let Some(Tok::Ident(name)) = part.first() else {
            continue;
        };
        // A method (`public float Speed() => …`) or property got here only without a body.
        if part
            .iter()
            .any(|t| matches!(t, Tok::Arrow) || is_punct(t, '('))
            && part.get(1).is_some_and(|t| !is_punct(t, '='))
        {
            return;
        }
        let init = match part.get(1) {
            Some(Tok::Punct('=')) => &part[2..],
            None => &[][..],
            Some(_) => continue,
        };
        fields.push((name.clone(), initial(kind.clone(), init)));
        let mut a = FieldAttributes::default();
        for attr in &found {
            match attr.name.as_str() {
                "Header" if n == 0 => {
                    if let Some(Tok::Str(s)) = attr.args.first() {
                        a.header = Some(s.clone());
                    }
                }
                "Space" if n == 0 => {
                    a.space += numbers(&attr.args)
                        .and_then(|v| v.first().copied())
                        .unwrap_or(8.0) as f32;
                }
                "Tooltip" => {
                    if let Some(Tok::Str(s)) = attr.args.first() {
                        a.tooltip = Some(s.clone());
                    }
                }
                "Range" => {
                    if let Some([min, max]) = numbers(&attr.args).as_deref() {
                        a.range = Some((*min, *max));
                    }
                }
                "HideInInspector" => a.hidden = true,
                _ => {}
            }
        }
        if a != FieldAttributes::default() {
            attrs.insert(name.clone(), a);
        }
    }
}

/// The serialized fields of the C# script at `path` (its class is named after the file) and
/// their Inspector attributes, in declaration order.
pub fn inspect(source: &str, path: &str) -> ScriptInfo {
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let toks = tokenize(source);
    let mut info = ScriptInfo::default();
    let Some((start, end)) = class_body(&toks, &stem) else {
        return info;
    };
    for stmt in statements(&toks[start..end]) {
        declaration(stmt, &mut info.fields, &mut info.attributes);
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(src: &str) -> Vec<(String, ScriptValue)> {
        inspect(src, "Assets/Player.cs").fields
    }

    #[test]
    fn public_and_serialized_fields_with_literal_defaults() {
        let src = r#"
using Misty;
// public float commented = 3;
public class Player : MonoBehaviour
{
    public float speed = 90f;          // turn rate
    public int lives = 3, coins;
    [SerializeField] private bool godMode = true;
    public string label = "Hero \"One\"";
    public Vector3 offset = new Vector3(0, 1.5f, -2);
    public Vector3 up = Vector3.up;
    public double scale = -1.5e2;
    private float hidden = 1;
    public static float shared = 2;
    public const int Max = 9;
    public readonly int fixedValue = 1;
    [System.NonSerialized] public float runtime = 4;
    public float computed = 2 * 3;
    public float Speed => speed;
    public float Prop { get; set; } = 5;
    public void Jump() { var x = 1; }
    public GameObject target;
    public float[] list = { 1, 2 };
    public float after = 0x10;
}
"#;
        let f = fields(src);
        let names: Vec<&str> = f.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "speed", "lives", "coins", "godMode", "label", "offset", "up", "scale", "computed",
                "target", "after"
            ]
        );
        let get = |n: &str| f.iter().find(|(k, _)| k == n).unwrap().1.clone();
        assert_eq!(get("speed"), ScriptValue::Number(90.0));
        assert_eq!(get("lives"), ScriptValue::Number(3.0));
        assert_eq!(get("coins"), ScriptValue::Number(0.0));
        assert_eq!(get("godMode"), ScriptValue::Bool(true));
        assert_eq!(get("label"), ScriptValue::Text("Hero \"One\"".into()));
        assert_eq!(get("offset"), ScriptValue::Vector3([0.0, 1.5, -2.0]));
        assert_eq!(get("up"), ScriptValue::Vector3([0.0, 1.0, 0.0]));
        assert_eq!(get("scale"), ScriptValue::Number(-150.0));
        // Not a literal: the type's zero (the script's own value applies in Play).
        assert_eq!(get("computed"), ScriptValue::Number(0.0));
        assert_eq!(get("after"), ScriptValue::Number(16.0));
        // An object field starts empty.
        assert_eq!(
            get("target"),
            ScriptValue::Object(crate::scene::ObjectValue::none("GameObject"))
        );
    }

    #[test]
    fn attributes_map_to_the_inspector_ones() {
        let src = r#"
public class Player : MonoBehaviour
{
    [Header("Movement")]
    [Tooltip("Metres per second")]
    [Range(0, 20)]
    public float speed = 5;
    [Space(12), HideInInspector] public int secret;
    [Space] public bool flag;
}
"#;
        let info = inspect(src, "Player.cs");
        let speed = &info.attributes["speed"];
        assert_eq!(speed.header.as_deref(), Some("Movement"));
        assert_eq!(speed.tooltip.as_deref(), Some("Metres per second"));
        assert_eq!(speed.range, Some((0.0, 20.0)));
        let secret = &info.attributes["secret"];
        assert!(secret.hidden);
        assert_eq!(secret.space, 12.0);
        assert_eq!(info.attributes["flag"].space, 8.0);
    }

    #[test]
    fn the_class_named_like_the_file_wins() {
        let src = r#"
public class Helper { public float notMine = 1; }
namespace Game {
    public class Player : MonoBehaviour { public float mine = 2; }
}
"#;
        assert_eq!(fields(src), [("mine".to_owned(), ScriptValue::Number(2.0))]);
        // Named otherwise: the first MonoBehaviour.
        let other = inspect(src, "Assets/Renamed.cs").fields;
        assert_eq!(other, [("mine".to_owned(), ScriptValue::Number(2.0))]);
        assert!(inspect("not C# at all {{{", "X.cs").fields.is_empty());
    }

    #[test]
    fn strings_and_comments_do_not_confuse_the_reader() {
        let src = r#"
public class Player : MonoBehaviour
{
    /* public float notAField = 1; */
    public string path = @"C:\Games\{x}";
    public string text = $"{1 + 2} }} {{ {(true ? "a" : "b")}"; public float next = 1;
    #if UNITY_EDITOR
    public float editorOnly = 3;
    #endif
    void Update() { string s = "public float fake = 1;"; }
}
"#;
        let f = fields(src);
        let names: Vec<&str> = f.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["path", "text", "next", "editorOnly"]);
        assert_eq!(f[0].1, ScriptValue::Text(r"C:\Games\{x}".into()));
    }
}
