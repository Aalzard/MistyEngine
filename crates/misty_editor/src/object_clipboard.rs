//! Edit ▸ Copy / Cut / Paste of GameObjects (Ctrl+C / Ctrl+X / Ctrl+V in the Hierarchy and Scene
//! view). Copies are deep (children included) and keep their world placement.
//!
//! Like the familiar editor, copied objects are also written to the system clipboard as text
//! (a header line plus JSON in the prefab format), so the OS paste event can carry them back.

use glam::Mat4;
use misty_engine::hecs::Entity;
use misty_engine::scene::Transform;
use misty_engine::scene::prefab::Prefab;
use serde::{Deserialize, Serialize};

use crate::actions::Action;
use crate::state::EditorState;

/// First line of the clipboard text holding GameObjects.
pub const HEADER: &str = "MistyEngine/GameObjects";

/// One copied top-level object with its children.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CopiedObject {
    /// The object in the `.prefab` file format.
    pub data: serde_json::Value,
    /// Prefab asset the copied root was an instance of.
    pub link: Option<String>,
    /// World matrix of the root when copied (column-major).
    pub world: [f32; 16],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ObjectClipboard {
    pub objects: Vec<CopiedObject>,
}

impl ObjectClipboard {
    pub fn to_text(&self) -> String {
        format!(
            "{HEADER}\n{}",
            serde_json::to_string(self).unwrap_or_default()
        )
    }

    /// Parse clipboard text written by [`Self::to_text`] (None for any other text).
    pub fn from_text(text: &str) -> Option<Self> {
        let json = text.strip_prefix(HEADER)?.trim_start();
        serde_json::from_str(json).ok()
    }
}

/// Apply a clipboard command. Returns the action back if it is not one.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::CopyObjects => {
            copy(s);
        }
        Action::CutObjects => {
            if copy(s) {
                s.record("Cut");
                for e in s.selection.top_level(&s.scene) {
                    s.scene.despawn(e);
                }
                s.selection.clear();
            }
        }
        Action::PasteObjects { text, as_child } => {
            let clip = match text {
                Some(t) => ObjectClipboard::from_text(&t),
                None => s.object_clipboard.clone(),
            };
            if let Some(clip) = clip.filter(|c| !c.objects.is_empty()) {
                paste(s, &clip, as_child);
            }
        }
        other => return Some(other),
    }
    None
}

/// Copy the top-level selection; false when nothing is selected.
fn copy(s: &mut EditorState) -> bool {
    let targets = s.selection.top_level(&s.scene);
    if targets.is_empty() {
        return false;
    }
    let objects = targets
        .into_iter()
        .filter_map(|e| {
            let data = serde_json::from_str(&Prefab::from_scene(&s.scene, e).to_json()).ok()?;
            Some(CopiedObject {
                data,
                link: s.scene.prefab_link(e),
                world: s.scene.world_matrix(e).to_cols_array(),
            })
        })
        .collect();
    let clip = ObjectClipboard { objects };
    s.os_clipboard_out = Some(clip.to_text());
    s.object_clipboard = Some(clip);
    true
}

/// Paste as siblings right after the active object (or as its children), keeping world placement.
fn paste(s: &mut EditorState, clip: &ObjectClipboard, as_child: bool) {
    let anchor = s.selection.active().filter(|e| s.scene.contains(*e));
    let parent = match anchor {
        Some(a) if as_child => Some(a),
        Some(a) => s.scene.parent(a),
        None => None,
    };
    let mut index = match anchor {
        Some(a) if !as_child => Some(s.scene.sibling_index(a) + 1),
        _ => None,
    };
    s.record("Paste");
    let mut pasted = Vec::new();
    for obj in &clip.objects {
        let prefab = match Prefab::from_json(&obj.data.to_string()) {
            Ok((p, _)) => p,
            Err(e) => {
                s.console.warn(format!("Paste failed: {e}"));
                continue;
            }
        };
        let e = s
            .scene
            .instantiate_prefab(&prefab, obj.link.as_deref(), parent);
        if let Some(i) = index.as_mut() {
            s.scene.set_parent(e, parent, Some(*i), false);
            *i += 1;
        }
        place(s, e, Mat4::from_cols_array(&obj.world));
        pasted.push(e);
    }
    if let Some(p) = parent {
        s.hierarchy.expanded.insert(p);
    }
    s.selection.set_many(pasted);
}

