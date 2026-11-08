//! Control paths: `<Device>/control[/sub]` (`<Keyboard>/w`, `<Mouse>/delta`,
//! `<Gamepad>/leftStick/x`, `<Touchscreen>/primaryTouch/position`), their display names, and
//! reading their values from a frame's [`Input`].

use glam::Vec2;

use super::value::Value;
use crate::input::touch::Touch;
use crate::scripting::Input;

/// Device layouts bindings can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Gamepad,
    Touchscreen,
    /// Mouse or touch, whichever is in use.
    Pointer,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 5] = [
        Self::Keyboard,
        Self::Mouse,
        Self::Gamepad,
        Self::Touchscreen,
        Self::Pointer,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
            Self::Gamepad => "Gamepad",
            Self::Touchscreen => "Touchscreen",
            Self::Pointer => "Pointer",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|d| d.name().eq_ignore_ascii_case(name.trim()))
    }

    /// `<Keyboard>`.
    pub fn layout(self) -> String {
        format!("<{}>", self.name())
    }
}

/// Which part of a 2D control a path reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Whole,
    X,
    Y,
    Up,
    Down,
    Left,
    Right,
}

impl Part {
    fn parse(s: Option<&str>) -> Option<Self> {
        Some(match s.map(str::to_ascii_lowercase).as_deref() {
            None => Part::Whole,
            Some("x") => Part::X,
            Some("y") => Part::Y,
            Some("up") => Part::Up,
            Some("down") => Part::Down,
            Some("left") => Part::Left,
            Some("right") => Part::Right,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Part::Whole => "",
            Part::X => " X",
            Part::Y => " Y",
            Part::Up => " Up",
            Part::Down => " Down",
            Part::Left => " Left",
            Part::Right => " Right",
        }
    }

    fn apply(self, v: Vec2) -> Value {
        match self {
            Part::Whole => Value::Vector2(v),
            Part::X => Value::Float(v.x),
            Part::Y => Value::Float(v.y),
            Part::Up => Value::Float(v.y.max(0.0)),
            Part::Down => Value::Float((-v.y).max(0.0)),
            Part::Left => Value::Float((-v.x).max(0.0)),
            Part::Right => Value::Float(v.x.max(0.0)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stick {
    Left,
    Right,
}

/// A control of a device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// Keyboard keys by their Input Manager names ("left shift" and "right shift" for
    /// `shift`).
    Keys(Vec<&'static str>),
    AnyKey,
    MouseButton(usize),
    MouseDelta(Part),
    MouseScroll(Part),
    MousePosition(Part),
    /// Joystick-layout button index (see `input::gamepad`).
    PadButton(usize),
    /// Joystick-layout axis index of a trigger (8 left, 9 right).
    PadTrigger(usize),
    PadStick(Stick, Part),
    PadDpad(Part),
    /// `touch` = None for the primary touch.
    TouchPosition(Option<usize>, Part),
    TouchDelta(Option<usize>, Part),
    TouchPress(Option<usize>),
    PointerPosition(Part),
    PointerDelta(Part),
    PointerPress,
}

/// Keyboard controls: (control name, Input Manager key name, display name).
pub const KEYS: &[(&str, &str, &str)] = &[
    ("space", "space", "Space"),
    ("enter", "return", "Enter"),
    ("escape", "escape", "Escape"),
    ("tab", "tab", "Tab"),
    ("backspace", "backspace", "Backspace"),
    ("delete", "delete", "Delete"),
    ("insert", "insert", "Insert"),
    ("home", "home", "Home"),
    ("end", "end", "End"),
    ("pageUp", "page up", "Page Up"),
    ("pageDown", "page down", "Page Down"),
    ("upArrow", "up", "Up Arrow"),
    ("downArrow", "down", "Down Arrow"),
    ("leftArrow", "left", "Left Arrow"),
    ("rightArrow", "right", "Right Arrow"),
    ("leftShift", "left shift", "Left Shift"),
    ("rightShift", "right shift", "Right Shift"),
    ("leftCtrl", "left ctrl", "Left Ctrl"),
    ("rightCtrl", "right ctrl", "Right Ctrl"),
    ("leftAlt", "left alt", "Left Alt"),
    ("rightAlt", "right alt", "Right Alt"),
    ("minus", "-", "Minus"),
    ("equals", "=", "Equals"),
    ("comma", ",", "Comma"),
    ("period", ".", "Period"),
    ("slash", "/", "Slash"),
    ("backslash", "\\", "Backslash"),
    ("semicolon", ";", "Semicolon"),
    ("quote", "'", "Quote"),
    ("leftBracket", "[", "Left Bracket"),
    ("rightBracket", "]", "Right Bracket"),
    ("backquote", "`", "Backquote"),
];

const LETTERS: &str = "abcdefghijklmnopqrstuvwxyz";

/// Gamepad buttons: (control name, joystick button index, display name).
pub const PAD_BUTTONS: &[(&str, usize, &str)] = &[
    ("buttonSouth", 0, "Button South"),
    ("buttonEast", 1, "Button East"),
    ("buttonWest", 2, "Button West"),
    ("buttonNorth", 3, "Button North"),
    ("leftShoulder", 4, "Left Shoulder"),
    ("rightShoulder", 5, "Right Shoulder"),
    ("select", 6, "Select"),
    ("start", 7, "Start"),
    ("leftStickPress", 8, "Left Stick Press"),
    ("rightStickPress", 9, "Right Stick Press"),
];

/// Other names for gamepad buttons (Xbox and PlayStation letters).
const PAD_ALIASES: &[(&str, &str)] = &[
    ("a", "buttonSouth"),
    ("cross", "buttonSouth"),
    ("b", "buttonEast"),
    ("circle", "buttonEast"),
    ("x", "buttonWest"),
    ("square", "buttonWest"),
    ("y", "buttonNorth"),
    ("triangle", "buttonNorth"),
];

/// A parsed control path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlPath {
    pub device: DeviceKind,
    pub control: Control,
}

/// Is `s` a function key name ("f1" … "f24")?
fn function_key(s: &str) -> Option<u32> {
    let n: u32 = s.strip_prefix(['f', 'F'])?.parse().ok()?;
    (1..=24).contains(&n).then_some(n)
}

/// Static key name for a letter, digit or function key ("w", "3", "f5").
fn leak_name(s: String) -> &'static str {
    // A small fixed set (letters, digits, f1–f24), so interning by lookup is enough.
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let mut v: Vec<String> = LETTERS.chars().map(String::from).collect();
        v.extend((0..10).map(|d| d.to_string()));
        v.extend((1..=24).map(|n| format!("f{n}")));
        v
    });
    names.iter().find(|n| **n == s).map_or("", |n| n.as_str())
}

impl ControlPath {
    /// Parse a path; errors name what's wrong.
    pub fn parse(path: &str) -> Result<Self, String> {
        let p = path.trim();
        let rest = p
            .strip_prefix('<')
            .ok_or_else(|| format!("'{p}': a path starts with <Device>"))?;
        let close = rest
            .find('>')
            .ok_or_else(|| format!("'{p}': missing '>'"))?;
        let device = DeviceKind::parse(&rest[..close])
            .ok_or_else(|| format!("'{p}': unknown device '{}'", &rest[..close]))?;
        let mut after = &rest[close + 1..];
        // `{usage}` / `{n}` qualifiers are accepted and ignored.
        if let Some(q) = after.strip_prefix('{') {
            after = q.split_once('}').map_or("", |(_, r)| r);
        }
        let segs: Vec<&str> = after
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let unknown = || format!("'{p}': no such control on {}", device.name());
        let first = segs.first().copied().ok_or_else(unknown)?;
        let sub = segs.get(1).copied();
        let lower = first.to_ascii_lowercase();
        let control = match device {
            DeviceKind::Keyboard => {
                if segs.len() > 1 {
                    return Err(unknown());
                }
                match lower.as_str() {
                    "anykey" => Control::AnyKey,
                    "shift" => Control::Keys(vec!["left shift", "right shift"]),
                    "ctrl" => Control::Keys(vec!["left ctrl", "right ctrl"]),
                    "alt" => Control::Keys(vec!["left alt", "right alt"]),
                    l if l.len() == 1 && l.chars().all(|c| c.is_ascii_alphanumeric()) => {
                        Control::Keys(vec![leak_name(l.to_owned())])
                    }
                    l if function_key(l).is_some() => Control::Keys(vec![leak_name(l.to_owned())]),
                    l => {
                        let k = KEYS
                            .iter()
                            .find(|(c, _, _)| c.eq_ignore_ascii_case(l))
                            .ok_or_else(unknown)?;
                        Control::Keys(vec![k.1])
                    }
                }
            }
            DeviceKind::Mouse => {
                let part = Part::parse(sub).ok_or_else(unknown)?;
                match lower.as_str() {
                    "leftbutton" | "press" => Control::MouseButton(0),
                    "rightbutton" => Control::MouseButton(1),
                    "middlebutton" => Control::MouseButton(2),
                    "delta" => Control::MouseDelta(part),
                    "scroll" => Control::MouseScroll(part),
                    "position" => Control::MousePosition(part),
                    _ => return Err(unknown()),
                }
            }
            DeviceKind::Gamepad => {
                let part = Part::parse(sub).ok_or_else(unknown)?;
                let name = PAD_ALIASES
                    .iter()
                    .find(|(a, _)| *a == lower)
                    .map_or(first, |(_, n)| *n);
                if let Some((_, i, _)) = PAD_BUTTONS
                    .iter()
                    .find(|(c, _, _)| c.eq_ignore_ascii_case(name))
                {
                    Control::PadButton(*i)
                } else {
                    match lower.as_str() {
                        "lefttrigger" => Control::PadTrigger(8),
                        "righttrigger" => Control::PadTrigger(9),
                        "leftstick" => Control::PadStick(Stick::Left, part),
                        "rightstick" => Control::PadStick(Stick::Right, part),
                        "dpad" => Control::PadDpad(part),
                        _ => return Err(unknown()),
                    }
                }
            }
            DeviceKind::Touchscreen => {
                // `primaryTouch/position`, `touch1/press`, or `position` / `press` / `delta`
                // (the primary touch).
                let (touch, ctrl, part) = if lower == "primarytouch" || lower.starts_with("touch") {
                    let touch = if lower == "primarytouch" {
                        None
                    } else {
                        Some(lower[5..].parse::<usize>().map_err(|_| unknown())?)
                    };
                    let ctrl = sub.ok_or_else(unknown)?.to_ascii_lowercase();
                    (
                        touch,
                        ctrl,
                        Part::parse(segs.get(2).copied()).ok_or_else(unknown)?,
                    )
                } else {
                    (None, lower.clone(), Part::parse(sub).ok_or_else(unknown)?)
                };
                match ctrl.as_str() {
                    "position" => Control::TouchPosition(touch, part),
                    "delta" => Control::TouchDelta(touch, part),
                    "press" | "tap" => Control::TouchPress(touch),
                    _ => return Err(unknown()),
                }
            }
            DeviceKind::Pointer => {
                let part = Part::parse(sub).ok_or_else(unknown)?;
                match lower.as_str() {
                    "position" => Control::PointerPosition(part),
                    "delta" => Control::PointerDelta(part),
                    "press" => Control::PointerPress,
                    _ => return Err(unknown()),
                }
            }
        };
        Ok(Self { device, control })
    }

