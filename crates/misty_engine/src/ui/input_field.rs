//! Input Field text editing: caret / selection movement, typing with content-type validation and
//! the character limit. Positions are character indices (not bytes).

use crate::scene::{InputContentType, UiInputField};

/// Caret and selection of the field being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct EditState {
    pub field: hecs::Entity,
    /// Caret position (0 = before the first character).
    pub caret: usize,
    /// Other end of the selection (= caret when nothing is selected).
    pub anchor: usize,
    /// Text when editing started (Escape restores it).
    pub original: String,
}

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    None,
    /// The text changed.
    Edited,
    /// Return: finish editing.
    Submit,
    /// Escape: restore the original text and finish.
    Cancel,
}

impl EditState {
    pub fn new(field: hecs::Entity, text: &str, caret: usize) -> Self {
        let caret = caret.min(text.chars().count());
        Self {
            field,
            caret,
            anchor: caret,
            original: text.to_owned(),
        }
    }

    /// Selected character range (start, end).
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    fn move_to(&mut self, pos: usize, extend: bool) {
        self.caret = pos;
        if !extend {
            self.anchor = pos;
        }
    }

    /// Delete the selection; true if there was one.
    fn delete_selection(&mut self, text: &mut String) -> bool {
        let (a, b) = self.selection();
        if a == b {
            return false;
        }
        *text = text.chars().take(a).chain(text.chars().skip(b)).collect();
        self.caret = a;
        self.anchor = a;
        true
    }

    /// Type `s` at the caret (replacing the selection); characters the field refuses are skipped.
    pub fn insert(&mut self, text: &mut String, s: &str, field: &UiInputField) -> bool {
        if field.read_only {
            return false;
        }
        let mut changed = false;
        for c in s.chars() {
            if c.is_control() {
                continue;
            }
            let mut probe = text.clone();
            let mut st = self.clone();
            st.delete_selection(&mut probe);
            let limit = field.character_limit as usize;
            if limit > 0 && probe.chars().count() >= limit {
                continue;
            }
            if !accepts(field.content_type, &probe, st.caret, c) {
                continue;
            }
            let at = st.caret;
            probe = probe
                .chars()
                .take(at)
                .chain(std::iter::once(c))
                .chain(probe.chars().skip(at))
                .collect();
            st.caret = at + 1;
            st.anchor = st.caret;
            *text = probe;
            *self = st;
            changed = true;
        }
        changed
    }

    /// Handle one key press (`key` uses the script key names: "left", "backspace", …).
    pub fn key(
        &mut self,
        text: &mut String,
        key: &str,
        shift: bool,
        ctrl: bool,
        field: &UiInputField,
    ) -> KeyOutcome {
        let len = text.chars().count();
        let editable = !field.read_only;
        match key {
            "left" => {
                let to = if self.has_selection() && !shift {
                    self.selection().0
                } else {
                    self.caret.saturating_sub(1)
                };
                self.move_to(to, shift);
            }
            "right" => {
                let to = if self.has_selection() && !shift {
                    self.selection().1
                } else {
                    (self.caret + 1).min(len)
                };
                self.move_to(to, shift);
            }
            "home" | "up" => self.move_to(0, shift),
            "end" | "down" => self.move_to(len, shift),
            "a" if ctrl => {
                self.anchor = 0;
                self.caret = len;
            }
            "backspace" if editable => {
                if self.delete_selection(text) {
                    return KeyOutcome::Edited;
                }
                if self.caret == 0 {
                    return KeyOutcome::None;
                }
                self.anchor = self.caret - 1;
                self.delete_selection(text);
                return KeyOutcome::Edited;
            }
            "delete" if editable => {
                if !self.delete_selection(text) {
                    if self.caret >= len {
                        return KeyOutcome::None;
                    }
                    self.anchor = self.caret + 1;
                    self.delete_selection(text);
                }
                return KeyOutcome::Edited;
            }
            "return" => return KeyOutcome::Submit,
            "escape" => return KeyOutcome::Cancel,
            _ => {}
        }
        KeyOutcome::None
    }
}

