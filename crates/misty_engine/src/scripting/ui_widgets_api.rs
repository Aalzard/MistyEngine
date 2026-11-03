//! Lua API for the UI widgets returned by `GetComponent`: Toggle, ToggleGroup, Slider,
//! Scrollbar, InputField, Dropdown, ScrollRect and RawImage, plus the event objects
//! (`onClick`, `onValueChanged`, `onEndEdit`) with `AddListener` / `RemoveListener` /
//! `RemoveAllListeners`.

use hecs::Entity;
use mlua::{Function, Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, with_world};
use super::ui_api::{LColor, get, set};
use crate::scene::{
    InputContentType, UiDropdown, UiInputField, UiRawImage, UiScrollRect, UiScrollbar, UiSlider,
    UiToggle, UiToggleGroup,
};
use crate::ui::driven::{clamp_scrollbar, clamp_slider};
use crate::ui::input_field::sanitize;

const LISTENERS: &str = "misty_ui_listeners";

fn key(e: Entity, event: &str) -> String {
    format!("{}:{event}", e.to_bits().get())
}

/// The listener table (created on first use).
fn table(lua: &Lua) -> Result<Table> {
    match lua.named_registry_value::<Option<Table>>(LISTENERS)? {
        Some(t) => Ok(t),
        None => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(LISTENERS, &t)?;
            Ok(t)
        }
    }
}

/// Functions added with `AddListener` for `event` of `e`.
pub fn listeners(lua: &Lua, e: Entity, event: &str) -> Vec<Function> {
    let Ok(t) = table(lua) else { return Vec::new() };
    let Ok(Some(list)) = t.get::<Option<Table>>(key(e, event)) else {
        return Vec::new();
    };
    list.sequence_values::<Function>().flatten().collect()
}

/// A UI event of one object (`button.onClick`, `slider.onValueChanged`, …).
#[derive(Clone, Copy)]
pub struct LUiEvent {
    pub entity: Entity,
    pub event: &'static str,
}

impl UserData for LUiEvent {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("AddListener", |lua, ev, f: Function| {
            let t = table(lua)?;
            let k = key(ev.entity, ev.event);
            let list = match t.get::<Option<Table>>(k.as_str())? {
                Some(l) => l,
                None => {
                    let l = lua.create_table()?;
                    t.set(k.as_str(), &l)?;
                    l
                }
            };
            list.push(f)
        });
        m.add_method("RemoveListener", |lua, ev, f: Function| {
            let t = table(lua)?;
            let k = key(ev.entity, ev.event);
            let Some(list) = t.get::<Option<Table>>(k.as_str())? else {
                return Ok(());
            };
            let kept = lua.create_table()?;
            for g in list.sequence_values::<Function>().flatten() {
                if g != f {
                    kept.push(g)?;
                }
            }
            t.set(k.as_str(), kept)
        });
        m.add_method("RemoveAllListeners", |lua, ev, ()| {
            table(lua)?.set(key(ev.entity, ev.event), Value::Nil)
        });
    }
}

fn event(e: Entity, event: &'static str) -> LUiEvent {
    LUiEvent { entity: e, event }
}

/// `interactable`, `gameObject` and `onValueChanged`, shared by the selectable widgets.
macro_rules! selectable_fields {
    ($f:ident, $t:ty) => {
        $f.add_field_method_get("interactable", |lua, w| {
            get(lua, w.0, |c: &$t| c.selectable.interactable)
        });
        $f.add_field_method_set("interactable", |lua, w, v: bool| {
            set(lua, w.0, |c: &mut $t| c.selectable.interactable = v)
        });
        $f.add_field_method_get("gameObject", |_, w| Ok(LGameObject(w.0)));
        $f.add_field_method_get("onValueChanged", |_, w| Ok(event(w.0, "onValueChanged")));
    };
}