    /// Does this control give a 2D vector?
    pub fn is_vector(&self) -> bool {
        use Control::*;
        matches!(
            self.control,
            MouseDelta(Part::Whole)
                | MouseScroll(Part::Whole)
                | MousePosition(Part::Whole)
                | PadStick(_, Part::Whole)
                | PadDpad(Part::Whole)
                | TouchPosition(_, Part::Whole)
                | TouchDelta(_, Part::Whole)
                | PointerPosition(Part::Whole)
                | PointerDelta(Part::Whole)
        )
    }

    /// Pointer positions and deltas: never picked up by Listen or rebinding (they move all
    /// the time).
    pub fn is_noisy(&self) -> bool {
        use Control::*;
        matches!(
            self.control,
            MouseDelta(_)
                | MousePosition(_)
                | TouchPosition(..)
                | TouchDelta(..)
                | PointerPosition(_)
                | PointerDelta(_)
        )
    }

    /// The control's own name ("W", "Left Stick", "Delta").
    pub fn control_name(&self) -> String {
        use Control::*;
        let touch = |t: &Option<usize>| match t {
            None => "Primary Touch ".to_owned(),
            Some(i) => format!("Touch #{i} "),
        };
        match &self.control {
            Keys(k) => match k.as_slice() {
                ["left shift", "right shift"] => "Shift".into(),
                ["left ctrl", "right ctrl"] => "Ctrl".into(),
                ["left alt", "right alt"] => "Alt".into(),
                [one] => KEYS
                    .iter()
                    .find(|(_, legacy, _)| legacy == one)
                    .map_or_else(|| one.to_uppercase(), |(_, _, d)| (*d).to_owned()),
                _ => "Key".into(),
            },
            AnyKey => "Any Key".into(),
            MouseButton(0) => "Left Button".into(),
            MouseButton(1) => "Right Button".into(),
            MouseButton(_) => "Middle Button".into(),
            MouseDelta(p) => format!("Delta{}", p.label()),
            MouseScroll(p) => format!("Scroll{}", p.label()),
            MousePosition(p) => format!("Position{}", p.label()),
            PadButton(i) => PAD_BUTTONS
                .iter()
                .find(|(_, b, _)| b == i)
                .map_or("Button", |(_, _, d)| d)
                .to_owned(),
            PadTrigger(8) => "Left Trigger".into(),
            PadTrigger(_) => "Right Trigger".into(),
            PadStick(Stick::Left, p) => format!("Left Stick{}", p.label()),
            PadStick(Stick::Right, p) => format!("Right Stick{}", p.label()),
            PadDpad(p) => format!("D-Pad{}", p.label()),
            TouchPosition(t, p) => format!("{}Position{}", touch(t), p.label()),
            TouchDelta(t, p) => format!("{}Delta{}", touch(t), p.label()),
            TouchPress(t) => format!("{}Press", touch(t)),
            PointerPosition(p) => format!("Position{}", p.label()),
            PointerDelta(p) => format!("Delta{}", p.label()),
            PointerPress => "Press".into(),
        }
    }

