//! Semantic versions ("0.0.2", "v1.2.3-beta.1") and their ordering.

use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifiers ("beta", "1"); empty for a release.
    pub pre: Vec<String>,
}

impl Version {
    /// Parse a version or tag: a leading `v` is ignored, missing parts are 0 and build
    /// metadata (`+...`) is dropped.
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        let t = t.strip_prefix(['v', 'V']).unwrap_or(t);
        let t = t.split('+').next()?;
        let (core, pre) = match t.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
            None => (t, Vec::new()),
        };
        let mut nums = core.split('.');
        let mut part = |required: bool| -> Option<u64> {
            match nums.next() {
                Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => n.parse().ok(),
                None if !required => Some(0),
                _ => None,
            }
        };
        let v = Self {
            major: part(true)?,
            minor: part(false)?,
            patch: part(false)?,
            pre,
        };
        if nums.next().is_some() || v.pre.iter().any(String::is_empty) {
            return None;
        }
        Some(v)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release is newer than its pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => cmp_pre(&self.pre, &other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// SemVer pre-release order: numeric ids by value and before alphanumeric ones.
fn cmp_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

/// True when `candidate` parses and is newer than `current` (unparsable = never newer).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Some(c), Some(r)) => c > r,
        _ => false,
    }
}
