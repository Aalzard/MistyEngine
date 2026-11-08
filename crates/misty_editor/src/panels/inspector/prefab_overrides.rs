//! Prefab instance overrides in the Inspector, the familiar way:
//! - instance roots get a Prefab row: Open, Select and the **Overrides ▾** dropdown (each change
//!   with Revert / Apply, then Revert All / Apply All);
//! - overridden properties show a bold label with a blue bar in the margin, and right-clicking
//!   the label offers Apply to Prefab (to the outermost prefab, or to a nested prefab the object
//!   comes from) and Revert;
//! - added components get a "+" and a blue bar on their header, components the instance removed
//!   are listed after the others; each header's menu gets Apply / Revert for its changes.
//!
//! Component editors draw their labels themselves, so the styling is applied after a card body
//! is drawn: its label texts are matched with the overridden field names ("use_gravity" ↔ "Use
//! Gravity") and repainted bold.

use std::cell::RefCell;

use egui::epaint::Shape;
use egui::layers::ShapeIdx;
use egui::{FontId, Id, Rect, RichText, Sense, Ui, pos2};
use misty_engine::hecs::Entity;
use misty_engine::scene::prefab::{Override, OverrideItem};

use crate::actions::Action;
use crate::prefabs::{display_name, field_label, read_only};
use crate::state::EditorState;
use crate::theme;

/// The selected object's prefab situation, worked out once per Inspector frame.
#[derive(Clone)]
struct Context {
    target: Entity,
    /// The outermost instance root above the target, and its asset.
    root: Entity,
    asset: String,
    items: Vec<OverrideItem>,
}

thread_local! {
    /// Open the Overrides dropdown on its next draw (screenshots).
    static OPEN_DROPDOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CURRENT: RefCell<Option<Context>> = const { RefCell::new(None) };
    /// Labels drawn as overridden this frame ("Light/Intensity"), for tests and screenshots.
    static STYLED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Work out the overrides that concern `e` (one selected object) for this Inspector frame.
pub fn begin(state: &mut EditorState, e: Entity) {
    STYLED.with(|s| s.borrow_mut().clear());
    let ctx = state.scene.outermost_prefab_root(e).and_then(|root| {
        let asset = state.scene.prefab_link(root)?;
        let items = crate::prefabs::overrides_of(state, root)?;
        Some(Context {
            target: e,
            root,
            asset,
            items,
        })
    });
    CURRENT.with(|c| *c.borrow_mut() = ctx);
}

/// Forget this frame's overrides.
pub fn end() {
    CURRENT.with(|c| *c.borrow_mut() = None);
}

fn current() -> Option<Context> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Labels drawn as overridden in the last Inspector frame ("Transform/Scale").
#[cfg(test)]
pub fn styled_labels() -> Vec<String> {
    STYLED.with(|s| s.borrow().clone())
}

/// The target's overrides on `component`.
fn component_items(ctx: &Context, component: &str) -> Vec<OverrideItem> {
    ctx.items
        .iter()
        .filter(|o| o.entity == Some(ctx.target) && o.key.component() == Some(component))
        .cloned()
        .collect()
}

/// Overridden fields of `component` on the target.
fn fields(ctx: &Context, component: &str) -> Vec<String> {
    component_items(ctx, component)
        .into_iter()
        .filter_map(|o| match o.key {
            Override::Property { property, .. } => Some(property),
            _ => None,
        })
        .collect()
}

/// Paint index where a component body starts (see [`style_body`]).
pub fn body_start(ui: &Ui) -> ShapeIdx {
    let layer = ui.layer_id();
    ui.ctx()
        .graphics(|g| g.get(layer).map(|l| l.next_idx()))
        .unwrap_or(ShapeIdx(0))
}

/// Letters and digits only, lower case: "Use Gravity" and "use_gravity" read the same.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Labels some editors use for a field, beyond its own name.
fn aliases(component: &str, field: &str) -> &'static [&'static str] {
    match (component, field) {
        ("Camera", "fov") => &["Field of View"],
        ("Camera", "near") | ("Camera", "far") => &["Clipping Planes"],
        ("Audio Source", "looping") => &["Loop"],
        ("Mesh Filter", "mesh") => &["Mesh"],
        ("Mesh Renderer", "materials" | "material") => &["Materials", "Material"],
        _ => &[],
    }
}