/// Can `c` go into `text` at `pos` for this content type?
pub fn accepts(kind: InputContentType, text: &str, pos: usize, c: char) -> bool {
    let has_minus = text.starts_with('-');
    let minus_ok = c == '-' && pos == 0 && !has_minus;
    // Nothing goes in front of a minus sign.
    if has_minus
        && pos == 0
        && matches!(
            kind,
            InputContentType::IntegerNumber | InputContentType::DecimalNumber
        )
    {
        return false;
    }
    match kind {
        InputContentType::Standard | InputContentType::Password => true,
        InputContentType::IntegerNumber => c.is_ascii_digit() || minus_ok,
        InputContentType::DecimalNumber => {
            c.is_ascii_digit() || minus_ok || (c == '.' && !text.contains('.'))
        }
    }
}

/// Keep only what the field accepts (used when scripts or the Inspector set the text).
pub fn sanitize(field: &UiInputField, text: &str) -> String {
    let mut out = String::new();
    let mut st = EditState::new(hecs::Entity::DANGLING, "", 0);
    let f = UiInputField {
        read_only: false,
        ..field.clone()
    };
    st.insert(&mut out, &text.replace(['\n', '\r'], ""), &f);
    out
}

/// Caret index closest to `x` given the caret stops of the shown text.
pub fn caret_at(stops: &[f32], x: f32) -> usize {
    let mut best = 0;
    let mut dist = f32::MAX;
    for (i, s) in stops.iter().enumerate() {
        let d = (s - x).abs();
        if d < dist {
            dist = d;
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: InputContentType, limit: u32) -> UiInputField {
        UiInputField {
            content_type: kind,
            character_limit: limit,
            ..Default::default()
        }
    }

    #[test]
    fn typing_moving_and_deleting() {
        let f = field(InputContentType::Standard, 0);
        let mut text = String::new();
        let mut st = EditState::new(hecs::Entity::DANGLING, &text, 0);
        assert!(st.insert(&mut text, "helo", &f));
        st.key(&mut text, "left", false, false, &f);
        st.insert(&mut text, "l", &f);
        assert_eq!((text.as_str(), st.caret), ("hello", 4));
        assert_eq!(
            st.key(&mut text, "backspace", false, false, &f),
            KeyOutcome::Edited
        );
        assert_eq!(text, "helo");
        st.key(&mut text, "home", false, false, &f);
        st.key(&mut text, "delete", false, false, &f);
        assert_eq!((text.as_str(), st.caret), ("elo", 0));
        assert_eq!(
            st.key(&mut text, "backspace", false, false, &f),
            KeyOutcome::None
        );
        assert_eq!(
            st.key(&mut text, "return", false, false, &f),
            KeyOutcome::Submit
        );
    }

    #[test]
    fn selection_is_replaced_and_deleted() {
        let f = field(InputContentType::Standard, 0);
        let mut text = "héllo world".to_owned();
        let mut st = EditState::new(hecs::Entity::DANGLING, &text, 0);
        for _ in 0..5 {
            st.key(&mut text, "right", true, false, &f);
        }
        assert_eq!(st.selection(), (0, 5));
        st.insert(&mut text, "bye", &f);
        assert_eq!(text, "bye world");
        st.key(&mut text, "a", false, true, &f);
        assert_eq!(st.selection(), (0, 9));
        st.key(&mut text, "backspace", false, false, &f);
        assert_eq!(text, "");
        // Left with a selection collapses to its start.
        text = "abc".into();
        st = EditState::new(hecs::Entity::DANGLING, &text, 3);
        st.key(&mut text, "left", true, false, &f);
        st.key(&mut text, "left", false, false, &f);
        assert_eq!((st.caret, st.anchor), (2, 2));
    }

    #[test]
    fn content_types_and_character_limit() {
        let int = field(InputContentType::IntegerNumber, 0);
        let mut text = String::new();
        let mut st = EditState::new(hecs::Entity::DANGLING, &text, 0);
        st.insert(&mut text, "-12a3-4", &int);
        assert_eq!(text, "-1234");
        let dec = field(InputContentType::DecimalNumber, 0);
        assert_eq!(sanitize(&dec, "3.1.4x"), "3.14");
        assert_eq!(
            sanitize(&field(InputContentType::Standard, 4), "abcdef"),
            "abcd"
        );
        assert_eq!(
            sanitize(&field(InputContentType::Standard, 0), "a\nb"),
            "ab"
        );
        let ro = UiInputField {
            read_only: true,
            ..Default::default()
        };
        assert!(!st.insert(&mut text, "9", &ro));
        assert_eq!(caret_at(&[0.0, 10.0, 20.0], 14.0), 1);
    }
}
