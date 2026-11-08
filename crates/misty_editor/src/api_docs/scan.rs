//! Reads the Lua binding definitions in the engine's `scripting` sources: what each file
//! registers (global tables' functions and values, userdata fields and methods), the closure
//! parameters for a signature, and the comment written above each registration.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

/// What kind of member a binding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Function,
    Property,
}

/// One binding found in the sources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// `""` = a global, `Input` = a global table, `#Transform` = an object type.
    pub owner: String,
    pub name: String,
    pub kind: Kind,
    /// Parameter list from the Rust closure (`None` when it isn't visible there).
    pub params: Option<Vec<String>>,
    /// The comment lines right above the registration.
    pub doc: String,
    /// Source file name (`input_api.rs`; no line number, so unrelated edits to a binding file
    /// don't make the committed reference stale).
    pub source: String,
    /// A property with a setter.
    pub settable: bool,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// The Lua name of a userdata type (`LVec3` → `Vector3`, `LTransform` → `Transform`).
pub fn lua_type(rust: &str) -> String {
    match rust {
        "LVec3" => "Vector3".into(),
        "LMixer" => "AudioMixer".into(),
        "LMixerGroup" => "AudioMixerGroup".into(),
        "LSnapshot" => "AudioMixerSnapshot".into(),
        "LDirector" => "PlayableDirector".into(),
        "LQuality" => "QualitySettings".into(),
        other => other
            .strip_prefix('L')
            .filter(|s| s.starts_with(|c: char| c.is_ascii_uppercase()))
            .unwrap_or(other)
            .to_owned(),
    }
}

/// Every binding registered in one source file.
pub fn scan_file(file: &str, text: &str) -> Vec<Found> {
    let lines: Vec<&str> = text.lines().collect();
    // Line starts, to turn byte offsets into line numbers.
    let mut starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    let line_of = |offset: usize| starts.partition_point(|s| *s <= offset) - 1;
    let globals_owner = global_userdata(text);
    let mut out = Vec::new();

    // Userdata blocks: `impl UserData for LType { … }` up to the closing brace at column 0.
    static IMPL: OnceLock<Regex> = OnceLock::new();
    static MEMBER: OnceLock<Regex> = OnceLock::new();
    let impl_re = re(&IMPL, r"(?m)^impl (?:mlua::)?UserData for (\w+)\s*\{");
    let member_re = re(
        &MEMBER,
        r#"\.add_(field_method_get|field_method_set|field_function_get|field_function_set|method|method_mut|function|function_mut|async_method|async_function)\(\s*"([A-Za-z_]\w*)""#,
    );
    for cap in impl_re.captures_iter(text) {
        let ty = &cap[1];
        let start = cap.get(0).map_or(0, |m| m.end());
        let end = text[start..].find("\n}").map_or(text.len(), |i| start + i);
        let owner = globals_owner
            .get(ty)
            .cloned()
            .unwrap_or_else(|| format!("#{}", lua_type(ty)));
        for m in member_re.captures_iter(&text[start..end]) {
            let what = &m[1];
            let name = m[2].to_owned();
            let at = start + m.get(0).map_or(0, |x| x.start());
            let after = start + m.get(0).map_or(0, |x| x.end());
            let line = line_of(at);
            let is_fn = matches!(
                what,
                "method"
                    | "method_mut"
                    | "function"
                    | "function_mut"
                    | "async_method"
                    | "async_function"
            );
            let skip = if what.starts_with("method") || what == "async_method" {
                2
            } else {
                1
            };
            let params = is_fn
                .then(|| closure_params(&text[after..end.min(after + 400)], skip))
                .flatten();
            let setter = what.ends_with("_set");
            let doc = comment_above(&lines, line);
            if let Some(prev) = out
                .iter_mut()
                .find(|f: &&mut Found| f.owner == owner && f.name == name)
            {
                // A getter and a setter: one property.
                prev.settable |= setter;
                if prev.doc.is_empty() {
                    prev.doc = doc;
                }
                continue;
            }
            out.push(Found {
                owner: owner.clone(),
                name,
                kind: if is_fn {
                    Kind::Function
                } else {
                    Kind::Property
                },
                params,
                doc,
                source: file.to_owned(),
                settable: setter,
            });
        }
    }

    // Global tables: `let t = g.get("Input")` or `g.set("Input", t)` name the variable, then
    // `t.set("Name", …)` registers a member. Variables are matched per top-level function.
    static FN_START: OnceLock<Regex> = OnceLock::new();
    let fn_re = re(&FN_START, r"(?m)^(?:pub(?:\([a-z]+\))? )?fn ");
    let mut cuts: Vec<usize> = fn_re.find_iter(text).map(|m| m.start()).collect();
    cuts.push(text.len());
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        scan_tables(file, &text[a..b], a, &lines, &line_of, &mut out);
    }
    out
}

