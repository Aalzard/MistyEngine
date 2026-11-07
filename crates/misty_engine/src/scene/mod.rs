//! Scene: an ECS world of GameObjects with an ordered parent/child hierarchy.

pub mod animator_component;
pub mod audio_components;
pub mod clouds;
pub mod components;
pub mod file;
pub mod fog_volume;
pub mod hierarchy_folder;
pub mod light_area;
pub mod light_glow;
pub mod light_shadow;
pub mod lightmapping;
pub mod lod_group;
pub mod multi_scene;
pub mod occlusion_area;
mod order;
pub mod physics2d_components;
pub mod physics_components;
pub mod playable_director;
pub mod post_process;
pub mod prefab;
pub mod reflection_probe;
pub mod registry;
pub mod render_settings;
pub mod screen_space_overrides;
pub mod screen_trace_overrides;
pub mod skinned_mesh;
pub mod sprite_components;
pub mod static_flags;
pub mod ui_components;
pub mod ui_layout_components;
pub mod ui_widgets;
pub mod water;
pub mod wheel_collider;
pub mod wind_zone;

use glam::{Mat4, Vec3};
use hecs::{Entity, EntityBuilder, World};

pub use crate::mesh_builder::EditableMesh;
pub use animator_component::Animator;
pub use audio_components::*;
pub use clouds::{CloudPreset, CloudsOverride};
pub use components::*;
pub use fog_volume::{FogOverride, FogQuality, FogVolume, SunShaftsOverride, SunShaftsQuality};
pub use hierarchy_folder::HierarchyFolder;
pub use light_glow::{LensFlare, LightHalo};
pub use lightmapping::{LightMode, LightmapSettings, ReceiveGi};
pub use lod_group::{LodFadeMode, LodGroup, LodLevel};
pub use multi_scene::{SceneEntry, SceneId};
pub use occlusion_area::OcclusionArea;
pub use physics_components::*;
pub use physics2d_components::*;
pub use playable_director::{DirectorUpdateMode, DirectorWrapMode, PlayableDirector, TrackBinding};
pub use post_process::*;
pub use reflection_probe::{ReflectionProbe, ReflectionProbeGroup};
pub use registry::SceneComponent;
pub use screen_space_overrides::{AmbientOcclusionOverride, ContactShadowsOverride};
pub use screen_trace_overrides::{
    MistyGlobalIlluminationOverride, ScreenSpaceGlobalIlluminationOverride, ScreenSpaceQuality,
    ScreenSpaceReflectionsOverride,
};
pub use skinned_mesh::SkinnedMeshRenderer;
pub use sprite_components::*;
pub use static_flags::StaticFlags;
pub use ui_components::*;
pub use ui_layout_components::*;
pub use ui_widgets::*;
pub use water::{Floater, WaterKind, WaterSurface};
pub use wheel_collider::{WheelCollider, WheelFrictionCurve, WheelHit, WheelState};
pub use wind_zone::WindZone;

