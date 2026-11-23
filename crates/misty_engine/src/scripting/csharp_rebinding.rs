//! C# `action.bindings` and interactive rebinding (`action.PerformInteractiveRebinding()`,
//! `InputActionRebindingExtensions.RebindingOperation` in MistyEngine.cs), on the Input
//! System's rebinding (`input/actions/system.rs`, `rebinding.rs`) like Lua's.
//!
//! Ops of the `inputSystem` native function: 85 binding count, 86 binding `v[0]` (text
//! `name\npath\neffectivePath\ngroups\nid`, result bits 1 composite, 2 part of a composite,
//! 4 overridden); 130 new rebind (`id` a set, `arg` a packed action, `v[0]` a binding index
//! or -1; the result is the operation id), and on an operation `id`: 131 exclude controls,
//! 132 cancel through, 133 target binding `v[0]`, 134 binding group, 135 wait `v[0]` seconds,
//! 136 start, 137 cancel (1 when it was listening), 138 dispose, 139 state (`v[0]` binding
//! index, `v[1]` started; result 0 when it is over).
//!
//! Completion reaches C# as input event 140 (complete, with the chosen path) or 141
//! (canceled) under key `1 << 62 | id`.

use super::{PendingArg, fail, flag, unpack, write_text};
use crate::scripting::{ScriptHost, World};

const EV_REBIND_COMPLETE: i32 = 140;
const EV_REBIND_CANCELED: i32 = 141;

fn rebind_key(id: u64) -> u64 {
    (1 << 62) | id
}

impl ScriptHost {
    /// Tell C# a rebinding operation ended (`path` None = canceled).
    pub(crate) fn cs_rebind_event(&mut self, id: u64, path: Option<String>) {
        let kind = if path.is_some() {
            EV_REBIND_COMPLETE
        } else {
            EV_REBIND_CANCELED
        };
        self.cs_event(rebind_key(id), kind, PendingArg::Rebind { id, path });
    }
}

/// Ops 85 / 86: an action's flat bindings.
pub(super) fn bindings(
    w: &mut World,
    op: i32,
    id: u64,
    arg: i64,
    v: &mut [f64; 16],
    buf: *mut u16,
    len: i32,
) -> i64 {
    let Some(s) = w.input_system.set(id as usize) else {
        return fail("these input actions are gone");
    };
    let (map, action) = unpack(arg);
    let Some(def) = s.asset.maps.get(map).and_then(|m| m.actions.get(action)) else {
        return fail("no such action");
    };
    let flats = def.flat_bindings();
    if op == 85 {
        return flats.len() as i64;
    }
    let index = v[0];
    let Some(fb) = (index >= 0.0)
        .then_some(index as usize)
        .and_then(|i| flats.get(i).copied())
    else {
        return fail(format!("binding index {index} is out of range"));
    };
    let b = &def.bindings[fb.binding];
    let (name, path, groups, bid, composite) = match fb.part {
        None => (&b.name, &b.path, &b.groups, &b.id, b.is_composite()),
        Some(p) => {
            let part = &b.parts[p];
            (&part.name, &part.path, &part.groups, &part.id, false)
        }
    };
    let effective = s
        .effective_path(map, action, index as usize)
        .unwrap_or_default();
    let overridden = effective != *path;
    let text = format!("{name}\n{path}\n{effective}\n{groups}\n{bid}");
    // The text length goes in v[15]; the result carries the flags.
    v[15] = write_text(buf, len, &text) as f64;
    flag(composite) | flag(fb.part.is_some()) << 1 | flag(overridden) << 2
}

/// Ops 130 ..= 139: rebinding operations.
pub(super) fn rebinding(
    w: &mut World,
    op: i32,
    id: u64,
    arg: i64,
    v: &mut [f64; 16],
    text: &str,
) -> i64 {
    let sys = &mut w.input_system;
    let done = |r: Result<(), String>| match r {
        Ok(()) => 1,
        Err(e) => fail(e),
    };
    let gone = || fail("the rebinding operation is over (completed, canceled or disposed)");
    if op == 130 {
        let (map, action) = unpack(arg);
        let index = (v[0] >= 0.0).then_some(v[0] as usize);
        return match sys.new_rebind(id as usize, map, action, index) {
            Ok(r) => r as i64,
            Err(e) => fail(format!("PerformInteractiveRebinding: {e}")),
        };
    }
    if op == 139 {
        return match sys.rebind(id) {
            Some(r) => {
                v[0] = r.flat as f64;
                v[1] = f64::from(u8::from(r.started));
                1
            }
            None => 0,
        };
    }
    if op == 137 || op == 138 {
        let started = sys.rebind(id).is_some_and(|r| r.started);
        sys.cancel_rebind(id);
        return flag(op == 137 && started);
    }
    let Some(r) = sys.rebind_mut(id) else {
        return gone();
    };
    match op {
        131 => {
            r.excluded.push(text.to_owned());
            1
        }
        132 => {
            r.cancel = vec![text.to_owned()];
            1
        }
        135 => {
            r.wait = v[0].max(0.0);
            1
        }
        133 => {
            if v[0] < 0.0 {
                return fail("WithTargetBinding: the index can't be negative");
            }
            done(sys.retarget_rebind(id, v[0] as usize))
        }
        134 => done(sys.rebind_group(id, text)),
        136 => {
            sys.start_rebind(id);
            1
        }
        _ => fail(format!("unknown rebinding op {op}")),
    }
}

#[cfg(test)]
#[path = "csharp_rebind_tests.rs"]
mod tests;
