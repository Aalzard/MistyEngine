//! Component registry: type-erased add/remove/clone/serialise per component, looked up by name.

use std::sync::OnceLock;

use hecs::{Entity, EntityBuilder, World};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::components::*;

/// A component type the editor and scene files know about.
pub trait SceneComponent: hecs::Component + Clone + Default + Serialize + DeserializeOwned {
    const NAME: &'static str;
    /// Add Component menu category; empty = not addable by users.
    const CATEGORY: &'static str;
    const REMOVABLE: bool = true;
}

impl SceneComponent for GameObject {
    const NAME: &'static str = "GameObject";
    const CATEGORY: &'static str = "";
    const REMOVABLE: bool = false;
}
impl SceneComponent for Node {
    const NAME: &'static str = "Node";
    const CATEGORY: &'static str = "";
    const REMOVABLE: bool = false;
}
impl SceneComponent for Transform {
    const NAME: &'static str = "Transform";
    const CATEGORY: &'static str = "";
    const REMOVABLE: bool = false;
}
impl SceneComponent for Camera {
    const NAME: &'static str = "Camera";
    const CATEGORY: &'static str = "Rendering";
}
impl SceneComponent for Light {
    const NAME: &'static str = "Light";
    const CATEGORY: &'static str = "Rendering";
}
impl SceneComponent for MeshFilter {
    const NAME: &'static str = "Mesh Filter";
    const CATEGORY: &'static str = "Mesh";
}
impl SceneComponent for MeshRenderer {
    const NAME: &'static str = "Mesh Renderer";
    const CATEGORY: &'static str = "Mesh";
}
impl SceneComponent for Rigidbody {
    const NAME: &'static str = "Rigidbody";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for BoxCollider {
    const NAME: &'static str = "Box Collider";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for SphereCollider {
    const NAME: &'static str = "Sphere Collider";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for CapsuleCollider {
    const NAME: &'static str = "Capsule Collider";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for Scripts {
    const NAME: &'static str = "Scripts";
    const CATEGORY: &'static str = "";
}
impl SceneComponent for PrefabLink {
    const NAME: &'static str = "Prefab";
    const CATEGORY: &'static str = "";
    const REMOVABLE: bool = false;
}
impl SceneComponent for MeshCollider {
    const NAME: &'static str = "Mesh Collider";
    const CATEGORY: &'static str = "Physics";
}

impl SceneComponent for AudioSource {
    const NAME: &'static str = "Audio Source";
    const CATEGORY: &'static str = "Audio";
}
impl SceneComponent for AudioListener {
    const NAME: &'static str = "Audio Listener";
    const CATEGORY: &'static str = "Audio";
}

/// Type-erased operations for one component type.
pub struct ComponentInfo {
    pub name: &'static str,
    pub category: &'static str,
    pub removable: bool,
    pub has: fn(&World, Entity) -> bool,
    pub add_default: fn(&mut World, Entity),
    pub remove: fn(&mut World, Entity),
    pub clone_into: fn(&World, Entity, &mut EntityBuilder),
    pub to_json: fn(&World, Entity) -> Option<serde_json::Value>,
    pub from_json: fn(&mut World, Entity, serde_json::Value) -> Result<(), String>,
}

impl ComponentInfo {
    pub fn of<T: SceneComponent>() -> Self {
        Self {
            name: T::NAME,
            category: T::CATEGORY,
            removable: T::REMOVABLE,
            has: |w, e| w.entity(e).is_ok_and(|r| r.has::<T>()),
            add_default: |w, e| {
                let _ = w.insert_one(e, T::default());
            },
            remove: |w, e| {
                let _ = w.remove_one::<T>(e);
            },
            clone_into: |w, e, b| {
                if let Ok(c) = w.get::<&T>(e) {
                    b.add((*c).clone());
                }
            },
            to_json: |w, e| {
                w.get::<&T>(e)
                    .ok()
                    .and_then(|c| serde_json::to_value(&*c).ok())
            },
            from_json: |w, e, v| {
                let c: T = serde_json::from_value(v).map_err(|err| err.to_string())?;
                w.insert_one(e, c).map_err(|err| err.to_string())
            },
        }
    }

    /// Shown in the Inspector / Add Component menu (not GameObject or Node).
    pub fn user_visible(&self) -> bool {
        !matches!(self.name, "GameObject" | "Node" | "Prefab")
    }
}

/// All built-in components, in registry order.
pub fn components() -> &'static [ComponentInfo] {
    static REGISTRY: OnceLock<Vec<ComponentInfo>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        vec![
            ComponentInfo::of::<GameObject>(),
            ComponentInfo::of::<Node>(),
            ComponentInfo::of::<Transform>(),
            ComponentInfo::of::<Camera>(),
            ComponentInfo::of::<Light>(),
            ComponentInfo::of::<MeshFilter>(),
            ComponentInfo::of::<MeshRenderer>(),
            ComponentInfo::of::<Rigidbody>(),
            ComponentInfo::of::<BoxCollider>(),
            ComponentInfo::of::<SphereCollider>(),
            ComponentInfo::of::<CapsuleCollider>(),
            ComponentInfo::of::<MeshCollider>(),
            ComponentInfo::of::<AudioSource>(),
            ComponentInfo::of::<AudioListener>(),
            ComponentInfo::of::<Scripts>(),
            ComponentInfo::of::<PrefabLink>(),
        ]
    })
}

pub fn find(name: &str) -> Option<&'static ComponentInfo> {
    components().iter().find(|c| c.name == name)
}