/// `g.set("Screen", LScreen)`: userdata types installed as global objects, by Rust type name.
fn global_userdata(text: &str) -> HashMap<String, String> {
    static SET: OnceLock<Regex> = OnceLock::new();
    let set_re = re(
        &SET,
        r#"(?:\bg|\bglobals|lua\.globals\(\))\.set\(\s*"(\w+)",\s*(L[A-Z]\w*)\s*\)"#,
    );
    set_re
        .captures_iter(text)
        .map(|c| (c[2].to_owned(), c[1].to_owned()))
        .collect()
}

fn scan_tables(
    file: &str,
    seg: &str,
    base: usize,
    lines: &[&str],
    line_of: &dyn Fn(usize) -> usize,
    out: &mut Vec<Found>,
) {
    static GET: OnceLock<Regex> = OnceLock::new();
    static SETVAR: OnceLock<Regex> = OnceLock::new();
    static MEMBER: OnceLock<Regex> = OnceLock::new();
    static GLOBALFN: OnceLock<Regex> = OnceLock::new();
    let get_re = re(
        &GET,
        r#"let\s+(?:mut\s+)?(\w+)\s*(?::\s*Table)?\s*=\s*(?:\bg|\bglobals|lua\.globals\(\))\s*\.get(?:::<Table>)?\(\s*"(\w+)""#,
    );
    let setvar_re = re(
        &SETVAR,
        r#"(?:\bg|\bglobals|lua\.globals\(\))\.set\(\s*"(\w+)",\s*([a-z_]\w*)\s*\)"#,
    );
    let member_re = re(&MEMBER, r#"\b([a-z_]\w*)\.set\(\s*"([A-Za-z_]\w*)",\s*"#);
    let globalfn_re = re(
        &GLOBALFN,
        r#"(?:\bg|\bglobals|lua\.globals\(\))\.set\(\s*"([A-Za-z_]\w*)",\s*lua\.create_(?:async_)?function"#,
    );
    let mut vars: HashMap<&str, String> = HashMap::new();
    for c in get_re.captures_iter(seg) {
        vars.insert(c.get(1).map_or("", |m| m.as_str()), c[2].to_owned());
    }
    for c in setvar_re.captures_iter(seg) {
        vars.insert(c.get(2).map_or("", |m| m.as_str()), c[1].to_owned());
    }
    let mut push = |owner: String, name: String, at: usize, after: usize| {
        let rest = &seg[after..seg.len().min(after + 400)];
        let (kind, params) = value_kind(rest);
        let line = line_of(base + at);
        if out.iter().any(|f| f.owner == owner && f.name == name) {
            return;
        }
        out.push(Found {
            owner,
            name,
            kind,
            params,
            doc: comment_above(lines, line),
            source: file.to_owned(),
            settable: false,
        });
    };
    for c in member_re.captures_iter(seg) {
        let var = c.get(1).map_or("", |m| m.as_str());
        let Some(owner) = vars.get(var) else { continue };
        let m = c.get(0).expect("match");
        push(owner.clone(), c[2].to_owned(), m.start(), m.end());
    }
    for c in globalfn_re.captures_iter(seg) {
        let m = c.get(0).expect("match");
        let name_end = c.get(1).map_or(m.end(), |n| n.end());
        // Skip `"name",` to the value.
        let after = seg[name_end..]
            .find(',')
            .map_or(m.end(), |i| name_end + i + 1);
        push(String::new(), c[1].to_owned(), m.start(), after);
    }
}

/// What a `t.set("Name", <value>)` value is: a function (with its closure's parameters when
/// visible) or a plain value.
fn value_kind(rest: &str) -> (Kind, Option<Vec<String>>) {
    let v = rest.trim_start();
    if v.starts_with("lua.create_function") || v.starts_with("lua.create_async_function") {
        let open = v.find('(').map_or(0, |i| i + 1);
        return (Kind::Function, closure_params(&v[open..], 1));
    }
    // `button(lua, …)?`: a helper returning a function.
    static HELPER: OnceLock<Regex> = OnceLock::new();
    let helper = re(&HELPER, r"^[a-z_]\w*(?:::\w+)*\(\s*lua\b");
    if helper.is_match(v) || v.starts_with("f.clone()") {
        return (Kind::Function, None);
    }
    (Kind::Property, None)
}

/// The Lua parameters of the closure that starts `text` (`|lua, (a, b): (T, U)|`), skipping the
/// first `skip` (the Lua state, and `self` for methods).
pub fn closure_params(text: &str, skip: usize) -> Option<Vec<String>> {
    // After a member name the text starts at the comma before the closure.
    let t = text.trim_start();
    let t = t.strip_prefix(',').map_or(t, str::trim_start);
    let t = t.strip_prefix("move").map_or(t, str::trim_start);
    let t = t.strip_prefix('|')?;
    let end = t.find('|')?;
    let parts = split_top(&t[..end]);
    let mut out = Vec::new();
    for p in parts.into_iter().skip(skip) {
        let p = p.trim().trim_start_matches("mut ").trim();
        if p.is_empty() || p == "()" || p == "_" {
            continue;
        }
        let (pat, ty) = match p.split_once(':') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (p, ""),
        };
        if pat.starts_with('(') {
            let names = split_top(pat.trim_start_matches('(').trim_end_matches(')'));
            let types = split_top(ty.trim_start_matches('(').trim_end_matches(')'));
            for (i, n) in names.iter().enumerate() {
                if let Some(s) = param(n.trim(), types.get(i).map_or("", |t| t.trim())) {
                    out.push(s);
                }
            }
        } else if let Some(s) = param(pat, ty) {
            out.push(s);
        }
    }
    Some(out)
}

