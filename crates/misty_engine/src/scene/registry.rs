//! Component registry: type-erased add/remove/clone/serialise per component, looked up by name.

use std::sync::OnceLock;

use hecs::{Entity, EntityBuilder, World};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::components::*;
use super::lod_group::LodGroup;
use super::physics_components::*;
use super::physics2d_components::*;
use super::sprite_components::SpriteRenderer;

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
impl SceneComponent for super::SkinnedMeshRenderer {
    const NAME: &'static str = "Skinned Mesh Renderer";
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
impl SceneComponent for CharacterController {
    const NAME: &'static str = "Character Controller";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for FixedJoint {
    const NAME: &'static str = "Fixed Joint";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for HingeJoint {
    const NAME: &'static str = "Hinge Joint";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for SpringJoint {
    const NAME: &'static str = "Spring Joint";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for super::WheelCollider {
    const NAME: &'static str = "Wheel Collider";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for Scripts {
    const NAME: &'static str = "Scripts";
    const CATEGORY: &'static str = "";
}
impl SceneComponent for super::HierarchyFolder {
    const NAME: &'static str = "Hierarchy Folder";
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
impl SceneComponent for Animation {
    const NAME: &'static str = "Animation";
    const CATEGORY: &'static str = "Miscellaneous";
}

impl SceneComponent for super::Animator {
    const NAME: &'static str = "Animator";
    const CATEGORY: &'static str = "Miscellaneous";
}

impl SceneComponent for super::PlayableDirector {
    const NAME: &'static str = "Playable Director";
    const CATEGORY: &'static str = "Miscellaneous";
}

impl SceneComponent for super::PostProcessVolume {
    const NAME: &'static str = "Post-process Volume";
    const CATEGORY: &'static str = "Rendering";
}

impl SceneComponent for super::ReflectionProbe {
    const NAME: &'static str = "Reflection Probe";
    const CATEGORY: &'static str = "Rendering";
}
impl SceneComponent for super::ReflectionProbeGroup {
    const NAME: &'static str = "Reflection Probe Group";
    const CATEGORY: &'static str = "";
}

impl SceneComponent for ParticleSystem {
    const NAME: &'static str = "Particle System";
    const CATEGORY: &'static str = "Effects";
}

impl SceneComponent for Terrain {
    const NAME: &'static str = "Terrain";
    const CATEGORY: &'static str = "Miscellaneous";
}
impl SceneComponent for NavMeshAgent {
    const NAME: &'static str = "Nav Mesh Agent";
    const CATEGORY: &'static str = "Navigation";
}
impl SceneComponent for NavMeshObstacle {
    const NAME: &'static str = "Nav Mesh Obstacle";
    const CATEGORY: &'static str = "Navigation";
}
impl SceneComponent for TerrainCollider {
    const NAME: &'static str = "Terrain Collider";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for LodGroup {
    const NAME: &'static str = "LOD Group";
    const CATEGORY: &'static str = "Rendering";
}
impl SceneComponent for crate::world_gen::WorldBuilder {
    const NAME: &'static str = "World Builder";
    const CATEGORY: &'static str = "";
}
impl SceneComponent for crate::world_gen::WorldStamp {
    const NAME: &'static str = "World Stamp";
    const CATEGORY: &'static str = "";
}
impl SceneComponent for crate::world_gen::ScatterSet {
    const NAME: &'static str = "Scatter";
    const CATEGORY: &'static str = "";
}
impl SceneComponent for super::WaterSurface {
    const NAME: &'static str = "Water Surface";
    const CATEGORY: &'static str = "Rendering";
}
impl SceneComponent for super::Floater {
    const NAME: &'static str = "Floater";
    const CATEGORY: &'static str = "Physics";
}
impl SceneComponent for SpriteRenderer {
    const NAME: &'static str = "Sprite Renderer";
    const CATEGORY: &'static str = "Rendering";
}
impl SceneComponent for Rigidbody2D {
    const NAME: &'static str = "Rigidbody 2D";
    const CATEGORY: &'static str = "Physics 2D";
}
impl SceneComponent for BoxCollider2D {
    const NAME: &'static str = "Box Collider 2D";
    const CATEGORY: &'static str = "Physics 2D";
}
impl SceneComponent for CircleCollider2D {
    const NAME: &'static str = "Circle Collider 2D";
    const CATEGORY: &'static str = "Physics 2D";
}
impl SceneComponent for CapsuleCollider2D {
    const NAME: &'static str = "Capsule Collider 2D";
    const CATEGORY: &'static str = "Physics 2D";
}
impl SceneComponent for PolygonCollider2D {
    const NAME: &'static str = "Polygon Collider 2D";
    const CATEGORY: &'static str = "Physics 2D";
}
impl SceneComponent for EdgeCollider2D {
    const NAME: &'static str = "Edge Collider 2D";
    const CATEGORY: &'static str = "Physics 2D";
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
    /// Every serialised field name, including ones left out at their defaults (for presets).
    pub fields: fn() -> Vec<String>,
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
            fields: || crate::preset::fields::names(&T::default()),
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
            // UI (see ui_components.rs).
            ComponentInfo::of::<super::RectTransform>(),
            ComponentInfo::of::<super::Canvas>(),
            ComponentInfo::of::<super::CanvasScaler>(),
            ComponentInfo::of::<super::UiImage>(),
            ComponentInfo::of::<super::UiText>(),
            ComponentInfo::of::<super::UiButton>(),
            // Widgets and auto layout (ui_widgets.rs, ui_layout_components.rs).
            ComponentInfo::of::<super::UiRawImage>(),
            ComponentInfo::of::<super::UiToggle>(),
            ComponentInfo::of::<super::UiToggleGroup>(),
            ComponentInfo::of::<super::UiSlider>(),
            ComponentInfo::of::<super::UiScrollbar>(),
            ComponentInfo::of::<super::UiInputField>(),
            ComponentInfo::of::<super::UiDropdown>(),
            ComponentInfo::of::<super::UiScrollRect>(),
            ComponentInfo::of::<super::UiMask>(),
            ComponentInfo::of::<super::UiRectMask2D>(),
            ComponentInfo::of::<super::HorizontalLayoutGroup>(),
            ComponentInfo::of::<super::VerticalLayoutGroup>(),
            ComponentInfo::of::<super::GridLayoutGroup>(),
            ComponentInfo::of::<super::ContentSizeFitter>(),
            ComponentInfo::of::<super::LayoutElement>(),
            ComponentInfo::of::<super::EventSystem>(),
            ComponentInfo::of::<Camera>(),
            ComponentInfo::of::<Light>(),
            ComponentInfo::of::<crate::vcam::CameraBrain>(),
            ComponentInfo::of::<crate::vcam::VirtualCamera>(),
            ComponentInfo::of::<super::LensFlare>(),
            ComponentInfo::of::<super::PostProcessVolume>(),
            ComponentInfo::of::<super::ReflectionProbe>(),
            ComponentInfo::of::<super::ReflectionProbeGroup>(),
            ComponentInfo::of::<super::FogVolume>(),
            ComponentInfo::of::<MeshFilter>(),
            ComponentInfo::of::<MeshRenderer>(),
            ComponentInfo::of::<super::SkinnedMeshRenderer>(),
            ComponentInfo::of::<SpriteRenderer>(),
            ComponentInfo::of::<LodGroup>(),
            ComponentInfo::of::<Rigidbody>(),
            ComponentInfo::of::<BoxCollider>(),
            ComponentInfo::of::<SphereCollider>(),
            ComponentInfo::of::<CapsuleCollider>(),
            ComponentInfo::of::<MeshCollider>(),
            ComponentInfo::of::<CharacterController>(),
            ComponentInfo::of::<FixedJoint>(),
            ComponentInfo::of::<HingeJoint>(),
            ComponentInfo::of::<SpringJoint>(),
            ComponentInfo::of::<super::WheelCollider>(),
            ComponentInfo::of::<Rigidbody2D>(),
            ComponentInfo::of::<BoxCollider2D>(),
            ComponentInfo::of::<CircleCollider2D>(),
            ComponentInfo::of::<CapsuleCollider2D>(),
            ComponentInfo::of::<PolygonCollider2D>(),
            ComponentInfo::of::<EdgeCollider2D>(),
            ComponentInfo::of::<AudioSource>(),
            ComponentInfo::of::<AudioListener>(),
            ComponentInfo::of::<super::AudioReverbZone>(),
            ComponentInfo::of::<super::AudioLowPassFilter>(),
            ComponentInfo::of::<super::AudioHighPassFilter>(),
            ComponentInfo::of::<super::AudioEchoFilter>(),
            ComponentInfo::of::<Animation>(),
            ComponentInfo::of::<super::Animator>(),
            // Animation Rigging (rigging).
            ComponentInfo::of::<crate::rigging::RigBuilder>(),
            ComponentInfo::of::<crate::rigging::Rig>(),
            ComponentInfo::of::<crate::rigging::TwoBoneIk>(),
            ComponentInfo::of::<crate::rigging::MultiAim>(),
            ComponentInfo::of::<crate::rigging::DampedTransform>(),
            ComponentInfo::of::<crate::rigging::ChainIk>(),
            ComponentInfo::of::<crate::rigging::OverrideTransform>(),
            ComponentInfo::of::<super::PlayableDirector>(),
            ComponentInfo::of::<ParticleSystem>(),
            ComponentInfo::of::<Terrain>(),
            ComponentInfo::of::<TerrainCollider>(),
            // World Builder (world_gen).
            ComponentInfo::of::<crate::world_gen::WorldBuilder>(),
            ComponentInfo::of::<crate::world_gen::WorldStamp>(),
            ComponentInfo::of::<crate::world_gen::ScatterSet>(),
            ComponentInfo::of::<super::WaterSurface>(),
            ComponentInfo::of::<super::Floater>(),
            ComponentInfo::of::<NavMeshAgent>(),
            ComponentInfo::of::<NavMeshObstacle>(),
            ComponentInfo::of::<Scripts>(),
            ComponentInfo::of::<PrefabLink>(),
            ComponentInfo::of::<super::HierarchyFolder>(),
        ]
    })
}

/// Components added together with `name` (like a RequireComponent attribute).
pub fn requires(name: &str) -> &'static [&'static str] {
    match name {
        "Fixed Joint" | "Hinge Joint" | "Spring Joint" => &["Rigidbody"],
        _ => &[],
    }
}

pub fn find(name: &str) -> Option<&'static ComponentInfo> {
    components().iter().find(|c| c.name == name)
}
