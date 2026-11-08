//! The UI event system while playing: pointer enter / exit / down / up / click / drag routed to
//! the widget under the pointer, the selection, keyboard input for Input Fields, arrow-key
//! navigation, Scroll Rect movement and value-changed events (found by comparing each widget's
//! value with the last frame, so changes made by scripts are reported too).

use std::collections::{HashMap, HashSet};

use glam::Vec2;
use hecs::Entity;

use super::driven::{clamp_scrollbar, clamp_slider, display_text, slider_value_at};
use super::dropdown::{self, OpenDropdown};
use super::input::{
    ButtonState, ancestor, interactable, is_selectable, raycast_hit, selectable, tint_color,
};
use super::input_field::{EditState, KeyOutcome, caret_at};
use super::layout::Rect;
use super::navigation::{NavDir, find_selectable};
use super::scroll;
use super::text::{builtin_font, caret_stops};
use super::tree::{UiLayout, layout};
use crate::scene::{
    EventSystem, MovementType, RectTransform, Scene, SliderDirection, UiButton, UiDropdown,
    UiInputField, UiScrollRect, UiScrollbar, UiSlider, UiText, UiToggle, UiToggleGroup,
};

/// One frame of pointer and keyboard input for the UI (screen pixels, y down).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointerInput {
    pub position: Option<Vec2>,
    pub down: bool,
    pub held: bool,
    pub up: bool,
    /// Wheel movement in pixels (y > 0 = away from the user).
    pub scroll: Vec2,
    /// Characters typed this frame.
    pub text: String,
    /// Keys pressed this frame, auto-repeats included (script key names).
    pub keys: Vec<String>,
    pub shift: bool,
    pub ctrl: bool,
    pub screen: Vec2,
    pub dt: f32,
    /// Navigation comes from the Input System's UI map (`navigate`, `submit`, `cancel`) instead
    /// of the arrow / Return / Space / Escape keys in `keys`.
    pub nav_from_actions: bool,
    /// Navigate this frame (stick / d-pad / arrows, y up); held directions repeat.
    pub navigate: Vec2,
    /// Submit / Cancel pressed this frame.
    pub submit: bool,
    pub cancel: bool,
}

/// A widget value passed to value-changed handlers.
#[derive(Clone, Debug, PartialEq)]
pub enum UiValue {
    Bool(bool),
    Number(f32),
    Int(i32),
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    /// A Button was clicked (or submitted with Return).
    Click(Entity),
    ValueChanged(Entity, UiValue),
    /// An Input Field finished editing with this text.
    EndEdit(Entity, String),
    /// A `<link="id">` of a rich text was clicked.
    LinkClicked(Entity, String),
    /// Cancel (Esc, gamepad East) was pressed with this object selected.
    Cancel(Entity),
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    /// Pointer offset from the handle centre along the slider (pixels).
    Slider {
        e: Entity,
        grab: f32,
    },
    /// Pointer offset from the handle start (0..1 of the sliding area).
    Scrollbar {
        e: Entity,
        grab: f32,
    },
    Scroll {
        e: Entity,
        start_pointer: Vec2,
        start_content: Vec2,
    },
    Text {
        e: Entity,
    },
}

/// Hover / press / selection / editing state (lives while playing).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiInteraction {
    /// Selectable under the pointer.
    pub hovered: Option<Entity>,
    /// Selectable the current press started on.
    pub pressed: Option<Entity>,
    pub selected: Option<Entity>,
    /// Input Field being typed into.
    pub editing: Option<EditState>,
    /// Seconds since the caret last moved (blinking).
    pub caret_time: f32,
    pub dropdown: Option<OpenDropdown>,
    press_origin: Option<Vec2>,
    /// Scroll Rect a press may start dragging once it moves past the threshold.
    pending_scroll: Option<Entity>,
    drag: Option<Drag>,
    velocity: HashMap<Entity, Vec2>,
    values: HashMap<Entity, UiValue>,
    /// Scrollbars moved by the pointer this frame (their Scroll Rect follows them).
    moved_bars: HashSet<Entity>,
    /// The rich text link a press started on (clicked when released over the same link).
    link_press: Option<(Entity, String)>,
    /// Canvas the current press started on: the pointer stays in its pixels while dragging.
    press_canvas: Option<Entity>,
    /// Held Navigate direction (repeats).
    repeat: super::navigation::MoveRepeat,
    started: bool,
}

