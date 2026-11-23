//! Native functions C# reaches through the end of the table: the contact points of the 2D
//! collision being reported (`Collision2D.contacts`) and the Animator IK pass
//! (`SetIKPosition` … in `OnAnimatorIK`). A child of `csharp_native`.

use std::cell::Cell;

use glam::{Quat, Vec3};
use hecs::Entity;

use super::{V4, id_of, with_object};
use crate::physics2d::Contact2D;
use crate::rigging::animator_ik::{self as ik, IkGoal, IkHint};

// ---- 2D contacts ---------------------------------------------------------------------------

thread_local! {
    /// The 2D collision being reported to C# (`me`'s side), while its callback runs.
    static CONTACT2D: Cell<Option<(Entity, Contact2D)>> = const { Cell::new(None) };
}

/// Makes `c` (seen from `me`) the collision `Collision2D` reads until dropped.
pub(in crate::scripting) struct Contact2DScope {
    previous: Option<(Entity, Contact2D)>,
}

impl Contact2DScope {
    pub(in crate::scripting) fn new(me: Entity, c: &Contact2D) -> Self {
        let previous = CONTACT2D.with(|x| x.replace(Some((me, *c))));
        Self { previous }
    }
}

impl Drop for Contact2DScope {
    fn drop(&mut self) {
        CONTACT2D.with(|x| x.set(self.previous));
    }
}

/// Floats per contact point written by [`contacts`].
const POINT_FLOATS: usize = 7;

/// op 0: the 2D collision being reported. Writes the relative velocity (2 floats), then per
/// point `point.xy, normal.xy, separation, normalImpulse, tangentImpulse` while `cap` floats
/// last; `ids` gets (me, other). Normals point towards `me`. Returns the point count (-1 when no
/// collision is being reported).
pub(super) extern "C" fn contacts(op: i32, buf: *mut f32, cap: i32, ids: *mut u64) -> i32 {
    if op != 0 {
        return -1;
    }
    let Some((me, c)) = CONTACT2D.with(|x| x.get()) else {
        return -1;
    };
    let (other, sign) = if c.a == me { (c.b, 1.0) } else { (c.a, -1.0) };
    let mut out: Vec<f32> = Vec::with_capacity(2 + POINT_FLOATS * 4);
    let rv = c.relative_velocity * sign;
    out.extend([rv.x, rv.y]);
    let points = c.contact_points();
    if points.is_empty() {
        // Like Lua: one point between the objects when nothing touches (switched off).
        let n = -c.normal * sign;
        out.extend([c.point.x, c.point.y, n.x, n.y, 0.0, 0.0, 0.0]);
    }
    for p in points {
        let n = -p.normal * sign;
        out.extend([
            p.point.x,
            p.point.y,
            n.x,
            n.y,
            p.separation,
            p.normal_impulse,
            p.tangent_impulse * sign,
        ]);
    }
    if !buf.is_null() {
        let n = out.len().min(cap.max(0) as usize);
        // SAFETY: C# passes a buffer of `cap` floats.
        unsafe { std::ptr::copy_nonoverlapping(out.as_ptr(), buf, n) };
    }
    if !ids.is_null() {
        // SAFETY: C# passes room for two ids.
        unsafe {
            ids.write(id_of(me));
            ids.add(1).write(id_of(other));
        }
    }
    ((out.len() - 2) / POINT_FLOATS) as i32
}

// ---- Animator IK ---------------------------------------------------------------------------

fn read(v: *mut V4) -> [f32; 4] {
    if v.is_null() {
        return [0.0; 4];
    }
    // SAFETY: C# passes a pointer to a Vector4 / Quaternion on its stack.
    unsafe { *v }
}

fn write(v: *mut V4, x: [f32; 4]) {
    if !v.is_null() {
        // SAFETY: as in `read`.
        unsafe { *v = x };
    }
}