/// After a component body is drawn: overridden property labels become bold with a blue bar,
/// and right-clicking them offers Apply / Revert.
pub fn style_body(ui: &mut Ui, state: &mut EditorState, component: &str, start: ShapeIdx) {
    let Some(ctx) = current() else {
        return;
    };
    let fields = fields(&ctx, component);
    if fields.is_empty() {
        return;
    }
    let wanted: Vec<(String, String)> = fields
        .iter()
        .flat_map(|f| {
            let own = std::iter::once((norm(f), f.clone()));
            let extra = aliases(component, f)
                .iter()
                .map(move |a| (norm(a), f.clone()));
            own.chain(extra)
        })
        .collect();
    let layer = ui.layer_id();
    let left = ui.min_rect().left();
    let value_x = crate::widgets::value_column(ui).unwrap_or(left + 140.0);
    // (shape index, text rect, field, text, font size, colour)
    let mut hits: Vec<(usize, Rect, String, String, f32, egui::Color32)> = Vec::new();
    ui.ctx().graphics(|g| {
        let Some(list) = g.get(layer) else {
            return;
        };
        for (i, cs) in list.all_entries().enumerate().skip(start.0) {
            let Shape::Text(t) = &cs.shape else {
                continue;
            };
            let rect = t.visual_bounding_rect();
            if rect.left() > value_x - 2.0 {
                continue;
            }
            let text = t.galley.text().to_owned();
            let n = norm(&text);
            if n.is_empty() {
                continue;
            }
            if let Some((_, field)) = wanted.iter().find(|(w, _)| *w == n) {
                let format = t.galley.job.sections.first().map(|s| s.format.clone());
                let size = format.as_ref().map_or(13.0, |f| f.font_id.size);
                let color = format.map_or(theme::TEXT, |f| f.color);
                hits.push((i, rect, field.clone(), text, size, color));
            }
        }
    });
    let family = theme::bold_family(ui.ctx());
    let mut seen: Vec<String> = Vec::new();
    for (i, rect, field, text, size, color) in hits {
        if seen.contains(&field) {
            continue;
        }
        seen.push(field.clone());
        let galley = ui.ctx().fonts_mut(|f| {
            f.layout_no_wrap(text.clone(), FontId::new(size, family.clone()), color)
        });
        ui.ctx().graphics_mut(|g| {
            g.entry(layer).mutate_shape(ShapeIdx(i), |cs| {
                if let Shape::Text(t) = &mut cs.shape {
                    t.galley = galley;
                }
            });
        });
        let bar = Rect::from_min_max(
            pos2(left - 7.0, rect.top() - 3.0),
            pos2(left - 5.0, rect.bottom() + 3.0),
        );
        ui.painter().rect_filled(bar, 0.0, theme::OVERRIDE);
        let hit = Rect::from_min_max(
            pos2(left, rect.top() - 3.0),
            pos2(value_x - 4.0, rect.bottom() + 3.0),
        );
        let resp = ui
            .interact(
                hit,
                Id::new(("prefab_property", component, &field)),
                Sense::click(),
            )
            .on_hover_text("Overridden in this instance (right-click to apply or revert)");
        resp.context_menu(|ui| property_menu(ui, state, &ctx, component, &field));
        STYLED.with(|s| s.borrow_mut().push(format!("{component}/{text}")));
    }
}

/// Right-click on an overridden property: Apply to the outermost prefab, or to a nested prefab
/// the object comes from; Revert.
fn property_menu(
    ui: &mut Ui,
    state: &mut EditorState,
    ctx: &Context,
    component: &str,
    field: &str,
) {
    let matches = |o: &OverrideItem| {
        o.entity == Some(ctx.target)
            && matches!(&o.key, Override::Property { component: c, property: p, .. }
                if c == component && p == field)
    };
    let Some(item) = ctx.items.iter().find(|o| matches(o)) else {
        return;
    };
    ui.label(RichText::new(format!("{component} ▸ {}", field_label(field))).weak());
    apply_buttons(ui, state, ctx, &[item.key.clone()], |o| matches(o));
    if ui.button("Revert").clicked() {
        state.push(Action::RevertPrefabOverrides {
            root: ctx.root,
            keys: vec![item.key.clone()],
        });
        ui.close();
    }
}