/// Settings from the scene's Event System (defaults without one).
pub fn settings(scene: &Scene) -> EventSystem {
    scene
        .walk()
        .into_iter()
        .find_map(|(e, _)| {
            let es = scene.world.get::<&EventSystem>(e).ok()?;
            (es.enabled && scene.active_in_hierarchy(e)).then(|| (*es).clone())
        })
        .unwrap_or_default()
}

/// Position along a slider/scrollbar axis as 0..1 of `r` (screen pixels), direction applied.
fn axis_t(r: Rect, p: Vec2, dir: SliderDirection) -> f32 {
    let t = if dir.axis() == 0 {
        (p.x - r.min.x) / r.size().x.max(1e-3)
    } else {
        (r.max.y - p.y) / r.size().y.max(1e-3)
    };
    let t = t.clamp(0.0, 1.0);
    if dir.reversed() { 1.0 - t } else { t }
}

fn is_drag_handler(scene: &Scene, e: Entity) -> bool {
    let w = &scene.world;
    w.get::<&UiSlider>(e).is_ok()
        || w.get::<&UiScrollbar>(e).is_ok()
        || w.get::<&UiInputField>(e).is_ok()
        || w.get::<&UiScrollRect>(e).is_ok_and(|s| s.enabled)
}

fn get<T: Clone + hecs::Component>(scene: &Scene, e: Entity) -> Option<T> {
    scene.world.get::<&T>(e).ok().map(|c| (*c).clone())
}

/// Every widget value, for change detection.
fn current_values(scene: &Scene) -> Vec<(Entity, UiValue)> {
    let w = &scene.world;
    let mut out = Vec::new();
    for (e, t) in w.query::<(Entity, &UiToggle)>().iter() {
        out.push((e, UiValue::Bool(t.is_on)));
    }
    for (e, s) in w.query::<(Entity, &UiSlider)>().iter() {
        out.push((e, UiValue::Number(s.value)));
    }
    for (e, s) in w.query::<(Entity, &UiScrollbar)>().iter() {
        out.push((e, UiValue::Number(s.value)));
    }
    for (e, f) in w.query::<(Entity, &UiInputField)>().iter() {
        out.push((e, UiValue::Text(f.text.clone())));
    }
    for (e, d) in w.query::<(Entity, &UiDropdown)>().iter() {
        out.push((e, UiValue::Int(d.value)));
    }
    out
}

impl UiInteraction {
    /// Feed one frame of input; returns clicks, value changes and finished edits.
    pub fn update(&mut self, scene: &mut Scene, input: &PointerInput) -> Vec<UiEvent> {
        let mut events = Vec::new();
        self.forget_missing(scene);
        if !self.started {
            self.ensure_groups(scene, true);
            self.selected = settings(scene)
                .first_selected
                .filter(|e| scene.contains(*e));
        }
        let l = layout(scene, input.screen);
        let hit_at = input.position.and_then(|p| raycast_hit(&l, scene, p));
        let hit = hit_at.map(|h| h.0);
        self.hovered = hit.and_then(|h| ancestor(scene, h, is_selectable));
        self.moved_bars.clear();
        // The pointer in canvas pixels: of the canvas a press started on while it lasts, else of
        // the canvas under it (an overlay's pixels are the image's).
        let hit_canvas = hit.and_then(|h| l.node(h)).map(|n| n.canvas);
        if input.down || self.press_origin.is_none() {
            self.press_canvas = hit_canvas;
        }
        let position = match (input.position, self.press_canvas) {
            (Some(_), Some(c)) if hit_canvas == Some(c) => hit_at.map(|h| h.1),
            (Some(p), Some(c)) => l.to_canvas(c, p),
            (p, _) => p,
        };

        let link_under =
            |p: Vec2| hit.and_then(|h| super::rich::link_at(scene, &l, h, p).map(|id| (h, id)));
        if input.down
            && let Some(p) = position
        {
            self.link_press = link_under(p);
            self.pointer_down(scene, &l, hit, p, &mut events);
        } else if input.down {
            self.end_edit(scene, &mut events);
            self.selected = None;
        }
        if input.held
            && let Some(p) = position
        {
            self.pointer_drag(scene, &l, p, input.dt);
        }
        if input.up {
            if let Some((e, id)) = self.link_press.take()
                && position
                    .and_then(|p| super::rich::link_at(scene, &l, e, p))
                    .is_some_and(|now| now == id)
                && hit == Some(e)
            {
                events.push(UiEvent::LinkClicked(e, id));
            }
            self.pointer_up(scene, &l, &mut events);
        }
        if input.scroll != Vec2::ZERO
            && let Some(h) = hit
        {
            self.wheel(scene, &l, h, input.scroll);
        }
        if self.editing.is_some() {
            self.type_keys(scene, input, &mut events);
        } else {
            self.navigate(scene, &l, input, &mut events);
        }
        self.scroll_rects(scene, &l, input.dt);
        self.caret_time += input.dt;
        self.diff_values(scene, &mut events);
        events
    }