fn param(name: &str, ty: &str) -> Option<String> {
    if ty == "()" || name.is_empty() {
        return None;
    }
    if ty.starts_with("Variadic") || ty.ends_with("MultiValue") {
        return Some("...".into());
    }
    let name = name.trim_start_matches('_');
    let name = if name.is_empty() { "value" } else { name };
    Some(if ty.starts_with("Option<") {
        format!("[{name}]")
    } else {
        name.to_owned()
    })
}

/// Split at commas outside brackets.
fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// The `//` or `///` comment lines directly above line `at` (attributes skipped), as one text.
fn comment_above(lines: &[&str], at: usize) -> String {
    let mut doc: Vec<&str> = Vec::new();
    let mut i = at;
    while i > 0 {
        i -= 1;
        let l = lines[i].trim();
        if l.starts_with("#[") {
            continue;
        }
        let Some(c) = l.strip_prefix("///").or_else(|| l.strip_prefix("//")) else {
            break;
        };
        if c.starts_with('!') || c.trim_start().starts_with("----") {
            break;
        }
        doc.push(c.trim());
    }
    doc.reverse();
    doc.join(" ").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
struct LScreen;

impl UserData for LScreen {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("width", |_, _| Ok(1));
        // True when the window covers the screen.
        f.add_field_method_get("fullScreen", |_, _| Ok(true));
        f.add_field_method_set("fullScreen", |_, _, on: bool| Ok(()));
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        /// Change the window size.
        m.add_function(
            "SetResolution",
            |_, (w, h, mode): (u32, u32, Option<Value>)| Ok(()),
        );
    }
}

impl UserData for LTransform {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("GetChild", |lua, t, i: usize| Ok(()));
        m.add_method("Translate", |lua, t, args: Variadic<Value>| Ok(()));
    }
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    g.set("Screen", LScreen)?;
    let p = lua.create_table()?;
    // Read an integer preference.
    p.set(
        "GetInt",
        lua.create_function(|_, (k, d): (String, Option<i64>)| Ok(0))?,
    )?;
    p.set("Version", 3)?;
    g.set("PlayerPrefs", p)?;
    g.set(
        "print",
        lua.create_function(|lua, a: Variadic<Value>| Ok(()))?,
    )?;
    Ok(())
}

fn more(lua: &Lua) -> Result<()> {
    let input: Table = lua.globals().get("Input")?;
    input.set("GetButton", button(lua, Input::held)?)?;
    Ok(())
}
"#;

    #[test]
    fn finds_userdata_members_tables_globals_and_comments() {
        let found = scan_file("x_api.rs", SAMPLE);
        let get = |o: &str, n: &str| {
            found
                .iter()
                .find(|f| f.owner == o && f.name == n)
                .unwrap_or_else(|| panic!("{o}.{n} in {found:#?}"))
        };
        let full = get("Screen", "fullScreen");
        assert_eq!(full.kind, Kind::Property);
        assert!(full.settable);
        assert_eq!(full.doc, "True when the window covers the screen.");
        let res = get("Screen", "SetResolution");
        assert_eq!(
            res.params.as_deref(),
            Some(&["w".to_owned(), "h".into(), "[mode]".into()][..])
        );
        assert_eq!(res.doc, "Change the window size.");
        assert_eq!(
            get("#Transform", "GetChild").params.as_deref(),
            Some(&["i".to_owned()][..])
        );
        assert_eq!(
            get("#Transform", "Translate").params.as_deref(),
            Some(&["...".to_owned()][..])
        );
        let gi = get("PlayerPrefs", "GetInt");
        assert_eq!(gi.kind, Kind::Function);
        assert_eq!(
            gi.params.as_deref(),
            Some(&["k".to_owned(), "[d]".into()][..])
        );
        assert_eq!(gi.doc, "Read an integer preference.");
        assert_eq!(gi.source, "x_api.rs");
        assert_eq!(get("PlayerPrefs", "Version").kind, Kind::Property);
        assert_eq!(
            get("", "print").params.as_deref(),
            Some(&["...".to_owned()][..])
        );
        let button = get("Input", "GetButton");
        assert_eq!((button.kind, button.params.clone()), (Kind::Function, None));
    }

    #[test]
    fn lua_type_names() {
        assert_eq!(lua_type("LVec3"), "Vector3");
        assert_eq!(lua_type("LRigidbody2D"), "Rigidbody2D");
        assert_eq!(lua_type("Plain"), "Plain");
    }
}