fn v3(a: [f32; 4]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// The IK pass of object `id`'s Animator. `index` is the AvatarIKGoal / AvatarIKHint /
/// HumanBodyBones number; `v` carries a position (xyz), rotation (xyzw) or weight (x).
/// ops: 0/1 set goal position / rotation, 2/3 set goal position / rotation weight, 4/5 get goal
/// position / rotation, 6/7 get the weights, 8 set hint position, 9 set hint weight, 10 get hint
/// position, 11 get hint weight, 12 set look-at position, 13 set look-at weight, body, head,
/// eyes, 14 set look-at clamp weight, 15 bone object (its id's bits in x and y; 0 for none).
/// Returns 1, or 0 when the object is gone or `index` is out of range.
pub(super) extern "C" fn animator_ik(id: u64, op: i32, index: i32, v: *mut V4) -> i32 {
    let i = i64::from(index);
    let a = read(v);
    with_object(id, |w, e| {
        let goal = IkGoal::from_index(i);
        let hint = IkHint::from_index(i);
        match op {
            0..=7 => {
                let Some(g) = goal else { return 0 };
                let x = w.animator.ik.goals_mut(e).goal_mut(g);
                match op {
                    0 => {
                        x.position = v3(a);
                        x.position_set = true;
                    }
                    1 => {
                        x.rotation = Quat::from_array(a).normalize();
                        x.rotation_set = true;
                    }
                    2 => x.position_weight = a[0].clamp(0.0, 1.0),
                    3 => x.rotation_weight = a[0].clamp(0.0, 1.0),
                    6 => write(v, [x.position_weight, 0.0, 0.0, 0.0]),
                    7 => write(v, [x.rotation_weight, 0.0, 0.0, 0.0]),
                    4 if x.position_set => write(v, x.position.extend(0.0).to_array()),
                    5 if x.rotation_set => write(v, x.rotation.to_array()),
                    _ => {
                        // Not set this frame: the animated pose.
                        let avatar = w.animator.ik_avatar(&w.scene, e);
                        if op == 4 {
                            let p = ik::effector_position(&w.scene, e, &avatar, g);
                            write(v, p.extend(0.0).to_array());
                        } else {
                            let q = ik::effector_rotation(&w.scene, e, &avatar, g);
                            write(v, q.to_array());
                        }
                    }
                }
                1
            }
            8..=11 => {
                let Some(h) = hint else { return 0 };
                let x = w.animator.ik.goals_mut(e).hint_mut(h);
                match op {
                    8 => {
                        x.position = v3(a);
                        x.set = true;
                    }
                    9 => x.weight = a[0].clamp(0.0, 1.0),
                    11 => write(v, [x.weight, 0.0, 0.0, 0.0]),
                    _ if x.set => write(v, x.position.extend(0.0).to_array()),
                    _ => {
                        let avatar = w.animator.ik_avatar(&w.scene, e);
                        let p = ik::hint_position(&w.scene, e, &avatar, h);
                        write(v, p.extend(0.0).to_array());
                    }
                }
                1
            }
            12..=14 => {
                let la = &mut w.animator.ik.goals_mut(e).look_at;
                match op {
                    12 => la.position = v3(a),
                    13 => {
                        la.weight = a[0].clamp(0.0, 1.0);
                        la.body_weight = a[1].clamp(0.0, 1.0);
                        la.head_weight = a[2].clamp(0.0, 1.0);
                        la.eyes_weight = a[3].clamp(0.0, 1.0);
                    }
                    _ => la.clamp_weight = a[0].clamp(0.0, 1.0),
                }
                1
            }
            15 => {
                let Some(b) = ik::human_body_bone(i) else {
                    return 0;
                };
                let avatar = w.animator.ik_avatar(&w.scene, e);
                let bits = ik::bone_entity(&w.scene, e, &avatar, b).map_or(0, id_of);
                write(
                    v,
                    [
                        f32::from_bits(bits as u32),
                        f32::from_bits((bits >> 32) as u32),
                        0.0,
                        0.0,
                    ],
                );
                1
            }
            _ => 0,
        }
    })
    .unwrap_or(0)
}