    fn forget_missing(&mut self, scene: &Scene) {
        let alive = |e: &Option<Entity>| e.filter(|x| scene.contains(*x));
        self.hovered = alive(&self.hovered);
        self.pressed = alive(&self.pressed);
        self.selected = alive(&self.selected);
        if self
            .editing
            .as_ref()
            .is_some_and(|ed| !scene.contains(ed.field) || !scene.active_in_hierarchy(ed.field))
        {
            self.editing = None;
        }
        if self
            .dropdown
            .as_ref()
            .is_some_and(|d| !scene.contains(d.list) || !scene.contains(d.dropdown))
        {
            self.dropdown = None;
        }
        self.velocity.retain(|e, _| scene.contains(*e));
    }

    fn pointer_down(
        &mut self,
        scene: &mut Scene,
        l: &UiLayout,
        hit: Option<Entity>,
        p: Vec2,
        events: &mut Vec<UiEvent>,
    ) {
        self.press_origin = Some(p);
        // An open list closes on any press outside it (the press goes no further).
        if let Some(open) = self.dropdown.clone()
            && !hit.is_some_and(|h| scene.is_ancestor(open.list, h))
        {
            dropdown::close(scene, &open);
            self.dropdown = None;
            self.pressed = None;
            return;
        }
        let target = self.hovered;
        if self
            .editing
            .as_ref()
            .is_some_and(|ed| Some(ed.field) != target)
        {
            self.end_edit(scene, events);
        }
        self.pressed = target;
        self.selected = target.filter(|t| interactable(scene, *t));
        self.drag = None;
        self.pending_scroll = None;
        let Some(handler) = hit.and_then(|h| ancestor(scene, h, is_drag_handler)) else {
            return;
        };
        if let Some(s) = get::<UiSlider>(scene, handler) {
            if s.selectable.interactable {
                self.press_slider(scene, l, handler, &s, hit, p);
            }
        } else if let Some(sb) = get::<UiScrollbar>(scene, handler) {
            if sb.selectable.interactable {
                self.press_scrollbar(scene, l, handler, &sb, p);
            }
        } else if let Some(f) = get::<UiInputField>(scene, handler) {
            if f.selectable.interactable {
                let shown = self
                    .editing
                    .as_ref()
                    .filter(|ed| ed.field == handler)
                    .map_or(0, |ed| ed.caret);
                let caret = pointer_caret(scene, l, &f, p, shown);
                if self.editing.as_ref().is_some_and(|ed| ed.field == handler) {
                    if let Some(ed) = self.editing.as_mut() {
                        ed.caret = caret;
                        ed.anchor = caret;
                    }
                } else {
                    self.editing = Some(EditState::new(handler, &f.text, caret));
                }
                self.caret_time = 0.0;
                self.drag = Some(Drag::Text { e: handler });
            }
        } else {
            self.pending_scroll = Some(handler);
            self.velocity.insert(handler, Vec2::ZERO);
        }
    }

    /// The rect a slider maps the pointer into: the handle's or fill's container.
    fn slider_area(scene: &Scene, l: &UiLayout, e: Entity, s: &UiSlider) -> Option<Rect> {
        let part = s.handle_rect.or(s.fill_rect);
        let container = part.and_then(|p| scene.parent(p)).unwrap_or(e);
        l.node(container).map(|n| n.screen)
    }

    fn press_slider(
        &mut self,
        scene: &mut Scene,
        l: &UiLayout,
        e: Entity,
        s: &UiSlider,
        hit: Option<Entity>,
        p: Vec2,
    ) {
        let Some(area) = Self::slider_area(scene, l, e, s) else {
            return;
        };
        // Pressing the handle keeps the grab offset; elsewhere the value jumps to the pointer.
        let on_handle = s
            .handle_rect
            .zip(hit)
            .is_some_and(|(h, hit)| scene.is_ancestor(h, hit));
        let mut grab = 0.0;
        if on_handle && let Some(n) = s.handle_rect.and_then(|h| l.node(h)) {
            let c = n.screen.center();
            grab = if s.direction.axis() == 0 {
                p.x - c.x
            } else {
                p.y - c.y
            };
        } else {
            let v = slider_value_at(s, axis_t(area, p, s.direction));
            if let Ok(mut c) = scene.world.get::<&mut UiSlider>(e) {
                c.value = v;
            }
        }
        self.drag = Some(Drag::Slider { e, grab });
    }

