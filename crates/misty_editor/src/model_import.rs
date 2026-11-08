//! Model files dropped into the Scene view or Hierarchy: one object per model node, each with a
//! Mesh Filter + Mesh Renderer using the materials extracted next to the model. Skinned or
//! morphing meshes get a Skinned Mesh Renderer bound to the node objects as bones, and the
//! file's animations (extracted as `.anim` clips) go on an Animation component on the root.
//!
//! Model files act as read-only prefabs: an instance links to the file, so it shows its
//! overrides and can revert them, and a Prefab Variant can be made of the model.

use std::path::Path;

use misty_engine::hecs::Entity;
use misty_engine::models::{self, Model};
use misty_engine::scene::{
    Animation, LodGroup, LodLevel, MeshFilter, MeshRef, MeshRenderer, Scene, Transform,
};
use misty_engine::skinning;

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
    let clips = model_clips(s, rel, &model);
    s.record("Instantiate Model");
    let root = spawn_with_clips(&mut s.scene, rel, &model, &materials, &clips, parent);
    s.scene.link_prefab(root, rel);
    s.scene.store_prefab_ids(root);
    if parent.is_none() {
        let pivot = s.scene_view.camera.pivot().round();
        s.scene.set_world_position(root, pivot);
    }
    s.hierarchy.expanded.insert(root);
    s.select(root);
    Some(root)
}

/// The model at `rel` as a read-only prefab: the objects an instance of it starts as.
pub fn model_prefab(
    assets: &mut crate::assets::AssetDatabase,
    rel: &str,
) -> Result<misty_engine::scene::prefab::Prefab, String> {
    let model = models::model(rel)?;
    let materials = assets.model_materials(rel, &model);
    let clips: Vec<String> = (0..model.animations.len())
        .filter_map(|i| models::animation_asset_path(rel, &model, i))
        .filter(|p| misty_engine::assets::absolute(p).exists())
        .collect();
    let mut scene = Scene::new("model");
    let root = spawn_with_clips(&mut scene, rel, &model, &materials, &clips, None);
    Ok(misty_engine::scene::prefab::Prefab::from_scene(&scene, root).with_ids())
}

/// Build the objects for `model`. `materials[i]` is the `.mat` for model material `i`.
/// A model with a single node becomes one object; otherwise a root named after the file holds
/// the node hierarchy.
#[cfg(test)]
pub fn spawn(
    scene: &mut Scene,
    rel: &str,
    model: &Model,
    materials: &[Option<String>],
    parent: Option<Entity>,
) -> Entity {
    spawn_with_clips(scene, rel, model, materials, &[], parent)
}

/// The model's animation clips (extracted next to it on first use), project-relative.
fn model_clips(s: &mut EditorState, rel: &str, model: &Model) -> Vec<String> {
    if let Err(e) = models::extract_animations(rel, model) {
        s.console
            .warn(format!("Could not extract animations of {rel}: {e}"));
    }
    (0..model.animations.len())
        .filter_map(|i| models::animation_asset_path(rel, model, i))
        .filter(|p| misty_engine::assets::absolute(p).exists())
        .collect()
}