    /// "W [Keyboard]".
    pub fn display(&self) -> String {
        format!("{} [{}]", self.control_name(), self.device.name())
    }

    /// Value of this control in `input`, reading only the devices `filter` allows.
    pub fn read(&self, input: &Input, filter: &DeviceFilter) -> Value {
        use Control::*;
        let f = |b: bool| Value::Float(if b { 1.0 } else { 0.0 });
        match &self.control {
            Keys(keys) if filter.keyboard => f(keys.iter().any(|k| input.held.contains(*k))),
            AnyKey if filter.keyboard => f(!input.held.is_empty()),
            MouseButton(i) if filter.mouse => f(input.mouse_held.get(*i).copied().unwrap_or(false)),
            MouseDelta(p) if filter.mouse => p.apply(mouse_delta(input)),
            MouseScroll(p) if filter.mouse => p.apply(input.scroll_notches()),
            MousePosition(p) if filter.mouse => p.apply(mouse_position(input)),
            PadButton(i) => strongest(filter.pads(input).map(|j| f(j.held[*i]))),
            PadTrigger(a) => strongest(filter.pads(input).map(|j| Value::Float(j.axes[*a]))),
            PadStick(s, p) => strongest(filter.pads(input).map(|j| p.apply(stick(j, *s)))),
            PadDpad(p) => strongest(filter.pads(input).map(|j| p.apply(dpad(j)))),
            TouchPosition(t, p) if filter.touch => {
                p.apply(touch(input, *t).map_or(Vec2::ZERO, |t| flip(input, t.position)))
            }
            TouchDelta(t, p) if filter.touch => {
                p.apply(touch(input, *t).map_or(Vec2::ZERO, |t| Vec2::new(t.delta.x, -t.delta.y)))
            }
            TouchPress(t) if filter.touch => f(touch(input, *t).is_some_and(|t| t.phase.is_down())),
            PointerPosition(p) => p.apply(match pointer_touch(input, filter) {
                Some(t) => flip(input, t.position),
                None if filter.mouse => mouse_position(input),
                None => Vec2::ZERO,
            }),
            PointerDelta(p) => p.apply(match pointer_touch(input, filter) {
                Some(t) => Vec2::new(t.delta.x, -t.delta.y),
                None if filter.mouse => mouse_delta(input),
                None => Vec2::ZERO,
            }),
            PointerPress => f(
                pointer_touch(input, filter).is_some_and(|t| t.phase.is_down())
                    || (filter.mouse && input.mouse_held[0]),
            ),
            // A device the filter leaves out reads as its default.
            _ => {
                if self.is_vector() {
                    Value::Vector2(Vec2::ZERO)
                } else {
                    Value::Float(0.0)
                }
            }
        }
    }
}

/// Mouse movement this frame, y up.
fn mouse_delta(input: &Input) -> Vec2 {
    Vec2::new(input.mouse_delta.x, -input.mouse_delta.y)
}

/// Screen pixels from the bottom-left (like `Input.mousePosition`).
fn mouse_position(input: &Input) -> Vec2 {
    input.mouse_position.map_or(Vec2::ZERO, |p| flip(input, p))
}

fn flip(input: &Input, p: Vec2) -> Vec2 {
    Vec2::new(p.x, input.screen_size.y - p.y)
}

fn stick(j: &crate::input::Joystick, s: Stick) -> Vec2 {
    match s {
        // Joystick axes are +1 down; gamepad sticks read +1 up.
        Stick::Left => Vec2::new(j.axes[0], -j.axes[1]),
        Stick::Right => Vec2::new(j.axes[3], -j.axes[4]),
    }
}

fn dpad(j: &crate::input::Joystick) -> Vec2 {
    let b = |i: usize| if j.held[i] { 1.0 } else { 0.0 };
    let from_buttons = Vec2::new(b(14) - b(13), b(11) - b(12));
    if from_buttons != Vec2::ZERO {
        from_buttons
    } else {
        Vec2::new(j.axes[5], j.axes[6])
    }
}

fn touch(input: &Input, t: Option<usize>) -> Option<&Touch> {
    match t {
        None => input
            .touches
            .iter()
            .find(|t| t.phase.is_down())
            .or(input.touches.first()),
        Some(i) => input.touches.get(i),
    }
}

fn pointer_touch<'a>(input: &'a Input, filter: &DeviceFilter) -> Option<&'a Touch> {
    if filter.touch {
        touch(input, None)
    } else {
        None
    }
}

