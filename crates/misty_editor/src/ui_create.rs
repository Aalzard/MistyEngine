//! GameObject ▸ UI ▸ …: Text, Image, Raw Image, Button, Toggle, Slider, Scrollbar, Dropdown,
//! Input Field, Canvas, Panel, Scroll View and Event System. UI elements go under the selected
//! canvas (or the scene's first canvas); a Canvas is created when there is none, and the first
//! Canvas brings an Event System along. The default hierarchies are in
//! `misty_engine::ui::presets`.

use misty_engine::hecs::Entity;
use misty_engine::scene::{Canvas, CanvasScaler, EventSystem, RectTransform, SliderDirection};
use misty_engine::ui::presets;

use crate::state::EditorState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiElement {
    Text,
    Image,
    RawImage,
    Button,
    Toggle,
    Slider,
    Scrollbar,
    Dropdown,
    InputField,
    Canvas,
    Panel,
    ScrollView,
    EventSystem,
}

impl UiElement {
    pub const ALL: [Self; 13] = [
        Self::Text,
        Self::Image,
        Self::RawImage,
        Self::Button,
        Self::Toggle,
        Self::Slider,
        Self::Scrollbar,
        Self::Dropdown,
        Self::InputField,
        Self::Canvas,
        Self::Panel,
        Self::ScrollView,
        Self::EventSystem,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Image => "Image",
            Self::RawImage => "Raw Image",
            Self::Button => "Button",
            Self::Toggle => "Toggle",
            Self::Slider => "Slider",
            Self::Scrollbar => "Scrollbar",
            Self::Dropdown => "Dropdown",
            Self::InputField => "Input Field",
            Self::Canvas => "Canvas",
            Self::Panel => "Panel",
            Self::ScrollView => "Scroll View",
            Self::EventSystem => "Event System",
        }
    }

    /// A menu separator goes after these (graphics | controls | containers | events).
    pub fn ends_group(self) -> bool {
        matches!(self, Self::RawImage | Self::InputField | Self::ScrollView)
    }
}

fn has_event_system(s: &EditorState) -> bool {
    s.scene
        .walk()
        .into_iter()
        .any(|(e, _)| s.scene.world.get::<&EventSystem>(e).is_ok())
}

fn new_canvas(s: &mut EditorState, parent: Option<Entity>) -> Entity {
    let name = s.scene.unique_name("Canvas", parent);
    let e = presets::spawn(&mut s.scene, &name, parent, RectTransform::stretched());
    s.scene.add(e, Canvas::default());
    s.scene.add(e, CanvasScaler::default());
    // Like the reference editor, the first canvas brings an Event System.
    if !has_event_system(s) {
        let name = s.scene.unique_name("EventSystem", None);
        presets::event_system(&mut s.scene, &name);
    }
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
        UiElement::EventSystem => {
            let name = s.scene.unique_name("EventSystem", None);
            presets::event_system(&mut s.scene, &name)
        }
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
    let name = s.scene.unique_name(el.name(), Some(parent));
    let scene = &mut s.scene;
    let e = match el {
        UiElement::Canvas | UiElement::EventSystem => unreachable!("created in create"),
        UiElement::Text => presets::text(scene, &name, parent),
        UiElement::Image => presets::image_object(scene, &name, parent),
        UiElement::RawImage => presets::raw_image(scene, &name, parent),
        UiElement::Button => presets::button(scene, &name, parent),
        UiElement::Toggle => presets::toggle(scene, &name, parent),
        UiElement::Slider => presets::slider(scene, &name, parent),
        UiElement::Scrollbar => {
            presets::scrollbar(scene, &name, parent, SliderDirection::LeftToRight)
        }
        UiElement::Dropdown => presets::dropdown(scene, &name, parent),
        UiElement::InputField => presets::input_field(scene, &name, parent),
        UiElement::Panel => presets::panel(scene, &name, parent),
        UiElement::ScrollView => presets::scroll_view(scene, &name, parent),
    };
    if !s.scene.children(e).is_empty() {
        s.hierarchy.expanded.insert(e);
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{Action, Create};
    use crate::project::ProjectBrowser;
    use misty_engine::scene::{UiSlider, UiText, UiToggle};

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
        assert_eq!(s.scene.game_object(button).layer, presets::UI_LAYER);
        let es = s.scene.find_by_name("EventSystem").expect("event system");
        assert_eq!(s.scene.parent(es), None);

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

        // A second canvas doesn't add a second Event System.
        create_ui(&mut s, UiElement::Canvas, None);
        let systems = s
            .scene
            .walk()
            .into_iter()
            .filter(|(e, _)| s.scene.world.get::<&EventSystem>(*e).is_ok())
            .count();
        assert_eq!(systems, 1);
    }

    #[test]
    fn widgets_get_their_default_hierarchies_and_duplicates_keep_them_wired() {
        let mut s = state();
        create_ui(&mut s, UiElement::Slider, None);
        let slider = s.selection.active().unwrap();
        let names: Vec<String> = s
            .scene
            .children(slider)
            .into_iter()
            .map(|c| s.scene.name(c))
            .collect();
        assert_eq!(names, ["Background", "Fill Area", "Handle Slide Area"]);
        let sl = (*s.scene.world.get::<&UiSlider>(slider).unwrap()).clone();
        let fill = sl.fill_rect.unwrap();
        assert_eq!(s.scene.name(fill), "Fill");
        // Duplicating points the copy at its own parts.
        let copy = s.scene.duplicate(slider);
        let c = (*s.scene.world.get::<&UiSlider>(copy).unwrap()).clone();
        assert_ne!(c.fill_rect, Some(fill));
        assert!(s.scene.is_ancestor(copy, c.fill_rect.unwrap()));
        assert!(
            s.scene
                .is_ancestor(copy, c.selectable.target_graphic.unwrap())
        );

        create_ui(&mut s, UiElement::Toggle, None);
        let t = s.selection.active().unwrap();
        let check = s.scene.world.get::<&UiToggle>(t).unwrap().graphic.unwrap();
        assert_eq!(s.scene.name(check), "Checkmark");
        for el in [
            UiElement::Dropdown,
            UiElement::InputField,
            UiElement::ScrollView,
        ] {
            create_ui(&mut s, el, None);
            let e = s.selection.active().unwrap();
            assert_eq!(s.scene.name(e), el.name());
            assert!(!s.scene.children(e).is_empty(), "{el:?}");
        }
        // Everything survives a save/load round trip.
        let json = s.scene.to_json();
        let (loaded, report) = misty_engine::scene::Scene::from_json("UI", &json).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(loaded.to_json(), json);
    }

    /// Every GameObject ▸ UI item through the real menu bar.
    #[test]
    fn gameobject_ui_menu_items_create_ui_objects() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        let mut h = Harness::builder().with_size([900.0, 700.0]).build_ui_state(
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
        for el in UiElement::ALL {
            let name = match el {
                UiElement::EventSystem => "EventSystem",
                other => other.name(),
            };
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
