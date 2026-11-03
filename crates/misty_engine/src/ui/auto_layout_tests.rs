use glam::Vec2;

use super::*;
use crate::scene::{Canvas, CanvasScaler, Padding, TextAnchor, UiScaleMode};
use crate::ui::tree::layout;

/// 400×300 canvas (scale 1) with a `w`×`h` panel in the middle.
fn panel(w: f32, h: f32) -> (Scene, Entity) {
    let mut s = Scene::new("UI");
    let canvas = s.spawn("Canvas", None);
    s.add(canvas, Canvas::default());
    s.add(
        canvas,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    let p = s.spawn("Panel", Some(canvas));
    s.add(p, RectTransform::sized(w, h));
    (s, p)
}

fn child(s: &mut Scene, parent: Entity, le: LayoutElement) -> Entity {
    let c = s.spawn("Child", Some(parent));
    s.add(c, RectTransform::sized(40.0, 20.0));
    s.add(c, le);
    c
}

fn pref(w: f32, h: f32) -> LayoutElement {
    LayoutElement {
        preferred_width: Some(w),
        preferred_height: Some(h),
        ..Default::default()
    }
}

fn rect_of(s: &Scene, e: Entity) -> Rect {
    layout(s, Vec2::new(400.0, 300.0)).node(e).unwrap().rect
}

#[test]
fn vertical_group_stacks_from_the_top_with_padding_and_spacing() {
    // Panel spans x 100..300, y 50..250.
    let (mut s, p) = panel(200.0, 200.0);
    s.add(
        p,
        VerticalLayoutGroup(LinearLayout {
            padding: Padding {
                left: 5.0,
                right: 5.0,
                top: 10.0,
                bottom: 0.0,
            },
            spacing: 4.0,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );
    let a = child(&mut s, p, pref(50.0, 30.0));
    let b = child(&mut s, p, pref(50.0, 20.0));
    let ra = rect_of(&s, a);
    assert_eq!(ra.min, Vec2::new(105.0, 210.0));
    assert_eq!(ra.max, Vec2::new(295.0, 240.0), "full width (force expand)");
    let rb = rect_of(&s, b);
    assert_eq!(rb.max.y, 206.0);
    assert_eq!(rb.size().y, 20.0);
    let h = sizes(&s, p, 1, None);
    assert_eq!(h.preferred, 10.0 + 30.0 + 4.0 + 20.0);
}

#[test]
fn horizontal_group_shares_spare_space_by_flexible_size() {
    let (mut s, p) = panel(200.0, 50.0);
    s.add(
        p,
        HorizontalLayoutGroup(LinearLayout {
            child_force_expand_width: false,
            ..Default::default()
        }),
    );
    let a = child(
        &mut s,
        p,
        LayoutElement {
            preferred_width: Some(40.0),
            flexible_width: Some(1.0),
            ..Default::default()
        },
    );
    let b = child(
        &mut s,
        p,
        LayoutElement {
            preferred_width: Some(40.0),
            flexible_width: Some(3.0),
            ..Default::default()
        },
    );
    // 120 spare: a gets 30, b 90.
    assert_eq!(rect_of(&s, a).size().x, 70.0);
    assert_eq!(rect_of(&s, b).size().x, 130.0);
    assert_eq!(rect_of(&s, b).min.x, 170.0);
    // Not controlling the height: children keep theirs, aligned by Child Alignment.
    if let Ok(mut g) = s.world.get::<&mut HorizontalLayoutGroup>(p) {
        g.0.control_child_height = false;
        g.0.child_alignment = TextAnchor::LowerLeft;
    }
    let ra = rect_of(&s, a);
    assert_eq!((ra.min.y, ra.size().y), (125.0, 20.0));
}

#[test]
fn not_enough_room_lerps_between_min_and_preferred() {
    let (mut s, p) = panel(100.0, 50.0);
    s.add(
        p,
        HorizontalLayoutGroup(LinearLayout {
            child_force_expand_width: false,
            ..Default::default()
        }),
    );
    let le = LayoutElement {
        min_width: Some(20.0),
        preferred_width: Some(80.0),
        ..Default::default()
    };
    let a = child(&mut s, p, le.clone());
    child(&mut s, p, le);
    // Totals: min 40, preferred 160; 100 available → halfway → 50 each.
    assert_eq!(rect_of(&s, a).size().x, 50.0);
}

#[test]
fn ignored_and_reversed_children() {
    let (mut s, p) = panel(200.0, 100.0);
    s.add(
        p,
        VerticalLayoutGroup(LinearLayout {
            reverse_arrangement: true,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );
    let a = child(&mut s, p, pref(10.0, 10.0));
    let b = child(&mut s, p, pref(10.0, 10.0));
    let free = child(
        &mut s,
        p,
        LayoutElement {
            ignore_layout: true,
            ..Default::default()
        },
    );
    assert_eq!(rect_of(&s, b).max.y, 200.0, "last child on top");
    assert_eq!(rect_of(&s, a).max.y, 190.0);
    assert_eq!(rect_of(&s, free).size(), Vec2::new(40.0, 20.0), "own rect");
}

#[test]
fn grid_fills_rows_and_reports_its_size() {
    let (mut s, p) = panel(130.0, 200.0);
    s.add(
        p,
        GridLayoutGroup {
            cell_size: Vec2::new(50.0, 40.0),
            spacing: Vec2::new(10.0, 5.0),
            ..Default::default()
        },
    );
    let kids: Vec<Entity> = (0..5)
        .map(|_| child(&mut s, p, LayoutElement::default()))
        .collect();
    // Panel x 135..265, top 250: two columns fit (50 + 10 + 50 ≤ 130).
    let r = |i: usize| rect_of(&s, kids[i]);
    assert_eq!(r(0).min, Vec2::new(135.0, 210.0));
    assert_eq!(r(1).min.x, 195.0);
    assert_eq!(r(2).min, Vec2::new(135.0, 165.0));
    assert_eq!(r(4).size(), Vec2::new(50.0, 40.0));
    assert_eq!(
        sizes(&s, p, 1, Some(130.0)).preferred,
        3.0 * 40.0 + 2.0 * 5.0
    );
    if let Ok(mut g) = s.world.get::<&mut GridLayoutGroup>(p) {
        g.constraint = GridConstraint::FixedColumnCount;
        g.constraint_count = 3;
        g.start_corner = GridCorner::UpperRight;
    }
    assert_eq!(r(0).max.x, 135.0 + 170.0, "first cell in the right column");
    assert_eq!(sizes(&s, p, 0, None).preferred, 170.0);
}

#[test]
fn content_size_fitter_grows_a_group_to_its_children() {
    let (mut s, p) = panel(200.0, 10.0);
    // Pivot at the top: the panel grows downwards.
    s.world.get::<&mut RectTransform>(p).unwrap().pivot = Vec2::new(0.5, 1.0);
    s.add(
        p,
        VerticalLayoutGroup(LinearLayout {
            spacing: 2.0,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );
    s.add(
        p,
        ContentSizeFitter {
            vertical_fit: FitMode::PreferredSize,
            ..Default::default()
        },
    );
    for _ in 0..3 {
        child(&mut s, p, pref(10.0, 20.0));
    }
    let r = rect_of(&s, p);
    assert_eq!(r.size(), Vec2::new(200.0, 64.0));
    assert_eq!(r.max.y, 150.0, "top edge stays put");
}

#[test]
fn text_reports_its_preferred_size() {
    let (mut s, p) = panel(200.0, 100.0);
    s.add(
        p,
        UiText {
            text: "Hello".into(),
            font_size: 20.0,
            ..Default::default()
        },
    );
    let w = sizes(&s, p, 0, None).preferred;
    let h = sizes(&s, p, 1, None).preferred;
    assert!(w > 30.0 && w < 70.0, "{w}");
    assert!(h > 18.0 && h < 30.0, "{h}");
    s.world.get::<&mut UiText>(p).unwrap().text = "Hello Hello".into();
    let wrapped = sizes(&s, p, 1, Some(w + 1.0)).preferred;
    assert!(wrapped > h * 1.5, "wraps onto two lines: {wrapped}");
}
