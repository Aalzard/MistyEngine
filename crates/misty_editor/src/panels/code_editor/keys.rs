//! Keyboard shortcuts of the Code Editor (Visual Studio style, with two-key chords such as
//! Ctrl+K, Ctrl+C). The same table feeds the Keyboard Shortcuts popup.

use egui::{Key, Modifiers};

/// A command reachable from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Save,
    SaveAll,
    CloseDoc,
    NextDoc,
    PrevDoc,
    Undo,
    Redo,
    SelectAll,
    Duplicate,
    CutLine,
    DeleteLine,
    MoveLinesUp,
    MoveLinesDown,
    InsertLineBelow,
    InsertLineAbove,
    Comment,
    Uncomment,
    ToggleComment,
    FormatDocument,
    Complete,
    NextOccurrence,
    AllOccurrences,
    AddCaretAbove,
    AddCaretBelow,
    Lowercase,
    Uppercase,
    Find,
    Replace,
    FindNext,
    FindPrev,
    FindWord,
    GoToLine,
    FindInFiles,
    GoToDefinition,
    GoToBrace,
    CollapseAll,
    ToggleAllFolds,
    ToggleFold,
    ExpandAll,
    ToggleWordWrap,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ScrollLineUp,
    ScrollLineDown,
    Help,
    Play,
    Stop,
}

pub struct Binding {
    pub mods: Modifiers,
    pub key: Key,
    /// Second key of a chord (pressed with or without Ctrl).
    pub then: Option<Key>,
    pub cmd: Cmd,
}

const C: Modifiers = Modifiers::COMMAND;
const CS: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CA: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);
const A: Modifiers = Modifiers::ALT;
const SA: Modifiers = Modifiers::SHIFT.plus(Modifiers::ALT);
const S: Modifiers = Modifiers::SHIFT;
const N: Modifiers = Modifiers::NONE;

const fn b(mods: Modifiers, key: Key, cmd: Cmd) -> Binding {
    Binding {
        mods,
        key,
        then: None,
        cmd,
    }
}

const fn chord(key: Key, then: Key, cmd: Cmd) -> Binding {
    Binding {
        mods: C,
        key,
        then: Some(then),
        cmd,
    }
}

/// Every binding; more specific modifiers come first.
pub const BINDINGS: &[Binding] = &[
    b(CS, Key::S, Cmd::SaveAll),
    b(C, Key::S, Cmd::Save),
    b(C, Key::F4, Cmd::CloseDoc),
    b(C, Key::W, Cmd::CloseDoc),
    b(CS, Key::Tab, Cmd::PrevDoc),
    b(C, Key::Tab, Cmd::NextDoc),
    b(CS, Key::Z, Cmd::Redo),
    b(C, Key::Z, Cmd::Undo),
    b(C, Key::Y, Cmd::Redo),
    b(C, Key::A, Cmd::SelectAll),
    b(C, Key::D, Cmd::Duplicate),
    chord(Key::E, Key::V, Cmd::Duplicate),
    b(CS, Key::L, Cmd::DeleteLine),
    b(C, Key::L, Cmd::CutLine),
    b(A, Key::ArrowUp, Cmd::MoveLinesUp),
    b(A, Key::ArrowDown, Cmd::MoveLinesDown),
    b(CS, Key::Enter, Cmd::InsertLineAbove),
    b(C, Key::Enter, Cmd::InsertLineBelow),
    chord(Key::K, Key::C, Cmd::Comment),
    chord(Key::K, Key::U, Cmd::Uncomment),
    b(C, Key::Slash, Cmd::ToggleComment),
    chord(Key::K, Key::D, Cmd::FormatDocument),
    chord(Key::K, Key::F, Cmd::FormatDocument),
    b(C, Key::Space, Cmd::Complete),
    chord(Key::K, Key::Space, Cmd::Complete),
    b(SA, Key::Period, Cmd::NextOccurrence),
    b(SA, Key::Semicolon, Cmd::AllOccurrences),
    b(SA, Key::Colon, Cmd::AllOccurrences),
    b(CA, Key::ArrowUp, Cmd::AddCaretAbove),
    b(CA, Key::ArrowDown, Cmd::AddCaretBelow),
    b(CS, Key::U, Cmd::Uppercase),
    b(C, Key::U, Cmd::Lowercase),
    b(CS, Key::F, Cmd::FindInFiles),
    b(C, Key::F, Cmd::Find),
    b(C, Key::H, Cmd::Replace),
    b(C, Key::F3, Cmd::FindWord),
    b(S, Key::F3, Cmd::FindPrev),
    b(N, Key::F3, Cmd::FindNext),
    b(C, Key::G, Cmd::GoToLine),
    b(N, Key::F12, Cmd::GoToDefinition),
    b(C, Key::CloseBracket, Cmd::GoToBrace),
    chord(Key::M, Key::O, Cmd::CollapseAll),
    chord(Key::M, Key::L, Cmd::ToggleAllFolds),
    chord(Key::M, Key::M, Cmd::ToggleFold),
    chord(Key::M, Key::P, Cmd::ExpandAll),
    chord(Key::E, Key::W, Cmd::ToggleWordWrap),
    b(C, Key::Equals, Cmd::ZoomIn),
    b(C, Key::Plus, Cmd::ZoomIn),
    b(C, Key::Minus, Cmd::ZoomOut),
    b(C, Key::Num0, Cmd::ZoomReset),
    b(C, Key::ArrowUp, Cmd::ScrollLineUp),
    b(C, Key::ArrowDown, Cmd::ScrollLineDown),
    b(N, Key::F1, Cmd::Help),
    b(S, Key::F5, Cmd::Stop),
    b(N, Key::F5, Cmd::Play),
];

