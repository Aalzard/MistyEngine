//! The Input Actions asset (`.inputactions`, JSON): action maps, their actions, each action's
//! bindings (plain control paths or composites made of parts), and the control schemes that
//! group bindings by device.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// File extension of Input Actions assets.
pub const EXTENSION: &str = "inputactions";

/// Is `path` an Input Actions asset?
pub fn is_actions_path(path: &str) -> bool {
    path.to_ascii_lowercase()
        .ends_with(&format!(".{EXTENSION}"))
}

/// How an action turns control values into started / performed / canceled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType {
    /// Continuous value; the most actuated control drives it. Started and performed when it
    /// leaves its default, performed again on every change, canceled back at the default.
    #[default]
    Value,
    /// Performed when pressed past the press point, canceled on release.
    Button,
    /// Every change of any bound control is performed, with no conflict resolution.
    PassThrough,
}

impl ActionType {
    pub const ALL: [ActionType; 3] = [Self::Value, Self::Button, Self::PassThrough];

    pub fn label(self) -> &'static str {
        match self {
            Self::Value => "Value",
            Self::Button => "Button",
            Self::PassThrough => "Pass Through",
        }
    }
}

/// The value type an action reads (`ReadValue`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlType {
    /// Whatever the driving control gives.
    #[default]
    Any,
    Button,
    Axis,
    Vector2,
}

impl ControlType {
    pub const ALL: [ControlType; 4] = [Self::Any, Self::Button, Self::Axis, Self::Vector2];

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Button => "Button",
            Self::Axis => "Axis",
            Self::Vector2 => "Vector 2",
        }
    }
}

/// One part of a composite ("up" = `<Keyboard>/w`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompositePart {
    /// Part name the composite reads ("up", "negative", "modifier", "button", …).
    pub name: String,
    pub path: String,
    /// Stable id (binding overrides and rebinding refer to it).
    pub id: String,
    /// Control schemes (binding groups) using this part, `;`-separated.
    pub groups: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub processors: String,
}

/// A binding of an action: one control path, or a composite of parts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    /// Shown in the editor for composites ("WASD"); empty for plain bindings.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Control path (`<Keyboard>/space`); empty for composites.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// Composite type (`2DVector`, `1DAxis`, `ButtonWithOneModifier`, …); empty for a plain
    /// binding.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub composite: String,
    /// Composite parameters (`mode=DigitalNormalized`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub parameters: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<CompositePart>,
    pub id: String,
    /// Control schemes using this binding, `;`-separated (composites: see the parts).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub groups: String,
    /// `hold(duration=0.5),tap`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub interactions: String,
    /// `invert,scale(factor=2)`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub processors: String,
}

impl Binding {
    pub fn is_composite(&self) -> bool {
        !self.composite.is_empty()
    }

    /// A plain binding to `path`.
    pub fn to(path: &str, groups: &str) -> Self {
        Self {
            path: path.into(),
            groups: groups.into(),
            ..Default::default()
        }
    }

