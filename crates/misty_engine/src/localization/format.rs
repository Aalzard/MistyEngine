//! Smart strings: `{0}` / `{name}` arguments and `{n:plural:one|other}` plural forms.
//!
//! - `{0}`, `{1}`… positional arguments; `{player}` named ones. Unknown ones stay as written.
//! - `{{` and `}}` are literal braces.
//! - `{count:plural:an apple|{} apples}` picks a form by the locale's plural rules; `{}` inside
//!   a form is the number, and forms may use other arguments (`{0:plural:one|{0} of {total}}`).
//!   Forms are listed in the language's order (English, Spanish: one|other; French: one|other
//!   with 0 and 1 singular; Arabic: zero|one|two|few|many|other; Japanese/Chinese: one form;
//!   Russian/Polish: one|few|many). With fewer forms than the language has, two forms mean
//!   one|other and three zero|one|other.
//! - `{name:upper}`, `{name:lower}` and `{name:title}` change the argument's case with the
//!   locale's rules (Turkish: i ↔ İ, ı ↔ I; see [`super::case`]).

use std::collections::BTreeMap;

/// Arguments of a smart string.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub positional: Vec<String>,
    pub named: BTreeMap<String, String>,
}

impl Args {
    pub fn none() -> Self {
        Self::default()
    }

    /// From Inspector entries: `name=value` is a named argument, anything else positional.
    pub fn from_entries(entries: &[String]) -> Self {
        let mut a = Self::default();
        for e in entries {
            match e.split_once('=') {
                Some((k, v)) if is_ident(k.trim()) => {
                    a.named.insert(k.trim().to_owned(), v.to_owned());
                }
                _ => a.positional.push(e.clone()),
            }
        }
        a
    }

    fn get(&self, name: &str) -> Option<&str> {
        match name.parse::<usize>() {
            Ok(i) => self.positional.get(i).map(String::as_str),
            Err(_) => self.named.get(name).map(String::as_str),
        }
    }
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !s.starts_with(|c: char| c.is_ascii_digit())
}

/// Fill in `text`'s placeholders for `locale`.
pub fn format(text: &str, args: &Args, locale: &str) -> String {
    format_inner(text, args, locale, None, 0)
}

/// Deepest plural form inside a plural form that is filled in (deeper ones stay literal), so
/// a malformed table cell can't overflow the stack.
const MAX_NESTING: usize = 16;

fn format_inner(
    text: &str,
    args: &Args,
    locale: &str,
    number: Option<&str>,
    depth: usize,
) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' && chars.get(i + 1) == Some(&'{') {
            out.push('{');
            i += 2;
            continue;
        }
        if c == '}' && chars.get(i + 1) == Some(&'}') {
            out.push('}');
            i += 2;
            continue;
        }
        if c != '{' {
            out.push(c);
            i += 1;
            continue;
        }
        // Find the matching close brace (forms may nest placeholders).
        let mut open = 0;
        let mut end = None;
        for (j, &d) in chars.iter().enumerate().skip(i) {
            match d {
                '{' => open += 1,
                '}' => {
                    open -= 1;
                    if open == 0 {
                        end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            // Unbalanced: the rest is literal.
            out.extend(&chars[i..]);
            break;
        };
        let inner: String = chars[i + 1..end].iter().collect();
        out.push_str(&placeholder(&inner, args, locale, number, depth));
        i = end + 1;
    }
    out
}

/// One `{…}`: `name`, empty (the plural's number) or `name:formatter:options`.
fn placeholder(
    inner: &str,
    args: &Args,
    locale: &str,
    number: Option<&str>,
    depth: usize,
) -> String {
    let (name, rest) = match inner.split_once(':') {
        Some((n, r)) => (n.trim(), Some(r)),
        None => (inner.trim(), None),
    };
    let value = if name.is_empty() {
        number
    } else {
        args.get(name)
    };
    let Some(value) = value else {
        return format!("{{{inner}}}");
    };
    let Some(rest) = rest else {
        return value.to_owned();
    };
    let (formatter, options) = rest.split_once(':').unwrap_or((rest, ""));
    match formatter.trim() {
        "plural" | "p" => {
            let forms = split_forms(options);
            let n: f64 = value.trim().parse().unwrap_or(0.0);
            let index = plural_index(locale, n, forms.len());
            let form = forms.get(index).copied().unwrap_or("");
            if depth >= MAX_NESTING {
                return form.to_owned();
            }
            format_inner(form, args, locale, Some(value), depth + 1)
        }
        "upper" => super::case::to_upper(value, locale),
        "lower" => super::case::to_lower(value, locale),
        "title" => super::case::to_title(value, locale),
        // Unknown formatters show the plain value.
        _ => value.to_owned(),
    }
}

/// `a|b|{x|y}` → ["a", "b", "{x|y}"]: bars inside nested braces don't split.
fn split_forms(options: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in options.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            '|' if depth == 0 => {
                out.push(&options[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&options[start..]);
    out
}

/// Number of plural forms a language uses.
pub fn plural_count(locale: &str) -> usize {
    match language(locale).as_str() {
        "ar" => 6,
        "ru" | "uk" | "pl" | "cs" | "sk" | "hr" | "sr" | "be" => 3,
        "ja" | "zh" | "ko" | "th" | "vi" | "id" | "ms" | "tr" => 1,
        _ => 2,
    }
}

/// Index of the form to use for `n` among `forms` forms.
pub fn plural_index(locale: &str, n: f64, forms: usize) -> usize {
    if forms <= 1 {
        return 0;
    }
    let whole = n.fract() == 0.0;
    let i = n.abs().trunc() as u64;
    if forms == plural_count(locale) {
        let lang = language(locale);
        let idx = match lang.as_str() {
            "fr" | "pt" if forms == 2 => usize::from(i >= 2),
            "ar" => {
                if !whole {
                    5
                } else {
                    match (i, i % 100) {
                        (0, _) => 0,
                        (1, _) => 1,
                        (2, _) => 2,
                        (_, 3..=10) => 3,
                        (_, 11..=99) => 4,
                        _ => 5,
                    }
                }
            }
            "ru" | "uk" | "pl" | "cs" | "sk" | "hr" | "sr" | "be" => {
                let (m10, m100) = (i % 10, i % 100);
                if lang == "pl" {
                    if i == 1 {
                        0
                    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                        1
                    } else {
                        2
                    }
                } else if lang == "cs" || lang == "sk" {
                    match i {
                        1 => 0,
                        2..=4 => 1,
                        _ => 2,
                    }
                } else if m10 == 1 && m100 != 11 {
                    0
                } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                    1
                } else {
                    2
                }
            }
            _ => usize::from(!(whole && i == 1)),
        };
        return idx.min(forms - 1);
    }
    // Generic rules when the forms don't match the language's count.
    match forms {
        2 => usize::from(!(whole && i == 1)),
        _ => {
            if whole && i == 0 {
                0
            } else if whole && i == 1 {
                1
            } else {
                2.min(forms - 1)
            }
        }
    }
}

/// "fr" of "fr-CA" / "fr_CA".
pub fn language(locale: &str) -> String {
    locale
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Languages written right to left.
pub fn is_right_to_left(locale: &str) -> bool {
    matches!(
        language(locale).as_str(),
        "ar" | "he" | "fa" | "ur" | "ps" | "yi" | "dv"
    )
}