/// Keys handled by the text area itself, listed in the popup after the table.
pub const BUILTIN_KEYS: &[(&str, &str)] = &[
    (
        "Ctrl+X / Ctrl+C / Ctrl+V",
        "Cut / copy / paste (the whole line when nothing is selected)",
    ),
    ("Shift+Del / Ctrl+Ins / Shift+Ins", "Cut / copy / paste"),
    (
        "Home / End",
        "Smart home (first character, then column 0) / end of line",
    ),
    (
        "Ctrl+Left / Ctrl+Right",
        "Previous / next word (Shift selects)",
    ),
    ("Ctrl+Home / Ctrl+End", "Start / end of the file"),
    (
        "Ctrl+Backspace / Ctrl+Del",
        "Delete the previous / next word",
    ),
    ("Tab / Shift+Tab", "Indent / outdent the selected lines"),
    ("Alt+Click", "Add a caret"),
    (
        "Alt+Shift+Arrows / Alt+Shift+Drag",
        "Box (column) selection",
    ),
    ("Esc", "Back to a single caret; close popups"),
    ("Ctrl+Wheel", "Zoom"),
    ("Double / triple click", "Select word / line"),
];

impl Cmd {
    /// Description for the shortcuts popup and menus.
    pub fn label(self) -> &'static str {
        match self {
            Cmd::Save => "Save",
            Cmd::SaveAll => "Save All",
            Cmd::CloseDoc => "Close file",
            Cmd::NextDoc => "Next file",
            Cmd::PrevDoc => "Previous file",
            Cmd::Undo => "Undo",
            Cmd::Redo => "Redo",
            Cmd::SelectAll => "Select All",
            Cmd::Duplicate => "Duplicate line / selection",
            Cmd::CutLine => "Cut line",
            Cmd::DeleteLine => "Delete line",
            Cmd::MoveLinesUp => "Move lines up",
            Cmd::MoveLinesDown => "Move lines down",
            Cmd::InsertLineBelow => "Insert line below",
            Cmd::InsertLineAbove => "Insert line above",
            Cmd::Comment => "Comment selection",
            Cmd::Uncomment => "Uncomment selection",
            Cmd::ToggleComment => "Toggle line comment",
            Cmd::FormatDocument => "Format document",
            Cmd::Complete => "Complete word (autocomplete)",
            Cmd::NextOccurrence => "Add caret at next occurrence",
            Cmd::AllOccurrences => "Add carets at all occurrences",
            Cmd::AddCaretAbove => "Add caret above",
            Cmd::AddCaretBelow => "Add caret below",
            Cmd::Lowercase => "Make lowercase",
            Cmd::Uppercase => "Make uppercase",
            Cmd::Find => "Find",
            Cmd::Replace => "Replace",
            Cmd::FindNext => "Find next",
            Cmd::FindPrev => "Find previous",
            Cmd::FindWord => "Find the word under the caret",
            Cmd::GoToLine => "Go to line",
            Cmd::FindInFiles => "Find in Files",
            Cmd::GoToDefinition => "Go to definition",
            Cmd::GoToBrace => "Go to matching bracket",
            Cmd::CollapseAll => "Collapse to definitions",
            Cmd::ToggleAllFolds => "Toggle all outlining",
            Cmd::ToggleFold => "Toggle outlining expansion",
            Cmd::ExpandAll => "Stop outlining (expand all)",
            Cmd::ToggleWordWrap => "Toggle word wrap",
            Cmd::ZoomIn => "Zoom in",
            Cmd::ZoomOut => "Zoom out",
            Cmd::ZoomReset => "Reset zoom",
            Cmd::ScrollLineUp => "Scroll one line up",
            Cmd::ScrollLineDown => "Scroll one line down",
            Cmd::Help => "Keyboard shortcuts",
            Cmd::Play => "Enter Play mode",
            Cmd::Stop => "Stop Play mode",
        }
    }

    /// Every shortcut text for this command ("Ctrl+D, Ctrl+E, V").
    pub fn shortcuts(self) -> String {
        BINDINGS
            .iter()
            .filter(|b| b.cmd == self)
            .map(binding_text)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn binding_text(b: &Binding) -> String {
    let first = key_text(b.mods, b.key);
    match b.then {
        Some(t) => format!("{first}, {}", t.name()),
        None => first,
    }
}

pub fn key_text(mods: Modifiers, key: Key) -> String {
    let mut s = String::new();
    if mods.command || mods.ctrl {
        s += "Ctrl+";
    }
    if mods.shift {
        s += "Shift+";
    }
    if mods.alt {
        s += "Alt+";
    }
    let name = match key {
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::Equals => "=",
        Key::Plus => "+",
        Key::Minus => "-",
        Key::Slash => "/",
        Key::Period => ".",
        Key::Semicolon => ";",
        Key::Colon => ";",
        Key::CloseBracket => "]",
        k => k.name(),
    };
    s + name
}

/// Result of a key press against the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    Command(Cmd),
    /// First key of a chord: wait for the second.
    ChordStarted(Key),
    /// The chord's second key matched nothing.
    NotACommand(String),
    /// Not a shortcut: the text area handles it.
    None,
}