pub struct Scene {
    pub name: String,
    pub world: World,
    /// Lighting window ▸ Environment (skybox, ambient, reflections, fog).
    pub render_settings: render_settings::RenderSettings,
    /// Navigation window ▸ agent/bake settings and the baked nav mesh.
    pub navigation: crate::navigation::NavigationSettings,
    /// Top-level objects in Hierarchy order.
    roots: Vec<Entity>,
    /// Play mode: material assets changed by scripts through `sharedMaterial`, by path (not saved).
    pub shared_materials: std::collections::BTreeMap<String, crate::assets::Material>,
    /// Open scenes (loaded or not), in Hierarchy order; `name`, `render_settings` and
    /// `navigation` above belong to the active one (see `multi_scene`).
    scenes: Vec<SceneEntry>,
    active: SceneId,
    next_scene_id: SceneId,
    /// Scene new root objects go into (`None` = the active scene).
    spawn_into: Option<SceneId>,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl Clone for Scene {
    /// Deep copy with identical entity ids (used for undo snapshots and Play mode).
    fn clone(&self) -> Self {
        let mut world = World::new();
        for e in self.world.iter().map(|r| r.entity()) {
            let mut b = EntityBuilder::new();
            for info in registry::components() {
                (info.clone_into)(&self.world, e, &mut b);
            }
            world.spawn_at(e, b.build());
        }
        Self {
            name: self.name.clone(),
            world,
            render_settings: self.render_settings.clone(),
            navigation: self.navigation.clone(),
            roots: self.roots.clone(),
            shared_materials: self.shared_materials.clone(),
            scenes: self.scenes.clone(),
            active: self.active,
            next_scene_id: self.next_scene_id,
            spawn_into: self.spawn_into,
        }
    }
}

impl Scene {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            world: World::new(),
            render_settings: Default::default(),
            navigation: Default::default(),
            roots: Vec::new(),
            shared_materials: Default::default(),
            scenes: vec![SceneEntry::new(0, name, None)],
            active: 0,
            next_scene_id: 1,
            spawn_into: None,
        }
    }

    /// The default new scene: Main Camera and Directional Light.
    pub fn default_3d(name: &str) -> Self {
        let mut s = Self::new(name);
        let cam = s.spawn("Main Camera", None);
        s.set_transform(cam, Transform::from_position(Vec3::new(0.0, 1.0, -10.0)));
        s.game_object_mut(cam).tag = "MainCamera".into();
        s.add(cam, Camera::default());
        s.add(cam, AudioListener::default());
        let light = s.spawn("Directional Light", None);
        s.set_transform(
            light,
            Transform::from_position(Vec3::new(0.0, 3.0, 0.0))
                .with_euler(Vec3::new(50.0, -30.0, 0.0)),
        );
        s.add(light, Light::directional());
        s
    }

    // ---- objects -------------------------------------------------------------------------

    /// New object with a Transform, appended under `parent` (or the scene root).
    pub fn spawn(&mut self, name: &str, parent: Option<Entity>) -> Entity {
        let name = self.unique_name(name, parent);
        let e = self.world.spawn((
            GameObject {
                name,
                component_order: vec![Transform::NAME.to_owned()],
                ..Default::default()
            },
            Transform::default(),
            Node {
                scene: self.spawn_target(),
                ..Default::default()
            },
        ));
        self.attach(e, parent, None);
        e
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.world.contains(e)
    }

    pub fn len(&self) -> usize {
        self.world.len() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.world.is_empty()
    }

    pub fn game_object(&self, e: Entity) -> hecs::Ref<'_, GameObject> {
        self.world
            .get::<&GameObject>(e)
            .expect("scene object has GameObject")
    }

    pub fn game_object_mut(&mut self, e: Entity) -> hecs::RefMut<'_, GameObject> {
        self.world
            .get::<&mut GameObject>(e)
            .expect("scene object has GameObject")
    }

    pub fn name(&self, e: Entity) -> String {
        self.game_object(e).name.clone()
    }

    pub fn transform(&self, e: Entity) -> Transform {
        *self
            .world
            .get::<&Transform>(e)
            .expect("scene object has Transform")
    }

    pub fn set_transform(&mut self, e: Entity, t: Transform) {
        *self
            .world
            .get::<&mut Transform>(e)
            .expect("scene object has Transform") = t;
    }

    /// Add (or replace) a component and record it in the Inspector order.
    pub fn add<T: SceneComponent>(&mut self, e: Entity, component: T) {
        let _ = self.world.insert_one(e, component);
        let mut go = self.game_object_mut(e);
        if !go.component_order.iter().any(|n| n == T::NAME) {
            go.component_order.push(T::NAME.to_owned());
        }
    }

    /// Add a default component by registry name. Returns false if unknown or already present.
    pub fn add_by_name(&mut self, e: Entity, name: &str) -> bool {
        let Some(info) = registry::find(name) else {
            return false;
        };
        if (info.has)(&self.world, e) {
            return false;
        }
        for required in registry::requires(name) {
            self.add_by_name(e, required);
        }
        (info.add_default)(&mut self.world, e);
        self.game_object_mut(e)
            .component_order
            .push(name.to_owned());
        true
    }

    pub fn remove_by_name(&mut self, e: Entity, name: &str) -> bool {
        let Some(info) = registry::find(name) else {
            return false;
        };
        if !info.removable || !(info.has)(&self.world, e) {
            return false;
        }
        (info.remove)(&mut self.world, e);
        self.game_object_mut(e)
            .component_order
            .retain(|n| n != name);
        true
    }

    /// Present components in Inspector order.
    pub fn component_names(&self, e: Entity) -> Vec<String> {
        self.game_object(e)
            .component_order
            .iter()
            .filter(|n| registry::find(n).is_some_and(|i| (i.has)(&self.world, e)))
            .cloned()
            .collect()
    }

    /// Move a component one slot up (-1) or down (+1); Transform stays first.
    pub fn move_component(&mut self, e: Entity, name: &str, dir: i32) {
        let mut go = self.game_object_mut(e);
        let Some(i) = go.component_order.iter().position(|n| n == name) else {
            return;
        };
        let j = i as i32 + dir;
        if j >= 1 && (j as usize) < go.component_order.len() && i >= 1 {
            go.component_order.swap(i, j as usize);
        }
    }

    // ---- hierarchy ----------------------------------------------------------------------

    pub fn roots(&self) -> &[Entity] {
        &self.roots
    }

    pub fn parent(&self, e: Entity) -> Option<Entity> {
        self.world.get::<&Node>(e).ok().and_then(|n| n.parent)
    }

    pub fn children(&self, e: Entity) -> Vec<Entity> {
        self.world
            .get::<&Node>(e)
            .map(|n| n.children.clone())
            .unwrap_or_default()
    }

    /// Siblings list that `e` lives in (children of its parent, or the roots).
    pub fn siblings(&self, parent: Option<Entity>) -> Vec<Entity> {
        match parent {
            Some(p) => self.children(p),
            None => self.roots.clone(),
        }
    }

    pub fn sibling_index(&self, e: Entity) -> usize {
        self.siblings(self.parent(e))
            .iter()
            .position(|x| *x == e)
            .unwrap_or(0)
    }

    /// True if `ancestor` is `e` or one of its parents.
    pub fn is_ancestor(&self, ancestor: Entity, e: Entity) -> bool {
        let mut cur = Some(e);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent(c);
        }
        false
    }

    fn detach(&mut self, e: Entity) {
        match self.parent(e) {
            Some(p) => {
                if let Ok(mut n) = self.world.get::<&mut Node>(p) {
                    n.children.retain(|c| *c != e);
                }
            }
            None => self.roots.retain(|c| *c != e),
        }
        if let Ok(mut n) = self.world.get::<&mut Node>(e) {
            n.parent = None;
        }
    }

    fn attach(&mut self, e: Entity, parent: Option<Entity>, index: Option<usize>) {
        let insert = |list: &mut Vec<Entity>| {
            let i = index.unwrap_or(list.len()).min(list.len());
            list.insert(i, e);
        };
        // Children always live in their parent's scene.
        if let Some(p) = parent {
            let scene = self.scene_of(p);
            self.set_scene_of_subtree(e, scene);
        }
        match parent {
            // Borrow of the parent's Node ends before the child's is taken (hecs checks at runtime).
            Some(p) => insert(
                &mut self
                    .world
                    .get::<&mut Node>(p)
                    .expect("parent has Node")
                    .children,
            ),
            None => insert(&mut self.roots),
        }
        if let Ok(mut n) = self.world.get::<&mut Node>(e) {
            n.parent = parent;
        }
    }

    /// Re-parent `e` (None = root) at `index` among the new siblings.
    /// With `keep_world` the object stays where it is in the world, like dragging in the Hierarchy.
    /// Returns false if the move would create a cycle.
    pub fn set_parent(
        &mut self,
        e: Entity,
        parent: Option<Entity>,
        index: Option<usize>,
        keep_world: bool,
    ) -> bool {
        if parent.is_some_and(|p| self.is_ancestor(e, p)) {
            return false;
        }
        let world = self.world_matrix(e);
        // Moving within the same list: adjust the index for the removed slot.
        let mut index = index;
        if self.parent(e) == parent
            && let Some(i) = index.as_mut()
            && self.sibling_index(e) < *i
        {
            *i -= 1;
        }
        self.detach(e);
        self.attach(e, parent, index);
        if keep_world {
            let parent_world = parent
                .map(|p| self.world_matrix(p))
                .unwrap_or(Mat4::IDENTITY);
            let local = parent_world.inverse() * world;
            let (scale, rotation, position) = local.to_scale_rotation_translation();
            let mut t = self.transform(e);
            t.position = position;
            t.scale = scale;
            t.set_rotation(rotation);
            self.set_transform(e, t);
        }
        true
    }

    /// Delete `e` and all of its children.
    pub fn despawn(&mut self, e: Entity) {
        if !self.contains(e) {
            return;
        }
        for c in self.children(e) {
            self.despawn(c);
        }
        self.detach(e);
        let _ = self.world.despawn(e);
    }

    /// Deep copy of `e` placed right after it, named "Name (1)", "Name (2)"…
    pub fn duplicate(&mut self, e: Entity) -> Entity {
        let parent = self.parent(e);
        let copy = self.copy_subtree(e, parent);
        let name = self.unique_name(&self.name(e), parent);
        self.game_object_mut(copy).name = name;
        let index = self.sibling_index(e) + 1;
        self.detach(copy);
        self.attach(copy, parent, Some(index));
        // A copy is a different object for save games.
        crate::save::fresh_ids_in(self, copy);
        copy
    }

    /// Deep copy of `e` appended under `parent` (keeps the name). References between objects
    /// of the copied subtree (a Slider's Fill, …) point at the copies.
    pub fn copy_subtree(&mut self, e: Entity, parent: Option<Entity>) -> Entity {
        let mut map = std::collections::HashMap::new();
        let copy = self.copy_nodes(e, parent, &mut map);
        for new in map.values() {
            file::remap_copied_refs(&mut self.world, *new, &map);
        }
        copy
    }

    fn copy_nodes(
        &mut self,
        e: Entity,
        parent: Option<Entity>,
        map: &mut std::collections::HashMap<Entity, Entity>,
    ) -> Entity {
        let mut b = EntityBuilder::new();
        for info in registry::components() {
            (info.clone_into)(&self.world, e, &mut b);
        }
        let copy = self.world.spawn(b.build());
        // Fresh links (same scene as the original); children are re-attached below.
        let scene = self.scene_of(e);
        *self.world.get::<&mut Node>(copy).expect("copied Node") = Node {
            scene,
            ..Default::default()
        };
        self.attach(copy, parent, None);
        map.insert(e, copy);
        for c in self.children(e) {
            self.copy_nodes(c, Some(copy), map);
        }
        copy
    }

    /// Depth-first Hierarchy order with depth.
    pub fn walk(&self) -> Vec<(Entity, usize)> {
        let mut out = Vec::with_capacity(self.len());
        fn visit(s: &Scene, e: Entity, depth: usize, out: &mut Vec<(Entity, usize)>) {
            out.push((e, depth));
            for c in s.children(e) {
                visit(s, c, depth + 1, out);
            }
        }
        for r in &self.roots {
            visit(self, *r, 0, &mut out);
        }
        out
    }

    /// Active itself and all parents active.
    pub fn active_in_hierarchy(&self, e: Entity) -> bool {
        let mut cur = Some(e);
        while let Some(c) = cur {
            if !self.game_object(c).active {
                return false;
            }
            cur = self.parent(c);
        }
        true
    }

    pub fn world_matrix(&self, e: Entity) -> Mat4 {
        let local = self.transform(e).local_matrix();
        match self.parent(e) {
            Some(p) => self.world_matrix(p) * local,
            None => local,
        }
    }

    pub fn world_position(&self, e: Entity) -> Vec3 {
        self.world_matrix(e).w_axis.truncate()
    }

    /// Place `e` at a world-space position (keeps its local rotation/scale).
    pub fn set_world_position(&mut self, e: Entity, p: Vec3) {
        let parent_world = self
            .parent(e)
            .map(|p| self.world_matrix(p))
            .unwrap_or(Mat4::IDENTITY);
        let mut t = self.transform(e);
        t.position = parent_world.inverse().transform_point3(p);
        self.set_transform(e, t);
    }

    pub fn world_rotation(&self, e: Entity) -> glam::Quat {
        self.world_matrix(e).to_scale_rotation_translation().1
    }

    /// Set the world-space rotation (keeps position and scale).
    pub fn set_world_rotation(&mut self, e: Entity, q: glam::Quat) {
        let parent = self
            .parent(e)
            .map(|p| self.world_rotation(p))
            .unwrap_or_default();
        let mut t = self.transform(e);
        t.set_rotation(parent.inverse() * q);
        self.set_transform(e, t);
    }

    /// "Cube", then "Cube (1)", "Cube (2)"… unique among the siblings under `parent`.
    pub fn unique_name(&self, base: &str, parent: Option<Entity>) -> String {
        let base = strip_number_suffix(base);
        let taken: Vec<String> = self
            .siblings(parent)
            .iter()
            .map(|s| self.name(*s))
            .collect();
        if !taken.iter().any(|n| n == base) {
            return base.to_owned();
        }
        (1..)
            .map(|n| format!("{base} ({n})"))
            .find(|name| !taken.contains(name))
            .unwrap_or_default()
    }

    pub fn find_by_name(&self, name: &str) -> Option<Entity> {
        self.walk()
            .into_iter()
            .map(|(e, _)| e)
            .find(|e| self.game_object(*e).name == name)
    }
}

