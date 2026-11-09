//! Press and hold to rename (Project window items, Hierarchy objects): holding the primary
//! button on an item for [`HOLD`] seconds without moving starts its inline rename, like a slow
//! second click in Explorer. A thin bar along the item's bottom edge fills while holding.
//! Moving more than [`SLOP`] points (a drag) cancels it.
//!
//! The button is still down when the rename starts, and its release lands on the item (maybe
//! beside the name field): egui would take that release as a click elsewhere and end the
//! rename at once. [`keep_focus`] guards the field until the button is released.

use egui::{Id, Pos2, Rect, Response, Ui};

/// How long to hold, in seconds.
pub const HOLD: f64 = 1.5;
/// How far the pointer may move while holding, in points.
pub const SLOP: f32 = 5.0;
/// Holds shorter than this draw nothing (an ordinary click shows no bar).
const SHOW_AFTER: f64 = 0.25;

/// A hold on one item this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hold {
    /// Not held (or cancelled by moving, or already fired for this press).
    None,
    /// Held this fraction of [`HOLD`] (0..1).
    Progress(f32),
    /// Held long enough this frame: start renaming.
    Fire,
}

/// The hold state from the press time and place, the time now and the pointer now.
/// `fired`: this press already started a rename.
pub fn state(press: Option<(f64, Pos2)>, now: f64, pointer: Option<Pos2>, fired: bool) -> Hold {
    let Some((start, origin)) = press else {
        return Hold::None;
    };
    if fired || pointer.is_none_or(|p| p.distance(origin) > SLOP) {
        return Hold::None;
    }
    let t = (now - start) / HOLD;
    if t >= 1.0 {
        Hold::Fire
    } else {
        Hold::Progress(t.max(0.0) as f32)
    }
}

fn fired_id() -> Id {
    Id::new("hold_to_rename_fired")
}

/// Check a hold on `resp` (an item that senses clicks). Keeps frames coming while held, so the
/// bar fills and the rename starts without moving the mouse. Returns [`Hold::Fire`] once per
/// press.
pub fn check(ui: &Ui, resp: &Response) -> Hold {
    if !resp.is_pointer_button_down_on() {
        return Hold::None;
    }
    let (press, now, pointer, plain) = ui.input(|i| {
        let press = i
            .pointer
            .primary_down()
            .then(|| i.pointer.press_start_time().zip(i.pointer.press_origin()))
            .flatten();
        (press, i.time, i.pointer.latest_pos(), i.modifiers.is_none())
    });
    if !plain {
        return Hold::None;
    }
    let fired_at = ui.data(|d| d.get_temp::<f64>(fired_id()));
    let fired = press.is_some_and(|(start, _)| fired_at == Some(start));
    let hold = state(press, now, pointer, fired);
    match hold {
        Hold::Fire => {
            if let Some((start, _)) = press {
                ui.data_mut(|d| d.insert_temp(fired_id(), start));
            }
        }
        Hold::Progress(_) => ui.ctx().request_repaint(),
        Hold::None => {}
    }
    hold
}

/// `resp` is dragged and the pointer has moved past [`SLOP`]: a real drag. (egui also starts a
/// drag after the button is held still for its click time; that is a hold, not a drag, and
/// would carry the item off before its rename could start.)
pub fn drag_moved(ui: &Ui, resp: &Response) -> bool {
    resp.dragged()
        && ui
            .input(|i| i.pointer.total_drag_delta())
            .is_some_and(|d| d.length() > SLOP)
}

/// Start carrying `payload` once `resp`'s drag has moved past [`SLOP`] (see [`drag_moved`]).
pub fn drag_payload<P: std::any::Any + Send + Sync>(ui: &Ui, resp: &Response, payload: P) {
    if drag_moved(ui, resp) && !egui::DragAndDrop::has_any_payload(ui.ctx()) {
        egui::DragAndDrop::set_payload(ui.ctx(), payload);
    }
}

/// The thin progress bar along the bottom of `rect`.
pub fn paint(ui: &Ui, rect: Rect, hold: Hold) {
    let Hold::Progress(t) = hold else { return };
    if f64::from(t) * HOLD < SHOW_AFTER {
        return;
    }
    let bar = Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - 2.0),
        egui::pos2(rect.left() + rect.width() * t, rect.bottom()),
    );
    ui.painter()
        .rect_filled(bar, 1.0, crate::theme::OVERRIDE.gamma_multiply(0.85));
}

/// For a rename field started by a hold: while the button that started it is still down (and
/// on the frame it is released), a lost focus is taken back instead of ending the rename.
/// `guard` is cleared once the button is up. Returns true when the focus was taken back.
pub fn keep_focus(ui: &Ui, edit: &Response, guard: &mut bool) -> bool {
    if !*guard {
        return false;
    }
    let (down, released) = ui.input(|i| (i.pointer.any_down(), i.pointer.any_released()));
    if !down && !released {
        *guard = false;
    }
    if edit.lost_focus() || !edit.has_focus() {
        edit.request_focus();
        return true;
    }
    false
}