/// Give `e` the world matrix `world` under its current parent.
fn place(s: &mut EditorState, e: Entity, world: Mat4) {
    let parent = s
        .scene
        .parent(e)
        .map(|p| s.scene.world_matrix(p))
        .unwrap_or(Mat4::IDENTITY);
    let (scale, rotation, position) = (parent.inverse() * world).to_scale_rotation_translation();
    let mut t: Transform = s.scene.transform(e);
    t.position = position;
    t.set_rotation(rotation);
    t.scale = scale;
    s.scene.set_transform(e, t);
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_object_clipboard_test"),
        ))
    }

    fn run(s: &mut EditorState, a: Action) {
        assert!(apply(s, a).is_none());
    }

    #[test]
    fn copy_paste_is_deep_and_pastes_as_sibling_after_the_selection() {
        let mut s = state();
        let roots = s.scene.roots().to_vec();
        let rig = s.scene.spawn("Rig", Some(roots[0]));
        s.scene.spawn("Lens", Some(rig));
        s.scene.set_world_position(rig, Vec3::new(1.0, 2.0, 3.0));
        s.selection.set(rig);
        run(&mut s, Action::CopyObjects);
        assert!(s.os_clipboard_out.as_deref().unwrap().starts_with(HEADER));
        run(
            &mut s,
            Action::PasteObjects {
                text: None,
                as_child: false,
            },
        );

        let copy = s.selection.active().unwrap();
        assert_ne!(copy, rig);
        assert_eq!(
            s.scene.parent(copy),
            Some(roots[0]),
            "sibling of the selection"
        );
        assert_eq!(s.scene.sibling_index(copy), s.scene.sibling_index(rig) + 1);
        assert_eq!(s.scene.name(copy), "Rig (1)");
        assert_eq!(s.scene.name(s.scene.children(copy)[0]), "Lens");
        assert!((s.scene.world_position(copy) - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-4);
        crate::commands::apply(&mut s, Action::Undo);
        assert!(!s.scene.contains(copy));
    }

    #[test]
    fn cut_removes_and_paste_from_clipboard_text_restores() {
        let mut s = state();
        let cube = s.scene.spawn("Cube", None);
        s.selection.set(cube);
        run(&mut s, Action::CutObjects);
        assert!(!s.scene.contains(cube));
        let text = s.os_clipboard_out.take().unwrap();
        s.object_clipboard = None;
        run(
            &mut s,
            Action::PasteObjects {
                text: Some(text),
                as_child: false,
            },
        );
        let pasted = s.selection.active().unwrap();
        assert_eq!(s.scene.name(pasted), "Cube");
        assert_eq!(s.scene.parent(pasted), None);
        // Other clipboard text is ignored.
        let n = s.scene.len();
        run(
            &mut s,
            Action::PasteObjects {
                text: Some("hello".into()),
                as_child: false,
            },
        );
        assert_eq!(s.scene.len(), n);
    }

    #[test]
    fn paste_as_child_keeps_world_position() {
        let mut s = state();
        let (a, b) = (s.scene.spawn("A", None), s.scene.spawn("B", None));
        s.scene.set_world_position(a, Vec3::new(5.0, 0.0, 0.0));
        s.scene.set_world_position(b, Vec3::new(0.0, 1.0, 0.0));
        s.selection.set(a);
        run(&mut s, Action::CopyObjects);
        s.selection.set(b);
        run(
            &mut s,
            Action::PasteObjects {
                text: None,
                as_child: true,
            },
        );
        let copy = s.selection.active().unwrap();
        assert_eq!(s.scene.parent(copy), Some(b));
        assert!((s.scene.world_position(copy) - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-4);
    }
}