/// "Apply to Prefab 'Cart'" (the outermost prefab), then one for each nested prefab between the
/// target and it ("Apply to Prefab 'Wheel'"), with the same changes in that prefab's terms.
fn apply_buttons(
    ui: &mut Ui,
    state: &mut EditorState,
    ctx: &Context,
    keys: &[Override],
    same: impl Fn(&OverrideItem) -> bool,
) {
    let label = format!("Apply to Prefab '{}'", display_name(&ctx.asset));
    if ui
        .add_enabled(!read_only(&ctx.asset), egui::Button::new(label))
        .clicked()
    {
        state.push(Action::ApplyPrefabOverrides {
            root: ctx.root,
            keys: keys.to_vec(),
        });
        ui.close();
    }
    let inner: Vec<Entity> = state
        .scene
        .prefab_roots(ctx.target)
        .into_iter()
        .filter(|r| *r != ctx.root)
        .collect();
    for root in inner {
        let Some(path) = state.scene.prefab_link(root) else {
            continue;
        };
        let Some(items) = crate::prefabs::overrides_of(state, root) else {
            continue;
        };
        let keys: Vec<Override> = items
            .into_iter()
            .filter(|o| same(o))
            .map(|o| o.key)
            .collect();
        if keys.is_empty() {
            continue;
        }
        let label = format!("Apply to Prefab '{}'", display_name(&path));
        if ui
            .add_enabled(!read_only(&path), egui::Button::new(label))
            .clicked()
        {
            state.push(Action::ApplyPrefabOverrides { root, keys });
            ui.close();
        }
    }
}

/// A component card header: blue bar for a component with overrides, "+" for one the instance
/// added. Call at the start of the header closure.
pub fn header_marker(ui: &mut Ui, component: &str) {
    let Some(ctx) = current() else {
        return;
    };
    let items = component_items(&ctx, component);
    if items.is_empty() {
        return;
    }
    let r = ui.max_rect();
    let bar = Rect::from_min_max(
        pos2(r.left() - 2.0, r.top() - 3.0),
        pos2(r.left(), r.bottom() + 3.0),
    );
    ui.painter().rect_filled(bar, 0.0, theme::OVERRIDE);
    let added = items
        .iter()
        .any(|o| matches!(o.key, Override::AddedComponent { .. }));
    if added {
        ui.label(RichText::new("+").strong().color(theme::OVERRIDE))
            .on_hover_text("Added in this instance (not in the prefab)");
    }
}

/// Apply / Revert entries for the component's changes, at the end of its ⋮ menu.
pub fn component_menu(ui: &mut Ui, state: &mut EditorState, component: &str) {
    let Some(ctx) = current() else {
        return;
    };
    let items = component_items(&ctx, component);
    if items.is_empty() {
        return;
    }
    let keys: Vec<Override> = items.iter().map(|o| o.key.clone()).collect();
    let added = items
        .iter()
        .any(|o| matches!(o.key, Override::AddedComponent { .. }));
    ui.separator();
    let target = ctx.target;
    apply_buttons(ui, state, &ctx, &keys, |o| {
        o.entity == Some(target) && o.key.component() == Some(component)
    });
    let revert = if added {
        "Revert (Remove Component)"
    } else {
        "Revert Component Overrides"
    };
    if ui.button(revert).clicked() {
        state.push(Action::RevertPrefabOverrides {
            root: ctx.root,
            keys,
        });
        ui.close();
    }
}