    /// A composite with `(part, path)` pairs, all in `groups`.
    pub fn composite(name: &str, kind: &str, parts: &[(&str, &str)], groups: &str) -> Self {
        Self {
            name: name.into(),
            composite: kind.into(),
            parts: parts
                .iter()
                .map(|(part, path)| CompositePart {
                    name: (*part).into(),
                    path: (*path).into(),
                    groups: groups.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    /// Is this binding (or any part) in the control scheme group `group`?
    pub fn in_group(&self, group: &str) -> bool {
        if self.is_composite() {
            self.parts.iter().any(|p| has_group(&p.groups, group))
        } else {
            has_group(&self.groups, group)
        }
    }
}

/// Does the `;`-separated list `groups` contain `group`?
pub fn has_group(groups: &str, group: &str) -> bool {
    groups.split(';').any(|g| g.trim() == group)
}

/// Add or remove `group` in the `;`-separated list `groups`.
pub fn set_group(groups: &mut String, group: &str, on: bool) {
    let mut list: Vec<&str> = groups
        .split(';')
        .map(str::trim)
        .filter(|g| !g.is_empty() && *g != group)
        .collect();
    if on {
        list.push(group);
    }
    *groups = list.join(";");
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Action {
    pub name: String,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ActionType,
    pub control_type: ControlType,
    /// Value actions check the controls' state when enabled (always on for Value actions;
    /// kept for the editor).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub initial_state_check: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub interactions: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub processors: String,
    pub bindings: Vec<Binding>,
}

impl Action {
    pub fn new(name: &str, kind: ActionType, control_type: ControlType) -> Self {
        Self {
            name: name.into(),
            kind,
            control_type,
            initial_state_check: kind == ActionType::Value,
            ..Default::default()
        }
    }

    pub fn with(mut self, binding: Binding) -> Self {
        self.bindings.push(binding);
        self
    }

    /// Bindings flattened like the scripting API numbers them: a composite, then its parts.
    pub fn flat_bindings(&self) -> Vec<FlatBinding> {
        let mut out = Vec::new();
        for (b, binding) in self.bindings.iter().enumerate() {
            out.push(FlatBinding {
                binding: b,
                part: None,
            });
            for p in 0..binding.parts.len() {
                out.push(FlatBinding {
                    binding: b,
                    part: Some(p),
                });
            }
        }
        out
    }
}

/// A binding (or one composite part) by position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatBinding {
    pub binding: usize,
    pub part: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionMap {
    pub name: String,
    pub id: String,
    pub actions: Vec<Action>,
}

/// A device a control scheme uses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceRequirement {
    /// `<Keyboard>`, `<Mouse>`, `<Gamepad>`, `<Touchscreen>`.
    pub path: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

/// A named set of devices ("Keyboard&Mouse", "Gamepad"); bindings join it through their
/// groups.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlScheme {
    pub name: String,
    /// Group name bindings use (defaults to the name).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub binding_group: String,
    pub devices: Vec<DeviceRequirement>,
}

impl ControlScheme {
    pub fn new(name: &str, devices: &[(&str, bool)]) -> Self {
        Self {
            name: name.into(),
            binding_group: String::new(),
            devices: devices
                .iter()
                .map(|(path, optional)| DeviceRequirement {
                    path: (*path).into(),
                    optional: *optional,
                })
                .collect(),
        }
    }

    /// The group bindings name to join this scheme.
    pub fn group(&self) -> &str {
        if self.binding_group.is_empty() {
            &self.name
        } else {
            &self.binding_group
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputActionAsset {
    pub name: String,
    pub maps: Vec<ActionMap>,
    pub control_schemes: Vec<ControlScheme>,
}

impl InputActionAsset {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut a: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        a.ensure_ids();
        Ok(a)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Read an asset file (from a mounted pak in builds).
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_json() + "\n").map_err(|e| e.to_string())
    }

    /// Give every map, action, binding and part without an id a stable one (derived from its
    /// position and names, so loading the same file twice gives the same ids).
    pub fn ensure_ids(&mut self) {
        for (m, map) in self.maps.iter_mut().enumerate() {
            if map.id.is_empty() {
                map.id = stable_id(&[&map.name, &m.to_string()]);
            }
            for (a, action) in map.actions.iter_mut().enumerate() {
                if action.id.is_empty() {
                    action.id = stable_id(&[&map.name, &action.name, &a.to_string()]);
                }
                for (b, binding) in action.bindings.iter_mut().enumerate() {
                    let base = [map.name.as_str(), action.name.as_str()];
                    if binding.id.is_empty() {
                        let what = format!("{b}{}{}", binding.path, binding.composite);
                        binding.id = stable_id(&[base[0], base[1], &what]);
                    }
                    for (p, part) in binding.parts.iter_mut().enumerate() {
                        if part.id.is_empty() {
                            let what = format!("{b}.{p}{}", part.name);
                            part.id = stable_id(&[base[0], base[1], &what]);
                        }
                    }
                }
            }
        }
    }

    pub fn find_map(&self, name: &str) -> Option<usize> {
        self.maps
            .iter()
            .position(|m| m.name.eq_ignore_ascii_case(name) || m.id == name)
    }

    /// An action by `"Map/Action"`, `"Action"` (first map that has it) or id.
    pub fn find_action(&self, name: &str) -> Option<(usize, usize)> {
        if let Some((map, action)) = name.split_once('/') {
            let m = self.find_map(map)?;
            let a = self.maps[m]
                .actions
                .iter()
                .position(|x| x.name.eq_ignore_ascii_case(action))?;
            return Some((m, a));
        }
        self.maps.iter().enumerate().find_map(|(m, map)| {
            map.actions
                .iter()
                .position(|x| x.name.eq_ignore_ascii_case(name) || x.id == name)
                .map(|a| (m, a))
        })
    }

    pub fn find_scheme(&self, name: &str) -> Option<usize> {
        self.control_schemes
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The starter asset Assets ▸ Create ▸ Input Actions makes: a Player map (Move, Look,
    /// Attack, Jump, Sprint, Crouch, Interact) and a UI map (Navigate, Submit, Cancel, Point,
    /// Click) for keyboard and mouse, gamepads and touch.
    pub fn starter(name: &str) -> Self {
        let mut a = Self {
            name: name.into(),
            maps: vec![player_map(), ui_map()],
            control_schemes: default_schemes(),
        };
        a.ensure_ids();
        a
    }
}

/// Keyboard&Mouse, Gamepad and Touch.
pub fn default_schemes() -> Vec<ControlScheme> {
    vec![
        ControlScheme::new(KEYBOARD_MOUSE, &[("<Keyboard>", false), ("<Mouse>", false)]),
        ControlScheme::new(GAMEPAD, &[("<Gamepad>", false)]),
        ControlScheme::new(TOUCH, &[("<Touchscreen>", false)]),
    ]
}

pub const KEYBOARD_MOUSE: &str = "Keyboard&Mouse";
pub const GAMEPAD: &str = "Gamepad";
pub const TOUCH: &str = "Touch";

fn player_map() -> ActionMap {
    const KM: &str = KEYBOARD_MOUSE;
    const GP: &str = GAMEPAD;
    let wasd = Binding::composite(
        "WASD",
        "2DVector",
        &[
            ("up", "<Keyboard>/w"),
            ("down", "<Keyboard>/s"),
            ("left", "<Keyboard>/a"),
            ("right", "<Keyboard>/d"),
        ],
        KM,
    );
    let arrows = Binding::composite(
        "Arrows",
        "2DVector",
        &[
            ("up", "<Keyboard>/upArrow"),
            ("down", "<Keyboard>/downArrow"),
            ("left", "<Keyboard>/leftArrow"),
            ("right", "<Keyboard>/rightArrow"),
        ],
        KM,
    );
    let mut stick = Binding::to("<Gamepad>/leftStick", GP);
    stick.processors = "stickDeadzone".into();
    let mut look_pad = Binding::to("<Gamepad>/rightStick", GP);
    look_pad.processors = "stickDeadzone".into();
    ActionMap {
        name: "Player".into(),
        id: String::new(),
        actions: vec![
            Action::new("Move", ActionType::Value, ControlType::Vector2)
                .with(wasd)
                .with(arrows)
                .with(stick),
            Action::new("Look", ActionType::Value, ControlType::Vector2)
                .with(Binding::to("<Mouse>/delta", KM))
                .with(look_pad),
            Action::new("Attack", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Mouse>/leftButton", KM))
                .with(Binding::to("<Gamepad>/buttonWest", GP))
                .with(Binding::to("<Touchscreen>/primaryTouch/tap", TOUCH)),
            Action::new("Jump", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/space", KM))
                .with(Binding::to("<Gamepad>/buttonSouth", GP)),
            Action::new("Sprint", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/leftShift", KM))
                .with(Binding::to("<Gamepad>/leftStickPress", GP)),
            Action::new("Crouch", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/c", KM))
                .with(Binding::to("<Gamepad>/buttonEast", GP)),
            Action::new("Interact", ActionType::Button, ControlType::Button)
                .with({
                    let mut b = Binding::to("<Keyboard>/e", KM);
                    b.interactions = "hold".into();
                    b
                })
                .with({
                    let mut b = Binding::to("<Gamepad>/buttonNorth", GP);
                    b.interactions = "hold".into();
                    b
                }),
        ],
    }
}

fn ui_map() -> ActionMap {
    const KM: &str = KEYBOARD_MOUSE;
    const GP: &str = GAMEPAD;
    ActionMap {
        name: "UI".into(),
        id: String::new(),
        actions: vec![
            Action::new("Navigate", ActionType::PassThrough, ControlType::Vector2)
                .with(Binding::composite(
                    "Arrows",
                    "2DVector",
                    &[
                        ("up", "<Keyboard>/upArrow"),
                        ("down", "<Keyboard>/downArrow"),
                        ("left", "<Keyboard>/leftArrow"),
                        ("right", "<Keyboard>/rightArrow"),
                    ],
                    KM,
                ))
                .with(Binding::to("<Gamepad>/dpad", GP))
                .with(Binding::to("<Gamepad>/leftStick", GP)),
            Action::new("Submit", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/enter", KM))
                .with(Binding::to("<Gamepad>/buttonSouth", GP)),
            Action::new("Cancel", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/escape", KM))
                .with(Binding::to("<Gamepad>/buttonEast", GP)),
            Action::new("Point", ActionType::PassThrough, ControlType::Vector2)
                .with(Binding::to("<Mouse>/position", KM))
                .with(Binding::to("<Touchscreen>/primaryTouch/position", TOUCH)),
            Action::new("Click", ActionType::PassThrough, ControlType::Button)
                .with(Binding::to("<Mouse>/leftButton", KM))
                .with(Binding::to("<Touchscreen>/primaryTouch/press", TOUCH)),
        ],
    }
}

/// A short hex id from `parts` (FNV-1a, 64 bits).
pub fn stable_id(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain([0x1f]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// A fresh random id for something added in the editor.
pub fn new_id() -> String {
    format!("{}", crate::asset_db::Guid::new())[..16].to_owned()
}