/// The value with the largest magnitude (gamepads: the most actuated pad wins).
fn strongest(values: impl Iterator<Item = Value>) -> Value {
    let mut best: Option<Value> = None;
    for v in values {
        if best.is_none_or(|b| v.magnitude() > b.magnitude()) {
            best = Some(v);
        }
    }
    best.unwrap_or_default()
}

/// Which devices bindings may read (a player's paired devices).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceFilter {
    pub keyboard: bool,
    pub mouse: bool,
    pub touch: bool,
    /// Joystick slots (0-based); None = every connected gamepad.
    pub gamepads: Option<Vec<usize>>,
}

impl Default for DeviceFilter {
    fn default() -> Self {
        Self::all()
    }
}

impl DeviceFilter {
    pub fn all() -> Self {
        Self {
            keyboard: true,
            mouse: true,
            touch: true,
            gamepads: None,
        }
    }

    pub fn none() -> Self {
        Self {
            keyboard: false,
            mouse: false,
            touch: false,
            gamepads: Some(Vec::new()),
        }
    }

    /// Allowed, connected gamepads.
    pub fn pads<'a>(
        &'a self,
        input: &'a Input,
    ) -> impl Iterator<Item = &'a crate::input::Joystick> {
        input
            .joysticks
            .iter()
            .enumerate()
            .filter(move |(i, j)| {
                !j.name.is_empty() && self.gamepads.as_ref().is_none_or(|g| g.contains(i))
            })
            .map(|(_, j)| j)
    }
}