/// [`spawn`], with an Animation component on the root playing `clips` (asset paths).
pub fn spawn_with_clips(
    scene: &mut Scene,
    rel: &str,
    model: &Model,
    materials: &[Option<String>],
    clips: &[String],
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
    let material_of = |index: usize| {
        model.meshes[index]
            .material
            .and_then(|m| materials.get(m).cloned().flatten())
    };
    let node_transform = |n: &models::ModelNode| {
        let mut t = Transform::from_position(n.position);
        t.set_rotation(n.rotation);
        t.scale = n.scale;
        t
    };

    // Generated LODs need a root for the LOD Group.
    let lods = !model.lod_heights.is_empty() && model.nodes.iter().any(|n| !n.lods.is_empty());
    if let [node] = model.nodes.as_slice()
        && node.meshes.len() <= 1
        && !lods
    {
        let e = scene.spawn(&stem, parent);
        scene.set_transform(e, node_transform(node));
        if let Some(&i) = node.meshes.first() {
            if skinning::is_skinned_node(model, 0) {
                let r = skinning::renderer_for(model, rel, 0, i, &[e], material_of(i));
                scene.add(e, r);
            } else {
                add_mesh(scene, e, i);
            }
        }
        add_clips(scene, e, clips);
        return e;
    }

    let root = scene.spawn(&stem, parent);
    let mut ids: Vec<Entity> = Vec::with_capacity(model.nodes.len());
    // Skinned meshes bind to bones made later: (object, node, mesh), added at the end.
    let mut skinned: Vec<(Entity, usize, usize)> = Vec::new();
    // Objects of each LOD level (generated LODs).
    let mut levels: Vec<Vec<Entity>> = vec![Vec::new(); model.lod_heights.len()];
    for (n, node) in model.nodes.iter().enumerate() {
        let p = node
            .parent
            .and_then(|i| ids.get(i).copied())
            .unwrap_or(root);
        let e = scene.spawn(&node.name, Some(p));
        scene.set_transform(e, node_transform(node));
        if !node.lods.is_empty() && levels.len() == node.lods.len() + 1 {
            // Each level is a child object ("Hull_LOD0", "Hull_LOD1", …) with its meshes.
            let sets = std::iter::once(&node.meshes).chain(&node.lods);
            for (level, meshes) in sets.enumerate() {
                let object = scene.spawn(&format!("{}_LOD{level}", node.name), Some(e));
                for (k, &i) in meshes.iter().enumerate() {
                    let target = if k == 0 {
                        object
                    } else {
                        scene.spawn(&model.meshes[i].name, Some(object))
                    };
                    add_mesh(scene, target, i);
                }
                levels[level].push(object);
            }
            ids.push(e);
            continue;
        }
        // Extra meshes of a node (multi-material glTF meshes) become children.
        let skin = skinning::is_skinned_node(model, n);
        for (k, &i) in node.meshes.iter().enumerate() {
            let target = if k == 0 {
                e
            } else {
                scene.spawn(&model.meshes[i].name, Some(e))
            };
            if skin {
                skinned.push((target, n, i));
            } else {
                add_mesh(scene, target, i);
            }
        }
        ids.push(e);
    }
    for (e, n, i) in skinned {
        let r = skinning::renderer_for(model, rel, n, i, &ids, material_of(i));
        scene.add(e, r);
    }
    if levels.iter().any(|l| !l.is_empty()) {
        let group = LodGroup {
            lods: levels
                .into_iter()
                .zip(&model.lod_heights)
                .map(|(renderers, h)| LodLevel {
                    screen_height: *h,
                    renderers,
                })
                .collect(),
            ..Default::default()
        };
        scene.add(root, group);
    }
    add_clips(scene, root, clips);
    root
}

/// Animation component playing the model's clips (the first one automatically).
fn add_clips(scene: &mut Scene, root: Entity, clips: &[String]) {
    if let Some(first) = clips.first() {
        scene.add(
            root,
            Animation {
                clip: Some(first.clone()),
                clips: clips.to_vec(),
                ..Default::default()
            },
        );
    }
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

        s.undo_step().expect("undo step");
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

    fn sample_model(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../SampleProject/Assets/Models")
            .join(name)
    }

    /// An FBX instance is built like a glTF one: bones, a Skinned Mesh Renderer bound to them,
    /// the blend shapes and an Animation playing the take.
    #[test]
    fn fbx_model_instantiates_like_a_gltf_one() {
        let rel = sample_model("Tentacle.fbx")
            .to_string_lossy()
            .replace('\\', "/");
        let model = models::model(&rel).unwrap();
        let mut scene = Scene::default();
        let clips = vec!["Assets/Models/Tentacle_Bend.anim".to_owned()];
        let e = spawn_with_clips(&mut scene, &rel, &model, &[], &clips, None);
        assert_eq!(scene.name(e), "Tentacle");
        let mesh = scene.find_by_name("TentacleMesh").expect("mesh object");
        let r = scene
            .world
            .get::<&misty_engine::scene::SkinnedMeshRenderer>(mesh)
            .expect("skinned renderer");
        assert_eq!(r.bones.len(), 4);
        assert!(r.bones.iter().all(Option::is_some));
        assert_eq!(r.blend_shape_weights.len(), 2);
        assert!(scene.world.get::<&Animation>(e).is_ok());
    }

    /// Generate LODs: each level is a child object, and a LOD Group on the root switches them.
    #[test]
    fn generated_lods_get_a_lod_group() {
        let dir = std::env::temp_dir().join(format!("misty_model_lods_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Crate.fbx");
        std::fs::copy(sample_model("Crate.fbx"), &file).unwrap();
        std::fs::write(dir.join("Crate.fbx.meta"), r#"{ "generate_lods": true }"#).unwrap();
        let rel = file.to_string_lossy().replace('\\', "/");
        let model = models::model(&rel).unwrap();
        assert_eq!(model.lod_heights.len(), 3);
        let mut scene = Scene::default();
        let e = spawn(&mut scene, &rel, &model, &[], None);
        let group = scene
            .world
            .get::<&LodGroup>(e)
            .expect("LOD Group on the root");
        assert_eq!(group.lods.len(), 3);
        for (i, level) in group.lods.iter().enumerate() {
            assert_eq!(level.renderers.len(), 1);
            let object = level.renderers[0];
            assert_eq!(scene.name(object), format!("Crate_LOD{i}"));
            assert!(mesh_of(&scene, object).is_some());
            assert!((level.screen_height - model.lod_heights[i]).abs() < 1e-6);
        }
        drop(group);
        std::fs::remove_dir_all(dir).ok();
    }
}