    fn press_scrollbar(
        &mut self,
        scene: &mut Scene,
        l: &UiLayout,
        e: Entity,
        sb: &UiScrollbar,
        p: Vec2,
    ) {
        let Some(area) = sb
            .handle_rect
            .and_then(|h| scene.parent(h))
            .and_then(|c| l.node(c))
            .map(|n| n.screen)
        else {
            return;
        };
        let t = axis_t(area, p, sb.direction);
        let size = sb.size.clamp(0.0, 1.0);
        let start = clamp_scrollbar(sb, sb.value) * (1.0 - size);
        if t >= start && t <= start + size {
            self.drag = Some(Drag::Scrollbar { e, grab: t - start });
        } else {
            // Clicking the bar pages towards the pointer.
            let step = if t < start { -size } else { size };
            let v = clamp_scrollbar(sb, sb.value + step);
            if let Ok(mut c) = scene.world.get::<&mut UiScrollbar>(e) {
                c.value = v;
            }
            self.moved_bars.insert(e);
        }
    }

    fn pointer_drag(&mut self, scene: &mut Scene, l: &UiLayout, p: Vec2, dt: f32) {
        if let (Some(sr), Some(origin)) = (self.pending_scroll, self.press_origin)
            && self.drag.is_none()
            && (p - origin).length() >= settings(scene).drag_threshold
        {
            let start = get::<UiScrollRect>(scene, sr)
                .and_then(|s| s.content)
                .and_then(|c| get::<RectTransform>(scene, c))
                .map_or(Vec2::ZERO, |rt| rt.anchored_position);
            self.drag = Some(Drag::Scroll {
                e: sr,
                start_pointer: origin,
                start_content: start,
            });
            // Dragging the list cancels the click on whatever was pressed.
            self.pressed = None;
            self.pending_scroll = None;
        }
        match self.drag.clone() {
            Some(Drag::Slider { e, grab }) => {
                let Some(s) = get::<UiSlider>(scene, e) else {
                    return;
                };
                let Some(area) = Self::slider_area(scene, l, e, &s) else {
                    return;
                };
                let mut q = p;
                q[s.direction.axis()] -= grab;
                let v = slider_value_at(&s, axis_t(area, q, s.direction));
                if let Ok(mut c) = scene.world.get::<&mut UiSlider>(e) {
                    c.value = v;
                }
            }
            Some(Drag::Scrollbar { e, grab }) => {
                let Some(sb) = get::<UiScrollbar>(scene, e) else {
                    return;
                };
                let Some(area) = sb
                    .handle_rect
                    .and_then(|h| scene.parent(h))
                    .and_then(|c| l.node(c))
                    .map(|n| n.screen)
                else {
                    return;
                };
                let free = (1.0 - sb.size.clamp(0.0, 1.0)).max(1e-4);
                let v = clamp_scrollbar(&sb, (axis_t(area, p, sb.direction) - grab) / free);
                if let Ok(mut c) = scene.world.get::<&mut UiScrollbar>(e) {
                    c.value = v;
                }
                self.moved_bars.insert(e);
            }
            Some(Drag::Scroll {
                e,
                start_pointer,
                start_content,
            }) => self.drag_scroll(scene, l, e, p - start_pointer, start_content, dt),
            Some(Drag::Text { e }) => {
                if let Some(f) = get::<UiInputField>(scene, e) {
                    let shown = self
                        .editing
                        .as_ref()
                        .filter(|ed| ed.field == e)
                        .map_or(0, |ed| ed.caret);
                    let caret = pointer_caret(scene, l, &f, p, shown);
                    if let Some(ed) = self.editing.as_mut().filter(|ed| ed.field == e)
                        && ed.caret != caret
                    {
                        ed.caret = caret;
                        self.caret_time = 0.0;
                    }
                }
            }
            None => {}
        }
    }

    fn pointer_up(&mut self, scene: &mut Scene, l: &UiLayout, events: &mut Vec<UiEvent>) {
        let clicked = self.pressed.take().filter(|p| self.hovered == Some(*p));
        self.drag = None;
        self.pending_scroll = None;
        self.press_origin = None;
        if let Some(b) = clicked.filter(|b| interactable(scene, *b)) {
            self.submit(scene, l, b, events);
        }
    }

