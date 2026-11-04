//! Hierarchy Folder: an empty object that groups others in the Hierarchy, shown with a
//! folder icon and an optional pastel row colour. Editor-only organisation, no runtime effect.

use serde::{Deserialize, Serialize};

/// Marker component of a Hierarchy folder. Not listed in the Inspector's component list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HierarchyFolder {
    /// Row tint (linear 0..1 RGB as picked); None = the plain folder look.
    pub color: Option<[f32; 3]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;

    /// Saved and loaded with the scene (colour kept), and absent from older files.
    #[test]
    fn folder_round_trips_through_scene_json() {
        let mut scene = Scene::new("Folders");
        let folder = scene.spawn("Props", None);
        let _ = scene.world.insert_one(
            folder,
            HierarchyFolder {
                color: Some([0.8, 0.9, 0.7]),
            },
        );
        scene.spawn("Crate", Some(folder));
        let (loaded, report) = Scene::from_json("Folders", &scene.to_json()).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let e = loaded.find_by_name("Props").unwrap();
        let f = loaded.world.get::<&HierarchyFolder>(e).unwrap();
        assert_eq!(f.color, Some([0.8, 0.9, 0.7]));
        assert_eq!(loaded.children(e).len(), 1);
        let crate_e = loaded.find_by_name("Crate").unwrap();
        assert!(loaded.world.get::<&HierarchyFolder>(crate_e).is_err());
        // Prefabs carry it too.
        let prefab = crate::scene::prefab::Prefab::from_scene(&scene, folder);
        assert!(prefab.to_json().contains("Hierarchy Folder"));
    }
}
