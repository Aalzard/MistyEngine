//! Semantic versions (`1.4.2`, `2.0.0-preview.3`) and version ranges as written in
//! `package.json` dependencies and `Packages/manifest.json`:
//!
//! | range            | means                                   |
//! |------------------|-----------------------------------------|
//! | `1.2.3`          | exactly 1.2.3 (`=1.2.3` too)            |
//! | `^1.2.3`         | `>=1.2.3 <2.0.0` (`^0.2.3`: `<0.3.0`)   |
//! | `~1.2.3`         | `>=1.2.3 <1.3.0`                        |
//! | `1.x`, `1.2.*`   | any 1.x / 1.2.x                         |
//! | `>=1.0 <2`       | both comparators (space = and)          |
//! | `1.x \|\| 3.x`   | either side                             |
//! | `*`, empty       | any version                             |
//!
//! Pre-release versions only match ranges that name a pre-release of the same x.y.z, like npm.

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Dot-separated pre-release identifiers ("preview.3" → ["preview", "3"]).
    pub pre: Vec<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
        }
    }

    /// `1.2.3`, `v1.2.3`, `1.2.3-beta.1+build` (build metadata is ignored).
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim().trim_start_matches(['v', '=']);
        let t = t.split('+').next().unwrap_or(t);
        let (core, pre) = match t.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (t, None),
        };
        let mut nums = core.split('.');
        let mut num = |what: &str| -> Result<u64, String> {
            nums.next()
                .ok_or_else(|| format!("'{text}': missing {what} number"))?
                .parse()
                .map_err(|_| format!("'{text}' is not a version (like 1.2.3)"))
        };
        let v = Self {
            major: num("major")?,
            minor: num("minor")?,
            patch: num("patch")?,
            pre: pre
                .map(|p| p.split('.').map(str::to_owned).collect())
                .unwrap_or_default(),
        };
        if nums.next().is_some() {
            return Err(format!("'{text}' has too many numbers"));
        }
        Ok(v)
    }

    fn core(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core().cmp(&other.core()).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release is newer than its pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Comparator {
    op: Op,
    version: Version,
}

impl Comparator {
    fn matches(&self, v: &Version) -> bool {
        let o = v.cmp(&self.version);
        match self.op {
            Op::Eq => o == Ordering::Equal,
            Op::Gt => o == Ordering::Greater,
            Op::Ge => o != Ordering::Less,
            Op::Lt => o == Ordering::Less,
            Op::Le => o != Ordering::Greater,
        }
    }
}

/// A parsed version range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    text: String,
    /// Alternatives (`||`), each a list of comparators that must all hold.
    sets: Vec<Vec<Comparator>>,
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.text.is_empty() {
            "*"
        } else {
            &self.text
        })
    }
}

/// `1`, `1.2`, `1.x`, `1.2.*` → the numbers given (wildcards end the list).
fn partial(text: &str) -> Result<(Vec<u64>, Vec<String>), String> {
    let t = text.trim_start_matches('v');
    let (core, pre) = match t.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
        None => (t, Vec::new()),
    };
    let mut nums = Vec::new();
    for part in core.split('.') {
        if matches!(part, "x" | "X" | "*") {
            break;
        }
        nums.push(
            part.parse()
                .map_err(|_| format!("'{text}' is not a version range"))?,
        );
    }
    if nums.len() > 3 {
        return Err(format!("'{text}' has too many numbers"));
    }
    Ok((nums, pre))
}

fn ver(n: &[u64], pre: Vec<String>) -> Version {
    Version {
        major: n.first().copied().unwrap_or(0),
        minor: n.get(1).copied().unwrap_or(0),
        patch: n.get(2).copied().unwrap_or(0),
        pre,
    }
}

/// The exclusive upper bound after bumping position `i` of `n`.
fn bump(n: &[u64], i: usize) -> Version {
    let mut v = [
        n.first().copied().unwrap_or(0),
        n.get(1).copied().unwrap_or(0),
        n.get(2).copied().unwrap_or(0),
    ];
    v[i] += 1;
    for x in v.iter_mut().skip(i + 1) {
        *x = 0;
    }
    // `<2.0.0-0`: below every 2.0.0 pre-release too.
    Version {
        major: v[0],
        minor: v[1],
        patch: v[2],
        pre: vec!["0".into()],
    }
}

fn term(t: &str) -> Result<Vec<Comparator>, String> {
    let c = |op, version| Comparator { op, version };
    let (op, rest) = if let Some(r) = t.strip_prefix(">=") {
        ("ge", r)
    } else if let Some(r) = t.strip_prefix("<=") {
        ("le", r)
    } else if let Some(r) = t.strip_prefix('>') {
        ("gt", r)
    } else if let Some(r) = t.strip_prefix('<') {
        ("lt", r)
    } else if let Some(r) = t.strip_prefix('^') {
        ("caret", r)
    } else if let Some(r) = t.strip_prefix('~') {
        ("tilde", r.trim_start_matches('>'))
    } else if let Some(r) = t.strip_prefix('=') {
        ("eq", r)
    } else {
        ("eq", t)
    };
    let rest = rest.trim();
    if rest.is_empty() || matches!(rest, "*" | "x" | "X") {
        return Ok(Vec::new());
    }
    let (n, pre) = partial(rest)?;
    if n.is_empty() {
        return Ok(Vec::new());
    }
    let lo = ver(&n, pre);
    Ok(match op {
        "ge" => vec![c(Op::Ge, lo)],
        "le" if n.len() < 3 => vec![c(Op::Lt, bump(&n, n.len() - 1))],
        "le" => vec![c(Op::Le, lo)],
        "gt" if n.len() < 3 => vec![c(Op::Ge, bump(&n, n.len() - 1))],
        "gt" => vec![c(Op::Gt, lo)],
        "lt" => vec![c(Op::Lt, lo)],
        "caret" => {
            // First non-zero number is the one that may not change.
            let i = n
                .iter()
                .position(|x| *x != 0)
                .unwrap_or(n.len() - 1)
                .min(n.len() - 1);
            vec![c(Op::Ge, lo), c(Op::Lt, bump(&n, i))]
        }
        "tilde" => {
            let i = if n.len() >= 2 { 1 } else { 0 };
            vec![c(Op::Ge, lo), c(Op::Lt, bump(&n, i))]
        }
        // `1.2` = `1.2.x`; `1.2.3` exactly.
        _ if n.len() < 3 => vec![c(Op::Ge, lo), c(Op::Lt, bump(&n, n.len() - 1))],
        _ => vec![c(Op::Eq, lo)],
    })
}