    /// Click / Return on a widget.
    fn submit(&mut self, scene: &mut Scene, l: &UiLayout, e: Entity, events: &mut Vec<UiEvent>) {
        if scene.world.get::<&UiButton>(e).is_ok() {
            events.push(UiEvent::Click(e));
        } else if scene.world.get::<&UiToggle>(e).is_ok() {
            if let Some(open) = self.dropdown.clone()
                && let Some((_, index)) = open.items.iter().find(|(it, _)| *it == e)
            {
                if let Ok(mut d) = scene.world.get::<&mut UiDropdown>(open.dropdown) {
                    d.value = *index as i32;
                }
                dropdown::close(scene, &open);
                self.dropdown = None;
                self.selected = Some(open.dropdown);
                return;
            }
            self.click_toggle(scene, e);
        } else if scene.world.get::<&UiDropdown>(e).is_ok() {
            if let Some(open) = self.dropdown.take() {
                dropdown::close(scene, &open);
            } else {
                self.dropdown = dropdown::open(scene, l, e);
            }
        } else if let Some(f) = get::<UiInputField>(scene, e)
            && self.editing.is_none()
        {
            let n = f.text.chars().count();
            let mut ed = EditState::new(e, &f.text, n);
            ed.anchor = 0;
            self.editing = Some(ed);
            self.caret_time = 0.0;
        }
    }

    fn click_toggle(&mut self, scene: &mut Scene, e: Entity) {
        let Some(t) = get::<UiToggle>(scene, e) else {
            return;
        };
        // A group that must keep one toggle on ignores clicks on the one that is on.
        let locked = t.is_on
            && t.group
                .and_then(|g| get::<UiToggleGroup>(scene, g))
                .is_some_and(|g| !g.allow_switch_off);
        if !locked && let Ok(mut c) = scene.world.get::<&mut UiToggle>(e) {
            c.is_on = !c.is_on;
        }
    }

    fn end_edit(&mut self, scene: &mut Scene, events: &mut Vec<UiEvent>) {
        if let Some(ed) = self.editing.take()
            && let Some(f) = get::<UiInputField>(scene, ed.field)
        {
            events.push(UiEvent::EndEdit(ed.field, f.text));
        }
    }

    fn type_keys(&mut self, scene: &mut Scene, input: &PointerInput, events: &mut Vec<UiEvent>) {
        let Some(mut ed) = self.editing.clone() else {
            return;
        };
        let Some(f) = get::<UiInputField>(scene, ed.field) else {
            self.editing = None;
            return;
        };
        let mut text = f.text.clone();
        let mut moved = ed.insert(&mut text, &input.text, &f);
        let mut finish = None;
        for k in &input.keys {
            let before = (ed.caret, ed.anchor);
            match ed.key(&mut text, k, input.shift, input.ctrl, &f) {
                KeyOutcome::Submit => finish = Some(false),
                KeyOutcome::Cancel => finish = Some(true),
                KeyOutcome::Edited => moved = true,
                KeyOutcome::None => moved |= before != (ed.caret, ed.anchor),
            }
            if finish.is_some() {
                break;
            }
        }
        if finish == Some(true) {
            text = ed.original.clone();
        }
        if let Ok(mut c) = scene.world.get::<&mut UiInputField>(ed.field) {
            c.text = text;
        }
        if moved {
            self.caret_time = 0.0;
        }
        self.editing = Some(ed);
        if finish.is_some() {
            self.end_edit(scene, events);
        }
    }

