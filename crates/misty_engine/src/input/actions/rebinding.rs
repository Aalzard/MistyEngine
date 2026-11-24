//! More of interactive rebinding (`PerformInteractiveRebinding`; the listening itself is in
//! [`super::system`]): `WithBindingGroup`, and which device a candidate control is on.

use super::path::DeviceKind;
use super::system::InputSystem;

/// The device a control path such as `<Gamepad>/buttonSouth` is on.
pub fn device_of(path: &str) -> Option<DeviceKind> {
    let rest = path.trim().strip_prefix('<')?;
    let end = rest.find('>')?;
    DeviceKind::parse(&rest[..end])
}

impl InputSystem {
    /// `WithBindingGroup(group)`: rebind a binding of control scheme group `group` ("Gamepad",
    /// "Keyboard&Mouse") and only listen to that scheme's devices. When the binding picked so
    /// far isn't in the group, the first binding of the action in it (not a composite) is
    /// picked instead.
    pub fn rebind_group(&mut self, id: u64, group: &str) -> Result<(), String> {
        let r = self.rebind(id).ok_or("the rebinding operation is gone")?;
        if r.started {
            return Err("the rebinding operation already started".into());
        }
        let (set, map, action, flat) = (r.set, r.map, r.action, r.flat);
        let s = self.set(set).ok_or("no such actions")?;
        let def = s
            .asset
            .maps
            .get(map)
            .and_then(|m| m.actions.get(action))
            .ok_or("no such action")?;
        let flats = def.flat_bindings();
        let in_group = |f: usize| {
            let fb = flats[f];
            let b = &def.bindings[fb.binding];
            match fb.part {
                Some(p) => {
                    let part = &b.parts[p];
                    if part.groups.trim().is_empty() {
                        super::asset::has_group(&b.groups, group)
                    } else {
                        super::asset::has_group(&part.groups, group)
                    }
                }
                None => !b.is_composite() && super::asset::has_group(&b.groups, group),
            }
        };
        let target = if flat < flats.len() && in_group(flat) {
            flat
        } else {
            (0..flats.len())
                .find(|f| in_group(*f))
                .ok_or_else(|| format!("the action has no binding in group '{group}'"))?
        };
        let devices: Vec<DeviceKind> = s
            .asset
            .control_schemes
            .iter()
            .find(|c| c.group().eq_ignore_ascii_case(group))
            .map(|c| {
                c.devices
                    .iter()
                    .filter_map(|d| DeviceKind::parse(d.path.trim_matches(['<', '>'])))
                    .collect()
            })
            .unwrap_or_default();
        if target != flat {
            self.retarget_rebind(id, target)?;
        }
        if let Some(r) = self.rebind_mut(id) {
            r.group = Some(group.to_owned());
            r.devices = devices;
        }
        Ok(())
    }
}

/// Does a rebind limited to `devices` (empty = all) take candidate control `path`?
pub fn listens_to(devices: &[DeviceKind], path: &str) -> bool {
    if devices.is_empty() {
        return true;
    }
    match device_of(path) {
        Some(d) => {
            devices.contains(&d)
                || (devices.contains(&DeviceKind::Pointer)
                    && matches!(d, DeviceKind::Mouse | DeviceKind::Touchscreen))
        }
        None => false,
    }
}

#[cfg(test)]
#[path = "rebinding_tests.rs"]
mod tests;
