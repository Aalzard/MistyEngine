//! Autocomplete: what is being typed, candidate names (keywords, names in the file, the Misty
//! API, component names) and their ranking.

use std::collections::BTreeSet;

use regex::Regex;

use super::api;
use super::lexer::{self, KEYWORDS, LineState, TokenKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Method,
    Property,
    /// Name found in this file.
    Local,
    Global,
    Keyword,
    Component,
}

impl ItemKind {
    pub fn icon(self) -> &'static str {
        match self {
            ItemKind::Method => "ƒ",
            ItemKind::Property => "◆",
            ItemKind::Local => "▫",
            ItemKind::Global => "▣",
            ItemKind::Keyword => "⌨",
            ItemKind::Component => "▤",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: ItemKind,
    pub signature: String,
    pub doc: String,
}

/// What the caret is completing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A plain name.
    Name,
    /// A member after `expr.` / `expr:` (`colon` = method call).
    Member { expr: String, colon: bool },
    /// The string inside `GetComponent("…`.
    Component,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub prefix: String,
    /// Column where the prefix starts (replaced on accept).
    pub start: usize,
    pub target: Target,
}

/// Completion context from the text before the caret, or `None` in comments and strings.
pub fn context(before: &str) -> Option<Context> {
    let chars: Vec<char> = before.chars().collect();
    let mut start = chars.len();
    while start > 0 && super::text::is_word(chars[start - 1]) {
        start -= 1;
    }
    let prefix: String = chars[start..].iter().collect();
    let head: String = chars[..start].iter().collect();
    if head.ends_with("GetComponent(\"") || head.ends_with("GetComponent('") {
        return Some(Context {
            prefix,
            start,
            target: Target::Component,
        });
    }
    // Inside a comment or an unfinished string: no completion.
    let (tokens, state) = lexer::tokenize_line(before, LineState::Normal);
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
    let target = match chars[..start].last() {
        Some(&sep @ ('.' | ':')) if !(sep == '.' && start >= 2 && chars[start - 2] == '.') => {
            let mut s = start - 1;
            while s > 0 && (super::text::is_word(chars[s - 1]) || chars[s - 1] == '.') {
                s -= 1;
            }
            let expr: String = chars[s..start - 1].iter().collect();
            if expr.is_empty() {
                return None;
            }
            Target::Member {
                expr,
                colon: sep == ':',
            }
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
pub fn complete(ctx: &Context, lines: &[String], caret_line: usize) -> Vec<Item> {
    let items = match &ctx.target {
        Target::Component => api::COMPONENTS
            .iter()
            .map(|c| Item {
                label: c.to_string(),
                kind: ItemKind::Component,
                signature: format!("GetComponent(\"{c}\")"),
                doc: String::new(),
            })
            .collect(),
        Target::Name => names(lines, caret_line, &ctx.prefix),
        Target::Member { expr, colon } => members(expr, *colon, lines),
    };
    rank(items, &ctx.prefix)
}

fn entry_item(e: &api::Entry) -> Item {
    Item {
        label: e.name.to_owned(),
        kind: if e.is_function() {
            ItemKind::Method
        } else if e.owner.is_empty() {
            ItemKind::Global
        } else {
            ItemKind::Property
        },
        signature: e.signature.to_owned(),
        doc: e.doc.to_owned(),
    }
}

/// Keywords, globals and names used in the file.
fn names(lines: &[String], caret_line: usize, prefix: &str) -> Vec<Item> {
    let mut out: Vec<Item> = KEYWORDS
        .iter()
        .chain(&["true", "false", "nil", "self"])
        .map(|k| Item {
            label: k.to_string(),
            kind: ItemKind::Keyword,
            signature: String::new(),
            doc: String::new(),
        })
        .collect();
    out.extend(api::globals().map(entry_item));
    out.extend(
        lexer::BUILTINS
            .iter()
            .filter(|b| **b != "print")
            .map(|b| Item {
                label: b.to_string(),
                kind: ItemKind::Global,
                signature: String::new(),
                doc: "Lua standard library".into(),
            }),
    );
    let known: BTreeSet<String> = out.iter().map(|i| i.label.clone()).collect();
    let mut locals = BTreeSet::new();
    for (l, ts) in lexer::tokenize(lines).iter().enumerate() {
        let chars: Vec<char> = lines[l].chars().collect();
        for t in ts {
            if matches!(t.kind, TokenKind::Identifier | TokenKind::Function) {
                let w: String = chars[t.start..t.end].iter().collect();
                // The word being typed isn't a suggestion for itself.
                if l == caret_line && w == prefix {
                    continue;
                }
                if !known.contains(&w) {
                    locals.insert(w);
                }
            }
        }
    }
    out.extend(locals.into_iter().map(|w| Item {
        label: w,
        kind: ItemKind::Local,
        signature: String::new(),
        doc: String::new(),
    }));
    out
}

/// Members of `expr`: an API table, `self`, a Transform / GameObject, a variable assigned from
/// `GetComponent("Type")`, or a table declared in this file.
fn members(expr: &str, colon: bool, lines: &[String]) -> Vec<Item> {
    let last = expr.rsplit('.').next().unwrap_or(expr);
    let owner = if api::is_global(expr) {
        Some(expr.to_owned())
    } else if expr == "self" {
        None
    } else if last == "transform" || last == "parent" {
        Some("#Transform".into())
    } else if last == "gameObject" {
        Some("#GameObject".into())
    } else {
        infer_type(last, lines)
    };
    let mut out: Vec<Item> = Vec::new();
    match owner {
        Some(o) => out.extend(api::members(&o).map(entry_item)),
        None if expr == "self" => {
            out.extend(api::members("#self").map(entry_item));
            out.extend(
                api::members("#GameObject")
                    .filter(|e| e.is_function())
                    .map(entry_item),
            );
            out.extend(class_members(lines));
        }
        None => {
            let own = table_members(last, lines);
            if own.is_empty() {
                // Unknown type: every object member, once each.
                let mut seen = BTreeSet::new();
                for e in api::entries().iter().filter(|e| e.owner.starts_with('#')) {
                    if seen.insert(e.name) {
                        out.push(entry_item(e));
                    }
                }
            }
            out.extend(own);
        }
    }
    if colon {
        out.retain(|i| i.kind == ItemKind::Method);
    }
    out
}

/// Type of a variable from its assignment in the file (`x = …GetComponent("Rigidbody")`).
fn infer_type(var: &str, lines: &[String]) -> Option<String> {
    let v = regex::escape(var);
    let get = Regex::new(&format!(r#"\b{v}\s*=\s*.*GetComponent\(\s*["'](\w+)["']"#)).ok()?;
    let ray = Regex::new(&format!(r"\b{v}\s*=\s*Physics\.(Raycast|SphereCast)\b")).ok()?;
    let vec = Regex::new(&format!(r"\b{v}\s*=\s*(Vector3\.|.*\.position\b)")).ok()?;
    for line in lines {
        if let Some(c) = get.captures(line) {
            return api::component_type(&c[1]);
        }
        if ray.is_match(line) {
            return Some("#RaycastHit".into());
        }
        if vec.is_match(line) {
            return Some("#Vector3".into());
        }
    }
    None
}

/// Fields and methods of the script's class (`self.`).
fn class_members(lines: &[String]) -> Vec<Item> {
    let class = lines
        .iter()
        .rev()
        .find_map(|l| {
            l.trim()
                .strip_prefix("return ")
                .map(|r| r.trim().to_owned())
        })
        .unwrap_or_default();
    let mut out = table_members(&class, lines);
    let assigned = Regex::new(r"\bself\.(\w+)\s*=[^=]").expect("valid regex");
    let known: BTreeSet<String> = out.iter().map(|i| i.label.clone()).collect();
    let mut extra = BTreeSet::new();
    for l in lines {
        for c in assigned.captures_iter(l) {
            if !known.contains(&c[1]) {
                extra.insert(c[1].to_owned());
            }
        }
    }
    out.extend(extra.into_iter().map(|f| Item {
        label: f,
        kind: ItemKind::Property,
        signature: String::new(),
        doc: "Set in this script".into(),
    }));
    out
}

/// Fields of `local Name = { a = 1, … }` and methods of `function Name:f()` / `Name.f`.
fn table_members(name: &str, lines: &[String]) -> Vec<Item> {
    if name.is_empty() || !name.chars().all(super::text::is_word) {
        return Vec::new();
    }
    let n = regex::escape(name);
    let open = Regex::new(&format!(r"^\s*(local\s+)?{n}\s*=\s*\{{")).expect("valid regex");
    let field = Regex::new(r"^\s*(\w+)\s*=").expect("valid regex");
    let method =
        Regex::new(&format!(r"^\s*function\s+{n}[.:](\w+)\s*(\([^)]*\))")).expect("valid regex");
    let mut out = Vec::new();
    let mut in_table = false;
    for l in lines {
        if open.is_match(l) {
            in_table = !l.contains('}');
            continue;
        }
        if in_table {
            if l.trim_start().starts_with('}') {
                in_table = false;
            } else if let Some(c) = field.captures(l) {
                out.push(Item {
                    label: c[1].to_owned(),
                    kind: ItemKind::Property,
                    signature: l.trim().trim_end_matches(',').to_owned(),
                    doc: "Field of this script".into(),
                });
            }
        }
        if let Some(c) = method.captures(l) {
            out.push(Item {
                label: c[1].to_owned(),
                kind: ItemKind::Method,
                signature: format!("{}{}", &c[1], &c[2]),
                doc: "Defined in this script".into(),
            });
        }
    }
    out
}

/// How well `label` matches `prefix` (lower is better), or `None`.
pub fn score(label: &str, prefix: &str) -> Option<u8> {
    if prefix.is_empty() {
        return Some(0);
    }
    if label == prefix {
        return Some(0);
    }
    if label.starts_with(prefix) {
        return Some(1);
    }
    let (l, p) = (label.to_lowercase(), prefix.to_lowercase());
    if l.starts_with(&p) {
        return Some(2);
    }
    if camel_match(label, prefix) {
        return Some(3);
    }
    l.contains(&p).then_some(4)
}

/// `gkd` matches `GetKeyDown`: each prefix letter starts a word part, in order.
fn camel_match(label: &str, prefix: &str) -> bool {
    let humps: Vec<char> = label
        .char_indices()
        .filter(|&(i, c)| i == 0 || c.is_uppercase() || label[..i].ends_with('_'))
        .map(|(_, c)| c.to_ascii_lowercase())
        .collect();
    let p: Vec<char> = prefix.chars().map(|c| c.to_ascii_lowercase()).collect();
    p.len() > 1 && humps.starts_with(&p)
}

/// Keep matching items, best first: match quality, then API before keywords before file names,
/// shorter, alphabetical. Duplicate labels are dropped.
pub fn rank(items: Vec<Item>, prefix: &str) -> Vec<Item> {
    let mut scored: Vec<(u8, Item)> = items
        .into_iter()
        .filter_map(|i| score(&i.label, prefix).map(|s| (s, i)))
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| {
        sa.cmp(sb)
            .then(a.kind.cmp(&b.kind))
            .then(a.label.len().cmp(&b.label.len()))
            .then(a.label.cmp(&b.label))
    });
    let mut seen = BTreeSet::new();
    scored
        .into_iter()
        .map(|(_, i)| i)
        .filter(|i| seen.insert(i.label.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_owned).collect()
    }

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn context_detects_members_components_and_strings() {
        let c = context("    if Input.GetK").unwrap();
        assert_eq!(c.prefix, "GetK");
        assert_eq!(c.start, 13);
        assert_eq!(
            c.target,
            Target::Member {
                expr: "Input".into(),
                colon: false
            }
        );
        let c = context("self.transform:Ro").unwrap();
        assert_eq!(
            c.target,
            Target::Member {
                expr: "self.transform".into(),
                colon: true
            }
        );
        let c = context("local rb = self.gameObject:GetComponent(\"Rig").unwrap();
        assert_eq!((c.prefix.as_str(), &c.target), ("Rig", &Target::Component));
        assert!(context("-- Input.Get").is_none());
        assert!(context("print(\"hello wor").is_none());
        assert!(context("x = 1.5").is_none());
        assert_eq!(context("a .. b").unwrap().target, Target::Name);
    }

    #[test]
    fn ranking_prefers_prefix_then_case_then_humps() {
        let items = ["GetKeyDown", "GetKey", "getkey2", "ForceGetKey", "Other"]
            .iter()
            .map(|l| Item {
                label: l.to_string(),
                kind: ItemKind::Method,
                signature: String::new(),
                doc: String::new(),
            })
            .collect();
        let ranked = rank(items, "GetKey");
        assert_eq!(
            labels(&ranked),
            vec!["GetKey", "GetKeyDown", "getkey2", "ForceGetKey"]
        );
        assert_eq!(score("GetKeyDown", "gkd"), Some(3));
        assert_eq!(score("GetKeyDown", "xyz"), None);
    }

    #[test]
    fn api_members_and_inferred_component_types() {
        let src = lines(
            "local Hero = {\n    speed = 4,\n}\nfunction Hero:Start()\n    self._rb = self.gameObject:GetComponent(\"Rigidbody\")\n    local hit = Physics.Raycast(a, b)\nend\nfunction Hero:Jump() end\nreturn Hero",
        );
        let ctx = |s: &str| context(s).unwrap();
        let input = complete(&ctx("Input.GetKeyD"), &src, 0);
        assert_eq!(input[0].label, "GetKeyDown");
        assert!(input[0].signature.starts_with("GetKeyDown("));
        let rb = complete(&ctx("self._rb:Add"), &src, 0);
        assert_eq!(labels(&rb), vec!["AddForce", "AddTorque"]);
        let hit = complete(&ctx("hit.poi"), &src, 0);
        assert_eq!(labels(&hit), vec!["point"]);
        let me = complete(&ctx("self."), &src, 0);
        for want in ["speed", "transform", "gameObject", "_rb", "Jump", "Start"] {
            assert!(labels(&me).contains(&want), "{want} in {:?}", labels(&me));
        }
        let calls = complete(&ctx("self:"), &src, 0);
        assert!(labels(&calls).contains(&"Jump") && !labels(&calls).contains(&"speed"));
        let comp = complete(&ctx("GetComponent(\"Anim"), &src, 0);
        assert_eq!(
            labels(&comp),
            vec!["Animator", "Animation", "SplineAnimate"]
        );
    }

    #[test]
    fn names_include_keywords_globals_and_locals() {
        let src = lines("local counter = 0\nlocal function bump() counter = counter + 1 end\ncou");
        let items = complete(&context("cou").unwrap(), &src, 2);
        assert_eq!(labels(&items), vec!["counter"]);
        let items = complete(&context("fu").unwrap(), &src, 2);
        assert_eq!(items[0].label, "function");
        let items = complete(&context("Vec").unwrap(), &src, 2);
        assert_eq!(items[0].label, "Vector3");
        assert_eq!(items[0].kind, ItemKind::Global);
    }
}