    /// Navigation: moves (arrow keys, or the Input System's Navigate with repeats) change the
    /// selection or a selected slider's value; Submit clicks the selection, Cancel closes an
    /// open dropdown or reaches the selection's scripts.
    fn navigate(
        &mut self,
        scene: &mut Scene,
        l: &UiLayout,
        input: &PointerInput,
        events: &mut Vec<UiEvent>,
    ) {
        let es = settings(scene);
        if !es.send_navigation_events {
            return;
        }
        let mut moves = Vec::new();
        let (mut submit, mut cancel) = (input.submit, input.cancel);
        if input.nav_from_actions {
            moves.extend(self.repeat.step(
                input.navigate,
                input.dt,
                es.move_repeat_delay,
                es.move_repeat_rate,
            ));
        } else {
            for k in &input.keys {
                match k.as_str() {
                    "return" | "space" => submit = true,
                    "escape" => cancel = true,
                    k => moves.extend(NavDir::from_key(k)),
                }
            }
            // A legacy gamepad stick / d-pad (when there is one) repeats like the actions do.
            moves.extend(self.repeat.step(
                input.navigate,
                input.dt,
                es.move_repeat_delay,
                es.move_repeat_rate,
            ));
        }
        if submit && let Some(s) = self.selected.filter(|s| interactable(scene, *s)) {
            self.submit(scene, l, s, events);
        }
        if cancel {
            if let Some(open) = self.dropdown.take() {
                dropdown::close(scene, &open);
                self.selected = Some(open.dropdown);
            } else if let Some(s) = self.selected {
                events.push(UiEvent::Cancel(s));
            }
        }
        for dir in moves {
            let Some(cur) = self.selected.filter(|e| l.node(*e).is_some()) else {
                self.selected = es.first_selected.filter(|e| scene.contains(*e));
                continue;
            };
            let v = dir.vec();
            if let Some(s) = get::<UiSlider>(scene, cur).filter(|s| s.selectable.interactable)
                && let axis = s.direction.axis()
                && v[axis] != 0.0
            {
                // Image y is down; vertical sliders grow upwards.
                let mut sign = if axis == 0 { v.x } else { -v.y };
                if s.direction.reversed() {
                    sign = -sign;
                }
                let step = if s.whole_numbers {
                    1.0
                } else {
                    (s.max_value - s.min_value) * 0.1
                };
                if let Ok(mut c) = scene.world.get::<&mut UiSlider>(cur) {
                    c.value = clamp_slider(&s, s.value + step * sign);
                }
                continue;
            }
            if let Some(next) = find_selectable(scene, l, cur, dir) {
                self.selected = Some(next);
            }
        }
    }

    /// View and content rects (canvas units) of a scroll rect.
    fn scroll_rects_of(
        scene: &Scene,
        l: &UiLayout,
        e: Entity,
        sr: &UiScrollRect,
    ) -> Option<(Rect, Rect, Vec2, f32)> {
        let content = sr.content?;
        let view = l.node(sr.viewport.unwrap_or(e))?;
        let c = l.node(content)?;
        let pivot = get::<RectTransform>(scene, content)?.pivot;
        Some((view.rect, c.rect, pivot, view.scale))
    }

    fn drag_scroll(
        &mut self,
        scene: &mut Scene,
        l: &UiLayout,
        e: Entity,
        pointer_delta: Vec2,
        start_content: Vec2,
        dt: f32,
    ) {
        let Some(sr) = get::<UiScrollRect>(scene, e) else {
            return;
        };
        let Some((view, content, pivot, scale)) = Self::scroll_rects_of(scene, l, e, &sr) else {
            return;
        };
        let Some(c) = sr.content else { return };
        let old = get::<RectTransform>(scene, c).map_or(Vec2::ZERO, |r| r.anchored_position);
        // Screen y is down, canvas y is up.
        let delta = Vec2::new(pointer_delta.x, -pointer_delta.y) / scale;
        let mut pos = start_content + delta;
        if !sr.horizontal {
            pos.x = old.x;
        }
        if !sr.vertical {
            pos.y = old.y;
        }
        let bounds = scroll::content_bounds(content, view, pivot);
        let off = scroll::offset(bounds, view, pos - old, sr.horizontal, sr.vertical);
        pos += off;
        if sr.movement_type == MovementType::Elastic {
            for axis in 0..2 {
                if off[axis] != 0.0 {
                    pos[axis] -= scroll::rubber(off[axis], view.size()[axis]);
                }
            }
        } else if sr.movement_type == MovementType::Unrestricted {
            pos -= off;
        }
        if let Ok(mut rt) = scene.world.get::<&mut RectTransform>(c) {
            rt.anchored_position = pos;
        }
        if dt > 0.0 {
            let v = self.velocity.entry(e).or_default();
            let new = (pos - old) / dt;
            *v = v.lerp(new, (dt * 10.0).min(1.0));
        }
    }