/// True when the rename field `edit` lost the keyboard without the user moving it: no click,
/// press or key in the last moment. Something else dropped it (the field missed a frame, a
/// menu closing, another widget asking for it on its own); the caller takes the focus back
/// instead of ending the rename. Clicking elsewhere, Tab, Enter and Escape still end it.
pub fn focus_dropped(ui: &Ui, edit: &Response) -> bool {
    let id = edit.id.with("rename_user_input");
    let (now, active) = ui.input(|i| {
        let pointer = i.pointer.any_pressed() || i.pointer.any_released() || i.pointer.any_down();
        let keys = i
            .events
            .iter()
            .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
        (i.time, pointer || keys)
    });
    if active {
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    if !edit.lost_focus() {
        return false;
    }
    let recent = ui
        .data(|d| d.get_temp::<f64>(id))
        .is_some_and(|t| now - t < USER_INPUT_WINDOW);
    !recent
}

/// How long after a click or key a lost focus counts as the user's doing, in seconds.
const USER_INPUT_WINDOW: f64 = 0.3;

/// What an inline rename field did this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Editing,
    /// Enter, or a click elsewhere: use the text.
    Commit,
    /// Escape.
    Cancel,
}

/// The focus rules of an inline rename field (`edit`, shown this frame with a stable id): it
/// takes the keyboard with the whole name selected on its first frame, takes it back when
/// something else drops it (see [`focus_dropped`]), and ends with Enter or a click elsewhere
/// ([`Field::Commit`]) or Escape ([`Field::Cancel`]). (Fields that asked for the focus every
/// frame could never lose it, so Enter and clicking away did nothing.)
pub fn field(ui: &Ui, edit: &Response, text: &str) -> Field {
    let pass = ui.ctx().cumulative_pass_nr();
    let shown_id = edit.id.with("rename_shown");
    let fresh = ui
        .data(|d| d.get_temp::<u64>(shown_id))
        .is_none_or(|last| last + 1 < pass);
    ui.data_mut(|d| d.insert_temp(shown_id, pass));
    let dropped = focus_dropped(ui, edit);
    if fresh {
        crate::panels::project_menus::select_all(ui.ctx(), edit.id, text);
        edit.request_focus();
        return Field::Editing;
    }
    let (enter, escape) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if escape {
        return Field::Cancel;
    }
    if enter || (edit.lost_focus() && !dropped) {
        return Field::Commit;
    }
    if !edit.has_focus() {
        edit.request_focus();
    }
    Field::Editing
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    /// A rename field alone: the text and what the field said last.
    struct One {
        text: String,
        outcome: Option<Field>,
        editing: bool,
    }

    fn field_harness() -> egui_kittest::Harness<'static, One> {
        let one = One {
            text: "Old Name".into(),
            outcome: None,
            editing: true,
        };
        egui_kittest::Harness::builder()
            .with_size([400.0, 200.0])
            .with_step_dt(0.1)
            .build_ui_state(
                |ui, o: &mut One| {
                    if !o.editing {
                        return;
                    }
                    let edit = ui.add(egui::TextEdit::singleline(&mut o.text).id(Id::new("f")));
                    let r = field(ui, &edit, &o.text.clone());
                    if r != Field::Editing {
                        o.outcome = Some(r);
                        o.editing = false;
                    }
                },
                one,
            )
    }

    #[test]
    fn rename_fields_select_the_name_keep_focus_and_end_on_enter_click_or_escape() {
        let mut h = field_harness();
        h.run_steps(3);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(Id::new("f")));
        // The whole name is selected: typing replaces it.
        h.event(egui::Event::Text("New".into()));
        h.run_steps(2);
        assert_eq!(h.state().text, "New");
        // Focus dropped by something else: taken back.
        h.ctx.memory_mut(|m| m.surrender_focus(Id::new("f")));
        h.run_steps(3);
        assert!(h.state().editing);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(Id::new("f")));
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert_eq!(h.state().outcome, Some(Field::Commit));

        let mut h = field_harness();
        h.run_steps(3);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert_eq!(h.state().outcome, Some(Field::Cancel));

        let mut h = field_harness();
        h.run_steps(3);
        let away = pos2(300.0, 150.0);
        h.hover_at(away);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: away,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        h.run_steps(3);
        assert_eq!(h.state().outcome, Some(Field::Commit), "click elsewhere");
    }

    #[test]
    fn holding_still_fires_after_the_hold_time_once() {
        let at = pos2(10.0, 10.0);
        let press = Some((2.0, at));
        assert_eq!(state(None, 5.0, Some(at), false), Hold::None);
        assert_eq!(state(press, 2.0, Some(at), false), Hold::Progress(0.0));
        let Hold::Progress(t) = state(press, 2.75, Some(at), false) else {
            panic!("holding")
        };
        assert!((t - 0.5).abs() < 1e-4);
        assert!(matches!(
            state(press, 2.0 + HOLD - 0.01, Some(at), false),
            Hold::Progress(_)
        ));
        assert_eq!(state(press, 2.0 + HOLD, Some(at), false), Hold::Fire);
        // Fired already for this press: nothing more until the next press.
        assert_eq!(state(press, 2.0 + HOLD + 1.0, Some(at), true), Hold::None);
    }

    #[test]
    fn moving_cancels_the_hold() {
        let at = pos2(10.0, 10.0);
        let press = Some((0.0, at));
        // A small wobble is fine; a drag is not.
        assert_eq!(
            state(press, HOLD, Some(at + egui::vec2(3.0, 3.0)), false),
            Hold::Fire
        );
        assert_eq!(
            state(press, HOLD, Some(at + egui::vec2(SLOP + 1.0, 0.0)), false),
            Hold::None
        );
        assert_eq!(state(press, HOLD, None, false), Hold::None);
    }
}
