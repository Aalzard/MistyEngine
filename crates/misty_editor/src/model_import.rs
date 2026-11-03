//! Model files dropped into the Scene view or Hierarchy: one object per model node, each with a
//! Mesh Filter + Mesh Renderer using the materials extracted next to the model.

use std::path::Path;

use misty_engine::hecs::Entity;
use misty_engine::models::{self, Model};
use misty_engine::scene::{MeshFilter, MeshRef, MeshRenderer, Scene, Transform};

use crate::state::EditorState;

/// Instantiate the model at `rel` under `parent`, or at the Scene view pivot. Undoable.
pub fn instantiate(s: &mut EditorState, rel: &str, parent: Option<Entity>) -> Option<Entity> {
    let model = match models::model(rel) {
        Ok(m) => m,
        Err(err) => {
            s.console
                .warn(format!("Could not instantiate model: {err}"));
            return None;
        }
    };
    let parent = parent.filter(|p| s.scene.contains(*p));
    let materials = s.assets.model_materials(rel, &model);
    s.record("Instantiate Model");
    let root = spawn(&mut s.scene, rel, &model, &materials, parent);
    if parent.is_none() {
        let pivot = s.scene_view.camera.pivot().round();
        s.scene.set_world_position(root, pivot);
    }
    s.hierarchy.expanded.insert(root);
    s.select(root);
    Some(root)
}

/// Build the objects for `model`. `materials[i]` is the `.mat` for model material `i`.
/// A model with a single node becomes one object; otherwise a root named after the file holds
/// the node hierarchy.
pub fn spawn(
    scene: &mut Scene,
    rel: &str,
    model: &Model,
    materials: &[Option<String>],
    parent: Option<Entity>,
) -> Entity {
    let stem = Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Model".into());
    let add_mesh = |scene: &mut Scene, e: Entity, index: usize| {
        scene.add(
            e,
            MeshFilter {
                mesh: Some(MeshRef::asset(rel, index)),
            },
        );
        let material = model.meshes[index]
            .material
            .and_then(|m| materials.get(m).cloned().flatten());
        scene.add(
            e,
            MeshRenderer {
                material,
                ..Default::default()
            },
        );
    };
    let node_transform = |n: &models::ModelNode| {
        let mut t = Transform::from_position(n.position);
        t.set_rotation(n.rotation);
        t.scale = n.scale;
        t
    };

    if let [node] = model.nodes.as_slice()
        && node.meshes.len() <= 1
    {
        let e = scene.spawn(&stem, parent);
        scene.set_transform(e, node_transform(node));
        if let Some(&i) = node.meshes.first() {
            add_mesh(scene, e, i);
        }
        return e;
    }

    let root = scene.spawn(&stem, parent);
    let mut ids: Vec<Entity> = Vec::with_capacity(model.nodes.len());
    for node in &model.nodes {
        let p = node
            .parent
            .and_then(|i| ids.get(i).copied())
            .unwrap_or(root);
        let e = scene.spawn(&node.name, Some(p));
        scene.set_transform(e, node_transform(node));
        // Extra meshes of a node (multi-material glTF meshes) become children.
        for (k, &i) in node.meshes.iter().enumerate() {
            if k == 0 {
                add_mesh(scene, e, i);
            } else {
                let child = scene.spawn(&model.meshes[i].name, Some(e));
                add_mesh(scene, child, i);
            }
        }
        ids.push(e);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;

    const TWO_OBJECTS: &str = "\
o Base
v 0 0 0
v 1 0 0
v 0 0 1
f 1 3 2
o Top
v 0 1 0
v 1 1 0
v 0 1 1
f 4 6 5
";

    fn mesh_of(scene: &Scene, e: Entity) -> Option<MeshRef> {
        scene.world.get::<&MeshFilter>(e).ok()?.mesh.clone()
    }

    #[test]
    fn model_becomes_one_object_per_node_and_undoes() {
        let root = std::env::temp_dir().join(format!("misty_model_drop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Models")).unwrap();
        let file = root.join("Assets/Models/Pair.obj");
        std::fs::write(&file, TWO_OBJECTS).unwrap();
        // Absolute path: independent of the process-wide project root other tests set.
        let rel = file.to_string_lossy().replace('\\', "/");

        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let before = s.scene.len();
        let e = instantiate(&mut s, &rel, None).expect("model loads");
        assert_eq!(s.scene.name(e), "Pair");
        assert_eq!(s.selection.active(), Some(e));
        let kids = s.scene.children(e).to_vec();
        let names: Vec<_> = kids.iter().map(|k| s.scene.name(*k)).collect();
        assert_eq!(names, ["Base", "Top"]);
        assert_eq!(mesh_of(&s.scene, kids[1]), Some(MeshRef::asset(&rel, 1)));
        assert!(s.scene.world.get::<&MeshRenderer>(kids[0]).is_ok());
        assert_eq!(mesh_of(&s.scene, e), None, "the root only groups the nodes");

        s.undo
            .undo(&mut s.scene, &mut s.selection)
            .expect("undo step");
        assert_eq!(s.scene.len(), before);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn single_node_model_is_a_single_object() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../SampleProject/Assets/Models/Ramp.obj");
        let rel = path.to_string_lossy().replace('\\', "/");
        let model = models::model(&rel).unwrap();
        let mut scene = Scene::default();
        let e = spawn(&mut scene, &rel, &model, &[], None);
        assert_eq!(scene.name(e), "Ramp");
        assert!(scene.children(e).is_empty());
        assert_eq!(mesh_of(&scene, e), Some(MeshRef::asset(&rel, 0)));
    }
}