#[derive(Clone, Copy)]
pub struct LToggle(pub Entity);
#[derive(Clone, Copy)]
pub struct LToggleGroup(pub Entity);
#[derive(Clone, Copy)]
pub struct LSlider(pub Entity);
#[derive(Clone, Copy)]
pub struct LScrollbar(pub Entity);
#[derive(Clone, Copy)]
pub struct LInputField(pub Entity);
#[derive(Clone, Copy)]
pub struct LDropdown(pub Entity);
#[derive(Clone, Copy)]
pub struct LScrollRect(pub Entity);
#[derive(Clone, Copy)]
pub struct LRawImage(pub Entity);

impl UserData for LToggle {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        selectable_fields!(f, UiToggle);
        f.add_field_method_get("isOn", |lua, t| get(lua, t.0, |c: &UiToggle| c.is_on));
        f.add_field_method_set("isOn", |lua, t, v: bool| {
            set(lua, t.0, |c: &mut UiToggle| c.is_on = v)
        });
        f.add_field_method_get("group", |lua, t| {
            get(lua, t.0, |c: &UiToggle| c.group.map(LToggleGroup))
        });
    }
}

impl UserData for LToggleGroup {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("allowSwitchOff", |lua, g| {
            get(lua, g.0, |c: &UiToggleGroup| c.allow_switch_off)
        });
        f.add_field_method_set("allowSwitchOff", |lua, g, v: bool| {
            set(lua, g.0, |c: &mut UiToggleGroup| c.allow_switch_off = v)
        });
        f.add_field_method_get("gameObject", |_, g| Ok(LGameObject(g.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("AnyTogglesOn", |lua, g, ()| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .query::<&UiToggle>()
                    .iter()
                    .any(|t| t.group == Some(g.0) && t.is_on)
            })
        });
        m.add_method("SetAllTogglesOff", |lua, g, ()| {
            with_world(lua, |w| {
                for t in w.scene.world.query_mut::<&mut UiToggle>() {
                    if t.group == Some(g.0) {
                        t.is_on = false;
                    }
                }
            })
        });
    }
}

impl UserData for LSlider {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        selectable_fields!(f, UiSlider);
        f.add_field_method_get("value", |lua, s| get(lua, s.0, |c: &UiSlider| c.value));
        f.add_field_method_set("value", |lua, s, v: f32| {
            set(lua, s.0, |c: &mut UiSlider| c.value = clamp_slider(c, v))
        });
        f.add_field_method_get("normalizedValue", |lua, s| {
            get(lua, s.0, crate::ui::driven::slider_normalized)
        });
        f.add_field_method_set("normalizedValue", |lua, s, v: f32| {
            set(lua, s.0, |c: &mut UiSlider| {
                c.value = crate::ui::driven::slider_value_at(c, v)
            })
        });
        f.add_field_method_get("minValue", |lua, s| {
            get(lua, s.0, |c: &UiSlider| c.min_value)
        });
        f.add_field_method_set("minValue", |lua, s, v: f32| {
            set(lua, s.0, |c: &mut UiSlider| {
                c.min_value = v;
                c.value = clamp_slider(c, c.value);
            })
        });
        f.add_field_method_get("maxValue", |lua, s| {
            get(lua, s.0, |c: &UiSlider| c.max_value)
        });
        f.add_field_method_set("maxValue", |lua, s, v: f32| {
            set(lua, s.0, |c: &mut UiSlider| {
                c.max_value = v;
                c.value = clamp_slider(c, c.value);
            })
        });
        f.add_field_method_get("wholeNumbers", |lua, s| {
            get(lua, s.0, |c: &UiSlider| c.whole_numbers)
        });
        f.add_field_method_set("wholeNumbers", |lua, s, v: bool| {
            set(lua, s.0, |c: &mut UiSlider| {
                c.whole_numbers = v;
                c.value = clamp_slider(c, c.value);
            })
        });
    }
}

impl UserData for LScrollbar {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        selectable_fields!(f, UiScrollbar);
        f.add_field_method_get("value", |lua, s| get(lua, s.0, |c: &UiScrollbar| c.value));
        f.add_field_method_set("value", |lua, s, v: f32| {
            set(lua, s.0, |c: &mut UiScrollbar| {
                c.value = clamp_scrollbar(c, v)
            })
        });
        f.add_field_method_get("size", |lua, s| get(lua, s.0, |c: &UiScrollbar| c.size));
        f.add_field_method_set("size", |lua, s, v: f32| {
            set(lua, s.0, |c: &mut UiScrollbar| c.size = v.clamp(0.0, 1.0))
        });
        f.add_field_method_get("numberOfSteps", |lua, s| {
            get(lua, s.0, |c: &UiScrollbar| c.number_of_steps)
        });
        f.add_field_method_set("numberOfSteps", |lua, s, v: u32| {
            set(lua, s.0, |c: &mut UiScrollbar| c.number_of_steps = v)
        });
    }
}

