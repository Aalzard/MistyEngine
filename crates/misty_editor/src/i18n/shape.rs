//! Arabic (and other right-to-left) text for egui.
//!
//! egui draws one glyph per character, left to right, without shaping. Arabic needs two things
//! it doesn't do, both done here on the text before it reaches egui:
//! 1. Joining: each letter takes its isolated, initial, medial or final form from its
//!    neighbours (`ar-reshaper` maps letters to the Arabic Presentation Forms, which the editor
//!    fonts have), lam + alef become one ligature.
//! 2. Order: the Unicode Bidirectional Algorithm (`unicode-bidi`) turns the logical order into
//!    the visual one, line by line: right-to-left runs are reversed, numbers and Latin words
//!    inside them keep reading left to right, brackets are mirrored.
//!
//! The result is plain text in visual order, so it must only be drawn, never edited (text
//! fields keep the logical text).

use std::borrow::Cow;

use unicode_bidi::{BidiInfo, Level};

/// Does `text` still need [`visual`]: does it have Arabic or Hebrew letters in logical form?
/// Text that went through [`visual`] has only presentation forms left, so shaping twice (a
/// translated label passed through `tr` again) changes nothing.
pub fn needs_shaping(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x05D0..=0x05EA | 0x0621..=0x064A | 0x066E..=0x06D3 | 0x06FA..=0x06FF | 0x0750..=0x077F)
    })
}

/// Paragraph direction for [`visual`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    /// From the first strong character (user content: object names, console lines).
    Auto,
    /// Right to left (editor text in an RTL language, even when it starts with a Latin word).
    Rtl,
}

/// The mirror image of a bracket-like character shown in a right-to-left run (rule L4).
fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

/// Join the Arabic letters of `text` (logical order kept).
pub fn join_letters(text: &str) -> String {
    thread_local! {
        static RESHAPER: ar_reshaper::ArabicReshaper = ar_reshaper::ArabicReshaper::default();
    }
    RESHAPER.with(|r| r.reshape(text))
}

/// One line of `text` in visual order (joined letters, right-to-left runs reversed).
fn visual_line(text: &str, base: Base) -> String {
    let joined = join_letters(text);
    let level = match base {
        Base::Auto => None,
        Base::Rtl => Some(Level::rtl()),
    };
    let info = BidiInfo::new(&joined, level);
    let Some(para) = info.paragraphs.first() else {
        return joined;
    };
    let line = para.range.clone();
    let (levels, runs) = info.visual_runs(para, line);
    let mut out = String::with_capacity(joined.len());
    for run in runs {
        let part = &joined[run.clone()];
        if levels[run.start].is_rtl() {
            out.extend(part.chars().rev().map(mirrored));
        } else {
            out.push_str(part);
        }
    }
    out
}

/// `text` ready for egui: Arabic letters joined and every line in visual order. Text without
/// right-to-left letters (or already in visual order) comes back unchanged (borrowed).
pub fn visual(text: &str, base: Base) -> Cow<'_, str> {
    if !needs_shaping(text) {
        return Cow::Borrowed(text);
    }
    let lines: Vec<String> = text.split('\n').map(|l| visual_line(l, base)).collect();
    Cow::Owned(lines.join("\n"))
}

/// Break the logical `text` into lines no wider than `max_width` (measured by `width`) at
/// spaces, then put each line in visual order. egui wraps visual text from the left, which
/// would put the end of a right-to-left sentence on its first line; lines broken here first
/// keep reading from the top.
pub fn visual_wrapped(
    text: &str,
    base: Base,
    max_width: f32,
    width: &mut dyn FnMut(&str) -> f32,
) -> String {
    let mut out: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && width(&visual_line(&candidate, base)) > max_width {
                out.push(visual_line(&line, base));
                line = word.to_owned();
            } else {
                line = candidate;
            }
        }
        out.push(visual_line(&line, base));
    }
    out.join("\n")
}

/// [`visual_wrapped`] with a character budget instead of a measured width (when no font is at
/// hand): about `max_chars` characters per line.
pub fn visual_wrapped_chars(text: &str, base: Base, max_chars: usize) -> String {
    visual_wrapped(text, base, max_chars as f32, &mut |l: &str| {
        l.chars().count() as f32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "مرحبا": meem initial, reh final, hah initial, beh medial, alef final; drawn right to
    /// left, so the visual string starts with the alef.
    #[test]
    fn arabic_word_is_joined_and_reversed() {
        let v = visual("\u{645}\u{631}\u{62d}\u{628}\u{627}", Base::Rtl);
        assert_eq!(v, "\u{FE8E}\u{FE92}\u{FEA3}\u{FEAE}\u{FEE3}");
        // Lam + alef is one ligature: "لا" → U+FEFB.
        assert_eq!(visual("\u{644}\u{627}", Base::Auto), "\u{FEFB}");
    }

    #[test]
    fn numbers_and_latin_words_keep_their_order() {
        // "حفظ 3 Misty" (save 3 Misty) in an RTL paragraph: the Arabic word is on the right,
        // "Misty" and "3" read left to right on the left.
        let v = visual("\u{62d}\u{641}\u{638} 3 Misty", Base::Rtl);
        assert_eq!(v, "Misty 3 \u{FEC6}\u{FED4}\u{FEA3}");
        // Version numbers stay whole.
        let v = visual(
            "\u{627}\u{644}\u{625}\u{635}\u{62f}\u{627}\u{631} 0.3.1",
            Base::Rtl,
        );
        assert!(v.starts_with("0.3.1 "), "{v}");
        // A Latin paragraph with one Arabic word only reverses that word.
        let v = visual("Open \u{645}\u{631}\u{62d}\u{628}\u{627} now", Base::Auto);
        assert_eq!(v, "Open \u{FE8E}\u{FE92}\u{FEA3}\u{FEAE}\u{FEE3} now");
    }

    #[test]
    fn brackets_are_mirrored_in_right_to_left_runs() {
        // "(مرحبا)" in RTL: the visual string starts with "(" again (the closing bracket,
        // mirrored), so it still reads as a bracketed word.
        let v = visual("(\u{645}\u{631}\u{62d}\u{628}\u{627})", Base::Rtl);
        assert_eq!(v, "(\u{FE8E}\u{FE92}\u{FEA3}\u{FEAE}\u{FEE3})");
    }

    #[test]
    fn plain_and_already_shaped_text_is_borrowed() {
        assert!(matches!(visual("File", Base::Rtl), Cow::Borrowed("File")));
        let once = visual("\u{645}\u{631}\u{62d}\u{628}\u{627}\u{61f}", Base::Rtl).into_owned();
        assert!(
            matches!(visual(&once, Base::Rtl), Cow::Borrowed(_)),
            "shaping twice: {once}"
        );
    }

    #[test]
    fn wrapping_keeps_the_first_words_on_the_first_line() {
        // Three Arabic words, at most two per line: the first two words go on the first line.
        let (a, b, c) = ("\u{628}\u{628}", "\u{62a}\u{62a}", "\u{62b}\u{62b}");
        let text = format!("{a} {b} {c}");
        let wrapped = visual_wrapped_chars(&text, Base::Rtl, 5);
        let lines: Vec<&str> = wrapped.lines().collect();
        assert_eq!(lines.len(), 2, "{wrapped:?}");
        assert_eq!(lines[0], visual(&format!("{a} {b}"), Base::Rtl));
        assert_eq!(lines[1], visual(c, Base::Rtl));
    }
}
