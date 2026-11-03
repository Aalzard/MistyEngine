//! GameObject ▸ UI ▸ Canvas / Text / Image / Button. UI elements go under the selected canvas
//! (or the scene's first canvas); a Canvas is created when there is none.

use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Canvas, CanvasScaler, RectTransform, TextAnchor, UiButton, UiImage, UiText,
};

use crate::state::EditorState;

/// Index of the "UI" layer in the Inspector's layer list.
pub const UI_LAYER: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiElement {
    Canvas,
    Text,
    Image,
    Button,
}

impl UiElement {
    pub const ALL: [Self; 4] = [Self::Canvas, Self::Text, Self::Image, Self::Button];

    pub fn name(self) -> &'static str {
        match self {
            Self::Canvas => "Canvas",
            Self::Text => "Text",
            Self::Image => "Image",
            Self::Button => "Button",
        }
    }
}

/// A new UI object with a Rect Transform on the UI layer.
fn spawn_ui(s: &mut EditorState, name: &str, parent: Option<Entity>, rt: RectTransform) -> Entity {
    let unique = s.scene.unique_name(name, parent);
    let e = s.scene.spawn(&unique, parent);
    s.scene.game_object_mut(e).layer = UI_LAYER;
    s.scene.add(e, rt);
    e
}

fn new_canvas(s: &mut EditorState, parent: Option<Entity>) -> Entity {
    let e = spawn_ui(s, "Canvas", parent, RectTransform::stretched());
    s.scene.add(e, Canvas::default());
    s.scene.add(e, CanvasScaler::default());
    e
}

/// The canvas new elements go into: `parent`'s canvas, else the scene's first one.
fn find_canvas(s: &EditorState, parent: Option<Entity>) -> Option<Entity> {
    if let Some(p) = parent
        && misty_engine::ui::canvas_of(&s.scene, p).is_some()
    {
        return Some(p);
    }
    s.scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .find(|e| s.scene.world.get::<&Canvas>(*e).is_ok())
}

pub fn create(s: &mut EditorState, el: UiElement, parent: Option<Entity>) {
    let parent = parent.filter(|p| s.scene.contains(*p));
    s.record(&format!("Create {}", el.name()));
    let e = match el {
        UiElement::Canvas => new_canvas(s, parent),
        _ => {
            let canvas = match find_canvas(s, parent) {
                Some(c) => c,
                None => new_canvas(s, None),
            };
            s.hierarchy.expanded.insert(canvas);
            element(s, el, canvas)
        }
    };
    s.select(e);
    s.console.info(format!("Created '{}'.", s.scene.name(e)));
}

fn element(s: &mut EditorState, el: UiElement, parent: Entity) -> Entity {
    match el {
        UiElement::Canvas => unreachable!("canvases are created by new_canvas"),
        UiElement::Text => {
            let e = spawn_ui(s, "Text", Some(parent), RectTransform::sized(160.0, 30.0));
            s.scene.add(e, UiText::default());
            e
        }
        UiElement::Image => {
            let e = spawn_ui(s, "Image", Some(parent), RectTransform::default());
            s.scene.add(e, UiImage::default());
            e
        }
        UiElement::Button => {
            let e = spawn_ui(s, "Button", Some(parent), RectTransform::sized(160.0, 30.0));
            s.scene.add(e, UiImage::default());
            s.scene.add(e, UiButton::default());
            let label = spawn_ui(s, "Text", Some(e), RectTransform::stretched());
            s.scene.add(
                label,
                UiText {
                    text: "Button".into(),
                    alignment: TextAnchor::MiddleCenter,
                    ..Default::default()
                },
            );
            s.hierarchy.expanded.insert(e);
            e
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{Action, Create};
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_ui_create_test"),
        ))
    }

    fn create_ui(s: &mut EditorState, el: UiElement, parent: Option<Entity>) {
        let what = Create::Ui(el);
        crate::commands::apply(s, Action::Create { what, parent });
    }

    #[test]
    fn button_creates_a_canvas_once_and_a_text_child() {
        let mut s = state();
        create_ui(&mut s, UiElement::Button, None);
        let canvas = s.scene.find_by_name("Canvas").expect("canvas made");
        let button = s.scene.find_by_name("Button").unwrap();
        assert_eq!(s.scene.parent(button), Some(canvas));
        assert_eq!(
            s.scene.component_names(canvas),
            ["Transform", "Rect Transform", "Canvas", "Canvas Scaler"]
        );
        assert_eq!(
            s.scene.component_names(button),
            ["Transform", "Rect Transform", "Image", "Button"]
        );
        let label = s.scene.children(button)[0];
        assert_eq!(s.scene.world.get::<&UiText>(label).unwrap().text, "Button");
        assert_eq!(s.selection.active(), Some(button));
        assert_eq!(s.scene.game_object(button).layer, UI_LAYER);

        create_ui(&mut s, UiElement::Image, None);
        let canvases = s
            .scene
            .walk()
            .into_iter()
            .filter(|(e, _)| s.scene.world.get::<&Canvas>(*e).is_ok())
            .count();
        assert_eq!(canvases, 1, "the existing canvas is reused");
        let image = s.selection.active().unwrap();
        assert_eq!(s.scene.parent(image), Some(canvas));

        // Created inside the button when it is the parent.
        create_ui(&mut s, UiElement::Text, Some(button));
        let text = s.selection.active().unwrap();
        assert_eq!(s.scene.parent(text), Some(button));
        assert_eq!(s.scene.name(text), "Text (1)");

        crate::commands::apply(&mut s, Action::Undo);
        assert!(s.scene.find_by_name("Text (1)").is_none(), "one undo step");
        assert!(s.scene.find_by_name("Button").is_some());
    }

    /// GameObject ▸ UI ▸ Canvas / Text / Image / Button through the real menu bar.
    #[test]
    fn gameobject_ui_menu_items_create_ui_objects() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| crate::menu_bar::show(ui, s),
            state(),
        );
        for el in UiElement::ALL {
            h.run();
            h.get_by_label("GameObject").click();
            h.run();
            h.get_by_label("UI ⏵").hover();
            h.run();
            h.get_by_label(el.name()).click();
            h.run();
            let actions = std::mem::take(&mut h.state_mut().actions);
            assert!(
                matches!(actions.as_slice(), [Action::Create { what: Create::Ui(x), parent: None }] if *x == el),
                "{el:?}: {actions:?}"
            );
            for a in actions {
                crate::commands::apply(h.state_mut(), a);
            }
        }
        let s = h.state();
        for name in ["Canvas", "Text", "Image", "Button"] {
            assert!(s.scene.find_by_name(name).is_some(), "{name}");
        }
        let canvas = s.scene.find_by_name("Canvas").unwrap();
        let image = s.scene.find_by_name("Image").unwrap();
        assert_eq!(
            s.scene.parent(image),
            Some(canvas),
            "elements join the first canvas"
        );
    }
}