fn content_type_name(t: InputContentType) -> &'static str {
    match t {
        InputContentType::Standard => "Standard",
        InputContentType::IntegerNumber => "IntegerNumber",
        InputContentType::DecimalNumber => "DecimalNumber",
        InputContentType::Password => "Password",
    }
}

impl UserData for LInputField {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        selectable_fields!(f, UiInputField);
        f.add_field_method_get("onEndEdit", |_, i| Ok(event(i.0, "onEndEdit")));
        f.add_field_method_get("text", |lua, i| {
            get(lua, i.0, |c: &UiInputField| c.text.clone())
        });
        f.add_field_method_set("text", |lua, i, v: Value| {
            let s = match v {
                Value::String(s) => s.to_string_lossy(),
                Value::Nil => String::new(),
                other => other.to_string()?,
            };
            set(lua, i.0, |c: &mut UiInputField| c.text = sanitize(c, &s))
        });
        f.add_field_method_get("characterLimit", |lua, i| {
            get(lua, i.0, |c: &UiInputField| c.character_limit)
        });
        f.add_field_method_set("characterLimit", |lua, i, v: u32| {
            set(lua, i.0, |c: &mut UiInputField| c.character_limit = v)
        });
        f.add_field_method_get("readOnly", |lua, i| {
            get(lua, i.0, |c: &UiInputField| c.read_only)
        });
        f.add_field_method_set("readOnly", |lua, i, v: bool| {
            set(lua, i.0, |c: &mut UiInputField| c.read_only = v)
        });
        f.add_field_method_get("contentType", |lua, i| {
            get(lua, i.0, |c: &UiInputField| {
                content_type_name(c.content_type)
            })
        });
        f.add_field_method_set("contentType", |lua, i, v: String| {
            let t = InputContentType::ALL
                .into_iter()
                .find(|t| content_type_name(*t).eq_ignore_ascii_case(&v))
                .ok_or_else(|| mlua::Error::runtime(format!("unknown content type '{v}'")))?;
            set(lua, i.0, |c: &mut UiInputField| c.content_type = t)
        });
    }
}

