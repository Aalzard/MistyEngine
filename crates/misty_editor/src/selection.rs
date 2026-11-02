//! Object selection: ordered list plus the active (last clicked) object.

use misty_engine::hecs::Entity;
use misty_engine::scene::Scene;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    list: Vec<Entity>,
    active: Option<Entity>,
}

impl Selection {
    pub fn active(&self) -> Option<Entity> {
        self.active
    }

    pub fn list(&self) -> &[Entity] {
        &self.list
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.list.contains(&e)
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.active = None;
    }

    /// Plain click: only this object.
    pub fn set(&mut self, e: Entity) {
        self.list = vec![e];
        self.active = Some(e);
    }

    pub fn set_many(&mut self, list: Vec<Entity>) {
        self.active = list.last().copied();
        self.list = list;
    }

    /// Ctrl+click: add or remove.
    pub fn toggle(&mut self, e: Entity) {
        if let Some(i) = self.list.iter().position(|x| *x == e) {
            self.list.remove(i);
            if self.active == Some(e) {
                self.active = self.list.last().copied();
            }
        } else {
            self.list.push(e);
            self.active = Some(e);
        }
    }

    /// Shift+click: everything between the active object and `e` in `order`.
    pub fn select_range(&mut self, e: Entity, order: &[Entity]) {
        let Some(anchor) = self.active.and_then(|a| order.iter().position(|x| *x == a)) else {
            self.set(e);
            return;
        };
        let Some(target) = order.iter().position(|x| *x == e) else {
            return;
        };
        let (a, b) = (anchor.min(target), anchor.max(target));
        self.list = order[a..=b].to_vec();
        self.active = Some(e);
    }

    /// Drop objects that no longer exist (after delete/undo).
    pub fn retain_existing(&mut self, scene: &Scene) {
        self.list.retain(|e| scene.contains(*e));
        if self.active.is_some_and(|a| !scene.contains(a)) {
            self.active = self.list.last().copied();
        }
    }

    /// Selected objects without any whose ancestor is also selected (for delete/duplicate/drag).
    pub fn top_level(&self, scene: &Scene) -> Vec<Entity> {
        self.list
            .iter()
            .copied()
            .filter(|e| {
                !self
                    .list
                    .iter()
                    .any(|other| other != e && scene.is_ancestor(*other, *e))
            })
            .collect()
    }
}
