//! The rebind listener: picking the next control of the right kind, binding groups, excluded
//! and canceling controls, waiting for a better match.

use super::{device_of, listens_to};
use crate::input::Joystick;
use crate::input::actions::asset::InputActionAsset;
use crate::input::actions::path::DeviceKind;
use crate::input::actions::state::ActionSet;
use crate::input::actions::system::{InputSystem, SystemEvent};
use crate::scene::Scene;
use crate::scripting::Input;

fn system() -> (InputSystem, usize, usize, usize) {
    let asset = InputActionAsset::starter("Player");
    let (m, a) = asset.find_action("Player/Jump").unwrap();
    let mut sys = InputSystem::new(std::path::Path::new("."), Default::default());
    let set = sys.add_set(ActionSet::new(asset, ""));
    sys.set_mut(set).unwrap().set_all_enabled(true);
    (sys, set, m, a)
}

fn press(keys: &[&str]) -> Input {
    let mut i = Input::default();
    for k in keys {
        i.held.insert((*k).to_owned());
        i.pressed.insert((*k).to_owned());
    }
    i
}

fn pad_with(f: impl FnOnce(&mut Joystick)) -> Input {
    let mut j = Joystick {
        name: "Pad".into(),
        ..Default::default()
    };
    f(&mut j);
    Input {
        joysticks: vec![j],
        ..Default::default()
    }
}

fn completed(ev: &[SystemEvent]) -> Option<String> {
    ev.iter().find_map(|e| match e {
        SystemEvent::RebindComplete { path, .. } => Some(path.clone()),
        _ => None,
    })
}

fn canceled(ev: &[SystemEvent]) -> bool {
    ev.iter()
        .any(|e| matches!(e, SystemEvent::RebindCanceled { .. }))
}

#[test]
fn device_of_reads_the_layout() {
    assert_eq!(
        device_of("<Gamepad>/buttonSouth"),
        Some(DeviceKind::Gamepad)
    );
    assert_eq!(device_of("<keyboard>/a"), Some(DeviceKind::Keyboard));
    assert_eq!(device_of("Keyboard/a"), None);
    assert!(listens_to(&[], "<Mouse>/leftButton"));
    assert!(listens_to(&[DeviceKind::Pointer], "<Mouse>/leftButton"));
    assert!(!listens_to(&[DeviceKind::Gamepad], "<Keyboard>/a"));
}

#[test]
fn the_action_is_disabled_while_listening_and_the_start_frame_is_skipped() {
    let (mut sys, set, m, a) = system();
    let scene = Scene::new("t");
    let id = sys.new_rebind(set, m, a, None).unwrap();
    // Not started yet: nothing is heard.
    assert!(completed(&sys.update(&scene, &press(&["k"]), 0.016)).is_none());
    assert!(sys.set(set).unwrap().state(m, a).unwrap().enabled);
    sys.start_rebind(id);
    assert!(!sys.set(set).unwrap().state(m, a).unwrap().enabled);
    assert!(sys.rebind(id).unwrap().started);
    // Nothing pressed: still listening.
    assert!(completed(&sys.update(&scene, &Input::default(), 0.016)).is_none());
    let ev = sys.update(&scene, &press(&["k"]), 0.016);
    assert_eq!(completed(&ev).as_deref(), Some("<Keyboard>/k"));
    assert!(sys.rebind(id).is_none(), "the operation is over");
}

#[test]
fn mouse_buttons_and_gamepad_buttons_triggers_and_stick_directions_bind_a_button() {
    let (mut sys, set, m, a) = system();
    let scene = Scene::new("t");
    let mut listen = |input: Input| {
        let id = sys.new_rebind(set, m, a, Some(0)).unwrap();
        sys.start_rebind(id);
        sys.update(&scene, &Input::default(), 0.016);
        let ev = sys.update(&scene, &input, 0.016);
        sys.cancel_rebind(id);
        // Let the action come back on.
        sys.update(&scene, &Input::default(), 0.016);
        completed(&ev)
    };
    let mut right = Input::default();
    right.mouse_pressed[1] = true;
    right.mouse_held[1] = true;
    assert_eq!(listen(right).as_deref(), Some("<Mouse>/rightButton"));
    assert_eq!(
        listen(pad_with(|j| {
            j.pressed[0] = true;
            j.held[0] = true;
        }))
        .as_deref(),
        Some("<Gamepad>/buttonSouth")
    );
    assert_eq!(
        listen(pad_with(|j| j.axes[9] = 0.9)).as_deref(),
        Some("<Gamepad>/rightTrigger")
    );
    // Joystick y +1 is down.
    assert_eq!(
        listen(pad_with(|j| j.axes[1] = -0.9)).as_deref(),
        Some("<Gamepad>/leftStick/up")
    );
    // Mouse motion never binds.
    let moved = Input {
        mouse_delta: glam::Vec2::new(40.0, 0.0),
        ..Default::default()
    };
    assert_eq!(listen(moved), None);
}

