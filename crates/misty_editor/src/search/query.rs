//! Search text parsing shared by the palette and the panel search boxes.
//!
//! `h: p: m: s:` at the start limit the palette to one provider, `=` starts a calculation, and
//! `t:Type`, `tag:Name`, `layer:Name` tokens filter objects / assets. Other tokens are name words.

/// Which providers a palette query asks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Hierarchy,
    Project,
    Menu,
    Settings,
}

impl Scope {
    /// Prefix that selects this scope (`h:` …).
    pub const PREFIXES: [(&'static str, Scope); 4] = [
        ("h:", Scope::Hierarchy),
        ("p:", Scope::Project),
        ("m:", Scope::Menu),
        ("s:", Scope::Settings),
    ];
}

/// Parsed search text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub scope: Scope,
    /// Lower-case name words.
    pub words: Vec<String>,
    /// `t:` values, lower-case with spaces removed ("meshrenderer").
    pub types: Vec<String>,
    /// `tag:` values, lower-case.
    pub tags: Vec<String>,
    /// `layer:` values, lower-case.
    pub layers: Vec<String>,
    /// Text after a leading `=`.
    pub calc: Option<String>,
}

/// `token` starts with `prefix` (ASCII case-insensitive); returns the rest.
fn strip<'a>(token: &'a str, prefix: &str) -> Option<&'a str> {
    let head = token.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &token[prefix.len()..])
}

/// Normalised type name: lower-case, no spaces ("Mesh Renderer" → "meshrenderer").
pub fn type_key(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

impl Query {
    /// Parse panel search text (no provider prefix or calculator).
    pub fn parse(text: &str) -> Self {
        let mut q = Self::default();
        for token in text.split_whitespace() {
            if let Some(t) = strip(token, "t:") {
                q.types.push(type_key(t));
            } else if let Some(t) = strip(token, "tag:") {
                q.tags.push(t.to_lowercase());
            } else if let Some(l) = strip(token, "layer:") {
                q.layers.push(l.to_lowercase());
            } else {
                q.words.push(token.to_lowercase());
            }
        }
        // A bare `t:` while typing filters nothing yet.
        q.types.retain(|t| !t.is_empty());
        q.tags.retain(|t| !t.is_empty());
        q.layers.retain(|t| !t.is_empty());
        q
    }

    /// Parse palette text: optional provider prefix, `=` calculator, then [`Query::parse`].
    pub fn parse_palette(text: &str) -> Self {
        let text = text.trim_start();
        if let Some(expr) = text.strip_prefix('=') {
            return Self {
                calc: Some(expr.trim().to_owned()),
                ..Self::default()
            };
        }
        for (prefix, scope) in Scope::PREFIXES {
            if let Some(rest) = strip(text, prefix) {
                return Self {
                    scope,
                    ..Self::parse(rest)
                };
            }
        }
        Self::parse(text)
    }

    /// Nothing typed but a prefix: list everything the scope has.
    #[cfg(test)]
    pub fn is_blank(&self) -> bool {
        self.words.is_empty()
            && self.types.is_empty()
            && self.tags.is_empty()
            && self.layers.is_empty()
            && self.calc.is_none()
    }

    /// Object/asset filters were given (they don't apply to menus and settings).
    pub fn has_filters(&self) -> bool {
        !self.types.is_empty() || !self.tags.is_empty() || !self.layers.is_empty()
    }

    pub fn wants(&self, scope: Scope) -> bool {
        self.scope == Scope::All || self.scope == scope
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_filters() {
        let q = Query::parse("Red  T:Mesh tag:Player LAYER:UI t:");
        assert_eq!(q.words, ["red"]);
        assert_eq!(q.types, ["mesh"]);
        assert_eq!(q.tags, ["player"]);
        assert_eq!(q.layers, ["ui"]);
        assert_eq!(q.scope, Scope::All);
        assert_eq!(type_key("Mesh Renderer"), "meshrenderer");
    }

    #[test]
    fn provider_prefixes() {
        for (text, scope) in [
            ("h:cam", Scope::Hierarchy),
            ("P:red", Scope::Project),
            ("m:create cube", Scope::Menu),
            ("s:physics", Scope::Settings),
            ("cube", Scope::All),
        ] {
            assert_eq!(Query::parse_palette(text).scope, scope, "{text}");
        }
        let q = Query::parse_palette("m:create cube");
        assert_eq!(q.words, ["create", "cube"]);
        assert!(Query::parse_palette("h:").is_blank());
        assert!(Query::parse_palette("h: t:Light").has_filters());
        // Prefixes only count at the start.
        assert_eq!(Query::parse_palette("cube h:x").scope, Scope::All);
        assert!(Query::parse_palette("p:x").wants(Scope::Project));
        assert!(!Query::parse_palette("p:x").wants(Scope::Menu));
    }

    #[test]
    fn calculator_prefix() {
        let q = Query::parse_palette(" =2*3");
        assert_eq!(q.calc.as_deref(), Some("2*3"));
        assert!(!q.is_blank());
    }
}
