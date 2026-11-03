//! Default object hierarchies of the UI widgets (what GameObject ▸ UI ▸ … creates), with the
//! references between their parts wired up. Shared by the editor menu, samples and tests.

use glam::Vec2;
use hecs::Entity;

use crate::scene::{
    EventSystem, InputContentType, MovementType, RectTransform, Scene, Selectable, SliderDirection,
    TextAnchor, UiButton, UiDropdown, UiImage, UiInputField, UiMask, UiRawImage, UiScrollRect,
    UiScrollbar, UiSlider, UiText, UiToggle,
};

/// Layer index of "UI".
pub const UI_LAYER: u8 = 4;

fn grey(v: f32) -> [f32; 4] {
    [v / 255.0, v / 255.0, v / 255.0, 1.0]
}

/// Rect from anchors and corner offsets (offset min = lower-left, offset max = upper-right).
pub fn rect(
    anchor_min: Vec2,
    anchor_max: Vec2,
    offset_min: Vec2,
    offset_max: Vec2,
) -> RectTransform {
    let mut rt = RectTransform {
        anchor_min,
        anchor_max,
        ..Default::default()
    };
    rt.set_offsets(offset_min, offset_max);
    rt
}

/// Rect filling its parent, inset by (left, bottom, right, top).
fn inset(l: f32, b: f32, r: f32, t: f32) -> RectTransform {
    rect(Vec2::ZERO, Vec2::ONE, Vec2::new(l, b), Vec2::new(-r, -t))
}

/// A UI object on the UI layer with a Rect Transform.
pub fn spawn(scene: &mut Scene, name: &str, parent: Option<Entity>, rt: RectTransform) -> Entity {
    let e = scene.spawn(name, parent);
    scene.game_object_mut(e).layer = UI_LAYER;
    scene.add(e, rt);
    e
}

fn image(
    scene: &mut Scene,
    name: &str,
    parent: Entity,
    rt: RectTransform,
    color: [f32; 4],
) -> Entity {
    let e = spawn(scene, name, Some(parent), rt);
    scene.add(
        e,
        UiImage {
            color,
            ..Default::default()
        },
    );
    e
}

fn label(
    scene: &mut Scene,
    name: &str,
    parent: Entity,
    rt: RectTransform,
    text: &str,
    align: TextAnchor,
) -> Entity {
    let e = spawn(scene, name, Some(parent), rt);
    scene.add(
        e,
        UiText {
            text: text.into(),
            alignment: align,
            wrap: false,
            ..Default::default()
        },
    );
    e
}

fn targeting(e: Entity) -> Selectable {
    Selectable {
        target_graphic: Some(e),
        ..Default::default()
    }
}

pub fn text(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(scene, name, Some(parent), RectTransform::sized(160.0, 30.0));
    scene.add(e, UiText::default());
    e
}

pub fn image_object(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    image(scene, name, parent, RectTransform::default(), [1.0; 4])
}

pub fn raw_image(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(scene, name, Some(parent), RectTransform::default());
    scene.add(e, UiRawImage::default());
    e
}

/// Full-screen translucent Image.
pub fn panel(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    image(
        scene,
        name,
        parent,
        inset(0.0, 0.0, 0.0, 0.0),
        [1.0, 1.0, 1.0, 100.0 / 255.0],
    )
}

pub fn button(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(scene, name, Some(parent), RectTransform::sized(160.0, 30.0));
    scene.add(e, UiImage::default());
    scene.add(e, UiButton::default());
    let t = label(
        scene,
        "Text",
        e,
        RectTransform::stretched(),
        "Button",
        TextAnchor::MiddleCenter,
    );
    scene.world.get::<&mut UiText>(t).unwrap().wrap = true;
    e
}

/// Toggle: Background box (the target graphic) with a Checkmark, and a Label.
pub fn toggle(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(scene, name, Some(parent), RectTransform::sized(160.0, 20.0));
    let corner = Vec2::new(0.0, 1.0);
    let bg_rt = RectTransform {
        anchor_min: corner,
        anchor_max: corner,
        anchored_position: Vec2::new(10.0, -10.0),
        size_delta: Vec2::splat(20.0),
        ..Default::default()
    };
    let bg = image(scene, "Background", e, bg_rt, [1.0; 4]);
    let check = image(
        scene,
        "Checkmark",
        bg,
        RectTransform::sized(12.0, 12.0),
        grey(50.0),
    );
    label(
        scene,
        "Label",
        e,
        inset(23.0, 1.0, 5.0, 3.0),
        "Toggle",
        TextAnchor::MiddleLeft,
    );
    scene.add(
        e,
        UiToggle {
            selectable: targeting(bg),
            graphic: Some(check),
            ..Default::default()
        },
    );
    e
}