#[test]
fn a_stick_binding_ignores_keys_and_buttons() {
    let (mut sys, set, _, _) = system();
    let scene = Scene::new("t");
    let (m, a) = sys
        .set(set)
        .unwrap()
        .asset
        .find_action("Player/Look")
        .unwrap();
    let id = sys.new_rebind(set, m, a, Some(1)).unwrap();
    assert!(sys.rebind(id).unwrap().expect_vector);
    sys.start_rebind(id);
    sys.update(&scene, &Input::default(), 0.016);
    assert!(completed(&sys.update(&scene, &press(&["k"]), 0.016)).is_none());
    let buttons = pad_with(|j| {
        j.pressed[0] = true;
        j.held[0] = true;
    });
    assert!(completed(&sys.update(&scene, &buttons, 0.016)).is_none());
    let ev = sys.update(&scene, &pad_with(|j| j.axes[0] = 1.0), 0.016);
    assert_eq!(completed(&ev).as_deref(), Some("<Gamepad>/leftStick"));
}

#[test]
fn a_binding_group_retargets_and_listens_to_its_devices_only() {
    let (mut sys, set, m, a) = system();
    let scene = Scene::new("t");
    // Jump: 0 Space (Keyboard&Mouse), 1 buttonSouth (Gamepad).
    let id = sys.new_rebind(set, m, a, Some(0)).unwrap();
    sys.rebind_group(id, "Gamepad").unwrap();
    assert_eq!(sys.rebind(id).unwrap().flat, 1);
    assert_eq!(sys.rebind(id).unwrap().group.as_deref(), Some("Gamepad"));
    sys.start_rebind(id);
    sys.update(&scene, &Input::default(), 0.016);
    // Keys don't bind a gamepad binding...
    assert!(completed(&sys.update(&scene, &press(&["k"]), 0.016)).is_none());
    // ...but Escape still cancels.
    let pad_north = pad_with(|j| {
        j.pressed[3] = true;
        j.held[3] = true;
    });
    let ev = sys.update(&scene, &pad_north, 0.016);
    assert_eq!(completed(&ev).as_deref(), Some("<Gamepad>/buttonNorth"));
    let s = sys.set(set).unwrap();
    assert_eq!(
        s.effective_path(m, a, 1).as_deref(),
        Some("<Gamepad>/buttonNorth")
    );
    assert_eq!(
        s.effective_path(m, a, 0).as_deref(),
        Some("<Keyboard>/space")
    );

    let id = sys.new_rebind(set, m, a, None).unwrap();
    sys.rebind_group(id, "Gamepad").unwrap();
    sys.start_rebind(id);
    sys.update(&scene, &Input::default(), 0.016);
    assert!(canceled(&sys.update(&scene, &press(&["escape"]), 0.016)));
    // A group without bindings is an error; a started operation can't change.
    let id = sys.new_rebind(set, m, a, None).unwrap();
    assert!(sys.rebind_group(id, "Touch").is_err());
    sys.start_rebind(id);
    assert!(sys.rebind_group(id, "Gamepad").is_err());
    assert!(sys.retarget_rebind(id, 1).is_err());
}

#[test]
fn excluded_and_canceling_controls() {
    let (mut sys, set, m, a) = system();
    let scene = Scene::new("t");
    let id = sys.new_rebind(set, m, a, Some(0)).unwrap();
    {
        let r = sys.rebind_mut(id).unwrap();
        r.excluded.push("Mouse".into());
        r.cancel = vec!["<Gamepad>/start".into()];
    }
    sys.start_rebind(id);
    sys.update(&scene, &Input::default(), 0.016);
    let mut click = Input::default();
    click.mouse_pressed[0] = true;
    assert!(completed(&sys.update(&scene, &click, 0.016)).is_none());
    // Escape is an ordinary key now.
    let ev = sys.update(&scene, &press(&["escape"]), 0.016);
    assert_eq!(completed(&ev).as_deref(), Some("<Keyboard>/escape"));

    let id = sys.new_rebind(set, m, a, Some(0)).unwrap();
    sys.rebind_mut(id).unwrap().cancel = vec!["<Gamepad>/start".into()];
    sys.start_rebind(id);
    sys.update(&scene, &Input::default(), 0.016);
    let start = pad_with(|j| {
        j.pressed[7] = true;
        j.held[7] = true;
    });
    let ev = sys.update(&scene, &start, 0.016);
    assert!(canceled(&ev), "{ev:?}");
    assert!(sys.rebind(id).is_none());
}

#[test]
fn waiting_for_another_match_takes_the_last_one() {
    let (mut sys, set, m, a) = system();
    let scene = Scene::new("t");
    let id = sys.new_rebind(set, m, a, Some(0)).unwrap();
    sys.rebind_mut(id).unwrap().wait = 0.1;
    sys.start_rebind(id);
    sys.update(&scene, &Input::default(), 0.016);
    assert!(completed(&sys.update(&scene, &press(&["q"]), 0.016)).is_none());
    assert!(completed(&sys.update(&scene, &press(&["r"]), 0.016)).is_none());
    assert!(completed(&sys.update(&scene, &Input::default(), 0.05)).is_none());
    let ev = sys.update(&scene, &Input::default(), 0.1);
    assert_eq!(completed(&ev).as_deref(), Some("<Keyboard>/r"));
}