impl UserData for LDropdown {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        selectable_fields!(f, UiDropdown);
        f.add_field_method_get("value", |lua, d| get(lua, d.0, |c: &UiDropdown| c.value));
        f.add_field_method_set("value", |lua, d, v: i32| {
            set(lua, d.0, |c: &mut UiDropdown| {
                c.value = v.clamp(0, (c.options.len() as i32 - 1).max(0))
            })
        });
        f.add_field_method_get("options", |lua, d| {
            let opts = get(lua, d.0, |c: &UiDropdown| c.options.clone())?;
            lua.create_sequence_from(opts)
        });
        f.add_field_method_set("options", |lua, d, t: Vec<String>| {
            set(lua, d.0, |c: &mut UiDropdown| c.options = t)
        });
        f.add_field_method_get("captionText", |lua, d| {
            get(lua, d.0, |c: &UiDropdown| {
                usize::try_from(c.value)
                    .ok()
                    .and_then(|i| c.options.get(i).cloned())
                    .unwrap_or_default()
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("AddOptions", |lua, d, t: Vec<String>| {
            set(lua, d.0, |c: &mut UiDropdown| c.options.extend(t))
        });
        m.add_method("ClearOptions", |lua, d, ()| {
            set(lua, d.0, |c: &mut UiDropdown| {
                c.options.clear();
                c.value = 0;
            })
        });
        // The caption follows the value every frame; kept for familiarity.
        m.add_method("RefreshShownValue", |_, _, ()| Ok(()));
    }
}

impl UserData for LScrollRect {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("enabled", |lua, s| {
            get(lua, s.0, |c: &UiScrollRect| c.enabled)
        });
        f.add_field_method_set("enabled", |lua, s, v: bool| {
            set(lua, s.0, |c: &mut UiScrollRect| c.enabled = v)
        });
        f.add_field_method_get("horizontal", |lua, s| {
            get(lua, s.0, |c: &UiScrollRect| c.horizontal)
        });
        f.add_field_method_set("horizontal", |lua, s, v: bool| {
            set(lua, s.0, |c: &mut UiScrollRect| c.horizontal = v)
        });
        f.add_field_method_get("vertical", |lua, s| {
            get(lua, s.0, |c: &UiScrollRect| c.vertical)
        });
        f.add_field_method_set("vertical", |lua, s, v: bool| {
            set(lua, s.0, |c: &mut UiScrollRect| c.vertical = v)
        });
        f.add_field_method_get("content", |lua, s| {
            get(lua, s.0, |c: &UiScrollRect| c.content.map(LGameObject))
        });
        f.add_field_method_get("gameObject", |_, s| Ok(LGameObject(s.0)));
    }
}

impl UserData for LRawImage {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("color", |lua, i| {
            get(lua, i.0, |c: &UiRawImage| LColor(c.color))
        });
        f.add_field_method_set("color", |lua, i, v: LColor| {
            set(lua, i.0, |c: &mut UiRawImage| c.color = v.0)
        });
        f.add_field_method_get("enabled", |lua, i| {
            get(lua, i.0, |c: &UiRawImage| c.enabled)
        });
        f.add_field_method_set("enabled", |lua, i, v: bool| {
            set(lua, i.0, |c: &mut UiRawImage| c.enabled = v)
        });
        f.add_field_method_get("texture", |lua, i| {
            get(lua, i.0, |c: &UiRawImage| c.texture.clone())
        });
        f.add_field_method_set("texture", |lua, i, v: Option<String>| {
            set(lua, i.0, |c: &mut UiRawImage| {
                c.texture = v.filter(|p| !p.is_empty())
            })
        });
        f.add_field_method_get("gameObject", |_, i| Ok(LGameObject(i.0)));
    }
}

/// `GetComponent` for widget names; `Ok(None)` = not a widget (or not present).
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = |f: fn(&hecs::World, Entity) -> bool| with_world(lua, |w| f(&w.scene.world, e));
    let present = match name {
        "Toggle" => has(|w, e| w.get::<&UiToggle>(e).is_ok())?,
        "ToggleGroup" => has(|w, e| w.get::<&UiToggleGroup>(e).is_ok())?,
        "Slider" => has(|w, e| w.get::<&UiSlider>(e).is_ok())?,
        "Scrollbar" => has(|w, e| w.get::<&UiScrollbar>(e).is_ok())?,
        "InputField" => has(|w, e| w.get::<&UiInputField>(e).is_ok())?,
        "Dropdown" => has(|w, e| w.get::<&UiDropdown>(e).is_ok())?,
        "ScrollRect" => has(|w, e| w.get::<&UiScrollRect>(e).is_ok())?,
        "RawImage" => has(|w, e| w.get::<&UiRawImage>(e).is_ok())?,
        _ => false,
    };
    if !present {
        return Ok(None);
    }
    let ud = match name {
        "Toggle" => lua.create_userdata(LToggle(e))?,
        "ToggleGroup" => lua.create_userdata(LToggleGroup(e))?,
        "Slider" => lua.create_userdata(LSlider(e))?,
        "Scrollbar" => lua.create_userdata(LScrollbar(e))?,
        "InputField" => lua.create_userdata(LInputField(e))?,
        "Dropdown" => lua.create_userdata(LDropdown(e))?,
        "ScrollRect" => lua.create_userdata(LScrollRect(e))?,
        _ => lua.create_userdata(LRawImage(e))?,
    };
    Ok(Some(Value::UserData(ud)))
}

/// `button.onClick`.
pub fn on_click(e: Entity) -> LUiEvent {
    event(e, "onClick")
}

#[cfg(test)]
#[path = "ui_widgets_tests.rs"]
mod tests;