/// Slider: Background, Fill Area ▸ Fill and Handle Slide Area ▸ Handle.
pub fn slider(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(scene, name, Some(parent), RectTransform::sized(160.0, 20.0));
    let band = |l: f32, r: f32| {
        rect(
            Vec2::new(0.0, 0.25),
            Vec2::new(1.0, 0.75),
            Vec2::new(l, 0.0),
            Vec2::new(-r, 0.0),
        )
    };
    image(scene, "Background", e, band(0.0, 0.0), grey(205.0));
    let area = spawn(scene, "Fill Area", Some(e), band(5.0, 15.0));
    let fill_rt = rect(
        Vec2::ZERO,
        Vec2::new(0.0, 1.0),
        Vec2::new(-5.0, 0.0),
        Vec2::new(5.0, 0.0),
    );
    let fill = image(scene, "Fill", area, fill_rt, [0.42, 0.6, 0.9, 1.0]);
    let slide = spawn(
        scene,
        "Handle Slide Area",
        Some(e),
        inset(10.0, 0.0, 10.0, 0.0),
    );
    let handle_rt = rect(
        Vec2::ZERO,
        Vec2::new(0.0, 1.0),
        Vec2::new(-10.0, 0.0),
        Vec2::new(10.0, 0.0),
    );
    let handle = image(scene, "Handle", slide, handle_rt, [1.0; 4]);
    scene.add(
        e,
        UiSlider {
            selectable: targeting(handle),
            fill_rect: Some(fill),
            handle_rect: Some(handle),
            ..Default::default()
        },
    );
    e
}

/// Scrollbar: background Image with Sliding Area ▸ Handle.
pub fn scrollbar(scene: &mut Scene, name: &str, parent: Entity, dir: SliderDirection) -> Entity {
    let size = if dir.axis() == 0 {
        Vec2::new(160.0, 20.0)
    } else {
        Vec2::new(20.0, 160.0)
    };
    let e = spawn(
        scene,
        name,
        Some(parent),
        RectTransform::sized(size.x, size.y),
    );
    scene.add(
        e,
        UiImage {
            color: grey(205.0),
            ..Default::default()
        },
    );
    let area = spawn(
        scene,
        "Sliding Area",
        Some(e),
        inset(10.0, 10.0, 10.0, 10.0),
    );
    let handle_rt = inset(-10.0, -10.0, -10.0, -10.0);
    let handle = image(scene, "Handle", area, handle_rt, [1.0; 4]);
    scene.add(
        e,
        UiScrollbar {
            selectable: targeting(handle),
            handle_rect: Some(handle),
            direction: dir,
            ..Default::default()
        },
    );
    e
}

/// Input Field: Image with Placeholder and Text children.
pub fn input_field(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(scene, name, Some(parent), RectTransform::sized(160.0, 30.0));
    scene.add(e, UiImage::default());
    let area = inset(10.0, 6.0, 10.0, 7.0);
    let ph = label(
        scene,
        "Placeholder",
        e,
        area,
        "Enter text...",
        TextAnchor::MiddleLeft,
    );
    scene.world.get::<&mut UiText>(ph).unwrap().color =
        [50.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0, 0.5];
    let t = label(scene, "Text", e, area, "", TextAnchor::MiddleLeft);
    scene.add(
        e,
        UiInputField {
            selectable: targeting(e),
            text_component: Some(t),
            placeholder: Some(ph),
            content_type: InputContentType::Standard,
            ..Default::default()
        },
    );
    e
}

/// Scroll View: Viewport (Mask) ▸ Content, plus horizontal and vertical scrollbars.
pub fn scroll_view(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(
        scene,
        name,
        Some(parent),
        RectTransform::sized(200.0, 200.0),
    );
    scene.add(
        e,
        UiImage {
            color: [1.0, 1.0, 1.0, 100.0 / 255.0],
            ..Default::default()
        },
    );
    let parts = scroll_parts(scene, e, true);
    scene.add(
        e,
        UiScrollRect {
            content: Some(parts.content),
            viewport: Some(parts.viewport),
            horizontal_scrollbar: parts.horizontal,
            vertical_scrollbar: Some(parts.vertical),
            ..Default::default()
        },
    );
    e
}

/// Scroll View that only scrolls vertically (no horizontal scrollbar).
pub fn vertical_scroll_view(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(
        scene,
        name,
        Some(parent),
        RectTransform::sized(200.0, 200.0),
    );
    scene.add(
        e,
        UiImage {
            color: [1.0, 1.0, 1.0, 100.0 / 255.0],
            ..Default::default()
        },
    );
    let parts = scroll_parts(scene, e, false);
    scene.add(
        e,
        UiScrollRect {
            content: Some(parts.content),
            viewport: Some(parts.viewport),
            horizontal: false,
            vertical_scrollbar: Some(parts.vertical),
            ..Default::default()
        },
    );
    e
}

struct ScrollParts {
    viewport: Entity,
    content: Entity,
    horizontal: Option<Entity>,
    vertical: Entity,
}

