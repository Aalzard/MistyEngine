//! The UI input module: the "UI" action map (Navigate, Submit, Cancel) the UI event system reads
//! for keyboard and gamepad navigation. It comes from the Event System's actions asset, or the
//! default UI actions (arrows / Return / Esc, gamepad stick / d-pad / South / East) when none is
//! set, and runs as its own action set beside the scripts' ones.

use glam::Vec2;

use super::asset::InputActionAsset;
use super::state::ActionSet;
use super::system::InputSystem;

/// What the UI reads from the Input System in one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiActions {
    /// Navigate (y up).
    pub navigate: Vec2,
    pub submit: bool,
    pub cancel: bool,
}

/// A running UI map.
pub struct UiModule {
    /// The actions asset it was made from (`None` = the default UI actions).
    pub asset: Option<String>,
    pub set: ActionSet,
    navigate: Option<(usize, usize)>,
    submit: Option<(usize, usize)>,
    cancel: Option<(usize, usize)>,
}

impl UiModule {
    /// The UI map of `asset` (the starter asset's when `None`), enabled.
    pub fn new(asset: Option<String>, actions: InputActionAsset) -> Self {
        let mut set = ActionSet::new(actions, asset.as_deref().unwrap_or(""));
        if let Some(m) = set.asset.find_map("UI") {
            set.set_map_enabled(m, true);
        }
        let find = |name: &str| set.asset.find_action(&format!("UI/{name}"));
        let (navigate, submit, cancel) = (find("Navigate"), find("Submit"), find("Cancel"));
        Self {
            asset,
            set,
            navigate,
            submit,
            cancel,
        }
    }

    /// This frame's Navigate value and Submit / Cancel presses.
    pub fn read(&self) -> UiActions {
        let state = |a: Option<(usize, usize)>| a.and_then(|(m, i)| self.set.state(m, i));
        UiActions {
            navigate: state(self.navigate).map_or(Vec2::ZERO, |s| s.value.as_vec2()),
            submit: state(self.submit).is_some_and(|s| s.pressed_this_frame),
            cancel: state(self.cancel).is_some_and(|s| s.pressed_this_frame),
        }
    }
}

impl InputSystem {
    /// The UI map's actions this frame, for the Event System whose actions asset is `asset`
    /// (`None` = the default UI actions). `None` while Active Input Handling is the Input
    /// Manager: the UI then reads the keys and joystick itself. The module starts (or changes
    /// asset) here and is stepped with the other action sets from the next frame on.
    pub fn ui_actions(&mut self, asset: Option<&str>) -> Option<UiActions> {
        if !self.handling.actions_allowed() {
            return None;
        }
        let key = asset
            .map(|a| a.replace('\\', "/"))
            .filter(|a| !a.is_empty());
        if self.ui_module.as_ref().is_none_or(|m| m.asset != key) {
            let actions = match &key {
                Some(path) => match self.load_asset(path) {
                    Ok(a) => a,
                    Err(e) => {
                        self.logs.push(format!(
                            "Event System actions asset {path}: {e}; using the default UI actions"
                        ));
                        InputActionAsset::starter("DefaultInputActions")
                    }
                },
                None => InputActionAsset::starter("DefaultInputActions"),
            };
            self.ui_module = Some(UiModule::new(key, actions));
        }
        self.ui_module.as_ref().map(UiModule::read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::actions::ActiveInputHandling;
    use crate::scene::Scene;
    use crate::scripting::Input;

    fn frame(sys: &mut InputSystem, input: &Input) -> UiActions {
        sys.update(&Scene::new("UI"), input, 1.0 / 60.0);
        sys.ui_actions(None).unwrap()
    }

    #[test]
    fn default_ui_map_reads_arrows_return_escape_and_the_pad() {
        let mut sys = InputSystem::new(std::path::Path::new("."), ActiveInputHandling::Both);
        let idle = Input::default();
        assert_eq!(frame(&mut sys, &idle), UiActions::default());
        let mut down = Input::default();
        down.held.insert("down".into());
        down.pressed.insert("down".into());
        assert_eq!(frame(&mut sys, &down).navigate, Vec2::new(0.0, -1.0));
        let mut ret = Input::default();
        ret.held.insert("return".into());
        ret.pressed.insert("return".into());
        let a = frame(&mut sys, &ret);
        assert!(a.submit && !a.cancel);
        // Held, it doesn't press again.
        assert!(!frame(&mut sys, &ret).submit);
        // Gamepad: d-pad right and East (cancel).
        let mut pad = Input::default();
        let mut j = crate::input::Joystick {
            name: "Pad".into(),
            ..Default::default()
        };
        j.axes[5] = 1.0;
        j.held[1] = true;
        j.pressed[1] = true;
        pad.joysticks.push(j);
        let a = frame(&mut sys, &pad);
        assert_eq!(a.navigate, Vec2::new(1.0, 0.0));
        assert!(a.cancel);
        // The Input Manager setting leaves the UI to the keys.
        sys.handling = ActiveInputHandling::InputManager;
        assert_eq!(sys.ui_actions(None), None);
    }
}