fn strip_number_suffix(name: &str) -> &str {
    if let Some(i) = name.rfind(" (")
        && name.ends_with(')')
        && name[i + 2..name.len() - 1]
            .chars()
            .all(|c| c.is_ascii_digit())
    {
        return &name[..i];
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &Scene, list: &[Entity]) -> Vec<String> {
        list.iter().map(|e| s.name(*e)).collect()
    }

    #[test]
    fn default_scene_has_camera_and_light() {
        let s = Scene::default_3d("SampleScene");
        assert_eq!(names(&s, s.roots()), ["Main Camera", "Directional Light"]);
        let cam = s.roots()[0];
        assert_eq!(
            s.component_names(cam),
            ["Transform", "Camera", "Audio Listener"]
        );
    }

    #[test]
    fn unique_names_follow_numbering() {
        let mut s = Scene::new("t");
        let a = s.spawn("Cube", None);
        s.spawn("Cube", None);
        let d = s.duplicate(a);
        assert_eq!(names(&s, s.roots()), ["Cube", "Cube (2)", "Cube (1)"]);
        assert_eq!(
            s.sibling_index(d),
            1,
            "duplicate goes right after the original"
        );
    }

    #[test]
    fn reparent_keeps_world_position_and_rejects_cycles() {
        let mut s = Scene::new("t");
        let parent = s.spawn("Parent", None);
        s.set_transform(
            parent,
            Transform {
                position: Vec3::new(5.0, 0.0, 0.0),
                scale: Vec3::splat(2.0),
                ..Default::default()
            }
            .with_euler(Vec3::new(0.0, 90.0, 0.0)),
        );
        let child = s.spawn("Child", None);
        s.set_transform(child, Transform::from_position(Vec3::new(1.0, 2.0, 3.0)));

        assert!(s.set_parent(child, Some(parent), None, true));
        assert_eq!(s.parent(child), Some(parent));
        assert!((s.world_position(child) - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-4);
        assert!(
            !s.set_parent(parent, Some(child), None, true),
            "cycle must be rejected"
        );
        assert_eq!(names(&s, s.roots()), ["Parent"]);
    }

    #[test]
    fn reorder_within_same_parent() {
        let mut s = Scene::new("t");
        let a = s.spawn("A", None);
        s.spawn("B", None);
        s.spawn("C", None);
        s.set_parent(a, None, Some(3), false);
        assert_eq!(names(&s, s.roots()), ["B", "C", "A"]);
        s.set_parent(a, None, Some(0), false);
        assert_eq!(names(&s, s.roots()), ["A", "B", "C"]);
    }

    #[test]
    fn duplicate_and_despawn_are_deep() {
        let mut s = Scene::new("t");
        let a = s.spawn("A", None);
        let b = s.spawn("B", Some(a));
        s.spawn("C", Some(b));
        let copy = s.duplicate(a);
        assert_eq!(s.len(), 6);
        assert_eq!(names(&s, &s.children(copy)), ["B"]);
        assert_ne!(s.children(copy)[0], b, "children are new objects");
        s.despawn(a);
        assert_eq!(s.len(), 3);
        assert_eq!(names(&s, s.roots()), ["A (1)"]);
    }

    #[test]
    fn clone_is_independent_snapshot() {
        let mut s = Scene::default_3d("t");
        let snap = s.clone();
        let cam = s.roots()[0];
        s.game_object_mut(cam).name = "Changed".into();
        s.spawn("New", None);
        assert_eq!(snap.name(cam), "Main Camera");
        assert_eq!(snap.len(), 2);
        assert_eq!(
            snap.component_names(cam),
            ["Transform", "Camera", "Audio Listener"]
        );
    }

    #[test]
    fn inactive_parent_hides_children() {
        let mut s = Scene::new("t");
        let a = s.spawn("A", None);
        let b = s.spawn("B", Some(a));
        assert!(s.active_in_hierarchy(b));
        s.game_object_mut(a).active = false;
        assert!(!s.active_in_hierarchy(b));
    }

    #[test]
    fn components_add_remove_and_order() {
        let mut s = Scene::new("t");
        let e = s.spawn("E", None);
        assert!(s.add_by_name(e, "Light"));
        assert!(s.add_by_name(e, "Camera"));
        assert!(!s.add_by_name(e, "Camera"), "no duplicates");
        s.move_component(e, "Camera", -1);
        assert_eq!(s.component_names(e), ["Transform", "Camera", "Light"]);
        s.move_component(e, "Camera", -1);
        assert_eq!(
            s.component_names(e)[0],
            "Transform",
            "Transform stays first"
        );
        assert!(!s.remove_by_name(e, "Transform"));
        assert!(s.remove_by_name(e, "Light"));
        assert_eq!(s.component_names(e), ["Transform", "Camera"]);
    }
}