    fn wheel(&mut self, scene: &mut Scene, l: &UiLayout, hit: Entity, wheel: Vec2) {
        let Some(e) = ancestor(scene, hit, |s, x| {
            s.world.get::<&UiScrollRect>(x).is_ok_and(|r| r.enabled)
        }) else {
            return;
        };
        let Some(sr) = get::<UiScrollRect>(scene, e) else {
            return;
        };
        let Some((view, content, pivot, scale)) = Self::scroll_rects_of(scene, l, e, &sr) else {
            return;
        };
        let Some(c) = sr.content else { return };
        // Wheel away from the user shows what is above: the content moves down.
        let mut d = Vec2::new(wheel.x, -wheel.y) / scale * sr.scroll_sensitivity;
        if sr.vertical && !sr.horizontal && d.x.abs() > d.y.abs() {
            d.y = d.x;
        }
        if !sr.horizontal {
            d.x = 0.0;
        }
        if !sr.vertical {
            d.y = 0.0;
        }
        if sr.movement_type == MovementType::Clamped {
            let bounds = scroll::content_bounds(content, view, pivot);
            d += scroll::offset(bounds, view, d, sr.horizontal, sr.vertical);
        }
        if let Ok(mut rt) = scene.world.get::<&mut RectTransform>(c) {
            rt.anchored_position += d;
        }
    }

    /// Inertia / elastic return for every scroll rect, then keep its scrollbars in sync.
    fn scroll_rects(&mut self, scene: &mut Scene, l: &UiLayout, dt: f32) {
        let list: Vec<(Entity, UiScrollRect)> = l
            .nodes
            .iter()
            .filter_map(|n| get::<UiScrollRect>(scene, n.entity).map(|s| (n.entity, s)))
            .filter(|(_, s)| s.enabled)
            .collect();
        for (e, sr) in list {
            let Some((view, content, pivot, _)) = Self::scroll_rects_of(scene, l, e, &sr) else {
                continue;
            };
            let Some(c) = sr.content else { continue };
            let mut bounds = scroll::content_bounds(content, view, pivot);
            let dragging = matches!(self.drag, Some(Drag::Scroll { e: d, .. }) if d == e);
            let mut delta = Vec2::ZERO;
            // A scrollbar moved by the pointer moves the content.
            for (axis, bar) in [(0, sr.horizontal_scrollbar), (1, sr.vertical_scrollbar)] {
                if let Some(b) = bar.filter(|b| self.moved_bars.contains(b))
                    && let Some(sb) = get::<UiScrollbar>(scene, b)
                {
                    delta[axis] = scroll::delta_for(bounds, view, axis, sb.value);
                    self.velocity.insert(e, Vec2::ZERO);
                }
            }
            if !dragging && delta == Vec2::ZERO && dt > 0.0 {
                let v = self.velocity.get(&e).copied().unwrap_or_default();
                let (d, nv) = scroll::settle(
                    bounds,
                    view,
                    v,
                    sr.movement_type,
                    sr.elasticity,
                    sr.inertia,
                    sr.deceleration_rate,
                    (sr.horizontal, sr.vertical),
                    dt,
                );
                delta = d;
                self.velocity.insert(e, nv);
            }
            if delta != Vec2::ZERO
                && let Ok(mut rt) = scene.world.get::<&mut RectTransform>(c)
            {
                rt.anchored_position += delta;
            }
            bounds.min += delta;
            bounds.max += delta;
            let n = scroll::normalized(bounds, view);
            for (axis, bar) in [(0, sr.horizontal_scrollbar), (1, sr.vertical_scrollbar)] {
                let Some(b) = bar.filter(|b| !self.moved_bars.contains(b)) else {
                    continue;
                };
                let size = (view.size()[axis] / bounds.size()[axis].max(1e-3)).clamp(0.0, 1.0);
                if let Ok(mut sb) = scene.world.get::<&mut UiScrollbar>(b) {
                    let v = n[axis].clamp(0.0, 1.0);
                    if (sb.value - v).abs() > 1e-4 {
                        sb.value = v;
                    }
                    if (sb.size - size).abs() > 1e-4 {
                        sb.size = size;
                    }
                }
            }
        }
    }

    /// Toggle groups: a toggle switched on turns the others off; on the first frame a group
    /// that may not be all-off gets its first toggle switched on.
    fn ensure_groups(&mut self, scene: &mut Scene, first: bool) {
        let toggles: Vec<(Entity, Entity, bool)> = scene
            .walk()
            .into_iter()
            .filter_map(|(e, _)| {
                let t = scene.world.get::<&UiToggle>(e).ok()?;
                let g = t
                    .group
                    .filter(|g| scene.world.get::<&UiToggleGroup>(*g).is_ok())?;
                scene.active_in_hierarchy(e).then_some((e, g, t.is_on))
            })
            .collect();
        let groups: HashSet<Entity> = toggles.iter().map(|t| t.1).collect();
        for g in groups {
            let members: Vec<&(Entity, Entity, bool)> =
                toggles.iter().filter(|t| t.1 == g).collect();
            let on: Vec<Entity> = members.iter().filter(|t| t.2).map(|t| t.0).collect();
            let keep = if on.len() > 1 {
                // The one that was just switched on wins.
                on.iter()
                    .copied()
                    .find(|e| self.values.get(e) == Some(&UiValue::Bool(false)))
                    .or(on.first().copied())
            } else if on.is_empty()
                && first
                && get::<UiToggleGroup>(scene, g).is_some_and(|g| !g.allow_switch_off)
            {
                members.first().map(|t| t.0)
            } else {
                continue;
            };
            for m in members {
                if let Ok(mut t) = scene.world.get::<&mut UiToggle>(m.0) {
                    t.is_on = Some(m.0) == keep;
                }
            }
        }
    }

