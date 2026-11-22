//! `SceneManager.LoadSceneAsync` / `UnloadSceneAsync` operations (Unity's `AsyncOperation`).
//!
//! A load is still a [`Request`] carried out between frames; the operation tracks it: it waits
//! (progress 0.9) while `allowSceneActivation` is false, is queued when allowed (switching
//! activation off right after the call takes it back, like in Unity), and is
//! done at the start of the next frame, when its `completed` callbacks run. A single load without
//! `DontDestroyOnLoad` objects replaces every script, so only additive loads, unloads and
//! loads with kept objects see their operation finish.

use super::{Request, World};

/// One operation; its id is its index + 1.
#[derive(Debug)]
pub struct SceneOp {
    request: Request,
    /// Queued in `World::requests` (carried out after the frame).
    sent: bool,
    done: bool,
}

/// Operation state: waiting for `allowSceneActivation` (0), sent (1), done (2), no such id (-1).
pub const WAITING: i32 = 0;
pub const LOADING: i32 = 1;
pub const DONE: i32 = 2;

impl World {
    /// Start an operation for `request` (queued now, in call order, when `allow` is true).
    pub(super) fn scene_op_start(&mut self, request: Request, allow: bool) -> i64 {
        self.scene_ops.push(SceneOp {
            request,
            sent: false,
            done: false,
        });
        let id = self.scene_ops.len() as i64;
        if allow {
            self.scene_op_allow(id, true);
        }
        id
    }

    /// `allowSceneActivation = allow`: queue the request, or take it back while it is still
    /// waiting for the end of the frame (no effect once it was carried out).
    pub(super) fn scene_op_allow(&mut self, id: i64, allow: bool) {
        let Some(i) = usize::try_from(id - 1)
            .ok()
            .filter(|i| *i < self.scene_ops.len())
        else {
            return;
        };
        let op = &mut self.scene_ops[i];
        if op.done || op.sent == allow {
            return;
        }
        if allow {
            op.sent = true;
            self.requests.push(op.request.clone());
        } else if let Some(pos) = self.requests.iter().rposition(|r| *r == op.request) {
            op.sent = false;
            self.requests.remove(pos);
        }
    }

    pub(super) fn scene_op_state(&self, id: i64) -> i32 {
        let op = usize::try_from(id - 1)
            .ok()
            .and_then(|i| self.scene_ops.get(i));
        match op {
            None => -1,
            Some(op) if op.done => DONE,
            Some(op) if op.sent => LOADING,
            Some(_) => WAITING,
        }
    }

    /// Start of a frame: operations sent before it were carried out between frames. Returns
    /// the ids that just finished.
    pub(super) fn scene_ops_tick(&mut self) -> Vec<i64> {
        let mut done = Vec::new();
        for (i, op) in self.scene_ops.iter_mut().enumerate() {
            if op.sent && !op.done {
                op.done = true;
                done.push(i as i64 + 1);
            }
        }
        done
    }
}
