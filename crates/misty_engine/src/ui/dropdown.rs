//! Dropdown lists: opening copies the dropdown's inactive Template under the canvas (drawn on
//! top, below the dropdown or above it when there is no room), with one Item per option.

use glam::Vec2;
use hecs::Entity;

use super::layout::Rect;
use super::tree::{UiLayout, canvas_of};
use crate::scene::{RectTransform, Scene, UiDropdown, UiText, UiToggle};

/// An open dropdown list (lives while playing).
#[derive(Clone, Debug, PartialEq)]
pub struct OpenDropdown {
    pub dropdown: Entity,
    /// Root of the copied template ("Dropdown List").
    pub list: Entity,
    /// (item toggle, option index).
    pub items: Vec<(Entity, usize)>,
}

/// Sibling-index path from `root` down to `e` (`None` if `e` is not below `root`).
fn path(scene: &Scene, root: Entity, e: Entity) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    let mut cur = e;
    while cur != root {
        out.push(scene.sibling_index(cur));
        cur = scene.parent(cur)?;
    }
    out.reverse();
    Some(out)
}

fn follow(scene: &Scene, root: Entity, path: &[usize]) -> Option<Entity> {
    let mut cur = root;
    for i in path {
        cur = *scene.children(cur).get(*i)?;
    }
    Some(cur)
}

fn rt(scene: &Scene, e: Entity) -> RectTransform {
    scene
        .world
        .get::<&RectTransform>(e)
        .map(|r| *r)
        .unwrap_or_default()
}

/// Rect of `e` inside `root_rect`, going down the given path of objects.
fn rect_down(scene: &Scene, root_rect: Rect, chain: &[Entity]) -> Rect {
    chain
        .iter()
        .fold(root_rect, |r, e| rt(scene, *e).rect_in(r))
}

/// Open `dropdown`'s list; `None` if it has no usable template.
pub fn open(scene: &mut Scene, layout: &UiLayout, dropdown: Entity) -> Option<OpenDropdown> {
    let d = scene
        .world
        .get::<&UiDropdown>(dropdown)
        .ok()
        .map(|r| (*r).clone())?;
    let template = d.template.filter(|t| scene.contains(*t))?;
    let item_text = d.item_text.filter(|t| scene.is_ancestor(template, *t));
    let canvas = canvas_of(scene, dropdown)?;
    let node = layout.node(dropdown)?;
    let canvas_rect = layout.node(canvas)?.rect;
    // The item: the item text's Toggle (or its parent); its parent is the list content.
    let item = item_text.and_then(|t| {
        let mut cur = Some(t);
        while let Some(c) = cur {
            if scene.world.get::<&UiToggle>(c).is_ok() {
                return Some(c);
            }
            cur = scene.parent(c).filter(|p| *p != template);
        }
        scene.parent(t).filter(|p| *p != template)
    })?;
    let item_path = path(scene, template, item)?;
    let text_path = item_text.and_then(|t| path(scene, item, t));
    let mut chain: Vec<Entity> = Vec::new();
    let mut cur = item;
    while cur != template {
        chain.push(cur);
        cur = scene.parent(cur)?;
    }
    chain.reverse();
    // The item must sit below the template (not be it), and the copy needs a RectTransform
    // to place: check before copying anything.
    if chain.is_empty() || scene.world.get::<&RectTransform>(template).is_err() {
        return None;
    }

    let template_rect = rt(scene, template).rect_in(node.rect);
    let content_rect = rect_down(scene, template_rect, &chain[..chain.len() - 1]);
    let item_h = rect_down(scene, template_rect, &chain).size().y.max(1.0);
    let margin = ((content_rect.size().y - item_h) * 0.5).max(0.0);

    let list = scene.copy_subtree(template, Some(canvas));
    scene.game_object_mut(list).name = "Dropdown List".into();
    scene.game_object_mut(list).active = true;
    let first = follow(scene, list, &item_path)?;
    let content = scene.parent(first)?;
    let mut items = Vec::new();
    for (i, option) in d.options.iter().enumerate() {
        let it = if i == 0 {
            first
        } else {
            scene.copy_subtree(first, Some(content))
        };
        scene.game_object_mut(it).name = format!("Item {i}: {option}");
        scene.game_object_mut(it).active = true;
        if let Ok(mut r) = scene.world.get::<&mut RectTransform>(it) {
            r.anchor_min.y = 1.0;
            r.anchor_max.y = 1.0;
            r.pivot.y = 0.5;
            r.size_delta.y = item_h;
            r.anchored_position.y = -(margin + item_h * (i as f32 + 0.5));
        }
        if let Some(t) = text_path.as_ref().and_then(|p| follow(scene, it, p))
            && let Ok(mut text) = scene.world.get::<&mut UiText>(t)
        {
            text.text = option.clone();
        }
        if let Ok(mut t) = scene.world.get::<&mut UiToggle>(it) {
            t.is_on = i as i32 == d.value;
        }
        items.push((it, i));
    }
    let content_h = margin * 2.0 + item_h * d.options.len() as f32;
    if let Ok(mut r) = scene.world.get::<&mut RectTransform>(content) {
        r.size_delta.y += content_h - content_rect.size().y;
    }

    // Shrink the list when the options don't fill it, keeping its top edge.
    let viewport = if chain.len() >= 3 { &chain[..1] } else { &[] };
    let viewport_h = rect_down(scene, template_rect, viewport).size().y;
    let mut rect = template_rect;
    if content_h < viewport_h {
        rect.min.y += viewport_h - content_h;
    }
    // No room below: open above the dropdown instead.
    if rect.min.y < canvas_rect.min.y {
        let h = rect.size().y;
        rect.min.y = node.rect.max.y + (node.rect.min.y - rect.max.y);
        rect.max.y = rect.min.y + h;
    }
    *scene.world.get::<&mut RectTransform>(list).ok()? = RectTransform {
        anchor_min: Vec2::ZERO,
        anchor_max: Vec2::ZERO,
        pivot: Vec2::ZERO,
        anchored_position: rect.min - canvas_rect.min,
        size_delta: rect.size(),
    };
    Some(OpenDropdown {
        dropdown,
        list,
        items,
    })
}

/// Remove an open list.
pub fn close(scene: &mut Scene, open: &OpenDropdown) {
    if scene.contains(open.list) {
        scene.despawn(open.list);
    }
}
