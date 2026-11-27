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

#[path = "csharp_parity.rs"]
mod parity;
pub(super) use parity::{Contact2DScope, Contact3DScope, ControllerHitScope};
use parity::{animator_ik, contacts};

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
pub(super) fn with_world<R>(f: impl FnOnce(&mut World) -> R) -> Option<R> {
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

pub(super) fn text(ptr: *const u16, len: i32) -> String {
    if ptr.is_null() || len <= 0 {
        return String::new();
    }
    // SAFETY: C# passes a pinned UTF-16 string of `len` chars.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
}

pub(super) fn id_of(e: Entity) -> u64 {
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
type V4 = [f32; 4];

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
    request: extern "C" fn(i32, i64, *const u16, i32, i32) -> i64,
    component_value: extern "C" fn(u64, i32, i32, i32, *mut V4) -> i32,
    camera_point: extern "C" fn(u64, i32, *const V3, *mut V3, *mut V3) -> i32,
    material: extern "C" fn(u64, i32, i32, *const u16, i32, *mut V4) -> i32,
    global: extern "C" fn(i32, i32, *mut V4) -> u64,
    component_text: extern "C" fn(u64, i32, i32, *mut u16, i32) -> i32,
    prefs: extern "C" fn(i32, *const u16, i32, *mut f64, *mut u16, i32) -> i32,
    rigidbody2d: extern "C" fn(u64, i32, *mut V3, i32) -> i32,
    nav_agent: extern "C" fn(u64, i32, *mut V3) -> i32,
    particles: extern "C" fn(u64, i32, *mut f32) -> i32,
    component_op: extern "C" fn(u64, i32, *const u16, i32) -> i32,
    scene_op: extern "C" fn(i64, i32) -> i32,
    message: extern "C" fn(u64, *const u16, i32, i32, f64, *const u16, i32) -> i32,
    physics2d:
        extern "C" fn(i32, *const V3, *const V3, f32, u32, *mut HitData, *mut u64, i32) -> i32,
    /// `csharp_input.rs`: EventSystem, touches, Camera.rect, Player Input (Manager), Input
    /// Actions, Gamepad / Keyboard / Mouse.
    input_system: extern "C" fn(i32, u64, i64, *mut f64, *mut u16, i32) -> i64,
    // ---- csharp_parity.rs ----
    contacts: extern "C" fn(i32, *mut f32, i32, *mut u64) -> i32,
    animator_ik: extern "C" fn(u64, i32, i32, *mut V4) -> i32,
    character_vec: extern "C" fn(u64, i32, *mut V3) -> i32,
    character_scalar: extern "C" fn(u64, i32, i32, f32, *mut f32) -> i32,
    time_scale: extern "C" fn(f32),
    transform_group: extern "C" fn(i32, *const u64, i32, *mut f32) -> i32,
    app: extern "C" fn(i32, i32, *const u16, i32, *mut u16, i32) -> i32,
    shape_query: extern "C" fn(i32, i32, *const f32, u32, *mut HitData, *mut u64, i32) -> i32,
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
    component_value,
    camera_point,
    material,
    global,
    component_text,
    prefs,
    rigidbody2d,
    nav_agent,
    particles,
    component_op,
    scene_op,
    message,
    physics2d,
    input_system: super::csharp::input::input_system,
    contacts,
    animator_ik,
    character_vec,
    character_scalar,
    time_scale,
    transform_group,
    app,
    shape_query,
};

/// `Application` and `ScreenCapture`. op 0: `CaptureScreenshot(text, superSize = arg)`;
/// 1: `persistentDataPath` (written to `out`, returns its length); 2: `HasDefine(text)` (1/0);
/// 3: `isEditor` (1/0). -1 outside a call from the engine (ops 0 and 1) or for an unknown op.
extern "C" fn app(op: i32, arg: i32, input: *const u16, len: i32, out: *mut u16, cap: i32) -> i32 {
    match op {
        0 => {
            let name = text(input, len);
            with_world(|w| crate::screen_capture::request(&w.root, &name, i64::from(arg)))
                .map_or(-1, |_| 1)
        }
        1 => with_world(|w| crate::screen_capture::data_folder(&w.root))
            .map_or(-1, |p| write_text(&p.to_string_lossy(), out, cap)),
        2 => i32::from(crate::build_profile::has_define(&text(input, len))),
        3 => i32::from(crate::build_profile::has_define("MISTY_EDITOR")),
        _ => -1,
    }
}

/// `TransformGroup` in C#: one call for `count` objects. `data` holds `width` floats per
/// object: op 0 reads world positions (3), 1 writes them, 2 reads world rotations (4), 3 writes
/// them, 4 turns the objects along directions (3), 5 writes 1 for a living object and 0 for a
/// destroyed one (1). Reads leave the floats of destroyed objects alone; writes skip them and
/// values that are not finite. Returns how many objects were read or written, -1 outside a
/// call from the engine.
extern "C" fn transform_group(op: i32, ids: *const u64, count: i32, data: *mut f32) -> i32 {
    use super::transform_group_api as group;
    let width = match op {
        0 | 1 | 4 => 3,
        2 | 3 => 4,
        5 => 1,
        _ => return -1,
    };
    if count <= 0 {
        return 0;
    }
    if ids.is_null() || data.is_null() {
        return -1;
    }
    // SAFETY: the C# side pins an id array of `count` entries and a value array of at least
    // `count * width` floats for the length of this call.
    let (ids, data) = unsafe {
        (
            std::slice::from_raw_parts(ids, count as usize),
            std::slice::from_raw_parts_mut(data, count as usize * width),
        )
    };
    with_world(|w| {
        let bodies = group::any_bodies(w);
        let mut done = 0;
        for (&id, v) in ids.iter().zip(data.chunks_exact_mut(width)) {
            let Some(e) = Entity::from_bits(id) else {
                if op == 5 {
                    v[0] = 0.0;
                }
                continue;
            };
            let ok = match op {
                0 => group::position(w, e)
                    .map(|p| v.copy_from_slice(&p.to_array()))
                    .is_some(),
                2 => group::rotation(w, e)
                    .map(|q| v.copy_from_slice(&q.to_array()))
                    .is_some(),
                5 => {
                    v[0] = f32::from(u8::from(w.scene.contains(e)));
                    true
                }
                _ if !v.iter().all(|x| x.is_finite()) => false,
                1 => group::set_position(w, e, Vec3::new(v[0], v[1], v[2])),
                3 => group::set_rotation(w, e, Quat::from_xyzw(v[0], v[1], v[2], v[3])),
                _ => group::look_along(w, e, Vec3::new(v[0], v[1], v[2])),
            };
            if ok && bodies && matches!(op, 1 | 3 | 4) {
                super::api::sync_body(w, e);
            }
            done += i32::from(ok);
        }
        done
    })
    .unwrap_or(-1)
}

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
            4 => world.get::<&crate::scene::Camera>(e).is_ok(),
            5 => world.get::<&crate::scene::Light>(e).is_ok(),
            6 => world.get::<&crate::scene::MeshRenderer>(e).is_ok(),
            10 => world.get::<&crate::scene::UiText>(e).is_ok(),
            11 => world.get::<&crate::scene::UiImage>(e).is_ok(),
            12 => world.get::<&crate::scene::UiButton>(e).is_ok(),
            13 => world.get::<&crate::scene::UiToggle>(e).is_ok(),
            14 => world.get::<&crate::scene::UiSlider>(e).is_ok(),
            15 => world.get::<&crate::scene::UiInputField>(e).is_ok(),
            20 => world.get::<&crate::scene::Rigidbody2D>(e).is_ok(),
            21 => {
                use crate::scene::{
                    BoxCollider2D, CapsuleCollider2D, CircleCollider2D, EdgeCollider2D,
                    PolygonCollider2D,
                };
                world.get::<&BoxCollider2D>(e).is_ok()
                    || world.get::<&CircleCollider2D>(e).is_ok()
                    || world.get::<&CapsuleCollider2D>(e).is_ok()
                    || world.get::<&PolygonCollider2D>(e).is_ok()
                    || world.get::<&EdgeCollider2D>(e).is_ok()
            }
            22 => world.get::<&crate::navigation::NavMeshAgent>(e).is_ok(),
            23 => world.get::<&crate::particles::ParticleSystem>(e).is_ok(),
            30 => world.get::<&BoxCollider>(e).is_ok(),
            31 => world.get::<&SphereCollider>(e).is_ok(),
            32 => world.get::<&CapsuleCollider>(e).is_ok(),
            33 => world.get::<&MeshCollider>(e).is_ok(),
            34 => world.get::<&crate::scene::BoxCollider2D>(e).is_ok(),
            35 => world.get::<&crate::scene::CircleCollider2D>(e).is_ok(),
            7 => world.get::<&crate::scene::CharacterController>(e).is_ok(),
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

// ---- character controller ------------------------------------------------------------------

/// Seconds a controller move made from the current callback covers.
fn move_dt(w: &World) -> f32 {
    if w.time.delta > 0.0 {
        w.time.delta
    } else {
        w.physics.fixed_dt
    }
}

/// `op`: 0 Move(v), 1 SimpleMove(v) (both write isGrounded into `v.x` as 0 / 1), 2 get
/// velocity, 3 get groundNormal, 4 get center, 5 set center. 1 done, 0 no object or no
/// Character Controller.
extern "C" fn character_vec(id: u64, op: i32, v: *mut V3) -> i32 {
    if v.is_null() {
        return 0;
    }
    // SAFETY: `v` points at a Vector3 on the C# stack.
    let value = Vec3::from_array(unsafe { *v });
    // SAFETY: as above.
    let write = |out: Vec3| unsafe { v.write(out.to_array()) };
    with_object(id, |w, e| {
        if w.scene
            .world
            .get::<&crate::scene::CharacterController>(e)
            .is_err()
        {
            return 0;
        }
        let dt = move_dt(w);
        match op {
            0 | 1 => {
                let World { physics, scene, .. } = w;
                let grounded = if op == 0 {
                    physics.move_character(scene, e, value, dt)
                } else {
                    physics.simple_move(scene, e, value, dt)
                };
                write(Vec3::new(f32::from(u8::from(grounded)), 0.0, 0.0));
            }
            2 => write(w.physics.character_velocity(e)),
            3 => write(w.physics.character_ground_normal(e)),
            4 => {
                let c = w
                    .scene
                    .world
                    .get::<&crate::scene::CharacterController>(e)
                    .map(|c| c.center);
                write(c.unwrap_or_default());
            }
            _ => {
                if let Ok(mut c) = w
                    .scene
                    .world
                    .get::<&mut crate::scene::CharacterController>(e)
                {
                    c.center = value;
                }
                let World { physics, scene, .. } = w;
                physics.set_controller_size(scene, e);
            }
        }
        1
    })
    .unwrap_or(0)
}

/// `Time.timeScale = value`.
extern "C" fn time_scale(value: f32) {
    let _ = with_world(|w| w.set_time_scale(value));
}

/// `which`: 0 isGrounded (read only), 1 height, 2 radius, 3 slopeLimit, 4 stepOffset,
/// 5 skinWidth, 6 minMoveDistance, 7 slopeSpeed; `set` 0 reads into `out`, 1 writes `value`.
/// 1 done, 0 no object or no Character Controller.
extern "C" fn character_scalar(id: u64, which: i32, set: i32, value: f32, out: *mut f32) -> i32 {
    with_object(id, |w, e| {
        let grounded = f32::from(u8::from(w.physics.character_grounded(e)));
        let Ok(mut c) = w
            .scene
            .world
            .get::<&mut crate::scene::CharacterController>(e)
        else {
            return 0;
        };
        if set != 0 {
            match which {
                1 => c.height = value.max(0.01),
                2 => c.radius = value.max(0.01),
                3 => c.slope_limit = value.clamp(0.0, 180.0),
                4 => c.step_offset = value.max(0.0),
                5 => c.skin_width = value.max(1e-4),
                6 => c.min_move_distance = value.max(0.0),
                7 => c.slope_speed = value.clamp(0.0, 1.0),
                _ => return 1,
            }
            drop(c);
            let World { physics, scene, .. } = w;
            physics.set_controller_size(scene, e);
        } else if !out.is_null() {
            let v = match which {
                1 => c.height,
                2 => c.radius,
                3 => c.slope_limit,
                4 => c.step_offset,
                5 => c.skin_width,
                6 => c.min_move_distance,
                7 => c.slope_speed,
                _ => grounded,
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

/// Floats [`shape_query`] reads.
const SHAPE_FLOATS: usize = 15;

/// Box and capsule queries, and sphere checks / sweeps of every hit. `kind`: 0 sphere, 1 box,
/// 2 capsule. `p` holds 15 floats: position (capsule: first point) xyz, half extents (capsule:
/// second point) xyz, radius, rotation xyzw, direction xyz, max distance. ops: 0 check (1/0),
/// 1 overlap (writes up to `cap` ids, returns how many), 2 the first sweep hit (`hits[0]`;
/// 1/0), 3 every sweep hit (up to `cap`, returns how many). -1 outside a call from the engine
/// or for a bad op / kind.
extern "C" fn shape_query(
    op: i32,
    kind: i32,
    p: *const f32,
    mask: u32,
    hits: *mut HitData,
    ids: *mut u64,
    cap: i32,
) -> i32 {
    use crate::physics::QueryShape;
    if p.is_null() {
        return -1;
    }
    // SAFETY: C# passes 15 floats.
    let p = unsafe { std::slice::from_raw_parts(p, SHAPE_FLOATS) };
    let v = |i: usize| Vec3::new(p[i], p[i + 1], p[i + 2]);
    let (mut center, rot) = (v(0), Quat::from_xyzw(p[7], p[8], p[9], p[10]));
    let shape = match kind {
        0 => QueryShape::Sphere { radius: p[6] },
        1 => QueryShape::Box { half_extents: v(3) },
        2 => {
            let (shape, c) = QueryShape::capsule(v(0), v(3), p[6]);
            center = c;
            shape
        }
        _ => return -1,
    };
    let (dir, max) = (v(11), p[14]);
    let cap = cap.max(0) as usize;
    with_world(|w| match op {
        0 => i32::from(w.physics.check(shape, center, rot, mask)),
        1 => {
            let found = w.physics.overlap(shape, center, rot, mask);
            if !ids.is_null() {
                for (i, e) in found.iter().take(cap).enumerate() {
                    // SAFETY: `ids` holds `cap` ids on the C# side.
                    unsafe { ids.add(i).write(id_of(*e)) };
                }
            }
            found.len() as i32
        }
        2 => match w.physics.cast(shape, center, rot, dir, max, mask) {
            Some(h) => {
                if !hits.is_null() {
                    // SAFETY: `hits` points at a HitData on the C# stack.
                    unsafe { hits.write(hit_data(w, &h)) };
                }
                1
            }
            None => 0,
        },
        3 => {
            let found = w.physics.cast_all(shape, center, rot, dir, max, mask);
            if !hits.is_null() {
                for (i, h) in found.iter().take(cap).enumerate() {
                    // SAFETY: `hits` holds `cap` HitData on the C# side.
                    unsafe { hits.add(i).write(hit_data(w, h)) };
                }
            }
            found.len() as i32
        }
        _ => -1,
    })
    .unwrap_or(-1)
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
/// Scene loads and quit, after the frame. With `track` not 0 a load / unload is an operation
/// (`LoadSceneAsync`; `track` 2 waits for `allowSceneActivation`) and its id is returned.
extern "C" fn request(op: i32, index: i64, ptr: *const u16, len: i32, track: i32) -> i64 {
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
    with_world(|w| {
        if track != 0 && op != 3 {
            w.scene_op_start(r, track == 1)
        } else {
            w.requests.push(r);
            0
        }
    })
    .unwrap_or(0)
}

/// A scene operation's state (`scene_ops.rs`: 0 waiting, 1 loading, 2 done, -1 none);
/// `allow` 0 / 1 sets `allowSceneActivation` (-1 only reads).
extern "C" fn scene_op(id: i64, allow: i32) -> i32 {
    with_world(|w| {
        if allow >= 0 {
            w.scene_op_allow(id, allow == 1);
        }
        w.scene_op_state(id)
    })
    .unwrap_or(-1)
}

// ---- Camera, Light, Renderer, Material, Screen, Cursor ------------------------------------

fn flag(on: bool) -> f32 {
    f32::from(u8::from(on))
}

/// Camera (`kind` 4), Light (5) and Mesh Renderer (6) values. `v` is read when `set` is not 0
/// and written otherwise. Returns 1, or 0 when the object has no such component (or value).
extern "C" fn component_value(id: u64, kind: i32, which: i32, set: i32, v: *mut V4) -> i32 {
    if v.is_null() {
        return 0;
    }
    with_object(id, |w, e| {
        // SAFETY: `v` points at four floats on the C# stack.
        let v = unsafe { &mut *v };
        let done = match kind {
            4 => camera_value(w, e, which, set != 0, v),
            5 => light_value(w, e, which, set != 0, v),
            6 => renderer_value(w, e, which, set != 0, v),
            10..=15 => ui_value(w, e, kind, which, set != 0, v),
            _ => None,
        };
        i32::from(done.is_some())
    })
    .unwrap_or(0)
}

/// which: 0 fieldOfView, 1 nearClipPlane, 2 farClipPlane, 3 orthographic, 4 orthographicSize,
/// 5 enabled, 6 cullingMask (the bits of the float), 7 backgroundColor, 8 depth,
/// 9 pixel width, height and aspect (read only).
fn camera_value(w: &mut World, e: Entity, which: i32, set: bool, v: &mut V4) -> Option<()> {
    let screen = super::camera_api::screen(w);
    let mut c = w.scene.world.get::<&mut crate::scene::Camera>(e).ok()?;
    let x = v[0];
    if set {
        match which {
            // NaN passes `clamp` and would poison the projection.
            0 if x.is_finite() => c.fov = x.clamp(1.0, 179.0),
            0 => {}
            1 => c.near = x.max(0.001),
            2 => c.far = x.max(0.002),
            3 => c.orthographic = x != 0.0,
            4 => c.ortho_size = x.max(0.001),
            5 => c.enabled = x != 0.0,
            6 => c.culling_mask = x.to_bits(),
            7 => c.background = *v,
            8 => c.depth = x,
            _ => return None,
        }
    } else {
        *v = match which {
            0 => [c.fov, 0.0, 0.0, 0.0],
            1 => [c.near, 0.0, 0.0, 0.0],
            2 => [c.far, 0.0, 0.0, 0.0],
            3 => [flag(c.orthographic), 0.0, 0.0, 0.0],
            4 => [c.ortho_size, 0.0, 0.0, 0.0],
            5 => [flag(c.enabled), 0.0, 0.0, 0.0],
            6 => [f32::from_bits(c.culling_mask), 0.0, 0.0, 0.0],
            7 => c.background,
            8 => [c.depth, 0.0, 0.0, 0.0],
            9 => {
                let size = screen * glam::Vec2::new(c.viewport[2], c.viewport[3]);
                [size.x, size.y, size.x / size.y.max(1e-4), 0.0]
            }
            _ => return None,
        };
    }
    Some(())
}

/// which: 0 enabled, 1 intensity, 2 range, 3 spotAngle, 4 color, 5 type (0 Spot,
/// 1 Directional, 2 Point, 3 Rectangle), 6 shadows (0 None, 1 Hard, 2 Soft).
fn light_value(w: &mut World, e: Entity, which: i32, set: bool, v: &mut V4) -> Option<()> {
    use crate::scene::{LightType, Shadows};
    let mut l = w.scene.world.get::<&mut crate::scene::Light>(e).ok()?;
    let x = v[0];
    if set {
        match which {
            0 => l.enabled = x != 0.0,
            1 => l.intensity = x.max(0.0),
            2 => l.range = x.max(0.0),
            3 => l.spot_angle = x.clamp(1.0, 179.0),
            4 => l.color = [v[0], v[1], v[2]],
            5 => {
                l.kind = match x as i32 {
                    0 => LightType::Spot,
                    1 => LightType::Directional,
                    3 => LightType::Rectangle,
                    _ => LightType::Point,
                }
            }
            6 => {
                l.shadows = match x as i32 {
                    0 => Shadows::None,
                    1 => Shadows::Hard,
                    _ => Shadows::Soft,
                }
            }
            _ => return None,
        }
    } else {
        *v = match which {
            0 => [flag(l.enabled), 0.0, 0.0, 0.0],
            1 => [l.intensity, 0.0, 0.0, 0.0],
            2 => [l.range, 0.0, 0.0, 0.0],
            3 => [l.spot_angle, 0.0, 0.0, 0.0],
            4 => [l.color[0], l.color[1], l.color[2], 1.0],
            5 => {
                let kind = match l.kind {
                    LightType::Spot => 0.0,
                    LightType::Directional => 1.0,
                    LightType::Point => 2.0,
                    LightType::Rectangle => 3.0,
                };
                [kind, 0.0, 0.0, 0.0]
            }
            6 => {
                let shadows = match l.shadows {
                    Shadows::None => 0.0,
                    Shadows::Hard => 1.0,
                    Shadows::Soft => 2.0,
                };
                [shadows, 0.0, 0.0, 0.0]
            }
            _ => return None,
        };
    }
    Some(())
}

/// which: 0 enabled, 1 casts shadows, 2 receiveShadows.
fn renderer_value(w: &mut World, e: Entity, which: i32, set: bool, v: &mut V4) -> Option<()> {
    let mut r = w
        .scene
        .world
        .get::<&mut crate::scene::MeshRenderer>(e)
        .ok()?;
    let field = match which {
        0 => &mut r.enabled,
        1 => &mut r.cast_shadows,
        2 => &mut r.receive_shadows,
        _ => return None,
    };
    if set {
        *field = v[0] != 0.0;
    } else {
        *v = [flag(*field), 0.0, 0.0, 0.0];
    }
    Some(())
}

/// Camera points. Screen points are pixels from the bottom-left, viewport points 0..1 from
/// the bottom-left; `z` is the distance in front of the camera.
/// op: 0 ScreenPointToRay (`out` origin, `out2` direction), 1 ViewportPointToRay,
/// 2 WorldToScreenPoint, 3 WorldToViewportPoint, 4 ScreenToWorldPoint, 5 ViewportToWorldPoint.
/// Returns 1, or 0 when the object has no Camera.
extern "C" fn camera_point(id: u64, op: i32, a: *const V3, out: *mut V3, out2: *mut V3) -> i32 {
    let Some(a) = read_v3(a) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    with_object(id, |w, e| {
        let Some(cam) = super::camera_api::camera_of(w, e) else {
            return 0;
        };
        // Screen points are measured in the camera's own part of the screen (split screen).
        let rect = super::camera_api::pixel_rect(w, e);
        let aspect = rect.1.x / rect.1.y;
        // Normalised camera coordinates, y down (what `Camera` works in).
        let uv = |p: Vec3, pixels: bool| {
            if pixels {
                super::camera_api::screen_to_uv(rect, p.truncate())
            } else {
                glam::Vec2::new(p.x, 1.0 - p.y)
            }
        };
        let (first, second) = match op {
            0 | 1 => cam.screen_ray(aspect, uv(a, op == 0)),
            2 | 3 => {
                let depth = (a - cam.position).dot(cam.forward());
                let clip = cam.view_projection(aspect) * a.extend(1.0);
                let ndc = if clip.w.abs() > 1e-6 {
                    clip.truncate() / clip.w
                } else {
                    Vec3::ZERO
                };
                let (x, y) = (ndc.x * 0.5 + 0.5, ndc.y * 0.5 + 0.5);
                let p = if op == 2 {
                    (rect.0 + glam::Vec2::new(x, y) * rect.1).extend(depth)
                } else {
                    Vec3::new(x, y, depth)
                };
                (p, Vec3::ZERO)
            }
            4 | 5 => {
                let (origin, dir) = cam.screen_ray(aspect, uv(a, op == 4));
                let along = dir.dot(cam.forward());
                let start = (origin - cam.position).dot(cam.forward());
                let t = if along.abs() > 1e-6 {
                    (a.z - start) / along
                } else {
                    0.0
                };
                (origin + dir * t, Vec3::ZERO)
            }
            _ => return 0,
        };
        // SAFETY: `out` (checked above) and `out2` point at Vector3s on the C# stack.
        unsafe {
            out.write(first.to_array());
            if !out2.is_null() {
                out2.write(second.to_array());
            }
        }
        1
    })
    .unwrap_or(0)
}

/// A Mesh Renderer's material: its own copy (`shared` 0, made on first use) or the asset
/// (`shared` 1), with the Standard shader's property names as in Lua.
/// op: 0 GetColor, 1 SetColor, 2 GetFloat, 3 SetFloat, 4 HasProperty, 5 IsKeywordEnabled,
/// 6 EnableKeyword, 7 DisableKeyword, 8 get mainTextureOffset, 9 set, 10 get
/// mainTextureScale, 11 set. Returns 1 (for 4 and 5: the answer), 0 when the material has no
/// such property, -1 when the object has no Mesh Renderer, -2 when Default-Material would be
/// changed.
extern "C" fn material(
    id: u64,
    shared: i32,
    op: i32,
    ptr: *const u16,
    len: i32,
    v: *mut V4,
) -> i32 {
    use super::material_api::{self as m, MaterialRef};
    let name = text(ptr, len);
    with_object(id, |w, e| {
        let Ok(path) = m::renderer(w, e, |r| r.material_at(0).map(str::to_owned)) else {
            return -1;
        };
        let target = if shared != 0 {
            MaterialRef::Shared(path)
        } else {
            MaterialRef::Instance(e, 0)
        };
        // SAFETY: `v` is null or points at four floats on the C# stack.
        let v = unsafe { v.as_mut() };
        if matches!(op, 1 | 3 | 6 | 7 | 9 | 11) {
            if target == MaterialRef::Shared(None) {
                return -2;
            }
            let value = v.map(|v| *v).unwrap_or_default();
            let done = m::write(w, &target, |mat| match op {
                1 => m::set_color(mat, &name, value),
                3 => m::set_float(mat, &name, value[0]),
                6 => m::switch_keyword(mat, &name, true),
                7 => m::switch_keyword(mat, &name, false),
                9 => mat.offset = [value[0], value[1]],
                _ => mat.tiling = [value[0], value[1]],
            });
            return if done.is_ok() { 1 } else { -1 };
        }
        let Ok(mat) = m::read(w, &target) else {
            return -1;
        };
        let got = match op {
            0 => m::get_color(&mat, &name),
            2 => m::get_float(&mat, &name).map(|f| [f, 0.0, 0.0, 0.0]),
            4 => return i32::from(m::has_property(&mat, &name)),
            5 => return i32::from(m::keyword(&mat, &name)),
            8 => Some([mat.offset[0], mat.offset[1], 0.0, 0.0]),
            10 => Some([mat.tiling[0], mat.tiling[1], 0.0, 0.0]),
            _ => None,
        };
        match (got, v) {
            (Some(value), Some(v)) => {
                *v = value;
                1
            }
            _ => 0,
        }
    })
    .unwrap_or(-1)
}

/// Things that belong to the game, not to one object.
/// which: 0 `Camera.main` (returns its object, 0 for none), 1 the game image's width and
/// height, 2 `Cursor.lockState` (0 None, 1 Locked, 2 Confined), 3 `Cursor.visible`.
extern "C" fn global(which: i32, set: i32, v: *mut V4) -> u64 {
    use super::CursorLockMode;
    with_world(|w| {
        if which == 0 {
            return super::camera_api::main_camera(w).map_or(0, id_of);
        }
        // SAFETY: `v` is null or points at four floats on the C# stack.
        let Some(v) = (unsafe { v.as_mut() }) else {
            return 0;
        };
        match (which, set != 0) {
            (1, false) => {
                let s = super::camera_api::screen(w);
                *v = [s.x, s.y, 0.0, 0.0];
            }
            (2, false) => {
                let at = CursorLockMode::ALL.iter().position(|m| *m == w.cursor.lock);
                *v = [at.unwrap_or(0) as f32, 0.0, 0.0, 0.0];
            }
            (2, true) => {
                let mode = CursorLockMode::ALL
                    .get(v[0] as usize)
                    .copied()
                    .unwrap_or(CursorLockMode::None);
                super::cursor_api::set_lock(&mut w.cursor, mode);
            }
            (3, false) => *v = [flag(w.cursor.visible), 0.0, 0.0, 0.0],
            (3, true) => w.cursor.visible = v[0] != 0.0,
            _ => return 0,
        }
        1
    })
    .unwrap_or(0)
}

// ---- UI components and PlayerPrefs -------------------------------------------------------

/// UI values. kind 10 Text (0 enabled, 1 fontSize, 2 color), 11 Image (0 enabled, 1 color,
/// 2 fillAmount, 3 raycastTarget), 12 Button (0 interactable), 13 Toggle (0 interactable,
/// 1 isOn), 14 Slider (0 interactable, 1 value, 2 minValue, 3 maxValue, 4 wholeNumbers,
/// 5 normalizedValue), 15 Input Field (0 interactable, 1 readOnly). Setting a value from a
/// script doesn't send On Value Changed (as in Lua).
fn ui_value(w: &mut World, e: Entity, kind: i32, which: i32, set: bool, v: &mut V4) -> Option<()> {
    use crate::scene::{UiButton, UiImage, UiInputField, UiSlider, UiText, UiToggle};
    use crate::ui::driven::{clamp_slider, slider_normalized, slider_value_at};
    let world = &mut w.scene.world;
    let x = v[0];
    let on = x != 0.0;
    // A bool or a number field, read into `v` or set from it.
    fn num(field: &mut f32, set: bool, x: f32, v: &mut V4) {
        if set {
            *field = x;
        } else {
            *v = [*field, 0.0, 0.0, 0.0];
        }
    }
    fn boolean(field: &mut bool, set: bool, on: bool, v: &mut V4) {
        if set {
            *field = on;
        } else {
            *v = [flag(*field), 0.0, 0.0, 0.0];
        }
    }
    fn color(field: &mut [f32; 4], set: bool, v: &mut V4) {
        if set {
            *field = *v;
        } else {
            *v = *field;
        }
    }
    match (kind, which) {
        (10, _) => {
            let mut t = world.get::<&mut UiText>(e).ok()?;
            match which {
                0 => boolean(&mut t.enabled, set, on, v),
                1 => num(&mut t.font_size, set, x.max(1.0), v),
                2 => color(&mut t.color, set, v),
                _ => return None,
            }
        }
        (11, _) => {
            let mut i = world.get::<&mut UiImage>(e).ok()?;
            match which {
                0 => boolean(&mut i.enabled, set, on, v),
                1 => color(&mut i.color, set, v),
                2 => num(&mut i.fill.amount, set, x.clamp(0.0, 1.0), v),
                3 => boolean(&mut i.raycast_target, set, on, v),
                _ => return None,
            }
        }
        (12, 0) => {
            let mut b = world.get::<&mut UiButton>(e).ok()?;
            boolean(&mut b.interactable, set, on, v);
        }
        (13, _) => {
            let mut t = world.get::<&mut UiToggle>(e).ok()?;
            match which {
                0 => boolean(&mut t.selectable.interactable, set, on, v),
                1 => boolean(&mut t.is_on, set, on, v),
                _ => return None,
            }
        }
        (14, _) => {
            let mut s = world.get::<&mut UiSlider>(e).ok()?;
            match which {
                0 => boolean(&mut s.selectable.interactable, set, on, v),
                1 if set => s.value = clamp_slider(&s, x),
                1 => *v = [s.value, 0.0, 0.0, 0.0],
                2 | 3 if set => {
                    if which == 2 {
                        s.min_value = x;
                    } else {
                        s.max_value = x;
                    }
                    s.value = clamp_slider(&s, s.value);
                }
                2 => *v = [s.min_value, 0.0, 0.0, 0.0],
                3 => *v = [s.max_value, 0.0, 0.0, 0.0],
                4 if set => {
                    s.whole_numbers = on;
                    s.value = clamp_slider(&s, s.value);
                }
                4 => *v = [flag(s.whole_numbers), 0.0, 0.0, 0.0],
                5 if set => s.value = slider_value_at(&s, x),
                5 => *v = [slider_normalized(&s), 0.0, 0.0, 0.0],
                _ => return None,
            }
        }
        (15, _) => {
            let mut f = world.get::<&mut UiInputField>(e).ok()?;
            match which {
                0 => boolean(&mut f.selectable.interactable, set, on, v),
                1 => boolean(&mut f.read_only, set, on, v),
                _ => return None,
            }
        }
        _ => return None,
    }
    Some(())
}

/// Copy `s` into `buf` (capacity `cap` UTF-16 units) and return its length; a longer string
/// is not copied, so the caller asks again with a bigger buffer.
fn write_text(s: &str, buf: *mut u16, cap: i32) -> i32 {
    let units: Vec<u16> = s.encode_utf16().collect();
    if !buf.is_null() && units.len() <= cap.max(0) as usize {
        // SAFETY: C# passes a buffer of `cap` chars.
        unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), buf, units.len()) };
    }
    units.len() as i32
}

/// The text of a Text (`kind` 10) or Input Field (15). Getting writes it to `buf` and returns
/// its length; setting reads `len` chars from `buf` and returns 0. -1 when the object has no
/// such component.
extern "C" fn component_text(id: u64, kind: i32, set: i32, buf: *mut u16, len: i32) -> i32 {
    use crate::scene::{UiInputField, UiText};
    let new = (set != 0).then(|| text(buf, len));
    with_object(id, |w, e| match kind {
        10 => {
            let Ok(mut t) = w.scene.world.get::<&mut UiText>(e) else {
                return -1;
            };
            match new {
                Some(s) => {
                    t.text = s;
                    0
                }
                None => write_text(&t.text, buf, len),
            }
        }
        15 => {
            let Ok(mut f) = w.scene.world.get::<&mut UiInputField>(e) else {
                return -1;
            };
            match new {
                Some(s) => {
                    f.text = crate::ui::input_field::sanitize(&f, &s);
                    0
                }
                None => write_text(&f.text, buf, len),
            }
        }
        _ => -1,
    })
    .unwrap_or(-1)
}

/// `PlayerPrefs`. op: 0 GetInt / 1 SetInt / 2 GetFloat / 3 SetFloat (`num` holds the default
/// or the value, and gets the result), 4 GetString (`buf` holds the default, `len` chars of
/// it, and `cap`… the same buffer; returns the length as `write_text`), 5 SetString, 6 HasKey,
/// 7 DeleteKey, 8 DeleteAll, 9 Save (1 saved, 0 failed).
extern "C" fn prefs(
    op: i32,
    key: *const u16,
    key_len: i32,
    num: *mut f64,
    buf: *mut u16,
    len: i32,
) -> i32 {
    use crate::prefs::{self, PrefValue};
    let key = text(key, key_len);
    // SAFETY: `num` is null or points at a double on the C# stack.
    let num = unsafe { num.as_mut() };
    match op {
        0 | 2 => {
            if let Some(n) = num {
                *n = if op == 0 {
                    prefs::get_int(&key, *n as i64) as f64
                } else {
                    prefs::get_float(&key, *n)
                };
            }
            1
        }
        1 => {
            prefs::set(&key, PrefValue::Int(num.map_or(0, |n| *n as i64)));
            1
        }
        3 => {
            prefs::set(&key, PrefValue::Float(num.map_or(0.0, |n| *n)));
            1
        }
        4 => {
            // `len` is the default's length on the way in; the buffer holds at least 256.
            let default = text(buf, len);
            let value = prefs::get_string(&key, &default);
            write_text(&value, buf, len.max(256))
        }
        5 => {
            prefs::set(&key, PrefValue::String(text(buf, len)));
            1
        }
        6 => i32::from(prefs::has_key(&key)),
        7 => {
            prefs::delete_key(&key);
            1
        }
        8 => {
            prefs::delete_all();
            1
        }
        _ => i32::from(prefs::save().is_ok()),
    }
}

// ---- 2D physics, navigation, particles -----------------------------------------------------

/// A Rigidbody 2D (`v` holds x, y; angles and scalars travel in `v.x`).
/// op: 0 get velocity, 1 set velocity, 2 get position, 3 AddForce (`mode` 0 Force,
/// 1 Impulse), 4 MovePosition, 5 get angularVelocity, 6 set angularVelocity, 7 get
/// gravityScale, 8 set gravityScale, 9 get mass, 10 get rotation, 11 MoveRotation,
/// 12 AddTorque (`mode` as AddForce). Returns 1, or 0 when the object has no Rigidbody 2D.
extern "C" fn rigidbody2d(id: u64, op: i32, v: *mut V3, mode: i32) -> i32 {
    use glam::Vec2;
    let Some(v) = (unsafe { v.as_mut() }) else {
        return 0;
    };
    with_object(id, |w, e| {
        if w.scene.world.get::<&crate::scene::Rigidbody2D>(e).is_err() {
            return 0;
        }
        let xy = Vec2::new(v[0], v[1]);
        let out = |v: &mut V3, p: Vec2| *v = [p.x, p.y, 0.0];
        match op {
            0 => out(v, w.physics2d.velocity(e).unwrap_or_default()),
            1 => w.physics2d.set_velocity(e, xy),
            2 => {
                let p = match w.physics2d.body_pose(e) {
                    Some((p, _)) => p,
                    None => w.scene.world_position(e).truncate(),
                };
                out(v, p);
            }
            3 => {
                let mode = if mode == 1 { "Impulse" } else { "Force" };
                let dv = super::physics2d_api::delta_velocity(w, e, xy, mode);
                let now = w.physics2d.velocity(e).unwrap_or_default();
                w.physics2d.set_velocity(e, now + dv);
            }
            4 => w.physics2d.move_body(e, Some(xy), None),
            5 => *v = [w.physics2d.angular_velocity(e).unwrap_or(0.0), 0.0, 0.0],
            6 => w.physics2d.set_angular_velocity(e, v[0]),
            7 => *v = [w.physics2d.gravity_scale(e).unwrap_or(1.0), 0.0, 0.0],
            8 => {
                if let Ok(mut b) = w.scene.world.get::<&mut crate::scene::Rigidbody2D>(e) {
                    b.gravity_scale = v[0];
                }
                w.physics2d.set_gravity_scale(e, v[0]);
            }
            9 => {
                let mass = w
                    .scene
                    .world
                    .get::<&crate::scene::Rigidbody2D>(e)
                    .map_or(1.0, |b| b.mass);
                *v = [mass, 0.0, 0.0];
            }
            10 => *v = [w.physics2d.body_pose(e).map_or(0.0, |p| p.1), 0.0, 0.0],
            11 => w.physics2d.move_body(e, None, Some(v[0])),
            12 => {
                let impulse = if mode == 1 {
                    v[0]
                } else {
                    v[0] * w.physics2d.fixed_dt
                };
                w.physics2d.apply_torque_impulse(e, impulse);
            }
            _ => return 0,
        }
        1
    })
    .unwrap_or(0)
}

/// A Nav Mesh Agent. op: 0 SetDestination (returns 2 when a path was planned, 1 when not),
/// 1 get destination, 2 remainingDistance, 3 hasPath, 4 pathPending, 5 isOnNavMesh,
/// 6 get isStopped, 7 set isStopped, 8 velocity, 9 desiredVelocity, 10 Warp (2 when placed),
/// 11 ResetPath, 12 get speed, 13 set speed, 14 get stoppingDistance, 15 set,
/// 16 get angularSpeed, 17 set, 18 get acceleration, 19 set, 20 nextPosition,
/// 21 pathStatus (0 complete, 1 partial, 2 invalid), 22 get enabled, 23 set enabled.
/// Values travel in `v` (scalars in `v.x`). Returns 0 when the object has no agent.
extern "C" fn nav_agent(id: u64, op: i32, v: *mut V3) -> i32 {
    use crate::navigation::NavMeshAgent;
    let Some(v) = (unsafe { v.as_mut() }) else {
        return 0;
    };
    let p = Vec3::from_array(*v);
    with_object(id, |w, e| {
        if w.scene.world.get::<&NavMeshAgent>(e).is_err() {
            return 0;
        }
        // Settings on the component.
        if (12..=19).contains(&op) || op == 22 || op == 23 {
            let Ok(mut a) = w.scene.world.get::<&mut NavMeshAgent>(e) else {
                return 0;
            };
            let field = match op {
                12 | 13 => &mut a.speed,
                14 | 15 => &mut a.stopping_distance,
                16 | 17 => &mut a.angular_speed,
                18 | 19 => &mut a.acceleration,
                _ => {
                    if op == 23 {
                        a.enabled = v[0] != 0.0;
                    } else {
                        *v = [flag(a.enabled), 0.0, 0.0];
                    }
                    return 1;
                }
            };
            if op % 2 == 1 {
                *field = v[0].max(0.0);
            } else {
                *v = [*field, 0.0, 0.0];
            }
            return 1;
        }
        let World {
            scene, navigation, ..
        } = w;
        match op {
            0 => {
                return if navigation.set_destination(scene, e, p) {
                    2
                } else {
                    1
                };
            }
            10 => return if navigation.warp(scene, e, p) { 2 } else { 1 },
            11 => navigation.reset_path(scene, e),
            _ => {
                let Some(st) = navigation.agent(scene, e) else {
                    *v = [0.0; 3];
                    return 1;
                };
                match op {
                    1 => *v = st.destination.to_array(),
                    2 => *v = [st.remaining_distance(), 0.0, 0.0],
                    3 => *v = [flag(st.has_path()), 0.0, 0.0],
                    4 => *v = [flag(st.path_pending), 0.0, 0.0],
                    5 => *v = [flag(st.on_mesh), 0.0, 0.0],
                    6 => *v = [flag(st.is_stopped), 0.0, 0.0],
                    7 => st.is_stopped = v[0] != 0.0,
                    8 => *v = st.velocity.to_array(),
                    9 => *v = st.desired_velocity.to_array(),
                    20 => *v = st.position.to_array(),
                    21 => {
                        let status = match st.status.name() {
                            "PathComplete" => 0.0,
                            "PathPartial" => 1.0,
                            _ => 2.0,
                        };
                        *v = [status, 0.0, 0.0];
                    }
                    _ => return 0,
                }
            }
        }
        1
    })
    .unwrap_or(0)
}

/// A Particle System (its live emitter). op: 0 Play, 1 Stop, 2 Pause, 3 Clear, 4 Emit(`v` as
/// a count), 5 isPlaying, 6 isPaused, 7 isStopped, 8 particleCount, 9 time. Values travel in
/// `v`. Returns 0 when the object has no Particle System.
extern "C" fn particles(id: u64, op: i32, v: *mut f32) -> i32 {
    use crate::particles::{self, PlayState};
    let Some(v) = (unsafe { v.as_mut() }) else {
        return 0;
    };
    with_object(id, |w, e| {
        let gravity = w.physics.gravity();
        let n = *v;
        let done = particles::with_emitter(&mut w.scene, e, gravity, |em, sys, m| match op {
            0 => em.play(sys, m, gravity),
            1 => em.stop(),
            2 => em.pause(),
            3 => em.clear(),
            4 => em.emit(sys, m, n.max(0.0) as u32),
            5 => *v = flag(em.is_playing()),
            6 => *v = flag(em.is_paused()),
            7 => *v = flag(em.state == PlayState::Stopped && em.count() == 0),
            8 => *v = em.count() as f32,
            _ => *v = em.time,
        });
        i32::from(done.is_some())
    })
    .unwrap_or(0)
}

/// `AddComponent` (op 0) and `Destroy(component)` (op 1) for engine components by name, and
/// `DontDestroyOnLoad` (op 2, no name) for the object, and adding a C# script by class name
/// (op 3, returns its slot).
/// Returns 1 when added / removed, 0 when nothing changed, -1 for an unknown name (logged).
extern "C" fn component_op(id: u64, op: i32, ptr: *const u16, len: i32) -> i32 {
    let name = text(ptr, len);
    with_object(id, |w, e| {
        if op == 2 {
            w.dont_destroy.push(e);
            return 1;
        }
        // Destroy(script): the slot, as text; removed at the end of the frame.
        if op == 4 {
            return match name.parse::<usize>() {
                Ok(slot) => {
                    w.destroy_scripts
                        .push((e, super::component_ops::ScriptSel::Slot(slot)));
                    1
                }
                Err(_) => 0,
            };
        }
        // AddComponent of a C# script by class name: its slot.
        if op == 3 {
            let root = w.root.clone();
            return match super::component_ops::find_script(&root, &format!("{name}.cs")) {
                Some(path) => super::component_ops::add_script(w, e, path) as i32,
                None => {
                    w.logs.push((
                        LogKind::Error,
                        format!("AddComponent<{name}>: no {name}.cs under Assets"),
                    ));
                    -1
                }
            };
        }
        let r = if op == 0 {
            super::component_ops::add(w, e, &name)
        } else {
            super::component_ops::remove(w, e, &name)
        };
        match r {
            Ok(done) => i32::from(done),
            Err(msg) => {
                w.logs.push((LogKind::Error, msg));
                -1
            }
        }
    })
    .unwrap_or(0)
}

/// C# `SendMessage` for the Lua scripts of `id` (queued until the C# callback returns): kind
/// -1 no value, 0 bool, 1 number, 2 whole number, 3 text, -2 a value Lua can't get (none is
/// passed). Returns 1 when the object has Lua scripts.
extern "C" fn message(
    id: u64,
    name: *const u16,
    len: i32,
    kind: i32,
    num: f64,
    txt: *const u16,
    txt_len: i32,
) -> i32 {
    use super::csharp::CsArg;
    let method = text(name, len);
    let arg = match kind {
        0 => Some(CsArg::Bool(num != 0.0)),
        1 => Some(CsArg::Number(num)),
        2 => Some(CsArg::Int(num as i64)),
        3 => Some(CsArg::Text(text(txt, txt_len))),
        _ => None,
    };
    with_object(id, |w, e| {
        i32::from(super::messages::queue_for_lua(w, e, method, arg))
    })
    .unwrap_or(0)
}

/// Physics2D queries: op 0 every ray hit from `a` along `b` up to `num` (written to `hits`),
/// 1 colliders in the circle at `a` of radius `num`, 2 in the box at `a` of size `b` turned
/// `num` degrees, 3 at the point `a` (object ids written to `ids`). Writes up to `cap` and
/// returns how many there are.
#[allow(clippy::too_many_arguments)]
extern "C" fn physics2d(
    op: i32,
    a: *const V3,
    b: *const V3,
    num: f32,
    mask: u32,
    hits: *mut HitData,
    ids: *mut u64,
    cap: i32,
) -> i32 {
    use glam::Vec2;
    let Some(a) = read_v3(a) else { return 0 };
    let b = read_v3(b).unwrap_or(Vec3::ZERO);
    let (a, b) = (Vec2::new(a.x, a.y), Vec2::new(b.x, b.y));
    let cap = cap.max(0) as usize;
    with_world(|w| {
        if op == 0 {
            let found = w.physics2d.raycast_all(a, b, num, mask);
            if !hits.is_null() {
                for (i, h) in found.iter().take(cap).enumerate() {
                    let data = HitData {
                        point: [h.point.x, h.point.y, 0.0],
                        normal: [h.normal.x, h.normal.y, 0.0],
                        distance: h.distance,
                        entity: id_of(h.entity),
                        body: h.body.map_or(0, id_of),
                    };
                    // SAFETY: `hits` holds `cap` HitData on the C# side.
                    unsafe { hits.add(i).write(data) };
                }
            }
            return found.len() as i32;
        }
        let found = match op {
            1 => w.physics2d.overlap_circle_all(a, num, mask),
            2 => w.physics2d.overlap_box_all(a, b, num, mask),
            _ => w.physics2d.overlap_point_all(a, mask),
        };
        if !ids.is_null() {
            for (i, e) in found.iter().take(cap).enumerate() {
                // SAFETY: `ids` holds `cap` ids on the C# side.
                unsafe { ids.add(i).write(id_of(*e)) };
            }
        }
        found.len() as i32
    })
    .unwrap_or(0)
}
