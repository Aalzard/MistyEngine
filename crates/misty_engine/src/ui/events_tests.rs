use glam::Vec2;

use super::*;
use crate::scene::{Canvas, CanvasScaler, InputContentType, UiScaleMode, VerticalLayoutGroup};
use crate::ui::presets;

const SCREEN: Vec2 = Vec2::new(800.0, 600.0);

fn canvas() -> (Scene, Entity) {
    let mut s = Scene::new("UI");
    let c = s.spawn("Canvas", None);
    s.add(c, RectTransform::stretched());
    s.add(c, Canvas::default());
    s.add(
        c,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    (s, c)
}

/// Runs frames against one scene and remembers the events.
struct Driver {
    ui: UiInteraction,
    events: Vec<UiEvent>,
}

impl Driver {
    fn new(s: &mut Scene) -> Self {
        let mut d = Self {
            ui: UiInteraction::default(),
            events: Vec::new(),
        };
        d.frame(s, PointerInput::default());
        d
    }

    fn frame(&mut self, s: &mut Scene, mut input: PointerInput) -> Vec<UiEvent> {
        input.screen = SCREEN;
        input.dt = 1.0 / 60.0;
        let ev = self.ui.update(s, &input);
        self.events.extend(ev.clone());
        ev
    }

    fn click(&mut self, s: &mut Scene, p: Vec2) -> Vec<UiEvent> {
        let mut ev = self.frame(s, at(p, true));
        ev.extend(self.frame(s, up(p)));
        ev
    }

    fn keys(&mut self, s: &mut Scene, keys: &[&str], text: &str) -> Vec<UiEvent> {
        self.frame(
            s,
            PointerInput {
                keys: keys.iter().map(|k| k.to_string()).collect(),
                text: text.into(),
                ..Default::default()
            },
        )
    }
}

fn at(p: Vec2, down: bool) -> PointerInput {
    PointerInput {
        position: Some(p),
        down,
        held: down,
        ..Default::default()
    }
}

fn held(p: Vec2) -> PointerInput {
    PointerInput {
        position: Some(p),
        held: true,
        ..Default::default()
    }
}

fn up(p: Vec2) -> PointerInput {
    PointerInput {
        position: Some(p),
        up: true,
        ..Default::default()
    }
}

fn centre(s: &Scene, e: Entity) -> Vec2 {
    layout(s, SCREEN).node(e).unwrap().screen.center()
}

#[test]
fn slider_value_follows_the_pointer() {
    let (mut s, c) = canvas();
    let e = presets::slider(&mut s, "Slider", c);
    {
        let mut sl = s.world.get::<&mut UiSlider>(e).unwrap();
        sl.min_value = 0.0;
        sl.max_value = 10.0;
    }
    // 160 wide in the middle: x 320..480; the handle slides over 330..470.
    let mut d = Driver::new(&mut s);
    let y = 300.0;
    d.frame(&mut s, at(Vec2::new(365.0, y), true));
    assert!((s.world.get::<&UiSlider>(e).unwrap().value - 2.5).abs() < 1e-4);
    d.frame(&mut s, held(Vec2::new(435.0, y)));
    assert!((s.world.get::<&UiSlider>(e).unwrap().value - 7.5).abs() < 1e-4);
    d.frame(&mut s, held(Vec2::new(900.0, y)));
    assert_eq!(s.world.get::<&UiSlider>(e).unwrap().value, 10.0, "clamped");
    d.frame(&mut s, up(Vec2::new(900.0, y)));
    let changes: Vec<&UiEvent> = d
        .events
        .iter()
        .filter(|ev| matches!(ev, UiEvent::ValueChanged(x, _) if *x == e))
        .collect();
    assert_eq!(changes.len(), 3, "{changes:?}");
    assert_eq!(*changes[2], UiEvent::ValueChanged(e, UiValue::Number(10.0)));
    // The fill and handle follow the value.
    let sl = (*s.world.get::<&UiSlider>(e).unwrap()).clone();
    let handle = layout(&s, SCREEN)
        .node(sl.handle_rect.unwrap())
        .unwrap()
        .screen;
    assert_eq!(handle.center().x, 470.0);
    // Whole numbers round; dragging the handle keeps the grab offset.
    s.world.get::<&mut UiSlider>(e).unwrap().whole_numbers = true;
    d.frame(&mut s, at(Vec2::new(475.0, y), true));
    assert_eq!(
        s.world.get::<&UiSlider>(e).unwrap().value,
        10.0,
        "grabbed the handle"
    );
    d.frame(&mut s, held(Vec2::new(408.0, y)));
    assert_eq!(s.world.get::<&UiSlider>(e).unwrap().value, 5.0);
}

#[test]
fn toggles_flip_and_groups_keep_one_on() {
    let (mut s, c) = canvas();
    let group = s.spawn("Group", Some(c));
    s.add(group, RectTransform::stretched());
    s.add(group, UiToggleGroup::default());
    let toggles: Vec<Entity> = (0..3)
        .map(|i| {
            let t = presets::toggle(&mut s, &format!("T{i}"), group);
            s.world
                .get::<&mut RectTransform>(t)
                .unwrap()
                .anchored_position
                .y = -40.0 * i as f32;
            let mut tc = s.world.get::<&mut UiToggle>(t).unwrap();
            tc.group = Some(group);
            tc.is_on = false;
            t
        })
        .collect();
    let on = |s: &Scene| -> Vec<bool> {
        toggles
            .iter()
            .map(|t| s.world.get::<&UiToggle>(*t).unwrap().is_on)
            .collect()
    };
    let mut d = Driver::new(&mut s);
    assert_eq!(on(&s), [true, false, false], "a group starts with one on");
    {
        let p = centre(&s, toggles[2]);
        d.click(&mut s, p)
    };
    assert_eq!(on(&s), [false, false, true]);
    {
        let p = centre(&s, toggles[2]);
        d.click(&mut s, p)
    };
    assert_eq!(
        on(&s),
        [false, false, true],
        "can't switch the last one off"
    );
    // A script turning one on turns the others off.
    s.world.get::<&mut UiToggle>(toggles[1]).unwrap().is_on = true;
    let ev = d.frame(&mut s, PointerInput::default());
    assert_eq!(on(&s), [false, true, false]);
    assert!(ev.contains(&UiEvent::ValueChanged(toggles[1], UiValue::Bool(true))));
    assert!(ev.contains(&UiEvent::ValueChanged(toggles[2], UiValue::Bool(false))));
    // Allow Switch Off; without a group a toggle just flips.
    s.world
        .get::<&mut UiToggleGroup>(group)
        .unwrap()
        .allow_switch_off = true;
    {
        let p = centre(&s, toggles[1]);
        d.click(&mut s, p)
    };
    assert_eq!(on(&s), [false, false, false]);
    // The checkmark is hidden while off.
    let check = s
        .world
        .get::<&UiToggle>(toggles[0])
        .unwrap()
        .graphic
        .unwrap();
    assert!(crate::ui::driven::visuals(&s).hidden.contains(&check));
}

#[test]
fn input_field_takes_typing_and_ends_editing() {
    let (mut s, c) = canvas();
    let f = presets::input_field(&mut s, "Name", c);
    s.world.get::<&mut UiInputField>(f).unwrap().character_limit = 8;
    let mut d = Driver::new(&mut s);
    let ev = {
        let p = centre(&s, f);
        d.click(&mut s, p)
    };
    assert!(ev.is_empty());
    assert_eq!(d.ui.editing.as_ref().map(|e| e.field), Some(f));
    let ev = d.keys(&mut s, &[], "Misty");
    assert_eq!(
        ev,
        [UiEvent::ValueChanged(f, UiValue::Text("Misty".into()))]
    );
    d.keys(&mut s, &["backspace", "left"], "");
    d.keys(&mut s, &[], " Engine!");
    assert_eq!(s.world.get::<&UiInputField>(f).unwrap().text, "Mis Engt");
    let field = (*s.world.get::<&UiInputField>(f).unwrap()).clone();
    assert_eq!(field.text.chars().count(), 8, "character limit");
    // Placeholder hides once there is text; the Text shows the field's text.
    let v = crate::ui::driven::visuals(&s);
    assert!(v.hidden.contains(&field.placeholder.unwrap()));
    assert_eq!(v.text[&field.text_component.unwrap()], "Mis Engt");
    let ev = d.keys(&mut s, &["return"], "");
    assert_eq!(ev, [UiEvent::EndEdit(f, "Mis Engt".into())]);
    assert!(d.ui.editing.is_none());
    assert_eq!(d.ui.selected, Some(f), "stays selected");
    // Escape restores the text from before editing; clicking elsewhere ends editing too.
    {
        let p = centre(&s, f);
        d.click(&mut s, p)
    };
    d.keys(&mut s, &["backspace"], "");
    let ev = d.keys(&mut s, &["escape"], "");
    assert!(
        ev.contains(&UiEvent::EndEdit(f, "Mis Engt".into())),
        "{ev:?}"
    );
    {
        let p = centre(&s, f);
        d.click(&mut s, p)
    };
    let ev = d.click(&mut s, Vec2::new(5.0, 5.0));
    assert_eq!(ev, [UiEvent::EndEdit(f, "Mis Engt".into())]);
    // Password fields show stars.
    s.world.get::<&mut UiInputField>(f).unwrap().content_type = InputContentType::Password;
    let v = crate::ui::driven::visuals(&s);
    assert_eq!(v.text[&field.text_component.unwrap()], "********");
}

#[test]
fn dropdown_opens_a_list_and_picks_an_option() {
    let (mut s, c) = canvas();
    let dd = presets::dropdown(&mut s, "Quality", c);
    s.world.get::<&mut UiDropdown>(dd).unwrap().options =
        vec!["Low".into(), "Medium".into(), "High".into()];
    let mut d = Driver::new(&mut s);
    let caption = s
        .world
        .get::<&UiDropdown>(dd)
        .unwrap()
        .caption_text
        .unwrap();
    assert_eq!(crate::ui::driven::visuals(&s).text[&caption], "Low");
    {
        let p = centre(&s, dd);
        d.click(&mut s, p)
    };
    let open = d.ui.dropdown.clone().expect("list open");
    assert_eq!(open.items.len(), 3);
    assert_eq!(s.name(open.items[2].0), "Item 2: High");
    let l = layout(&s, SCREEN);
    let list = l.node(open.list).unwrap().screen;
    let below = l.node(dd).unwrap().screen;
    assert!(list.min.y >= below.max.y - 2.0, "opens below the dropdown");
    assert!(list.size().y < 150.0, "shrunk to three items: {list:?}");
    let item2 = l.node(open.items[2].0).unwrap().screen.center();
    let ev = d.click(&mut s, item2);
    assert_eq!(ev, [UiEvent::ValueChanged(dd, UiValue::Int(2))]);
    assert!(d.ui.dropdown.is_none() && !s.contains(open.list), "closed");
    // Clicking outside an open list just closes it.
    {
        let p = centre(&s, dd);
        d.click(&mut s, p)
    };
    assert!(d.ui.dropdown.is_some());
    assert!(d.click(&mut s, Vec2::new(5.0, 5.0)).is_empty());
    assert!(d.ui.dropdown.is_none());
    assert_eq!(s.world.get::<&UiDropdown>(dd).unwrap().value, 2);
}

#[test]
fn dropdown_whose_item_is_its_template_does_not_open() {
    let (mut s, c) = canvas();
    let dd = presets::dropdown(&mut s, "Quality", c);
    let template = s.world.get::<&UiDropdown>(dd).unwrap().template.unwrap();
    // Item Text set to the Template itself, which has a Toggle.
    s.add(template, UiToggle::default());
    {
        let mut d = s.world.get::<&mut UiDropdown>(dd).unwrap();
        d.options = vec!["Low".into(), "High".into()];
        d.item_text = Some(template);
    }
    let mut d = Driver::new(&mut s);
    let before = s.walk().len();
    let p = centre(&s, dd);
    d.click(&mut s, p);
    assert!(d.ui.dropdown.is_none());
    assert_eq!(s.walk().len(), before, "no half-built list left behind");
}

#[test]
fn scroll_rect_drags_clamps_and_syncs_its_scrollbar() {
    let (mut s, c) = canvas();
    let sv = presets::scroll_view(&mut s, "Scroll View", c);
    let sr = (*s.world.get::<&UiScrollRect>(sv).unwrap()).clone();
    s.world.get::<&mut UiScrollRect>(sv).unwrap().movement_type = MovementType::Clamped;
    s.world.get::<&mut UiScrollRect>(sv).unwrap().inertia = false;
    let content = sr.content.unwrap();
    // A button inside the content: dragging over it scrolls instead of clicking.
    let b = presets::button(&mut s, "Item", content);
    let mut d = Driver::new(&mut s);
    let start = centre(&s, b);
    let pos = |s: &Scene| {
        s.world
            .get::<&RectTransform>(content)
            .unwrap()
            .anchored_position
    };
    d.frame(&mut s, at(start, true));
    d.frame(&mut s, held(start - Vec2::new(0.0, 50.0)));
    assert_eq!(pos(&s).y, 50.0, "dragged up 50 pixels");
    let ev = d.frame(&mut s, up(start - Vec2::new(0.0, 50.0)));
    assert!(!ev.contains(&UiEvent::Click(b)), "drag cancels the click");
    // Content is 300 tall in a 183 tall viewport: at most 117 up.
    d.frame(&mut s, at(start, true));
    d.frame(&mut s, held(start - Vec2::new(0.0, 400.0)));
    d.frame(&mut s, up(start - Vec2::new(0.0, 400.0)));
    assert!((pos(&s).y - 117.0).abs() < 1e-3, "{:?}", pos(&s));
    let bar = (*s
        .world
        .get::<&UiScrollbar>(sr.vertical_scrollbar.unwrap())
        .unwrap())
    .clone();
    assert!(bar.value.abs() < 1e-4, "bottom");
    assert!((bar.size - 183.0 / 300.0).abs() < 1e-3);
    // The wheel scrolls back up (content moves down).
    d.frame(
        &mut s,
        PointerInput {
            position: Some(start),
            scroll: Vec2::new(0.0, 1000.0),
            ..Default::default()
        },
    );
    assert_eq!(pos(&s).y, 0.0);
}

#[test]
fn layout_group_content_and_navigation() {
    let (mut s, c) = canvas();
    let col = s.spawn("Column", Some(c));
    s.add(col, RectTransform::sized(200.0, 200.0));
    s.add(col, VerticalLayoutGroup::default());
    let a = presets::button(&mut s, "A", col);
    let b = presets::button(&mut s, "B", col);
    let mut d = Driver::new(&mut s);
    assert!(centre(&s, a).y < centre(&s, b).y, "A above B");
    {
        let p = centre(&s, a);
        d.click(&mut s, p)
    };
    assert_eq!(d.ui.selected, Some(a));
    d.keys(&mut s, &["down"], "");
    assert_eq!(d.ui.selected, Some(b));
    let ev = d.keys(&mut s, &["return"], "");
    assert_eq!(ev, [UiEvent::Click(b)]);
    assert_eq!(d.ui.state(&s, b), ButtonState::Selected);
}
