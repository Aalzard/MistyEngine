//! The engine functions C# scripts call (`Misty.Internal.NativeApi` in `dotnet/MistyEngine.cs`).
//!
//! C# gets a table of plain `extern "C"` function pointers in `Host.Init`. Each works on the
//! [`World`] installed for the current call into .NET ([`Enter`]); outside a call (or for a
//! destroyed object) they report failure and the C# side throws. Objects are named by their
//! `hecs::Entity` bits; strings travel as UTF-16 pointer + length.

use std::cell::Cell;

use glam::{Quat, Vec3};
use hecs::Entity;

use super::{LogKind, World};

// ---- the world, for callbacks from C# ------------------------------------------------------

thread_local! {
    /// The game world while C# code runs on this thread (null otherwise).
    static WORLD: Cell<*mut World> = const { Cell::new(std::ptr::null_mut()) };
}

/// Installs `w` for C# callbacks until dropped.
pub(super) struct Enter {
    previous: *mut World,
}

impl Enter {
    pub(super) fn new(w: &mut World) -> Self {
        let previous = WORLD.with(|c| c.replace(w as *mut World));
        Self { previous }
    }
}

impl Drop for Enter {
    fn drop(&mut self) {
        WORLD.with(|c| c.set(self.previous));
    }
}

/// Run `f` on the world installed by [`Enter`] (None outside a C# call).
fn with_world<R>(f: impl FnOnce(&mut World) -> R) -> Option<R> {
    let p = WORLD.with(|c| c.get());
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` was set by `Enter::new` from a `&mut World` that outlives the guard, on this
    // thread, and the code holding that borrow is blocked inside the C# call that led here; the
    // C# side runs one callback at a time, so there is no other live reference.
    Some(f(unsafe { &mut *p }))
}

/// `f` on the live object `id` (None when the id is no object, it was destroyed, or no world
/// is installed).
fn with_object<R>(id: u64, f: impl FnOnce(&mut World, Entity) -> R) -> Option<R> {
    let e = Entity::from_bits(id)?;
    with_world(|w| w.scene.contains(e).then(|| f(w, e))).flatten()
}

fn text(ptr: *const u16, len: i32) -> String {
    if ptr.is_null() || len <= 0 {
        return String::new();
    }
    // SAFETY: C# passes a pinned UTF-16 string of `len` chars.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
}

fn id_of(e: Entity) -> u64 {
    e.to_bits().get()
}

/// The Input Manager functions are off when only the Input System is active.
fn legacy_input(w: &World) -> bool {
    w.input_system.handling.legacy_allowed()
}

// ---- the table ---------------------------------------------------------------------------

/// Same layout as `Misty.Internal.TransformData`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TransformData {
    position: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}

type V3 = [f32; 3];
type Q = [f32; 4];

/// Same field order as `Misty.Internal.NativeApi`.
#[repr(C)]
pub(super) struct NativeApi {
    log: extern "C" fn(i32, *const u16, i32),
    get_transform: extern "C" fn(u64, *mut TransformData) -> i32,
    set_transform: extern "C" fn(u64, *const TransformData) -> i32,
    euler_to_quat: extern "C" fn(*const V3, *mut Q),
    quat_to_euler: extern "C" fn(*const Q, *mut V3),
    get_string: extern "C" fn(u64, i32, *mut u16, i32) -> i32,
    set_string: extern "C" fn(u64, i32, *const u16, i32) -> i32,
    get_world: extern "C" fn(u64, *mut TransformData) -> i32,
    set_world_position: extern "C" fn(u64, *const V3) -> i32,
    set_world_rotation: extern "C" fn(u64, *const Q) -> i32,
    get_flag: extern "C" fn(u64, i32) -> i32,
    set_active: extern "C" fn(u64, i32) -> i32,
    relative: extern "C" fn(u64, i32, i32) -> u64,
    set_parent: extern "C" fn(u64, u64, i32) -> i32,
    find: extern "C" fn(i32, *const u16, i32) -> u64,
    find_child: extern "C" fn(u64, *const u16, i32) -> u64,
    find_all_with_tag: extern "C" fn(*const u16, i32, *mut u64, i32) -> i32,
    instantiate: extern "C" fn(u64, i32, *const V3, *const Q, u64) -> u64,
    destroy: extern "C" fn(u64) -> i32,
    script_enabled: extern "C" fn(u64, i32, i32) -> i32,
    input_key: extern "C" fn(i32, *const u16, i32) -> i32,
    input_mouse: extern "C" fn(i32, i32) -> i32,
    input_axis: extern "C" fn(i32, *const u16, i32, *mut f32) -> i32,
    input_button: extern "C" fn(i32, *const u16, i32) -> i32,
    input_vector: extern "C" fn(i32, *mut V3),
    input_flag: extern "C" fn(i32) -> i32,
    look_at: extern "C" fn(u64, *const V3) -> i32,
    has_component: extern "C" fn(u64, i32) -> i32,
    rigidbody_vec: extern "C" fn(u64, i32, *mut V3, i32) -> i32,
    rigidbody_move_rotation: extern "C" fn(u64, *const Q) -> i32,
    rigidbody_scalar: extern "C" fn(u64, i32, i32, f32, *mut f32) -> i32,
    raycast: extern "C" fn(*const V3, *const V3, f32, u32, f32, *mut HitData) -> i32,
    raycast_all: extern "C" fn(*const V3, *const V3, f32, u32, *mut HitData, i32) -> i32,
    overlap_sphere: extern "C" fn(*const V3, f32, u32, *mut u64, i32) -> i32,
    gravity: extern "C" fn(i32, *mut V3),
    layer: extern "C" fn(i32, *const u16, i32, *mut u16, i32) -> i32,
    audio: extern "C" fn(u64, i32) -> i32,
    audio_scalar: extern "C" fn(u64, i32, i32, f32, *mut f32) -> i32,
    animator_param: extern "C" fn(u64, i32, *const u16, i32, i32, f32, *mut f32) -> i32,
    animator_play: extern "C" fn(u64, i32, *const u16, i32, i32, f32, f32) -> i32,
    request: extern "C" fn(i32, i64, *const u16, i32),
}

pub(super) static NATIVE_API: NativeApi = NativeApi {
    log,
    get_transform,
    set_transform,
    euler_to_quat,
    quat_to_euler,
    get_string,
    set_string,
    get_world,
    set_world_position,
    set_world_rotation,
    get_flag,
    set_active,
    relative,
    set_parent,
    find,
    find_child,
    find_all_with_tag,
    instantiate,
    destroy,
    script_enabled,
    input_key,
    input_mouse,
    input_axis,
    input_button,
    input_vector,
    input_flag,
    look_at,
    has_component,
    rigidbody_vec,
    rigidbody_move_rotation,
    rigidbody_scalar,
    raycast,
    raycast_all,
    overlap_sphere,
    gravity,
    layer,
    audio,
    audio_scalar,
    animator_param,
    animator_play,
    request,
};

/// Same layout as `Misty.Internal.HitData`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct HitData {
    point: V3,
    normal: V3,
    distance: f32,
    entity: u64,
    /// The hit collider's Rigidbody object (0 for none).
    body: u64,
}

fn hit_data(w: &World, h: &crate::physics::RaycastHit) -> HitData {
    HitData {
        point: h.point.to_array(),
        normal: h.normal.to_array(),
        distance: h.distance,
        entity: id_of(h.entity),
        body: h
            .body
            .filter(|b| w.scene.world.get::<&crate::scene::Rigidbody>(*b).is_ok())
            .map_or(0, id_of),
    }
}

fn read_v3(p: *const V3) -> Option<Vec3> {
    // SAFETY: C# passes pointers to Vector3s on its stack (or null).
    (!p.is_null()).then(|| Vec3::from_array(unsafe { *p }))
}

// ---- console and math ----------------------------------------------------------------------

extern "C" fn log(kind: i32, ptr: *const u16, len: i32) {
    let message = text(ptr, len);
    let kind = match kind {
        1 => LogKind::Warning,
        2 => LogKind::Error,
        _ => LogKind::Info,
    };
    if with_world(|w| w.logs.push((kind, message.clone()))).is_none() {
        eprintln!("[C#] {message}");
    }
}

extern "C" fn euler_to_quat(e: *const V3, out: *mut Q) {
    if e.is_null() || out.is_null() {
        return;
    }
    // SAFETY: both point at structs on the C# stack.
    unsafe {
        let q = crate::camera::euler_to_quat(Vec3::from_array(*e));
        out.write(q.to_array());
    }
}

extern "C" fn quat_to_euler(q: *const Q, out: *mut V3) {
    if q.is_null() || out.is_null() {
        return;
    }
    // SAFETY: both point at structs on the C# stack.
    unsafe {
        let e = crate::camera::quat_to_euler(Quat::from_array(*q).normalize());
        out.write(e.to_array());
    }
}

// ---- transforms ----------------------------------------------------------------------------

extern "C" fn get_transform(id: u64, out: *mut TransformData) -> i32 {
    if out.is_null() {
        return 0;
    }
    with_object(id, |w, e| {
        let t = w.scene.transform(e);
        let data = TransformData {
            position: t.position.to_array(),
            rotation: t.rotation.to_array(),
            scale: t.scale.to_array(),
        };
        // SAFETY: `out` points at a TransformData on the C# stack.
        unsafe { out.write(data) };
        1
    })
    .unwrap_or(0)
}

extern "C" fn set_transform(id: u64, data: *const TransformData) -> i32 {
    if data.is_null() {
        return 0;
    }
    // SAFETY: `data` points at a TransformData on the C# stack.
    let d = unsafe { data.read() };
    with_object(id, |w, e| {
        let mut t = w.scene.transform(e);
        t.position = Vec3::from_array(d.position);
        t.set_rotation(Quat::from_array(d.rotation).normalize());
        t.scale = Vec3::from_array(d.scale);
        w.scene.set_transform(e, t);
        super::api::sync_body(w, e);
        1
    })
    .unwrap_or(0)
}

extern "C" fn get_world(id: u64, out: *mut TransformData) -> i32 {
    if out.is_null() {
        return 0;
    }
    with_object(id, |w, e| {
        let (scale, _, _) = w.scene.world_matrix(e).to_scale_rotation_translation();
        let data = TransformData {
            position: w.scene.world_position(e).to_array(),
            rotation: w.scene.world_rotation(e).to_array(),
            scale: scale.to_array(),
        };
        // SAFETY: `out` points at a TransformData on the C# stack.
        unsafe { out.write(data) };
        1
    })
    .unwrap_or(0)
}

extern "C" fn set_world_position(id: u64, p: *const V3) -> i32 {
    if p.is_null() {
        return 0;
    }
    // SAFETY: points at a Vector3 on the C# stack.
    let p = Vec3::from_array(unsafe { *p });
    with_object(id, |w, e| {
        w.scene.set_world_position(e, p);
        super::api::sync_body(w, e);
        1
    })
    .unwrap_or(0)
}

extern "C" fn set_world_rotation(id: u64, q: *const Q) -> i32 {
    if q.is_null() {
        return 0;
    }
    // SAFETY: points at a Quaternion on the C# stack.
    let q = Quat::from_array(unsafe { *q }).normalize();
    with_object(id, |w, e| {
        w.scene.set_world_rotation(e, q);
        super::api::sync_body(w, e);
        1
    })
    .unwrap_or(0)
}

extern "C" fn look_at(id: u64, point: *const V3) -> i32 {
    if point.is_null() {
        return 0;
    }
    // SAFETY: points at a Vector3 on the C# stack.
    let point = Vec3::from_array(unsafe { *point });
    with_object(id, |w, e| {
        let dir = (point - w.scene.world_position(e)).normalize_or_zero();
        if dir != Vec3::ZERO {
            let up = if dir.y.abs() > 0.999 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let q = glam::camera::lh::view::look_to_quat(dir, up).inverse();
            w.scene.set_world_rotation(e, q);
            super::api::sync_body(w, e);
        }
        1
    })
    .unwrap_or(0)
}

// ---- objects -------------------------------------------------------------------------------

/// `which`: 0 name, 1 tag. Returns the full length (copying what fits), -1 for no object.
extern "C" fn get_string(id: u64, which: i32, buf: *mut u16, cap: i32) -> i32 {
    with_object(id, |w, e| {
        let s = match which {
            1 => w.scene.game_object(e).tag.clone(),
            _ => w.scene.name(e),
        };
        let units: Vec<u16> = s.encode_utf16().collect();
        if !buf.is_null() && cap > 0 {
            let n = units.len().min(cap as usize);
            // SAFETY: `buf` holds `cap` chars on the C# side.
            unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), buf, n) };
        }
        units.len() as i32
    })
    .unwrap_or(-1)
}

/// `which`: 0 name, 1 tag. 1 done, 0 an undefined tag, -1 no object.
extern "C" fn set_string(id: u64, which: i32, ptr: *const u16, len: i32) -> i32 {
    let value = text(ptr, len);
    with_object(id, |w, e| {
        if which == 1 {
            if !w.tags_layers.has_tag(&value) {
                return 0;
            }
            w.scene.game_object_mut(e).tag = value;
        } else {
            w.scene.game_object_mut(e).name = value;
        }
        1
    })
    .unwrap_or(-1)
}

/// `which`: 0 activeSelf, 1 activeInHierarchy (-1 for no object), 2 whether the object exists.
extern "C" fn get_flag(id: u64, which: i32) -> i32 {
    if which == 2 {
        return with_object(id, |_, _| 1).unwrap_or(0);
    }
    with_object(id, |w, e| {
        let on = match which {
            1 => w.scene.active_in_hierarchy(e),
            _ => w.scene.game_object(e).active,
        };
        i32::from(on)
    })
    .unwrap_or(-1)
}

extern "C" fn set_active(id: u64, on: i32) -> i32 {
    with_object(id, |w, e| {
        w.scene.set_active(e, on != 0);
        1
    })
    .unwrap_or(0)
}

/// `which`: 0 the parent (0 for none), 1 child `index` (0 past the end), 2 the child count.
extern "C" fn relative(id: u64, which: i32, index: i32) -> u64 {
    with_object(id, |w, e| match which {
        0 => w.scene.parent(e).map_or(0, id_of),
        1 => usize::try_from(index)
            .ok()
            .and_then(|i| w.scene.children(e).get(i).copied())
            .map_or(0, id_of),
        _ => w.scene.children(e).len() as u64,
    })
    .unwrap_or(0)
}

/// 1 done, 0 the move would put the object under itself, -1 no object (or no such parent).
extern "C" fn set_parent(id: u64, parent: u64, keep_world: i32) -> i32 {
    with_object(id, |w, e| {
        let parent = match parent {
            0 => None,
            p => match Entity::from_bits(p).filter(|p| w.scene.contains(*p)) {
                Some(p) => Some(p),
                None => return -1,
            },
        };
        if parent == Some(e) || !w.scene.set_parent(e, parent, None, keep_world != 0) {
            return 0;
        }
        super::api::sync_body(w, e);
        1
    })
    .unwrap_or(-1)
}

/// Active objects with `tag` in Hierarchy order; None for an undefined tag.
fn tagged(w: &World, tag: &str) -> Option<Vec<Entity>> {
    if !w.tags_layers.has_tag(tag) {
        return None;
    }
    Some(
        w.scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| w.scene.active_in_hierarchy(*e) && w.scene.game_object(*e).tag == tag)
            .collect(),
    )
}

/// `which`: 0 by name, 1 the first active object with a tag (`u64::MAX` for an undefined
/// tag). 0 when nothing matches.
extern "C" fn find(which: i32, ptr: *const u16, len: i32) -> u64 {
    let name = text(ptr, len);
    with_world(|w| match which {
        1 => match tagged(w, &name) {
            Some(found) => found.first().map_or(0, |e| id_of(*e)),
            None => u64::MAX,
        },
        _ => w.scene.find_by_name(&name).map_or(0, id_of),
    })
    .unwrap_or(0)
}

/// A child by name or a descendant by a slash-separated path; 0 when missing.
extern "C" fn find_child(id: u64, ptr: *const u16, len: i32) -> u64 {
    let path = text(ptr, len);
    with_object(id, |w, e| {
        let mut cur = e;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            match w
                .scene
                .children(cur)
                .into_iter()
                .find(|c| w.scene.name(*c) == part)
            {
                Some(c) => cur = c,
                None => return 0,
            }
        }
        if cur == e { 0 } else { id_of(cur) }
    })
    .unwrap_or(0)
}

/// Writes up to `cap` ids; returns how many there are (-1 for an undefined tag).
extern "C" fn find_all_with_tag(ptr: *const u16, len: i32, out: *mut u64, cap: i32) -> i32 {
    let tag = text(ptr, len);
    with_world(|w| match tagged(w, &tag) {
        None => -1,
        Some(found) => {
            if !out.is_null() && cap > 0 {
                for (i, e) in found.iter().take(cap as usize).enumerate() {
                    // SAFETY: `out` holds `cap` ids on the C# side.
                    unsafe { out.add(i).write(id_of(*e)) };
                }
            }
            found.len() as i32
        }
    })
    .unwrap_or(-1)
}

/// Copy object `src`; `flags` bit 0: place at `pos` (world), bit 1: turn to `rot` (world);
/// `parent` 0 for the scene root. The copy's scripts are made after the current callback, like
/// Lua's `Instantiate`. Returns the copy, or 0 (the reason is in the Console).
extern "C" fn instantiate(src: u64, flags: i32, pos: *const V3, rot: *const Q, parent: u64) -> u64 {
    let Some(src) = Entity::from_bits(src) else {
        return 0;
    };
    // SAFETY: both point at structs on the C# stack (read only when their flag is set).
    let pos = (flags & 1 != 0 && !pos.is_null()).then(|| Vec3::from_array(unsafe { *pos }));
    let rot = (flags & 2 != 0 && !rot.is_null()).then(|| Quat::from_array(unsafe { *rot }));
    with_world(|w| {
        let copy = match super::api::instantiate(w, super::api::Source::Object(src), pos) {
            Ok(e) => e,
            Err(err) => {
                w.logs.push((LogKind::Error, format!("Instantiate: {err}")));
                return 0;
            }
        };
        if let Some(q) = rot {
            w.scene.set_world_rotation(copy, q.normalize());
        }
        if let Some(p) = Entity::from_bits(parent).filter(|p| w.scene.contains(*p)) {
            // With a position the copy keeps its world placement; without, it takes the
            // original's local placement under the new parent (Unity's default).
            w.scene
                .set_parent(copy, Some(p), None, pos.is_some() || rot.is_some());
        }
        super::api::sync_body(w, copy);
        id_of(copy)
    })
    .unwrap_or(0)
}

/// Destroy the object (and its children) after this frame's updates.
extern "C" fn destroy(id: u64) -> i32 {
    with_object(id, |w, e| {
        w.destroy.push(e);
        1
    })
    .unwrap_or(0)
}

/// A script's checkbox: `set` -1 reads it, 0 / 1 sets it. Returns the value, -1 for no object.
extern "C" fn script_enabled(id: u64, slot: i32, set: i32) -> i32 {
    let Ok(slot) = usize::try_from(slot) else {
        return -1;
    };
    with_object(id, |w, e| {
        if set >= 0 {
            w.scene.set_script_enabled(e, slot, set != 0);
        }
        let on = w
            .scene
            .world
            .get::<&crate::scene::Scripts>(e)
            .ok()
            .and_then(|s| s.list.get(slot).map(|r| r.enabled))
            .unwrap_or(false);
        i32::from(on)
    })
    .unwrap_or(-1)
}

// ---- input ---------------------------------------------------------------------------------

/// `phase`: 0 held, 1 pressed this frame, 2 released. -1 when only the Input System is on.
extern "C" fn input_key(phase: i32, ptr: *const u16, len: i32) -> i32 {
    let name = text(ptr, len);
    with_world(|w| {
        if !legacy_input(w) {
            return -1;
        }
        i32::from(match phase {
            1 => w.input.key_down(&name),
            2 => w.input.key_up(&name),
            _ => w.input.key(&name),
        })
    })
    .unwrap_or(0)
}

extern "C" fn input_mouse(phase: i32, button: i32) -> i32 {
    let Ok(b) = usize::try_from(button) else {
        return 0;
    };
    with_world(|w| {
        if !legacy_input(w) {
            return -1;
        }
        i32::from(match phase {
            1 => w.input.mouse_down(b),
            2 => w.input.mouse_up(b),
            _ => w.input.mouse(b),
        })
    })
    .unwrap_or(0)
}

/// 1 with the value in `out`, 0 for an axis that isn't set up, -1 when only the Input System
/// is on.
extern "C" fn input_axis(raw: i32, ptr: *const u16, len: i32, out: *mut f32) -> i32 {
    let name = text(ptr, len);
    with_world(|w| {
        if !legacy_input(w) {
            return -1;
        }
        let v = if raw != 0 {
            w.axes.axis_raw(&name)
        } else {
            w.axes.axis(&name)
        };
        match v {
            Some(v) if !out.is_null() => {
                // SAFETY: `out` points at a float on the C# stack.
                unsafe { out.write(v) };
                1
            }
            _ => 0,
        }
    })
    .unwrap_or(0)
}

/// 1 / 0 for the button's state, -2 for a button that isn't set up, -1 when only the Input
/// System is on.
extern "C" fn input_button(phase: i32, ptr: *const u16, len: i32) -> i32 {
    let name = text(ptr, len);
    with_world(|w| {
        if !legacy_input(w) {
            return -1;
        }
        let test = match phase {
            1 => super::Input::button_pressed,
            2 => super::Input::button_released,
            _ => super::Input::button_held,
        };
        match w.axes.button(&name, &w.input, test) {
            Some(on) => i32::from(on),
            None => -2,
        }
    })
    .unwrap_or(0)
}

/// `which`: 0 mousePosition (pixels, y up), 1 mouseScrollDelta (notches), 2 mouseDelta.
extern "C" fn input_vector(which: i32, out: *mut V3) {
    if out.is_null() {
        return;
    }
    let v = with_world(|w| match which {
        1 => {
            let n = w.input.scroll_notches();
            Vec3::new(n.x, n.y, 0.0)
        }
        2 => {
            let d = w.input.mouse_delta;
            Vec3::new(d.x, -d.y, 0.0)
        }
        _ => {
            let p = w.input.mouse_position.unwrap_or_default();
            Vec3::new(p.x, super::camera_api::screen(w).y - p.y, 0.0)
        }
    })
    .unwrap_or_default();
    // SAFETY: `out` points at a Vector3 on the C# stack.
    unsafe { out.write(v.to_array()) };
}

/// `which`: 0 anyKey, 1 anyKeyDown.
extern "C" fn input_flag(which: i32) -> i32 {
    with_world(|w| {
        i32::from(match which {
            1 => w.input.any_key_down(),
            _ => w.input.any_key(),
        })
    })
    .unwrap_or(0)
}

// ---- engine components ---------------------------------------------------------------------

/// `kind`: 0 Rigidbody, 1 any Collider, 2 AudioSource, 3 Animator. 1 when the object has one.
extern "C" fn has_component(id: u64, kind: i32) -> i32 {
    use crate::scene::{
        Animator, AudioSource, BoxCollider, CapsuleCollider, MeshCollider, Rigidbody,
        SphereCollider,
    };
    with_object(id, |w, e| {
        let world = &w.scene.world;
        let has = match kind {
            0 => world.get::<&Rigidbody>(e).is_ok(),
            1 => {
                world.get::<&BoxCollider>(e).is_ok()
                    || world.get::<&SphereCollider>(e).is_ok()
                    || world.get::<&CapsuleCollider>(e).is_ok()
                    || world.get::<&MeshCollider>(e).is_ok()
            }
            2 => world.get::<&AudioSource>(e).is_ok(),
            3 => world.get::<&Animator>(e).is_ok(),
            _ => false,
        };
        i32::from(has)
    })
    .unwrap_or(0)
}

fn has_rigidbody(w: &World, e: Entity) -> bool {
    w.scene.world.get::<&crate::scene::Rigidbody>(e).is_ok()
}

/// `op`: 0 get velocity, 1 set velocity, 2 get angular velocity, 3 set angular velocity,
/// 4 AddForce, 5 AddTorque (`mode` 0 Force, 1 Impulse, 2 VelocityChange, 3 Acceleration),
/// 6 MovePosition, 7 get the body's position. 1 done, 0 no object or no Rigidbody.
extern "C" fn rigidbody_vec(id: u64, op: i32, v: *mut V3, mode: i32) -> i32 {
    if v.is_null() {
        return 0;
    }
    // SAFETY: `v` points at a Vector3 on the C# stack.
    let value = Vec3::from_array(unsafe { *v });
    // SAFETY: as above.
    let write = |out: Vec3| unsafe { v.write(out.to_array()) };
    with_object(id, |w, e| {
        if !has_rigidbody(w, e) {
            return 0;
        }
        let mode = match mode {
            1 => "Impulse",
            2 => "VelocityChange",
            3 => "Acceleration",
            _ => "Force",
        };
        match op {
            0 => write(w.physics.velocity(e).unwrap_or_default()),
            1 => w.physics.set_velocity(e, value),
            2 => write(w.physics.angular_velocity(e).unwrap_or_default()),
            3 => w.physics.set_angular_velocity(e, value),
            4 => {
                let dv = super::api::delta_velocity(w, e, value, mode);
                let vel = w.physics.velocity(e).unwrap_or_default();
                w.physics.set_velocity(e, vel + dv);
            }
            5 => {
                let dw = super::api::delta_velocity(w, e, value, mode);
                let ang = w.physics.angular_velocity(e).unwrap_or_default();
                w.physics.set_angular_velocity(e, ang + dw);
            }
            6 => {
                let rot = w.scene.world_rotation(e);
                let World { physics, scene, .. } = w;
                physics.move_body(scene, e, value, rot);
            }
            _ => write(match w.physics.body_pose(e) {
                Some((p, _)) => p,
                None => w.scene.world_position(e),
            }),
        }
        1
    })
    .unwrap_or(0)
}

extern "C" fn rigidbody_move_rotation(id: u64, q: *const Q) -> i32 {
    if q.is_null() {
        return 0;
    }
    // SAFETY: points at a Quaternion on the C# stack.
    let q = Quat::from_array(unsafe { *q }).normalize();
    with_object(id, |w, e| {
        if !has_rigidbody(w, e) {
            return 0;
        }
        let pos = w.scene.world_position(e);
        let World { physics, scene, .. } = w;
        physics.move_body(scene, e, pos, q);
        1
    })
    .unwrap_or(0)
}

/// `which`: 0 mass, 1 isKinematic, 2 useGravity (booleans as 0 / 1); `set` 0 reads into `out`,
/// 1 writes `value`. 1 done, 0 no object or no Rigidbody.
extern "C" fn rigidbody_scalar(id: u64, which: i32, set: i32, value: f32, out: *mut f32) -> i32 {
    with_object(id, |w, e| {
        let Ok(mut b) = w.scene.world.get::<&mut crate::scene::Rigidbody>(e) else {
            return 0;
        };
        if set != 0 {
            let on = value != 0.0;
            match which {
                1 => b.is_kinematic = on,
                2 => b.use_gravity = on,
                _ => b.mass = value.max(1e-4),
            }
            let mass = b.mass;
            drop(b);
            match which {
                1 => w.physics.set_kinematic(e, on),
                2 => w.physics.set_use_gravity(e, on),
                _ => w.physics.set_mass(e, mass),
            }
        } else if !out.is_null() {
            let v = match which {
                1 => f32::from(u8::from(b.is_kinematic)),
                2 => f32::from(u8::from(b.use_gravity)),
                _ => b.mass,
            };
            // SAFETY: `out` points at a float on the C# stack.
            unsafe { out.write(v) };
        }
        1
    })
    .unwrap_or(0)
}

// ---- physics queries -----------------------------------------------------------------------

/// The first hit along a ray (`radius` 0) or a sphere sweep; 1 with `out` filled, else 0.
extern "C" fn raycast(
    origin: *const V3,
    dir: *const V3,
    max: f32,
    mask: u32,
    radius: f32,
    out: *mut HitData,
) -> i32 {
    let (Some(o), Some(d)) = (read_v3(origin), read_v3(dir)) else {
        return 0;
    };
    with_world(|w| {
        let hit = if radius > 0.0 {
            w.physics.sphere_cast(o, radius, d, max, mask)
        } else {
            w.physics.raycast(o, d, max, mask)
        };
        match hit {
            Some(h) => {
                if !out.is_null() {
                    // SAFETY: `out` points at a HitData on the C# stack.
                    unsafe { out.write(hit_data(w, &h)) };
                }
                1
            }
            None => 0,
        }
    })
    .unwrap_or(0)
}

/// Every hit along a ray, nearest first: writes up to `cap`, returns how many there are.
extern "C" fn raycast_all(
    origin: *const V3,
    dir: *const V3,
    max: f32,
    mask: u32,
    out: *mut HitData,
    cap: i32,
) -> i32 {
    let (Some(o), Some(d)) = (read_v3(origin), read_v3(dir)) else {
        return 0;
    };
    with_world(|w| {
        let hits = w.physics.raycast_all(o, d, max, mask);
        if !out.is_null() && cap > 0 {
            for (i, h) in hits.iter().take(cap as usize).enumerate() {
                // SAFETY: `out` holds `cap` HitData on the C# side.
                unsafe { out.add(i).write(hit_data(w, h)) };
            }
        }
        hits.len() as i32
    })
    .unwrap_or(0)
}

/// Objects whose colliders touch the sphere: writes up to `cap` ids, returns how many.
extern "C" fn overlap_sphere(
    center: *const V3,
    radius: f32,
    mask: u32,
    out: *mut u64,
    cap: i32,
) -> i32 {
    let Some(c) = read_v3(center) else { return 0 };
    with_world(|w| {
        let found = w.physics.overlap_sphere(c, radius, mask);
        if !out.is_null() && cap > 0 {
            for (i, e) in found.iter().take(cap as usize).enumerate() {
                // SAFETY: `out` holds `cap` ids on the C# side.
                unsafe { out.add(i).write(id_of(*e)) };
            }
        }
        found.len() as i32
    })
    .unwrap_or(0)
}

/// `set` 0 reads `Physics.gravity` into `v`, 1 sets it from `v`.
extern "C" fn gravity(set: i32, v: *mut V3) {
    if v.is_null() {
        return;
    }
    with_world(|w| {
        if set != 0 {
            // SAFETY: `v` points at a Vector3 on the C# stack.
            w.physics.set_gravity(Vec3::from_array(unsafe { *v }));
        } else {
            // SAFETY: as above.
            unsafe { v.write(w.physics.gravity().to_array()) };
        }
    });
}

/// `op`: 0 NameToLayer(name) (the layer or -1), 1 LayerToName(layer `len`) into `buf` (returns
/// the length), 2 GetMask for one name (the mask bits).
extern "C" fn layer(op: i32, ptr: *const u16, len: i32, buf: *mut u16, cap: i32) -> i32 {
    let name = if op == 1 {
        String::new()
    } else {
        text(ptr, len)
    };
    with_world(|w| {
        let l = &w.tags_layers;
        match op {
            1 => {
                let units: Vec<u16> = l.layer_name(len).encode_utf16().collect();
                if !buf.is_null() && cap > 0 {
                    let n = units.len().min(cap as usize);
                    // SAFETY: `buf` holds `cap` chars on the C# side.
                    unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), buf, n) };
                }
                units.len() as i32
            }
            2 => l.get_mask(&[name.as_str()]) as i32,
            _ => l.name_to_layer(&name),
        }
    })
    .unwrap_or(-1)
}

// ---- audio, animation, scenes --------------------------------------------------------------

/// `op`: 0 Play, 1 Stop, 2 Pause, 3 UnPause, 4 isPlaying. 1 / 0 (isPlaying's answer), -1 when
/// the object has no Audio Source.
extern "C" fn audio(id: u64, op: i32) -> i32 {
    with_object(id, |w, e| {
        if w.scene.world.get::<&crate::scene::AudioSource>(e).is_err() {
            return -1;
        }
        match op {
            0 => {
                let World { audio, scene, .. } = w;
                audio.play(scene, e);
            }
            1 => w.audio.stop(e),
            2 => w.audio.pause(e),
            3 => w.audio.unpause(e),
            _ => return i32::from(w.audio.is_playing(e)),
        }
        1
    })
    .unwrap_or(-1)
}

/// `which`: 0 volume, 1 pitch, 2 loop, 3 mute, 4 time (playback position); `set` 0 reads into
/// `out`, 1 writes `value`. 1 done, 0 no object or no Audio Source.
extern "C" fn audio_scalar(id: u64, which: i32, set: i32, value: f32, out: *mut f32) -> i32 {
    with_object(id, |w, e| {
        if which == 4 {
            if w.scene.world.get::<&crate::scene::AudioSource>(e).is_err() {
                return 0;
            }
            if set != 0 {
                w.audio.set_time(e, value);
            } else if !out.is_null() {
                // SAFETY: `out` points at a float on the C# stack.
                unsafe { out.write(w.audio.time(e)) };
            }
            return 1;
        }
        let Ok(mut s) = w.scene.world.get::<&mut crate::scene::AudioSource>(e) else {
            return 0;
        };
        if set != 0 {
            match which {
                1 => s.pitch = value.clamp(-3.0, 3.0),
                2 => s.looping = value != 0.0,
                3 => s.mute = value != 0.0,
                _ => s.volume = value.clamp(0.0, 1.0),
            }
        } else if !out.is_null() {
            let v = match which {
                1 => s.pitch,
                2 => f32::from(u8::from(s.looping)),
                3 => f32::from(u8::from(s.mute)),
                _ => s.volume,
            };
            // SAFETY: as above.
            unsafe { out.write(v) };
        }
        1
    })
    .unwrap_or(0)
}

/// `kind`: 0 float, 1 int, 2 bool, 3 trigger; `set` 0 reads into `out`, 1 writes `value`
/// (a trigger: 1 sets, 0 resets). 1 done, 0 no object or no Animator.
extern "C" fn animator_param(
    id: u64,
    kind: i32,
    ptr: *const u16,
    len: i32,
    set: i32,
    value: f32,
    out: *mut f32,
) -> i32 {
    use crate::animation::animator::{BOOL, FLOAT, INT, TRIGGER};
    let name = text(ptr, len);
    let kinds = match kind {
        1 => INT,
        2 => BOOL,
        3 => TRIGGER,
        _ => FLOAT,
    };
    with_object(id, |w, e| {
        if w.scene.world.get::<&crate::scene::Animator>(e).is_err() {
            return 0;
        }
        let World {
            animator, scene, ..
        } = w;
        if set != 0 {
            animator.set_param(scene, e, &name, kinds, value);
        } else if !out.is_null() {
            let v = animator.param(scene, e, &name, kinds);
            // SAFETY: `out` points at a float on the C# stack.
            unsafe { out.write(v) };
        }
        1
    })
    .unwrap_or(0)
}

/// `op`: 0 Play(state, layer, normalizedTime `a`, NaN for none), 1 CrossFade(state, duration
/// `a`, layer, offset `b`), 2 CrossFadeInFixedTime(state, seconds `a`, layer, offset `b`).
/// 1 when the state was found, 0 otherwise (or no Animator).
extern "C" fn animator_play(
    id: u64,
    op: i32,
    ptr: *const u16,
    len: i32,
    layer: i32,
    a: f32,
    b: f32,
) -> i32 {
    let state = text(ptr, len);
    with_object(id, |w, e| {
        if w.scene.world.get::<&crate::scene::Animator>(e).is_err() {
            return 0;
        }
        let World {
            animator, scene, ..
        } = w;
        let layer = i64::from(layer);
        let found = match op {
            1 => animator.cross_fade(scene, e, &state, a, false, layer, b),
            2 => animator.cross_fade(scene, e, &state, a, true, layer, b),
            _ => animator.play(scene, e, &state, layer, (!a.is_nan()).then_some(a)),
        };
        i32::from(found)
    })
    .unwrap_or(0)
}

/// `op`: 0 LoadScene, 1 LoadScene additive, 2 UnloadScene (by `index` when >= 0, else by
/// name), 3 Application.Quit. Carried out after the frame.
extern "C" fn request(op: i32, index: i64, ptr: *const u16, len: i32) {
    use super::Request;
    use crate::build::SceneRef;
    let scene = if index >= 0 {
        SceneRef::Index(index)
    } else {
        SceneRef::Name(text(ptr, len))
    };
    let r = match op {
        1 => Request::LoadSceneAdditive(scene),
        2 => Request::UnloadScene(scene),
        3 => Request::Quit,
        _ => Request::LoadScene(scene),
    };
    with_world(|w| w.requests.push(r));
}