/// Every control the editor's path picker lists for a device: (path, display name).
pub fn controls(device: DeviceKind) -> Vec<(String, String)> {
    let mut names: Vec<String> = Vec::new();
    match device {
        DeviceKind::Keyboard => {
            names.extend(LETTERS.chars().map(String::from));
            names.extend((0..10).map(|d| d.to_string()));
            names.extend(KEYS.iter().map(|(c, _, _)| (*c).to_owned()));
            names.extend((1..=12).map(|n| format!("f{n}")));
            names.extend(["shift", "ctrl", "alt", "anyKey"].map(String::from));
        }
        DeviceKind::Mouse => {
            for c in [
                "leftButton",
                "rightButton",
                "middleButton",
                "delta",
                "scroll",
            ] {
                names.push(c.into());
            }
            for c in ["delta/x", "delta/y", "scroll/y", "position"] {
                names.push(c.into());
            }
        }
        DeviceKind::Gamepad => {
            names.extend(PAD_BUTTONS.iter().map(|(c, _, _)| (*c).to_owned()));
            for c in [
                "leftTrigger",
                "rightTrigger",
                "leftStick",
                "rightStick",
                "dpad",
            ] {
                names.push(c.into());
            }
            for stick in ["leftStick", "rightStick", "dpad"] {
                for p in ["up", "down", "left", "right"] {
                    names.push(format!("{stick}/{p}"));
                }
            }
            names.extend(
                ["leftStick/x", "leftStick/y", "rightStick/x", "rightStick/y"].map(String::from),
            );
        }
        DeviceKind::Touchscreen => {
            for c in [
                "primaryTouch/press",
                "primaryTouch/position",
                "primaryTouch/delta",
            ] {
                names.push(c.into());
            }
        }
        DeviceKind::Pointer => {
            names.extend(["press", "position", "delta"].map(String::from));
        }
    }
    names
        .into_iter()
        .map(|n| {
            let path = format!("{}/{n}", device.layout());
            let label = ControlPath::parse(&path).map_or(n.clone(), |c| c.control_name());
            (path, label)
        })
        .collect()
}