/// Viewport, Content and scrollbars of a scroll view (`horizontal` = add a horizontal bar).
fn scroll_parts(scene: &mut Scene, e: Entity, horizontal: bool) -> ScrollParts {
    let bar = 20.0;
    let bottom = if horizontal { bar - 3.0 } else { 0.0 };
    let mut vp_rt = inset(0.0, bottom, bar - 3.0, 0.0);
    vp_rt.set_pivot_keep_rect(Vec2::new(0.0, 1.0));
    let viewport = image(scene, "Viewport", e, vp_rt, [1.0; 4]);
    scene.add(
        viewport,
        UiMask {
            show_mask_graphic: false,
            ..Default::default()
        },
    );
    let content_rt = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::ONE,
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::ZERO,
        size_delta: Vec2::new(0.0, 300.0),
    };
    let content = spawn(scene, "Content", Some(viewport), content_rt);
    let horizontal = horizontal.then(|| {
        let h = scrollbar(
            scene,
            "Scrollbar Horizontal",
            e,
            SliderDirection::LeftToRight,
        );
        let mut rt = rect(
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::ZERO,
            Vec2::new(-(bar - 3.0), bar),
        );
        rt.set_pivot_keep_rect(Vec2::ZERO);
        *scene.world.get::<&mut RectTransform>(h).unwrap() = rt;
        h
    });
    let v = scrollbar(scene, "Scrollbar Vertical", e, SliderDirection::BottomToTop);
    let mut rt = rect(
        Vec2::new(1.0, 0.0),
        Vec2::ONE,
        Vec2::new(-bar, bottom),
        Vec2::ZERO,
    );
    rt.set_pivot_keep_rect(Vec2::ONE);
    *scene.world.get::<&mut RectTransform>(v).unwrap() = rt;
    ScrollParts {
        viewport,
        content,
        horizontal,
        vertical: v,
    }
}

/// Dropdown: Label, Arrow and an inactive Template (a vertical Scroll View with one Item).
pub fn dropdown(scene: &mut Scene, name: &str, parent: Entity) -> Entity {
    let e = spawn(scene, name, Some(parent), RectTransform::sized(160.0, 30.0));
    scene.add(e, UiImage::default());
    let caption = label(
        scene,
        "Label",
        e,
        inset(10.0, 6.0, 25.0, 7.0),
        "Option A",
        TextAnchor::MiddleLeft,
    );
    let arrow_rt = RectTransform {
        anchor_min: Vec2::new(1.0, 0.5),
        anchor_max: Vec2::new(1.0, 0.5),
        anchored_position: Vec2::new(-15.0, 0.0),
        size_delta: Vec2::new(10.0, 6.0),
        ..Default::default()
    };
    image(scene, "Arrow", e, arrow_rt, grey(50.0));

    let template_rt = RectTransform {
        anchor_min: Vec2::ZERO,
        anchor_max: Vec2::new(1.0, 0.0),
        pivot: Vec2::new(0.5, 1.0),
        anchored_position: Vec2::new(0.0, 2.0),
        size_delta: Vec2::new(0.0, 150.0),
    };
    let template = image(scene, "Template", e, template_rt, [1.0; 4]);
    scene.game_object_mut(template).active = false;
    let parts = scroll_parts(scene, template, false);
    if let Ok(mut rt) = scene.world.get::<&mut RectTransform>(parts.content) {
        rt.size_delta.y = 28.0;
    }
    scene.add(
        template,
        UiScrollRect {
            content: Some(parts.content),
            viewport: Some(parts.viewport),
            horizontal: false,
            movement_type: MovementType::Clamped,
            vertical_scrollbar: Some(parts.vertical),
            ..Default::default()
        },
    );
    let item_rt = rect(
        Vec2::new(0.0, 0.5),
        Vec2::new(1.0, 0.5),
        Vec2::new(0.0, -10.0),
        Vec2::new(0.0, 10.0),
    );
    let item = spawn(scene, "Item", Some(parts.content), item_rt);
    let item_bg = image(
        scene,
        "Item Background",
        item,
        inset(0.0, 0.0, 0.0, 0.0),
        [1.0; 4],
    );
    let check_rt = RectTransform {
        anchor_min: Vec2::new(0.0, 0.5),
        anchor_max: Vec2::new(0.0, 0.5),
        anchored_position: Vec2::new(10.0, 0.0),
        size_delta: Vec2::splat(10.0),
        ..Default::default()
    };
    let check = image(scene, "Item Checkmark", item, check_rt, grey(50.0));
    let item_label = label(
        scene,
        "Item Label",
        item,
        inset(20.0, 1.0, 10.0, 2.0),
        "Option A",
        TextAnchor::MiddleLeft,
    );
    scene.add(
        item,
        UiToggle {
            selectable: Selectable {
                target_graphic: Some(item_bg),
                colors: crate::scene::ColorBlock {
                    highlighted: [0.85, 0.9, 1.0, 1.0],
                    ..Default::default()
                },
                ..Default::default()
            },
            graphic: Some(check),
            ..Default::default()
        },
    );
    scene.add(
        e,
        UiDropdown {
            selectable: targeting(e),
            template: Some(template),
            caption_text: Some(caption),
            item_text: Some(item_label),
            ..Default::default()
        },
    );
    e
}

/// The Event System object.
pub fn event_system(scene: &mut Scene, name: &str) -> Entity {
    let e = scene.spawn(name, None);
    scene.add(e, EventSystem::default());
    e
}