/// Components the instance removed from the target, after the others (Revert brings one back).
pub fn removed_components(ui: &mut Ui, state: &mut EditorState) {
    let Some(ctx) = current() else {
        return;
    };
    let removed: Vec<OverrideItem> = ctx
        .items
        .iter()
        .filter(|o| o.entity == Some(ctx.target))
        .filter(|o| matches!(o.key, Override::RemovedComponent { .. }))
        .cloned()
        .collect();
    for item in removed {
        let name = item.key.component().unwrap_or_default().to_owned();
        egui::Frame::new()
            .fill(theme::CARD_BG)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let r = ui.max_rect();
                    let bar = Rect::from_min_max(
                        pos2(r.left() - 8.0, r.top() - 4.0),
                        pos2(r.left() - 6.0, r.bottom() + 4.0),
                    );
                    ui.painter().rect_filled(bar, 0.0, theme::OVERRIDE);
                    ui.label(
                        RichText::new(format!("{name} (Removed)"))
                            .strikethrough()
                            .color(theme::TEXT_DIM),
                    )
                    .on_hover_text("This instance removed the prefab's component");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let keys = vec![item.key.clone()];
                        let apply = ui.add_enabled(
                            !read_only(&ctx.asset),
                            egui::Button::new("Apply").small(),
                        );
                        if apply
                            .on_hover_text(format!("Remove it from {}", display_name(&ctx.asset)))
                            .clicked()
                        {
                            state.push(Action::ApplyPrefabOverrides {
                                root: ctx.root,
                                keys: keys.clone(),
                            });
                        }
                        if ui.small_button("Revert").clicked() {
                            state.push(Action::RevertPrefabOverrides {
                                root: ctx.root,
                                keys,
                            });
                        }
                    });
                });
            });
        ui.add_space(5.0);
    }
}

/// The Prefab row under the header of an instance root: Open, Select, Overrides ▾ (outermost
/// instances), or the red Missing Prefab state with Unpack.
pub fn prefab_row(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let Some(rel) = state.scene.prefab_link(e) else {
        return;
    };
    let name = display_name(&rel);
    let missing = state.assets.prefab(&rel).is_err();
    let outermost = state.scene.outermost_prefab_root(e) == Some(e);
    let variant = state.assets.prefab(&rel).is_ok_and(|p| p.base().is_some());
    let model = read_only(&rel);
    let kind = match (missing, model, variant) {
        (true, _, _) => "Missing Prefab",
        (_, true, _) => "Model Prefab",
        (_, _, true) => "Prefab Variant",
        _ => "Prefab",
    };
    ui.horizontal(|ui| {
        let icon = crate::state::PREFAB_ICON;
        if missing {
            ui.label(
                RichText::new(format!("{icon} Missing Prefab"))
                    .strong()
                    .color(theme::MISSING_PREFAB),
            )
            .on_hover_text(format!("{rel} can't be found"));
            ui.label(RichText::new(name).color(theme::MISSING_PREFAB));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Unpack")
                    .on_hover_text("Make it plain objects")
                    .clicked()
                {
                    state.push(Action::UnpackPrefab(e));
                }
            });
            return;
        }
        ui.label(kind);
        ui.label(RichText::new(format!("{icon} {name}")).color(theme::PREFAB))
            .on_hover_text(&rel);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if outermost {
                overrides_dropdown(ui, state, e, &rel);
            }
            if ui
                .button("Select")
                .on_hover_text("Show the prefab asset in the Project window")
                .clicked()
            {
                state.push(Action::SelectPrefabAsset(e));
            }
            let open = ui
                .add_enabled(!model, egui::Button::new("Open"))
                .on_hover_text("Edit the prefab in Prefab Mode")
                .on_disabled_hover_text("Model files are read-only: make a Prefab Variant");
            if open.clicked() {
                state.push(Action::OpenPrefab {
                    path: rel.clone(),
                    from: Some(e),
                });
            }
        });
    });
}

/// Where an override sits: the object's name and a short description.
pub fn describe(state: &mut EditorState, rel: &str, o: &OverrideItem) -> (String, String) {
    let object = o.entity.filter(|e| state.scene.contains(*e));
    let name = |state: &EditorState| object.map(|e| state.scene.name(e)).unwrap_or_default();
    match &o.key {
        Override::Property {
            component,
            property,
            ..
        } => (
            name(state),
            format!("{component} ▸ {}", field_label(property)),
        ),
        Override::AddedComponent { component, .. } => {
            (name(state), format!("{component} (added component)"))
        }
        Override::RemovedComponent { component, .. } => {
            (name(state), format!("{component} (removed component)"))
        }
        Override::AddedObject { name: n, .. } => (n.clone(), "Added object".to_owned()),
        Override::RemovedObject { object } => {
            let n = state
                .assets
                .prefab(rel)
                .ok()
                .and_then(|p| p.object_name(*object))
                .unwrap_or_else(|| "Object".to_owned());
            (n, "Removed object".to_owned())
        }
    }
}

