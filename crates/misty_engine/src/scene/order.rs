//! Hierarchy order of a few objects without walking the whole scene.

use hecs::Entity;

use super::{Node, Scene};

impl Scene {
    /// Sibling-index path from the root (compares like Hierarchy order).
    fn order_path(&self, e: Entity) -> Vec<usize> {
        let mut path = Vec::new();
        let mut cur = Some(e);
        while let Some(c) = cur {
            let parent = self.parent(c);
            let index = match parent {
                Some(p) => self
                    .world
                    .get::<&Node>(p)
                    .ok()
                    .and_then(|n| n.children.iter().position(|x| *x == c)),
                None => self.roots().iter().position(|x| *x == c),
            };
            path.push(index.unwrap_or(usize::MAX));
            cur = parent;
        }
        path.reverse();
        path
    }

    /// Sort `list` into Hierarchy order (the order [`Scene::walk`] visits). Costs a parent
    /// walk per object, so it suits the handful of lights, cameras or volumes in a scene.
    pub fn sort_hierarchy_order(&self, list: &mut [Entity]) {
        let mut keyed: Vec<(Vec<usize>, Entity)> =
            list.iter().map(|e| (self.order_path(*e), *e)).collect();
        keyed.sort();
        for (slot, (_, e)) in list.iter_mut().zip(keyed) {
            *slot = e;
        }
    }

    /// Active objects with component `T`, in Hierarchy order.
    pub fn active_with<T: hecs::Component>(&self) -> Vec<Entity> {
        let mut list: Vec<Entity> = self
            .world
            .query::<(Entity, &T)>()
            .iter()
            .map(|(e, _)| e)
            .filter(|e| self.active_in_hierarchy(*e))
            .collect();
        self.sort_hierarchy_order(&mut list);
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Light;

    #[test]
    fn matches_walk_order() {
        let mut s = Scene::new("t");
        let a = s.spawn("A", None);
        let b = s.spawn("B", None);
        let a1 = s.spawn("A1", Some(a));
        let b1 = s.spawn("B1", Some(b));
        let a2 = s.spawn("A2", Some(a));
        let a11 = s.spawn("A11", Some(a1));
        let walk: Vec<Entity> = s.walk().into_iter().map(|(e, _)| e).collect();
        let mut list = vec![b1, a2, a11, b, a1, a];
        s.sort_hierarchy_order(&mut list);
        assert_eq!(list, walk);
        for e in [b1, a11, a] {
            s.add(e, Light::default());
        }
        s.game_object_mut(b).active = false;
        assert_eq!(s.active_with::<Light>(), vec![a, a11]);
    }
}