/// Display name of a path for binding lists ("W [Keyboard]"); the text itself when it
/// doesn't parse, "<No Binding>" when empty.
pub fn display_path(path: &str) -> String {
    if path.trim().is_empty() {
        return "<No Binding>".into();
    }
    ControlPath::parse(path).map_or_else(|_| path.to_owned(), |c| c.display())
}

/// Short display name ("W", "Button South").
pub fn short_name(path: &str) -> String {
    if path.trim().is_empty() {
        return String::new();
    }
    ControlPath::parse(path).map_or_else(|_| path.to_owned(), |c| c.control_name())
}

/// `<Keyboard>/leftShift` for the Input Manager key name "left shift" (the editor's Listen
/// button and rebinding turn key presses into paths with it).
pub fn keyboard_path(key: &str) -> Option<String> {
    let k = key.trim().to_ascii_lowercase();
    if let Some((c, _, _)) = KEYS.iter().find(|(_, legacy, _)| *legacy == k) {
        return Some(format!("<Keyboard>/{c}"));
    }
    let simple = (k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()))
        || function_key(&k).is_some();
    simple.then(|| format!("<Keyboard>/{k}"))
}

/// The path of a gamepad joystick button index (`<Gamepad>/buttonSouth` for 0, the d-pad for
/// 11–14).
pub fn pad_button_path(index: usize) -> Option<String> {
    if let Some((c, _, _)) = PAD_BUTTONS.iter().find(|(_, i, _)| *i == index) {
        return Some(format!("<Gamepad>/{c}"));
    }
    let dir = match index {
        11 => "up",
        12 => "down",
        13 => "left",
        14 => "right",
        _ => return None,
    };
    Some(format!("<Gamepad>/dpad/{dir}"))
}

/// Normalised path text (`<keyboard>/LEFTSHIFT` → `<Keyboard>/leftShift`) when it parses.
pub fn normalize(path: &str) -> String {
    let Ok(c) = ControlPath::parse(path) else {
        return path.trim().to_owned();
    };
    let Some(slash) = path.find('>') else {
        return path.trim().to_owned();
    };
    let rest = path[slash + 1..].trim_start_matches('/');
    let list = controls(c.device);
    let lower = rest.to_ascii_lowercase();
    list.into_iter()
        .map(|(p, _)| p)
        .find(|p| {
            p.split_once('>')
                .is_some_and(|(_, r)| r.trim_start_matches('/').to_ascii_lowercase() == lower)
        })
        .unwrap_or_else(|| format!("{}/{rest}", c.device.layout()))
}