/// Match a key press; `chord` holds the first key of a chord in progress.
pub fn lookup(mods: Modifiers, key: Key, chord: &mut Option<Key>) -> Lookup {
    if let Some(first) = chord.take() {
        // Modifier keys alone don't end the chord.
        return match BINDINGS
            .iter()
            .find(|b| b.key == first && b.then == Some(key) && !mods.alt)
        {
            Some(b) => Lookup::Command(b.cmd),
            None => Lookup::NotACommand(format!(
                "The key combination (Ctrl+{}, {}) is not a command.",
                first.name(),
                key_text(mods, key)
            )),
        };
    }
    for b in BINDINGS {
        if b.key == key && mods.matches_exact(b.mods) {
            if b.then.is_some() {
                *chord = Some(key);
                return Lookup::ChordStarted(key);
            }
            return Lookup::Command(b.cmd);
        }
    }
    Lookup::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_and_plain_shortcuts() {
        let mut chord = None;
        assert_eq!(
            lookup(C, Key::D, &mut chord),
            Lookup::Command(Cmd::Duplicate)
        );
        assert_eq!(
            lookup(CS, Key::S, &mut chord),
            Lookup::Command(Cmd::SaveAll)
        );
        assert_eq!(lookup(C, Key::K, &mut chord), Lookup::ChordStarted(Key::K));
        // Second key with or without Ctrl.
        assert_eq!(lookup(C, Key::C, &mut chord), Lookup::Command(Cmd::Comment));
        assert_eq!(chord, None);
        lookup(C, Key::K, &mut chord);
        assert_eq!(
            lookup(N, Key::U, &mut chord),
            Lookup::Command(Cmd::Uncomment)
        );
        lookup(C, Key::M, &mut chord);
        assert!(matches!(
            lookup(N, Key::Q, &mut chord),
            Lookup::NotACommand(_)
        ));
        assert_eq!(lookup(N, Key::A, &mut chord), Lookup::None);
        assert_eq!(lookup(S, Key::F5, &mut chord), Lookup::Command(Cmd::Stop));
    }

    #[test]
    fn every_command_has_a_shortcut_text() {
        assert_eq!(Cmd::Duplicate.shortcuts(), "Ctrl+D, Ctrl+E, V");
        assert_eq!(Cmd::MoveLinesUp.shortcuts(), "Alt+Up");
        assert_eq!(Cmd::Comment.shortcuts(), "Ctrl+K, C");
        for b in BINDINGS {
            assert!(!b.cmd.label().is_empty());
        }
    }
}