/// The Overrides ▾ dropdown of an outermost instance root.
fn overrides_dropdown(ui: &mut Ui, state: &mut EditorState, root: Entity, rel: &str) {
    let items = crate::prefabs::overrides_of(state, root).unwrap_or_default();
    let count = items.len();
    let title = if count == 0 {
        "Overrides ▾".to_owned()
    } else {
        format!("Overrides ({count}) ▾")
    };
    let editable = !read_only(rel);
    let menu = ui.menu_button(title, |ui| {
        ui.set_min_width(300.0);
        ui.label(theme::bold(
            ui.ctx(),
            format!("Overrides to {}", display_name(rel)),
        ));
        if items.is_empty() {
            ui.label(RichText::new("No overrides").color(theme::TEXT_DIM));
        }
        let mut last_object: Option<String> = None;
        for o in &items {
            let (object, what) = describe(state, rel, o);
            if last_object.as_ref() != Some(&object) {
                ui.add_space(2.0);
                ui.label(RichText::new(&object).color(theme::PREFAB).strong());
                last_object = Some(object.clone());
            }
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                let bar = ui.available_rect_before_wrap();
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        pos2(bar.left() - 6.0, bar.top() + 2.0),
                        pos2(bar.left() - 4.0, bar.top() + 16.0),
                    ),
                    0.0,
                    theme::OVERRIDE,
                );
                ui.label(&what);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let keys = vec![o.key.clone()];
                    if ui
                        .add_enabled(editable, egui::Button::new("Apply").small())
                        .on_hover_text(format!("Write this change to {}", display_name(rel)))
                        .clicked()
                    {
                        state.push(Action::ApplyPrefabOverrides {
                            root,
                            keys: keys.clone(),
                        });
                    }
                    if ui
                        .small_button("Revert")
                        .on_hover_text("Go back to the prefab's value")
                        .clicked()
                    {
                        state.push(Action::RevertPrefabOverrides { root, keys });
                    }
                });
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            let any = count > 0;
            if ui
                .add_enabled(any, egui::Button::new("Revert All"))
                .on_hover_text("Discard every change of this instance")
                .clicked()
            {
                state.push(Action::RevertPrefab(root));
                ui.close();
            }
            if ui
                .add_enabled(any && editable, egui::Button::new("Apply All"))
                .on_hover_text("Write every change of this instance to the prefab")
                .on_disabled_hover_text("Model files are read-only")
                .clicked()
            {
                state.push(Action::ApplyPrefab(root));
                ui.close();
            }
        });
    });
    if OPEN_DROPDOWN.with(|o| o.replace(false)) {
        egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&menu.response));
    }
}

/// `MISTY_PREFAB_DEMO`: overrides on the selected instance, for screenshots (memory only).
pub fn demo(state: &mut EditorState, what: &str) {
    if what == "missing" {
        let e = state.scene.spawn("Old Crate", None);
        state
            .scene
            .link_prefab(e, "Assets/Prefabs/Old Crate.prefab");
        state.scene.spawn("Lid", Some(e));
        return;
    }
    let Some(root) = state
        .selection
        .active()
        .filter(|e| state.scene.prefab_link(*e).is_some())
    else {
        return;
    };
    state.scene.add(root, misty_engine::scene::Light::default());
    let added = state.scene.spawn("Lantern", Some(root));
    let _ = added;
    let child = state.scene.children(root).into_iter().next();
    if let Some(c) = child {
        let mut t = state.scene.transform(c);
        t.scale *= 1.5;
        state.scene.set_transform(c, t);
    }
    state.hierarchy.expanded.insert(root);
    match what {
        "bold" => {
            if let Some(c) = child {
                state.select(c);
            }
        }
        _ => OPEN_DROPDOWN.with(|o| o.set(true)),
    }
}
