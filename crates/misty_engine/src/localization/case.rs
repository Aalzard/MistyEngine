//! Locale-aware upper and lower case.
//!
//! Rust's `to_uppercase` / `to_lowercase` follow the language-neutral Unicode rules, which are
//! wrong for Turkish and Azerbaijani: those languages have a dotted and a dotless i in both
//! cases (i ↔ İ, ı ↔ I). "istanbul" upper-cases to "İSTANBUL" in Turkish, not "ISTANBUL", and
//! "KIRMIZI" lower-cases to "kırmızı", not "kirmizi". Every place the runtime changes the case
//! of text for a locale goes through [`to_upper`] / [`to_lower`].

use super::format::language;

/// Languages with the dotted / dotless i pairs.
fn dotted_i(locale: &str) -> bool {
    matches!(language(locale).as_str(), "tr" | "az")
}

/// `text` in upper case for `locale` ("tr": i → İ, ı → I).
pub fn to_upper(text: &str, locale: &str) -> String {
    if !dotted_i(locale) {
        return text.to_uppercase();
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'i' => out.push('İ'),
            // 'ı' upper-cases to 'I' with the neutral rules too.
            _ => out.extend(c.to_uppercase()),
        }
    }
    out
}

/// `text` in lower case for `locale` ("tr": I → ı, İ → i).
pub fn to_lower(text: &str, locale: &str) -> String {
    if !dotted_i(locale) {
        return text.to_lowercase();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // "I" followed by a combining dot above is a dotted capital written in two parts.
            'I' if chars.peek() == Some(&'\u{307}') => {
                chars.next();
                out.push('i');
            }
            'I' => out.push('ı'),
            'İ' => out.push('i'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// `text` with the first letter of each word in upper case for `locale` (the rest unchanged).
pub fn to_title(text: &str, locale: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start = true;
    for c in text.chars() {
        if start && c.is_alphabetic() {
            let mut buf = [0u8; 4];
            out.push_str(&to_upper(c.encode_utf8(&mut buf), locale));
            start = false;
        } else {
            out.push(c);
            if c.is_whitespace() {
                start = true;
            } else if c.is_alphabetic() {
                start = false;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkish_has_dotted_and_dotless_i() {
        assert_eq!(to_upper("istanbul", "tr"), "İSTANBUL");
        assert_eq!(to_upper("ılık", "tr-TR"), "ILIK");
        assert_eq!(to_lower("KIRMIZI", "tr"), "kırmızı");
        assert_eq!(to_lower("İZMİR", "tr"), "izmir");
        assert_eq!(to_lower("I\u{307}", "tr"), "i");
        assert_eq!(to_upper("şeker çiğ öğün", "tr"), "ŞEKER ÇİĞ ÖĞÜN");
        assert_eq!(to_title("iyi günler", "tr"), "İyi Günler");
        assert_eq!(to_upper("iyi", "az"), "İYİ");
    }

    #[test]
    fn other_languages_use_the_neutral_rules() {
        assert_eq!(to_upper("istanbul", "en"), "ISTANBUL");
        assert_eq!(to_lower("KIRMIZI", "en-US"), "kirmizi");
        assert_eq!(to_upper("straße", "de"), "STRASSE");
        assert_eq!(to_title("good morning", "en"), "Good Morning");
        // Arabic has no case: unchanged.
        assert_eq!(to_upper("مرحبا", "ar"), "مرحبا");
    }
}
