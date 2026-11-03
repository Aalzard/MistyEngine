//! Automatic instancing: draws that share mesh, pipeline and material bind group become one
//! instanced draw call.
//!
//! Every mesh keeps its per-draw slot (`draws[i]`: model matrix, colour, material values).
//! A batch is a run of entries in the `slots` buffer; the shaders read
//! `draws[slots[instance_index]]`, so one call can draw any set of slots.

use std::collections::HashMap;

/// What must match for two draws to share a call (resource addresses, valid for one frame).
pub type BatchKey = [usize; 3];

/// Address of a resource, for comparing keys and skipping redundant binds within a frame.
pub fn addr<T>(r: &T) -> usize {
    r as *const T as usize
}

/// One draw call: `count` instances starting at `first` in the slots buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    pub first: u32,
    pub count: u32,
    /// A member's index in `Frame::meshes` (its mesh, pipeline and textures are the batch's).
    pub draw: usize,
}

/// How a list may be regrouped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Merge {
    /// Each draw on its own (instancing off).
    None,
    /// Only neighbours with the same key merge: the order is kept (transparent queue).
    Neighbours,
    /// Equal keys anywhere merge into the batch of their first member, so batches keep the
    /// order in which their keys first appear (opaque queues front to back, shadow casters).
    Anywhere,
}

/// Reusable grouping state (keeps its allocations between frames).
#[derive(Default)]
pub struct Batcher {
    groups: HashMap<BatchKey, u32>,
    keyed: Vec<(u32, u32)>,
}

impl Batcher {
    /// Append the draws of `order` that have a key to `slots`, grouped per `merge`.
    pub fn build(
        &mut self,
        order: impl Iterator<Item = usize>,
        key: impl Fn(usize) -> Option<BatchKey>,
        merge: Merge,
        slots: &mut Vec<u32>,
    ) -> Vec<Batch> {
        self.groups.clear();
        self.keyed.clear();
        let mut last: Option<BatchKey> = None;
        let mut next_group = 0u32;
        for i in order {
            let Some(k) = key(i) else { continue };
            let group = match merge {
                Merge::None => None,
                Merge::Neighbours => (last == Some(k)).then(|| next_group - 1),
                Merge::Anywhere => self.groups.get(&k).copied(),
            }
            .unwrap_or_else(|| {
                next_group += 1;
                if merge == Merge::Anywhere {
                    self.groups.insert(k, next_group - 1);
                }
                next_group - 1
            });
            last = Some(k);
            self.keyed.push((group, i as u32));
        }
        // Stable: members keep their order inside a batch.
        if merge == Merge::Anywhere {
            self.keyed.sort_by_key(|(g, _)| *g);
        }
        let mut batches: Vec<Batch> = Vec::new();
        let mut current = u32::MAX;
        for &(group, draw) in &self.keyed {
            let slot = slots.len() as u32;
            slots.push(draw);
            match batches.last_mut() {
                Some(b) if group == current => b.count += 1,
                _ => batches.push(Batch {
                    first: slot,
                    count: 1,
                    draw: draw as usize,
                }),
            }
            current = group;
        }
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: usize) -> BatchKey {
        [k, 0, 0]
    }

    #[test]
    fn anywhere_groups_by_first_appearance() {
        // Draw i has key keys[i]; None = not drawable.
        let keys = [Some(1), Some(2), Some(1), None, Some(3), Some(2), Some(1)];
        let mut slots = vec![99]; // existing entries are kept
        let batches = Batcher::default().build(
            0..keys.len(),
            |i| keys[i].map(key),
            Merge::Anywhere,
            &mut slots,
        );
        assert_eq!(slots, vec![99, 0, 2, 6, 1, 5, 4]);
        let summary: Vec<(u32, u32, usize)> =
            batches.iter().map(|b| (b.first, b.count, b.draw)).collect();
        assert_eq!(summary, vec![(1, 3, 0), (4, 2, 1), (6, 1, 4)]);
    }

    #[test]
    fn neighbours_keep_order_and_none_splits_everything() {
        let keys = [1, 1, 2, 1, 1];
        let mut slots = Vec::new();
        let mut b = Batcher::default();
        let batches = b.build(
            [4, 3, 2, 1, 0].into_iter(),
            |i| Some(key(keys[i])),
            Merge::Neighbours,
            &mut slots,
        );
        assert_eq!(slots, vec![4, 3, 2, 1, 0]);
        assert_eq!(
            batches.iter().map(|b| b.count).collect::<Vec<_>>(),
            vec![2, 1, 2]
        );
        slots.clear();
        let single = b.build(0..5, |i| Some(key(keys[i])), Merge::None, &mut slots);
        assert_eq!(single.len(), 5);
        assert!(single.iter().all(|b| b.count == 1));
    }
}