    fn diff_values(&mut self, scene: &mut Scene, events: &mut Vec<UiEvent>) {
        // Keep script-set values in range.
        for s in scene.world.query_mut::<&mut UiSlider>() {
            let v = clamp_slider(s, s.value);
            if v != s.value {
                s.value = v;
            }
        }
        self.ensure_groups(scene, false);
        let now = current_values(scene);
        if self.started {
            // Hierarchy order, so handlers run top to bottom.
            let order: HashMap<Entity, usize> = scene
                .walk()
                .into_iter()
                .enumerate()
                .map(|(i, (e, _))| (e, i))
                .collect();
            let mut changed: Vec<(Entity, UiValue)> = now
                .iter()
                // New widgets (instantiated, list items) only start reporting next frame.
                .filter(|(e, v)| self.values.get(e).is_some_and(|old| old != v))
                .cloned()
                .collect();
            changed.sort_by_key(|(e, _)| order.get(e).copied().unwrap_or(usize::MAX));
            events.extend(
                changed
                    .into_iter()
                    .map(|(e, v)| UiEvent::ValueChanged(e, v)),
            );
        }
        self.values = now.into_iter().collect();
        self.started = true;
    }

    /// Colour Tint state of a selectable.
    pub fn state(&self, scene: &Scene, e: Entity) -> ButtonState {
        Self::state_of(Some(self), scene, e)
    }

    /// `None` interaction = edit mode: normal or disabled.
    pub fn state_of(this: Option<&Self>, scene: &Scene, e: Entity) -> ButtonState {
        if !interactable(scene, e) {
            return ButtonState::Disabled;
        }
        let Some(i) = this else {
            return ButtonState::Normal;
        };
        if i.pressed == Some(e) && i.hovered == Some(e) {
            ButtonState::Pressed
        } else if i.selected == Some(e) {
            ButtonState::Selected
        } else if i.hovered == Some(e) {
            ButtonState::Highlighted
        } else {
            ButtonState::Normal
        }
    }

    /// Tint of every selectable's target graphic.
    pub fn tints(this: Option<&Self>, scene: &Scene, l: &UiLayout) -> HashMap<Entity, [f32; 4]> {
        l.nodes
            .iter()
            .filter_map(|n| {
                let s = selectable(scene, n.entity)?;
                let target = s.target_graphic?;
                let state = Self::state_of(this, scene, n.entity);
                Some((target, tint_color(&s.colors, state)))
            })
            .collect()
    }

    /// Is the caret drawn this frame (it blinks at the field's rate)?
    pub fn caret_visible(&self, rate: f32) -> bool {
        rate <= 0.0 || (self.caret_time * rate).fract() < 0.5
    }
}

/// Caret index under the pointer in an Input Field's text.
/// `shown_caret` = the caret the text is currently scrolled for.
fn pointer_caret(
    scene: &Scene,
    l: &UiLayout,
    f: &UiInputField,
    p: Vec2,
    shown_caret: usize,
) -> usize {
    let Some(tc) = f.text_component else { return 0 };
    let (Some(n), Some(t)) = (l.node(tc), get::<UiText>(scene, tc)) else {
        return 0;
    };
    let shown = display_text(f, &f.text);
    let px = (t.font_size * n.scale).round().max(1.0);
    let (stops, _, _) = caret_stops(builtin_font(), &shown, px, n.screen.size(), t.alignment);
    let shift = text_shift(&stops, n.screen.size().x, shown_caret);
    caret_at(&stops, p.x - n.screen.min.x + shift)
}

/// How far long text is shifted left so the caret at `caret` stays inside a box `width` wide.
pub fn text_shift(stops: &[f32], width: f32, caret: usize) -> f32 {
    let x = stops.get(caret).copied().unwrap_or(0.0);
    (x - width).max(0.0)
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
