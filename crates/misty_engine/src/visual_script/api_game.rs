//! Native engine nodes for saving small values (`PlayerPrefs`), driving an Animator's
//! parameters and locking the cursor. The same members exist in Lua; as natives their pins are
//! typed and named for people (`Key`, `Value`, `Default`), so a graph can save a score or set
//! an Animator's `Speed` without wiring Any pins.

use super::natives::{Cx, Slot};
use super::value::Value;
use crate::animation::animator;
use crate::animation::controller::ParamKind;
use crate::prefs::{self, PrefValue};
use crate::scene::Animator;
use crate::scripting::CursorLockMode;

// ---- PlayerPrefs ------------------------------------------------------------------------------

/// `PlayerPrefs.SetInt(Key, Value)`.
pub fn prefs_set_int(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    prefs::set(&cx.s(i[0]), PrefValue::Int(cx.i(i[1])));
    Ok(())
}

/// `PlayerPrefs.GetInt(Key, Default)`.
pub fn prefs_get_int(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = prefs::get_int(&cx.s(i[0]), cx.i(i[1]));
    cx.set(o[0], Value::Int(v));
    Ok(())
}

/// `PlayerPrefs.SetFloat(Key, Value)`.
pub fn prefs_set_float(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    prefs::set(&cx.s(i[0]), PrefValue::Float(cx.f(i[1]) as f64));
    Ok(())
}

/// `PlayerPrefs.GetFloat(Key, Default)`.
pub fn prefs_get_float(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = prefs::get_float(&cx.s(i[0]), cx.f(i[1]) as f64);
    cx.set(o[0], Value::Float(v as f32));
    Ok(())
}

/// `PlayerPrefs.SetString(Key, Value)`.
pub fn prefs_set_string(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    prefs::set(&cx.s(i[0]), PrefValue::String(cx.s(i[1]).to_string()));
    Ok(())
}

/// `PlayerPrefs.GetString(Key, Default)`.
pub fn prefs_get_string(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = prefs::get_string(&cx.s(i[0]), &cx.s(i[1]));
    cx.set(o[0], Value::Str(v.into()));
    Ok(())
}

/// `PlayerPrefs.HasKey(Key)`.
pub fn prefs_has_key(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = prefs::has_key(&cx.s(i[0]));
    cx.set(o[0], Value::Bool(v));
    Ok(())
}

/// `PlayerPrefs.DeleteKey(Key)`.
pub fn prefs_delete_key(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    prefs::delete_key(&cx.s(i[0]));
    Ok(())
}

/// `PlayerPrefs.Save()`: write the values to the player's prefs file.
pub fn prefs_save(_: &mut Cx<'_>, _: &[Slot], _: &[Slot]) -> Result<(), String> {
    prefs::save()
}

// ---- Animator ---------------------------------------------------------------------------------

/// The target (`i[0]`), which must have an Animator.
fn animated(cx: &Cx<'_>, i: &[Slot]) -> Result<hecs::Entity, String> {
    let e = cx.obj(i[0])?;
    if cx.world.scene.world.get::<&Animator>(e).is_err() {
        return Err(format!("'{}' has no Animator", cx.world.scene.name(e)));
    }
    Ok(e)
}

fn set_param(cx: &mut Cx<'_>, i: &[Slot], kinds: &[ParamKind], v: f32) -> Result<(), String> {
    let e = animated(cx, i)?;
    let name = cx.s(i[1]);
    let w = &mut *cx.world;
    w.animator.set_param(&w.scene, e, &name, kinds, v);
    Ok(())
}

fn get_param(cx: &mut Cx<'_>, i: &[Slot], kinds: &[ParamKind]) -> Result<f32, String> {
    let e = animated(cx, i)?;
    let name = cx.s(i[1]);
    let w = &mut *cx.world;
    Ok(w.animator.param(&w.scene, e, &name, kinds))
}

/// `animator:SetFloat(Name, Value)`, eased over `Damp Time` seconds when it is above 0.
pub fn animator_set_float(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let (v, damp) = (cx.f(i[2]), cx.f(i[3]));
    if damp <= 0.0 {
        return set_param(cx, i, animator::FLOAT, v);
    }
    let e = animated(cx, i)?;
    let name = cx.s(i[1]);
    let w = &mut *cx.world;
    let dt = w.time.delta;
    w.animator.set_float_damped(&w.scene, e, &name, v, damp, dt);
    Ok(())
}

pub fn animator_get_float(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = get_param(cx, i, animator::FLOAT)?;
    cx.set(o[0], Value::Float(v));
    Ok(())
}

pub fn animator_set_bool(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let v = if cx.b(i[2]) { 1.0 } else { 0.0 };
    set_param(cx, i, animator::BOOL, v)
}

pub fn animator_get_bool(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = get_param(cx, i, animator::BOOL)?;
    cx.set(o[0], Value::Bool(v >= 0.5));
    Ok(())
}

pub fn animator_set_integer(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let v = cx.i(i[2]) as f32;
    set_param(cx, i, animator::INT, v)
}

pub fn animator_get_integer(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = get_param(cx, i, animator::INT)?;
    cx.set(o[0], Value::Int(v.round() as i64));
    Ok(())
}

pub fn animator_set_trigger(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    set_param(cx, i, animator::TRIGGER, 1.0)
}

pub fn animator_reset_trigger(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    set_param(cx, i, animator::TRIGGER, 0.0)
}

// ---- Cursor -----------------------------------------------------------------------------------

/// `Cursor.lockState = Value` (`None`, `Locked` or `Confined`); locking hides the cursor.
pub fn cursor_lock_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let name = cx.s(i[0]);
    let Some(mode) = CursorLockMode::ALL
        .into_iter()
        .find(|m| m.name().eq_ignore_ascii_case(name.trim()))
    else {
        return Err(format!(
            "Cursor lock state: use None, Locked or Confined (got '{name}')"
        ));
    };
    let c = &mut cx.world.cursor;
    if mode == CursorLockMode::Locked && c.lock != mode {
        c.visible = false;
    }
    if mode == CursorLockMode::None && c.lock == CursorLockMode::Locked {
        c.visible = true;
    }
    c.lock = mode;
    Ok(())
}

pub fn cursor_lock_get(cx: &mut Cx<'_>, _: &[Slot], o: &[Slot]) -> Result<(), String> {
    let name = cx.world.cursor.lock.name();
    cx.set(o[0], Value::Str(name.into()));
    Ok(())
}
