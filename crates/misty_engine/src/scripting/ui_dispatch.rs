//! Running UI events: the persistent calls set in the Inspector (On Click, On Value Changed,
//! On End Edit) and the Lua listeners added with `AddListener`.

use hecs::Entity;
use mlua::Value;

use super::{LogKind, ScriptHost, World, ui_widgets_api};
use crate::scene::{
    ButtonCall, UiButton, UiDropdown, UiInputField, UiScrollbar, UiSlider, UiToggle,
};
use crate::ui::{UiEvent, UiValue};

/// Inspector calls for an event on `e`.
fn persistent_calls(w: &World, e: Entity, event: &str) -> Vec<ButtonCall> {
    let world = &w.scene.world;
    let calls = match event {
        "onClick" => world.get::<&UiButton>(e).map(|c| c.on_click.clone()),
        "onEndEdit" => world.get::<&UiInputField>(e).map(|c| c.on_end_edit.clone()),
        _ => world
            .get::<&UiToggle>(e)
            .map(|c| c.on_value_changed.clone())
            .or_else(|_| {
                world
                    .get::<&UiSlider>(e)
                    .map(|c| c.on_value_changed.clone())
            })
            .or_else(|_| {
                world
                    .get::<&UiScrollbar>(e)
                    .map(|c| c.on_value_changed.clone())
            })
            .or_else(|_| {
                world
                    .get::<&UiInputField>(e)
                    .map(|c| c.on_value_changed.clone())
            })
            .or_else(|_| {
                world
                    .get::<&UiDropdown>(e)
                    .map(|c| c.on_value_changed.clone())
            }),
    };
    calls.unwrap_or_default()
}

impl ScriptHost {
    pub(super) fn ui_event(&mut self, ev: UiEvent) {
        let (e, event, label, arg) = match ev {
            UiEvent::Click(b) => (b, "onClick", "On Click", None),
            UiEvent::ValueChanged(e, v) => (e, "onValueChanged", "On Value Changed", Some(v)),
            UiEvent::EndEdit(e, t) => (e, "onEndEdit", "On End Edit", Some(UiValue::Text(t))),
        };
        let to_lua = |v: &UiValue| -> Value {
            match v {
                UiValue::Bool(b) => Value::Boolean(*b),
                UiValue::Number(n) => Value::Number(*n as f64),
                UiValue::Int(i) => Value::Integer(*i as i64),
                UiValue::Text(t) => self
                    .lua
                    .create_string(t)
                    .map(Value::String)
                    .unwrap_or(Value::Nil),
            }
        };
        let arg = arg.as_ref().map(to_lua);
        let calls = self.with_world(|w| persistent_calls(w, e, event));
        for call in calls {
            if let Some(t) = call.target.filter(|_| !call.method.is_empty()) {
                self.send_message_with(t, &call.method, arg.clone(), label);
            }
        }
        for f in ui_widgets_api::listeners(&self.lua, e, event) {
            let result = match &arg {
                Some(a) => f.call::<()>(a.clone()),
                None => f.call::<()>(()),
            };
            if let Err(err) = result {
                self.log(LogKind::Error, format!("{label} listener: {err}"));
            }
            self.flush_spawned();
        }
    }
}