impl Range {
    pub fn any() -> Self {
        Self {
            text: "*".into(),
            sets: vec![Vec::new()],
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let mut sets = Vec::new();
        for alt in text.split("||") {
            let mut set = Vec::new();
            // Hyphen ranges: `1.0.0 - 2.0.0`.
            let words: Vec<&str> = alt.split_whitespace().collect();
            if let [a, "-", b] = words.as_slice() {
                set.extend(term(&format!(">={a}"))?);
                set.extend(term(&format!("<={b}"))?);
            } else {
                // `>= 1.0` with a space: glue operators to their version.
                let mut glued: Vec<String> = Vec::new();
                for w in words {
                    match glued.last_mut() {
                        Some(last)
                            if matches!(
                                last.as_str(),
                                ">" | ">=" | "<" | "<=" | "=" | "^" | "~"
                            ) =>
                        {
                            last.push_str(w)
                        }
                        _ => glued.push(w.to_owned()),
                    }
                }
                for w in glued {
                    set.extend(term(&w)?);
                }
            }
            sets.push(set);
        }
        Ok(Self {
            text: text.to_owned(),
            sets,
        })
    }

    pub fn matches(&self, v: &Version) -> bool {
        self.sets.iter().any(|set| {
            set.iter().all(|c| c.matches(v))
                && (v.pre.is_empty()
                    || set.iter().any(|c| {
                        !c.version.pre.is_empty()
                            && c.version.core() == v.core()
                            && c.version.pre != ["0"]
                    }))
        })
    }

    /// The highest of `versions` this range allows.
    #[cfg(test)]
    pub fn best<'a>(&self, versions: impl IntoIterator<Item = &'a Version>) -> Option<&'a Version> {
        versions.into_iter().filter(|v| self.matches(v)).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn ok(range: &str, version: &str) -> bool {
        Range::parse(range).unwrap().matches(&v(version))
    }

    #[test]
    fn versions_parse_print_and_order() {
        assert_eq!(v("1.2.3"), Version::new(1, 2, 3));
        assert_eq!(v("v2.0.0-preview.3+abc").to_string(), "2.0.0-preview.3");
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("1.2.3.4").is_err());
        assert!(Version::parse("one").is_err());
        let mut list = [
            v("1.0.0"),
            v("1.0.0-beta.2"),
            v("1.0.0-beta.10"),
            v("0.9.9"),
            v("1.0.0-alpha"),
        ];
        list.sort();
        let s: Vec<String> = list.iter().map(|x| x.to_string()).collect();
        assert_eq!(
            s,
            [
                "0.9.9",
                "1.0.0-alpha",
                "1.0.0-beta.2",
                "1.0.0-beta.10",
                "1.0.0"
            ]
        );
    }

    #[test]
    fn ranges_match_like_npm() {
        assert!(ok("1.2.3", "1.2.3") && !ok("1.2.3", "1.2.4"));
        assert!(ok("^1.2.3", "1.9.0") && !ok("^1.2.3", "2.0.0") && !ok("^1.2.3", "1.2.2"));
        assert!(ok("^0.2.3", "0.2.9") && !ok("^0.2.3", "0.3.0"));
        assert!(ok("~1.2.3", "1.2.9") && !ok("~1.2.3", "1.3.0"));
        assert!(ok("1.x", "1.7.1") && !ok("1.x", "2.0.0"));
        assert!(ok("1.2.*", "1.2.5") && !ok("1.2.*", "1.3.0"));
        assert!(ok(">=1.0.0 <2.0.0", "1.5.0") && !ok(">=1.0.0 <2.0.0", "2.0.0"));
        assert!(ok(">= 1.0", "1.0.0"));
        assert!(ok("1.x || 3.x", "3.1.0") && !ok("1.x || 3.x", "2.1.0"));
        assert!(ok("*", "9.9.9") && ok("", "0.0.1"));
        assert!(ok("1.0.0 - 2.0.0", "2.0.0") && !ok("1.0.0 - 2.0.0", "2.0.1"));
        // Pre-releases only when asked for.
        assert!(!ok("^1.0.0", "1.1.0-beta"));
        assert!(ok(">=1.1.0-beta", "1.1.0-beta.2"));
        assert!(!ok("^1.0.0", "2.0.0-beta"));
        assert!(Range::parse("^one").is_err());
        let r = Range::parse("^1.0.0").unwrap();
        let vs = [v("1.0.0"), v("1.4.0"), v("2.0.0")];
        assert_eq!(r.best(&vs), Some(&vs[1]));
        assert_eq!(r.to_string(), "^1.0.0");
    }
}
